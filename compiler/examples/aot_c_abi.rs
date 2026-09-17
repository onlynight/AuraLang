//! Tier 2b C ABI 包装函数测试
//!
//! 验证 `--cabi` 开关生成裸 C ABI 包装函数：
//! 1. 编译 Aura 源文件，启用 C ABI 包装
//! 2. 检查生成的 LLVM IR 中包含 `define c` 函数
//! 3. 检查 Windows 下有 `dllexport`，Unix 下有 `visibility("default")`
//!
//! 运行：`cargo run --features "llvm,dynamic-ffi" -p compiler --example aot_c_abi`

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OutputFormat, TargetTriple};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;

const SRC: &str = r#"
    fun add(a: Int, b: Int): Int = a + b
    fun multiply(a: Int, b: Int): Int = a * b
    fun main() = {
        let x = add(3, 4)
        let y = multiply(x, 2)
    }
"#;

fn main() {
    let work_dir = std::env::temp_dir().join(format!("aura_c_abi_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work_dir);

    // 1. 编译源文件
    let mut lexer = Lexer::new(SRC);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        eprintln!("lex error: {}", e.message);
        return;
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        eprintln!("syntax error: {}", e.message);
        return;
    }
    let hir = desugar_program(&program);

    // 2. 配置 AOT 选项（启用 C ABI）
    let options = AotOptions {
        c_abi: true,
        target: TargetTriple::default(),
        ..Default::default()
    };

    let codegen = AotCodeGenerator::new(options);
    let ir = codegen.generate_ir_with_mode(&hir, false, false, false).unwrap();

    // 3. 写入 .ll 文件
    let ll_path = work_dir.join("c_abi_test.ll");
    std::fs::write(&ll_path, &ir).unwrap();

    println!("* LLVM IR generated: {}", ll_path.display());

    // 4. 检查 IR 中包含 C ABI 包装函数
    let has_c_abi = ir.contains("define c ");
    println!("  contains `define c` function: {}", has_c_abi);

    // 5. 检查具体的 C ABI 函数名
    let has_add_wrapper = ir.contains("aura_c_add");
    let has_multiply_wrapper = ir.contains("aura_c_multiply");
    println!("  contains aura_c_add: {}", has_add_wrapper);
    println!("  contains aura_c_multiply: {}", has_multiply_wrapper);

    // 6. 检查导出属性
    #[cfg(target_os = "windows")]
    {
        let has_dllexport = ir.contains("dllexport");
        println!("  contains dllexport: {}", has_dllexport);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let has_visibility = ir.contains("visibility");
        println!("  Contains visibility(\"default\"): {}", has_visibility);
    }

    // 7. 打印 C ABI 函数定义（简化版）
    println!("\n--- C ABI wrapper function IR snippets ---");
    for line in ir.lines() {
        if line.contains("define c ") || line.contains("aura_c_") {
            println!("{}", line);
        }
    }

    // 8. 清理
    let _ = std::fs::remove_dir_all(&work_dir);

    // 9. 验证结果
    if has_c_abi && has_add_wrapper && has_multiply_wrapper {
        println!("\n* C ABI wrapper function generation verification passed");
    } else {
        eprintln!("\n* C ABI wrapper function generation failed");
        std::process::exit(1);
    }
}
