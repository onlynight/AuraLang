# Photon Aura VM vs Rust VM：运行时内存对比与优化方案

> 日期：2026-10-07 · 任务：**只做分析，不改代码**（本报告不含任何源码改动）
> 测量工具：`tools/photon-prof/memprof.py`（5ms 高频采样）+ `AURA_VM_LOADONLY` 装载期分离
> 被测对象（沿用 benchmark 的两条执行链）：
>   ② **Rust VM**：`rust/target/release/aura.exe run <src>`（Rust 解释器，含编译）
>   ④ **Photon Aura VM**：`build/hat-vmhost/current.exe` + `build/vm/current.auc`（Photon 编译的 Aura VM）

---

## TL;DR

| 维度 | ② Rust VM | ④ Photon Aura VM |
|---|---|---|
| **固定私有提交** | ~10 MB（exe 8 MB + 堆） | **66.6 MB**（= 64 MiB 静态堆 BSS，恒定不变） |
| **基础工作集** | 17.9 MB | 8.7 MB（BSS 页惰性触碰） |
| **执行期内存增长** | ≈ 0（同负载下 +0~1.2 MB） | **线性增长**：`Long` 操作 ≈62.5 B/次；函数调用 ≈35.6 B/次 |
| **回收能力** | ARC + 空闲链表（复用） | **只增不减**（bump arena，临时装箱不回收） |
| **上限** | 无（Vec 动态增长） | **64 MiB 硬上限 → 超出即 SIGSEGV** |

**最重要的一条**：同一份"Long 累加 100 万次"的负载——

- Rust VM：17.9 MB（**增长 0**）
- Photon Aura VM：峰值 **71.2 MB**，其中 62.5 MB 是执行期新分配（**每次 `as Long` ≈62.5 字节，从不回收**）

并且**再放大 10% 就到头**：`loop 1.1M` 在 Photon Aura VM 上**段错误**。

---

## 一、测量方法与口径

三个口径（Windows / psutil）：

| 口径 | 含义 | 用途 |
|---|---|---|
| `peak_wset` | 峰值工作集（物理驻留） | "任务管理器"视角的实际内存占用 |
| `peak_private` | 峰值私有提交（PagefileUsage） | 占用的 commit charge（多进程/内存敏感场景） |
| `peak_vms` | 峰值虚拟地址空间 | 含预留，不代表实际占用 |

**关键实验装置**：

1. `memprof.py`：启动目标进程、5 ms 间隔采样、记录各口径峰值（多次运行取峰）。
2. `AURA_VM_LOADONLY=1`（VM 内建开关）：装载 `.auc` 后**立即停止** ⇒ 精确分离"装载期内存"与"执行期内存"。
3. **分离实验**：成对构造同构 guest（只差一个类型/操作），逐变量归因。

⚠ **口径警告**：② Rust VM 的 `aura run <src>` 是"编译 + 执行"单进程，其峰值含编译期内存（实测编译单独占 15.6 MB/7.7 MB private）；④ 只含装载 + 执行。对比执行期内存时应分别看（见附表）。

---

## 二、主数据

### 2.1 Photon Aura VM（④）

| 场景 | 动态指令量 | peak_wset | peak_private | 增量 vs 装载基线 |
|---|---|---|---|---|
| LOADONLY（仅装载 loop1m.auc） | 0 | 8.4 MB | 66.6 MB | — （基线） |
| micro1（最简 guest） | ~0 | 8.7 MB | 66.6 MB | +0.3 |
| smallstep（Int 循环 1.5M 次） | ~13.5 M | 8.7 MB | 66.6 MB | **+0** |
| intloop（Int 循环 1M 次，含乘/减） | ~15 M | 8.7 MB | 66.6 MB | **+0** |
| longstep（`x = i as Long` × 1M） | ~11 M | **71.2 MB** | 66.6 MB | **+62.5** |
| loop1m（`acc = acc + (i*2-1) as Long` ×1M） | 53 M | **71.2 MB** | 66.6 MB | **+62.5** |
| fib28（Int 递归，1.03 M 次调用） | 31.4 M | **45.3 MB** | 66.6 MB | **+36.6** |
| loop1.1M（同 loop1m，N=1.1M） | 58 M | — | — | **段错误 (SIGSEGV)** |
| loop2M（N=2M） | 106 M | — | — | **段错误 (SIGSEGV)** |

