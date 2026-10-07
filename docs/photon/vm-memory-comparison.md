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
