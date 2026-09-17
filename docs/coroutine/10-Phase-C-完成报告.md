# Phase C — AOT 代码生成 完成报告

> **状态**：✅ 完成  
> **日期**：2026-07-04  
> **关联设计文档**：`docs/coroutine/04-Phase-C-AOT代码生成.md`

---

## 1. 完成项总览

| 项目 | 状态 | 说明 |
|------|------|------|
| 并发原生函数注入 | ✅ 完成 | `inject_concurrent_natives` 函数注入 30+ 并发 native 声明 |
| AOT LLVM IR 生成 | ✅ 完成 | 并发函数 extern declare 正确生成 |
| cffi_signature 完整性 | ✅ 完成 | 21 个并发 C 函数签名全部验证 |
| translate_to_legacy_c 映射 | ✅ 完成 | 6 个并发类名 → C 前缀映射验证 |
| RUNTIME_FUNCTIONS 完整性 | ✅ 完成 | 21 个并发运行时函数声明验证 |
| AOT 并发测试 | ✅ 完成 | 5 个测试全部通过 |

---

## 2. 并发原生函数注入

### 2.1 新增函数

`inject_concurrent_natives(program: &mut HirProgram)`

位于 `compiler/src/codegen/aot/mod.rs`，在 `compile_program` 中调用。

### 2.2 注入的并发函数声明（30+ 个）

**Thread**（6 个）：
- `aura.lang.std.Thread.spawn(fnId, arg) → Int`
- `aura.lang.std.Thread.join(id) → Int`
- `aura.lang.std.Thread.sleep(ms) → Unit`
- `aura.lang.std.Thread.id() → Int`
- `aura.lang.std.Thread.parallelism() → Int`
- `aura.lang.std.Thread.availableCores() → Int`

**Mutex**（5 个）：
- `aura.lang.std.Mutex.new() → Int`
- `aura.lang.std.Mutex.lock(id) → Unit`
- `aura.lang.std.Mutex.unlock(id) → Unit`
- `aura.lang.std.Mutex.tryLock(id) → Boolean`
- `aura.lang.std.Mutex.destroy(id) → Unit`

**Atomic**（6 个）：
- `aura.lang.std.Atomic.new(initial) → Int`
- `aura.lang.std.Atomic.load(id) → Int`
- `aura.lang.std.Atomic.store(id, val) → Unit`
- `aura.lang.std.Atomic.add(id, delta) → Int`
- `aura.lang.std.Atomic.sub(id, delta) → Int`
- `aura.lang.std.Atomic.cas(id, expected, desired) → Boolean`

**RwLock**（6 个）：
- `aura.lang.std.RwLock.new() → Int`
- `aura.lang.std.RwLock.readLock(id) → Unit`
- `aura.lang.std.RwLock.writeLock(id) → Unit`
- `aura.lang.std.RwLock.readUnlock(id) → Unit`
- `aura.lang.std.RwLock.writeUnlock(id) → Unit`
- `aura.lang.std.RwLock.destroy(id) → Unit`

**Condvar**（5 个）：
- `aura.lang.std.Condvar.new() → Int`
- `aura.lang.std.Condvar.wait(id, mutexId) → Unit`
- `aura.lang.std.Condvar.signal(id) → Unit`
- `aura.lang.std.Condvar.broadcast(id) → Unit`
- `aura.lang.std.Condvar.destroy(id) → Unit`

**Barrier**（3 个）：
- `aura.lang.std.Barrier.new(count) → Int`
- `aura.lang.std.Barrier.wait(id) → Int`
- `aura.lang.std.Barrier.destroy(id) → Unit`

**Channel**（3 个）：
- `aura.lang.std.Channel.new(cap) → Int`
- `aura.lang.std.Channel.send(id, val) → Unit`
- `aura.lang.std.Channel.recv(id) → Any`

### 2.3 映射链

```
Aura 函数名                    sanitizellvm()                translate_to_legacy_c()
─────────────────────────────────────────────────────────────────────────────────
aura.lang.std.Mutex.new  →  aura_lang_std_Mutex_new  →  aura_mutex_new
aura.lang.std.Thread.id  →  aura_lang_std_Thread_id  →  aura_thread_id
aura.lang.std.Atomic.cas →  aura_lang_std_Atomic_cas →  aura_atomic_cas
```

---

## 3. 修改文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `compiler/src/codegen/aot/mod.rs` | 修改 | 新增 `inject_concurrent_natives` 函数 + `compile_program` 调用 |
| `compiler/src/codegen/aot/runtime.rs` | 修改 | `translate_to_legacy_c`、`RUNTIME_FUNCTIONS`、`RuntimeFn` 设为 public |
| `compiler/tests/concurrent_aot_tests.rs` | **新增** | 5 个 AOT 并发测试 |

---

## 4. 测试结果

```
test test_aot_generates_concurrent_ffi_declarations ... ok
test test_cffi_signature_concurrent_completeness ... ok
test test_concurrent_natives_injection ... ok
test test_runtime_functions_concurrent_complete ... ok
test test_translate_to_legacy_c_concurrent ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 4.1 测试覆盖

| 测试 | 覆盖内容 |
|------|---------|
| test_concurrent_natives_injection | 并发 native 声明注入正确性 |
| test_aot_generates_concurrent_ffi_declarations | AOT LLVM IR 生成并发 extern declare |
| test_cffi_signature_concurrent_completeness | 21 个并发 C 函数签名完整性 |
| test_translate_to_legacy_c_concurrent | 6 个并发类名 → C 前缀映射 |
| test_runtime_functions_concurrent_complete | 21 个并发运行时函数声明完整性 |

---

## 5. 现有基础设施

Phase A 已建立的 AOT 并发基础设施在 Phase C 中被完整利用：

| 组件 | 位置 | 说明 |
|------|------|------|
| RUNTIME_FUNCTIONS | `runtime.rs` | 21 个并发 C 函数 LLVM IR declare |
| cffi_signature | `runtime.rs` | 21 个并发 C 函数 C ABI 签名 |
| translate_to_legacy_c | `runtime.rs` | 类名 → C 前缀映射（Thread/Mutex/RwLock/Atomic/Condvar/Barrier） |
| compile_std_cffi | `linker.rs` | 编译 `aura_std_cffi.c` + `aura_syscalls.c` 为 .obj |

---

## 6. 待后续完成

| 项目 | 优先级 | 说明 |
|------|--------|------|
| Phase D: JIT 并发支持 | 高 | Cranelift 后端并发指令代码生成 |
| Phase E: 标准库暴露 | 高 | Thread/Mutex/Atomic/Future Aura 类 + Coroutine.aura/Actor.aura 改造 |
| sema/checker.rs 注册 | 中 | 并发函数类型签名注册到语义检查器 |
| HIR desugar 支持 | 中 | `import aura.concurrent.*` 语法糖到 native 调用 |
| 字节码序列化版本升级 | 低 | 新指令需递增 .auc 版本号 |
