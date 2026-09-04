//! Aura VM 集成与单元测试
//!
//! 覆盖 技术方案 §7.1 的字节码执行：算术 / 控制流 / 函数调用与递归 / 原生调度 /
//! 对象模型（NewObject / SetField / GetField）/ 数组。

use compiler::codegen::compile_source;
use compiler::codegen::opcode::{
    BytecodeFunction, BytecodeModule, BytecodeNative, Const, OpCode,
};
use compiler::vm::{Value, Vm, VmOptions};

/// 编译源码并返回 `main` 的执行结果（要求 main 返回一个可断言的值）
fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("编译应成功");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    vm.run().expect("运行应成功")
}

#[test]
fn arithmetic_and_return() {
    let v = run_main("fun main(): Int { return (2 + 3) * 4 - 1 }");
    assert_eq!(v, Value::Int(19));
}

#[test]
fn recursion_fib() {
    let src = r#"
        fun fib(n: Int): Int {
            if (n < 2) { return n }
            return fib(n - 1) + fib(n - 2)
        }
        fun main(): Int { return fib(10) }
    "#;
    assert_eq!(run_main(src), Value::Int(55));
}

#[test]
fn while_loop_sum() {
    let src = r#"
        fun main(): Int {
            var s = 0
            var i = 0
            while (i < 10) {
                s = s + i
                i = i + 1
            }
            return s
        }
    "#;
    assert_eq!(run_main(src), Value::Int(45));
}

