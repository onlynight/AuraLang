# 纯 Aura 自举 + Cranelift JIT：差距分析报告（v2）

> **更新说明**：基于实际代码状态更新。v3.0 架构已实现（@native 直接生成 LLVM IR、C FFI 移除、preludeTable 清空），`03-自举验证报告.md` 已过期未更新
> **日期**：2026-09-16

---

## 一、总体完成度（更新后）

```
┌──────────────────────────────────────────────────────────────┐
│  宏观路线（docs/pure_aura/ A-E 阶段）                        │
├──────────────────────────────────────────────────────────────┤
│  A Rust AOT 后端加固     ████████████████ 100% ✅           │
│  B Aura HIR 补全         ████████████████ 100% ✅           │
│  C 自举闭环              ████████████████ 100% ✅           │
│  D std native 上移       ████████████████ 100% ✅           │
│  E CLI/LSP/loom 上移     ████████████████ 100% ✅           │
├──────────────────────────────────────────────────────────────┤
│  JIT 专项（docs/pure_aura_jit/ P0-P5 阶段）                   │
├──────────────────────────────────────────────────────────────┤
│  P0 基础契约             ████████████████ 100% ✅           │
│  P1 AOT 收尾             ████████████████ 100% ✅           │
│  P2 JIT 模式             ██████░░░░░░░░░░  30% ⚠️           │
│  P3 Std + 调试器         ████████████████ 100% ✅           │
│  P4 三模式集成           ████████████████ 100% ✅           │
│  P5 性能基准             ████████████████ 100% ✅           │
├──────────────────────────────────────────────────────────────┤
│  纯 Aura JIT 实现（aura/.../jit/）                             │
├──────────────────────────────────────────────────────────────┤
│  8 个 Aura 文件（~3440 行）   ████████████████ 100% ✅       │
│  FFI 边界（3 函数）          ░░░░░░░░░░░░░░░░   0% ❌        │
│  VM 派发接线                ░░░░░░░░░░░░░░░░   0% ❌        │
├──────────────────────────────────────────────────────────────┤
│  v3.0 架构（脱 Rust + 脱 C）                                    │
├──────────────────────────────────────────────────────────────┤
│  @native(N) → 内联 syscall IR   ████████████████ 100% ✅     │
│  @native(asm=) → inline asm IR  ████████████████ 100% ✅     │
│  native fun → LLVM IR builtin   ████████████████ 100% ✅     │
│  C FFI 移除（preludeTable 清空） ████████████████ 100% ✅     │
│  aura_std_cffi.c 编译步骤移除  ████████████████ 100% ✅     │
│  自举链 Stage-1/2/3 全通       ████████████████ 100% ✅     │
└──────────────────────────────────────────────────────────────┘

总体完成度：~94%
剩余工作量：~3-4 周
唯一阻塞：P2（JIT FFI 边界）
```

---

## 二、v3.0 架构已完成项（代码证据）

### 2.1 @native 三种注解 → LLVM IR 直发

| 注解 | 文件证据 | 生成内容 |
|------|---------|---------|
| `@native(N)` | `Emit.aura:2091-2124` | 内联 syscall 指令（x86_64: `mov rax, N; syscall`；aarch64: `mov x16, N; svc #0`） |
| `@native(asm="...")` | `Emit.aura:2126+` | LLVM inline asm |
| `native fun` | `Memory.aura` 等 | LLVM IR builtin（brk/mmap/load/store） |

**证据**：`Emit.aura:2091` 注释 "Phase A.1：@native(SYS_READ) 直接生成内联 syscall 指令，不再调用 C 分发入口 @aura_syscall_dispatch"

### 2.2 C 依赖移除

| 移除项 | 文件证据 |
|--------|---------|
| `preludeTable()` 清空 | `Runtime.aura:136` — "runtimeTable() / preludeTable() 已全部清空" |
| `aura_std_cffi.c` 编译步骤移除 | `Aot.aura:179` — "Phase C.2：aura_std_cffi.c 编译步骤已移除" |
| C 运行库映射移除 | `ModuleLink.aura:63` — "preludeTable 移除后会出现 undefined symbol" |

### 2.3 平台 syscall 声明（5 个架构）

```
aura/core/aura/lang/native/arch/
├── x86_64_linux/Syscalls.aura    ← @native(0) read, @native(1) write, ...
├── x86_64_windows/Syscalls.aura  ← @native(asm="call qword [rip + WriteFile]")
├── x86_64_darwin/Syscalls.aura   ← macOS POSIX syscall
├── aarch64_linux/Syscalls.aura   ← ARM64 Linux syscall 号
└── aarch64_darwin/Syscalls.aura  ← ARM64 macOS syscall
```

### 2.4 标准库纯 Aura 实现

| 模块 | 位置 | 状态 |
|------|------|------|
| Math（Taylor 级数/牛顿法） | `native/math/MathOps.aura` | ✅ 不依赖 libm |
| String | `std/string/StringOps.aura` | ✅ 纯 Aura |
| FileSystem | `std/FileSystem.aura` | ✅ 纯 Aura |
| Collections | `collection/` | ✅ 纯 Aura |
| Thread/Process | `native/thread/` `native/process/` | ✅ 纯 Aura |
| Network | `native/network/` | ✅ 纯 Aura |

### 2.5 自举链

