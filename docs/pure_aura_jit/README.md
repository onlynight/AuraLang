# �?Aura 化技术方案（全域�?
> **文档定位**：除 `compiler/src/bootstrap/`（最小引导层，保�?Rust）、Cranelift（JIT 机器码后端，保留 Rust）、syscall 运行库（Rust 重写）外，全部组件的 Aura 化执行方�?> **配套目录**：`docs/pure_aura/`（宏观路�?A–E 阶段）、`aura/compiler/`（纯 Aura 编译器实现）、`aura/core/`（纯 Aura 标准库）
> **代码边界**：除 bootstrap + Cranelift + syscall 运行库（Rust 重写）外，编译器前端、VM 解释器、AOT 发射器�?*JIT �?Aura �?*、标准库、CLI、LSP、调试器、loom 构建系统全部�?Aura 实现
> **执行模式**：覆�?VM 解释�?+ JIT（Cranelift�? AOT（LLVM）三种执行模�?> **文档日期**�?026-09-13（与 `docs/pure_aura/03-差距分析.md` 同步�?> **状�?*：宏观路�?A–E 阶段 85�?00% 完成；本方案聚焦**JIT FFI 边界 + VM 集成 + Std Tar/Zstd + 全域验证**
> **实际进度**：AOT �?100%（自举链已跑通）、Toolchain �?100%（CLI/LSP/loom/debugger 完成）、JIT �?Aura �?�?100%�? 文件 ~3440 行）、Std ⚠️ 85%（仅 Tar/Zstd 待补�?
---

## 〇、JIT �?Aura 化策�?
### 0.1 JIT 分层：纯 Aura �?vs Native �?
JIT 模式分为两层，纯 Aura 侧可 Aura 化，native 侧通过 FFI 边界保留 Rust�?
| �?| 职责 | 实现 | Aura 化状�?|
|----|------|------|------------|
| **�?Aura �?*（L3–L7�?| Clif IR 生成、优化、派发、状态管�?| `aura/.../jit/`�? 文件，~3440 行） | �?100% |
| **FFI 边界**（L8–L9�?| Cranelift 编译、mmap 加载、派发调�?| `compiler/src/bootstrap/jit_ffi.rs`（新增） | �?保留 Rust |

**�?Aura �?*（Aura 化）�?- `JitState.aura`：热点检测、编译顺序规划、分发表管理
- `JitOpt.aura`�? 个优�?pass（常量折叠、死码消除、跳转线程化...�?- `JitLower.aura`：字节码 �?Cranelift 文本 IR�?clif�?- `JitDispatch.aura`：派发决策（NATIVE/SKIP/DEFER）、解释器回退
- `JitCore.aura`：字节码预解码、JitUnit 形态、分支重解析
- `JitAbi.aura`：JitValue ABI 定义�?3 个类型标签）
- `JitUtil.aura`：公共辅助（行表/记录�?函数�?CSV 解析�?- `JitRuntime.aura`：W^X 段加载描述（FFI 占位�?
**Native �?*（保�?Rust，FFI 边界）：
- `jit_compile(clif_text)`：Cranelift 编译 .clif �?机器�?blob
- `jit_load(blob)`：mmap(RW) �?copy �?mprotect(RX) �?注册分发�?- `jit_call(entry_token, args, out)`：dispatch_table + call_indirect

### 0.2 为什�?JIT 可以�?Aura 重构

**关键洞察**：JIT �?*纯函数翻�?*部分（Clif IR 生成、优化、派发逻辑）可以用 Aura 实现�?只有**运行期机器码操作**（Cranelift 编译、mmap）必须保�?Rust�?
```text
�?Aura 侧（Aura 化）                     Native 侧（保留 Rust�?─────────────────────────────             ─────────────────────────
字节�?�?JitUnit (JitCore.aura)           �?FFI: jit_compile(clif)
JitUnit �?优化 (JitOpt.aura)              �?Cranelift 编译 �?机器�?优化结果 �?Clif IR (JitLower.aura)       �?FFI: jit_load(blob)
Clif IR �?派发决策 (JitDispatch.aura)    �?mmap + mprotect
状态管�?(JitState.aura)                  �?FFI: jit_call(entry, args, out)
                                      �?dispatch_table + call_indirect
```

**对比 AOT**�?| 维度 | AOT | JIT |
|------|-----|-----|
| �?Aura �?| HIR �?LLVM IR 文本 | 字节�?�?Clif IR 文本 |
| Native �?| 外部进程（llc/clang�?| 进程内库（Cranelift�?|
| 为什么不能脱�?Rust | 不需要（文本 IR + 外部工具链） | 必须进程内库（毫秒级编译�?|
| Aura 化程�?| �?100%（发射器�?Aura�?| �?�?Aura �?100% + native �?FFI |

