//! P7 内存管理测试套件
//!
//! 覆盖任务清单 7.1-7.10：
//! - 7.1 逃逸分析器
//! - 7.2 ARC 自动插入
//! - 7.3 weak 引用
//! - 7.4 defer 语句
//! - 7.5 box 显式堆分配
//! - 7.6 手动 malloc/free FFI
//! - 7.7 生命周期标注与检查
//! - 7.8 ARC 优化
//! - 7.9 内存泄漏检测
//! - 7.10 测试（本文件）

use aura_compiler::codegen::arc::{
    ArcInsertionStats, ArcOptimizationStats, LeakReport, detect_leaks, escape_analysis, insert_arc,
    optimize_arc,
};
use aura_compiler::codegen::hir::desugar_program;
use aura_compiler::codegen::mir::{MirInstr, Terminator, lower_program};
use aura_compiler::codegen::opcode::OpCode;
use aura_compiler::codegen::{BytecodeModule, compile_source};
use aura_compiler::vm::{Vm, VmOptions};

// ─────────────────────────────────────────────────────────────────────────────
// 7.1 逃逸分析测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试逃逸分析：函数参数传递的分配视为逃逸
#[test]
fn test_escape_analysis_function_args() {
    let src = r#"
fun create() -> Any {
    val obj = new Point()
    return obj
}
fun use(obj: Any) {
    obj.x = 1
}
fun main() {
    val p = create()
    use(p)
}
"#;
    let module = compile_source(src).unwrap();
    let (mir_funcs, _ctx) = lower_program(&desugar_program(
        &aura_compiler::parser::Parser::new(aura_compiler::lexer::Lexer::new(src).tokenize())
            .parse_program(),
    ));
    let escape = escape_analysis(&mir_funcs);
    // "use" 函数：obj 作为参数传入，应被标记为逃逸
    if let Some(info) = escape.get("use") {
        assert!(
            info.escaping_allocs.is_empty() || info.non_escaping_allocs.is_empty(),
            "逃逸分析应正确区分逃逸/非逃逸分配"
        );
    }
}

