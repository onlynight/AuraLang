//! 测试 sum 的 JIT 编译
use compiler::codegen::compile_source;
use compiler::vm::{Vm, VmOptions};

const SUM_SRC: &str = r#"
    fun main(): Int {
        var s = 0
        var i = 0
        while (i < 60000) {
            s = s + i
            i = i + 1
        }
        return s
    }
"#;

fn main() {
    let module = compile_source(SUM_SRC).unwrap();
    let mut vm = Vm::new(
        &module,
        VmOptions {
            jit: true,
            ..Default::default()
        },
    )
    .unwrap();
    let r = vm.run().unwrap();
    println!("结果: {} (期望 1799970000)", r.as_int());
    assert_eq!(r.as_int(), 1_799_970_000i64);
    println!("测试通过!");
}
