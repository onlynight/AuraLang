//! Aura VM 性能基准（5.15）
//!
//! 测量解释器执行速度、函数调用开销、对象分配、集合操作等关键路径。
//!
//! 运行：`cargo bench --bench vm_benchmarks`

use std::hint::black_box;
use std::time::Instant;

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Vm, VmOptions, Value};

fn bench_fib() {
    let src = r#"
        fun fib(n: Int): Int {
            if (n < 2) { return n }
            return fib(n - 1) + fib(n - 2)
        }
        fun main(): Int { return fib(20) }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        let result = vm.run().unwrap();
        assert_eq!(result, Value::Int(6765));
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!("fib(20) x100: {:.3} ms/op", elapsed.as_secs_f64() * 1000.0 / 100.0);
}

fn bench_sum_loop() {
    let src = r#"
        fun main(): Int {
            var s = 0
            var i = 0
            while (i < 10000) {
                s = s + i
                i = i + 1
            }
            return s
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    let mut total = 0i64;
    for _ in 0..100 {
        let result = vm.run().unwrap();
        total += result.as_int();
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!("sum(10000) x100: {:.3} ms/op", elapsed.as_secs_f64() * 1000.0 / 100.0);
    assert_eq!(total, 100 * 49_995_000);
}

fn bench_recursive_count() {
    let src = r#"
        fun count(n: Int): Int {
            if (n == 0) { return 1 }
            return count(n - 1) + 1
        }
        fun main(): Int { return count(100) }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        let result = vm.run().unwrap();
        assert_eq!(result.as_int(), 101);
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!("count(100) x100: {:.3} ms/op", elapsed.as_secs_f64() * 1000.0 / 100.0);
}

fn bench_factorial() {
    let src = r#"
        fun fact(n: Int): Int {
            if (n <= 1) { return 1 }
            return n * fact(n - 1)
        }
        fun main(): Int { return fact(20) }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..100 {
        let result = vm.run().unwrap();
        assert_eq!(result.as_int(), 2_432_902_008_176_640_000);
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!("fact(20) x100: {:.3} ms/op", elapsed.as_secs_f64() * 1000.0 / 100.0);
}

/// 运行所有基准测试
pub fn run_all() {
    println!("=== Aura VM Benchmarks ===\n");
    black_box(bench_fib);
    black_box(bench_sum_loop);
    black_box(bench_recursive_count);
    black_box(bench_factorial);
    println!("\nDone.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_benchmarks() {
        run_all();
    }
}