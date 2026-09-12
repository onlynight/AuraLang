//! Bootstrap 最小引导层测试（完全 Aura 化 Phase 1 验收）。
//!
//! 覆盖《完全Aura化技术方案.md》Phase 1 全部验收标准：
//! VM 核心 / JIT 核心 / AOT 核心 / Any 核心 / 类型内省 / 内存管理 /
//! FFI AOT 直连（三态模式）/ 空值数值检查 / 运行时（协程 + GC）。

use std::rc::Rc;

use compiler::bootstrap::Trap;
use compiler::bootstrap::any_core;
use compiler::bootstrap::aot_core::{AotConfig, AotGenerator};
use compiler::bootstrap::jit_core;
use compiler::bootstrap::jit_core::Deopt;
use compiler::bootstrap::memory;
use compiler::bootstrap::runtime::{Coroutine, CoroutineState, GcHeap};
use compiler::bootstrap::type_core;
use compiler::bootstrap::value_check;
use compiler::bootstrap::vm_core::{BytecodeModule, FfiCache, FuncDef, Insn, Step, Value, Vm};

/// 相对跳转偏移：位于 `from` 的分支指令跳到 `to`。
fn off(from: usize, to: usize) -> i32 {
    to as i32 - (from as i64 + 1) as i32
}

// ---------------------------------------------------------------------------
// 测试模块构建辅助
// ---------------------------------------------------------------------------

/// vm 模块：算术 / 循环 / 调用 / 异常 / 字符串
fn build_vm_module() -> BytecodeModule {
    let mut m = BytecodeModule::new("vmmod");
    // main(): 10 + 32 = 42
    m.add_func(FuncDef::new(
        "main",
        0,
        2,
        vec![
            Insn::Const(Value::Int(10)), // 0
            Insn::StoreLocal(0),         // 1
            Insn::Const(Value::Int(32)), // 2
            Insn::StoreLocal(1),         // 3
            Insn::LoadLocal(0),          // 4
            Insn::LoadLocal(1),          // 5
            Insn::Add,                   // 6
            Insn::Ret,                   // 7
        ],
    ));
    // sum_to(n): 累加 n, n-1, ..., 1（倒计数循环）
    m.add_func(FuncDef::new(
        "sum_to",
        1,
        2,
        vec![
            Insn::Const(Value::Int(0)),   // 0
            Insn::StoreLocal(1),          // 1: acc = 0
            Insn::LoadLocal(0),           // 2: cond = n != 0
            Insn::JmpIfFalse(off(3, 13)), // 3
            Insn::LoadLocal(1),           // 4
            Insn::LoadLocal(0),           // 5
            Insn::Add,                    // 6: acc += n
            Insn::StoreLocal(1),          // 7
            Insn::LoadLocal(0),           // 8
            Insn::Const(Value::Int(1)),   // 9
            Insn::Sub,                    // 10: n -= 1
            Insn::StoreLocal(0),          // 11
            Insn::Jmp(off(12, 2)),        // 12
            Insn::LoadLocal(1),           // 13: exit
            Insn::Ret,                    // 14
        ],
    ));
    // div_zero(): 1 / 0 → Trap
    m.add_func(FuncDef::new(
        "div_zero",
        0,
        0,
        vec![
            Insn::Const(Value::Int(1)), // 0
            Insn::Const(Value::Int(0)), // 1
            Insn::Div,                  // 2
            Insn::Ret,                  // 3
        ],
    ));
    // concat(): "foo" + "bar"
    m.add_func(FuncDef::new(
        "concat",
        0,
        0,
        vec![
            Insn::Const(Value::Str(Rc::from("foo"))), // 0
            Insn::Const(Value::Str(Rc::from("bar"))), // 1
            Insn::Add,                                // 2
            Insn::Ret,                                // 3
        ],
    ));
    // add(a, b): a + b
    m.add_func(FuncDef::new(
        "add",
        2,
        2,
        vec![
            Insn::LoadLocal(0), // 0
            Insn::LoadLocal(1), // 1
            Insn::Add,          // 2
            Insn::Ret,          // 3
        ],
    ));
    // main2(): add(20, 22)
    m.add_func(FuncDef::new(
        "main2",
        0,
        0,
        vec![
            Insn::Const(Value::Int(20)),                         // 0
            Insn::Const(Value::Int(22)),                         // 1
            Insn::Call(m.function_index("add").unwrap() as u16), // 2
            Insn::Ret,                                           // 3
        ],
    ));
    m
}

