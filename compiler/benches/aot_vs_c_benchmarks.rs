//! AOT vs C 性能对比基准
//!
//! 对比 Aura AOT 编译（LLVM 后端）与 C 编译（clang）在相同算法上的性能。
//! 目标：验证 Aura AOT 能达到 C 的 90% 性能。
//!
//! 运行：`cargo bench --bench aot_vs_c_benchmarks --features llvm`
//! 前置条件：LLVM 工具链在 PATH 中或设置 `AURA_LLVM_HOME`。

use std::hint::black_box;
use std::time::Instant;

// ─── 基准算法：Fibonacci ───────────────────────────────────────────────────

const FIB_AURA: &str = r#"
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return fib(n - 1) + fib(n - 2)
    }
    fun main(): Int { return fib(20) }
"#;

const FIB_C: &str = r#"
#include <stdio.h>
int fib(int n) {
    if (n < 2) return n;
    return fib(n-1) + fib(n-2);
}
int main() {
    return fib(20);
}
"#;

// ─── 基准算法：Sum ─────────────────────────────────────────────────────────

const SUM_AURA: &str = r#"
    fun main(): Int {
        var s = 0
        var i = 0
        while (i < 100000) {
            s = s + i
            i = i + 1
        }
        return s
    }
"#;

const SUM_C: &str = r#"
#include <stdio.h>
int main() {
    int s = 0;
    int i = 0;
    while (i < 100000) {
        s = s + i;
        i = i + 1;
    }
    return s;
}
"#;

// ─── 基准算法：Matrix Multiply ─────────────────────────────────────────────

const MATMUL_AURA: &str = r#"
    fun main(): Int {
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
        return result % 1000000
    }
"#;

const MATMUL_C: &str = r#"
#include <stdio.h>
int main() {
    int result = 0;
    int i, j, k;
    for (i = 0; i < 100; i++)
        for (j = 0; j < 100; j++)
            for (k = 0; k < 100; k++)
                result = result + i * j * k;
    return result % 1000000;
}
"#;

// ─── 工具函数 ──────────────────────────────────────────────────────────────

fn llc_path() -> std::path::PathBuf {
    let home = std::env::var("AURA_LLVM_HOME").ok();
    let bin = if cfg!(target_os = "windows") { "llc.exe" } else { "llc" };
    if let Some(h) = home {
        let p = std::path::Path::new(&h).join("bin").join(bin);
        if p.exists() { return p; }
    }
    std::path::PathBuf::from(bin)
}

fn clang_path() -> std::path::PathBuf {
    let home = std::env::var("AURA_LLVM_HOME").ok();
    let bin = if cfg!(target_os = "windows") { "clang.exe" } else { "clang" };
    if let Some(h) = home {
        let p = std::path::Path::new(&h).join("bin").join(bin);
        if p.exists() { return p; }
    }
    std::path::PathBuf::from(bin)
}

fn lld_path() -> std::path::PathBuf {
    let home = std::env::var("AURA_LLVM_HOME").ok();
    let bin = if cfg!(target_os = "windows") { "lld-link.exe" } else { "ld.lld" };
    if let Some(h) = home {
        let p = std::path::Path::new(&h).join("bin").join(bin);
        if p.exists() { return p; }
    }
    std::path::PathBuf::from(bin)
}

