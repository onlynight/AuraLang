# JIT 模式语言测试：宿主换轨 + 失败分类（R165，2026-10-22）

> 承接 `photon-langtest-aot-jit-diagnosis-r164.md`。
> AOT 侧 `16-script-mode` 按用户决定**忽略**（AOT 不支持脚本模式），本轮只动 JIT。

## 一、结论：之前的 0/26 是**测错了宿主**，不是 JIT 不能用

`langtest-3modes.ps1` 原先把 JIT 目标指向 `build/jit-verify/jit_vmhost.exe`
（`.auc` + `JitBackend` 闭包同进程）。那是 `docs/pure_aura_jit/09` 明确标注的
**P5-④「不承诺」**组合：JIT 自检 13/13 全绿，但 `.auc` 分派实参被 P5-① 布局腐坏，
**连 4 行 hello 都崩**。拿它出数只能得到 0/26，与语言测试内容无关。

改用**纯 Aura VM 宿主** `build/hat-vmhost/current.exe`（同一份 `VmRunner`，不带
JIT 闭包）后：

| 宿主 | PASS |
|---|---|
| `jit_vmhost.exe`（.auc + JitBackend 闭包，P5-④） | 0 / 26 |
| **`current.exe`（纯 Aura VM 宿主）** | **9 / 26** |

⇒ 同一个 `.auc`、同一份解释器源码，差别只在宿主闭包 ⇒ 印证 P5-① 是**宿主构建
布局**问题，而非 VM 值模型整体不可用。

## 二、剩余 17 个失败的分类（这才是真靶点）

| 类别 | 数量 | 现象 | 判断 |
|---|---|---|---|
| **INT-DIV-0** | **8** | `0xC0000094`（整数除零） | 单个根因最可能：某 opcode/索引 `% cap` 的 cap=0（空表）—— 8 例同形 |
| **UNKNOWN-FN** | **4** | `Unknown function: __throw` / `__new_exception` / `ThreadOps.cores` / `Collections.e…` | 内建**路由**没接住：这些名字在 `callOrBuiltin` 里有分支（如 `Vm.aura:2038` 的 `__throw`），但调用走到 `callFunction` 的函数表查找后报错 ⇒ 分派入口不一致 |
| **AV** | 3 | `0xC0000005` | `04-control-flow` / `07-exception-aot-test` / `14-string-interp` |
| **Invalid closure id** | 1 | 闭包 id 失效（`03-functions`） | 闭包表与函数表口径不一致 |
| **LOAD-FAIL** | 1 | `loadAonly valid=false funcs=0 err=[]`（`01-lexer`） | 装载器静默失败（**连错误信息都没设**，本身是个缺陷）；同批另两个 `.auc`（`11-imports`/`17-function-overload`）在装载期栈溢出 |

按「一个根因覆盖多少例」排序，下一轮首选 **INT-DIV-0（8 例）**，其次
**UNKNOWN-FN（4 例，看着像一处路由选择就能全解）**。

## 三、本轮落地的工具改动

`scripts/photon/langtest-3modes.ps1`：

- **默认 JIT 宿主改为 `build/hat-vmhost/current.exe`**（原默认是 P5-④ 的
  `jit_vmhost`，出不了数）；`-JitHost <path>` 仍可切回去做对照。
- **新增 `-SlotSwap`（默认开）**：纯宿主硬编码 `build/vm/current.auc`，改为
  「备份 → 换槽 → 跑 → 恢复」；`-SlotSwap:$false` 配合 `-JitHost jit_vmhost`
  则走 `AURA_VM_AUC` 通道。跑完自动恢复槽文件。
- 判定口径不变（退出码 0 + 未超时），CSV 多一列 `Note` 记录 VM 错误原文。

`build/_vmhost_build.ps1`（新增，本地工具）：用 HAT 驱动把任意 `.aura` 宿主
源码编成原生 exe（`driver → COFF → lld-link`）。两个必踩的坑已写进注释：
1. `.ps1` 必须存 **UTF-8 BOM**，否则中文注释按 ANSI 解码会报「语句块缺少 }」；
2. `kernel32.Lib` 必须锁 **`um\x64`**，否则 `Get-ChildItem -Recurse | Select -First 1`
   会拿到 arm64 变体，lld 报 `machine type arm64 conflicts with x64`。

## 四、可以立刻验证的一件事

`01-lexer` 的 `valid=false err=[]` 是**装载器不设错误信息**的静默失败；
用 `AURA_VM_LOADONLY=1` 可以一行拿到装载摘要：

```
VM-ERROR=loadonly valid=false numeric=false err=[] funcs=0 consts=0 name=
```

同批里 `07-exception-simple` / `test_this` / `10-memory` 却是
`valid=true numeric=true funcs=5..8 consts=11..16 name=default` —— 说明 `.auc`
格式本身没问题（三者头部字节完全相同），是**内容相关**的装载分支。

## 五、续查（同日）：8 例 INT-DIV-0 收敛到**一个函数**

用最小特性探针二分（`build/divprobe/`，每例 `aura build` → 槽交换 → 纯宿主）：

| 探针 | 内容 | 结果 |
|---|---|---|
| p1 | `7 % 3` + while 循环 | exit 0 ✅ |
| p2 | class + 方法 + 字段 | exit 0 ✅ |
| p4 | `arrayListOf` + add + 下标 | exit 0 ✅ |
| p5 | 字符串插值 `"hi ${name}!"` | exit 0 ✅（插值里 name 为空，另一处小缺口） |
| **p6** | **`println(toStr(1.5))`** | **0xC0000094** ❌ |
| **p7** | `val f: Float = 1.5; println(toStr(f))` | **0xC0000094** ❌ |

⇒ **8 例 INT-DIV-0 是同一个根因：`toStr(Float)` / Float 渲染路径上的整数除零**。
定位范围收窄到 `VmOps` 的「Float 文本通道」（`isFloatText` / `floatMantissaOf` /
`floatPow10` 一族，`VmOps.aura:95-200`）与 `Vm.aura` 的 toStr 分派；
`%` 常量、类、列表、插值均无关。

## 六、结构性阻塞：VM 侧修复目前**无法验证**

- `jit_vmhost.aura` **可以**用当前驱动重建（`build/vmhost-jv2/jit_vmhost.exe`，
  582,144 B，自检 13/13 全绿）——但 `.auc` 路径**旧构建与新构建崩得一模一样**
  （同一 hello `.auc`，两边都 AV）⇒ P5-④ 是**代码缺陷**，不是陈旧二进制，
  重建成不了救命稻草。
- 纯 VM 宿主 `build/hat-vmhost/current.aura` **重建失败**：补 `Heap.init()` 后仍是
  `0xC00000FD`（栈溢出），而 Oct 17 的旧 `current.exe` 是好的。

⇒ 现状是：**唯一能跑 `.auc` 的宿主（current.exe）无法重建**，所以 `VmOps`/`Vm`
里的 Float 修复即使写出来，也没有可执行的验证载体。下一轮的第一件事应该是
**让 `current.aura` 能被当前后端编出可用宿主**（对齐 jit_vmhost 的
`Heap.init` + 自检/VEH 骨架），否则所有 VM 侧修复都是"改了看不到效果"。
