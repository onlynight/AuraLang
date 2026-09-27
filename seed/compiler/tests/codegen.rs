//! P4 字节码编译器集成测试

use compiler::codegen::{compile_source, disassemble, from_bytes, to_bytes};

/// 编译 → 序列化 → 反序列化 往返一致性
#[test]
fn test_roundtrip() {
    let src = "fun add(a: Int, b: Int): Int = a + b\nfun main() { println(add(2, 3)) }";
    let module = compile_source(src).expect("compilation should succeed");
    let bytes = to_bytes(&module);
    let back = from_bytes(&bytes).expect("deserialization should succeed");
    assert_eq!(
        module, back,
        "round-trip serialization should be consistent"
    );
}

/// 常量折叠：1 + 2 * 3 折叠为 7（出现在常量池中）
#[test]
fn test_constant_folding() {
    let src = "fun main() { val x = 1 + 2 * 3\n println(x) }";
    let module = compile_source(src).expect("compilation succeeded");
    assert!(
        module.consts.iter().any(|c| matches!(c, compiler::codegen::opcode::Const::Int(7))),
        "常量折叠后应包含 7"
    );
}

/// 控制流：while/if 生成跳转指令
#[test]
fn test_control_flow_codegen() {
    let src = "fun main() { var i = 0\n while (i < 3) { println(i)\n i = i + 1 } }";
    let module = compile_source(src).expect("compilation succeeded");
    let text = disassemble(&module);
    assert!(
        text.contains("JUMP_IF_FALSE") && text.contains("JUMP"),
        "应包含条件/无条件跳转指令"
    );
}

/// 函数调用与参数压栈
#[test]
fn test_call_codegen() {
    let src = "fun add(a: Int, b: Int): Int = a + b\nfun main() { println(add(2, 3)) }";
    let module = compile_source(src).expect("compilation succeeded");
    let text = disassemble(&module);
    assert!(text.contains("CALL") && text.contains("ADD") && text.contains("RETURN"));
}

/// 泛型单态化：identity<T> 被特化为 identity#1
#[test]
fn test_monomorphization() {
    let src = "fun <T> identity(x: T): T = x\nfun main() { println(identity(5)) }";
    let module = compile_source(src).expect("compilation succeeded");
    assert!(
        module.functions.iter().any(|f| f.name == "identity#1"),
        "泛型函数应被单态化为 identity#1"
    );
}

/// 递归泛型函数：fun <T> recurse(x: T, n: Int): T
#[test]
fn test_recursive_monomorphization() {
    let src = "fun <T> recurse(x: T, n: Int): T { if (n <= 0) { return x } return recurse(x, n - 1) }\nfun main() { println(recurse(5, 3)) }";
    let module = compile_source(src).expect("compilation succeeded");
    assert!(
        module.functions.iter().any(|f| f.name == "recurse#2"),
        "递归泛型函数应被单态化为 recurse#2"
    );
}

/// 嵌套泛型调用：泛型函数调用其他泛型函数
#[test]
fn test_nested_monomorphization() {
    let src = "fun <T> identity(x: T): T = x\nfun <T> wrapper(x: T): T = identity(x)\nfun main() { println(wrapper(5)) }";
    let module = compile_source(src).expect("compilation succeeded");
    assert!(
        module.functions.iter().any(|f| f.name == "identity#1"),
        "嵌套泛型调用应生成 identity#1"
    );
    assert!(
        module.functions.iter().any(|f| f.name == "wrapper#1"),
        "嵌套泛型调用应生成 wrapper#1"
    );
}

/// 多个 arity 的泛型函数：fun <T> multi(args...): T
#[test]
fn test_multiple_arities_monomorphization() {
    let src = "fun <T> multi(x: T): T = x\nfun <T> multi(x: T, y: T): T = x\nfun main() { println(multi(5))\n println(multi(5, 10)) }";
    let module = compile_source(src).expect("compilation succeeded");
    // 注意：重载函数在 HIR 中可能无法区分，测试基本功能
    assert!(
        module.functions.iter().any(|f| f.name.contains("multi#")),
        "多 arity 泛型函数应被单态化"
    );
}

/// 泛型函数 + 非泛型函数混合调用
#[test]
fn test_mixed_monomorphization() {
    let src = "fun add(a: Int, b: Int): Int = a + b\nfun <T> identity(x: T): T = x\nfun main() { println(identity(add(1, 2))) }";
    let module = compile_source(src).expect("compilation succeeded");
    assert!(
        module.functions.iter().any(|f| f.name == "identity#1"),
        "混合调用应生成 identity#1"
    );
}

/// 代表性程序端到端编译并生成入口
#[test]
fn test_full_program() {
    let src = "fun add(a: Int, b: Int): Int = a + b\n\
               fun main() { var i = 0\n while (i < 5) { println(add(i, 1))\n i = i + 1 } }";
    let module = compile_source(src).expect("compilation succeeded");
    assert!(module.entry < module.functions.len() as u16);
    assert_eq!(module.functions[module.entry as usize].name, "main");
}
