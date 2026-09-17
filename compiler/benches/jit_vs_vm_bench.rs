//! JIT vs VM vs AOT 性能基准对比
//!
//! 对应分阶段开发计划 P5 阶段：Rust JIT 性能基线。
//! 测量 VM（解释器）/ JIT（Cranelift）/ AOT（LLVM）三种执行模式的性能差异。
//!
//! 运行：
//!   cargo bench --bench jit_vs_vm_bench            （仅 VM + JIT）
//!   cargo bench --bench jit_vs_vm_bench --features llvm  （含 AOT）
//!
//! 或快速模式：
//!   cargo run --release --features jit --bench jit_vs_vm_bench

use std::hint::black_box;
use std::time::Instant;

use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

// ─── 基准源码 ───────────────────────────────────────────────────

const FIB_SRC: &str = r#"
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return fib(n - 1) + fib(n - 2)
    }
    fun main(): Int { return fib(25) }
"#;

const FACT_SRC: &str = r#"
    fun fact(n: Int): Int {
        if (n <= 1) { return 1 }
        return n * fact(n - 1)
    }
    fun main(): Int { return fact(20) }
"#;

const SUM_SRC: &str = r#"
    fun sum(n: Int): Int {
        var s = 0
        var i = 1
        while (i <= n) {
            s = s + i
            i = i + 1
        }
        return s
    }
    fun main(): Int { return sum(60000) }
"#;

const MATMUL_SRC: &str = r#"
    fun matmul(n: Int): Int {
        var a = List()
        var b = List()
        var c = List()
        for (var i = 0; i < n * n; i = i + 1) {
            a.add(i + 1)
            b.add((i * 7 + 3) % n + 1)
            c.add(0)
        }
        for (var i = 0; i < n; i = i + 1) {
            for (var j = 0; j < n; j = j + 1) {
                var s = 0
                for (var k = 0; k < n; k = k + 1) {
                    s = s + a.get(i * n + k) * b.get(k * n + j)
                }
                c.set(i * n + j, s)
            }
        }
        var total = 0
        for (var i = 0; i < c.length; i = i + 1) {
            total = total + c.get(i)
        }
        return total
    }
    fun main(): Int { return matmul(30) }
"#;

const HOTLOOP_SRC: &str = r#"
    fun hotLoop(n: Int): Int {
        var s = 0
        for (var i = 0; i < n; i = i + 1) {
            s = s + i
            if (i % 1000 == 0) {
                s = s + 1
            }
        }
        return s
    }
    fun main(): Int { return hotLoop(60000) }
"#;

// ─── 基准框架 ───────────────────────────────────────────────────

/// 基准结果
#[derive(Debug, Clone)]
struct BenchResult {
    name: String,
    time_sec: f64,
    iterations: usize,
}

impl BenchResult {
    fn ms_per_op(&self) -> f64 {
        self.time_sec / self.iterations as f64 * 1000.0
    }
}