### 2.2 Rust VM（②，含编译）

| 场景 | peak_wset | peak_private |
|---|---|---|
| micro1 | 17.9 MB | 10.1 MB |
| fib28 | 19.1 MB | 11.3 MB |
| loop1m | 17.9 MB | 10.1 MB |
| `build fib28`（纯编译，不执行） | 15.6 MB | 7.7 MB |
| `build loop1m`（纯编译，不执行） | 10.9 MB | 2.9 MB |

⇒ Rust VM 的 53 M 指令 Long 负载（loop1m）与最简 guest（micro1）**内存完全相同（17.9 MB）**——执行期零增长。

---

## 三、发现 1：Photon 的 66.6 MB 私有提交 = 64 MiB 静态堆（BSS）

**段表证据**（`llvm-objdump -h`）：

| 段 | Photon 宿主 | Rust VM |
|---|---|---|
| `.text`（代码） | **384 KB** | 4.95 MB |
| `.rdata` | 11 KB | 2.72 MB |
| `.data` | **64 MiB**（0x04002218） | 2 KB |
| exe 文件 | 67.5 MB | 7.9 MB |

**"67.5 MB 的宿主"里 64 MB 是静态堆**：`PhotonRuntime.aura:3484` 的
`writer.dataSymbolName = "toStrBuffer:32,heapArena:67108864,heapBump:8,..."`
—— `heapArena` 是一块 **64 MiB 的 `.data`（非 `.bss`）**，AOT 生成的代码以
`mov rax, [rel heapBump]; lea rdx, [rel heapArena]` 直接 RIP 寻址（免间接层）。

**三个后果**：

1. **私有提交恒定 66.6 MB**（任何 guest、包括空转）——BSS 全部被 commit；
2. **exe 文件 64 MB**（.data 的 raw 数据随文件分发）；
3. 工作集却只有 8.4 MB —— 未被触碰的 BSS 页不进物理内存（**唯一的好消息**）。

---

## 四、发现 2（核心）：`Long` 操作每次分配 ≈62.5 字节，且**从不回收**

### 4.1 分离实验（逐变量归因）

| guest（1M 次迭代） | 循环体 | wset 增量 |
|---|---|---|
| intloop | `acc = i * 2 - 1`（Int） | **+0** |
| longstep | `x = i as Long`（Long） | **+62.5 MB** |
| loop1m | `acc = acc + (i*2-1) as Long` | **+62.5 MB** |

⇒ 去掉 `as Long` 即零增长；加上 `as Long` 即线性增长。
**loop1m 的全部 62.5 MB 由 `as Long` 一项解释；`Long` 赋值/读取本身不额外收费**
（longstep 只做 1 次转换+1 次 store，与 loop1m 的"转换+加法+store"同价）。

### 4.2 源码链路（只读定位，未改动）

```
as Long（字节码：CAST / Builtin.toLong）
   └─ VM dispatch → op==117 boxAlloc()            [Vm.aura:117 臂]
        ├─ val ptr = Memory.alloc(24)               ← 24 B box 本体
        ├─ Memory.write64(ptr+16, toI64(toStrAny(v)))  ← 值写入
        └─ this.heapKinds.put(toStrAny(ptr), "box")  ← 注册表：key = 指针的字符串形式！
```

- `toStrAny(ptr)` **每次生成一个指针字符串**（如 `"140737000000000"`）；
- `heapKinds`（`HashMap<String,String>`，`Vm.aura:187`）是**全局堆分配注册表**；
- 唯一的回收路径是 `freeRegistered()`（`Vm.aura:2462`），**只在显式 ARC 释放指令
  （DEC_REF/RELEASE 类）被执行时触发** —— 而 VM 实测表现为**临时装箱从不触发**；
