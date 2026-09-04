//! 简化版 VM vs JIT 性能对比（无需 LLVM）
//!
//! 运行：`cargo run --release --features jit -p aura-compiler --example jit_bench_simple`

use std::time::Instant;

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Vm, VmOptions};

const FIB_SRC: &str = r#"
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return fib(n - 1) + fib(n - 2)
    }
    fun main(): Int { return fib(25) }
"#;

fn vm_bench(src: &str, iters: usize, expected: i64) -> f64 {
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..iters {
        let r = vm.run().unwrap();
        assert_eq!(r.as_int(), expected);
        vm.reset_for_reuse();
    }
    start.elapsed().as_secs_f64() / iters as f64
}

fn jit_bench(src: &str, iters: usize, expected: i64) -> f64 {
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(
        &module,
        VmOptions {
            jit: true,
            ..Default::default()
        },
    )
    .unwrap();
    // 预热：首次运行触发 JIT 编译
    let r = vm.run().unwrap();
    assert_eq!(r.as_int(), expected, "JIT 结果不一致");
    vm.reset_for_reuse();
    // 计时
    let start = Instant::now();
    for _ in 0..iters {
        let r = vm.run().unwrap();
        assert_eq!(r.as_int(), expected);
        vm.reset_for_reuse();
    }
    start.elapsed().as_secs_f64() / iters as f64
}

fn main() {
    println!("=== Aura VM vs JIT 性能对比 ===\n");

    // fib(25) = 75025
    println!("--- fib(25) ---");
    let vm = vm_bench(FIB_SRC, 10, 75025);
    println!("  VM(字节码解释器): {:.4} ms/op", vm * 1000.0);
    let jit = jit_bench(FIB_SRC, 10, 75025);
    println!(
        "  VM(JIT 热点编译):  {:.4} ms/op (JIT 加速 {:.1}x vs VM)",
        jit * 1000.0,
        vm / jit
    );
}