/// JIT 模块：square 叶子函数 + caller 非叶子 + driver 循环
fn build_jit_module() -> BytecodeModule {
    let mut m = BytecodeModule::new("jitmod");
    // square(x): x * x（叶子，可 JIT）
    let square = m.add_func(FuncDef::new(
        "square",
        1,
        1,
        vec![
            Insn::LoadLocal(0), // 0
            Insn::LoadLocal(0), // 1
            Insn::Mul,          // 2
            Insn::Ret,          // 3
        ],
    ));
    // caller(x): 非叶子（含 Call），JIT 拒绝编译 → 去优化回退解释器
    m.add_func(FuncDef::new(
        "caller",
        1,
        1,
        vec![
            Insn::LoadLocal(0),        // 0
            Insn::Call(square as u16), // 1
            Insn::Ret,                 // 2
        ],
    ));
    // driver(n): acc += square(i)，i in 0..n
    m.add_func(FuncDef::new(
        "driver",
        1,
        3,
        vec![
            Insn::Const(Value::Int(0)),   // 0
            Insn::StoreLocal(1),          // 1: i = 0
            Insn::Const(Value::Int(0)),   // 2
            Insn::StoreLocal(2),          // 3: acc = 0
            Insn::LoadLocal(1),           // 4
            Insn::LoadLocal(0),           // 5
            Insn::Lt,                     // 6: cond = i < n
            Insn::JmpIfFalse(off(7, 18)), // 7
            Insn::LoadLocal(2),           // 8
            Insn::LoadLocal(1),           // 9
            Insn::Call(square as u16),    // 10
            Insn::Add,                    // 11: acc += square(i)
            Insn::StoreLocal(2),          // 12
            Insn::LoadLocal(1),           // 13
            Insn::Const(Value::Int(1)),   // 14
            Insn::Add,                    // 15: i += 1
            Insn::StoreLocal(1),          // 16
            Insn::Jmp(off(17, 4)),        // 17
            Insn::LoadLocal(2),           // 18: exit
            Insn::Ret,                    // 19
        ],
    ));
    m
}

/// FFI 模块：三个直连入口
fn build_ffi_module(abs_slot: u16, strlen_slot: u16, add_slot: u16) -> BytecodeModule {
    let mut m = BytecodeModule::new("ffimod");
    m.add_func(FuncDef::new(
        "call_abs",
        0,
        0,
        vec![
            Insn::Const(Value::Int(-42)), // 0
            Insn::CallFfi(abs_slot),      // 1
            Insn::Ret,                    // 2
        ],
    ));
    m.add_func(FuncDef::new(
        "call_strlen",
        0,
        0,
        vec![
            Insn::Const(Value::Str(Rc::from("hello"))), // 0
            Insn::CallFfi(strlen_slot),                 // 1
            Insn::Ret,                                  // 2
        ],
    ));
    m.add_func(FuncDef::new(
        "call_add",
        0,
        0,
        vec![
            Insn::Const(Value::Int(20)), // 0
            Insn::Const(Value::Int(22)), // 1
            Insn::CallFfi(add_slot),     // 2
            Insn::Ret,                   // 3
        ],
    ));
    m
}

