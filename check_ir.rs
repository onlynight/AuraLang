use compiler::codegen::aot::AotCodeGenerator;
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;

fn main() {
    let src = r#"
        @native(asm = "rdtsc")
        fun rdtsc(): Long { }
        fun main(): Int { rdtsc(); return 0 }
    "#;
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let hir = desugar_program(&program);
    
    // Check HIR
    for f in &hir.natives {
        println!("Native func: name={}, is_native={}, native_attr={:?}", f.name, f.is_native, f.native_attr);
    }
    for f in &hir.functions {
        if f.name.contains("rdtsc") {
            println!("Function: name={}, is_native={}, native_attr={:?}", f.name, f.is_native, f.native_attr);
        }
    }
    
    let codegen = AotCodeGenerator::new(Default::default());
    match codegen.generate_ir(&hir) {
        Ok(ir) => {
            println!("=== IR (rdtsc/asm/aura_cpu lines) ===");
            println!("{}", ir.lines().filter(|l| l.contains("rdtsc") || l.contains("asm sideeffect") || l.contains("aura_cpu")).collect::<Vec<_>>().join("\n"));
            println!("=== IR (all native wrappers) ===");
            println!("{}", ir.lines().filter(|l| l.contains("define") && (l.contains("rdtsc") || l.contains("Memory"))).collect::<Vec<_>>().join("\n"));
        }
        Err(e) => println!("Error: {}", e),
    }
}
