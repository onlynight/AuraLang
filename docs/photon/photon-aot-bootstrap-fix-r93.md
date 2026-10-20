# Photon AOT 自举编译修复报告

> 2026-10-19 21:30–22:50
> 目标：修复「Photon AOT 无法编译 Aura 自举编译器」
> 结果：**修复完成** —— 12m19.5s 跑通全链路，峰值私有内存 21.78 GB，
> 产出 `aura-compiler.exe`（2.9 MB），自编译产物可运行。

---

## 一、最终结果

| 指标 | 值 |
|---|---|
| 结果 | **`===RESULT===success`，exit 0** |
| 墙钟耗时 | **12m19.509s**（739.5 s） |
| 峰值工作集 | 20.11 GB（21,592,256,512 B）@ t=738.1 s |
| 峰值私有内存 | **21.78 GB**（23,388,250,112 B）@ t=738.1 s |
| `.hat` | 10.3 MB（10,835,246 B / 10,835,246 字符） |
| 主对象 | 4.0 MB（4,197,298 B） |
| 运行时对象 | 44,765 B |
| **产出 exe** | **2.9 MB（3,051,008 B），可执行** |
| 驱动 | `build/hat-native/PhotonHatCompile.exe` 1,760,256 B（2026-10-19 22:37 重建） |

### 阶段耗时（驱动内置 `AURA_PHOTON_TIME=1`）

| 阶段 | 耗时 | 占比 |
|---|---:|---:|
| link（114+ 模块 → 合并 HIR，242,544 节点） | 128.2 s | 17.3% |
| ssaBuild（HIR → SSA） | 101.6 s | 13.7% |
| hatSerialize | 0.99 s | 0.1% |
| getterPatch | 1.10 s | 0.1% |
| hatWrite（10.3 MB） | 45.7 s | 6.2% |
| **前端合计** | **277.6 s** | **37.5%** |
| hatReadParse（后端重读 .hat） | 3.8 s | 0.5% |
| phaseB（LIR） | 1.9 s | 0.3% |
| phaseC（DAG/指令选择） | 215.8 s | 29.2% |
| ra（寄存器分配） | 95.6 s | 12.9% |
| emit（x86 编码） | 92.6 s | 12.5% |
| writer（COFF 写出） | 48.8 s | 6.6% |
| objWrite + 链接 | 1.2 s | 0.2% |
| **后端合计** | **459.7 s** | **62.2%** |

### 内存曲线（50 ms 采样，8349 点）

```
t=  0.1s  priv=    1.3MB   ← 进程启动
t= 34.2s  priv=  773.1MB   ← link 早期
t=134.7s  priv= 2761.2MB   ← link 后期（大模块）
t=168-201s priv= 4428.5MB  ← link 收尾
t=235.0s  priv=11867.0MB   ← SSA 峰值
t=277.6s  priv=11931MB → 31MB   ← ★ RegionReset：11.9GB 在 300ms 内整块归还
t=302.5s  priv= 3718.3MB   ← 后端：重读 .hat 起步
t=525-561s priv= 9767.2MB  ← 后端平台期
t=707.6s  priv=22034.3MB   ← emit/writer 膨胀
t=738.1s  priv=22305MB     ← 峰值；随后进程退出，OS 回收至 0
```

修复前（同一驱动、同一输入）：**3m24.8s 即撞 8 GiB 闸门中止，峰值 20.7 GB 且仍在增长**；
修复后**跑完全程**，且前端内存出现设计预期的「涨-块回收」形态（RegionReset 回收 11.9 GB）。

---

## 二、修复清单（7 项，按发现顺序）

### 1. 区域符号重复定义 + 结构体布局冲突（P0，直接致 20.7GB 泄漏）

`aura_link_fix.obj` 与 `aura_std_cffi.obj` **各定义了一整套区域 API**——
`RegionEnter/RegionReset/RegionScratchTarget/RegionRetainTarget/ScratchMark/ScratchRewind/StrdupRetain`
以及状态变量 `g_scr/g_rgn_ret/g_rgn_active/g_rgn_target`（`llvm-nm` 证交集 13 个符号）。
`/FORCE:MULTIPLE` + 链接顺序（link_fix 在 cffi 之前）⇒ 链接器静默取 link_fix 的那份。