/// JIT + FFI：热点叶子函数内直连 C 函数
fn build_jit_ffi_module(abs_slot: u16) -> BytecodeModule {
    let mut m = BytecodeModule::new("jitffimod");
    // abs_hot(x): abs(x)（叶子，FFI 直连，可 JIT）
    let abs_hot = m.add_func(FuncDef::new(
        "abs_hot",
        1,
        1,
        vec![
            Insn::LoadLocal(0),      // 0
            Insn::CallFfi(abs_slot), // 1
            Insn::Ret,               // 2
        ],
    ));
    // driver(n): acc += abs_hot(i - 10)
    m.add_func(FuncDef::new(
        "driver",
        1,
        3,
        vec![
            Insn::Const(Value::Int(0)),   // 0
            Insn::StoreLocal(1),          // 1: i = 0
            Insn::Const(Value::Int(0)),   // 2
            Insn::StoreLocal(2),          // 3: acc = 0
            Insn::LoadLocal(1),           // 4
            Insn::LoadLocal(0),           // 5
            Insn::Lt,                     // 6: cond = i < n
            Insn::JmpIfFalse(off(7, 20)), // 7
            Insn::LoadLocal(2),           // 8
            Insn::LoadLocal(1),           // 9
            Insn::Const(Value::Int(10)),  // 10
            Insn::Sub,                    // 11
            Insn::Call(abs_hot as u16),   // 12
            Insn::Add,                    // 13: acc += abs_hot(i-10)
            Insn::StoreLocal(2),          // 14
            Insn::LoadLocal(1),           // 15
            Insn::Const(Value::Int(1)),   // 16
            Insn::Add,                    // 17: i += 1
            Insn::StoreLocal(1),          // 18
            Insn::Jmp(off(19, 4)),        // 19
            Insn::LoadLocal(2),           // 20: exit
            Insn::Ret,                    // 21
        ],
    ));
    m
}

/// 协程模块：gen(limit) 依次 yield 0..limit，最终返回最后收到的恢复值
fn build_coroutine_module() -> BytecodeModule {
    let mut m = BytecodeModule::new("comod");
    m.add_func(FuncDef::new(
        "gen",
        1,
        3,
        vec![
            Insn::Const(Value::Int(0)),   // 0
            Insn::StoreLocal(1),          // 1: i = 0
            Insn::LoadLocal(1),           // 2
            Insn::LoadLocal(0),           // 3
            Insn::Lt,                     // 4: cond = i < limit
            Insn::JmpIfFalse(off(5, 14)), // 5
            Insn::LoadLocal(1),           // 6
            Insn::Yield,                  // 7: yield i
            Insn::StoreLocal(2),          // 8: 收到恢复值
            Insn::LoadLocal(1),           // 9
            Insn::Const(Value::Int(1)),   // 10
            Insn::Add,                    // 11: i += 1
            Insn::StoreLocal(1),          // 12
            Insn::Jmp(off(13, 2)),        // 13
            Insn::LoadLocal(2),           // 14: exit
            Insn::Ret,                    // 15
        ],
    ));
    m
}

/// AOT 模块：内联 + DCE + FFI 直连
fn build_aot_module(abs_slot: u16, strlen_slot: u16) -> BytecodeModule {
    let mut m = BytecodeModule::new("aotmod");
    // small(a, b): a + b（直线型，可内联）
    m.add_func(FuncDef::new(
        "small",
        2,
        2,
        vec![
            Insn::LoadLocal(0), // 0
            Insn::LoadLocal(1), // 1
            Insn::Add,          // 2
            Insn::Ret,          // 3
        ],
    ));
    // unused(): 死代码
    m.add_func(FuncDef::new(
        "unused",
        0,
        0,
        vec![
            Insn::Const(Value::Int(1)),
            Insn::Ret,
        ],
    ));
    // main(): abs(-42) + small(6,7) + 1 + strlen("hello") = 42 + 13 + 1 + 5 = 61
    m.add_func(FuncDef::new(
        "main",
        0,
        0,
        vec![
            Insn::Const(Value::Int(-42)),                          // 0
            Insn::CallFfi(abs_slot),                               // 1
            Insn::Const(Value::Int(6)),                            // 2
            Insn::Const(Value::Int(7)),                            // 3
            Insn::Call(m.function_index("small").unwrap() as u16), // 4
            Insn::Add,                                             // 5
            Insn::Const(Value::Int(1)),                            // 6
            Insn::Add,                                             // 7
            Insn::Const(Value::Str(Rc::from("hello"))),            // 8
            Insn::CallFfi(strlen_slot),                            // 9
            Insn::Add,                                             // 10
            Insn::Ret,                                             // 11
        ],
    ));
    m
}

