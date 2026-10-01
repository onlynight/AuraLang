# ADR-001：极简 VM 指令集与唯一编号源

- **状态**：已接受（2026-10-01）
- **关联**：VM-PA-00 v3.1 §2.1 / P0.3 / P0.6；决策 D2、D5

## 背景

Aura VM（`aura/compiler/aura/lang/compiler/vm/`）的指令分派基于字符串助记符，
`Opcodes.aura` 定义了一套从未被分派器引用的数字编号（死代码→伪接线）。Rust VM 有
独立的 106 条 OpCode 编号。三套编号互不相通。

## 决策

1. **`Opcodes.aura` 是 Aura VM 唯一、权威的指令编号源**。所有 Aura VM 支持的指令
   在此定义数值常量（按功能分组，每组 10 个号段），分派器只对数值比较，不出现指令
   名字符串比较。
2. **编号对齐 Aura 文本字节码约定**（Codegen.aura 发出的助记符），与 Rust
   `OpCode::byte()` 编号**无关**——`.auc` 由 AucLoader 翻译为文本助记符后统一走
   `Opcodes.fromName` 编号，单点转换。
3. **协程指令（`Yield`/`NewCoroutine`/`ResumeCoroutine`）不进入指令集**（决策
   D5：协程废弃，仅保留 Actor）。旧 `.auc` 若含协程指令按"未知指令"报错退让。
4. **`GC_MARK`/`GC_SWEEP` 常量移除**（原 `Opcodes.aura` 定义但无分派、无实现）。
   GC 属 P1.5 ARC 扩展的工作范围，届时若需 VM 级指令再按本 ADR 流程追加。
5. **指令下沉边界**沿用 VM-PA-00 §2.1：集合构造/元素操作、并发语义层、ARC 扩展、
   FFI 助手、枚举、闭包、跨模块调用等以 Aura 标准库 + `CallNative` 形态落地，
   VM 指令集保持最小。

## 争议项落定

- **协程（3 条）**：废弃（D5，2026-10-01）。并发模型仅保留 Actor（P3.3）。
- **闭包（2 条）**：**下沉为 VM 层闭包对象**——`NewObject` + 字段 + `Call` 组合，
  闭包对象含「函数标识字段 + 捕获值字段」，调用时先构造实参数组再 `Call`；接受
  每次闭包调用多一层对象查找的性能代价（JIT 为独立并行线，成熟后覆盖热点）。
  执行落地在 P1.3；现有 `Closures.aura` 作为其实现基础保留（见 P0.6 决议）。

## P0.6 死代码决议

| 模块 | 决议 | 理由 |
|------|------|------|
| `Opcodes.aura` | **接线**（P0.4 完成） | 成为真实编号源；伪 import 变为真实依赖 |
| `Closures.aura` | **保留**，作为 P1.3 闭包实现的基座 | 有独立单元测试（`phase5_vm_tests`），upvalue 注册/解析 API 与 ADR 闭包决议兼容；P1.3 接线时并入 |
| `TailCall.aura` | **保留**，作为 P1.6 `ReturnTail` 的基座 | 同上；尾位置检测/递归检测逻辑在 P1.6 接入分派器 |

三项均不再是「有实现但无调用方」的悬置状态：Opcodes 已接线；Closures/TailCall
有明确的 P1 接线计划与本 ADR 背书。

## 后果

- 新增/修改指令必须先改 `Opcodes.aura` 再改分派器，编号源单一。
- 旧 `.auc` 中协程指令将无法在 Aura VM 上执行（D5 的预期代价）。