更糟的是两者**结构体布局不同**（link_fix 用 `chunks[256]` 定长数组，cffi 用动态数组），
同一符号名被两种布局互相解读 ⇒ 状态踩踏。后果：
- `ScratchRewind` 是 link_fix 的空转实现（不真正回拨）⇒ 内存只涨不落；
- cffi 的 `g_rgn_active` 是一份**独立 static**，从未被 `RegionEnter` 置位
  ⇒ `aura_scr_alloc` 永远走 `aura_mem_alloc`（堆），8 GiB 闸门提前触发。

**修法**：`aura_link_fix.c` 分成两个编译变体（同一份源码）：
- 默认（驱动用）：只保留 `String_*` 补齐，**不定义任何区域符号**；
- `-DAURA_LINK_FIX_EXE`（产出 exe 用）：提供完整、且与 cffi 语义一致的区域实现 + `Memory_*` 包装。
`PhotonPipeline` 的产出 exe 链接改用 `build/aura_link_fix_exe.obj`（重复链接点 2 处）。

### 2. `scratchRewind` 丢弃 chunk、记账不回扣

原实现两个分支都把 `g_scr.n` 清零/截断且**从不 VirtualFree**，被丢弃的 64 MB 块既不在链上也无从释放。
**修法**：`AuraRgn` 增加 `cur_idx`；rewind 只回拨 `(cur_idx, off)`、**保留 chunk 列表**，
bump 越过当前块时优先复用下一个已提交块；记账按 `new_used = Σ sizes[0..idx-2] + off` 精确扣回。

### 3. 字符串临时物无视 target —— 跨模块登记表被回收（最隐蔽的一个）

`aura_scr_alloc`（concat/substr/dup/strdup 家族）此前**无视 `g_rgn_target` 一律进 scratch**。
于是 `HirLowerer` 的跨模块登记表（`knownTypes` / `typeMethods` / `funcRetTys` / `localVarTys`…）
和 `AotModuleLinker.cacheBlob/stdMods` 都落在 scratch 里，**模块级 rewind 后全部悬垂**。

症状极具迷惑性：编译过程不崩、产物能生成，但 `.hat` 里 **1690 处调用返回类型从 `Int`/`Unit`
退化成 `Basic`**（方法签名解析失败），最终对象丢掉整个 `StringOps` 模块、`Collections` 退化成
裸名 `alloc/copy/free` 外调，链接失败。

**修法**：`aura_scr_alloc` 尊重 target —— `g_rgn_target==0`（retain 相）走
`aura_rgn_alloc_retain`，只有 `RegionScratchTarget()` 期间（parse 中间物）才进 scratch。

### 4. `kvGet` 每次迭代分配一个 `key + "="`（2.33 亿次分配 / 5.8 GB）

```aura
if (e.startsWith(key + "=")) { ... }   // ← 每个表项都新建一个临时串
```
登记表（局部变量/成员/函数返回类型）随规模增长，每次查找产生 O(表长) 次小分配。
`AURA_ATTR_DUMP` 归因：**单点 233,158,161 次、平均 26 B、累计 5.8 GB**。
**修法**：原地比较（先比长度、再逐字符比 key、再验 `=`），零分配。

### 5. `textOf` 每次访问逐字符反转义（O(L²)）