// ---------------------------------------------------------------------------
// 1. VM 核心
// ---------------------------------------------------------------------------

#[test]
fn test_vm_core() {
    let module = build_vm_module();
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let mut vm = Vm::new(&module, &mut ffi);

    // 算术 + 局部变量
    assert_eq!(vm.call("main", &[]).unwrap(), Value::Int(42));
    // 循环 + 分支
    assert_eq!(
        vm.call("sum_to", &[Value::Int(10)]).unwrap(),
        Value::Int(55)
    );
    assert_eq!(
        vm.call("sum_to", &[Value::Int(100)]).unwrap(),
        Value::Int(5050)
    );
    assert_eq!(vm.call("sum_to", &[Value::Int(0)]).unwrap(), Value::Int(0));
    // 函数调用
    assert_eq!(vm.call("main2", &[]).unwrap(), Value::Int(42));
    // 字符串拼接
    assert_eq!(
        vm.call("concat", &[]).unwrap(),
        Value::Str(Rc::from("foobar"))
    );
    // 异常处理：除零产生 Trap 并沿调用链传播
    let err = vm.call("div_zero", &[]).unwrap_err();
    assert!(err.message.contains("除零"), "实际: {}", err.message);
    // 参数个数校验
    assert!(vm.call("add", &[Value::Int(1)]).is_err());
    // Trap 之后 VM 仍可继续执行（状态一致）
    assert_eq!(vm.call("main", &[]).unwrap(), Value::Int(42));
}

#[test]
fn test_vm_core_float_and_mixed() {
    let mut m = BytecodeModule::new("fmod");
    m.add_func(FuncDef::new(
        "half",
        1,
        1,
        vec![
            Insn::LoadLocal(0),             // 0
            Insn::Const(Value::Float(2.0)), // 1
            Insn::Div,                      // 2
            Insn::Ret,                      // 3
        ],
    ));
    let mut ffi = FfiCache::new();
    let mut vm = Vm::new(&m, &mut ffi);
    // Int / Float 混合运算
    assert_eq!(
        vm.call("half", &[Value::Int(5)]).unwrap(),
        Value::Float(2.5)
    );
}

// ---------------------------------------------------------------------------
// 2. JIT 核心
// ---------------------------------------------------------------------------

#[test]
fn test_jit_core() {
    let module = build_jit_module();
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let mut vm = Vm::new(&module, &mut ffi);
    vm.enable_jit(5); // 低阈值便于测试

    // Σ i² for i in 0..50 = 40425
    let r = vm.call("driver", &[Value::Int(50)]).unwrap();
    assert_eq!(r, Value::Int(40425));

    // 热点检测 + 编译：square 被编译为 JIT 单元
    let jit = vm.jit().unwrap();
    assert!(jit.compiles >= 1, "热点函数应被编译");
    let square_idx = module.function_index("square").unwrap();
    assert!(jit.is_compiled(square_idx));
    assert!(jit.unit(square_idx).is_some());
    // caller 非叶子函数（含 Call）不应被编译
    let caller_idx = module.function_index("caller").unwrap();
    assert!(!jit.is_compiled(caller_idx));
}

#[test]
fn test_jit_core_deopt() {
    // JIT 单元遇到除零 → Deopt → 解释器产生精确 Trap
    let mut m = BytecodeModule::new("deoptmod");
    m.add_func(FuncDef::new(
        "bad_div",
        1,
        1,
        vec![
            Insn::LoadLocal(0),         // 0
            Insn::Const(Value::Int(0)), // 1
            Insn::Div,                  // 2
            Insn::Ret,                  // 3
        ],
    ));
    let mut ffi = FfiCache::new();
    let mut vm = Vm::new(&m, &mut ffi);
    vm.enable_jit(1);

    let err = vm.call("bad_div", &[Value::Int(9)]).unwrap_err();
    assert!(err.message.contains("除零"), "实际: {}", err.message);
    assert_eq!(vm.jit().unwrap().deopts, 1, "应记录一次去优化");
}

