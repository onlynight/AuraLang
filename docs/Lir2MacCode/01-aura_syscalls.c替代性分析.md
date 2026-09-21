# aura_syscalls.c 替代性分析

> **日期**: 2026-07-12
> **对象**: `aura/runtime/cffi/aura_syscalls.c`（1055 行 C 代码）
> **目标**: 判断是否可用新编译后端（Photon / Aura AOT 内联 IR）生成的机器码完全替代
> **结论**: 该文件大部分已是死代码，残留依赖可分阶段消除

---

## 一、文件结构总览

`aura_syscalls.c` 包含 **8 个功能组**，每组的状态独立分析。

| 功能组 | 行范围 | 行数 | 是否被新后端引用 | 替代方案 |
|-------|--------|------|----------------|---------|
| A. syscall 分发器 + 包装函数 | 51–237 | 187 | ❌ 否 | 编译器已直接生成 inline asm `syscall` |
| B. Memory alloc/free | 242–264 | 23 | ❌ 否 | 编译器直接调用 libc `malloc`/`free` |
| C. CPU 内联汇编（rdtsc/fence/atomic） | 270–327 | 58 | ⚠️ 部分 | Rust 后端仍引用；Aura 后端已改用 LLVM 指令 |
| D. 线程原语（pthread / Win32） | 333–458 | 126 | ⚠️ 仅 Rust 后端 | Emit.aura 仍引用 `aura_thread_*` |
| E. 同步原语（Mutex/CondVar/RwLock/Barrier/TLS） | 376–593, 631–852 | ~300 | ❌ 否 | 仅 Rust 后端 runtime.rs 声明，Aura 后端未引用 |
| F. 原子操作（C11 / Interlocked） | 498–542, 744–779 | ~50 | ❌ 否 | Aura 后端已改用 `atomicrmw` |
| G. SHA256 密码学哈希 | 903–1011 | 109 | ❌ 否 | 可纯 Aura 重实现 |
| H. setjmp/longjmp 异常桥 | 1017–1049 | 33 | ⚠️ 仅 Rust 后端 | Aura AOT 使用 handler 栈，不依赖 setjmp |

---

## 二、逐组详细分析

### 2.1 组 A: syscall 分发器（行 51–237）— 🔴 死代码

**当前状态**: 编译器已完全替代。

```
旧路径: @native(N) → call @aura_syscall_dispatch(N, ...) → C switch → 具体 syscall
新路径: @native(N) → inline asm "syscall"（编译器直接生成 IR）
```

**证据**:
- `Emit.aura` 第 2636–2669 行：直接生成 `mov rax, N; syscall` inline asm
- `emit.rs` 第 619–694 行：同上，使用命名寄存器约束

**结论**: **完全可删除**。`aura_syscall_dispatch` 和所有 `aura_syscall_*` 包装函数在两个后端中均无调用点。

### 2.2 组 B: Memory alloc/free（行 242–264）— 🔴 死代码

**当前状态**: 编译器已完全替代。

```
旧路径: @native (builtin) → call @aura_memory_alloc/free → C: mmap/munmap
新路径: @native (builtin) → call @malloc/@free（libc）
```

**证据**:
- `Emit.aura` 第 2779–2786 行：直接生成 `call @malloc` / `call @free`
- `emit.rs` 第 775–780 行：同上

**结论**: **完全可删除**。

### 2.3 组 C: CPU 内联汇编（行 270–327）— 🟡 部分需要

| 函数 | Rust 后端 | Aura AOT 后端 | 替代方案 |
|------|----------|-------------|---------|
| `aura_cpu_rdtsc` | ✅ 引用 | ❌ 不引用（走通用 inline asm） | 可保留（Rust 路径需要） |
| `aura_cpu_mem_fence` | ✅ 引用 | ❌ 不引用（走 LLVM `fence seq_cst`） | 可保留（Rust 路径需要） |
| `aura_cpu_atomic_add` | ✅ 引用 | ❌ 不引用（走 LLVM `atomicrmw add`） | 可保留（Rust 路径需要） |

**结论**: 仅供 Rust 后端 AOT 路径使用。如需完全脱 Rust，可在 Rust 后端中也改为 LLVM 原生指令。

### 2.4 组 D: 线程原语（行 333–458）— 🟡 可完全替代

**当前状态**: `Emit.aura` 第 2796–2812 行仍引用 `aura_thread_create/join/sleep/id`。

```
ThreadOps.create → call @aura_thread_create(i64 %arg.0, i64 %arg.1)
ThreadOps.join   → call @aura_thread_join(i64 %arg.0)
ThreadOps.sleep  → call @aura_thread_sleep(i64 %arg.0)
ThreadOps.id     → call @aura_thread_id()
```

