//! AOT vs JIT vs VM 完整性能对比基准
//!
//! 运行：
//!   VM + JIT: `cargo run --release --features jit -p compiler --example full_bench`
//!   AOT 需要: 设置 AURA_LLVM_HOME 并加 --features "llvm,jit"

#[cfg(feature = "llvm")]
use std::path::Path;
use std::time::Instant;

use compiler::codegen::compile_source;
use compiler::vm::{Vm, VmOptions};

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

const FIB_EXPECTED: i64 = 75025;
const SUM_EXPECTED: i64 = 1_799_970_000;

/// 生成在进程内循环运行 N 次的 AOT 源码
fn aot_loop_src(fib: bool, iters: i64) -> String {
    if fib {
        format!(
            r#"
    fun fib(n: Int): Int {{
        if (n < 2) {{ return n }}
        return fib(n - 1) + fib(n - 2)
    }}
    fun main(): Int {{
        var r = 0
        var i = 0
        while (i < {}) {{
            r = r + fib(25)
            i = i + 1
        }}
        return r
    }}
"#,
            iters
        )
    } else {
        format!(
            r#"
    fun sum_loop(): Int {{
        var s = 0
        var i = 0
        while (i < 60000) {{
            s = s + i
            i = i + 1
        }}
        return s
    }}
    fun main(): Int {{
        var r = 0
        var i = 0
        while (i < {}) {{
            r = r + sum_loop()
            i = i + 1
        }}
        return r
    }}
"#,
            iters
        )
    }
}

/// AOT 不启动新进程的基准：可执行文件内部循环 N 次，只测量执行时间
///
/// 原理：
/// 1. 生成一个 main() 函数，内部循环调用目标函数 N 次
/// 2. 编译为 AOT 可执行文件
/// 3. 启动一次可执行文件，测量总时间
/// 4. 减去进程启动时间，得到纯执行时间
/// 5. 纯执行时间 / N = 每次操作的时间（不含进程启动开销）
#[cfg(feature = "llvm")]
fn aot_inprocess_bench(src: &str, iters: usize, expected: i64) -> Option<f64> {
    use std::process::Command;

    let llvm_home = match std::env::var("AURA_LLVM_HOME") {
        Ok(v) => v,
        Err(_) => return None,
    };
    let bin = Path::new(&llvm_home).join("bin");

    // 先生成一个空的 main() 来测量进程启动时间
    let startup_src = "fun main(): Int { return 0 }";
    let startup_exe = build_aot_exe(startup_src, &llvm_home, &bin, "startup")?;
    let startup_start = Instant::now();
    let _ = Command::new(&startup_exe).output().ok()?;
    let startup_time = startup_start.elapsed().as_secs_f64();
    let _ = std::fs::remove_file(&startup_exe);

    // 生成在进程内循环 N 次的源码
    let loop_src = aot_loop_src(src.contains("fib"), iters as i64);
    let exe = build_aot_exe(&loop_src, &llvm_home, &bin, "bench_inproc")?;

    // 启动一次可执行文件，测量总时间
    let start = Instant::now();
    let out = Command::new(&exe).output().ok()?;
    let total_dur = start.elapsed().as_secs_f64();

    // 验证结果（返回值为 N * expected）
    let raw = out.status.code().unwrap_or(-1);
    let code = if raw < 0 { raw as u32 as i64 } else { raw as i64 };
    let expected_total = expected * (iters as i64);
    if code != expected_total {
        eprintln!(
            "AOT inprocess result mismatch: expected {} got {}",
            expected_total, code
        );
        return None;
    }

    // 纯执行时间 = 总时间 - 进程启动时间
    let exec_dur = total_dur - startup_time;
    if exec_dur <= 0.0 {
        eprintln!(
            "Execution time anomaly: total={:.4}ms, startup={:.4}ms",
            total_dur * 1000.0,
            startup_time * 1000.0
        );
        return None;
    }

    // 每次操作时间 = 纯执行时间 / N
    let dur = exec_dur / iters as f64;
    let _ = std::fs::remove_file(&exe);
    Some(dur)
}

