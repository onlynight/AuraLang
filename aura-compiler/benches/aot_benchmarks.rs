//! AOT vs JIT vs 字节码性能对比基准（6.17）
//!
//! 对比同一种算法在三条执行路径上的耗时：
//! - 字节码解释器（VM）
//! - JIT 编译（VM + Cranelift，如果启用）
//! - AOT 原生编译（LLVM 后端生成的二进制）
//!
//! 运行：`cargo bench --bench aot_benchmarks --features llvm`
//! 注意：AOT 部分需要设置 `AURA_LLVM_HOME` 或 LLVM 在 PATH 中。

use std::hint::black_box;
use std::time::Instant;

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Value, Vm, VmOptions};

const FIB_SRC: &str = r#"
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return fib(n - 1) + fib(n - 2)
    }
    fun main(): Int { return fib(20) }
"#;

const SUM_SRC: &str = r#"
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

/// 通过字节码 VM 运行并计时
fn bench_vm(src: &str, iters: usize, expected: i64) -> f64 {
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..iters {
        let result = vm.run().unwrap();
        assert_eq!(result.as_int(), expected);
        vm.reset_for_reuse();
    }
    start.elapsed().as_secs_f64() * 1000.0 / iters as f64
}

/// 通过 JIT VM 运行并计时（如果启用 jit feature）
#[cfg(feature = "jit")]
fn bench_jit(src: &str, iters: usize, expected: i64) -> f64 {
    let module = compile_source(src).unwrap();
    let opts = VmOptions {
        jit: true,
        ..Default::default()
    };
    let mut vm = Vm::new(&module, opts).unwrap();
    let start = Instant::now();
    for _ in 0..iters {
        let result = vm.run().unwrap();
        assert_eq!(result.as_int(), expected);
        vm.reset_for_reuse();
    }
    start.elapsed().as_secs_f64() * 1000.0 / iters as f64
}

/// 没有 jit feature 时跳过
#[cfg(not(feature = "jit"))]
fn bench_jit(_src: &str, _iters: usize, _expected: i64) -> f64 {
    f64::NAN
}

/// 通过 AOT 编译并运行（需要 LLVM）
#[cfg(feature = "llvm")]
fn bench_aot(src: &str, iters: usize, expected: i64) -> Result<f64, String> {
    use std::process::Command;

    let llvm_home = std::env::var("AURA_LLVM_HOME").ok().or_else(|| {
        // 尝试从 PATH 寻找 llc
        None
    });

    // 生成 LLVM IR
    let codegen = aura_compiler::codegen::aot::AotCodeGenerator::new(
        aura_compiler::codegen::aot::AotOptions::default(),
    );
    let mut lexer = aura_compiler::lexer::Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = aura_compiler::parser::Parser::new(tokens);
    let program = parser.parse_program();
    let hir = aura_compiler::codegen::hir::desugar_program(&program);
    let ir = codegen.generate_ir(&hir).map_err(|e| e.to_string())?;

    let tmp = std::env::temp_dir().join(format!("aura_bench_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let ll_path = tmp.join("main.ll");
    let o_path = tmp.join("main.obj");
    let exe_path = tmp.join("main.exe");
    std::fs::write(&ll_path, &ir).unwrap();

    // 调用 llc
    let mut cmd = Command::new(llc_path(llvm_home.as_deref()));
    cmd.arg(&ll_path)
        .arg("-o")
        .arg(&o_path)
        .arg("-O2")
        .arg("-filetype=obj");
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "llc 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    // 调用 lld-link / clang
    let mut link_cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new(
            std::env::var("AURA_LLVM_HOME")
                .map(|h| std::path::Path::new(&h).join("bin").join("lld-link.exe"))
                .unwrap_or_else(|_| "lld-link".into()),
        );
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
        return Err(format!(
            "链接失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

    // 运行多次并计时
    let start = Instant::now();
    for _ in 0..iters {
        let out = Command::new(&exe_path)
            .output()
            .map_err(|e| e.to_string())?;
        let code = out.status.code().unwrap_or(-1) as i64;
        // 在 Windows 上，进程退出码是 i32；将负数转回正确值
        let code = if code < 0 { code + 256 } else { code };
        assert_eq!(code, expected, "AOT 运行结果应为 {}", expected);
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0 / iters as f64;

    let _ = std::fs::remove_dir_all(&tmp);
    Ok(elapsed)
}

#[cfg(feature = "llvm")]
fn llc_path(home: Option<&str>) -> std::path::PathBuf {
    if let Some(h) = home {
        let p = std::path::Path::new(h).join("bin").join({
            if cfg!(target_os = "windows") {
                "llc.exe"
            } else {
                "llc"
            }
        });
        if p.exists() {
            return p;
        }
    }
    if cfg!(target_os = "windows") {
        std::path::PathBuf::from("llc.exe")
    } else {
        std::path::PathBuf::from("llc")
    }
}

#[cfg(not(feature = "llvm"))]
fn bench_aot(_src: &str, _iters: usize, _expected: i64) -> Result<f64, String> {
    Err("未启用 llvm feature".to_string())
}

fn run_fib() {
    let expected = 6765;
    let iters = 50;

    let vm_ms = bench_vm(FIB_SRC, iters, expected);
    println!("[fib(20)] VM(bytecode):   {:.3} ms/op", vm_ms);

    #[cfg(feature = "jit")]
    {
        let jit_ms = bench_jit(FIB_SRC, iters, expected);
        println!("[fib(20)] VM(JIT):        {:.3} ms/op", jit_ms);
    }

    #[cfg(feature = "llvm")]
    {
        match bench_aot(FIB_SRC, iters, expected) {
            Ok(aot_ms) => {
                println!("[fib(20)] AOT(native):     {:.3} ms/op", aot_ms);
                println!("[fib(20)] AOT 加速比:      {:.1}x vs VM", vm_ms / aot_ms);
            }
            Err(e) => println!("[fib(20)] AOT: 跳过 ({})", e),
        }
    }
}

fn run_sum() {
    let expected = 4_999_950_000i64;
    let iters = 50;

    let vm_ms = bench_vm(SUM_SRC, iters, expected);
    println!("[sum(100000)] VM(bytecode): {:.3} ms/op", vm_ms);

    #[cfg(feature = "llvm")]
    {
        match bench_aot(SUM_SRC, iters, expected) {
            Ok(aot_ms) => {
                println!("[sum(100000)] AOT(native):  {:.3} ms/op", aot_ms);
                println!("[sum(100000)] AOT 加速比:   {:.1}x vs VM", vm_ms / aot_ms);
            }
            Err(e) => println!("[sum(100000)] AOT: 跳过 ({})", e),
        }
    }
}

/// 运行所有 AOT 对比基准
pub fn run_all() {
    println!("=== Aura AOT vs VM Benchmark ===\n");
    black_box(run_fib);
    black_box(run_sum);
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
