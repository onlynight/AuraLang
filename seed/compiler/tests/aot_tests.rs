#![cfg(feature = "llvm")]

//! AOT（LLVM）后端集成测试
//!
//! 覆盖：
//! 1. LLVM IR 文本生成（算术、函数调用、控制流、结构体、FFI、runtime）
//! 2. 目标三元组格式化
//! 3. 类型映射
//! 4. 优化级别
//! 5. C 后端备选方案
//!
//! 完整链路测试（IR → llc → 链接 → 运行）需要 LLVM 工具链可用时才执行，
//! 通过 `AURA_LLVM_HOME` 环境变量存在判断。

use compiler::codegen::aot::{
    AotCodeGenerator, AotOptions, OptimizationLevel, TargetTriple, aot_compile,
};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;

/// 解析源码为 AST
fn parse(src: &str) -> compiler::ast::Program {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

/// 生成 LLVM IR 文本
fn gen_ir(src: &str) -> String {
    let program = parse(src);
    let hir = desugar_program(&program);
    let codegen = AotCodeGenerator::new(AotOptions::default());
    codegen.generate_ir(&hir).expect("IR generation failed")
}

#[test]
fn aot_ir_arithmetic() {
    let ir = gen_ir("fun main(): Int { return 1 + 2 * 3 }");
    assert!(ir.contains("@main"), "should contain main function");
    assert!(ir.contains("mul"), "should contain multiplication");
    assert!(ir.contains("add"), "should contain addition");
}

#[test]
fn aot_ir_function_call() {
    let ir = gen_ir(
        "fun add(a: Int, b: Int): Int { return a + b }\nfun main(): Int { return add(1, 2) }",
    );
    assert!(ir.contains("@add"), "should contain add function");
    assert!(ir.contains("call i32 @add"), "should contain call to add");
}

#[test]
fn aot_ir_if_control_flow() {
    let ir = gen_ir("fun main(): Int { var x = 5\nif (x > 3) { return 1 }\nreturn 0 }");
    assert!(ir.contains("icmp"), "should contain integer comparison");
    assert!(ir.contains("br i1"), "should contain conditional branch");
}

#[test]
fn aot_ir_while_loop() {
    let ir = gen_ir(
        "fun sum(n: Int): Int { var i = 0\nvar s = 0\nwhile (i < n) { s = s + i\ni = i + 1 }\nreturn s }\nfun main(): Int { return sum(10) }",
    );
    assert!(
        ir.contains("loop.cond"),
        "should contain loop condition block"
    );
    assert!(ir.contains("loop.body"), "should contain loop body block");
}

#[test]
fn aot_ir_struct_definition() {
    let ir = gen_ir("struct Point(val x: Int, val y: Int)\nfun main(): Int { return 0 }");
    assert!(
        ir.contains("%struct.Point"),
        "should contain struct type definition"
    );
}

#[test]
fn aot_ir_ffi_decl() {
    let ir = gen_ir(
        r#"
        extern "c" "raylib" {
            fun DrawCircle(x: Int, y: Int, radius: Float, color: Int)
        }
        fun main(): Int { return 0 }
    "#,
    );
    assert!(
        ir.contains("DrawCircle"),
        "should contain FFI function declaration"
    );
}

#[test]
fn aot_ir_runtime_decls() {
    let ir = gen_ir("fun main(): Int { return 1 }");
    assert!(
        ir.contains("aura_arc_increment"),
        "should contain ARC runtime declaration"
    );
    assert!(
        ir.contains("aura_malloc"),
        "should contain malloc runtime declaration"
    );
}

#[test]
fn aot_target_triples() {
    assert_eq!(
        TargetTriple::windows_x86_64().to_string(),
        "x86_64-pc-windows-msvc"
    );
    assert_eq!(
        TargetTriple::linux_aarch64().to_string(),
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(
        TargetTriple::linux_armv7().to_string(),
        "armv7-unknown-linux-gnueabihf"
    );
}

#[test]
fn aot_opt_levels() {
    assert_eq!(OptimizationLevel::Aggressive.as_llvm_flag(), "-O2");
    assert_eq!(OptimizationLevel::Extreme.as_llvm_flag(), "-O3");
    assert_eq!(OptimizationLevel::None.as_llvm_flag(), "-O0");
}

#[test]
fn aot_c_backend() {
    let src = "fun add(a: Int, b: Int): Int { return a + b }\nfun main(): Int { return add(1, 2) }";
    let program = parse(src);
    let hir = desugar_program(&program);
    let c = compiler::codegen::aot::c_backend::generate_c_code(&hir).expect("C generation failed");
    assert!(c.contains("#include"), "C code should contain header");
    assert!(c.contains("int32_t"), "C code should contain integer type");
    assert!(c.contains("add"), "C code should contain add function");
}

#[test]
fn aot_module_entry() {
    // 没有 main 时合成 main
    let ir = gen_ir("fun compute(): Int { return 42 }");
    assert!(ir.contains("@main"), "should synthesize main function");
    assert!(ir.contains("compute"), "should contain compute function");
    assert!(ir.contains("call"), "synthesized main should call compute");
}

#[test]
fn aot_compile_llvm_ir() {
    let src = "fun main(): Int { return 42 }";
    // 使用 --aot 输出 LLVM IR（不依赖 llc）
    let out = std::env::temp_dir().join("aura_aot_test.ll");
    let result = aot_compile(src, &out, AotOptions::default());
    assert!(result.is_ok(), "AOT compilation (LLVM IR) should succeed");
    if let Ok(output) = result {
        assert!(output.ir_text.contains("@main"));
    }
}

/// 完整链路测试：仅当 LLVM 工具可用时执行
#[test]
fn aot_full_pipeline_when_llvm_available() {
    let llvm_home = std::env::var("AURA_LLVM_HOME").ok();
    if llvm_home.is_none() {
        eprintln!("skipped full pipeline test: AURA_LLVM_HOME not set");
        return;
    }

    let src =
        "fun add(a: Int, b: Int): Int { return a + b }\nfun main(): Int { return add(20, 22) }";
    let tmp = std::env::temp_dir().join("aura_aot_full");
    std::fs::create_dir_all(&tmp).unwrap();

    let options = AotOptions::default();
    let codegen = AotCodeGenerator::new(options.clone());

    let program = parse(src);
    let hir = desugar_program(&program);
    let ir = codegen.generate_ir(&hir).unwrap();

    // 1. 写 .ll
    let ll_path = tmp.join("main.ll");
    std::fs::write(&ll_path, &ir).unwrap();

    // 2. llc → .o
    let o_path = tmp.join("main.obj");
    let llc = compiler::codegen::aot::linker::link_to_object(&ll_path, &o_path, &options);
    assert!(
        llc.is_ok(),
        "llc compilation should succeed: {:?}",
        llc.err()
    );

    // 3. lld-link → .exe
    let exe_path = tmp.join("main.exe");
    let link = compiler::codegen::aot::linker::link_to_executable(&o_path, &exe_path, &options);
    assert!(link.is_ok(), "linking should succeed: {:?}", link.err());

    // 4. 运行并验证结果
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new(&exe_path).output().expect("run should succeed");
        let code = output.status.code().unwrap_or(-1);
        assert_eq!(
            code, 42,
            "AOT run result should be 42 (add(20,22)), got: {}",
            code
        );
    }
}

/// 回归：if 语句无 return 分支时必须生成 merge 块（§9.2.4）
/// 且生成 IR 必须能被 llc 接受（此前 merge 块缺失 → 非法 IR）
#[test]
fn aot_if_statement_merge_block() {
    let src = "fun main(): Int { var x = 5\nif (x > 3) { x = x + 1 }\nreturn x }";
    let program = parse(src);
    let hir = desugar_program(&program);
    let codegen = AotCodeGenerator::new(AotOptions::default());
    let ir = codegen.generate_ir(&hir).unwrap();

    // if 分支不 return → 必须有 merge 标签定义（此前缺失）
    assert!(
        ir.contains("bb_merge_"),
        "if statement should generate merge block (got: {})",
        ir
    );

    // 有条件分支与 AND merge 引用成对
    let has_br_i1 = ir.contains("br i1");
    let has_merge = ir.contains("bb_merge_");
    assert!(
        has_br_i1 && has_merge,
        "if should contain conditional branch and merge block"
    );
}

/// 回归：字符串全局常量必须在模块顶层（§9.2.1 generate_globals）
/// 且使用 `@` 前缀；此前被错误放入函数体内
#[test]
fn aot_string_global_at_module_level() {
    let src = "fun main(): Int { val name = \"hello\"\nreturn 0 }";
    let program = parse(src);
    let hir = desugar_program(&program);
    let codegen = AotCodeGenerator::new(AotOptions::default());
    let ir = codegen.generate_ir(&hir).unwrap();

    // 全局常量用 @ 前缀
    assert!(
        ir.contains("@str_data."),
        "string global constant should use @ prefix"
    );
    // 必须出现在函数之外（模块顶层）：检查 define 之前/之后位置
    // 简化断言：@str_data 定义行不在函数体内（其后紧跟的指令不是 alloca 等）
    let lines: Vec<&str> = ir.lines().collect();
    let has_global_line =
        lines.iter().any(|l| l.contains("@str_data.") && l.contains("private constant"));
    assert!(
        has_global_line,
        "should have module-level string constant definition: {}",
        ir
    );

    // 函数体内只应引用（getelementptr），不应重复定义
    let def_count = ir.matches("@str_data.").count();
    assert_eq!(
        def_count, 2,
        "@str_data should be defined once and referenced once (actual {})",
        def_count
    );
}

/// 回归：DWARF 元数据必须是真实 LLVM 元数据（§9.5），
/// 含 DISubprogram / DILocation / llvm.dbg.cu 注册
#[test]
fn aot_dwarf_metadata_real() {
    let src = "fun add(a: Int, b: Int): Int { return a + b }\nfun main(): Int { return add(1, 2) }";
    let program = parse(src);
    let hir = desugar_program(&program);
    let opts = AotOptions {
        debug_info: true,
        ..Default::default()
    };
    let codegen = AotCodeGenerator::new(opts);
    let ir = codegen.generate_ir(&hir).unwrap();

    assert!(
        ir.contains("distinct !DICompileUnit"),
        "should have DICompileUnit"
    );
    assert!(ir.contains("!DISubprogram"), "should have DISubprogram");
    assert!(ir.contains("!DILocation"), "should have DILocation");
    assert!(
        ir.contains("!llvm.dbg.cu = !{!1}"),
        "should register compilation unit"
    );
    assert!(
        ir.contains("!dbg !"),
        "function body should have !dbg metadata"
    );
    // DISubprogram 不应被注释掉
    assert!(
        !ir.contains("; !DISubprogram"),
        "DISubprogram should not be commented out"
    );
}