#[test]
fn test_jit_core_direct_dispatch() {
    // JIT 执行与解释执行语义一致：直接派发编译单元
    let module = build_jit_module();
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let mut vm = Vm::new(&module, &mut ffi);
    vm.enable_jit(2);
    let square_idx = module.function_index("square").unwrap();

    // 第 1 次：解释执行，尚未编译
    assert_eq!(vm.call("caller", &[Value::Int(7)]).unwrap(), Value::Int(49));
    assert!(!vm.jit().unwrap().is_compiled(square_idx));
    // 第 2 次达到阈值：编译并以 JIT 直接派发执行
    assert_eq!(vm.call("caller", &[Value::Int(7)]).unwrap(), Value::Int(49));
    assert!(vm.jit().unwrap().is_compiled(square_idx));

    // JIT 单元可独立直接执行（验证直接派发路径）
    let unit = vm.jit().unwrap().unit(square_idx).unwrap().clone();
    drop(vm); // ffi 被 Vm 独占借用，先释放
    let r = jit_core::execute(&unit, &[Value::Int(12)], &mut ffi);
    assert_eq!(r.unwrap(), Value::Int(144));
    // 参数错误 → Deopt（而非错误结果）
    let r = jit_core::execute(&unit, &[], &mut ffi);
    assert_eq!(r.unwrap_err(), Deopt);
}

// ---------------------------------------------------------------------------
// 3. AOT 核心
// ---------------------------------------------------------------------------

#[test]
fn test_aot_core() {
    let module = build_aot_module(0, 1);
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let aot = AotGenerator::new(&module, &ffi, AotConfig::default());
    let ir = aot.emit_llvm_ir("main").unwrap();

    // FFI AOT 直连：declare + 直接 call（非函数指针）
    assert!(ir.contains("declare i32 @abs(i32)"), "缺少 abs 声明:\n{ir}");
    assert!(ir.contains("declare i64 @strlen(ptr)"), "缺少 strlen 声明");
    assert!(ir.contains("call i32 @abs("), "缺少 abs 直接调用");
    assert!(ir.contains("call i64 @strlen("), "缺少 strlen 直接调用");
    // 消除间接调用
    assert!(!ir.contains("call ptr"), "存在函数指针间接调用");

    // 内联优化：small 已内联进 main，不产生调用
    assert!(
        !ir.contains("call i64 @\"aura.bs.small\""),
        "small 应被内联"
    );
    // 死代码消除：unused 不可达，不发射
    assert!(!ir.contains("aura.bs.unused"), "unused 应被 DCE");
    // 入口函数存在
    assert!(ir.contains("define i64 @\"aura.bs.main\""));
    // 字符串常量全局
    assert!(ir.contains("@.str0 = private unnamed_addr constant"));
}

#[test]
fn test_aot_core_keeps_reachable_call() {
    // 不满足内联条件的函数保持真实直接调用
    let mut m = BytecodeModule::new("keepcall");
    let big = m.add_func(FuncDef::new(
        "big",
        1,
        1,
        vec![
            Insn::LoadLocal(0), // 0
            Insn::LoadLocal(0), // 1
            Insn::Mul,          // 2
            Insn::LoadLocal(0), // 3
            Insn::Add,          // 4
            Insn::LoadLocal(0), // 5
            Insn::Add,          // 6
            Insn::LoadLocal(0), // 7
            Insn::Add,          // 8
            Insn::LoadLocal(0), // 9
            Insn::Add,          // 10
            Insn::Ret,          // 11
        ],
    ));
    m.add_func(FuncDef::new(
        "main",
        0,
        0,
        vec![
            Insn::Const(Value::Int(3)), // 0
            Insn::Call(big as u16),     // 1
            Insn::Ret,                  // 2
        ],
    ));
    let ffi = FfiCache::new();
    let aot = AotGenerator::new(&m, &ffi, AotConfig::default());
    let ir = aot.emit_llvm_ir("main").unwrap();
    assert!(
        ir.contains("call i64 @\"aura.bs.big\""),
        "超阈值函数应保持直接调用:\n{ir}"
    );
}