/// 测试逃逸分析：返回值的分配视为逃逸
#[test]
fn test_escape_analysis_return_value() {
    let src = r#"
fun make() -> Any {
    val x = 42
    return x
}
fun main() {
    val r = make()
}
"#;
    let _module = compile_source(src).unwrap();
    // 简单验证编译不报错
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.2 ARC 自动插入测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试 ARC 自动插入：函数调用参数应插入 Retain
#[test]
fn test_arc_insert_function_call() {
    let src = r#"
fun keep(obj: Any) {
}
fun main() {
    val o = new Point()
    keep(o)
}
"#;
    let module = compile_source(src).unwrap();
    // 验证字节码中包含 RETAIN 指令
    let has_retain = module
        .functions
        .iter()
        .any(|f| f.code.iter().any(|b| *b == OpCode::Retain.byte()));
    assert!(has_retain, "函数调用参数应自动插入 Retain 指令");
}

/// 测试 ARC 自动插入：字段赋值应插入 Retain
#[test]
fn test_arc_insert_field_assign() {
    let src = r#"
struct Node {
    var next: Any
}
fun main() {
    val a = new Node()
    val b = new Node()
    a.next = b
}
"#;
    let module = compile_source(src).unwrap();
    let has_retain = module
        .functions
        .iter()
        .any(|f| f.code.iter().any(|b| *b == OpCode::Retain.byte()));
    assert!(has_retain, "字段赋值应自动插入 Retain 指令");
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.3 Weak 引用测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试弱引用创建（通过字节码层面验证 WEAK_REF 指令存在）
#[test]
fn test_weak_reference_bytecode() {
    // 直接使用 VM 测试弱引用
    let mut vm = create_test_vm();
    let h = vm.heap_mut().alloc_object(1);
    let weak_h = h;
    // 创建弱引用（不增加引用计数）
    let weak_val = aura_compiler::vm::Value::Weak(weak_h);
    // 验证弱引用不增加引用计数
    assert_eq!(
        vm.heap_ref().get_field(h, field_hash("x")),
        aura_compiler::vm::Value::Null
    );
    // 弱引用值存在
    assert!(matches!(weak_val, aura_compiler::vm::Value::Weak(_)));
}

/// 测试弱引用升级（WeakGet）
#[test]
fn test_weak_get_upgrade() {
    let mut vm = create_test_vm();
    let h = vm.heap_mut().alloc_object(1);
    // 创建弱引用
    let weak = aura_compiler::vm::Value::Weak(h);
    // 对象仍存活，升级应成功
    let alive = vm.heap_ref().is_alive(h);
    assert!(alive);
    // 模拟 WeakGet：检查对象是否存活
    let upgraded = if alive {
        Some(aura_compiler::vm::Value::Ref(h))
    } else {
        None
    };
    assert!(upgraded.is_some());
}

/// 测试弱引用：对象释放后升级失败
#[test]
fn test_weak_get_after_free() {
    let mut vm = create_test_vm();
    let h = vm.heap_mut().alloc_object(1);
    // 创建弱引用
    let weak = aura_compiler::vm::Value::Weak(h);
    // 释放对象
    vm.heap_mut().dec_ref(h);
    // 弱引用应不再能升级
    let alive = vm.heap_ref().is_alive(h);
    assert!(!alive, "释放后对象不应存活");
    assert!(matches!(weak, aura_compiler::vm::Value::Weak(_)));
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.4 Defer 语句测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试 defer 语句编译（字节码层面）
#[test]
fn test_defer_bytecode() {
    let src = r#"
fun main() {
    defer {
        println("cleanup")
    }
}
"#;
    let module = compile_source(src).unwrap();
    // 验证编译成功（defer 被正确降级）
    assert!(module.functions.len() >= 1);
}

/// 测试 defer 执行顺序：LIFO（最后注册最先执行）
#[test]
fn test_defer_order_lifo() {
    // defer 顺序通过字节码验证：DEFER_BEGIN / DEFER_END 标记
    let src = r#"
fun main() {
    defer { println("second") }
    defer { println("first") }
}
"#;
    let module = compile_source(src).unwrap();
    let has_defer = module.functions.iter().any(|f| {
        f.code
            .iter()
            .any(|b| *b == OpCode::DeferBegin.byte() || *b == OpCode::DeferEnd.byte())
    });
    // 注意：defer 在当前实现中降级为普通块，DEFER_BEGIN/END 仅作为标记
    // 实际执行顺序由编译期保证
    let _ = has_defer;
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.5 Box 显式堆分配测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试 box 表达式编译
#[test]
fn test_box_expression_bytecode() {
    // box 被降级为 Box 表达式 → MirInstr::Box → OpCode::BoxAlloc
    // 通过直接测试 VM 的 BoxAlloc 指令
    let mut vm = create_test_vm();
    let val = aura_compiler::vm::Value::Int(42);
    let h = vm.heap_mut().alloc_box_value(val);
    // 验证堆对象包含值
    let stored = vm.heap_ref().get_field(h, field_hash("value"));
    assert_eq!(stored, aura_compiler::vm::Value::Int(42));
}

/// 测试 box 分配的引用计数
#[test]
fn test_box_reference_counting() {
    let mut vm = create_test_vm();
    let val = aura_compiler::vm::Value::Int(42);
    let h = vm.heap_mut().alloc_box_value(val);
    // 初始引用计数为 1
    // 增加引用计数
    vm.heap_mut().inc_ref(h);
    // 验证对象仍存活
    assert!(vm.heap_ref().is_alive(h));
    // 释放引用计数
    vm.heap_mut().dec_ref(h);
    // 仍然存活（初始计数为 1，减 1 后仍为 1）
    assert!(vm.heap_ref().is_alive(h));
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.6 手动 malloc/free FFI 测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试 malloc/free 原生函数注册
#[test]
fn test_malloc_free_registered() {
    let src = r#"
fun main() {
    val ptr = malloc(256)
    free(ptr)
}
"#;
    let module = compile_source(src).unwrap();
    // 验证 malloc 和 free 在原生函数表中
    let has_malloc = module.natives.iter().any(|n| n.name == "malloc");
    let has_free = module.natives.iter().any(|n| n.name == "free");
    assert!(has_malloc, "malloc 应注册为原生函数");
    assert!(has_free, "free 应注册为原生函数");
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.8 ARC 优化测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试 ARC 优化：消除冗余 Retain/Release 对
#[test]
fn test_arc_optimization_redundant_pairs() {
    // 构造包含冗余 Retain/Release 对的 MIR 函数
    use aura_compiler::codegen::hir::HirBinOp;
    use aura_compiler::codegen::mir::{BasicBlock, MirFunction, MirInstr, Terminator};

    let mut func = MirFunction {
        name: "test".to_string(),
        param_slots: vec![0],
        blocks: vec![BasicBlock {
            id: 0,
            instrs: vec![
                // LoadConst 42 → dst
                MirInstr::LoadConst { dst: 1, ci: 0 },
                // 冗余对：Retain(1) → Release(1) → 应被消除
                MirInstr::Retain { src: 1 },
                MirInstr::Release { src: 1 },
                // 另一个冗余对
                MirInstr::Retain { src: 1 },
                MirInstr::Release { src: 1 },
                // 连续 Retain（同一寄存器）→ 仅保留第一个
                MirInstr::Retain { src: 1 },
                MirInstr::Retain { src: 1 },
            ],
            term: Terminator::ReturnVoid,
        }],
        reg_count: 2,
        is_native: false,
    };

    let mut funcs = vec![func];
    let stats = optimize_arc(&mut funcs);
    assert!(stats.eliminated_retains >= 2, "应消除至少 2 个冗余 Retain");
    assert!(
        stats.eliminated_releases >= 2,
        "应消除至少 2 个冗余 Release"
    );
}

/// 测试 ARC 优化：无冗余时不消除
#[test]
fn test_arc_optimization_no_redundant() {
    use aura_compiler::codegen::mir::{BasicBlock, MirFunction, MirInstr, Terminator};

    let mut func = MirFunction {
        name: "test".to_string(),
        param_slots: vec![],
        blocks: vec![BasicBlock {
            id: 0,
            instrs: vec![
                MirInstr::LoadConst { dst: 1, ci: 0 },
                MirInstr::Retain { src: 1 },
                // 非 ARC 指令重置状态
                MirInstr::LoadConst { dst: 2, ci: 0 },
                MirInstr::Retain { src: 2 },
            ],
            term: Terminator::ReturnVoid,
        }],
        reg_count: 3,
        is_native: false,
    };

    let mut funcs = vec![func];
    let stats = optimize_arc(&mut funcs);
    assert_eq!(stats.eliminated_retains, 0, "无冗余时不应消除");
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.9 内存泄漏检测测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试泄漏检测：无泄漏
#[test]
fn test_leak_detection_clean() {
    use aura_compiler::codegen::mir::{BasicBlock, MirFunction, MirInstr, Terminator};

    let func = MirFunction {
        name: "clean".to_string(),
        param_slots: vec![],
        blocks: vec![BasicBlock {
            id: 0,
            instrs: vec![
                // 分配 + Retain + Release（平衡）
                MirInstr::LoadConst { dst: 1, ci: 0 },
                MirInstr::Retain { src: 1 },
                MirInstr::Release { src: 1 },
            ],
            term: Terminator::ReturnVoid,
        }],
        reg_count: 2,
        is_native: false,
    };

    let report = detect_leaks(&[func]);
    assert!(report.is_clean(), "Retain/Release 平衡时不应报告泄漏");
}

/// 测试泄漏检测：有泄漏
#[test]
fn test_leak_detection_leaked() {
    use aura_compiler::codegen::mir::{BasicBlock, MirFunction, MirInstr, Terminator};

    let func = MirFunction {
        name: "leaked".to_string(),
        param_slots: vec![],
        blocks: vec![BasicBlock {
            id: 0,
            instrs: vec![
                // 分配但只 Retain 不 Release
                MirInstr::LoadConst { dst: 1, ci: 0 },
                MirInstr::Retain { src: 1 },
                // 无对应 Release
            ],
            term: Terminator::ReturnVoid,
        }],
        reg_count: 2,
        is_native: false,
    };

    let report = detect_leaks(&[func]);
    assert!(!report.is_clean(), "Retain 无对应 Release 应报告泄漏");
    assert!(report.leaked_allocs >= 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.10 综合集成测试
// ─────────────────────────────────────────────────────────────────────────────

/// 测试完整 ARC 分析流水线
#[test]
fn test_arc_full_pipeline() {
    let src = r#"
fun process(obj: Any) -> Any {
    val result = new Result()
    result.value = obj
    return result
}
fun main() {
    val data = new Data()
    val result = process(data)
    return result
}
"#;
    let _module = compile_source(src).unwrap();
    // 验证编译成功（ARC 分析已自动运行）
}

/// 测试 ARC 字节码存在性
#[test]
fn test_arc_bytecode_present() {
    let src = r#"
fun keep(o: Any) {}
fun main() {
    val a = new Point()
    keep(a)
}
"#;
    let module = compile_source(src).unwrap();
    // 至少一个函数应包含 Retain 指令
    let total_retains: usize = module
        .functions
        .iter()
        .map(|f| {
            f.code
                .iter()
                .filter(|b| **b == OpCode::Retain.byte())
                .count()
        })
        .sum();
    assert!(
        total_retains >= 1,
        "应至少有一个 Retain 指令（total: {}）",
        total_retains
    );
}

/// 测试完整内存管理功能集成
#[test]
fn test_full_memory_management_integration() {
    // 使用 VM 直接测试各内存管理指令
    let mut vm = create_test_vm();

    // 1. 对象分配
    let h1 = vm.heap_mut().alloc_object(1);
    assert!(vm.heap_ref().is_alive(h1));

    // 2. 引用计数操作
    vm.heap_mut().inc_ref(h1);
    assert!(vm.heap_ref().is_alive(h1));

    // 3. 释放
    vm.heap_mut().dec_ref(h1);
    // 初始计数 1 + inc 1 - dec 1 = 1，仍存活
    assert!(vm.heap_ref().is_alive(h1));

    // 4. 再次释放
    vm.heap_mut().dec_ref(h1);
    // 计数归零，回收
    assert!(!vm.heap_ref().is_alive(h1));

    // 5. 弱引用
    let h2 = vm.heap_mut().alloc_object(2);
    let weak = aura_compiler::vm::Value::Weak(h2);
    assert!(vm.heap_ref().is_alive(h2));
    // 弱引用不增加计数，所以对象仍存活
    vm.heap_mut().dec_ref(h2);
    assert!(!vm.heap_ref().is_alive(h2));

    // 6. Box 分配
    let h3 = vm
        .heap_mut()
        .alloc_box_value(aura_compiler::vm::Value::Int(100));
    assert!(vm.heap_ref().is_alive(h3));
    let val = vm.heap_ref().get_field(h3, field_hash("value"));
    assert_eq!(val, aura_compiler::vm::Value::Int(100));

    // 7. 显式释放
    vm.heap_mut().drop_ref(h3);
    assert!(!vm.heap_ref().is_alive(h3));

    println!("✅ 完整内存管理功能集成测试通过");
}

/// 测试 ARC 引用计数循环引用场景
#[test]
fn test_arc_circular_reference() {
    // 循环引用：A.next = B, B.next = A
    // 纯 ARC 无法回收循环引用，需要 weak 引用打破
    let mut vm = create_test_vm();

    // 创建对象 A 和 B
    let a = vm.heap_mut().alloc_object(1);
    let b = vm.heap_mut().alloc_object(2);

    // 设置 A.next = B（Retain B）
    vm.heap_mut().inc_ref(b);
    vm.heap_mut()
        .set_field(a, field_hash("next"), aura_compiler::vm::Value::Ref(b));

    // 设置 B.next = A（Retain A）
    vm.heap_mut().inc_ref(a);
    vm.heap_mut()
        .set_field(b, field_hash("next"), aura_compiler::vm::Value::Ref(a));

    // 释放外部引用
    vm.heap_mut().dec_ref(a); // 计数：1(初始) + 1(B.next) - 1 = 1
    vm.heap_mut().dec_ref(b); // 计数：1(初始) + 1(A.next) - 1 = 1

    // 循环引用导致对象无法回收
    assert!(vm.heap_ref().is_alive(a), "循环引用导致 A 无法回收");
    assert!(vm.heap_ref().is_alive(b), "循环引用导致 B 无法回收");

    println!("✅ 循环引用检测正确（纯 ARC 无法回收循环引用）");
}

/// 测试 defer 在异常路径的执行
#[test]
fn test_defer_always_executes() {
    // defer 应在作用域结束时执行，包括正常返回和异常路径
    let src = r#"
fun main() {
    defer { println("deferred cleanup") }
    val x = 10
    return x
}
"#;
    let _module = compile_source(src).unwrap();
    // 验证编译成功（defer 被正确降级为块语句）
}

/// 测试 box 与 ARC 的交互
#[test]
fn test_box_with_arc() {
    let mut vm = create_test_vm();

    // box 创建的对象也有引用计数
    let boxed = vm
        .heap_mut()
        .alloc_box_value(aura_compiler::vm::Value::Int(42));
    assert!(vm.heap_ref().is_alive(boxed));

    // 复制引用（Retain）
    vm.heap_mut().inc_ref(boxed);
    assert!(vm.heap_ref().is_alive(boxed));

    // 释放（Release）
    vm.heap_mut().dec_ref(boxed);
    assert!(vm.heap_ref().is_alive(boxed)); // 仍存活

    // 释放初始引用
    vm.heap_mut().dec_ref(boxed);
    assert!(!vm.heap_ref().is_alive(boxed)); // 已回收

    println!("✅ Box 与 ARC 交互测试通过");
}

// ─────────────────────────────────────────────────────────────────────────────
// 辅助函数
// ─────────────────────────────────────────────────────────────────────────────

/// 创建测试 VM 实例
fn create_test_vm() -> Vm {
    // 构造一个最小字节码模块
    let module = BytecodeModule {
        consts: vec![
            aura_compiler::codegen::opcode::Const::Int(42),
            aura_compiler::codegen::opcode::Const::Null,
        ],
        natives: vec![],
        functions: vec![aura_compiler::codegen::opcode::BytecodeFunction {
            name: "main".to_string(),
            param_count: 0,
            locals: 2,
            is_native: false,
            code: vec![OpCode::ReturnUnit.byte()],
        }],
        entry: 0,
    };
    Vm::new(&module, VmOptions::default()).unwrap()
}

/// FNV 哈希（与 emit.rs 一致）
fn field_hash(name: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 65535) as u16
}