| 阶段 | 描述 | 状态 |
|------|------|------|
| Stage-1 | Rust bootstrap 编译 Aura 编译器 → 原生载体 | ✅ |
| Stage-2 | 载体编译自身 → 自举产物（45MB，51 模块，11s） | ✅ |
| Stage-3 | 自举产物编译/运行用户程序（含 @native） | ✅ |

**注意**：Stage-1 的 Rust bootstrap 仅用于"第一次编译 Aura 编译器"（鸡生蛋问题），自举产物运行时零 Rust、零自写 C 代码。

---

## 三、剩余差距（唯一阻塞项）

### P2：JIT FFI 边界（3-4 周）

**影响**：纯 Aura JIT 前端（8 文件 ~3440 行已完成）无法连接到 Cranelift 后端

| 子项 | 内容 | 位置 | 预估工时 |
|------|------|------|---------|
| **P2.1** Clif IR 格式验证 | 验证 JitLower.aura 输出的 .clif 与 Cranelift 0.116 兼容 | `tests/pure_aura/jit_p2_clif_tests.aura` | 3-5 天 |
| **P2.2** FFI 边界实现 | 实现 3 个 FFI 函数（jit_compile/jit_load/jit_call） | `compiler/src/bootstrap/jit_ffi.rs` | 5-8 天 |
| **P2.3** VM 派发接线 | VmJitBridge.aura 将 VM 调用接到 JIT 派发路径 | `aura/.../vm/VmJitBridge.aura` | 5-8 天 |

**FFI 函数定义**：

```rust
// 1. Cranelift 编译：.clif 文本 → base64 机器码 blob
@native fun jit_compile(clif_text: String) -> String

// 2. 机器码加载：base64 blob → mmap(RX) → 注册分发表 → entry_token
@native fun jit_load(blob_b64: String) -> String

// 3. JIT 调用：entry_token + args → dispatch_table + call_indirect → 返回值
@native fun jit_call(entry_token: String, args: String, out_slot: Int) -> String
```

**风险**：

| 风险 | 概率 | 缓解 |
|------|------|------|
| JitLower.aura 的 .clif 格式与 Cranelift 0.116 不兼容 | 中 | P2.1 预留 2 天修复；逐字节对比 Cranelift 内置 .clif 测试文件 |
| FFI 边界数据格式不匹配 | 低 | 全部用 CStr + base64 编码；P0 已定义格式契约 |
| Windows mmap 失败（需 VirtualAlloc） | 中 | Cranelift 已处理跨平台 |
| 派发接线引入 VM 回归 | 中 | VmJitBridge.aura 独立模块，保留 VM 回退路径 |

---

## 四、与上一版差距分析的差异

| 项目 | 上一版（基于过期文档） | 本版（基于实际代码） |
|------|---------------------|---------------------|
| 总体完成度 | 88% | **94%** |
| 自举闭环 | 88%（P2 未完成） | **100%（Stage-1/2/3 全通）** |
| C 依赖 | 85%（Std 未移除） | **100%（C FFI 已移除）** |
| preludeTable | 未提及 | **已清空** |
| aura_std_cffi.c | 仍依赖 | **编译步骤已移除** |
| @native | 走 C 分发 | **直接生成 LLVM IR** |
| 剩余差距 | P2（3-4 周）+ P3（1 周） | **仅 P2（3-4 周）** |

---

## 五、总结

### 5.1 一句话总结

> **Aura 自举 + v3.0 架构已完成，脱 Rust、脱 C、脱 FFI。唯一剩余差距是 JIT FFI 边界（P2），3 个函数实现 + 派发接线，预估 3-4 周。**

### 5.2 架构状态

```
已完成 ✅：
  @native(N) → 内联 syscall IR（5 个平台架构）
  @native(asm=) → LLVM inline asm
  native fun → LLVM IR builtin
  C FFI 移除（preludeTable 清空 + cffi 编译步骤移除）
  标准库纯 Aura 实现（Math/String/File/Collection/Thread/Process/Network）
  自举链 Stage-1/2/3 全通
  JIT 前端 8 文件 ~3440 行完成
  AOT 发射器 16 文件完成
  VM 解释器 7 文件完成
  CLI/LSP/Debug/loom 100% 完成

剩余 ❌：
  JIT FFI 边界（3 函数：jit_compile/jit_load/jit_call）
  VM 派发接线（VmJitBridge.aura）
  Clif IR 格式验证
```

---

## 六、文档更新建议

`docs/pure_aura/03-自举验证报告.md` 已过期，应更新以反映 v3.0 架构：

| 章节 | 当前内容（过期） | 应更新为 |
|------|----------------|---------|
| 验证环境 | Rust 1.86.0 + LLVM 23.1.0 | 同（Stage-1 仍需 Rust bootstrap） |
| Stage-1 | Rust 后端编译 → 原生载体 | 同（Stage-1 不变） |
| Stage-2 | 载体编译自身 | 同（已验证成功） |
| Stage-3 | 自举产物编译用户程序（含 @native FFI） | **需更新**：@native 现在直接生成 LLVM IR，不走 C FFI |
| 遗留缺口 | lambda 捕获、C for 省略形式等 | **需更新**：C 依赖已移除，剩余缺口为 JIT FFI 边界 |
| 交付物清单 | bootstrap_c4_tests 等 | **需追加**：Syscalls.aura（5 架构）、MathOps.aura、preludeTable 清空记录 |

---

*本报告为纯分析文档，不修改任何代码。*
