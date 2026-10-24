# Photon AOT 运行时 ARC 实施报告（P2：发射器注入）

> 2026-10-20 / R149。承接 `photon_aot_arc_p0_p1_2026-10-20.md`（P0/P1 已实施）。
> 目标读者：编译器/运行时维护者。
> 结论摘要：**P2 的发射器机制已全部实现并通过编译/冒烟验证，但「注入式
> release」在本代码库当前的所有权模型下不可靠** —— 逐条开启释放路径都会
> 产生「释放仍被引用的对象」，实测 3 类崩溃/损坏。因此 P2 落地形态为
> **默认关闭 + 显式实验开关**，并把 P3 的入口条件（逃逸分析/所有权标注）
> 用可复现的证据固定下来。

---

## 一、P2 交付物（代码）

| 文件 | 内容 |
|---|---|
| `seed/compiler/src/codegen/aot/emit.rs` | D4 发射层：`ArcLocal` 局部量表与作用域栈、`arc_retain`/`arc_release`、作用域退出与 `return` 路径释放、`is_copy_expr` 复制类判定（含分支/块递归）、闭包 env 计数、`AURA_ARC*` 开关 |
| `seed/compiler/src/codegen/aot/runtime.rs` | `aura_arc_retain`/`aura_arc_release` 运行时声明 |
| `seed/compiler/src/codegen/aot/linker.rs` | `AURA_LINK_MAP=1` → 链接期输出 `.map`（失衡点地址反查符号，诊断设施） |
| `seed/compiler/src/std/cffi/aura_std_cffi.c` | 32 位头标签（`flags` 低 2 位：list/map/env）、`aura_arc_alloc_env`、析构级联（默认关）、`AURA_ARC_DEBUG` 失衡点/调用栈/释放日志/AV 捕获 |
| `seed/compiler/src/std/cffi/aura_std_cffi.h` | 新增入口声明 |

### 开关矩阵（全部为**发射期**环境变量）

| 变量 | 默认 | 作用 |
|---|---|---|
| `AURA_ARC` | **关** | 总开关。开 = 发射 retain/release 调用点 |
| `AURA_ARC_RELEASE` | **关** | **release 总闸**。关 = 只 retain（只增不减，内存安全） |
| `AURA_ARC_SCOPE` | 开 | 作用域退出释放（需 `AURA_ARC_RELEASE=1`） |
| `AURA_ARC_RETURN` | 开 | `return` 路径释放（需 `AURA_ARC_RELEASE=1`） |
| `AURA_ARC_RETAIN` | 开 | 全部 retain（诊断用，可关） |
| `AURA_ARC_DESTRUCT` | **关**（运行期） | 容器/闭包析构级联（release 元素） |
| `AURA_ARC_DEBUG` | 关（运行期） | 过量释放检测 + 调用栈 + 释放日志 + AV 捕获 |
| `AURA_LINK_MAP` | 关（链接期） | 输出 `<exe>.map` |

设计原则：**默认路径零影响**（`AURA_ARC` 未设 ⇒ 发射器不插任何指令，
产物逐字节不变）；**危险的释放路径必须显式开启**（不会静默产生内存错误）。

---

## 二、实现了什么（D4 规则表）

1. **槽位存储**：`emit_assign` 对已存在槽位「先算新值 → release 旧值 →
   retain 新值（复制类）→ 存回」。
2. **新建槽**：`emit_variable_decl` 复制类初值 retain（否则视为所有权转移）。
3. **复制类判定 `is_copy_expr`**：`Var`（非单例/枚举名）、字段读、下标读、
   返回别名的 C 调用（`getAt`/`mapGet`/`listAppend`/`mapSet`…），并
   **递归穿透 `if`/块/`await`**（任一分支借用即 retain）。
4. **作用域退出**：逆序 release 本作用域登记的 ARC 局部量；已终止块
   （return/break/continue 路径）跳过（泄漏方向安全）。
5. **`return`**：复制类返回值先 retain，再逆序 release 全部打开作用域。
6. **形参/实参**：改用**借用语义**（被调方不 release 形参、调用点不 retain）。
7. **闭包**：env 用 `aura_arc_alloc_env` 分配并打 `AURA_MEM_TAG_ENV`
   标签，捕获项在 env 构造时 retain。
8. **字段写**：只 retain 新值（不 release 旧值）。
9. **C 侧**：`flags` 低 2 位作块类型标签（`magic + 高位为 0` 双重断言把
   i8* 上的假阳性压到 ~2^-30）；`rc` 归零时可选析构级联。

---

## 三、为什么 release 默认关闭（实测证据）

> 全部实验用 `AURA_ARC=1` 构建驱动 `PhotonHatCompile-arc.exe`，跑
> `tests/photon/P1/01_hello_world.aura`（即用自举编译器编译一个样例）。
> 判定口径：`===RESULT===success` + 无 AV + 产物 `.hat` 与基线一致。