- 分摊成本 = 24 B box + 指针字符串 + HashMap 节点 ≈ **62.5 B/次** ✓（与实测吻合）。

### 4.3 与 Rust VM 的对照

Rust VM 的 `Value` 是 Rust enum（数值内联，无堆）——同一 Long 负载**零增长**。
⇒ 差距不在"要不要分配"，而在**Photon 的 Plan A 值模型必须给 64 位 Long 找一个堆落点，
且这条路径没有回收**。

---

## 五、发现 3：函数调用每次 ≈35.6 字节（部分归因）

- fib28：1,028,457 次调用 → **+36.6 MB** ⇒ 35.6 B/调用。
- 已确认 `callOrBuiltin` 前 150 行无直接 `Memory.alloc`；**逐行归因未完成**。
- 两个候选路径（下一步验证）：
  1. `incRef` 的慢路径：`val key = VmOps.toStrAny(v)`（非 tagged 值每次分配 key 字符串）；
  2. `argStrOf(k)` 参数表 / 调用窗的字符串化。
- 验证方法：在 `incRef`/`argStrOf` 内加瞬时计数探针（**需改代码，等批准**）。

---

## 六、发现 4：64 MiB 上限 → **段错误**（不是报错）

| N | 预期 arena 用量 | 结果 |
|---|---|---|
| 1,000,000（loop1m） | ≈62.5 MB | ✅ 正常输出 |
| 1,100,000（loop1.1M） | ≈68.8 MB | ❌ **SIGSEGV（rc=139）** |
| 2,000,000（loop2M） | ≈125 MB | ❌ **SIGSEGV** |
| 1,500,000（smallstep，仅 4.5M 指令） | ≈5 MB | ✅ 正常（对照，排除"大常量编码"假设） |

**结论**：阈值精确落在 64 MiB 静止 arena 的容量边界上（62.5 MB OK / 68.8 MB 崩）。
**超限行为是段错误**——bump 越界后继续写（或返回 0 指针被解引用），
没有"分配失败 → 明确报错"的护栏。对一个"要把 VM 做成可信运行时"的项目，
这是**可用性级缺陷**：任何跑得久一点的 Long 密集程序都会在约 100 万次操作处静默崩溃。

---

## 七、优化方案（按优先级；均**未实施**，供决策）

### P0-1 `Long` 装箱的回收（收益最大，直击 64 MB 上限）

| 方案 | 做法 | 预期收益 | 成本/风险 |
|---|---|---|---|
| **A. 循环回边水位重置** | 在基本块回边（循环头）处记录 arena 水位，回边时对"本迭代内分配且未逃逸的 box"重置 bump 指针 | loop1m 峰值 71.2 → ~10 MB；N 上限提升到"单迭代分配量"级别 | 需要保守逃逸判定（值未被 store 进跨迭代存活的局部/容器才可回收）；误判 → 提前释放 |
| **B. 同槽覆盖回收** | `STORE_LOCAL` 覆盖旧值时，若旧值是本 VM 注册的 box，且**该槽是唯一引用**，直接 `freeRegistered`（复用现成的释放路径） | 每次 Long 赋值回收上一代 box ⇒ 净分配 ≈ 常数 | 需保证 box 不被别名共享（若同一 box 被复制到两槽，回收其一即悬垂）→ 需配合"box 不共享"约束或最小引用计数 |
| **C. box 自由链表** | 给"box"单开 freelist（`freeRegistered` 时把 24B 块还池；`boxAlloc` 先取池） | 分配速率不变但 arena 用量恒定 | 纯分配器改造，语义风险最低；需确认 `freeRegistered` 的调用频率（当前疑似几乎不被 ARC 触发） |
| **D. 值模型特化（根治）** | SSA 层对 Long 局部变量做栈上 64 位直存，免装箱 | 彻底消除 | 影响面最大（SSA/发射器/栈布局），适合作为中期路线 |

