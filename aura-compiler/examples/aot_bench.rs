//! AOT vs JIT vs VM 性能对比（6.17）
//!
//! 运行：`cargo run --release --features "llvm,jit" -p aura-compiler --example aot_bench`
//! 需要设置 AURA_LLVM_HOME 指向 LLVM 安装目录

use std::process::Command;
use std::time::Instant;

use aura_compiler::codegen::aot::{AotCodeGenerator, AotOptions};
use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Vm, VmOptions};

const FIB_SRC: &str = r#"
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return fib(n - 1) + fib(n - 2)
    }
    fun main(): Int { return fib(25) }
"#;

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

/// 通过启用 JIT 的 VM 运行计时（需要 jit feature）
///
/// 说明：JIT 热点检测基于累计调用计数（默认阈值 10_000），且 `reset_for_reuse`
/// 会清零计数。为公平对比「长期运行进程中的 JIT 加速」，此处不重置 VM，
/// 让调用计数在多次运行间累计（模拟真实长驻进程），首轮预热后计时。
/// 若热点编译结果与解释器不一致（已知 P5 实验性 JIT 缺陷），返回 NaN 并提示。
#[cfg(feature = "jit")]
fn jit_bench(src: &str, iters: usize, expected: i64) -> f64 {
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions { jit: true, ..Default::default() }).unwrap();
    // 预热：前若干次运行累计调用计数，触发 JIT 编译
    let mut first_err: Option<String> = None;
    for _ in 0..iters / 2 {
        match vm.run() {
            Ok(r) => {
                if r.as_int() != expected {
                    first_err = Some(format!(
                        "JIT 结果不一致: 期望 {} 实际 {} （已知 P5 实验性 JIT 正确性缺陷）",
                        expected, r
                    ));
                }
            }
            Err(e) => {
                first_err = Some(format!("JIT 运行错误: {}", e));
            }
        }
    }
    if let Some(err) = first_err {
        eprintln!("[JIT 不可用] {}", err);
        return f64::NAN;
    }
    // 计时：JIT 已生效（或确认回退解释器）
    let start = Instant::now();
    for _ in 0..iters / 2 {
        let r = vm.run().unwrap();
        assert_eq!(r.as_int(), expected);
    }
    start.elapsed().as_secs_f64() / (iters / 2) as f64
}

/// 未启用 jit feature 时返回 NaN（标记不可用）
#[cfg(not(feature = "jit"))]
fn jit_bench(_src: &str, _iters: usize, _expected: i64) -> f64 {
    f64::NAN
}

fn aot_bench(src: &str, iters: usize, expected: i64) -> Result<f64, String> {
    let llvm_home = std::env::var("AURA_LLVM_HOME").map_err(|_| "未设置 AURA_LLVM_HOME".to_string())?;
    let bin = std::path::Path::new(&llvm_home).join("bin");

    // 生成 LLVM IR
    let codegen = AotCodeGenerator::new(AotOptions::default());
    let mut lexer = aura_compiler::lexer::Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = aura_compiler::parser::Parser::new(tokens);
    let program = parser.parse_program();
    let hir = aura_compiler::codegen::hir::desugar_program(&program);
    let ir = codegen.generate_ir(&hir).map_err(|e| e.to_string())?;

    let tmp = std::env::temp_dir().join(format!("aura_bench_ex_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let ll = tmp.join("b.ll");
    let obj = tmp.join("b.obj");
    let exe = if cfg!(target_os = "windows") {
        tmp.join("b.exe")
    } else {
        tmp.join("b")
    };
    std::fs::write(&ll, &ir).unwrap();

    // llc
    let llc = bin.join({
        if cfg!(target_os = "windows") { "llc.exe" } else { "llc" }
    });
    let out = Command::new(&llc)
        .arg(&ll)
        .arg("-o")
        .arg(&obj)
        .arg("-O3")
        .arg("-filetype=obj")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("llc: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // link
    let mut linker = if cfg!(target_os = "windows") {
        let mut c = Command::new(bin.join("lld-link.exe"));
        c.arg(&obj)
            .arg(format!("/out:{}", exe.display()))
            .arg("/entry:main")
            .arg("/subsystem:console");
        c
    } else {
        let mut c = Command::new("clang");
        c.arg(&obj).arg("-o").arg(&exe).arg("-O3");
        c
    };
    let out = linker.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("link: {}", String::from_utf8_lossy(&out.stderr)));
    }

    // 计时运行（Windows 退出码为 i32，负数补码需转换回无符号值）
    let start = Instant::now();
    for _ in 0..iters {
        let out = Command::new(&exe).output().map_err(|e| e.to_string())?;
        let raw = out.status.code().unwrap_or(-1);
        let code = if raw < 0 { raw as u32 as i64 } else { raw as i64 };
        assert_eq!(code, expected, "AOT 结果应为 {}", expected);
    }
    let dur = start.elapsed().as_secs_f64() / iters as f64;

    let _ = std::fs::remove_dir_all(&tmp);
    Ok(dur)
}

fn main() {
    println!("=== Aura AOT vs JIT vs VM 性能对比基准 (6.17) ===\n");

    // fib(25) = 75025
    println!("--- fib(25) ---");
    let vm = vm_bench(FIB_SRC, 10, 75025);
    println!("  VM(字节码解释器): {:.4} ms/op", vm * 1000.0);
    let jit = jit_bench(FIB_SRC, 12, 75025);
    if jit.is_finite() {
        println!("  VM(JIT 热点编译):  {:.4} ms/op (JIT 加速 {:.1}x vs VM)", jit * 1000.0, vm / jit);
    } else {
        println!("  VM(JIT): 未启用 jit feature，跳过");
    }
    match aot_bench(FIB_SRC, 50, 75025) {
        Ok(aot) => {
            println!("  AOT(LLVM 原生):   {:.4} ms/op", aot * 1000.0);
            println!("  AOT 加速比:        {:.1}x vs VM", vm / aot);
            if jit.is_finite() {
                println!("  AOT vs JIT:        {:.1}x", jit / aot);
            }
        }
        Err(e) => println!("  AOT: 跳过 ({})", e),
    }

    // sum(60_000) = 1_799_970_000
    println!("\n--- sum(60_000) ---");
    let vm = vm_bench(SUM_SRC, 10, 1_799_970_000i64);
    println!("  VM(字节码解释器): {:.4} ms/op", vm * 1000.0);
    let jit = jit_bench(SUM_SRC, 12, 1_799_970_000i64);
    if jit.is_finite() {
        println!("  VM(JIT 热点编译):  {:.4} ms/op (JIT 加速 {:.1}x vs VM)", jit * 1000.0, vm / jit);
    } else {
        println!("  VM(JIT): 未启用 jit feature，跳过");
    }
    match aot_bench(SUM_SRC, 50, 1_799_970_000i64) {
        Ok(aot) => {
            println!("  AOT(LLVM 原生):   {:.4} ms/op", aot * 1000.0);
            println!("  AOT 加速比:        {:.1}x vs VM", vm / aot);
            if jit.is_finite() {
                println!("  AOT vs JIT:        {:.1}x", jit / aot);
            }
        }
        Err(e) => println!("  AOT: 跳过 ({})", e),
    }
}