#[cfg(feature = "llvm")]
fn build_aot_exe(src: &str, llvm_home: &str, bin: &Path, name: &str) -> Option<std::path::PathBuf> {
    use compiler::codegen::aot::{AotCodeGenerator, AotOptions};
    use std::path::Path;
    use std::process::Command;

    let codegen = AotCodeGenerator::new(AotOptions::default());
    let mut lexer = compiler::lexer::Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = compiler::parser::Parser::new(tokens);
    let program = parser.parse_program();
    let hir = compiler::codegen::hir::desugar_program(&program);
    let ir = match codegen.generate_ir(&hir) {
        Ok(ir) => ir,
        Err(_) => return None,
    };

    let tmp = std::env::temp_dir().join(format!("aura_aot_build_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let ll = tmp.join(format!("{}.ll", name));
    let obj = tmp.join(format!("{}.obj", name));
    let exe = if cfg!(target_os = "windows") {
        tmp.join(format!("{}.exe", name))
    } else {
        tmp.join(name)
    };
    std::fs::write(&ll, &ir).ok()?;

    let llc = Path::new(llvm_home).join("bin").join(if cfg!(target_os = "windows") {
        "llc.exe"
    } else {
        "llc"
    });
    let out = Command::new(&llc)
        .arg(&ll)
        .arg("-o")
        .arg(&obj)
        .arg("-O3")
        .arg("-filetype=obj")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let mut linker = if cfg!(target_os = "windows") {
        let mut c = Command::new(Path::new(llvm_home).join("bin").join("lld-link.exe"));
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
    let out = match linker.output() {
        Ok(o) => o,
        Err(_) => return None,
    };
    if !out.status.success() {
        return None;
    }

    Some(exe)
}

#[cfg(not(feature = "llvm"))]
fn aot_inprocess_bench(_src: &str, _iters: usize, _expected: i64) -> Option<f64> {
    None
}

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

#[cfg(feature = "jit")]
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
    assert_eq!(r.as_int(), expected, "JIT result mismatch");
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

#[cfg(not(feature = "jit"))]
fn jit_bench(_src: &str, _iters: usize, _expected: i64) -> Option<f64> {
    None
}

#[cfg(feature = "llvm")]
fn aot_bench(src: &str, iters: usize, expected: i64) -> Option<f64> {
    use compiler::codegen::aot::{AotCodeGenerator, AotOptions};
    use std::path::Path;
    use std::process::Command;

    let llvm_home = match std::env::var("AURA_LLVM_HOME") {
        Ok(v) => v,
        Err(_) => return None,
    };
    let bin = Path::new(&llvm_home).join("bin");

    let codegen = AotCodeGenerator::new(AotOptions::default());
    let mut lexer = compiler::lexer::Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = compiler::parser::Parser::new(tokens);
    let program = parser.parse_program();
    let hir = compiler::codegen::hir::desugar_program(&program);
    let ir = match codegen.generate_ir(&hir) {
        Ok(ir) => ir,
        Err(_) => return None,
    };

    let tmp = std::env::temp_dir().join(format!("aura_full_bench_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);
    let ll = tmp.join("bench.ll");
    let obj = tmp.join("bench.obj");
    let exe = if cfg!(target_os = "windows") { tmp.join("bench.exe") } else { tmp.join("bench") };
    std::fs::write(&ll, &ir).ok()?;

    let llc = bin.join(if cfg!(target_os = "windows") { "llc.exe" } else { "llc" });
    let out = Command::new(&llc)
        .arg(&ll)
        .arg("-o")
        .arg(&obj)
        .arg("-O3")
        .arg("-filetype=obj")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

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
    let out = match linker.output() {
        Ok(o) => o,
        Err(_) => return None,
    };
    if !out.status.success() {
        return None;
    }

    let start = Instant::now();
    for _ in 0..iters {
        let out = Command::new(&exe).output().ok()?;
        let raw = out.status.code().unwrap_or(-1);
        let code = if raw < 0 { raw as u32 as i64 } else { raw as i64 };
        assert_eq!(code, expected);
    }
    let dur = start.elapsed().as_secs_f64() / iters as f64;
    let _ = std::fs::remove_dir_all(&tmp);
    Some(dur)
}

#[cfg(not(feature = "llvm"))]
fn aot_bench(_src: &str, _iters: usize, _expected: i64) -> Option<f64> {
    None
}

fn print_bench(_name: &str, vm: f64, jit: Option<f64>, aot: Option<f64>, aot_inproc: Option<f64>) {
    println!("  VM(bytecode interpreter):  {:.3} ms/op", vm * 1000.0);
    if let Some(j) = jit {
        println!(
            "  JIT(Cranelift):    {:.3} ms/op ({:.1}x vs VM)",
            j * 1000.0,
            vm / j
        );
    } else {
        println!("  JIT: jit feature not enabled");
    }
    if let Some(a) = aot {
        println!(
            "  AOT(separate process):    {:.3} ms/op ({:.1}x vs VM) [includes process startup overhead]",
            a * 1000.0,
            vm / a
        );
    } else {
        println!("  AOT(separate process):    LLVM not installed");
    }
    if let Some(a) = aot_inproc {
        println!(
            "  AOT(inprocess loop):  {:.3} ms/op ({:.1}x vs VM) [no process startup]",
            a * 1000.0,
            vm / a
        );
        if let Some(j) = jit {
            println!("  ─── JIT vs AOT(inprocess): {:.2}x ───", j / a);
        }
    } else {
        println!("  AOT(inprocess loop):  LLVM not installed");
    }
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║          Aura execution engine performance benchmark (P7)          ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // fib(25)
    println!("─── fib(25) ─────────────────────────────────────────────────");
    let vm_fib = vm_bench(FIB_SRC, 10, FIB_EXPECTED);
    let jit_fib = {
        #[cfg(feature = "jit")]
        {
            Some(jit_bench(FIB_SRC, 10, FIB_EXPECTED))
        }
        #[cfg(not(feature = "jit"))]
        {
            None
        }
    };
    let aot_fib = aot_bench(FIB_SRC, 50, FIB_EXPECTED);
    let aot_fib_inproc = aot_inprocess_bench(FIB_SRC, 10, FIB_EXPECTED);
    print_bench("fib(25)", vm_fib, jit_fib, aot_fib, aot_fib_inproc);
    println!();

    // sum(60000)
    println!("─── sum(60000) ───────────────────────────────────────────────");
    let vm_sum = vm_bench(SUM_SRC, 10, SUM_EXPECTED);
    let jit_sum = {
        #[cfg(feature = "jit")]
        {
            Some(jit_bench(SUM_SRC, 10, SUM_EXPECTED))
        }
        #[cfg(not(feature = "jit"))]
        {
            None
        }
    };
    let aot_sum = aot_bench(SUM_SRC, 50, SUM_EXPECTED);
    // sum 值太大，用 1 次迭代避免 i32 溢出
    let aot_sum_inproc = aot_inprocess_bench(SUM_SRC, 1, SUM_EXPECTED);
    print_bench("sum(60000)", vm_sum, jit_sum, aot_sum, aot_sum_inproc);
    println!();

    // 汇总
    println!("=== Complete Summary ===");
    println!(
        "| Scenario     | VM (ms)  | JIT (ms) | JIT speedup | AOT sep proc (ms) | AOT inproc (ms) |"
    );
    println!("|------------|----------|----------|----------|--------------|----------------|");
    println!(
        "| fib(25)   | {:9.3} | {:8.3} | {:>5.1}x | {:12.3} | {:14.3} |",
        vm_fib * 1000.0,
        jit_fib.map_or(f64::NAN, |j| j * 1000.0),
        jit_fib.map_or(f64::NAN, |j| vm_fib / j),
        aot_fib.map_or(f64::NAN, |a| a * 1000.0),
        aot_fib_inproc.map_or(f64::NAN, |a| a * 1000.0),
    );
    println!(
        "| sum(60000)| {:9.3} | {:8.3} | {:>5.1}x | {:12.3} | {:14.3} |",
        vm_sum * 1000.0,
        jit_sum.map_or(f64::NAN, |j| j * 1000.0),
        jit_sum.map_or(f64::NAN, |j| vm_sum / j),
        aot_sum.map_or(f64::NAN, |a| a * 1000.0),
        aot_sum_inproc.map_or(f64::NAN, |a| a * 1000.0),
    );
    println!();
    println!(
        "Note: AOT separate process includes ~4ms process startup overhead per run; AOT inprocess loop does not include process startup"
    );
}