> **推荐组合**：先 C（低风险，立即把"无限增长"变成"常数"），再评估 A/B。
> 三者都以"减少 arena 净增长"为目标，可独立验证：改后 `longstep` 的 wset 增量应 ≈0。

### P0-2 上限健壮性（独立于优化，先兜底）

- 分配失败（bump 越界）**返回明确错误**并打印（如 `[OOM] heapArena exhausted at +N bytes`），
  退出码非 0 —— 而非现在的段错误；
- 中期：`AURA_MEM_LIMIT_MB` 同款的 `AURA_HEAP_MB` 允许调大 arena（当前 64 MB 硬编码在 `dataSymbolName`）。

### P1 消除 66.6 MB 固定提交

- 现状：`heapArena:67108864` 落在 **`.data`** ⇒ 文件 64 MB + 加载全 commit。
- 方案 a（最低成本）：**挪进 `.bss`**（未初始化段，raw size=0）——文件 67.5 → ~3 MB；
  提交是否降为按需取决于 PE 加载行为，需实测（值得做一个 30 分钟实验）。
- 方案 b（彻底）：arena 改为 `VirtualAlloc(MEM_RESERVE 64MB)` + 按需 `MEM_COMMIT`，
  代码侧把 `[rel heapArena]` 改成"基址变量"（发射器小改，一処）；
- 方案 c：分块 arena（chunk 链，每块 1–2 MB 惰性提交）。
- 收益场景：内存敏感部署、多实例并行（每进程省 ~60 MB commit）。

### P1 调用路径的 35.6 B/次（先归因再定方案）

- 先做 §五的探针实验定位（改代码小、一次性）；
- 若确认是 `incRef` 的 `toStrAny` key 分配：把 key 改为**数值键**（指针直接作 `Long` 键或
  两级结构），免字符串化 —— 同时省下 boxAlloc 里的同类 key（**预计一并消掉 62.5 B 中的
  一大部分**，与 P0-1 相互放大）。
- 若确认是参数表字符串化：`argStrOf` 预解析数值槽已有（`arg2Ints` 等），把更多槽纳入。

### P2 结构建议

- **给 arena 加"使用率"自检**：装载后/退出前打印 `heapBump - heapArena`（VM 已有
  `heapBump` 符号与 `aura_mem_*` 同款诊断传统，接一个 3 行探针即可），
  让"还剩多少"成为随时可见的数字；
- 在 CI/五门中加一条 **Long 密集压力门**（如 N=1.1M 的 loop）——当前它会红，
  修复后转绿，防止回归。

---

## 八、附录：复现命令

```bash
# 单场景采样（3 轮取峰值）
C:/Users/zhang/.workbuddy/binaries/python/envs/default/Scripts/python.exe \
  tools/photon-prof/memprof.py --runs 3 --label <标签> -- <命令...>

# 全矩阵（micro1/fib28/loop1m × 两个 VM + 纯编译对照）
tools/photon-prof/mem-matrix.sh

# 装载期基线（只装载不执行）
AURA_VM_LOADONLY=1 <python> tools/photon-prof/memprof.py --runs 2 --label LOADONLY -- build/hat-vmhost/current.exe

# 分离实验 guest 位于 build/prof2/（intloop / longstep / smallstep / loop1100k ...）
# 由任意 .aura 生成 .auc：rust/target/release/aura.exe run <src> --vm=aura  （产物写死 build/vm/current.auc）
```

**已知局限**：

1. ② Rust VM 的 `aura run` 含编译期内存，与 ④ 的"装载+执行"口径不同（已单独给纯编译基线）；
2. fib28 的 35.6 B/调用未逐行归因（候选路径见 §五）；
3. 62.5 B/次 是**摊还值**（含 box + 注册表 + 字符串 key），未拆分为单项精确值。

