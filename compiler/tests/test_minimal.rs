use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

#[test]
fn test_is_check() {
    let src = r#"
        fun main(): Int {
            if (1 is Int) { return 1 }
            return 0
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let result = vm.run().unwrap();
    assert_eq!(result, Value::Int(1));
}

#[test]
fn test_as_cast() {
    let src = r#"
        fun main(): Int {
            return (3.14f as Int) + 100
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let result = vm.run().unwrap();
    assert_eq!(result, Value::Int(103));
}