**替代方案**: 不使用 LLVM IR `declare`，不使用 libc/pthread，**完全通过 syscall + inline asm 实现**。

| 原语 | 替代方案 | 难度 |
|------|---------|------|
| `join` | `@native(SYS_WAIT4)` → `wait4()` syscall | 🟢 低 |
| `sleepMs` | `@native(SYS_NANOSLEEP)` + Memory | 🟢 低 |
| `currentId` | `@native(SYS_GETTID)` → `gettid()` syscall | 🟢 低 |
| `cores` | `@native(SYS_SCHED_GETAFFINITY)` + bit count | 🟢 低 |
| `create` | `@native(clone_trampoline)` → `clone()` + arch_prctl + trampoline | 🟡 中 |

**结论**: **可以完全替代**，不需要 LLVM IR `declare`，不需要 C 包装层。详细方案见 `02-线程原语纯Aura实现方案.md`。

### 2.5 组 E: 同步原语（Mutex/CondVar/RwLock/Barrier/TLS）— 🔴 死代码

**当前状态**: 仅 Rust 后端 `runtime.rs` 中声明，Aura AOT 后端未引用。

**证据**:
- `aura/core/aura/lang/native/` 目录下无任何文件声明这些函数为 `@native`
- `Emit.aura` 仅引用 `ThreadOps`（`aura_thread_*`），不引用 Mutex/CondVar 等

**结论**: **完全可删除**。如果 Aura 侧需要 Mutex/CondVar，应使用纯 Aura 实现（基于 `@native(asm="lock xaddq")` 原子指令自旋），不需要 C 层支持。

### 2.6 组 F: 原子操作（C11 / Interlocked）— 🔴 死代码

**当前状态**: 仅 Rust 后端 runtime.rs 声明。

**替代方案**: 编译器已直接生成 LLVM `atomicrmw` 和 `cmpxchg` 指令（`Emit.aura` 第 2681–2694 行）。

**结论**: **完全可删除**。

### 2.7 组 G: SHA256（行 903–1011）— 🟡 可重实现

**当前状态**: 仅 Rust 后端 stdlib 引用。Aura 侧 `std/Encoding.aura` 和 `std/Zstd.aura` 未引用 `aura_sha256`。

**替代方案**: 纯 Aura 重实现 SHA256（约 200 行 Aura 代码）。SHA256 是纯位运算 + 常量表，完全可以在 Aura 中实现。

**结论**: 可删除，但需纯 Aura 重实现 SHA256。如果短期内不做，可暂时保留此段 C 代码。

### 2.8 组 H: setjmp/longjmp 异常桥（行 1017–1049）— 🟡 仅 Rust 后端

**当前状态**: 仅 Rust 后端 `emit.rs` 第 508 行引用：

```
s.push_str("@aura_exception_value = external global i8*\n");
```

**替代方案**: Aura AOT 后端使用 handler 栈（帧号 + 目标 IP + 栈高 + 落点槽）实现异常处理，不依赖 setjmp/longjmp。

**结论**: 仅供 Rust 后端使用。如果完全脱 Rust，可删除。

---

## 三、替代可行性总结

### 3.1 完全可删除的组（无需替代）

| 组 | 行数 | 说明 |
|----|------|------|
| A. syscall 分发器 | 187 | 编译器已直接生成 inline asm `syscall` |
| B. Memory alloc/free | 23 | 编译器直接调用 libc `malloc`/`free` |
| E. 同步原语 | ~300 | 仅 Rust runtime.rs 声明，Aura 未引用 |
| F. 原子操作 | ~50 | 编译器已生成 `atomicrmw` / `cmpxchg` |

**总计: ~560 行 C 代码已是死代码，可直接删除。**

### 3.2 可替换但仍需保留的组

| 组 | 行数 | 替换方案 | 预计工作量 |
|----|------|---------|-----------|
| C. CPU 内联汇编 | 58 | Rust 后端改为 LLVM `fence`/`atomicrmw` 指令 | 低（Rust 侧改动） |
| D. 线程原语 | 126 | Emit.aura 改为 `declare` + 直接调用 pthread/Win32 | 低（Aura 侧改动） |
| G. SHA256 | 109 | 纯 Aura 重实现 SHA256 | 中（~200 行 Aura） |
| H. 异常桥 | 33 | 删除（Aura AOT 已用 handler 栈） | 低（仅 Rust 后端需要） |

### 3.3 替代后的最终状态

