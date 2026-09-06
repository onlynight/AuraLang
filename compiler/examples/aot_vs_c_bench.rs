//! AOT vs C 性能对比基准示例
//!
//! 对比 Aura AOT 编译（LLVM 后端）与 C 编译（clang -O2）在相同算法上的性能。
//! 方法：循环嵌入程序内部，单次进程运行计时，消除进程启动开销。
//!
//! 运行：`cargo run --release --example aot_vs_c_bench --features llvm`

use std::time::Instant;

// ─── 基准算法（Aura + C，循环嵌入 main 内部）────────────────────────────

// fib: 每次迭代计算 fib(20)
const FIB_AURA: &str = r#"
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return fib(n - 1) + fib(n - 2)
    }
    fun main(): Int {
        var s = 0
        var i = 0
        while (i < 100) {
            s = s + fib(20)
            i = i + 1
        }
        return s % 1000000
    }
"#;

const FIB_C: &str = r#"
int fib(int n) {
    if (n < 2) return n;
    return fib(n-1) + fib(n-2);
}
int main() {
    int s = 0;
    for (int i = 0; i < 100; i++) s += fib(20);
    return s % 1000000;
}
"#;

// sum: 每次迭代计算 sum(100k)
const SUM_AURA: &str = r#"
    fun sum_one() {
        var s = 0
        var i = 0
        while (i < 100000) {
            s = s + i
            i = i + 1
        }
        return s
    }
    fun main(): Int {
        var total = 0
        var i = 0
        while (i < 100) {
            total = total + sum_one()
            i = i + 1
        }
        return total % 1000000
    }
"#;

const SUM_C: &str = r#"
int sum_one() {
    int s = 0, i = 0;
    while (i < 100000) { s += i; i++; }
    return s;
}
int main() {
    int total = 0;
    for (int i = 0; i < 100; i++) total += sum_one();
    return total % 1000000;
}
"#;

// matmul: 每次迭代计算 100x100x100 三重循环
const MATMUL_AURA: &str = r#"
    fun matmul_one(): Int {
        var result = 0
        var i = 0
        while (i < 100) {
            var j = 0
            while (j < 100) {
                var k = 0
                while (k < 100) {
                    result = result + i * j * k
                    k = k + 1
                }
                j = j + 1
            }
            i = i + 1
        }
        return result
    }
    fun main(): Int {
        var total = 0
        var i = 0
        while (i < 100) {
            total = total + matmul_one()
            i = i + 1
        }
        return total % 1000000
    }
"#;

const MATMUL_C: &str = r#"
int matmul_one() {
    int result = 0, i, j, k;
    for (i = 0; i < 100; i++)
        for (j = 0; j < 100; j++)
            for (k = 0; k < 100; k++)
                result += i * j * k;
    return result;
}
int main() {
    int total = 0;
    for (int i = 0; i < 100; i++) total += matmul_one();
    return total % 1000000;
}
"#;

// ─── 工具函数 ──────────────────────────────────────────────────────────────

fn tool_path(name: &str) -> std::path::PathBuf {
    let home = std::env::var("AURA_LLVM_HOME").ok();
    let bin = if cfg!(target_os = "windows") { format!("{}.exe", name) } else { name.to_string() };
    if let Some(h) = home {
        let p = std::path::Path::new(&h).join("bin").join(&bin);
        if p.exists() {
            return p;
        }
    }
    std::path::PathBuf::from(bin)
}