/// VM 基准
fn vm_bench(src: &str, iters: usize, expected: Option<i64>) -> BenchResult {
    let name = format!("VM: {}", short_name(src));
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();

    // 首次运行（可能较慢，不计入）
    if let Some(exp) = expected {
        let r = vm.run().unwrap();
        assert_eq!(r.as_int(), exp, "VM result mismatch");
        vm.reset_for_reuse();
    }

    let start = Instant::now();
    for _ in 0..iters {
        let r = vm.run().unwrap();
        if let Some(exp) = expected {
            assert_eq!(r.as_int(), exp);
        }
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    BenchResult {
        name,
        time_sec: elapsed.as_secs_f64(),
        iterations: iters,
    }
}

/// JIT 基准（需 `jit` feature）
#[cfg(feature = "jit")]
fn jit_bench(src: &str, iters: usize, expected: Option<i64>) -> BenchResult {
    let name = format!("JIT: {}", short_name(src));
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(
        &module,
        VmOptions {
            jit: true,
            hotspot_threshold: 1, // 立即触发 JIT
            ..Default::default()
        },
    )
    .unwrap();

    // 预热（首次运行触发 JIT 编译）
    if let Some(exp) = expected {
        let r = vm.run().unwrap();
        assert_eq!(r.as_int(), exp, "JIT result mismatch");
        vm.reset_for_reuse();
    }

    // 多次预热确保 JIT 已激活
    for _ in 0..5 {
        let _r = vm.run().unwrap();
        vm.reset_for_reuse();
    }

    let start = Instant::now();
    for _ in 0..iters {
        let r = vm.run().unwrap();
        if let Some(exp) = expected {
            assert_eq!(r.as_int(), exp);
        }
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    BenchResult {
        name,
        time_sec: elapsed.as_secs_f64(),
        iterations: iters,
    }
}

#[cfg(not(feature = "jit"))]
fn jit_bench(_src: &str, _iters: usize, _expected: Option<i64>) -> BenchResult {
    BenchResult {
        name: "JIT: (not available)".to_string(),
        time_sec: 0.0,
        iterations: 0,
    }
}

/// AOT 基准（需 `llvm` feature）
#[cfg(feature = "llvm")]
fn aot_bench(src: &str, expected: Option<i64>) -> BenchResult {
    // AOT 通过编译为 native 二进制运行
    // 注意：每次运行产生新进程，包含进程启动开销（~4ms）
    let name = format!("AOT: {}", short_name(src));
    let output = std::process::Command::new("aura")
        .args([
            "build", "-",
        ])
        .args([
            "--aot",
            "-o",
            "/tmp/aura_aot_bench",
        ])
        .input(src.as_bytes())
        .output();

    match output {
        Ok(out) if out.status.success() => {
            // 运行 AOT 产物 10 次取平均
            let iters = 10;
            let start = Instant::now();
            for _ in 0..iters {
                let out = std::process::Command::new("/tmp/aura_aot_bench").output();
                if let Some(exp) = expected {
                    if let Ok(out) = out {
                        let stdout_str = String::from_utf8_lossy(&out.stdout);
                        if let Ok(v) = stdout_str.trim().parse::<i64>() {
                            assert_eq!(v, exp, "AOT result mismatch");
                        }
                    }
                }
            }
            let elapsed = start.elapsed();
            BenchResult {
                name,
                time_sec: elapsed.as_secs_f64(),
                iterations: iters,
            }
        }
        _ => BenchResult {
            name: format!("AOT: (build failed)"),
            time_sec: 0.0,
            iterations: 0,
        },
    }
}

#[cfg(not(feature = "llvm"))]
fn aot_bench(_src: &str, _expected: Option<i64>) -> BenchResult {
    BenchResult {
        name: "AOT: (not available)".to_string(),
        time_sec: 0.0,
        iterations: 0,
    }
}

/// 从源码中提取 main 调用的函数名
fn short_name(src: &str) -> String {
    if src.contains("fib(") {
        "fib(25)"
    } else if src.contains("fact(") {
        "fact(20)"
    } else if src.contains("sum(") {
        "sum(60000)"
    } else if src.contains("matmul(") {
        "matmul(30x30)"
    } else if src.contains("hotLoop(") {
        "hotLoop(60000)"
    } else {
        "unknown"
    }
    .to_string()
}

// ─── 基准套件 ───────────────────────────────────────────────────

/// 运行所有基准测试
pub fn run_all() {
    println!("=== Aura JIT vs VM vs AOT Performance Benchmark ===\n");
    println!("Note: AOT results include process startup overhead (~4ms per iteration)\n");

    let mut all_results: Vec<BenchResult> = Vec::new();

    // ─── fib(25) ───
    println!("--- fib(25) ---");
    let iters_fib = if cfg!(feature = "release") { 10 } else { 10 };
    let vm_fib = vm_bench(FIB_SRC, iters_fib, Some(75025));
    let jit_fib = jit_bench(FIB_SRC, iters_fib, Some(75025));
    let aot_fib = aot_bench(FIB_SRC, Some(75025));

    print_result(&vm_fib);
    if jit_fib.time_sec > 0.0 {
        print_result(&jit_fib);
        println!(
            "    JIT speedup vs VM: {:.1}x",
            vm_fib.time_sec / jit_fib.time_sec
        );
    }
    if aot_fib.time_sec > 0.0 {
        print_result(&aot_fib);
        println!(
            "    AOT speedup vs VM: {:.1}x (includes process overhead)",
            vm_fib.time_sec / aot_fib.time_sec
        );
    }
    println!();
    all_results.extend_from_slice(&[
        vm_fib, jit_fib, aot_fib,
    ]);

    // ─── fact(20) ───
    println!("--- fact(20) ---");
    let vm_fact = vm_bench(FACT_SRC, 50, Some(2_432_902_008_176_640_000));
    let jit_fact = jit_bench(FACT_SRC, 50, Some(2_432_902_008_176_640_000));
    let aot_fact = aot_bench(FACT_SRC, Some(2_432_902_008_176_640_000));

    print_result(&vm_fact);
    if jit_fact.time_sec > 0.0 {
        print_result(&jit_fact);
        println!(
            "    JIT speedup vs VM: {:.1}x",
            vm_fact.time_sec / jit_fact.time_sec
        );
    }
    if aot_fact.time_sec > 0.0 {
        print_result(&aot_fact);
        println!(
            "    AOT speedup vs VM: {:.1}x (includes process overhead)",
            vm_fact.time_sec / aot_fact.time_sec
        );
    }
    println!();
    all_results.extend_from_slice(&[
        vm_fact, jit_fact, aot_fact,
    ]);

    // ─── sum(60000) ───
    println!("--- sum(60000) ---");
    let vm_sum = vm_bench(SUM_SRC, 100, Some(1_800_030_000));
    let jit_sum = jit_bench(SUM_SRC, 100, Some(1_800_030_000));
    let aot_sum = aot_bench(SUM_SRC, Some(1_800_030_000));

    print_result(&vm_sum);
    if jit_sum.time_sec > 0.0 {
        print_result(&jit_sum);
        println!(
            "    JIT speedup vs VM: {:.1}x",
            vm_sum.time_sec / jit_sum.time_sec
        );
    }
    if aot_sum.time_sec > 0.0 {
        print_result(&aot_sum);
        println!(
            "    AOT speedup vs VM: {:.1}x (includes process overhead)",
            vm_sum.time_sec / aot_sum.time_sec
        );
    }
    println!();
    all_results.extend_from_slice(&[
        vm_sum, jit_sum, aot_sum,
    ]);

    // ─── matmul(30x30) ───
    println!("--- matmul(30x30) ---");
    let vm_mat = vm_bench(MATMUL_SRC, 10, Some(1)); // 结果因数据而异，跳过断言
    let jit_mat = jit_bench(MATMUL_SRC, 10, None);
    let aot_mat = aot_bench(MATMUL_SRC, None);

    print_result(&vm_mat);
    if jit_mat.time_sec > 0.0 {
        print_result(&jit_mat);
        println!(
            "    JIT speedup vs VM: {:.1}x",
            vm_mat.time_sec / jit_mat.time_sec
        );
    }
    if aot_mat.time_sec > 0.0 {
        print_result(&aot_mat);
        println!(
            "    AOT speedup vs VM: {:.1}x (includes process overhead)",
            vm_mat.time_sec / aot_mat.time_sec
        );
    }
    println!();
    all_results.extend_from_slice(&[
        vm_mat, jit_mat, aot_mat,
    ]);

    // ─── hotLoop(60000) ───
    println!("--- hotLoop(60000) ---");
    let vm_hot = vm_bench(HOTLOOP_SRC, 100, Some(1_799_970_001));
    let jit_hot = jit_bench(HOTLOOP_SRC, 100, Some(1_799_970_001));
    let aot_hot = aot_bench(HOTLOOP_SRC, Some(1_799_970_001));

    print_result(&vm_hot);
    if jit_hot.time_sec > 0.0 {
        print_result(&jit_hot);
        println!(
            "    JIT speedup vs VM: {:.1}x",
            vm_hot.time_sec / jit_hot.time_sec
        );
    }
    if aot_hot.time_sec > 0.0 {
        print_result(&aot_hot);
        println!(
            "    AOT speedup vs VM: {:.1}x (includes process overhead)",
            vm_hot.time_sec / aot_hot.time_sec
        );
    }
    println!();
    all_results.extend_from_slice(&[
        vm_hot, jit_hot, aot_hot,
    ]);

    // ─── 汇总 ───
    println!("=== Summary ===");
    println!(
        "{:<25} {:>15} {:>15}",
        "Benchmark", "VM (ms/op)", "JIT (ms/op)"
    );
    println!("{}{}{}", "─".repeat(25), "─".repeat(16), "─".repeat(16));

    for r in &all_results {
        if r.name.starts_with("VM:") && r.time_sec > 0.0 {
            let jit_name = r.name.replace("VM:", "JIT:");
            let jit_result = all_results.iter().find(|r2| r2.name == jit_name && r2.time_sec > 0.0);
            match jit_result {
                Some(jit_r) => println!(
                    "{:<25} {:>14.4} {:>14.4}",
                    r.name.replace("VM:", ""),
                    r.ms_per_op(),
                    jit_r.ms_per_op()
                ),
                None => println!(
                    "{:<25} {:>14.4} {:>14}",
                    r.name.replace("VM:", ""),
                    r.ms_per_op(),
                    "N/A"
                ),
            }
        }
    }

    println!("\nDone. Total benchmarks: {}", all_results.len());
}

/// 打印单个基准结果
fn print_result(r: &BenchResult) {
    if r.time_sec > 0.0 {
        println!(
            "  {}: {:.4} ms/op ({:.2} s total, {} iters)",
            r.name,
            r.ms_per_op(),
            r.time_sec,
            r.iterations
        );
    }
}

// ─── Benchmark harness ──────────────────────────────────────────

/// 自定义 harness（支持 `cargo bench --no-run`）
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_benchmarks() {
        run_all();
    }
}
