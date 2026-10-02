// Aura Seed Compiler — CLI entry point
// Rebuilt from library source to include emit.rs fix for CallVirtual bare-name resolution.

use std::env;
use std::process;
use std::fs;

fn main() {
    // 增加线程栈大小（导入解析递归深度大）
    let stack_size = 64 * 1024 * 1024; // 64MB
    let handle = std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(|| {
            real_main();
        })
        .expect("failed to spawn thread");
    handle.join().expect("thread panicked");
}

fn real_main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: aura <command> [options]");
        eprintln!("Commands: run, compile, check, version, build, stdlib-compile, export-header");
        process::exit(1);
    }

    match args[1].as_str() {
        "version" => {
            println!("Aura Seed Compiler 0.1.0 (with emit.rs fix)");
        }
        // ── export-header <entry.aura> --out <header.h> ──
        //
        // P3.4: 由 Aura 源码生成 C 头文件，声明 `--cabi --shared` 产物的
        // `aura_c_*` 导出原型，供 C / Rust 侧 `#include` 后直接链接调用。
        //
        // 与 Demo（`examples/ext_ffi_demo/demo_cffi/utils.h`）的契约一致：
        // 只导出**用户函数**（排除 native 声明与合成入口），函数名前缀 `aura_c_`，
        // 类型按「Aura `String` ≡ NUL 结尾 `i8*`」的既定 ABI 映射。
        "export-header" => {
            let entry = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if entry.is_empty() {
                eprintln!("Error: no input file specified");
                eprintln!("Usage: aura export-header <entry.aura> --out <header.h>");
                process::exit(1);
            }
            let out = args
                .iter()
                .position(|a| a == "--out")
                .and_then(|idx| args.get(idx + 1))
                .map(|s| s.as_str())
                .unwrap_or("");
            if out.is_empty() {
                eprintln!("Error: --out <header.h> required for export-header");
                process::exit(1);
            }
            let source = match fs::read_to_string(entry) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading file: {}", e);
                    process::exit(1);
                }
            };
            let expanded = compiler::codegen::resolve_aura_imports(&source, Some(entry));
            let header = match compiler::codegen::aot::export_c_header(&expanded, entry) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(1);
                }
            };
            if let Err(e) = fs::write(out, header) {
                eprintln!("Error writing {}: {}", out, e);
                process::exit(1);
            }
            println!("[ok] {} exported", out);
        }
        // ── stdlib-compile <src-dir> --output <out-dir> ──
        //
        // 把一棵目录树下的全部 .aura 逐个编译为 .auc，输出目录**镜像**输入目录
        // 结构（相对 src-dir），供 `std/embedded_stdlib.rs` 的 `core_auc!`
        // （`include_bytes!`）嵌入。输出路径约定见该文件头部说明：
        //
        //   aura/core/aura/lang/String.aura → <out>/aura/lang/String.auc
        //
        // 为什么需要这条命令：嵌入镜像是**编译期**烧进二进制的，修改
        // `aura/core/**` 后必须先重生成 .auc 再 `cargo build`；而 VM 的
        // `do_call_native` **始终优先**派发嵌入版本，不重生成则改动完全不生效。
        "stdlib-compile" => {
            let src_dir = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if src_dir.is_empty() {
                eprintln!("Error: no source directory specified");
                eprintln!("Usage: aura stdlib-compile <src-dir> --output <out-dir>");
                process::exit(1);
            }
            let out_dir = args
                .iter()
                .position(|a| a == "--output")
                .and_then(|idx| args.get(idx + 1))
                .map(|s| s.as_str())
                .unwrap_or("");
            if out_dir.is_empty() {
                eprintln!("Error: --output <dir> required for stdlib-compile");
                process::exit(1);
            }
            let src_root = std::path::PathBuf::from(src_dir);
            if !src_root.is_dir() {
                eprintln!("Error: source directory not found: {}", src_dir);
                process::exit(1);
            }
            let mut files: Vec<std::path::PathBuf> = Vec::new();
            collect_aura_files(&src_root, &mut files);
            files.sort();
            if files.is_empty() {
                eprintln!("Error: no .aura files under {}", src_dir);
                process::exit(1);
            }

            let mut ok = 0usize;
            let mut failed = 0usize;
            for file in &files {
                let source = match fs::read_to_string(file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[stdlib-compile] read {}: {}", file.display(), e);
                        failed += 1;
                        continue;
                    }
                };
                let entry = file.to_string_lossy().to_string();
                let expanded = compiler::codegen::resolve_aura_imports(&source, Some(&entry));
                let module = match compiler::codegen::compile_source(&expanded) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("[stdlib-compile] {}: {}", entry, e);
                        failed += 1;
                        continue;
                    }
                };
                // 输出路径：去 src-dir 前缀 + 扩展名换为 .auc
                let rel = file.strip_prefix(&src_root).unwrap_or(file);
                let mut out_path = std::path::PathBuf::from(out_dir);
                out_path.push(rel);
                out_path.set_extension("auc");
                if let Some(parent) = out_path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        eprintln!("[stdlib-compile] mkdir {}: {}", parent.display(), e);
                        failed += 1;
                        continue;
                    }
                }
                let out_str = out_path.to_string_lossy().to_string();
                match compiler::codegen::serialize::write_auc(&out_str, &module) {
                    Ok(_) => {
                        ok += 1;
                        println!(
                            "[stdlib-compile] {} -> {} ({} funcs)",
                            entry,
                            out_path.display(),
                            module.functions.len()
                        );
                    }
                    Err(e) => {
                        eprintln!("[stdlib-compile] write {}: {}", out_path.display(), e);
                        failed += 1;
                    }
                }
            }
            println!(
                "[stdlib-compile] {} ok / {} failed (of {})",
                ok,
                failed,
                files.len()
            );
            if failed > 0 {
                process::exit(1);
            }
        }
        "run" | "compile" | "check" | "build" => {
            let entry = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if entry.is_empty() {
                eprintln!("Error: no input file specified");
                process::exit(1);
            }
            if args[1] == "build" {
                // build [-b photon] [--aot] <entry> --output <path>
                // 跳过 -b 及其参数（photon 后端标记）、--aot 等 flag
                let mut i = 2;
                let backend = if i < args.len() && args[i] == "-b" && i + 1 < args.len() {
                    let be = args[i + 1].clone();
                    i += 2;
                    Some(be)
                } else {
                    None
                };
                // 跳过 --aot flag（不消费参数）
                while i < args.len() && args[i].starts_with("--") {
                    i += 1;
                }
                let entry = args.get(i).map(|s| s.as_str()).unwrap_or("");
                if entry.is_empty() {
                    eprintln!("Error: no input file specified");
                    process::exit(1);
                }
                let output = args.iter()
                    .position(|a| a == "--output")
                    .and_then(|idx| args.get(idx + 1))
                    .map(|s| s.as_str())
                    .unwrap_or("");
                if output.is_empty() {
                    eprintln!("Error: --output <path> required for build");
                    process::exit(1);
                }
                let use_aot = args.iter().any(|a| a == "--aot");
                // P3.4: `--cabi` 生成裸 C ABI 导出包装（`aura_c_*`），供 C/Rust 侧
                // `dlsym`/`libloading` 直接消费；`--shared` 走 SharedLibrary 输出格式
                // （由 `output_path` 的 `.dll`/`.so`/`.dylib` 扩展名自动判定，此处仅
                // 用于校验与用法提示）。两者配套使用时即 README 中的
                // `loom build --member utils --aot --shared --cabi`。
                let c_abi = args.iter().any(|a| a == "--cabi");
                let shared = args.iter().any(|a| a == "--shared");
                // `-b photon` 由 **Aura 自举编译器**（`aura/compiler/.../Main.aura`
                // 的 `photonBuildExeFile`）实现，Rust 种子没有 photon 后端。
                // 旧实现把它静默忽略、照样产出 `.auc` 字节码 —— 调用方以为拿到了
                // PE/COFF 产物，实际是字节码，属**静默误报**。此处显式报错。
                if backend.as_deref() == Some("photon") {
                    eprintln!(
                        "Error: the `photon` backend is not available in the Rust seed compiler.\n\
                         Use the Aura-native compiler instead:\n\
                         \x20 build/bin/aura.exe build -b photon <entry> --output <path>\n\
                         \x20 (or: aura/compiler/aura/lang/compiler/Main.aura with -b photon)"
                    );
                    process::exit(1);
                }
                // 其余未知后端保持旧行为（忽略），`aot` 由 `--aot` 选择。
                match fs::read_to_string(entry) {
                    Ok(source) => {
                        // 先解析 import，内联所有引用的模块
                        let expanded = compiler::codegen::resolve_aura_imports(
                            &source,
                            Some(entry),
                        );
                        let output_path = std::path::Path::new(output);
                        if use_aot {
                            // AOT 编译：生成原生可执行文件 / 动态库
                            let mut options = compiler::codegen::aot::AotOptions::default();
                            options.c_abi = c_abi;
                            if shared && !output.ends_with(".dll")
                                && !output.ends_with(".so")
                                && !output.ends_with(".dylib")
                            {
                                eprintln!(
                                    "Error: --shared requires the output to be a library path \
                                     (.dll/.so/.dylib): {}",
                                    output
                                );
                                process::exit(1);
                            }
                            match compiler::codegen::aot::aot_compile(&expanded, output_path, options) {
                                Ok(_) => {
                                    println!("[ok] {} built", output);
                                }
                                Err(e) => {
                                    eprintln!("Error: {}", e);
                                    process::exit(1);
                                }
                            }
                        } else {
                            // Bytecode 编译：生成 .auc 文件
                            match compiler::codegen::compile_source(&expanded) {
                                Ok(module) => {
                                    // 序列化模块到 .auc 文件
                                    match compiler::codegen::serialize::write_auc(output, &module) {
                                        Ok(_) => {
                                            println!("[ok] {} built", output);
                                        }
                                        Err(e) => {
                                            eprintln!("Error writing {}: {}", output, e);
                                            process::exit(1);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Error: {}", e);
                                    process::exit(1);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading file: {}", e);
                        process::exit(1);
                    }
                }
            } else {
                match fs::read_to_string(entry) {
                    Ok(source) => {
                        // 先解析 import，内联所有引用的模块
                        let expanded = compiler::codegen::resolve_aura_imports(
                            &source,
                            Some(entry),
                        );
                        match compiler::codegen::compile_source(&expanded) {
                            Ok(module) => {
                                println!("[ok] {} compiled ({} functions)", entry, module.functions.len());
                                if args[1] == "run" {
                                    // Execute the compiled module
                                    let mut opts = compiler::vm::VmOptions::default();
                                    // P3.4: 把入口脚本所在目录及其父目录加入 FFI 库搜索路径，
                                    // 使 `extern "C" "utils"` 这类声明能找到 workspace 内的
                                    // `libs/<name>.dll` 产物（见 interp.rs::ensure_lib_loaded）。
                                    opts.lib_search_dirs = ffi_search_dirs(entry);
                                    match compiler::vm::Vm::new(&module, opts) {
                                        Ok(mut vm) => {
                                            match vm.run() {
                                                Ok(val) => {
                                                    if let compiler::vm::Value::Str(s) = val {
                                                        println!("{}", s);
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("Runtime error: {}", e);
                                                    process::exit(1);
                                                }
                                            }
                                            if let Some(code) = vm.requested_exit_code() {
                                                process::exit(code);
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("VM init error: {}", e);
                                            process::exit(1);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading file: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}

/// P3.4: 生成 FFI 动态库搜索目录列表（按优先级）。
///
/// 顺序：
/// 1. 当前工作目录（覆盖 `cd examples/ext_ffi_demo && aura run demo_cffi/src/main.aura`）
/// 2. 入口脚本所在目录（覆盖 `aura run demo_cffi/src/main.aura`，CWD 在仓库根）
/// 3. 入口脚本目录的各级父目录，直到仓库根（覆盖 workspace 内 `libs/<name>/libs/*.dll`）
///
/// 只返回去重后的、真实存在的目录，避免候选路径表被大量无效路径撑爆。
fn ffi_search_dirs(entry: &str) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    let mut push = |p: std::path::PathBuf| {
        if let Ok(canon) = p.canonicalize() {
            let s = canon.to_string_lossy().to_string();
            if !dirs.contains(&s) {
                dirs.push(s);
            }
        }
    };
    if let Ok(cwd) = std::env::current_dir() {
        push(cwd);
    }
    if let Some(parent) = std::path::Path::new(entry).parent() {
        let mut cur = parent.to_path_buf();
        // 向上最多 6 级（仓库根 / workspace 根一般都在其中）
        for _ in 0..6 {
            push(cur.clone());
            match cur.parent() {
                Some(p) if p.as_os_str().len() > 0 => cur = p.to_path_buf(),
                _ => break,
            }
        }
    }
    dirs
}

/// 递归收集 `dir` 下的全部 `.aura` 文件（不跟随符号链接，跳过隐藏目录）。
fn collect_aura_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_aura_files(&path, out);
        } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
            out.push(path);
        }
    }
}