### 0.3 AOT 模式 native 内容不动

AOT 模式�?native 相关内容**不做任何修改**�?
| 组件 | 位置 | 说明 | 是否修改 |
|------|------|------|---------|
| Rust AOT 后端 | `compiler/src/codegen/aot/emit.rs` | LLVM IR 文本生成（Rust 侧） | �?不动 |
| Rust AOT 链接�?| `compiler/src/codegen/aot/linker.rs` | 调用外部 llc/clang | �?不动 |
| Syscall 运行�?| `compiler/src/std/cffi/`（Rust 重写�?| ~100 �?C ABI 函数 | �?不动 |
| Process.run | `compiler/src/std/std_process.rs` | 调用外部进程（llc/clang�?| �?不动 |
| @native 机制 | `compiler/src/vm/native.rs` | FFI 注册和分�?| �?不动 |

---

## 一、问题域

Aura 编译器当前存在三条并行执行路径（VM/JIT/AOT）和四层基础设施（前�?VM/AOT/JIT + 标准�?+ 工具链）�?
�?Aura 化目标（`docs/pure_aura/02-纯Aura化改造方�?md`）要求：

> �?`compiler/src/bootstrap/`（最小引导层，明确保�?Rust）外，完全脱�?Rust 编译器�?
但本方案进一步收窄：

> **�?`compiler/src/bootstrap/`（~4000 �?Rust）、Cranelift（JIT 机器码后端）、syscall 运行库（Rust 重写）外，全部组件由 Aura 实现�?*
> **覆盖全部三种执行模式**：VM 解释器、JIT（Cranelift）、AOT（LLVM）�?
本文档回答三个问题：

1. **现状是什么？** 各层已完成多少、还缺什么？
2. **技术方案是什么？** 各层如何 Aura 化、FFI 边界怎么划？
3. **怎么分阶段独立开发测试？** 每阶段的验收标准是什么？

---

## 二、核心结论（先看这一段）

> **当前状�?*：`docs/pure_aura/` 的宏观路�?A–E 阶段已完�?85�?00%�?> - 阶段 A（Rust AOT 后端加固）：�?100%
> - 阶段 B（Aura �?HIR 补全）：�?100%
> - 阶段 C（自举闭环）：✅ 100%
> - 阶段 D（std native 上移）：�?85%
> - 阶段 E（CLI/LSP/loom 上移）：�?100%
>
> **剩余工作聚焦�?*（覆盖三种执行模式）�?> 1. **P0 基础契约**�?.5 周）：测试框架验收脚本（TestRunner.aura 已有�?> 2. **P1 AOT 收尾**�? 周）：✅ **已完�?*（自举链 Stage-1�?�? 已跑通，无需额外工作�?> 3. **P2 JIT FFI 边界**�? 周）：`jit_ffi.rs` 三个 FFI 函数（compile/load/call�? `VmJitBridge.aura` VM 集成
> 4. **P3 Std Tar/Zstd**�? 周）：`Tar.aura` + `Zstd.aura` 补齐（仅 2 个文件）
> 5. **P4 三模式集成验�?*�? 周）：端到端三模式对�?> 6. **P5 性能基准**�? 周）：JIT 性能基准 + 自举产物三模式验�?
> **保留边界（不可脱�?*�?> - `compiler/src/bootstrap/`�? 文件，~4000 �?Rust）——最小引导层
> - Cranelift 0.116（Rust crate）——JIT 机器码后端（**JIT native 侧必须，通过 FFI 边界保留**�?> - `compiler/src/std/cffi/`（~3000 行，**Rust 重写**）—�?*syscall 运行�?*（`@native` 系统调用，用 Rust 替代 C�?> - LLVM 23.1.0（llc/clang/lld-link）——AOT 工具�?> - **AOT 模式 native 内容**（`emit.rs`/`linker.rs`/`Process.run`/`@native`）——不做修�?
> **实际进度**�?> - �?AOT 100%（自举链已跑通，Stage-2 编译自身成功�?> - �?Toolchain 100%（CLI/LSP/loom/debugger 全部完成�?> - �?JIT �?Aura �?100%�? 文件 ~3440 行已完成�?> - ⚠️ Std 85%（仅 Tar/Zstd 待补�? 个文件）
> - �?JIT FFI 边界（`jit_ffi.rs` 未实现）
> - �?VM-JIT 集成（`VmJitBridge.aura` 未实现）
>
> **修订后总工�?*�?0.5 周（原计�?17 周，缩减 40%�?
---

