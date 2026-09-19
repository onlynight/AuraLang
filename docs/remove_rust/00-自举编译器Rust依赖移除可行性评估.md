# Aura 自举编译器 Rust 依赖移除可行性评估

> **版本**: 1.0
> **日期**: 2026-09-19
> **状态**: 评估完成
> **结论**: 不能完全移除 Rust 代码依赖（VM 为空壳，自举仅覆盖 AOT 路径）

---

## 核心结论：不能完全移除 Rust 代码依赖

当前项目存在严重的文档与代码的认知差距。文档声称 "Phase S4: 完全脱离 Rust ✅"，但实际代码审查揭示出多个关键矛盾。

---

## 一、架构现状全景

| 轨道 | 位置 | 规模 | 状态 |
|------|------|------|------|
| Rust 实现 | `compiler/` + `cli/` | ~100 文件 / ~2.7MB | 完全可用 |
| Aura 实现 | `aura/compiler/` + `aura/core/` + `aura/toolchain/` | 233 文件 / ~2.1MB | 部分可用 |

---

## 二、可移除的 Rust 组件（已有 Aura 替代）

| Rust 模块 | Aura 对应 | 替代成熟度 |
|-----------|----------|-----------|
| `lexer.rs` (68KB) | `Lexer.aura` | ✅ 可替换 |
| `parser.rs` (164KB) | `Parser.aura` | ✅ 可替换 |
| `ast.rs` (26KB) | `Ast.aura` | ✅ 可替换 |
| `sema/checker.rs` (192KB) | `TypeChecker.aura` + `Type.aura` + `TypeInfo.aura` + `SymbolTable.aura` | ✅ 可替换 |
| `codegen/hir.rs` (278KB) | `Hir.aura` + `Desugar.aura` + `Mono.aura` + `Inline.aura` + `Fold.aura` | ✅ 可替换 |
| `codegen/mir.rs` (54KB) | `Mir.aura` + `MirLower.aura` + `MirOpt.aura` | ✅ 可替换 |
| `codegen/emit.rs` (36KB) | `Codegen.aura` | ✅ 可替换 |
| `codegen/aot/emit.rs` (265KB) | `Emit.aura` (9228行) | ✅ 可替换 |
| `codegen/aot/linker.rs` (24KB) | `Linker.aura` | ✅ 可替换 |
| `codegen/aot/c_backend.rs` (24KB) | `CBackend.aura` | ✅ 可替换 |
| `linker.rs` (14KB) | `Linker.aura` | ✅ 可替换 |
| `package.rs` (67KB) | `Package.aura` | ✅ 可替换 |
| `signing.rs` (7.5KB) | `Signing.aura` | ✅ 可替换 |
| `signature.rs` (13KB) | `Signature.aura` | ✅ 可替换 |
| `auz/` (44KB) | `Auz.aura` | ✅ 可替换 |
| `source_map.rs` (9KB) | `SourceMap.aura` | ✅ 可替换 |
| `span.rs` (2KB) | `Span.aura` | ✅ 可替换 |
| `token.rs` (9.5KB) | `Token.aura` | ✅ 可替换 |
| `errors.rs` (4KB) | `CompileError.aura` | ✅ 可替换 |
| `std/std_*.rs` (20+ 文件) | `std/*.aura` (20+ 模块) | ✅ 可替换 |
| `bootstrap/any_core.rs` | `Any.aura` + `toStr()` | ✅ 可替换 |
| `bootstrap/type_core.rs` | `Type.aura` + `TypeInfo.aura` | ✅ 可替换 |
| `bootstrap/value_check.rs` | `ValueCheck.aura` | ✅ 可替换 |
| `cli/main.rs` (1557行) | `Main.aura` + `Commands.aura` + `CompilerApi.aura` | ✅ 可替换 |
| `cli/lsp_main.rs` | `LspApi.aura` + `LspServer.aura` | ⚠️ 部分替换 |
| `cli/debugger_main.rs` | `debugger/Main.aura` | ⚠️ 部分替换 |

---

## 三、无法移除的关键 Rust 依赖

### 🔴 3.1 Aura VM 是占位桩

`Vm.aura` 的 `interpret()` 方法：

```aura
fun interpret(bytecode: Array<Byte>): Any {
    // Phase 4 占位实现
    return null
}
```

`VmRunner.aura` 虽然 ~900 行，但只是极限制的文字型 VM：
- 字节码是文本字符串（每行一条指令），不是二进制 `.auc` 格式
- 栈和局部变量全是 String 类型，无法处理 Int/Float/对象/数组
- 仅支持 ~25 个操作码，无对象/类/泛型/闭包/FFI 指令
- 无二进制 `.auc` 反序列化能力
- 无 ARC/GC 集成
- 无 JIT 桥接

**对比 Rust VM**（`mod.rs` 1870行 + `interp.rs` 86KB + `native.rs` 53KB + `jit.rs` 53KB + `heap.rs` 16KB + 20 个子模块），差距是质的。

### 🔴 3.2 自举流程完全不依赖 VM，走 AOT 路径

实际自举流程：

