//! Aura VM 集成与单元测试
//!
//! 覆盖 技术方案 §7.1 的字节码执行：算术 / 控制流 / 函数调用与递归 / 原生调度 /
//! 对象模型（NewObject / SetField / GetField）/ 数组。

use aura_compiler::codegen::opcode::{
    BytecodeFunction, BytecodeModule, BytecodeNative, Const, OpCode,
};
use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Vm, VmOptions, Value};

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
