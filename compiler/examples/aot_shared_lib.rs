//! Tier 2 动态库生成测试（Phase 4.1）
//!
//! 验证 `OutputFormat::SharedLibrary` 路径：
//! 1. 生成 JitValue ABI 包装函数（blob_mode = true）
//! 2. 包装函数以 external linkage 导出（wrapper_exported = true）
//! 3. 跳过 main 合成
//! 4. 产出 .so / .dylib / .dll
//!
//! 运行：`cargo run --features llvm -p compiler --example aot_shared_lib`

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OutputFormat};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;

const SRC: &str = r#"
    fun add(a: Int, b: Int): Int = a + b
    fun multiply(a: Int, b: Int): Int = a * b
    fun fib(n: Int): Int {
        if (n < 2) { return n }
        return add(fib(n - 1), fib(n - 2))
    }
"#;

fn main() {
    let output_dir = std::env::temp_dir().join(format!("aura_aot_shared_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&output_dir);
    let _ = std::fs::remove_dir_all(&output_dir);
    let _ = std::fs::create_dir_all(&output_dir);

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
    let output = match codegen.compile(&hir, &output_dir, OutputFormat::SharedLibrary) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("SharedLibrary 编译失败: {}", e);
            return;
        }
    };

    println!("✓ SharedLibrary 编译成功");
    println!("  LLVM IR: {:?}", output.ll_path);
    println!("  Object:  {:?}", output.object_path);
    println!("  DynamicLib: {:?}", output.shared_library_path);

    // 验证 IR 中包装函数以 external linkage 导出
    if let Some(ll_path) = &output.ll_path {
        let ir = std::fs::read_to_string(ll_path).unwrap();
        let wrapper_count = ir.matches("aura_aot_").count();
        let external_wrappers = ir
            .lines()
            .filter(|l| {
                l.starts_with("define i64 @\"aura_aot_") && !l.starts_with("define internal")
            })
            .count();
        let internal_wrappers =
            ir.lines().filter(|l| l.starts_with("define internal i64 @\"aura_aot_")).count();
        let has_main = ir.lines().any(|l| l.contains("define") && l.contains("@main"));

        println!("\n  IR 分析:");
        println!("    包装函数总数: {}", wrapper_count);
        println!("    external 包装函数: {}", external_wrappers);
        println!("    internal 包装函数: {}", internal_wrappers);
        println!("    包含 main: {}", has_main);

        assert!(external_wrappers > 0, "应有 external 包装函数");
        assert!(
            internal_wrappers == 0,
            "SharedLibrary 模式下不应有 internal 包装函数"
        );
        assert!(!has_main, "SharedLibrary 模式下不应有 main 函数");
        println!("    ✓ 验证通过");
    }

    // 清理
    let _ = std::fs::remove_dir_all(&output_dir);
}
