# Photon AOT 运行时 ARC 实施报告 —— P0 运行时地基 + P1 容器 ARC 化（R148）

> 2026-10-20。依据 `arc_implementation_plan.md`（D1–D7 / P0–P4 路线）实施前两个阶段。
> 改动范围：**仅** `seed/compiler/src/std/cffi/aura_std_cffi.c`（+563/−68）与
> `aura_std_cffi.h`（+15/−2）。发射器（emit.rs / aot/Emit.aura）零改动 —— P0/P1 按
> 方案定为「C 侧先走通，不开编译器发射」。

---

## 一、实施内容

### P0：运行时地基（零行为变化）

| 项 | 落地 | 说明 |
|---|---|---|
| D2 头改造 | `AuraMemHdr.pad → {rc, magic, flags}` | 16B 不变（size 8 + rc 4 + magic 2 + flags 2），负载 16B 对齐 / Plan A 不受影响；rc 独立 32 位字段 ⇒ 原子模式可直接 Interlocked |
| 分配即持有 | `aura_mem_alloc` 经 `aura_hdr_init` 写 rc=1 + magic=0x4155("AU") | realloc 整块搬迁，rc/magic 原样保留（dynlist 热路径零额外开销） |
| hdr_sane 强校验 | magic + size 双校验 | double-free / use-after-free / 外来指针可检测（误配概率 ~2^-48） |
| D2 流程 | `aura_arc_retain` / `aura_arc_release` | 五道防线：NULL → Plan A 装箱（低位 1）→ D1 时代闸门（g_rgn_active）→ D2b .rdata 区间 → D2c 悬空区域 → magic。任一不过即安全 no-op，**绝不写不该写的内存** |
| D2b .rdata 注册 | `aura_lit_register`：解析主模块 PE 头取 `.rdata` 区段 | 采用方案候选②（不依赖发射器符号，两条发射链一致适用）；llvm-objdump 实测字面量落 `.rdata`（驱动 obj 68KB / exe 192KB） |
| D2c 防线 | RegionReset 前登记已归还区块区间（有界表 1024） | **关键修正**：初版仅按区间判定 → 自举实测 1.86M 次假阳性（RegionReset 归还的地址空间被后端堆段增长复用），已改为「区间命中 + VirtualQuery 探活」：页未提交才算真悬空，页已复用放行到 magic 校验。修正后 rgn=0 / rgn_reused=1.86M / bad_magic=0 |
| 泄漏报告 | `AURA_LEAK_REPORT=1` 退出时 HeapWalk 走 CRT 进程堆 | 按 magic 识别本运行时负载：size 直方图（与分配侧同桶口径）+ rc 直方图 + top-20 最大块 + arc stats；热路径零成本（退出时一次） |
| D5 native 审计 | 8 处修正 | `env_get`/`fromCharCode`/`io_readFile` 裸 malloc → `aura_mem_alloc_heap`（带头、跨 RegionReset 存活，语义逐字节不变）；`padStart`/`time_format`/`readLine` static 缓冲互相覆盖 → 每次独立副本；`replace`/`substringBefore` 借用返回（`return s`）→ 新副本（否则 P2 发射器把结果当新持有 ⇒ rc 少 1 ⇒ 提前释放） |
| 旧 ARC 入口 | `aura_arc_increment/decrement` 转发到门控 retain/release | 旧实现把 refcount 写在负载首字节且无闸门（对 .rdata 会写坏只读段）；原零调用者。decrement 改为返回递减后的 rc（头文件同步） |

### P1：dynlist / Map 容器 ARC 化