```
冻结载体 aura-compiler.exe (Rust编译的Stage-0)
  → 编译 Main.aura → LLVM IR → llc → clang → n1.exe
n1.exe (Aura AOT 后端编译的)
  → 编译 Main.aura → LLVM IR → llc → clang → n2.exe
  → 行为一致性验证
```

VM 完全不在自举路径中。Aura 编译器能编译自己为原生 exe（通过 AOT），但不能通过 VM 执行任何代码。

### 🔴 3.3 仍需 Rust 编译器的场景

| 场景 | 为什么需要 Rust |
|------|----------------|
| Stage-0 初始引导 | 冻结二进制是一次性由 Rust 编译器产出的 |
| `.auc` 字节码执行 | Rust VM 是唯一能执行 `.auc` 文件的能力 |
| 开发/调试 | Rust 编译器是实际开发的日常工具 |
| 基准测试 | Rust/Aura 编译器性能对比 |
| 文档生成 | `docgen.rs` (70KB) 无 Aura 对应实现 |
| 完整 LSP | Rust LSP (39KB) 有完整功能，Aura 版本功能不全 |
| 调试器 | Rust 调试器有完整 GDB 集成，Aura 版本有限 |

### 🔴 3.4 外部原生依赖不可避免

```
LLVM 23.1.0 (llc/clang)     ← AOT 编译必需（文本 IR → 机器码）
Cranelift (JIT 后端)        ← 决策保留为 native 库
libc / Windows CRT          ← 系统调用必需
aura_syscalls.c (1055行)    ← C FFI 桥接层
```

---

## 四、文档声称 vs 代码现实对比

| 文档声明 | 代码现实 | 差距等级 |
|----------|----------|----------|
| S2 自举编译 ✅ | Aura AOT 后端 (Emit.aura 9228行) 确实能生成 LLVM IR | 🟢 真实 |
| S3 自举运行 ✅ | 冻结二进制能自举编译 Main.aura → 一致 | 🟢 真实 |
| S4 完全脱离 Rust ✅ | Rust 编译器仍被大量使用 | 🔴 虚假 |
| "VM 自举" ✅ | Vm.aura 是 `return null` 占位 | 🔴 虚假 |
| "9 个 bootstrap 模块全部移除" | 代码中 bootstrap/ 目录完整保留 | 🟡 仅在AOT路径未引用 |
| "完整 VM 可用 Aura 编写" | VmRunner.aura 是简化文字VM | 🔴 虚假 |
| "19 个标准库模块全部替换" | std/*.aura 存在但大量依赖 @native 桥接 | 🟡 真实但需C运行时 |
| "工具链全部 Aura 实现" | CLI/LSP/Debugger/Loom 有 Aura 版本但功能不全 | 🟡 部分真实 |

---

## 五、当前实际的自举成熟度

```
真实能力（已实现）：
  ✅ Aura 编译器（通过 Rust Stage-0）可 AOT 编译自身 → 原生 exe
  ✅ 原生 exe 可通过内置 Aura AOT 后端再次编译自身
  ✅ 行为一致性验证通过
  ✅ 冻结二进制消除构建时对 cargo 的依赖（仅需 LLVM 工具）

未实现的能力：
  ❌ Aura VM 无法执行 .auc 字节码（是占位桩）
  ❌ 无 VM 执行 → 无 JIT 执行 → 无脚本模式执行
  ❌ Rust 编译器仍用于日常开发和测试
  ❌ docgen / 完整 LSP / 完整 Debugger 无 Aura 对应
  ❌ LLVM 工具链作为外部原生依赖不可避免
```

---

## 六、最终结论

### 不能完全删除 Rust 代码依赖

**原因归纳为三点：**

1. **Aura VM 是空壳**：`Vm.aura` 的 `interpret()` 返回 `null`，`VmRunner.aura` 是仅支持 ~25 个操作码的文字型简化 VM。

2. **自举走的是 AOT 路径，不是 VM 路径**：自举验证成功证明了 "Aura AOT 后端能生成 LLVM IR → 经 llc/clang 编译为原生 exe"，但这只覆盖了"编译"能力，不覆盖"运行"能力。

3. **Rust 编译器仍是日常开发基础设施**：`.auc` 执行、完整 CLI 命令、文档生成、LSP、调试器、基准测试都依赖 Rust 编译器。

### 可行的 Rust 移除路径

| 优先级 | 任务 | 预计工作量 |
|--------|------|-----------|
| **P0** | 实现完整 Aura VM（替换 `Vm.aura` 占位，支持二进制 .auc、完整指令集、对象/类、ARC、GC） | ~2000-3000 行 Aura |
| **P1** | 实现 Aura VM 的 .auc 二进制加载器（替代 Rust `serialize.rs`） | ~500 行 Aura |
| **P2** | 实现 Aura docgen（替代 `docgen.rs`） | ~2000 行 Aura |
| **P3** | 完善 Aura LSP/Debugger 功能覆盖 | ~2000 行 Aura |
| **P4** | 完成 VM 执行路径的 JIT 桥接（替代 Rust `jit.rs`） | ~1000 行 Aura |
| **P5** | 移除 Rust bootstrap/ 目录（在 VM 可用后） | 低 |

**在此之前，Rust 代码是不可避免的依赖。** 当前的 "Rust 移除" 只是 AOT 编译路径的自举，不是完整工具链的自举。
