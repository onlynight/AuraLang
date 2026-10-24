# R167：VM 宿主为什么重建不出来 —— 两个后端/前端缺陷

日期：2026-10-22　分支：dev　前置：R165/R166（JIT 语言测试 9/26、8 例 INT-DIV-0 收敛到 `toStr(Float)`）

## 一、上轮留下的结构性阻塞

`build/hat-vmhost/current.exe`（Oct-17，67 MB）是**唯一能跑 `.auc` 的宿主**，但用当前驱动重建
同源码必然 `0xC00000FD`（栈溢出）。于是 `VmOps` / `Vm` 里的任何 VM 侧修复都**没有验证载体**。

本轮把它拆开了：两个独立缺陷叠加，都不是 VM 逻辑问题。

## 二、根因 A：`Memory.free(addr)` 被解析成 `Allocator_free(addr)` —— 自调用

### 证据链

1. 重建出的宿主在 `AucReader.readFileToBuffer` 里崩（栈溢出）。插桩步进到
   `[RF4] fd=...` 之后、`[RF4b]` 之前，即 `Allocator.free(pathBuf)` 这一行。
2. `Allocator.free` 本体在 Photon 运行时里是**空操作**（`Memory_free` 是 bump 分配器的
   no-op），单独调用（`build/vmhost-src/freeprobe.aura`）实测 `exit=0` —— 所以不是它的语义。
3. 反汇编 + 重定位给出了铁证（`build/vmhost-rf3/probe.obj`）：

   ```
   0xECFE  IMAGE_REL_AMD64_REL32  Allocator_getInstance
   0xED5E  IMAGE_REL_AMD64_REL32  Allocator_free        ← Allocator_free 函数体内的调用
   ```

   `Allocator.free` 体里那条 `Memory.free(addr)` 的 `call`，重定位指向**它自己** ⇒ 无限递归。
4. `.hat` 里同一处已经是错的：

   ```
   @fn Allocator_free(@addr: Long) -> #public|
   bb bb1094:
     @t7558 = @call @Allocator_free(@addr, @t7550) : Basic     ← 自调用
   ```

   说明缺陷在**前端名字解析**（SsaBuilder），不在发射器。

### 根因

`SsaBuilder.resolveMethodSymbol` 的判定阶梯：

```
0) 运行库裸名   0.1) extern 前缀剥离   0) 集合族
1) 接收者静态类型已知 → 声明类
2) 隐式 this：curClass 声明了同名方法就用它        ← 这里
3) 全局唯一属主   3) 保底裸名
```

`Memory.free(addr)` 的接收者是**单例模块名**（不是实例），`buildCall` 会把 `kids[0]` 这个
占位接收者丢掉，方法名保持裸名 `free`。但名字解析**先**走到第 2 步：当前所在类是
`Allocator`，它恰好声明了 `free` ⇒ 解析成 `Allocator_free`。

**为什么只有全量程序才复发**：`free` 这个名字同时被 `Memory`（native 声明，无函数体，
不进 `classMethods`）和 `Allocator`（有体）声明；小探针里两个类未必同时进
`classMethods`，走的是别的一条路。这是**登记顺序敏感**，不是随机。

### 修法（已落地）

在阶梯里插入 `1.5)`：接收者是单例 `Memory` 且方法名属于「内存内置族」时，直接给运行库
符号，表与 `InstructionSelection.resolveMethodSymbol` 的
「`name == "free"/"alloc"/…" && recvTy == "Long"`」一组**严格对齐**（那里能映射的这里才映射，
避免造出 runtime 里不存在的 `Memory_*`）。

修复后同一处重定位变成 `Memory_free`，`Allocator.free` 不再自调用，栈溢出消失。

## 三、根因 B：字面量接收者 `"aura.lang.".charCodeAt(qc)` 译成野指针

栈溢出修掉后，宿主能装载 `.auc`，但在 `Vm.callOrBuiltin` 的限定名剥离段崩（`0xC0000005`）。

逐字节探针（`[Q1]/[Q2]/[Q3]`）把它钉死在一行：