| 路径 | 动作 |
|---|---|
| `dynlist_push` | 入场 retain；**装箱检查内联在调用点**（自举后端 3.68 亿次 retain 调用中 ~89% 是装箱整数跳过，-O0 编译下每次都是真实函数调用 —— 内联后总墙钟从 +5.6% 回到 +0.7%） |
| `listRemoveAt` | 被移除元素 release（C 函数返回**列表**而非元素 ⇒ 所有权无转出对象，D3「从容器移除时持有者负责 release」） |
| `listPop` | **不** release（返回被弹出元素 ⇒ 所有权转出给调用方，D4「返回值所有权随转移」） |
| `listSet` | 覆盖：release 旧值 + retain 新值；越界追加走 push |
| `mapSet` / `hashMapPut` | 同上；键插入 retain（mapSet 仍 strdup 新建键，rc=1 随存储转移） |
| `split`/`mapKeys`/`mapValues`/`filter`/`map`/`take`/`listOf`/`pairOf` | 经 push 自动 retain |
| 静态注册表情族（`aura_collections_listOf/arrayListOf/linked*/hashSet*`，`aura_lists[32]` 永恒存储） | 不 retain（同 .rdata 字面量类：永恒存储 = 文档化泄漏；这些是 legacy 兼容垫片，上限 32 列表） |

---

## 二、验收结果（全绿）

### C 级单测（`/tmp/arc_test.c`，24 项）
alloc/retain/release 记账不变量；release 归零真正 free；.rdata 字面量与静态缓存
retain/release 后内容完好；区域 era release no-op；**D2c 悬空区域指针 release 不崩**
（区间防线命中）；fromCharCode/env_get 带头分配可 release；split/removeAt/pop/
append 内容与长度语义不变；Map 覆盖不坏旧值。**24/24 PASS**，泄漏报告输出
rc 直方图与 arc stats 符合预期。

### 冒烟（驱动全链，10 例）
P0/P1/helloworld/simple/simple_var/var_test：驱动 success + 产物 exe 与 VM
stdout 逐字节一致（产物 exe 的 `VEHI` 行是 PhotonRuntime.aura 的**既有** VEH 诊断，
写 stderr，与本次改动无关 —— 该 exe 不链接 aura_std_cffi.c）。
**PASS=10 FAIL=0**。

### 自举全链（Main.aura → aura-compiler.exe，驱动带 AURA_LEAK_REPORT=1）
- `===RESULT===success`，exit 0；产物无参运行打全 BOOT-1…BOOT-4 标记；
- **.hat（10,913,325 B）md5=263a3bbd… 与 .obj（4,213,496 B）md5=54b33c02… 均与
  R147 基线逐字节一致**（多次运行复验）；
- 驱动峰值 WS 4809 MB（R147 同量级）。

### 泄漏报告 × 归因对账（P0 验收项）
后端时代（RegionReset 后）退出时：

| 口径 | 数字 |
|---|---|
| live 堆块 | **41,275,574 块 / 4334 MB** |
| size 桶 | ≤64B: 39.7M/544MB（串临时物主体）；≤8K: 229K/632MB（列表数组）；≤1M+>1M: 3774/2839MB（活结构） |
| rc 直方图 | rc=1: 35.87M（未共享）；**rc=2: 4.80M（容器持有）**；rc=3-5: 601K |
| arc stats | retain=6,076,459 release=348 **freed=0**（P1 无发射器 release ⇒ 无一归零，符合预期）；noop era=5.1M lit=34.7M boxed=3.68亿 **rgn=0**（无真悬空）rgn_reused=1.86M（区间假阳性排除）**bad_magic=0** |

归因（AURA_ATTR_DUMP=1 同跑，top-30 站点）：`dynlist_push` realloc 增长
1,077,006 次 / 2958MB（与 R145 记录一致）、AOT 调用点 16.1M 次 / 2586MB、
`aura_scr_alloc`（concat/dup/substr 串临时物）29.9M 次 / **977MB**、
`aura_malloc` 307MB、`mem_realloc` 233MB、`dynlist_new` 337MB。
串临时物 ~1GB + AOT 级字符串构建 ≈ 方案「~1.2GB 临时物」量级 ✓ 对账闭合。

### 性能 A/B（背靠背，同机）
| 运行时 | 墙钟 |
|---|---|
| 旧（git stash） | 129.7s |
| 新（初版，未内联装箱检查） | 136.9s（+5.6%） |
| 新（内联后，最终） | **130.6s（+0.7%，噪声带内）** |

---

## 三、与方案的偏差 / 新发现（供 P2 参考）