#[test]
fn for_loop_and_condition() {
    let src = r#"
        fun main(): Int {
            var total = 0
            for (k in 0..5) {
                if (k % 2 == 0) { total = total + k }
            }
            return total
        }
    "#;
    // 0 + 2 + 4 = 6
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn if_else_expression() {
    let src = r#"
        fun max(a: Int, b: Int): Int { if (a > b) { return a } else { return b } }
        fun main(): Int { return max(3, 7) }
    "#;
    assert_eq!(run_main(src), Value::Int(7));
}

#[test]
fn string_and_bool_ops() {
    let src = r#"
        fun main(): Int {
            val t = true
            val f = false
            if (t && !f) { return 1 } else { return 0 }
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn nested_calls() {
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun mul(a: Int, b: Int): Int { return a * b }
        fun main(): Int { return add(mul(2, 3), mul(4, 5)) }
    "#;
    // 6 + 20 = 26
    assert_eq!(run_main(src), Value::Int(26));
}

#[test]
fn calc_example_runs() {
    // 与 examples/calc.aura 等价的核心计算，验证端到端执行
    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun fib(n: Int): Int {
            if (n < 2) { return n }
            return fib(n - 1) + fib(n - 2)
        }
        fun main(): Int { return fib(6) + add(10, 20) }
    "#;
    assert_eq!(run_main(src), Value::Int(8 + 30));
}

/// 直接构造字节码，验证对象模型（NewObject / SetField / GetField）正确
#[test]
fn object_field_roundtrip() {
    // 字段索引（与 emit 的 field_index 同义：FNV mod 65535），此处只需 Set/Get 一致
    let field_x: u16 = 12345;

    // main():
    //   NewObject(0)
    //   StoreVar(0)            // locals[0] = obj
    //   LoadConst(0)=42
    //   LoadVar(0)=obj
    //   SetField(field_x)
    //   LoadVar(0)
    //   GetField(field_x)
    //   Return
    let mut code = Vec::new();
    OpCode::NewObject(0).write(&mut code);
    OpCode::StoreVar(0).write(&mut code);
    OpCode::LoadConst(0).write(&mut code);
    OpCode::LoadVar(0).write(&mut code);
    OpCode::SetField(field_x).write(&mut code);
    OpCode::LoadVar(0).write(&mut code);
    OpCode::GetField(field_x).write(&mut code);
    OpCode::Return.write(&mut code);

    let module = BytecodeModule {
        consts: vec![Const::Int(42)],
        natives: vec![BytecodeNative {
            name: "println".to_string(),
            param_count: 1,
        }],
        functions: vec![BytecodeFunction {
            name: "main".to_string(),
            param_count: 0,
            locals: 1,
            is_native: false,
            code,
        }],
        entry: 0,
    };

    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    let result = vm.run().expect("运行应成功");
    assert_eq!(result, Value::Int(42));
    // 运行后堆中无泄漏（对象已释放或存活但对象模型自洽）
    assert_eq!(vm.live_objects(), 1);
}

/// 数组分配、索引写入与读取
#[test]
fn array_roundtrip() {
    // main():
    //   LoadConst(0)=3      // length
    //   NewArray
    //   StoreVar(0)         // locals[0]=arr
    //   LoadConst(1)=99     // 待写入值
    //   LoadVar(0)          // arr
    //   LoadConst(2)=1      // index
    //   SetIndex            // arr[1] = 99
    //   LoadVar(0)
    //   LoadConst(2)=1      // index
    //   GetIndex            // arr[1] -> 99
    //   Return
    let mut code = Vec::new();
    OpCode::LoadConst(0).write(&mut code); // length 3
    OpCode::NewArray.write(&mut code);
    OpCode::StoreVar(0).write(&mut code);
    OpCode::LoadConst(1).write(&mut code); // 99
    OpCode::LoadVar(0).write(&mut code);
    OpCode::LoadConst(2).write(&mut code); // index 1
    OpCode::SetIndex.write(&mut code);
    OpCode::LoadVar(0).write(&mut code);
    OpCode::LoadConst(2).write(&mut code);
    OpCode::GetIndex.write(&mut code);
    OpCode::Return.write(&mut code);

    let module = BytecodeModule {
        consts: vec![Const::Int(3), Const::Int(99), Const::Int(1)],
        natives: vec![],
        functions: vec![BytecodeFunction {
            name: "main".to_string(),
            param_count: 0,
            locals: 1,
            is_native: false,
            code,
        }],
        entry: 0,
    };
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    let result = vm.run().expect("运行应成功");
    assert_eq!(result, Value::Int(99));
}

/// JIT 热点编译 + 原生派发：结果必须与解释器一致
#[cfg(feature = "jit")]
#[test]
fn jit_dispatch_matches_interpreter() {
    let src = r#"
        fun sum(n: Int): Int {
            var s = 0
            var i = 0
            while (i < n) {
                s = s + i
                i = i + 1
            }
            return s
        }
        fun main(): Int { return sum(10) }
    "#;
    let module = compile_source(src).expect("编译应成功");
    // 阈值设为 1：首次调用即编译为原生码并走原生派发路径
    let opts = VmOptions {
        jit: true,
        hotspot_threshold: 1,
        ..Default::default()
    };
    let mut vm = Vm::new(&module, opts).expect("VM 初始化");
    assert_eq!(vm.run().expect("运行应成功"), Value::Int(45));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5.6 方法 / 接口调用测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 对象虚方法表查找：alloc_object_with_vtable 后通过 get_vtable_method 查方法
#[test]
fn method_dispatch_via_vtable() {
    use std::collections::HashMap;

    let mut heap = compiler::vm::Heap::new();

    // 构建 vtable：method_idx 0 → func 1
    let mut vtable = HashMap::new();
    vtable.insert(0u16, 1usize);

    // 分配带 vtable 的对象
    let h = heap.alloc_object_with_vtable(42, vtable);
    assert!(h >= 0);

    // 设置字段
    heap.set_field(h, 100, Value::Int(99));

    // 查方法：method_idx 0 应返回 func 1
    let method = heap.get_vtable_method(h, 0);
    assert_eq!(method, Some(1usize));

    // 查不存在的 method_idx
    let method2 = heap.get_vtable_method(h, 99);
    assert_eq!(method2, None);

    // 验证字段仍可读写
    assert_eq!(heap.get_field(h, 100), Value::Int(99));

    // 不含 vtable 的对象应返回 None
    let h2 = heap.alloc_object(0);
    assert_eq!(heap.get_vtable_method(h2, 0), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5.7 集合类型测试
// ═══════════════════════════════════════════════════════════════════════════════

/// List 创建、追加、弹出、长度
#[test]
fn list_operations() {
    // main():
    //   LoadConst(0)=2   NewList          StoreVar(0)  // list
    //   LoadConst(1)=10  LoadVar(0)       ListPush
    //   LoadConst(2)=20  LoadVar(0)       ListPush
    //   LoadVar(0)       ListLen          StoreVar(1)
    //   LoadVar(0)       ListPop          StoreVar(2)
    //   LoadVar(2)       Return
    let mut code = Vec::new();
    OpCode::LoadConst(0).write(&mut code); // capacity 2
    OpCode::NewList.write(&mut code);
    OpCode::StoreVar(0).write(&mut code);
    OpCode::LoadConst(1).write(&mut code); // 10
    OpCode::LoadVar(0).write(&mut code);
    OpCode::ListPush.write(&mut code);
    OpCode::LoadConst(2).write(&mut code); // 20
    OpCode::LoadVar(0).write(&mut code);
    OpCode::ListPush.write(&mut code);
    OpCode::LoadVar(0).write(&mut code);
    OpCode::ListLen.write(&mut code);
    OpCode::StoreVar(1).write(&mut code);
    OpCode::LoadVar(0).write(&mut code);
    OpCode::ListPop.write(&mut code);
    OpCode::StoreVar(2).write(&mut code);
    OpCode::LoadVar(2).write(&mut code);
    OpCode::Return.write(&mut code);

    let module = BytecodeModule {
        consts: vec![Const::Int(2), Const::Int(10), Const::Int(20)],
        natives: vec![],
        functions: vec![BytecodeFunction {
            name: "main".to_string(),
            param_count: 0,
            locals: 3,
            is_native: false,
            code,
        }],
        entry: 0,
    };
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    assert_eq!(vm.run().expect("运行应成功"), Value::Int(20));
}

/// Map 创建、写入、读取、长度
#[test]
fn map_operations() {
    // 栈布局（MapSet）：值在下、键在中、Map 引用在顶
    // 即压栈顺序：LoadConst(value), LoadConst(key), LoadVar(map)
    let mut code = Vec::new();
    OpCode::NewMap.write(&mut code);
    OpCode::StoreVar(0).write(&mut code);
    // MapSet: 压 value(100), key("k1"), map(Ref)
    OpCode::LoadConst(1).write(&mut code); // 100 (value)
    OpCode::LoadConst(0).write(&mut code); // "k1" (key)
    OpCode::LoadVar(0).write(&mut code); // map
    OpCode::MapSet.write(&mut code);
    // MapGet: 压 key("k1"), map(Ref)
    OpCode::LoadConst(0).write(&mut code); // "k1" (key)
    OpCode::LoadVar(0).write(&mut code); // map
    OpCode::MapGet.write(&mut code);
    OpCode::Return.write(&mut code);

    let module = BytecodeModule {
        consts: vec![Const::Str("k1".to_string()), Const::Int(100)],
        natives: vec![],
        functions: vec![BytecodeFunction {
            name: "main".to_string(),
            param_count: 0,
            locals: 1,
            is_native: false,
            code,
        }],
        entry: 0,
    };
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    assert_eq!(vm.run().expect("运行应成功"), Value::Int(100));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5.8 协程调度测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 协程创建与 Yield 挂起
#[test]
fn coroutine_yield_and_resume() {
    use compiler::vm::CoroutineScheduler;

    let mut scheduler = CoroutineScheduler::new();
    assert_eq!(scheduler.active_count(), 0);
    assert_eq!(scheduler.ready_count(), 0);

    // spawn 返回协程 ID（从 1 开始）
    let id = scheduler.spawn(0);
    assert_eq!(id, 1);
    assert_eq!(scheduler.active_count(), 1);
    assert_eq!(scheduler.ready_count(), 1);

    // 保存帧（模拟 Yield）
    use compiler::vm::Frame;
    let frames = vec![Frame {
        func: 0,
        ip: 5,
        locals: vec![Value::Int(42)],
        stack: vec![Value::Int(99)],
        coroutine_id: 1,
    }];
    scheduler.save_frames(id, frames, Value::Int(99));
    assert_eq!(scheduler.ready_count(), 1);

    // 恢复帧
    let restored = scheduler.restore_frames(id).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].locals[0], Value::Int(42));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5.9 动态 FFI 测试
// ═══════════════════════════════════════════════════════════════════════════════

/// DynamicLoader 基础操作
#[test]
fn dynamic_loader_basic() {
    use compiler::vm::DynamicLoader;

    let mut loader = DynamicLoader::new();
    assert_eq!(loader.len(), 0);
    assert!(!loader.contains("foo"));

    // 注册函数
    fn dummy(_args: &[Value]) -> Value {
        Value::Int(42)
    }
    loader.register_func("foo", dummy);
    assert!(loader.contains("foo"));
    assert_eq!(loader.len(), 1);

    let f = loader.get("foo").unwrap();
    assert_eq!(f(&[]), Value::Int(42));
}

/// 动态加载占位（无 dynamic-ffi feature 时应静默成功）
#[test]
fn dynamic_loader_load_lib_noop() {
    use compiler::vm::DynamicLoader;

    let mut loader = DynamicLoader::new();
    // 无 dynamic-ffi 时应返回 Ok（空操作）
    let result = loader.load_lib("nonexistent.so");
    assert!(result.is_ok());
    assert_eq!(loader.len(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5.10 ARC 生命周期测试
// ═══════════════════════════════════════════════════════════════════════════════

/// ARC 引用计数与回收
#[test]
fn arc_reference_counting() {
    let mut heap = compiler::vm::Heap::new();
    let h1 = heap.alloc_object(0);
    assert_eq!(heap.live_count(), 1);

    // 重复引用
    let _h2 = h1; // Clone handle
    heap.inc_ref(h1);
    heap.inc_ref(h1);

    // 减两次，仍存活
    heap.dec_ref(h1);
    heap.dec_ref(h1);
    assert_eq!(heap.live_count(), 1);

    // 减第三次，应回收
    heap.dec_ref(h1);
    assert_eq!(heap.live_count(), 0);
}

/// DropRef 显式释放
#[test]
fn drop_ref_explicit() {
    let mut heap = compiler::vm::Heap::new();
    let h = heap.alloc_object(0);
    heap.set_field(h, 100, Value::Int(99));
    assert_eq!(heap.live_count(), 1);

    heap.drop_ref(h);
    assert_eq!(heap.live_count(), 0);

    // 重复 drop_ref 无害
    heap.drop_ref(h);
    assert_eq!(heap.live_count(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5.14 JIT 编译范围扩展测试（需 jit feature）
// ═══════════════════════════════════════════════════════════════════════════════

/// JIT 可编译性判定：叶子整数函数应返回 true
#[cfg(feature = "jit")]
#[test]
fn jit_compilable_leaf_int() {
    use compiler::vm::{DecodedFunction, Instr};

    let code = vec![
        Instr::LoadConst(0), // Int
        Instr::LoadVar(0),
        Instr::Add,
        Instr::StoreVar(1),
        Instr::LoadVar(1),
        Instr::Return,
    ];
    let f = DecodedFunction {
        name: "add".to_string(),
        param_count: 1,
        locals: 2,
        is_native: false,
        code,
    };
    let consts = vec![Const::Int(1)];
    assert!(compiler::vm::jit::is_jit_compilable(&f, &consts));
}

/// JIT 不可编译：含 Call 指令应返回 false
#[cfg(feature = "jit")]
#[test]
fn jit_not_compilable_with_call() {
    use compiler::vm::{DecodedFunction, Instr};

    let code = vec![
        Instr::LoadConst(0),
        Instr::Call(0), // 非叶子调用
        Instr::Return,
    ];
    let f = DecodedFunction {
        name: "foo".to_string(),
        param_count: 0,
        locals: 1,
        is_native: false,
        code,
    };
    let consts = vec![Const::Int(1)];
    assert!(!compiler::vm::jit::is_jit_compilable(&f, &consts));
}

/// JIT 不可编译：含非常量应返回 false
#[cfg(feature = "jit")]
#[test]
fn jit_not_compilable_non_int_const() {
    use compiler::vm::{DecodedFunction, Instr};

    let code = vec![
        Instr::LoadConst(0), // Float 常量
        Instr::Return,
    ];
    let f = DecodedFunction {
        name: "foo".to_string(),
        param_count: 0,
        locals: 0,
        is_native: false,
        code,
    };
    let consts = vec![Const::Float(3.14)];
    assert!(!compiler::vm::jit::is_jit_compilable(&f, &consts));
}
