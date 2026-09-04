//! P10 并发运行时 Demo Runner — 编译并运行 examples/p10_*.aura
//!
//! 运行: `cargo run --example p10_demo_runner -p aura-compiler`

use std::fs;
use std::path::Path;

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Value, Vm, VmOptions};

/// 编译并运行 .aura 源码，返回最终结果
fn compile_and_run(label: &str, src: &str) -> Result<Value, String> {
    println!("\n═══════════════════════════════════════════════");
    println!("  📦 {}", label);
    println!("═══════════════════════════════════════════════");

    let module = compile_source(src).map_err(|e| format!("编译失败: {}", e))?;
    let mut vm = Vm::new(&module, VmOptions::default())
        .map_err(|e| format!("VM 初始化失败: {}", e))?;
    let result = vm.run().map_err(|e| format!("运行失败: {}", e))?;

    println!("  🎯 最终结果: {}", result);
    println!("  ✅ 执行成功");
    Ok(result)
}

/// 运行 .aura 文件
fn run_aura_file(path: &Path) -> Result<Value, String> {
    let label = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
    let src = fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {}", label, e))?;
    compile_and_run(label, &src)
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║     P10 并发运行时 Demo Runner                            ║");
    println!("║     协程 / Actor / Channel / Select 综合演示              ║");
    println!("╚══════════════════════════════════════════════════════════╝");

    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("examples");
    let files = [
        "coroutine_dispatch.aura",
        "actor_system.aura",
        "message_channel.aura",
        "select_multiplex.aura",
        "concurrent_integration.aura",
    ];

    let mut passed = 0;
    let mut failed = 0;

    for file in &files {
        let path = examples_dir.join(file);
        match run_aura_file(&path) {
            Ok(_) => {
                println!("  ✅ {}", file);
                passed += 1;
            }
            Err(e) => {
                println!("  ❌ {}: {}", file, e);
                failed += 1;
            }
        }
    }

    println!("\n═══════════════════════════════════════════════");
    println!("  📊 汇总: {} 通过, {} 失败, 共 {} 个", passed, failed, files.len());
    if failed == 0 {
        println!("  🎉 所有 P10 Demo 通过!");
    } else {
        println!("  ⚠️  有 {} 个 Demo 失败", failed);
    }
    println!("═══════════════════════════════════════════════");
}
