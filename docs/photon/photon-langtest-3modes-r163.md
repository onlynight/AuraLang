# 语言测试三模式矩阵（R163，2026-10-22）

> 用例：`tests/language-test/*.aura`（26 个）
> 运行器：`scripts/photon/langtest-3modes.ps1`（新增）
> 结果 CSV：`build/langtest-3modes/r163/results.csv`
> 判据：退出码 0 且未超时 = PASS（与仓库既有 `lt-suite` / `hat-langtest.py` 口径一致）

三种执行模式：

| 模式 | 载体 | 说明 |
|---|---|---|
| **Aura VM** | `rust/target/release/aura.exe run <src>` | 种子 VM 编译成字节码后解释 |
| **Photon JIT** | `aura build <src> --output .auc` → `build/jit-verify/jit_vmhost.exe`（`AURA_VM_AUC`） | Photon 原生宿主，内含 `JitBackend` |
| **Photon AOT** | `PhotonHatCompile.exe`（HAT 驱动）→ COFF → lld-link → 原生 exe | 驱动自带链接，产物落 `AURA_HAT_OUT` |

## 一、结果

| 模式 | PASS | 占比 |
|---|---|---|
| **Aura VM** | **26 / 26** | 100% |
| **Photon AOT** | **25 / 26** | 96% |
| **Photon JIT** | **0 / 26** | 0% |
| 三模式全过 | 0 / 26 | — |

耗时：26 用例 × 3 模式全量跑完 **2 分 33 秒**（AOT 每例约 3–4 s，其余 <1 s）。

## 二、AOT 唯一失败：`16-script-mode`（既有缺陷，非本轮回归）

- 现象：驱动编译成功、exe 生成，**运行即 `0xC0000005`**（`run-exit=-1073741819`）。
- 对照实验：用**本轮修复前**的驱动（`build/hat-native/drv-pre-t3.exe`，即
  `PhotonHatCompile.exe.pre-t3.bak`）重编同一用例 → exe 同样 `0xC0000005`。
  ⇒ **与本轮 R162 两条运行时修复无关**，是既有缺口。
- 其余 25 个用例（含 classes / 异常 / imports / stdlib / annotation / overload）全过。

## 三、JIT 0/26：宿主 `.auc` 执行路径整体不可用（既有，非本轮回归）

不是「某些用例不支持」，而是**任意程序都跑不了**：

1. 26 个用例全部 `exit=-1073741819`（AV），无一例外；
2. 最小复现：一个只有 `println("hello-jit")` 的 `.auc`，宿主日志走到
   `[VMLOADOK]` → `[62] fname=[println] argc=1 fn=[main]` 之后**直接把内存当输出
   打出来**并 AV —— 即字节码已加载、调用分发这一步就崩；
3. 历史对照：`build/jit-lan-tests/results.csv`（2026-10-10，134 个 `tests/**`
   用例）的分类是 **106 FAIL-JIT-ONLY + 28 FAIL-BOTH + 0 PASS-BOTH** ——
   JIT 目标当时就一个都没过。

⇒ 与仓库里已记录的 P5-①「宿主布局漂移」是同一件事（`jit_vmhost` 内部
`env_get`/多实参外调在该布局下被腐坏）。

**重要区分**：**JIT 的代码生成本身是好的** ——
`bench-jit-aot-vm` 里 `sum(1e9)=250 ms`（0.25 ns/iter，手编机器码经
`JitBackend` 装载执行）。坏的是 **VM 宿主的 `.auc` 加载/调用分发路径**。
所以「JIT 0/26」不能拿来评价 JIT 代码质量。

## 四、可以看到的东西

- VM 与 AOT 在 25 个用例上**语义一致**（同一个 `.aura` 两边都 exit 0）；
  脚本额外记录了「末行输出」比对，全部 25 个一致用例的末行只在 JIT 侧不同
  （JIT 无输出，故列 `VM/JIT`）。
- `11-imports` / `14-string-interp` / `15-advanced` / `test_super` 这类跨模块、
  继承、字符串插值用例在 AOT 上通过 —— 说明 HAT 链的语义覆盖是成立的。

## 五、复现

```powershell
# 全量 26 × 3
powershell -File scripts\photon\langtest-3modes.ps1

# 只跑几个（子串匹配）
powershell -File scripts\photon\langtest-3modes.ps1 -Only 05,16 -Tag probe
```

脚本要点（踩过的坑）：

- 环境变量一律用进程级 `[Environment]::SetEnvironmentVariable` 注入 ——
  PS 5.1 下 `$psi.EnvironmentVariables` 为 `$null`（同 `bench-jit-aot-vm.ps1`
  的坑，那次的后果是 JIT 恒 -1）；
- 保存/恢复用 `Hashtable.Add()`，**索引赋值**在本脚本里实测会抛
  「无法对 Null 数组进行索引」；
- `.ps1` 必须存 **UTF-8 BOM**：无 BOM 时 PS 5.1 按 ANSI 读中文注释，
  行号会错位、报错定位失真。
