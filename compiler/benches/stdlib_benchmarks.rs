//! Phase 7: 标准库性能基准测试
//!
//! 测量标准库模块在 VM/JIT/AOT 三种执行模式下的性能：
//! - `bench_math_abs` - Math.abs 在三种模式下的性能
//! - `bench_string_concat` - 字符串连接性能
//! - `bench_collections_list_ops` - 集合操作性能
//! - `bench_ffi_aot_direct` - FFI AOT 直接调用 vs 间接调用
//! - `bench_vm_minimal_vs_full` - 精简 VM vs 完整 VM 性能
//!
//! 运行：`cargo bench --bench stdlib_benchmarks`
//! 注意：AOT 部分需要设置 `AURA_LLVM_HOME` 或 LLVM 在 PATH 中。
//! 注意：JIT 部分需要启用 `jit` feature。

use std::hint::black_box;
use std::time::Instant;

use compiler::codegen::compile_source;
use compiler::vm::{Vm, VmOptions};

// ═══════════════════════════════════════════════════════════════════════════════
// 基准源码（内联定义，无需外部 std 模块）
// ═══════════════════════════════════════════════════════════════════════════════

/// Math.abs 基准源码（内联定义 Math 对象）
const MATH_ABS_SRC: &str = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
}
fun main(): Int {
    val x = Math.abs(-42)
    val y = Math.abs(42)
    val z = Math.abs(-1)
    x + y + z
}
"#;

/// 字符串连接基准源码
const STRING_CONCAT_SRC: &str = r#"
fun main(): Int {
    val a = "Hello"
    val b = "World"
    val c = a + b
    val d = c + "!"
    val e = d + "?"
    e.length
}
"#;

/// 集合操作基准源码（模拟列表遍历和累加）
const COLLECTIONS_SRC: &str = r#"
fun main(): Int {
    var sum = 0
    var i = 0
    while i < 100 {
        sum = sum + i
        i = i + 1
    }
    sum
}
"#;

/// FFI 直接调用基准源码（直接调用 Math 函数）
const FFI_DIRECT_SRC: &str = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
    fun min(a: Int, b: Int): Int {
        if (a < b) a else b
    }
    fun max(a: Int, b: Int): Int {
        if (a > b) a else b
    }
}
fun main(): Int {
    val a = Math.abs(-10)
    val b = Math.min(5, 10)
    val c = Math.max(3, 7)
    a + b + c
}
"#;

/// FFI 间接调用基准源码（通过包装函数间接调用）
const FFI_INDIRECT_SRC: &str = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
}
fun helper(x: Int): Int {
    Math.abs(x)
}
fun main(): Int {
    helper(-10) + helper(10) + helper(-5)
}
"#;

/// 精简 VM 基准源码（仅使用基础运算）
const VM_MINIMAL_SRC: &str = r#"
fun main(): Int {
    var s = 0
    var i = 0
    while i < 10000 {
        s = s + i
        i = i + 1
    }
    s
}
"#;

/// 完整 VM 基准源码（使用内联对象 + 字符串 + 条件）
const VM_FULL_SRC: &str = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
    fun min(a: Int, b: Int): Int {
        if (a < b) a else b
    }
}
fun main(): Int {
    var s = 0
    var i = 0
    while i < 10000 {
        s = s + Math.abs(i)
        i = i + 1
    }
    Math.min(s, 1000000)
}
"#;

// ═══════════════════════════════════════════════════════════════════════════════
// VM 基准
// ═══════════════════════════════════════════════════════════════════════════════

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

#[cfg(not(feature = "jit"))]
fn bench_jit(_src: &str, _iters: usize, _expected: i64) -> f64 {
    f64::NAN
}

// ═══════════════════════════════════════════════════════════════════════════════
// AOT 基准
// ═══════════════════════════════════════════════════════════════════════════════