`Hir.textOf` 命中含 `\` 的文本时逐字符 `result = result + ch` 重建。
`textOf` 是前端最热访问之一（每个节点、每次类型解析都会调用）。
**修法**：新增 `textRaw` 列 —— `add()` 时一次落列（两条输入路径：编译器传原文、
`.hat` 反序列化传转义串），`textOf` **O(1) 直返**；旧逻辑保留为 `textRaw` 缺失时的回退。

### 6. `Hir.add` / `Ast.add` 的逐字符转义（O(L²)）

多行长文本（`Emit.aura` 的 LLVM IR 模板）每字符一次拼接。
**修法**：分块（substring 切段）+ 两两归并，O(L·log k)，输出逐字节等价。

### 7. 产出 exe 缺少区域实现

自举对象引用 `Memory_regionScratchTarget / Memory_scratchMark / Memory_strdupRetain` 等，
而产出 exe 不链接 CFFI。由第 1 项的 `-DAURA_LINK_FIX_EXE` 变体补齐（含修复后的 arena 语义），
保证「驱动编译正确」与「产出的编译器运行正确」一致。

---

## 三、验证

| 验证项 | 结果 |
|---|---|
| hello world 冒烟（驱动编译→链接→运行） | `Hello, World!`，exit 0，exe 51,712 B |
| 自举全链路 | `success`，exit 0，全部产物生成 |
| 自编译产物运行 | `BOOT-1` → `BOOT-1a` → `CEA-1`，exit 0 |
| 符号唯一性（驱动链接四对象） | 区域符号仅 `aura_std_cffi.obj` 一份；link_fix 仅 `String_*` 补齐 |

---

## 四、遗留瓶颈（下一轮，按收益排序）

1. **后端 459.7 s / 峰值 22.3 GB**——占墙钟 62%，且内存从 3.7 GB 涨到 22.3 GB。
   `phaseC 215.8 s`（DAG/指令选择）+ `ra 95.6 s` + `emit 92.6 s` + `writer 48.8 s`。
   后端在 `RegionReset` 之后运行、不享受区域回收，需单独做内存治理（同样怀疑 per-node 拼接）。
2. **`hatWrite 45.7 s`** 写 10.3 MB 明显异常（≈0.23 MB/s），疑似 O(n²) 拼接。
3. **SSA 101.6 s / 前端峰值 11.9 GB**：SSA 阶段无 mark/rewind 点，只能等 `RegionReset` 整块回收；
   可在 SSA 内按函数批量 mark/rewind（现有 arena 已支持）。
4. **默认 `AURA_MEM_LIMIT_MB=8192` 不够**：自举需 ≥ 24 GB 上限（本次用 16000 仍在后端达到 21.78 GB）。
   建议自举脚本固定 `AURA_MEM_LIMIT_MB=24000`，或按阶段分进程（前端/后端）。

---

## 五、复现

```powershell
# 1) （可选）重建 seed —— 本次只改了 Aura 源码与 CFFI，seed 无需重建
# 2) 重建驱动
cd D:\Code\AuraLang
$env:AURA_LLVM_HOME = "D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc"
rust\target\release\aura.exe build --aot aura\photon\aura\lang\compiler\photon\PhotonHatCompile.aura --output build\hat-native\PhotonHatCompile.exe

# 3) 两个 link_fix 变体
clang -c seed\compiler\src\std\cffi\aura_link_fix.c -o build\aura_link_fix.obj
clang -c -DAURA_LINK_FIX_EXE seed\compiler\src\std\cffi\aura_link_fix.c -o build\aura_link_fix_exe.obj

# 4) 测量（脚本内置 AURA_MEM_LIMIT_MB=16000、AURA_PHOTON_TIME=1、50ms 采样）
powershell -ExecutionPolicy Bypass -File scripts\photon\measure-bootstrap.ps1 -OutDir build\hat-bootstrap-r93-clean
```

产物：`build/hat-bootstrap-r93-clean/{aura-compiler.hat, aura-compiler.obj, aura_runtime.obj, aura-compiler.exe, summary.json, timeline.csv}`

---

## 六、改动文件

| 文件 | 改动 |
|---|---|
| `seed/compiler/src/std/cffi/aura_std_cffi.c` | `cur_idx` + chunk 复用；`ScratchMark/Rewind` 不丢块、记账精确；`aura_scr_alloc` 尊重 target；前置声明 |
| `seed/compiler/src/std/cffi/aura_link_fix.c` | 拆双变体；`AURA_LINK_FIX_EXE` 下提供完整区域实现 + `Memory_*`；`String_*` 改用私有 bump 区 |
| `aura/compiler/aura/lang/compiler/hir/Hir.aura` | `textRaw` 列 + O(1) `textOf`；`add()` 分块转义/反转义；`kvGet` 原地比较 |
| `aura/compiler/aura/lang/compiler/ast/Ast.aura` | `add()` 分块转义 |
| `aura/photon/aura/lang/compiler/photon/PhotonPipeline.aura` | 产出 exe 链接 `aura_link_fix_exe.obj`（2 处） |
| `scripts/photon/measure-bootstrap.ps1` | 新增：50 ms 采样 + 增量 timeline + `[time]` 阶段解析 + `AURA_MEM_LIMIT_MB` 参数 |