```
[Q1] qc=0
[Q2] cA=112          ← funcName0.charCodeAt(0)  正常（'p'）
（崩）               ← "aura.lang.".charCodeAt(0)  接收者是指向 .rdata 的字面量
```

- 换成局部变量接收者（`val qref: String = "aura.lang."`）后**整套流程跑通**（exit=0）。
- 同一表达式在小函数里是对的：`build/vmhost-src/litprobe.aura`
  （`"abcdef".charCodeAt(i)` ×3）输出 `97/98/99`，exit=0。
- 加局部变量到 200 个（`build/vmhost-src/slotprobe/k*.aura`）也不复现
  ⇒ **不是简单的槽数/帧大小阈值**，而是大函数 + 深调用栈下的布局敏感缺陷（P5-① 家族）。

因此这是**规避**（workaround）而非根治：在 `Vm.aura` 的 `qual24` 段把字面量落成局部，
并注明理由。根治要动 Photon 后端对「字面量作方法接收者」的取值路径，单独一轮。

## 四、修复后的状态

| 项 | 值 |
|---|---|
| 重建宿主 | `build/vmhost-r167/current.exe`，**500,736 B**，构建 7.6 s（原来是 67 MB / 无源码可重建） |
| hello `.auc` | 装载 → 分派 → `[VMDONE]`，**exit=0**（不再崩） |
| 语言测试 JIT 列 | **7/26**（旧宿主 9/26；见下逐例对比） |
| AOT 列 | **25/26**（唯一失败仍是 `16-script-mode`，用户已决定忽略）——**无回退** |
| VM 列 | 26/26 |

逐例变化（`build/langtest-3modes/r167new/results.csv` vs `vmhost26`）：

- 新宿主**修好**：`04-control-flow`、`14-string-interp`、`toString_implicit_test`（原来都是 AV）
- 新宿主**退化**：`10-memory`、`07-error-handling`、`test_super`、`test_super_only`、`test_this`

净 −2，说明「能重建」还没等于「能替代旧二进制」。

## 五、遗留：值模型仍然错

重建宿主跑 `hello.auc`（`println("hello-jit")`）时输出的是 **`0`**，旧宿主输出 `hello-jit`。
即 `LOAD_CONST` 取到的不是常量池里的字符串，而是它的**下标**。这与 `Vm.aura` 里多处
「`KIN`/数值模式 vs 文本模式」分支的取值错位一致（同属 P5-① 家族：大函数里某个
取值点位被译成错的槽）。

⇒ **下一步（T5）**：把 P5-① 当独立靶点治理，而不是继续逐点绕行。切入点建议：

1. 用 `Vm.callOrBuiltin` 这个 37 KB 帧的函数做样本，比对「同一表达式在小函数/大函数里的
   产码差异」，定位取值点位（栈槽 offset、`disp8`/`disp32`、字面量 `lea` vs `mov`）。
2. 复现资产已备：`build/vmhost-src/litprobe.aura`（正例）、`slotprobe/k*.aura`（槽数梯度）、
   `build/_vmhost_build.ps1`（HAT 驱动 → COFF → lld-link，8 秒一轮）。
3. 修好后再回来跑 `.auc` 26 例——那时 8 例 `toStr(Float)` 除零、4 例 `Unknown function`
   才有真实的验证载体。

## 六、本轮改动清单

| 文件 | 改动 |
|---|---|
| `aura/compiler/aura/lang/compiler/mir/SsaBuilder.aura` | `+1.5)` 单例 `Memory` 内置操作直接给运行库符号（根治自调用） |
| `aura/compiler/aura/lang/compiler/vm/Vm.aura` | `qual24` 段的字面量接收者落局部（规避后端缺陷） |

复现/验证资产（`build/` 被 gitignore，仅本地）：

- `build/_vmhost_build.ps1` —— 宿主构建器（`-Driver` 可指定驱动；`.ps1` 必须 UTF-8 BOM；
  `kernel32.Lib` 必须锁 `um\x64`）
- `build/vmhost-src/{probe,loadprobe,envprobe,freeprobe,litprobe}.aura`、`slotprobe/k*.aura`
- `build/vmhost-rf3/probe.obj` —— 自调用重定位证据