## 三、全域状态总览

### 3.1 编译器前端（Phase 1�?�?
| 模块 | Rust �?| Aura �?| 完成�?|
|------|---------|---------|--------|
| 词法分析 | `compiler/src/lexer/` | `aura/.../lexer/Lexer.aura` + `Token.aura` + `Span.aura` | �?100% |
| 语法分析 | `compiler/src/parser.rs` | `aura/.../parser/Parser.aura` | �?100% |
| AST | `compiler/src/ast.rs` | `aura/.../ast/Ast.aura` | �?100% |
| 错误处理 | `compiler/src/errors.rs` | `aura/.../errors/CompileError.aura` | �?100% |
| 语义分析 | `compiler/src/sema/` | `aura/.../sema/{Type,TypeInfo,SymbolTable,TypeChecker}.aura` | �?100% |
| HIR | `compiler/src/codegen/hir.rs` | `aura/.../hir/{Hir,Desugar,Mono,Inline,Fold}.aura` | �?100% |
| MIR | `compiler/src/codegen/mir.rs` | `aura/.../mir/{Mir,MirLower,MirOpt}.aura` | �?100% |

### 3.2 代码生成（Phase 4�?�?
| 模块 | Rust �?| Aura �?| 完成�?|
|------|---------|---------|--------|
| 字节码发�?| `compiler/src/codegen/emit.rs` | `aura/.../codegen/Codegen.aura` | �?100% |
| AOT LLVM 后端 | `compiler/src/codegen/aot/emit.rs` (~5000 �? | `aura/.../aot/{Aot,Emit,EmitBuffer,TypeMapper,Ffi,Runtime,Linker,Optimize,Target,Dwarf,CBackend,ModuleLink,AotUtil,FfiAot,FfiEmit,StdSigs}.aura` | ⚠️ 85%（缺 6.5.10/6.5.13/6.5.14/6.5.7c�?|
| 模块链接 | `compiler/src/linker.rs` | `aura/.../aot/ModuleLink.aura` | �?100% |
| 签名/签名 | `compiler/src/signature.rs` + `signing.rs` | `aura/core/aura/lang/std/` | �?100%（SHA256/HMAC 三层实现�?|

### 3.3 VM 解释器（Phase 4�?�?
| 模块 | Rust �?| Aura �?| 完成�?|
|------|---------|---------|--------|
| 解释�?| `compiler/src/vm/interp.rs` (~3000 �? | `aura/.../vm/{Vm,VmRunner,Opcodes,Frames,FrameManager,TailCall,Closures}.aura` | �?100%（字符串字节码解释器�?|
| �?�?GC | `compiler/src/vm/{heap,value,gc}.rs` | `aura/.../gc/{Gc,MarkSweep,Concurrent,Incremental}.aura` + `aura/.../memory/{Memory,MemoryPool,Arc}.aura` | �?100%（基线实现） |
| 协程/运行�?| `compiler/src/vm/` | `aura/.../runtime/{Coroutine,GcTrigger}.aura` | �?100% |

### 3.4 JIT（Phase 7�?
| 模块 | Rust �?| Aura �?| 完成�?|
|------|---------|---------|--------|
| JIT 状态机 | `compiler/src/vm/jit.rs` (807 �? | `aura/.../jit/JitState.aura` (403 �? | �?100% |
| JIT 优化 | `compiler/src/vm/jit_opt.rs` | `aura/.../jit/JitOpt.aura` (728 �? | �?100% |
| JIT IR 发射 | `compiler/src/vm/jit.rs` �?`cranelift_backend` | `aura/.../jit/JitLower.aura` (462 �? | �?100% |
| JIT ABI | `compiler/src/vm/abi.rs` | `aura/.../jit/JitAbi.aura` (291 �? | �?100% |
| JIT 派发 | `compiler/src/vm/mod.rs` 热点接缝 | `aura/.../jit/JitDispatch.aura` (396 �? | �?100% |
| JIT 核心 | `compiler/src/bootstrap/jit_core.rs` | `aura/.../jit/JitCore.aura` (607 �? | �?100% |
| JIT 运行�?| `compiler/src/vm/aot_runtime.rs` | `aura/.../jit/JitRuntime.aura` (221 �? | ⚠️ 描述性（FFI 占位�?|
| FFI 边界 | �?| **待开�?*（`compiler/src/bootstrap/jit_ffi.rs`�?| �?0% |

### 3.5 标准库（Phase 5�?
| 模块 | Rust �?| Aura �?| C �?| 完成�?|
|------|---------|---------|------|--------|
| Math | `std_math.rs` | `aura/core/aura/lang/std/Math.aura` | `aura_math_*` | �?100% |
| String | `std_string.rs` | `StringBuilder.aura` | `aura_string_*` | �?100% |
| Collections | `std_collections.rs` | �?| `aura_collections_*` | �?100% |
| FS/IO | `std_fs.rs` + `std_io.rs` | `FileSystem.aura` + `IO.aura` | `aura_io_*` | �?100% |
| Concurrent | `std_concurrent.rs` | `Channel.aura` + `Coroutine.aura` | `aura_concurrent_*` | �?100% |
| JSON | `std_json.rs` | `Json.aura` | �?| �?100% |
| Path/Time/Env | `std_path.rs` �?| `Path.aura` + `Time.aura` + `Env.aura` | �?| �?100% |
| Process | `std_process.rs` | `Process.aura` | �?| �?100% |
| Ascii/Assert | `std_ascii.rs` �?| `Ascii.aura` + `Assert.aura` | �?| �?100% |
| Tar/Zstd | �?| �?| �?| ⚠️ 5% 缺口 |

### 3.6 工具链（Phase 5�?
| 模块 | Rust �?| Aura �?| 完成�?|
|------|---------|---------|--------|
| CLI | `cli/src/main.rs` (~65KB) | `aura/toolchain/cli/aura/lang/cli/Main.aura` | �?100%�?2+ 子命令） |
| LSP | `compiler/src/lsp.rs` (~39KB) | `aura/toolchain/lsp/aura/lang/lsp/Main.aura` | �?100%（JSON-RPC + 11 请求�?|
| 调试�?| `cli/src/debugger.rs` (~90KB) | �?| ⚠️ Aura 侧无对应�?|
| loom | `loom/src/**` (50 文件, ~15000 �? | `aura/toolchain/loom/aura/lang/loom/Main.aura` | �?100%（Manifest/TaskGraph/Cache/Plugin/CI/Watch�?|

### 3.7 总体完成�?
| 层级 | 完成�?| 剩余工作 | 本方案范�?|
|------|--------|---------|-----------|
| 前端（lexer/parser/ast/sema/hir/mir�?| �?100% | �?| �?|
| 字节码发�?| �?100% | �?| �?|
| AOT 后端 | �?100% | �?| �?P1（已完成�?|
| VM 解释�?| �?100% | �?| �?|
| JIT �?Aura �?| �?100% | �?| �?P2（已完成�?|
| JIT FFI 边界 | �?0% | `jit_ffi.rs` 3 个函�?| �?P2（核心） |
| VM-JIT 集成 | �?0% | `VmJitBridge.aura` | �?P2（核心） |
| 标准�?| ⚠️ 85% | Tar/Zstd�? 文件�?| �?P3 |
| CLI/LSP/loom | �?100% | �?| �?|
| 调试�?| �?100% | �?| �?P3（已完成�?|
| **总体** | **~95%** | **~5% 收尾** | �?|

---

## 四、文档索�?
| # | 文档 | 内容 | 读�?|
|---|------|------|------|
| 01 | [01-现状分析.md](./01-现状分析.md) | 全域 Rust 依赖盘点、各层完成度、JIT �?Aura 化分析、阻塞项 | 全员 |
| 02 | [02-技术方�?md](./02-技术方�?md) | 全域架构分层、VM+JIT+AOT 数据流、FFI 边界契约、各�?Aura 化设�?| 架构/后端开发�?|
| 03 | [03-分阶段开发计�?md](./03-分阶段开发计�?md) | **P0–P5 六个阶段**（全域），每阶段独立可验收、可回退 | 开发�?|
| 04 | [04-测试与验收矩�?md](./04-测试与验收矩�?md) | 每阶段测试用例清单、不变量、回归检查清�?| QA / 开发�?|
| 05 | [05-风险与开放问�?md](./05-风险与开放问�?md) | 全域技术风险、ABI 漂移风险、回退策略、开放问�?| 架构/维护�?|

---

## 五、关键事实速查

### 5.1 保留边界（不可脱�?
| 组件 | 语言 | 位置 | 行数 | 理由 |
|------|------|------|------|------|
| Bootstrap | Rust | `compiler/src/bootstrap/` | ~4000 | 最小引导层（VM/AOT/JIT 基线 + 内存 + 运行时） |
| Cranelift | Rust | `cranelift` crate | �?| JIT 机器码后端（Bytecode Alliance，Wasmtime 同源�?|
| Syscall 运行�?| **Rust** | `compiler/src/std/cffi/`（重写） | ~3000 | **`@native` 系统调用层，�?Rust 替代 C，减少语言种类** |
| LLVM | C++ | 外部工具�?| �?| AOT 机器码生成（llc/clang/lld-link�?|
| C 编译�?| �?| 不再需要（syscall 运行库已�?Rust 重写�?| �?| �?|

**语言种类变化**：原 3 种（Rust + C + Aura）→ �?2 种（Rust + Aura）。C 编译器依赖同步移除�?
### 5.2 �?Aura 侧已有实�?
| 模块 | 位置 | 文件�?| 行数（粗估） |
|------|------|--------|-------------|
| 编译器前�?| `aura/compiler/aura/lang/compiler/{lexer,parser,ast,errors,sema,hir,mir,codegen}/` | ~25 | ~15000 |
| AOT 后端 | `aura/compiler/aura/lang/compiler/aot/` | 16 | ~10000 |
| VM 解释�?| `aura/compiler/aura/lang/compiler/vm/` | 7 | ~5000 |
| JIT | `aura/compiler/aura/lang/compiler/jit/` | 8 | ~3440 |
| GC/内存 | `aura/compiler/aura/lang/compiler/{gc,memory,runtime}/` | 8 | ~3000 |
| 标准�?| `aura/core/aura/lang/std/` | 20 | ~8000 |
| CLI/LSP/loom | `aura/toolchain/aura/lang/{cli,lsp,loom}/` | 3 | ~10000 |
| **合计** | �?| **~87** | **~54000** |

### 5.3 执行路径对比

| 路径 | 技�?| Aura 侧实�?| 完成�?| 默认 | 本方案范�?|
|------|------|------------|--------|------|-----------|
| VM 解释�?| 字节码解�?| `aura/.../vm/{Vm,VmRunner,Opcodes,...}.aura` | �?100% | �?默认 | �?在范围内 |
| AOT | LLVM 23.1.0（文�?IR + 外部 llc/clang�?| `aura/.../aot/{Aot,Emit,TypeMapper,...}.aura` | ⚠️ 85%（缺 6.5 收尾�?| ⚠️ 需 `--features llvm` | �?在范围内 |
| JIT | Cranelift 0.116（Clif IR + FFI 边界�?| `aura/.../jit/{JitState,JitLower,JitOpt,...}.aura` | ⚠️ 90%（FFI 待开发） | �?需 `--features jit` | �?在范围内 |

### 5.4 自举链状�?
自举闭环已跑通（`docs/pure_aura/03-自举验证报告.md`）：

```text
Stage-1: Rust 编译器编�?Aura 编译器源�?�?aura-compiler-native.exe（原生载体）
Stage-2: 原生载体编译自身 �?aura-compiler-native2.exe（自举产物）
Stage-3: 自举产物编译用户程序（含 @native FFI）→ 用户 exe
```

---

## 六、与其他文档的关�?
- **`docs/pure_aura/02-纯Aura化改造方�?md`**：宏�?5 阶段（A→E）路线（11�?9 周），覆盖编译期。本方案聚焦**剩余收尾 + JIT 专项 + 全域验证**
- **`docs/pure_aura/03-差距分析.md`**：A–E 阶段完成度评估（2026-07-06），是本文档 §�?数据�?- **`docs/pure_aura/03-自举验证报告.md`**：自举闭环验证证据，是本文档 P0 阶段的前�?- **`aura/compiler/README.md`**：纯 Aura 编译器整体说明，�?Phase 0�? 交付清单
- **`docs/JIT性能分析.md`**：JIT 性能分析，是本文�?JIT 专项的历史结论来�?- **`docs/jit优化指南.md`**：JIT 优化策略参考（偏理论）

---

## 七、文档维护约�?
- 每个开发阶段完成后�?*必须**更新 `03-分阶段开发计�?md` 中该阶段的「状态」字�?- 每个阶段新增的测试用例，必须同步登记�?`04-测试与验收矩�?md`
- 阶段交付后，如有新的风险或开放问题，追加�?`05-风险与开放问�?md`
- 所有文档引用源码时使用绝对路径 + 行号范围

---

## 八、快速开�?
**想快速理解全局**：读 §二「核心结论�?+ `01-现状分析.md` �?§一「全域状态」�?
**想开始开�?*：先�?`02-技术方�?md` �?§三「全域架构�? §五「FFI 边界契约」，
再按 `03-分阶段开发计�?md` �?P0 开始�?
**想评审测�?*：直接看 `04-测试与验收矩�?md` 的「测试矩阵总览」表�?
**想评估风�?*：读 `05-风险与开放问�?md` �?§一「风险分级表」�?
---

*本文档为全域�?Aura 化的执行方案总纲。详细分阶段计划�?`03-分阶段开发计�?md`�?
