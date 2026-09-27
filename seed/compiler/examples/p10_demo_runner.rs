//! P10 并发运行时 Demo Runner — 编译并运行 examples/p10_*.aura
//!
//! 运行: `cargo run --example p10_demo_runner -p compiler`

use std::fs;
use std::path::Path;

use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

/// 编译并运行 .aura 源码，返回最终结果
fn compile_and_run(label: &str, src: &str) -> Result<Value, String> {
    println!("\n═══════════════════════════════════════════════");
    println!("  📦 {}", label);
    println!("═══════════════════════════════════════════════");

    let module = compile_source(src).map_err(|e| format!("compilation failed: {}", e))?;
    let mut vm = Vm::new(&module, VmOptions::default())
        .map_err(|e| format!("VM initialization failed: {}", e))?;
    let result = vm.run().map_err(|e| format!("run failed: {}", e))?;

    println!("  Target result: {}", result);
    println!("  * execution succeeded");
    Ok(result)
}

/// 运行 .aura 文件
fn run_aura_file(path: &Path) -> Result<Value, String> {
    let label = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
    let src = fs::read_to_string(path).map_err(|e| format!("  failed to read {}: {}", label, e))?;
    compile_and_run(label, &src)
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║     P10 Concurrency runtime Demo Runner         ║");
    println!("║     Coroutine / Actor / Channel / Select demo ║");
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
                println!("  * {}", file);
                passed += 1;
            }
            Err(e) => {
                println!("  ! {}: {}", file, e);
                failed += 1;
            }
        }
    }

    println!("\n═══════════════════════════════════════════════");
    println!(
        "  Summary: {} passed, {} failed, {} total",
        passed,
        failed,
        files.len()
    );
    if failed == 0 {
        println!("  All P10 Demos passed!");
    } else {
        println!("  ! {} Demos failed", failed);
    }
    println!("═══════════════════════════════════════════════");
}