---

## 九、P0-2b 追测：AURA_HEAP_MB 扩容生效后的内存矩阵（2026-10-08）

P0-2b（`AURA_HEAP_MB` 环境变量扩容，commit `9a32e7d`）落地后重跑本矩阵。
测量方法不变（`memprof.py` 3 轮取峰；环境变量经 Popen 继承传给宿主）。
场景已固化进 `tools/photon-prof/mem-matrix.sh` 末段。

### 9.1 数据

| 场景 | exit | wall_ms(最快) | peak_wset（物理） | peak_private（提交） |
|---|---|---|---|---|
| PhotonVM loop1m 默认（62.5MB 负载，刚好塞进默认 arena） | 0 | 5510 | 66.8 | 64.7 |
| PhotonVM loop1m mb128（不需要却扩容） | 0 | 5422 | 66.9 | **193.0** |
| PhotonVM loop1100k 默认（68MB > 64MiB） | **70 [OOM]** | 5655 | 67.5 | 64.7 |
| PhotonVM loop1100k mb128（最小够用） | 0 | 6031 | **101.9** | 193.0 |
| PhotonVM loop1100k mb256 | 0 | 5985 | **101.9** | 321.2 |
| PhotonVM loop1100k mb512 | 0 | 6018 | **101.9** | 577.7 |
| RustVM loop1m（原生堆参照） | 0 | 1288 | 14.8 | 9.4 |
| RustVM loop1100k（同负载，无上限） | 0 | 1390 | 14.7 | 9.3 |
| RustVM 纯编译 loop1100k（拆分口径） | 0 | 15 | 7.1 | 1.5 |

### 9.2 结论

1. **扩容语义 = 预提交（MEM_RESERVE|MEM_COMMIT）**：`peak_private` 恒等于
   `64.7 + N MB`（N = AURA_HEAP_MB）——扩容块在 Heap_init 一次性全额提交，
   与实际用量无关。193.0 / 321.2 / 577.7 三个数严格线性。
2. **物理内存只随触碰增长**：loop1100k 在 mb128/256/512 三档下 `peak_wset`
   全部是 101.9 MB——68MB 负载只触碰新块前 ~35MB，多预留的部分**零物理代价**
   （预提交未触碰页不驻留）。超配 512 跑 68MB 负载只浪费 commit charge，
   不浪费 RAM。
3. **扩容无运行时开销**：wall_ms 各档差异（5.4~6.0s）在噪声内；Heap_init 的
   一次 NtAllocateVirtualMemory 是微秒级一次性成本。
4. **OOM 守卫干净**：默认（或 mb=64 不足）时 loop1100k 走 P0-2a 的
   `[OOM] HEAP FULL + exit 70`，峰值停在 64.7MB 提交——**不再段错误**
   （修复前的行为是 SIGSEGV）。
5. **跨 VM 差距的本质**：同一 1.1M Long 负载，Rust VM 只用 9.3MB private——
   其原生分配器**逐次回收**被覆写的 Long 装箱；Photon bump arena 只增不减，
   62.5B/次 × 1.1M ≈ 68MB 全部滞留。AURA_HEAP_MB 是**绕开**（买更大的
   一次性 arena），不是**修复**（回收）；P1 方向仍是 §七的空闲链表/区间回收。

---

## 十、P0-2c：`as Long` 「从不回收」真根因与修复（2026-10-08）

§七 P0-1 的四个候选方案（水位重置/同槽回收/自由链表/值模型特化）**全部未采用**——
深挖后发现 §4.2 的「boxAlloc + heapKinds」链路是**陈旧描述**：当前 .auc 里 `as Long`
编译为 `CALL_NATIVE_ARGS("aura_cast", 2)`，真正的泄漏是**两处与装箱无关的解释器 bug**：

### 10.1 根因 1：`aura_cast` 臂对类型名做 `toStr(指针)`（≈32B/次）

