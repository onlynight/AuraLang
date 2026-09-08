use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OutputFormat};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;

const SRC: &str = include_str!("../../examples/ext_ffi_demo/libs/utils/src/lib.aura");

fn main() {
    let output_dir = std::env::temp_dir().join("aura_ext_ffi_demo_utils");
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

    if let Some(ll_path) = &output.ll_path {
        let ir = std::fs::read_to_string(ll_path).unwrap();
        let wrapper_count = ir.matches("aura_aot_").count();
        let external_wrappers = ir
            .lines()
            .filter(|l| {
                l.starts_with("define i64 @\"aura_aot_") && !l.starts_with("define internal")
            })
            .count();
        let has_main = ir.lines().any(|l| l.contains("define") && l.contains("@main"));
        println!("\n  IR 分析:");
        println!("    包装函数总数: {}", wrapper_count);
        println!("    external 包装函数: {}", external_wrappers);
        println!("    包含 main: {}", has_main);
    }

    if let Some(lib_path) = &output.shared_library_path {
        println!("\n✓ 动态库路径: {}", lib_path.display());
    }
}