/// 编译 C 源码，运行一次，返回耗时（ms）
fn bench_c(c_src: &str, label: &str) -> Result<f64, String> {
    use std::process::Command;
    let tmp = std::env::temp_dir().join(format!("aura_bench_c_{}_{}", label, std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let c_path = tmp.join("main.c");
    let exe_path = tmp.join("main");
    std::fs::write(&c_path, c_src).unwrap();

    let out = Command::new(tool_path("clang"))
        .arg(&c_path)
        .arg("-o")
        .arg(&exe_path)
        .arg("-O2")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "clang 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    let start = Instant::now();
    let out = Command::new(&exe_path).output().map_err(|e| e.to_string())?;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(elapsed_ms)
}

/// 编译 Aura AOT 源码，运行一次，返回耗时（ms）
fn bench_aot(src: &str, label: &str) -> Result<f64, String> {
    use std::process::Command;
    let tmp = std::env::temp_dir().join(format!("aura_bench_aot_{}_{}", label, std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    // 生成 LLVM IR
    let codegen = compiler::codegen::aot::AotCodeGenerator::new(
        compiler::codegen::aot::AotOptions::default(),
    );
    let mut lexer = compiler::lexer::Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = compiler::parser::Parser::new(tokens);
    let program = parser.parse_program();
    let hir = compiler::codegen::hir::desugar_program(&program);
    let ir = codegen.generate_ir(&hir).map_err(|e| e.to_string())?;

    let ll_path = tmp.join("main.ll");
    let o_path = tmp.join("main.o");
    let exe_path = tmp.join("main");
    std::fs::write(&ll_path, &ir).unwrap();

    // llc 编译
    let out = Command::new(tool_path("llc"))
        .arg(&ll_path)
        .arg("-o")
        .arg(&o_path)
        .arg("-O2")
        .arg("-filetype=obj")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "llc 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    // 链接
    let out = if cfg!(target_os = "windows") {
        Command::new(tool_path("clang"))
            .arg(&o_path)
            .arg("-o")
            .arg(&exe_path)
            .arg("-Wl,/entry:main")
            .arg("-Wl,/subsystem:console")
            .output()
            .map_err(|e| e.to_string())?
    } else {
        Command::new(tool_path("clang"))
            .arg(&o_path)
            .arg("-o")
            .arg(&exe_path)
            .arg("-O2")
            .output()
            .map_err(|e| e.to_string())?
    };
    if !out.status.success() {
        return Err(format!(
            "链接失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    // 运行一次
    let start = Instant::now();
    let _out = Command::new(&exe_path).output().map_err(|e| e.to_string())?;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(elapsed_ms)
}

/// 运行多次取平均值
fn bench_avg(c_src: &str, aura_src: &str, label: &str, runs: usize) {
    println!("=== {} ===", label);
    let mut c_times = Vec::new();
    let mut aot_times = Vec::new();

    for _ in 0..runs {
        if let Ok(t) = bench_c(c_src, label) {
            c_times.push(t);
        }
        if let Ok(t) = bench_aot(aura_src, label) {
            aot_times.push(t);
        }
    }

    if c_times.is_empty() || aot_times.is_empty() {
        println!("  跳过（编译或运行失败）");
        println!();
        return;
    }

    let c_avg: f64 = c_times.iter().sum::<f64>() / c_times.len() as f64;
    let aot_avg: f64 = aot_times.iter().sum::<f64>() / aot_times.len() as f64;
    let speedup = c_avg / aot_avg;

    println!("  C:   {:.2} ms", c_avg);
    println!("  AOT: {:.2} ms", aot_avg);
    println!("  AOT 速度: {:.1}x vs C", speedup);
    if speedup >= 0.9 {
        println!("  ✅ AOT 达到 C 的 {}% 速度", speedup * 100.0);
    } else {
        println!("  ⚠️  AOT 仅为 C 的 {}% 速度", speedup * 100.0);
    }
    println!();
}

fn main() {
    println!("=== Aura AOT vs C 性能对比基准 ===");
    println!("（每次运行含 100 次内部迭代，消除进程启动开销）\n");
    bench_avg(FIB_C, FIB_AURA, "fib(20) × 100", 5);
    bench_avg(SUM_C, SUM_AURA, "sum(100k) × 100", 5);
    bench_avg(MATMUL_C, MATMUL_AURA, "matmul(100³) × 100", 3);
    println!("Done.");
}
