# 语言测试 AOT / JIT 失败定位（R164，2026-10-22）

> 承接 `photon-langtest-3modes-r163.md`：AOT 25/26、JIT 0/26。
> 本轮把两条失败各钉到**具体指令 / 具体分支**，但均未落地代码修复
> （原因与下一步见文末）。

## 一、AOT：`16-script-mode`（脚本模式）

### 1) 最小复现（4 行，无 imports、无 main）

```aura
println("hello-script")
```

驱动编译成功、exe 生成，运行**先正确打印 `hello-script`，随后 `0xC0000005`**。
对照：

| 用例 | 结果 |
|---|---|
| `println("hello-script")`（脚本模式，无 main） | 打印正确 → **AV** |
| `fun main(): Int { println("hello-main"); return 0 }` | exit 0 ✅ |
| `fun main(): Unit { println("hello-unit") }` | exit 0 ✅ |
| `fun main(): Int { helper(); ... }` + `fun helper(): Unit` | exit 0 ✅ |

⇒ **不是** Unit 返回值、不是多函数、不是 std 调用的问题，是**脚本模式的 main 合成**。

### 2) 崩点（链接后反汇编，m1.exe `main` @ `0x14000414e`）

```
1400041c8:  movq $0x0, %rcx
1400041cf:  movq $0x0, %rdx
1400041d6:  movq 0x10(%rcx,%rdx,8), %rax     ← 读地址 0x10 → AV（rcx=rdx=0）
```

同形态在 main 里出现 **3 次**（`0x41d6 / 0x422b / 0x4280`），之前是几十条
`movq $0, -0xNN(%rbp)` 的零初始化。而 m1 的源码只有一条 `println` 语句
—— 说明**合成的 main 体里塞进了远多于「一条语句」的东西**，且其中包含
「以 null 为基址的字段读」（`__obj_slot_get(null, i)`）。

### 3) 根因判断

`Hir.synthesizeMainInKids`（`aura/compiler/aura/lang/compiler/hir/Hir.aura:734`）
在「无 main」分支里：

```aura
if (this.isHirDeclaration(id, false)) { declKids = ... } else { stmtKids = ... }
```

`isHirDeclaration(id, false)` 的第二个参数为 **false** ⇒ 顶层 `HirValDecl` /
`HirVarDecl` 被当作**语句**（这是脚本模式的设计：`var i = 0` 要成为 main 的局部
变量）。问题在于这些 `kids` 是**模块合并后**的顶层节点（`ModuleLink.link()` 已经
把 std 与各 import 模块的节点并进来），于是**来自 std / 被 import 模块的顶层
val/var 与其初始化表达式也被卷进合成的 main 体**，在「单例尚未构造」的时刻执行
→ 字段读的基址是 null（rcx=0）→ 读 `0x10` 段错误。

对照：**已有 main** 的分支只把 `HirValDecl/HirVarDecl` 拼到 main 体首
（P40x-f），**不搬语句**，所以 m2/m3/m4/m5 全绿。

⇒ 修法方向（未实施）：合成 main 时只把**入口模块**自己的顶层可执行语句包进去；
合并进来的模块级 val/var 应按声明处理（或按「模块初始化」另行发射）。
诊断口径：`synthesizeMainInKids` 的 `stmtKids` 条数应与入口模块顶层语句数相等
（m1 应为 1）—— 现在明显多得多。

## 二、JIT：`jit_vmhost` 跑不了任何 `.auc`

### 1) 三个已确认的事实

1. **JIT 自检全绿**：`AURA_VM_SELFTEST=1` → `SELFTEST-RESULT pass=13 fail=0`、
   `SELFTEST-ALL-PASS (core)`。即 `allocRW → writeCode → VirtualProtect RX →
   executeNative` 这条**原生链是好的**（基准里 sum(1e9)=250 ms 也印证）。
2. **崩在 .auc 的解释分派**：日志走到
   `[62] fname=[println] argc=1 fn=[main]` 之后开始喷内存并 AV。
   该行来自 `Vm.aura:1182` 的 op 62（`CALL_NATIVE_ARGS`）诊断探针 ——
   **`fname` 与 `argc` 都解析正确**，说明失败点在 `callOrBuiltin(fname, argc)`
   取实参：实参是裸值/错位，`println` 把它当 `char*` 解引用 ⇒ 打印内存。
3. **纯 VM 宿主分两种健康度**：`build/hat-vmhost/current.exe`（无 JitBackend 闭包）
   跑 hello `.auc` **完全正常**（`hello-jit`，exit 0）；同一份 `VmRunner` 源码在
   `jit_vmhost`（带 JitBackend/JitStackMap/OsrTable 闭包）里就崩 —— 与
   `docs/pure_aura_jit/09 §七·七` 记录的 **P5-① 发射器实参窗腐坏（布局敏感）**
   一致；文档已把它标为 **P5-④「.auc + JIT 闭包组合装载」不承诺**。

### 2) 另一层：纯宿主也跑不全语言测试

`current.exe` 跑 `02-types-variables.auc` → `0xC0000094`（**整数除零**），无输出。
⇒ 除了 P5-①，`VmRunner` 的**值模型/字节码覆盖**本身也有缺口
（宿主源码头部已列：Int 装箱 `(v<<1)|1` 与裸 Long 双形态、`as Int` 不重装箱、
方法内多参错位、natives 结果裸值入栈被 `toStrAny` 当指针解引用）。

## 三、为什么本轮没有改代码

- **AOT**：修法要改 `synthesizeMainInKids` 的「哪些节点算语句」口径，而它同时
  服务脚本模式与模块合并两条路径；改错会让**已有 main** 的用例回归（25 个绿点
  全靠那条路径）。必须先补一个 `println("x")` 脚本模式的最小用例进
  `tests/language-test/`，再改口径并全量回归 26 例 —— 这是独立一轮的工作量。
- **JIT**：P5-① 是发射器的实参腐坏（布局敏感、无稳定复现），P5-④ 组合装载
  文档已明确不承诺。硬改宿主源码只会把崩点挪到别处（本地已验证：
  同一驱动对同一源码的产物逐字节确定，崩点随**宿主源码**变，而不随运行次数变）。

## 四、下一步（可执行清单）

1. `tests/language-test/` 增加 `18-script-mode-min.aura`（就一行 `println("hi")`），
   把它作为脚本模式的常驻回归点。
2. 在 `synthesizeMainInKids` 里给 `stmtKids` 加**来源过滤**（只收入口模块的顶层
   语句），或给 `Hir` 节点加「来源模块」标记后按模块过滤；改完跑 26 例全量 +
   自举等价性（`.hat`/`.obj` 逐字节）。
3. JIT 侧：先用 `current.exe`（纯宿主）把 26 个 `.auc` 的通过率打出来，把
   「VmRunner 值模型缺口」与「P5-① 闭包布局」**分开计数**，再决定先修哪个。