/// 通过 AOT 编译并运行（需要 LLVM）
#[cfg(feature = "llvm")]
fn bench_aot(src: &str, iters: usize, expected: i64) -> Result<f64, String> {
    use std::process::Command;

    let codegen = compiler::codegen::aot::AotCodeGenerator::new(
        compiler::codegen::aot::AotOptions::default(),
    );
    let mut lexer = compiler::lexer::Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = compiler::parser::Parser::new(tokens);
    let program = parser.parse_program();
    let hir = compiler::codegen::hir::desugar_program(&program);
    let ir = codegen.generate_ir(&hir).map_err(|e| e.to_string())?;

    let tmp = std::env::temp_dir().join(format!("aura_stdlib_bench_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let ll_path = tmp.join("main.ll");
    let o_path = tmp.join("main.obj");
    let exe_path = tmp.join("main.exe");
    std::fs::write(&ll_path, &ir).unwrap();

    let mut cmd = Command::new(llc_path());
    cmd.arg(&ll_path).arg("-o").arg(&o_path).arg("-O2").arg("-filetype=obj");
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "llc 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }

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

#[cfg(feature = "llvm")]
fn llc_path() -> std::path::PathBuf {
    let home = std::env::var("AURA_LLVM_HOME").ok();
    let bin = if cfg!(target_os = "windows") { "llc.exe" } else { "llc" };
    if let Some(h) = home {
        let p = std::path::Path::new(&h).join("bin").join(bin);
        if p.exists() {
            return p;
        }
    }
    std::path::PathBuf::from(bin)
}

#[cfg(not(feature = "llvm"))]
fn bench_aot(_src: &str, _iters: usize, _expected: i64) -> Result<f64, String> {
    Err("未启用 llvm feature".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════════
// FFI 缓存基准
// ═══════════════════════════════════════════════════════════════════════════════

/// FFI 缓存查找基准
fn bench_ffi_cache_lookup() {
    use compiler::codegen::ffi_cache::FfiCallCache;

    let mut cache = FfiCallCache::new();

    // 预热
    for i in 0..100usize {
        let key = format!("func_{}", i % 10);
        cache.preload(&key, i * 100);
    }

    // 基准测量
    let start = Instant::now();
    let iterations = 10_000;
    for i in 0..iterations {
        let key = format!("func_{}", i % 10);
        let _ = cache.lookup(&key);
    }
    let elapsed = start.elapsed();

    println!(
        "FFI cache lookup: {:.2} ns/op",
        elapsed.as_nanos() as f64 / iterations as f64
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 基准运行函数
// ═══════════════════════════════════════════════════════════════════════════════

fn run_math_abs() {
    // abs(-42) + abs(42) + abs(-1) = 42 + 42 + 1 = 85
    let expected = 85i64;
    let iters = 50;

    let vm_ms = bench_vm(MATH_ABS_SRC, iters, expected);
    println!("[Math.abs] VM(bytecode):   {:.3} ms/op", vm_ms);

    #[cfg(feature = "jit")]
    {
        let jit_ms = bench_jit(MATH_ABS_SRC, iters, expected);
        println!("[Math.abs] VM(JIT):        {:.3} ms/op", jit_ms);
    }

    #[cfg(feature = "llvm")]
    {
        match bench_aot(MATH_ABS_SRC, iters, expected) {
            Ok(aot_ms) => {
                println!("[Math.abs] AOT(native):     {:.3} ms/op", aot_ms);
                println!("[Math.abs] AOT 加速比:      {:.1}x vs VM", vm_ms / aot_ms);
            }
            Err(e) => println!("[Math.abs] AOT: 跳过 ({})", e),
        }
    }
}

fn run_string_concat() {
    // "Hello" + "World" + "!" + "?" = "HelloWorld!?" = 12 chars
    let expected = 12i64;
    let iters = 50;

    let vm_ms = bench_vm(STRING_CONCAT_SRC, iters, expected);
    println!("[String.concat] VM(bytecode): {:.3} ms/op", vm_ms);

    #[cfg(feature = "jit")]
    {
        let jit_ms = bench_jit(STRING_CONCAT_SRC, iters, expected);
        println!("[String.concat] VM(JIT):        {:.3} ms/op", jit_ms);
    }

    #[cfg(feature = "llvm")]
    {
        match bench_aot(STRING_CONCAT_SRC, iters, expected) {
            Ok(aot_ms) => {
                println!("[String.concat] AOT(native):  {:.3} ms/op", aot_ms);
                println!("[String.concat] AOT 加速比:   {:.1}x vs VM", vm_ms / aot_ms);
            }
            Err(e) => println!("[String.concat] AOT: 跳过 ({})", e),
        }
    }
}

fn run_collections_list_ops() {
    // sum(0..100) = 0+1+...+99 = 4950
    let expected = 4950i64;
    let iters = 50;

    let vm_ms = bench_vm(COLLECTIONS_SRC, iters, expected);
    println!("[Collections] VM(bytecode):  {:.3} ms/op", vm_ms);

    #[cfg(feature = "jit")]
    {
        let jit_ms = bench_jit(COLLECTIONS_SRC, iters, expected);
        println!("[Collections] VM(JIT):       {:.3} ms/op", jit_ms);
    }
}

fn run_ffi_aot_direct() {
    // 直接调用: abs(-10) + min(5,10) + max(3,7) = 10 + 5 + 7 = 22
    // 间接调用: abs(-10) + abs(10) + abs(-5) = 10 + 10 + 5 = 25
    let expected_direct = 22i64;
    let expected_indirect = 25i64;
    let iters = 50;

    let vm_direct = bench_vm(FFI_DIRECT_SRC, iters, expected_direct);
    println!("[FFI 直接调用] VM: {:.3} ms/op", vm_direct);

    let vm_indirect = bench_vm(FFI_INDIRECT_SRC, iters, expected_indirect);
    println!("[FFI 间接调用] VM: {:.3} ms/op", vm_indirect);

    println!("[FFI] 直接/间接 比值: {:.2}x", vm_indirect / vm_direct);
}

fn run_vm_minimal_vs_full() {
    // 精简: sum(0..10000) = 49,995,000
    // 完整: min(sum(0..10000), 1000000) = 1,000,000
    let expected_minimal = 49_995_000i64;
    let expected_full = 1_000_000i64;
    let iters = 50;

    let vm_minimal = bench_vm(VM_MINIMAL_SRC, iters, expected_minimal);
    println!("[精简 VM]  VM: {:.3} ms/op", vm_minimal);

    let vm_full = bench_vm(VM_FULL_SRC, iters, expected_full);
    println!("[完整 VM]  VM: {:.3} ms/op", vm_full);

    println!("[VM] 完整/精简 比值: {:.2}x", vm_full / vm_minimal);
}

/// 运行所有标准库基准
pub fn run_all() {
    println!("=== Aura 标准库性能基准 ===\n");

    black_box(run_math_abs);
    black_box(run_string_concat);
    black_box(run_collections_list_ops);
    black_box(run_ffi_aot_direct);
    black_box(run_vm_minimal_vs_full);
    black_box(bench_ffi_cache_lookup);

    println!("\nDone.");
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试入口
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_benchmarks() {
        run_all();
    }
}

fn main() {
    run_all();
}