```
aura_syscalls.c  → 0 行（完全消除）
  ├─ 组 A/B/E/F: 直接删除（死代码）
  ├─ 组 C:    Rust 后端改为 LLVM 原生指令
  ├─ 组 D:    Emit.aura 改为 LLVM declare + 直接调用
  ├─ 组 G:    纯 Aura 重实现
  └─ 组 H:    删除（仅 Rust 后端需要，Aura AOT 已替代）
```

---

## 四、分阶段实施计划

### Phase 1: 清理死代码（立即，无需改动编译器）

**任务**: 删除组 A/B/E/F 的全部代码。

**前置**: 确认 Rust 后端和 Aura AOT 后端均无调用点。

| 文件 | 行 | 操作 |
|------|-----|------|
| `aura_syscalls.c` | 51–237 | 删除组 A（syscall 分发器） |
| `aura_syscalls.c` | 242–264 | 删除组 B（Memory alloc/free） |
| `aura_syscalls.c` | 376–593, 631–852 | 删除组 E（同步原语） |
| `aura_syscalls.c` | 498–542, 744–779 | 删除组 F（原子操作） |

**预期结果**: `aura_syscalls.c` 从 1055 行降至约 ~230 行。

**验证**:
- Rust 后端: `cargo build` 通过
- Aura AOT 自举: `aura build Main.aura --aot` 通过
- Phase C.2 测试: `aura run tests/pure_aura/native_c2_aot_tests.aura` 通过

### Phase 2: 替换线程原语（改动 Emit.aura）

**任务**: 将 `Emit.aura` 第 2796–2812 行的 `call @aura_thread_create` 等改为 LLVM IR `declare` + 直接调用 pthread/Win32 API。

**Rust 后端**: 类似改动 `emit.rs` 第 700–709 行（可选，仅当完全脱 Rust 时需要）。

### Phase 3: 纯 Aura 重实现 SHA256（可选）

**任务**: 在 `aura/core/aura/lang/std/Encoding.aura` 或新文件中用纯 Aura 实现 SHA256。

**预估**: ~200 行 Aura 代码。

### Phase 4: 删除剩余 C 代码（完全脱 Rust 时）

**任务**: 删除组 C/H 的 C 实现。

**前置**: 完成完全脱 Rust（Phase S5）。

---

## 五、风险与注意事项

### 5.1 平台差异

线程原语替换需注意平台差异：

| 平台 | 线程 API | 参数布局 | 需要适配 |
|------|---------|---------|---------|
| Linux x86_64 | `pthread_create` | `(pthread_t*, attr*, start_routine, arg*)` | 需处理函数指针类型 |
| Windows x86_64 | `CreateThread` | `(SECURITY_ATTRIBUTES*, SIZE_T, LPTHREAD_START_ROUTINE, LPVOID, DWORD, LPDWORD)` | 需处理函数指针类型 |
| macOS x86_64 | `pthread_create` | 同 Linux | 同 Linux |
| Linux aarch64 | `pthread_create` | 同 Linux | 同 Linux |

**建议**: 保留平台分支逻辑（与 `Emit.aura` 中已有的 `winCrtWrapper` 模式一致）。

### 5.2 函数指针传递

`ThreadOps.create(fn_id, arg)` 当前通过 C 分派表 `__aura_fn_table[]` 传递函数索引。如果改为直接调用 pthread API，需要将函数索引转换为函数指针：

```
Emit.aura 当前：
  call i64 @aura_thread_create(i64 %arg.0, i64 %arg.1)

改为：
  %fn = load i8*, i8** getelementptr([N x i8*], [N x i8*]* @__aura_fn_table, i32 0, i32 %fn_id)
  call i64 @pthread_create(i64 %tid, i64 0, i8* %fn_trampoline, i64 %arg)
```

**复杂度**: 中等。需要 `inttoptr` + `load` + `call` 三步。

### 5.3 自举一致性

所有改动必须同时满足：
1. Rust 后端编译通过
2. Aura AOT 自举编译通过
3. 行为一致性验证通过

**建议**: 每个 Phase 完成后运行完整的自举验证脚本。

---

## 六、最终结论

| 指标 | 值 |
|------|-----|
| 当前 C 代码行数 | 1055 |
| 已确认为死代码 | ~560 行（53%） |
| 可完全替代（syscall+asm） | ~226 行（21%） |
| 最终可消除行数 | ~786 行（75%） |
| 最终残留行数 | ~270 行（SHA256 + CPU 内联汇编 + 异常桥） |

> **结论**: `aura_syscalls.c` 可以作为新编译后端的完全替代品。通过分四阶段实施，可以完全消除对 C FFI 桥接层的依赖，使编译器在 LLVM IR 生成、编译、自举全链路中不再依赖任何外部 C 代码。