### 3.1 二分结果

| 配置 | 结果 | 现象 |
|---|---|---|
| 全部规则（含字段写 release、析构级联） | ✗ | AV `0xC0000005`，6.4s |
| 仅 `return` 路径释放（`AURA_ARC_SCOPE=0`） | ✗ | AV `0xC0000005`，6.4s |
| 仅作用域退出释放（`AURA_ARC_RETURN=0`） | ✗ | `lld-link: undefined symbol: H纯R`（**符号名被改写**） |
| 释放全关（`SCOPE=0;RETURN=0`） | ✓ | `success`，exit 0，`.hat` 与基线同 md5 |
| 释放全关 + 补字段写/包装体 retain | ✓ | 同上 |
| 全部规则 + 仅关析构级联 | ✗ | AV，11s（推迟但仍在） |
| 全部规则 + 字段写 retain + 包装体 retain（无析构） | ✗ | AV，11.9s |

关键观察：**ARC 开启前后 `.hat` 始终与基线逐字节一致**
（`b8c8fd13…`，472,612B）。也就是说前端/中端结果是正确的，损坏发生在
「用这些对象继续做后端」的阶段 —— 即**对象被提前释放后仍在使用**。

### 3.2 三类根因（均已定位到具体函数）

**(1) 调用点/被调方所有权不配对（已修，但暴露了架构性问题）**

IR 证据（`FileUtils.mkdirP`）：`Stdio.stringToBuffer(path)` 调用前**没有**
`aura_arc_retain`，而被调方在出口 release 形参：

```
%var.5608 = load i8*, i8** %var.5605      ; path
%var.5609 = call i64 @aura_user_Stdio_stringToBuffer(i8* %var.5607, i8* %var.5608)
```

调用栈（`AURA_ARC_DEBUG` + `.map` 反查）：
`main` → `FileUtils_mkdirP+0xce`（release） ← 之前 `Stdio_stringToBuffer+0x106`
已把同一指针释放（`prior free: back = 0 releases ago`）。

原因：`emit_call` 内部有**数十条**特化/改派早返回路径（内建特化、方法
改写、幽灵首参剔除、构造器直调），任何一条都绕过尾部的实参配平。**结论**：
跨路径的「调用者 retain / 被调方 release」约定不可维护 → 改为借用语义。

**(2) 分支表达式的 phi 值是借用值（已修）**

`Lowering.lowerCallValue` 的
`val op = if (…) { srcVal.op } else { "" }`：旧 `is_copy_expr` 只看顶层
（`If` 落到 `_ => false`），不 retain；作用域退出 release 时把
`MirValue.op`（长寿命字段）踩掉，**每次调用少一次引用**，第二次调用即 UAF。
修正：`is_copy_expr` 递归 `If`/`Block`/`Await`（任一分支借用即 retain）。

**(3) 逃逸引用未计数（未闭合 —— P3 的主战场）**

即使补上 (1)(2) 与字段写 retain、`{i8*, i64}` 包装体 retain，仍在 11.9s
产生 AV（`addr=0x2460039313D`，把字符串字节当地址读）。剩余未计数别名：

| 别名来源 | 说明 |
|---|---|
| 结构体**值**副本 | `%struct.X` 按值 load/store/insertvalue 复制，字段指针被多副本共享，无引用计数可依据 |
| 静态注册表 | `aura_lists[32]` 等 C 侧静态数组持有指针，非堆对象，不参与 rc |
| `Any` 包装 | `{ i8*, i64 }` 字符串结构体、装箱整数在 `i8*` 上混用（Plan A） |
| 模块级全局 | `@name = global i64`（顶层 `val`/`var`）跨函数长寿命持有 |
| `Enum`/`Option` 载荷 | `insertvalue { i8*, i64 }` 构造，载荷生命周期与枚举值耦合 |

只要有一处别名未计数，release 就会把对象交还给堆，而另一处仍在读它。

**(4) 析构级联额外放大风险**

容器析构会 release 元素，前提是「元素的**所有**别名都已计数」。当前
不成立 ⇒ 级联默认关闭（`AURA_ARC_DESTRUCT=1` 才开）。关闭后：容器自身
块与 `items` 数组仍会释放，元素按引用泄漏（与 P1 前基线同量级）。

---

## 四、结论与 P3 入口条件

1. **P2 机制可用**：发射层、C 侧标签/析构/诊断设施齐全，开关矩阵可逐条
   验证；`AURA_ARC` 默认关 ⇒ 生产路径零影响。
2. **注入式 release 需要「逃逸/所有权」判定**，仅靠语法形态（`is_copy_expr`）
   不够 —— 本报告的 3 类根因就是判定缺失的三种表现。