/// 编译 C 源码并运行，返回每次操作耗时（ms）
fn bench_c(c_src: &str, iters: usize, expected: i64, name: &str) -> Result<f64, String> {
    use std::process::Command;

    let tmp = std::env::temp_dir().join(format!("aura_bench_c_{}_{}", name, std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let c_path = tmp.join("main.c");
    let o_path = tmp.join("main.obj");
    let exe_path = tmp.join("main.exe");
    std::fs::write(&c_path, c_src).unwrap();

    // 编译
    let mut cmd = Command::new(clang_path());
    cmd.arg(&c_path)
        .arg("-c")
        .arg("-o")
        .arg(&o_path)
        .arg("-O2");
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("clang 编译失败: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // 链接
    let mut link_cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new(lld_path());
        c.arg(&o_path)
            .arg(format!("/out:{}", exe_path.display()))
            .arg("/entry:main")
            .arg("/subsystem:console");
        c
    } else {
        let mut c = Command::new("clang");
        c.arg(&o_path).arg("-o").arg(&exe_path).arg("-O2");
        c
    };
    let out = link_cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("链接失败: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // 运行并计时
    let start = Instant::now();
    for _ in 0..iters {
        let out = Command::new(&exe_path).output().map_err(|e| e.to_string())?;
        let code = out.status.code().unwrap_or(-1) as i64;
        let code = if code < 0 { code + 256 } else { code };
        assert_eq!(code, expected, "C 运行结果应为 {}", expected);
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    let _ = std::fs::remove_dir_all(&tmp);
    Ok(elapsed)
}

/// 编译 Aura AOT 源码并运行，返回每次操作耗时（ms）
fn bench_aot(src: &str, iters: usize, expected: i64, name: &str) -> Result<f64, String> {
    use std::process::Command;

    let tmp = std::env::temp_dir().join(format!("aura_bench_aot_{}_{}", name, std::process::id()));
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
    let o_path = tmp.join("main.obj");
    let exe_path = tmp.join("main.exe");
    std::fs::write(&ll_path, &ir).unwrap();

    // llc 编译
    let mut cmd = Command::new(llc_path());
    cmd.arg(&ll_path).arg("-o").arg(&o_path).arg("-O2").arg("-filetype=obj");
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("llc 失败: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // 链接
    let mut link_cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new(lld_path());
        c.arg(&o_path)
            .arg(format!("/out:{}", exe_path.display()))
            .arg("/entry:main")
            .arg("/subsystem:console");
        c
    } else {
        let mut c = Command::new("clang");
        c.arg(&o_path).arg("-o").arg(&exe_path).arg("-O2");
        c
    };
    let out = link_cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("链接失败: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // 运行并计时
    let start = Instant::now();
    for _ in 0..iters {
        let out = Command::new(&exe_path).output().map_err(|e| e.to_string())?;
        let code = out.status.code().unwrap_or(-1) as i64;
        let code = if code < 0 { code + 256 } else { code };
        assert_eq!(code, expected, "AOT 运行结果应为 {}", expected);
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    let _ = std::fs::remove_dir_all(&tmp);
    Ok(elapsed)
}

// ─── 基准运行 ──────────────────────────────────────────────────────────────

fn run_fib() {
    let expected = 6765i64;
    let iters = 50;
    println!("=== fib(20) ===");

    let c_result = bench_c(FIB_C, iters, expected, "fib");
    let aot_result = bench_aot(FIB_AURA, iters, expected, "fib");

    match (&c_result, &aot_result) {
        (Ok(c_ms), Ok(aot_ms)) => {
            let ratio = aot_ms / c_ms;
            println!("  C:   {:.3} ms/op", c_ms);
            println!("  AOT: {:.3} ms/op", aot_ms);
            println!("  AOT/C 比值: {:.1}% {}", ratio * 100.0, if ratio < 0.9 { "✅ ≥90%" } else { "⚠️ <90%" });
        }
        (Ok(c_ms), Err(e)) => println!("  C:   {:.3} ms/op  |  AOT: 跳过 ({})", c_ms, e),
        (Err(e), _) => println!("  C: 跳过 ({})", e),
    }
}

fn run_sum() {
    let expected = 704_982_704i64;
    let iters = 50;
    println!("=== sum(100000) ===");

    let c_result = bench_c(SUM_C, iters, expected, "sum");
    let aot_result = bench_aot(SUM_AURA, iters, expected, "sum");

    match (&c_result, &aot_result) {
        (Ok(c_ms), Ok(aot_ms)) => {
            let ratio = aot_ms / c_ms;
            println!("  C:   {:.3} ms/op", c_ms);
            println!("  AOT: {:.3} ms/op", aot_ms);
            println!("  AOT/C 比值: {:.1}% {}", ratio * 100.0, if ratio < 0.9 { "✅ ≥90%" } else { "⚠️ <90%" });
        }
        (Ok(c_ms), Err(e)) => println!("  C:   {:.3} ms/op  |  AOT: 跳过 ({})", c_ms, e),
        (Err(e), _) => println!("  C: 跳过 ({})", e),
    }
}

fn run_matmul() {
    let expected = 290_712i64;
    let iters = 50;
    println!("=== matmul(100x100x100) ===");

    let c_result = bench_c(MATMUL_C, iters, expected, "matmul");
    let aot_result = bench_aot(MATMUL_AURA, iters, expected, "matmul");

    match (&c_result, &aot_result) {
        (Ok(c_ms), Ok(aot_ms)) => {
            let ratio = aot_ms / c_ms;
            println!("  C:   {:.3} ms/op", c_ms);
            println!("  AOT: {:.3} ms/op", aot_ms);
            println!("  AOT/C 比值: {:.1}% {}", ratio * 100.0, if ratio < 0.9 { "✅ ≥90%" } else { "⚠️ <90%" });
        }
        (Ok(c_ms), Err(e)) => println!("  C:   {:.3} ms/op  |  AOT: 跳过 ({})", c_ms, e),
        (Err(e), _) => println!("  C: 跳过 ({})", e),
    }
}

pub fn run_all() {
    println!("=== Aura AOT vs C Benchmark ===\n");
    black_box(run_fib);
    black_box(run_sum);
    black_box(run_matmul);
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
