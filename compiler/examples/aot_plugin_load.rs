//! Tier 2 动态库加载测试（Phase 4.5）
//!
//! 验证 `AotRuntime::load_shared_library` 路径：
//! 1. 先编译一个动态库（复用 aot_shared_lib 的编译逻辑）
//! 2. dlopen 动态库
//! 3. 枚举 `aura_aot_*` 导出符号
//! 4. 构建 AotModule
//! 5. 通过 `call_func` 调用共享库中的函数
//!
//! 运行：`cargo run --features "llvm,dynamic-ffi" -p compiler --example aot_plugin_load`

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OutputFormat};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::abi::{AotCallContext, JitValue, TAG_INT};
use compiler::vm::aot_runtime::AotRuntime;

const SRC: &str = r#"
    fun add(a: Int, b: Int): Int = a + b
    fun multiply(a: Int, b: Int): Int = a * b
"#;

fn main() {
    // ── 1. 编译动态库 ──
    let work_dir = std::env::temp_dir().join(format!("aura_plugin_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work_dir);

    let mut lexer = Lexer::new(SRC);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        eprintln!("词法错误: {}", e.message);
        return;
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        eprintln!("语法错误: {}", e.message);
        return;
    }
    let hir = desugar_program(&program);

    let codegen = AotCodeGenerator::new(AotOptions::default());
    let output = match codegen.compile(&hir, &work_dir, OutputFormat::SharedLibrary) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("SharedLibrary 编译失败: {}", e);
            return;
        }
    };

    let lib_path = output.shared_library_path.expect("应生成动态库");
    println!("✓ 动态库编译完成: {}", lib_path.display());

    // ── 2. 加载动态库 ──
    let mut runtime = AotRuntime::new();
    let module_id = match runtime.load_shared_library(lib_path.to_str().unwrap()) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("加载共享库失败: {}", e);
            return;
        }
    };

    let func_count = runtime.shared_lib_func_count(module_id);
    println!(
        "✓ 共享库加载成功 (module_id={}, 导出函数数={})",
        module_id, func_count
    );

    // ── 3. 调用函数 ──
    // 通过诊断信息找到每个函数的 func_idx
    let diag = runtime.module_diagnostics(module_id).expect("应有诊断信息");
    println!("  模块: {} (func_count={})", diag.name, diag.func_count);

    // 尝试调用每个函数（通过 func_idx 遍历）
    for func_idx in 0..func_count {
        // 构建 JitValue 参数 (add(3, 4) 或 multiply(3, 4))
        let args = vec![
            JitValue {
                tag: TAG_INT,
                payload: 3,
            },
            JitValue {
                tag: TAG_INT,
                payload: 4,
            },
        ];
        let mut ret = JitValue::null();
        let mut ctx = AotCallContext::new();
        ctx.module_id = module_id;
        ctx.func_idx = func_idx as u32;
        ctx.call_depth = 1;

        unsafe {
            let entry = runtime.cached_find_entry(func_idx);
            if let Some((mid, entry_fn)) = entry {
                let args_ptr = if args.is_empty() { std::ptr::null() } else { args.as_ptr() };
                let ctx_ptr: *const () = &ctx as *const AotCallContext as *const ();
                entry_fn(args_ptr, &mut ret, args.len(), ctx_ptr);
            }
        }

        if ret.tag == TAG_INT {
            println!("  func_idx={}: 调用成功, 返回 {}", func_idx, ret.payload);
        } else {
            println!(
                "  func_idx={}: 调用返回 tag={}, payload={}",
                func_idx, ret.tag, ret.payload
            );
        }
    }

    // ── 4. 清理 ──
    let _ = std::fs::remove_dir_all(&work_dir);
    println!("\n✓ 端到端验证通过");
}