#[test]
fn test_aot_core_rejects_yield() {
    let module = build_coroutine_module();
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let aot = AotGenerator::new(&module, &ffi, AotConfig::default());
    let err = aot.emit_llvm_ir("gen").unwrap_err();
    assert!(err.contains("Yield"), "实际: {err}");
}

// ---------------------------------------------------------------------------
// 4. FFI AOT 直连（三态模式）
// ---------------------------------------------------------------------------

#[test]
fn test_ffi_aot_direct_vm() {
    // VM 模式：启动时预加载地址，执行期按槽位直连（无符号查找）
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let abs_slot = ffi.slot("abs").unwrap() as u16;
    let strlen_slot = ffi.slot("strlen").unwrap() as u16;
    let add_slot = ffi.slot("aura_bootstrap_add_i32").unwrap() as u16;

    let module = build_ffi_module(abs_slot, strlen_slot, add_slot);
    let mut vm = Vm::new(&module, &mut ffi);

    assert_eq!(vm.call("call_abs", &[]).unwrap(), Value::Int(42));
    assert_eq!(vm.call("call_strlen", &[]).unwrap(), Value::Int(5));
    assert_eq!(vm.call("call_add", &[]).unwrap(), Value::Int(42));

    // 直连计数（预加载条目被实际调用）
    assert_eq!(vm.ffi().entry(abs_slot as usize).unwrap().calls, 1);
    assert_eq!(vm.ffi().entry(strlen_slot as usize).unwrap().calls, 1);
    assert_eq!(vm.ffi().entry(add_slot as usize).unwrap().calls, 1);
}

#[test]
fn test_ffi_aot_direct_jit() {
    // JIT 模式：热点叶子函数内的 FFI 调用点解析为内联缓存槽位，直接派发
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let abs_slot = ffi.slot("abs").unwrap() as u16;

    let module = build_jit_ffi_module(abs_slot);
    let mut vm = Vm::new(&module, &mut ffi);
    vm.enable_jit(5);

    // Σ |i - 10| for i in 0..50 = 835
    let r = vm.call("driver", &[Value::Int(50)]).unwrap();
    assert_eq!(r, Value::Int(835));

    let jit = vm.jit().unwrap();
    let abs_hot_idx = module.function_index("abs_hot").unwrap();
    assert!(jit.is_compiled(abs_hot_idx), "热点 FFI 叶子函数应被编译");
    let unit = jit.unit(abs_hot_idx).unwrap();
    assert_eq!(
        unit.ffi_slots,
        vec![abs_slot as usize],
        "FFI 内联缓存应绑定槽位"
    );
    assert_eq!(vm.ffi().entry(abs_slot as usize).unwrap().calls, 50);
}

#[test]
fn test_ffi_aot_direct_aot() {
    // AOT 模式：FFI 生成 declare + 直接 call 指令；
    // 此处验证自定义 C ABI 库同样直连
    let mut ffi = FfiCache::new();
    ffi.preload_std();
    let add_slot = ffi.slot("aura_bootstrap_add_i32").unwrap() as u16;

    let mut m = BytecodeModule::new("aotffi");
    m.add_func(FuncDef::new(
        "main",
        0,
        0,
        vec![
            Insn::Const(Value::Int(40)), // 0
            Insn::Const(Value::Int(2)),  // 1
            Insn::CallFfi(add_slot),     // 2
            Insn::Ret,                   // 3
        ],
    ));
    let aot = AotGenerator::new(&m, &ffi, AotConfig::default());
    let ir = aot.emit_llvm_ir("main").unwrap();
    assert!(
        ir.contains("declare i32 @aura_bootstrap_add_i32(i32, i32)"),
        "自定义 C ABI 库声明缺失:\n{ir}"
    );
    assert!(ir.contains("call i32 @aura_bootstrap_add_i32("));
    assert!(!ir.contains("call ptr"));
}

