# Phase D: JIT 后端并发支持

## 目标

在 JIT（Cranelift）后端中支持并发指令的编译，以及多线程安全的 JIT 编译管理。

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `compiler/src/vm/jit.rs` | 修改 | 并发指令编译 |
| `compiler/src/vm/jit_opt.rs` | 修改 | 优化并发指令 |
| `compiler/src/vm/jit_native.rs` | 修改 | 原生并发调用 |

## 设计

### 1. 并发指令编译

在 JIT 的指令编译逻辑中新增并发指令处理：

```rust
// jit.rs 中的编译逻辑
match instr {
    Instr::ThreadSpawn(idx) => {
        // 生成 Cranelift 外部函数调用
        // call extern(aura_thread_create)
        let func_ptr = import_aura_thread_create(builder);
        builder.call(func_ptr, &[func_arg, arg]);
    }
    
    Instr::MutexLock => {
        // call extern(aura_mutex_lock)
        let func_ptr = import_aura_mutex_lock(builder);
        builder.call(func_ptr, &[mutex_arg]);
    }
    
    Instr::AtomicAdd => {
        // 方案 A: call extern(aura_atomic_add)
        // 方案 B: 使用 Cranelift 原子操作内建（如果支持）
        let func_ptr = import_aura_atomic_add(builder);
        builder.call(func_ptr, &[addr_arg, delta_arg]);
    }
    
    Instr::Yield => {
        // 当前不支持
        // 需要：使用 Cranelift 的 yield/branch 或调用 aura_coroutine_yield
        jit_state.skip(idx, "yield not supported in JIT yet");
    }
}
```

### 2. 外部函数导入

```rust
use cranelift::ir::ExternalName;

/// 导入 C 运行时函数
fn import_runtime_function(builder: &mut cranelift::frontend::FunctionBuilder, name: &str) -> cranelift::ir::FuncRef {
    let ext_name = ExternalName::user(user_function_index(name));
    builder.import_data(ext_name, cranelift::ir::DataSectionKind::Data)
}

/// 获取用户函数索引
fn user_function_index(name: &str) -> u32 {
    // 维护一个运行时函数索引表
    RUNTIME_FUNC_TABLE.get(name).copied().unwrap_or(0)
}
```

### 3. 多线程 JIT 编译

当多个 OS 线程同时触发 JIT 时，需要避免重复编译：

```rust
pub struct JitState {
    compiled: Mutex<HashMap<usize, JitEntry>>,
    /// 正在编译中的函数：idx -> Arc<Condvar>（等待编译结果的线程）
    compiling: Mutex<HashMap<usize, Arc<Condvar>>>,
    skipped: Mutex<HashMap<usize, String>>,
    dispatch_table: Mutex<Vec<Option<JitEntry>>>,
}

impl JitState {
    /// 线程安全地获取或编译函数
    pub fn get_or_compile(&self, idx: usize, module: &BytecodeModule) -> Option<JitEntry> {
        // 1. 检查已编译缓存
        if let Some(entry) = self.compiled.lock().unwrap().get(&idx) {
            return Some(*entry);
        }
        
        // 2. 检查是否正在编译中
        {
            let mut compiling = self.compiling.lock().unwrap();
            if let Some(cv) = compiling.get(&idx) {
                // 等待编译完成
                drop(compiling);
                let _ = self.compiled.lock().unwrap().get(&idx);
                // 条件变量等待
                return self.compiled.lock().unwrap().get(&idx).copied();
            }
            // 标记为正在编译
            let cv = Arc::new(Condvar::new());
            compiling.insert(idx, cv);
        }
        
        // 3. 执行编译
        let entry = self.do_compile(idx, module);
        
        // 4. 注册结果
        if let Some(entry) = entry {
            self.compiled.lock().unwrap().insert(idx, entry);
        } else {
            self.skipped.lock().unwrap().insert(idx, "compilation failed".into());
        }
        
        // 5. 唤醒等待者
        if let Some(cv) = self.compiling.lock().unwrap().remove(&idx) {
            cv.notify_all();
        }
        
        self.compiled.lock().unwrap().get(&idx).copied()
    }
}
```

### 4. 线程本地 VM 状态

当前 JIT 使用 `thread_local!` 管理 VM 状态，需要在多线程场景下：

```rust
// 每个线程有独立的 VM 实例
thread_local! {
    static CURRENT_VM: RefCell<Option<VmHandle>> = RefCell::new(None);
}

impl JitState {
    /// 在当前线程的 VM 上执行
    pub fn call_on_current_vm(&self, idx: usize, args: &[JitValue]) -> Option<JitValue> {
        CURRENT_VM.with(|vm| {
            let entry = self.get_or_compile(idx, &vm.borrow().module)?;
            entry(args)
        })
    }
}
```

## 测试计划

```rust
// compiler/tests/jit_concurrent_tests.rs
#[test]
fn test_jit_thread_spawn() { ... }
#[test]
fn test_jit_mutex() { ... }
#[test]
fn test_jit_atomic() { ... }
#[test]
fn test_jit_multi_thread_compile() { ... }
#[test]
fn test_jit_yield_fallback() { ... }
```

## 依赖

- Phase A: C 运行时原语
- Phase B: VM 指令集定义
- Phase C: AOT 函数表（共享 runtime.rs）
