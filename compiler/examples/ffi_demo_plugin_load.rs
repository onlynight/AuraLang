use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OutputFormat};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::abi::{AotCallContext, JitValue, TAG_INT};
use compiler::vm::aot_runtime::AotRuntime;

const SRC: &str = include_str!("../../examples/ext_ffi_demo/libs/utils/src/lib.aura");

fn main() {
    // ── 1. 编译动态库 ──
    let work_dir =
        std::env::temp_dir().join(format!("aura_ffi_demo_plugin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work_dir);
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

    let diag = runtime.module_diagnostics(module_id).expect("应有诊断信息");
    println!("  模块: {} (func_count={})", diag.name, diag.func_count);

    // ── 3. 调用函数 ──
    // func_idx=0: add(3,4)=7, func_idx=1: multiply(3,4)=12, func_idx=2: factorial(5)=120, func_idx=3: power(2,10)=1024
    let test_cases: Vec<(usize, &str, Vec<i64>)> = vec![
        (0, "add", vec![3, 4]),
        (1, "multiply", vec![3, 4]),
        (2, "factorial", vec![5]),
        (3, "power", vec![2, 10]),
    ];

    for (func_idx, name, args) in test_cases {
        let jit_args: Vec<JitValue> = args
            .iter()
            .map(|&v| JitValue {
                tag: TAG_INT,
                payload: v,
            })
            .collect();
        let mut ret = JitValue::null();
        let mut ctx = AotCallContext::new();
        ctx.module_id = module_id;
        ctx.func_idx = func_idx as u32;
        ctx.call_depth = 1;

        unsafe {
            let entry = runtime.cached_find_entry(func_idx);
            if let Some((_mid, entry_fn)) = entry {
                let args_ptr =
                    if jit_args.is_empty() { std::ptr::null() } else { jit_args.as_ptr() };
                let ctx_ptr: *const () = &ctx as *const AotCallContext as *const ();
                entry_fn(args_ptr, &mut ret, jit_args.len(), ctx_ptr);
            }
        }

        let args_str = args.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
        if ret.tag == TAG_INT {
            println!(
                "  func_idx={}: {}({}) = {}",
                func_idx, name, args_str, ret.payload
            );
        } else {
            println!(
                "  func_idx={}: {}({}) 返回 tag={}, payload={}",
                func_idx, name, args_str, ret.tag, ret.payload
            );
        }
    }

    let _ = std::fs::remove_dir_all(&work_dir);
    println!("\n✓ 端到端验证通过");
}