// ---------------------------------------------------------------------------
// 5. Any 核心
// ---------------------------------------------------------------------------

#[test]
fn test_any_core() {
    use compiler::bootstrap::any_core::{equals, hash_code, to_string};
    // toString
    assert_eq!(to_string(&Value::Null), "null");
    assert_eq!(to_string(&Value::Bool(false)), "false");
    assert_eq!(to_string(&Value::Int(123)), "123");
    assert_eq!(to_string(&Value::Float(2.5)), "2.5");
    assert_eq!(to_string(&Value::Str(Rc::from("ok"))), "ok");
    // equals
    assert!(equals(&Value::Int(3), &Value::Int(3)));
    assert!(
        equals(&Value::Int(3), &Value::Float(3.0)),
        "Int/Float 数值相等"
    );
    assert!(!equals(
        &Value::Str(Rc::from("a")),
        &Value::Str(Rc::from("b"))
    ));
    // hashCode：稳定 + 区分
    assert_eq!(hash_code(&Value::Int(9)), hash_code(&Value::Int(9)));
    assert_ne!(hash_code(&Value::Int(9)), hash_code(&Value::Int(10)));
    assert_ne!(
        hash_code(&Value::Str(Rc::from("9"))),
        hash_code(&Value::Int(9))
    );
}

// ---------------------------------------------------------------------------
// 6. 类型内省核心
// ---------------------------------------------------------------------------

#[test]
fn test_type_core() {
    use compiler::bootstrap::type_core::{cast, is_of_type, type_of};
    assert_eq!(type_of(&Value::Null), "Null");
    assert_eq!(type_of(&Value::Int(1)), "Int");
    assert!(is_of_type(&Value::Float(1.0), "Float"));
    assert!(!is_of_type(&Value::Int(1), "Float"));
    // cast
    assert_eq!(cast(&Value::Bool(true), "Int").unwrap(), Value::Int(1));
    assert_eq!(cast(&Value::Int(4), "Float").unwrap(), Value::Float(4.0));
    assert_eq!(
        cast(&Value::Str(Rc::from("42")), "Int").unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        cast(&Value::Int(7), "Str").unwrap(),
        Value::Str(Rc::from("7"))
    );
    // 非法转换 → Trap
    assert!(cast(&Value::Str(Rc::from("xyz")), "Int").is_err());
    assert!(cast(&Value::Int(1), "Null").is_err());
}

// ---------------------------------------------------------------------------
// 7. 内存管理
// ---------------------------------------------------------------------------

#[test]
fn test_memory() {
    // malloc/free 写读一致
    let p = memory::ManagedPtr::new(32).unwrap();
    assert_eq!(unsafe { memory::block_size(p.as_ptr()) }, 32);
    // arc 计数
    unsafe {
        assert_eq!(memory::arc_increment(p.as_ptr()), 2);
        assert_eq!(memory::arc_increment(p.as_ptr()), 3);
        assert_eq!(memory::arc_decrement(p.as_ptr()), 2);
        assert_eq!(memory::arc_decrement(p.as_ptr()), 1);
    }
    // 字符串操作
    let a = memory::string_new("aura");
    let b = memory::string_new("lang");
    assert_eq!(memory::string_length(&a).unwrap(), 4);
    let c = memory::string_concat(&a, &b).unwrap();
    assert!(matches!(&c, Value::Str(s) if s.as_ref() == "auralang"));
    // 类型错误
    assert!(memory::string_length(&Value::Int(4)).is_err());
    // 零长度分配拒绝
    assert!(memory::malloc(0).is_err());
}