3. P3 可选路线（按性价比排序）：
   - **阶段 region 回收**（最契合本编译器）：前端/每模块后端各开一个
     arena，整块 `RegionReset`，天然规避别名问题；P0 已有 D1 时代闸门
     与 D2c 区间防线可直接复用。代价：峰值内存换吞吐。
   - **局部逃逸分析**：对 `String`/容器局部量做「块内独占且不逃逸」判定
     （`is_copy_expr` 的加强版：需要 use-def + 字段/容器写点集合），
     只对独占量插 release。工作量集中在 `emit.rs` 的 use 收集。
   - **所有权标注**：`owned`/`borrow` 语法，改动面最大但语义最清晰。
4. 诊断设施保留（默认关，零成本）：`AURA_ARC_DEBUG`（失衡点 + 调用栈 +
   释放日志 + AV 捕获）、`AURA_LINK_MAP`（地址反查），后续任何内存问题
   都可 5 分钟内定位到函数。

---

## 五、验收

| 项 | 结果 |
|---|---|
| 默认路径（`AURA_ARC` 未设）自举 | 见 §5.1 |
| `.hat`/`.obj` 与 R147 基线 | 见 §5.1（逐字节） |
| `AURA_ARC=1`（仅 retain，无 release）自举 | 见 §5.2 |
| 冒烟（VM vs 产物 exe） | 10/10 |
| 释放路径（`AURA_ARC_RELEASE=1`） | 已知不安全，默认关闭（§三） |

### 5.1 默认路径（`AURA_ARC` 未设 —— 回归证明）

`build/hat-bootstrap-r149/`：

| 项 | 结果 |
|---|---|
| 驱动自举 | `===RESULT===success`，exit 0 |
| `aura-compiler.hat` | md5 `263a3bbde2064968e8873ac40c000114` = R147 基线 **逐字节一致** |
| `aura-compiler.obj` | md5 `54b33c02fc81f96a04880b02e7b99f22` = R147 基线 **逐字节一致** |
| 产物 `aura-compiler.exe` 无参自检 | `BOOT-4` ✓ |

⇒ P2 的全部发射层改动在默认路径上**零影响**（`AURA_ARC` 未设时发射器
不插任何指令，`arc_managed`/`arc_emit_enabled` 全部短路）。

### 5.2 retain-only（`AURA_ARC=1`，release 总闸关闭）

IR 统计（驱动本体）：`aura_arc_retain` **1064** 处、`aura_arc_release`
**0** 处、`aura_arc_alloc_env` 1 处。

`build/hat-bootstrap-r149-arc/`：

| 项 | 结果 |
|---|---|
| 自举 | `===RESULT===success`，exit 0 |
| `aura-compiler.hat` | `263a3bbde2064968e8873ac40c000114` = 基线 **逐字节一致** |
| `aura-compiler.obj` | `54b33c02fc81f96a04880b02e7b99f22` = 基线 **逐字节一致** |
| 产物 `aura-compiler.exe` | 3,069,440 B（与默认路径同尺寸），无参自检 `BOOT-4` ✓ |
| 冒烟（VM vs 产物 exe，6 例） | **6/6 PASS** |
| 运行期 ARC 统计 | `retain=132,368,288`（含 C 侧 push）、`release=348`、`freed=0`、`bad_magic=0`、`noop(boxed=368M era=401M lit=85M)` |
| 存活堆 | 41.03M 块 / 3887 MB（默认路径 41.28M / 4334 MB，同量级） |

⇒ retain 路径（发射器 + C 侧容器）**完全安全**：只增不减，无假阳性
（`bad_magic=0`），且不改变任何编译产物字节。

---

## 六、数据一览（一次跑通的三档配置）

| 配置 | 结果 | `.hat`/`.obj` | 说明 |
|---|---|---|---|
| 默认（`AURA_ARC` 未设） | ✓ success + BOOT-4 | 与 R147 逐字节一致 | 生产路径，零改动 |
| `AURA_ARC=1`（retain-only） | ✓ success + BOOT-4 | 与 R147 逐字节一致 | 计数安全，无释放 |
| `AURA_ARC=1 AURA_ARC_RELEASE=1` | ✗ AV / 符号名损坏 | `.hat` 仍一致 | 释放路径实验，默认关 |
| 上加 `AURA_ARC_DESTRUCT=1` | ✗ 更快暴露 | — | 析构级联依赖完整计数，默认关 |

自举耗时观察：冷启动一轮约 11–13 分钟（前端 117 个模块 + 后端 1992 函数），
三档差异在噪声内（`RELCHK-FINAL funcs=1992 strConsts=16553 rdataSize=187080`
三档完全一致）。

## 七、回滚

- 默认路径不涉及：`git checkout` 五个文件后重建驱动即可完全回到 R148。
- 若要保留诊断设施但禁用机制：不设任何 `AURA_ARC*` 环境变量即可。