1. **D2b 采用候选②**（运行时解析 PE 取 .rdata），非候选①（发射器符号
   `__aura_lit_begin/end`）—— 零发射器改动、双发射链一致；符号方案留作 P3
   「字面量省略」的编译期判定基础。
2. **D2c 区间表有假阳性**（详见上）：地址空间复用使「落在已归还区间」≠「悬空」。
   已加 VirtualQuery 探活；教训：**防线本身也需要随运行时分辨真假阳性**。
3. **产物 exe 是第三套运行时**：自举产物（aura-compiler.exe）不链接
   aura_std_cffi.c，而链接 `aura_runtime.obj`（PhotonRuntime.aura 发射的机器码
   运行时：strcat/strdupRetain/listAppend…）+ `aura_link_fix_exe.obj`。
   P2 的发射规则必须**三侧对齐**（seed emit.rs / aot/Emit.aura / PhotonRuntime.aura），
   比方案「双发射器」的表述多一路；P0/P1 的 ARC 地基在产物 exe 侧尚不存在。
4. **removeAt 与 pop 的所有权划分**：方案 D4 表「remove/pop：所有权转出」与 P1
   「listRemoveAt 对被移除元素 release」表述冲突。按 C ABI 实测裁决：
   listRemoveAt 返回**列表**（元素无转出对象）⇒ release；listPop 返回**元素** ⇒
   转出不 release。已在代码注释中固化该契约。
5. **C 侧无 `clear` 绑定**（方案 P1 提及 listRemoveAt/clear）：AOT CFFI 只有
   remove/removeAt/pop 三个移除入口；`Collections_clear` 符号不存在（P2 如需
   覆盖语义要补）。
6. **photon-suite.ps1 / photon-hat-suite.ps1 已腐化**：seed 编译器移除 photon
   后端后 `-b photon` 直接报错，且 VM stdout 混有 `[ok]/[vm]` 进度行使差分口径
   失效。本轮改用「驱动直跑 + VM 进度行过滤 + stdout 分离」口径；建议下轮重修
   这两个脚本（或改走驱动直跑）。
7. **-O0 编译放大了调用开销**：C 运行时无 -O 旗标 ⇒ 新增的 retain/release 调用
   按全价函数调用计费。P3 可评估 `-O2` 编译 C 运行时（全局收益，需全量回归），
   以及发射侧的 retain/release 配对消除（同槽相邻 cancel）与字面量省略。
8. `aura_arc_decrement` 签名由 void 改为 int（返回递减后 rc）：原零调用者，
   头文件已同步；若 P2 要在 Aura 侧调用需按新签名。

## 四、下一阶段（P2）入口条件

- 发射规则表（D4）三侧同轮落地，先挂 `AURA_ARC=1` 开关；实现顺序：局部槽覆盖/
  作用域退出 → 字段/列表 → 实参传递；
- 验收：.hat/.obj 逐字节一致（ARC 不改输出）+ 自举 success + BOOT-4 + 泄漏报告
  显示后端 live ≈ 活结构真实大小（~2.5GB 量级）；
- 前置修补：两个腐化的 suite 脚本；PhotonRuntime.aura 侧补 ARC 地基（头 + 闸门）。

## 五、复现命令

```powershell
# 单测（24 项）
clang arc_test.c aura_std_cffi.c aura_syscalls.c -o arc_test.exe   # 同目录 -I cffi
AURA_LEAK_REPORT=1 ./arc_test.exe

# 重建驱动（C 改动经 clang 热编译进链接，mtime 缓存自动失效）
rust\target\release\aura.exe build --aot aura/photon/aura/lang/compiler/photon/PhotonHatCompile.aura --output build\hat-native\PhotonHatCompile.exe

# 自举验收（泄漏报告 + BOOT-4 + md5 对比）
powershell -File build\arc-acceptance.ps1 -OutDir build\hat-bootstrap-arc3
```

回滚：`git checkout seed/compiler/src/std/cffi/aura_std_cffi.{c,h}` 后重建驱动
（Aura/Rust 侧零改动，发射产物不受影响）。