```aura
val targetTy: String = toStr(this.stack.pop())   // 弹出的是 "Long" 字符串指针！
```

photon 运行时 `toStr` 是**纯整数**打印器（P347）：对字符串指针打出**指针值的十进制**
（≈14 位数字，66B 临时串、永不回收）。后果双重：

1. 泄漏：longstep 1M 次 `as Long` 实测 arena **+66.0MB**（§四的「62.5B/次」即此）；
2. 更严重——`targetTy` 是垃圾数字，`== "Long"` 等比较**全部失败**，恒走 else
   **透传**：`as Long` 从未真正转换过（Plan A 下 Int/Long 本就同为 64 位 tagged，
   透传碰巧语义正确，掩盖了 bug 本身）。

**修复**：类型名改 `VmOps.toStrAny`（真实字符串原样返回，零分配）；`as Long` 走
显式恒等 `push(cv)`（Plan A 无窄化/加宽，恒等即正确语义）。

### 10.2 根因 2：`RETAIN`/`RELEASE` 回压栈顶（值栈 +2 槽/迭代，无上界倍增）

发射端约定（seed interp.rs 2026-10-02 已修的同款）：`LoadVar src; RETAIN` ——
RETAIN **消费**栈顶。Photon VM 的 `retain()`/`release()` 却是 `incRef()`/`decRef()`——
**pop 后又 push 回去**，每对净增 1 个栈槽。紧密循环里每次调用后都插有 ARC 指令，
值栈按 ~16B/迭代**无上界倍增**：longstep 1M 次实测 VmStack cap 涨到
**2,097,088 槽**，各代旧缓冲（8B/槽）累计 ≈31MB 全部滞留 bump arena。
种子侧同一 bug 当时的症状是「8 秒涨到 7GB」。

**修复**：`retain()`/`release()` 改为消费语义（pop 后不回压；tagged 快路径直通；
对象/箱按 rc±1，归零走 `freeRegistered`）。

### 10.3 修复后内存矩阵（memprof 3 轮取峰）

| 场景 | 修复前 wset | 修复后 wset | 修复前行为 | 修复后行为 |
|---|---|---|---|---|
| longstep 1M | 71.2 MB | **4.2 MB** | +62.5MB 线性增长 | 常数 |
| loop1m | 71.2 MB | **4.3 MB** | +62.5MB | 常数 |
| loop1100k 默认（无 AURA_HEAP_MB） | OOM exit 70 | **4.3 MB** | [OOM] HEAP FULL | **exit 0 正确输出** |
| fib28 | — | **8.9 MB** | +37.8B/调用 | 常数 |

- loop1100k **不再需要 AURA_HEAP_MB**：泄漏消失后默认 64MiB 静态 arena 足以容纳
  （真实存活数据只有常数级）；P0-2a 的 OOM 守卫与 P0-2b 的扩容机制保持原样，
  作为「真实存活集」超限时 的兜底/扩容通道（AURA_HEAP_MB=128 回归 ✓）。
- fib28 计时中位 3353→3288ms（**-2%**，值栈不再频繁倍增搬运）。
- 语义探针（`x=100 as Long; y=x+1; ...` 五值）与五门回归全绿。

### 10.4 方法论注记

本轮定位靠三个可复用工具/手段（均已入库）：

1. **`tools/photon-prof/auc-disasm.py`**：.auc 反汇编器（seed serialize v7 格式，
   natives/函数/操作码全解析）——直接看 guest 字节码里到底有什么；
2. **`HeapDiag.heapbump()`（extern `aura_heapbump`）**：arena 水位探针，配合
   「指令数 → 水位」曲线（`[IC]` 探针）一眼识别**倍增跳变 vs 线性泄漏**；
3. **可变探针最小化原则**：println 探针本身经同一 bump arena 分配，会污染测量
   （本轮 [CN] 探针曾把 loop1m 推进 OOM）——探针要么一次性（前 N 次）、要么用
   累加器均值。
