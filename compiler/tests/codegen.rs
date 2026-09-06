//! P4 字节码编译器集成测试

use compiler::codegen::{compile_source, disassemble, from_bytes, to_bytes};

/// 编译 → 序列化 → 反序列化 往返一致性
#[test]
fn test_roundtrip() {
    let src = "fun add(a: Int, b: Int): Int = a + b\nfun main() { println(add(2, 3)) }";
    let module = compile_source(src).expect("应编译成功");
    let bytes = to_bytes(&module);
    let back = from_bytes(&bytes).expect("应反序列化成功");
    assert_eq!(module, back, "往返序列化应一致");
}

/// 常量折叠：1 + 2 * 3 折叠为 7（出现在常量池中）
#[test]
fn test_constant_folding() {
    let src = "fun main() { val x = 1 + 2 * 3\n println(x) }";
    let module = compile_source(src).expect("编译成功");
    assert!(
        module.consts.iter().any(|c| matches!(c, compiler::codegen::opcode::Const::Int(7))),
        "常量折叠后应包含 7"
    );
}

/// 控制流：while/if 生成跳转指令
#[test]
fn test_control_flow_codegen() {
    let src = "fun main() { var i = 0\n while (i < 3) { println(i)\n i = i + 1 } }";
    let module = compile_source(src).expect("编译成功");
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
    let module = compile_source(src).expect("编译成功");
    let text = disassemble(&module);
    assert!(text.contains("CALL") && text.contains("ADD") && text.contains("RETURN"));
}

/// 泛型单态化：identity<T> 被特化为 identity#1
#[test]
fn test_monomorphization() {
    let src = "fun <T> identity(x: T): T = x\nfun main() { println(identity(5)) }";
    let module = compile_source(src).expect("编译成功");
    assert!(
        module.functions.iter().any(|f| f.name == "identity#1"),
        "泛型函数应被单态化为 identity#1"
    );
}

/// 代表性程序端到端编译并生成入口
#[test]
fn test_full_program() {
    let src = "fun add(a: Int, b: Int): Int = a + b\n\
               fun main() { var i = 0\n while (i < 5) { println(add(i, 1))\n i = i + 1 } }";
    let module = compile_source(src).expect("编译成功");
    assert!(module.entry < module.functions.len() as u16);
    assert_eq!(module.functions[module.entry as usize].name, "main");
}