// ---------------------------------------------------------------------------
// 8. 空值/数值检查
// ---------------------------------------------------------------------------

#[test]
fn test_value_check() {
    use compiler::bootstrap::value_check::*;
    assert!(is_null(&Value::Null) && !is_not_null(&Value::Null));
    assert!(is_zero(&Value::Int(0)) && is_zero(&Value::Float(0.0)));
    assert!(is_positive(&Value::Int(1)) && is_negative(&Value::Int(-1)));
    assert!(is_nan(&Value::Float(f64::NAN)));
    assert!(is_infinite(&Value::Float(f64::INFINITY)));
    assert!(!is_nan(&Value::Int(0)));
    assert!(!is_zero(&Value::Null));
}

// ---------------------------------------------------------------------------
// 9. 运行时：协程 + GC
// ---------------------------------------------------------------------------

#[test]
fn test_runtime_coroutine() {
    let module = build_coroutine_module();
    let mut ffi = FfiCache::new();
    ffi.preload_std();

    let mut co = Coroutine::new(&module, &mut ffi, "gen", &[Value::Int(4)]).unwrap();
    assert_eq!(co.state(), CoroutineState::Fresh);

    // 依次让出 0,1,2,3；恢复值被协程接收（最终返回最后收到的恢复值）
    let s = co.resume(Value::Null).unwrap();
    assert_eq!(s, Step::Yielded(Value::Int(0)));
    assert_eq!(co.state(), CoroutineState::Suspended);

    let s = co.resume(Value::Null).unwrap();
    assert_eq!(s, Step::Yielded(Value::Int(1)));

    let s = co.resume(Value::Null).unwrap();
    assert_eq!(s, Step::Yielded(Value::Int(2)));

    let s = co.resume(Value::Null).unwrap();
    assert_eq!(s, Step::Yielded(Value::Int(3)));

    // 第 5 次恢复交付 77：i=4 循环退出，返回最后收到的恢复值 77
    let s = co.resume(Value::Int(77)).unwrap();
    assert_eq!(s, Step::Done(Value::Int(77)));
    assert_eq!(co.state(), CoroutineState::Finished);

    // 结束后不可恢复
    assert!(co.resume(Value::Null).is_err());
}

#[test]
fn test_runtime_gc() {
    let mut heap = GcHeap::new();
    let _a = heap.alloc(16).unwrap();
    let b = heap.alloc(32).unwrap();
    let _c = heap.alloc(64).unwrap();
    assert_eq!(heap.live_objects(), 3);

    // b 为根 → a、c 被回收
    heap.add_root(&b).unwrap();
    let stats = heap.collect();
    assert_eq!(stats.freed_objects, 2);
    assert_eq!(stats.freed_bytes, 16 + 64);
    assert_eq!(stats.live_objects, 1);

    // 移除根后 b 也被回收
    heap.remove_root(&b);
    let stats = heap.collect();
    assert_eq!(stats.freed_objects, 1);
    assert_eq!(stats.live_objects, 0);
    assert_eq!(stats.collections, 2);

    // 非指针根 → Trap
    assert!(heap.add_root(&Value::Int(1)).is_err());
}

// ---------------------------------------------------------------------------
// 10. Trap 语义（异常处理核心）
// ---------------------------------------------------------------------------

#[test]
fn test_trap_display_and_propagation() {
    let t = Trap::new("something failed");
    assert_eq!(t.to_string(), "bootstrap trap: something failed");
    let module = build_vm_module();
    let mut ffi = FfiCache::new();
    let mut vm = Vm::new(&module, &mut ffi);
    // 未定义函数 → Trap
    let e = vm.call("no_such_fn", &[]).unwrap_err();
    assert!(e.message.contains("no_such_fn"));
    // Layer 0 协议集成：Trap 后 any/type/value_check 协议仍可用
    let _ = any_core::to_string(&Value::Int(e.message.len() as i64));
    let _ = type_core::type_of(&Value::Null);
    let _ = value_check::is_null(&Value::Null);
}
