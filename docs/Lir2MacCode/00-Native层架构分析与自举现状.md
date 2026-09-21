# Native 层架构分析与自举现状

> **日期**: 2026-07-12
> **范围**: `aura/core/aura/lang/native/` + `aura/compiler/.../aot/Emit.aura` + `compiler/src/codegen/aot/emit.rs`
> **结论**: `*.ll` 文件已完全消除，`@native` 体系自举闭环已打通，无需重写

---

## 一、关于 `*.ll` 文件

**整个仓库中不存在任何 `.ll` 文件**（`**/*.ll` 全局搜索零结果）。历史上的 LLVM IR 文件依赖已被完全消除。

当前 `@native` 系统**在编译期动态生成 LLVM IR 文本**，不存在预先生成的 `.ll` 片段。

---

## 二、`@native` 注解的完整映射链

### 2.1 三条路径总览

| 注解形式 | 编译器行为 | 目标指令 |
|---------|-----------|---------|
| `@native(N)`（syscall 号） | 生成 inline asm `syscall` 指令 | Linux: `mov rax,N; syscall` / aarch64: `mov x16,N; svc #0` / Windows: CRT 映射 |
| `@native(asm="...")`（内联汇编） | 已知指令走专用路径，其余走通用 LLVM inline asm | `fence` → `fence seq_cst` / `atomic` → `atomicrmw` / `arcInc` → `atomicrmw` / 其余 → inline asm |
| `@native`（无参数，内置） | 编译器内置函数 | `Memory.read` → `inttoptr+load` / `Memory.alloc` → `call malloc` / `Memory.copy` → `llvm.memcpy` |

### 2.2 关键代码位置

| 位置 | 文件 | 行数 |
|------|------|------|
| Rust 后端 IR 发射 | `compiler/src/codegen/aot/emit.rs` (`emit_native_wrapper`) | 548–818 |
| Aura 自举后端 IR 发射 | `aura/compiler/.../aot/Emit.aura` (`emitNativeWrapper`) | 2586–2857 |
| 纯 Aura FFI 发射器 | `aura/compiler/.../aot/FfiEmit.aura` | 269（完全纯 Aura） |

### 2.3 两侧发射器对齐状态

Rust 与 Aura 两侧发射器在语义上已对齐（注释中明确标注"与 Rust 侧 `emit_native_wrapper` 对齐"）。差异点：

| 特性 | Rust 后端 | Aura 自举后端 |
|------|----------|-------------|
| syscall | inline asm `syscall` | inline asm `syscall`（同） |
| fence | `call @aura_cpu_mem_fence()` | `fence seq_cst`（更优：LLVM 原生指令） |
| atomic | `call @aura_cpu_atomic_add()` | `atomicrmw add/sub`（更优：LLVM 原生指令） |
| ARC refcount | `lock inc/dec` inline asm | `atomicrmw add/sub seq_cst`（更优：平台无关） |
| Memory.alloc | `call @malloc()` | `call @malloc()`（同） |
| Memory.read | `inttoptr + load` | `inttoptr + load`（同） |

---

## 三、自举成熟度现状

### 3.1 已完成（AOT 路径）

```
Phase S1  原生桥接     ✅  Syscalls/Memory/Cpu/Allocator/Console 全部完成
Phase S2  自举编译     ✅  Rust 编译器编译 Main.aura → LLVM IR → llc → clang → 原生载体
Phase S3  自举运行     ✅  原生载体再次编译自身 → n2.exe，行为一致性验证通过
Phase S4  脱离 Rust    ✅（AOT 路径）  Rust 仅作一次性引导（冻结二进制），无需 cargo
```

**实际验证**: 65 代连续自举产物，证明 Aura AOT 后端可编译编译器自身。

### 3.2 已知缺口

| 缺口 | 严重性 | 说明 |
|------|--------|------|
| Aura VM 是占位桩 | 🔴 高 | `Vm.aura::interpret()` 返回 `null`；自举走 AOT 路径，不经过 VM |
| JIT FFI 边界 | 🟡 中 | `jit_ffi.rs`（394 行 Rust）封装 Cranelift，保留为 native 库 |
| `aura_syscalls.c` | 🟢 低 | 1055 行 C 文件，**大部分功能已被编译器内联 asm 替代**；仅线程原语和异常桥仍需 C 实现 |

### 3.3 Rust 依赖移除可行性评估

> 详见 `docs/remove_rust/00-自举编译器Rust依赖移除可行性评估.md`

**结论**: 不能完全移除 Rust 代码依赖。原因：

1. **Aura VM 是空壳**: `Vm.aura::interpret()` 返回 `null`，`VmRunner.aura` 是仅支持 ~25 个操作码的文字型简化 VM
2. **自举走 AOT 路径**: 自举验证成功证明了 "Aura AOT 后端能生成 LLVM IR → 经 llc/clang 编译为原生 exe"，但这只覆盖了"编译"能力，不覆盖"运行"能力
3. **Rust 编译器仍是日常开发基础设施**: `.auc` 执行、完整 CLI 命令、文档生成、LSP、调试器、基准测试都依赖 Rust 编译器

---

## 四、是否需要重写

### 4.1 直接回答

> **不需要重写 `.ll` 文件——它们已经不存在了。**

当前 `@native` 体系通过编译期动态生成 LLVM IR 文本，已经实现了 AOT 路径的自举闭环。

### 4.2 剩余 Rust 依赖的迁移路径

| 优先级 | 任务 | 估计工作量 |
|--------|------|-----------|
| P0 | 实现完整 Aura VM（替换 `Vm.aura` 占位，支持二进制 `.auc`、完整指令集、对象/类、ARC、GC） | ~2000–3000 行 Aura |
| P1 | `.auc` 二进制加载器 | ~500 行 Aura |
| P2 | Aura docgen（替代 `docgen.rs`） | ~2000 行 Aura |
| P3 | 完善 Aura LSP / Debugger | ~2000 行 Aura |
| P4 | VM 执行路径 JIT 桥接 | ~1000 行 Aura |
| P5 | 移除 Rust bootstrap/ 目录（VM 可用后） | 低 |

### 4.3 外部原生依赖（不可避免）

```
LLVM 23.1.0 (llc/clang)     ← AOT 编译必需（文本 IR → 机器码）
Cranelift (JIT 后端)        ← 决策保留为 native 库
libc / Windows CRT          ← 系统调用必需
aura_syscalls.c (1055行)    ← C FFI 桥接层（大部分已废弃，见下一节分析）
```
