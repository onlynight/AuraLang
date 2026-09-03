//! Aura 编译器构建脚本。
//!
//! 职责：
//! 1. 当启用 `llvm` feature 时，探测本机 LLVM 安装路径，输出 `cargo:rustc-env`
//!    环境变量，供运行时 `aot` 模块使用。
//! 2. 探测方式按优先级：
//!    - 环境变量 `AURA_LLVM_HOME`（显式指定）
//!    - 环境变量 `LLVM_CONFIG`（指向 llvm-config 二进制）
//!    - 常见系统路径（`/usr/lib/llvm-*`, `C:\Program Files\LLVM`, 等）
//! 3. 输出 LLVM 版本（如果检测到），供编译时检查。
//!
//! 对应 技术方案 §9.7.2 build.rs。

use std::env;
use std::path::{Path, PathBuf};

const AURA_LLVM_HOME_ENV: &str = "AURA_LLVM_HOME";
const LLVM_CONFIG_ENV: &str = "LLVM_CONFIG";

fn main() {
    // 即使 `llvm` feature 未启用，也可以输出 LLVM 路径信息（作为信息性 cargo:warning）
    let home = detect_llvm_home();

    if let Some(ref home) = home {
        println!("cargo:rustc-env=AURA_LLVM_HOME={}", home.display());
        println!(
            "cargo:warning=Aura: LLVM 安装路径检测到 -> {}",
            home.display()
        );

        // 尝试获取版本
        if let Some(version) = llvm_version(home) {
            println!("cargo:rustc-env=AURA_LLVM_VERSION={}", version);
            println!("cargo:warning=Aura: LLVM 版本 -> {}", version);
        }
    } else if cfg!(feature = "llvm") {
        println!("cargo:warning=Aura: llvm feature 已启用但未检测到 LLVM 安装路径");
        println!("cargo:warning=Aura: 请设置 AURA_LLVM_HOME 或 LLVM_CONFIG 环境变量");
        println!("cargo:warning=Aura: 例如 AURA_LLVM_HOME=D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc");
    }

    // 常规构建指令：为不同目标输出库搜索路径（llvm-sys 也会做这些，这里仅提示）
    println!("cargo:rerun-if-env-changed=AURA_LLVM_HOME");
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG");
    println!("cargo:rerun-if-changed=build.rs");
}

/// 探测 LLVM 安装目录，按优先级尝试多种策略。
fn detect_llvm_home() -> Option<PathBuf> {
    // 1. 显式环境变量
    if let Ok(v) = env::var(AURA_LLVM_HOME_ENV) {
        let p = PathBuf::from(v);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. 通过 LLVM_CONFIG 反推
    if let Ok(v) = env::var(LLVM_CONFIG_ENV) {
        if let Some(parent) = PathBuf::from(v).parent() {
            // llvm-config 通常在 bin/ 下，所以 parent 是 bin，再 parent 才是 LLVM_HOME
            if let Some(home) = parent.parent() {
                if home.exists() {
                    return Some(home.to_path_buf());
                }
            }
            if parent.exists() {
                return Some(parent.to_path_buf());
            }
        }
    }

    // 3. 常见系统路径（Windows）
    if cfg!(target_os = "windows") {
        let candidates = [
            r"C:\Program Files\LLVM",
            r"C:\Program Files (x86)\LLVM",
        ];
        for c in &candidates {
            if Path::new(c).exists() {
                return Some(PathBuf::from(c));
            }
        }
        // 查找 C:\DevTools\LLVM\ 下最新的版本子目录
        let base = Path::new(r"C:\DevTools\LLVM");
        if base.exists() {
            if let Ok(entries) = std::fs::read_dir(base) {
                let mut versions: Vec<(u32, PathBuf)> = Vec::new();
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("clang+llvm-") {
                        if let Some(rest) = name_str.strip_prefix("clang+llvm-") {
                            if let Some(major) = rest
                                .split('.')
                                .next()
                                .and_then(|s| s.parse::<u32>().ok())
                            {
                                versions.push((major, entry.path()));
                            }
                        }
                    }
                }
                versions.sort_by(|a, b| b.0.cmp(&a.0));
                if let Some((_, path)) = versions.first() {
                    return Some(path.clone());
                }
            }
        }
    }

    // 4. 常见系统路径（Linux / macOS）
    let candidates = [
        "/usr/lib/llvm-17",
        "/usr/lib/llvm-18",
        "/usr/lib/llvm-19",
        "/usr/lib/llvm-20",
        "/usr/lib/llvm-21",
        "/usr/lib/llvm-22",
        "/usr/lib/llvm-23",
        "/usr/lib/llvm",
        "/opt/llvm",
        "/usr/local/opt/llvm",
        "/usr/local/llvm",
    ];
    for c in &candidates {
        if Path::new(c).exists() {
            return Some(PathBuf::from(c));
        }
    }

    None
}

/// 尝试从 LLVM 目录获取版本字符串。
fn llvm_version(home: &Path) -> Option<String> {
    // 尝试直接读取版本信息（llvm-config 是可靠方式）
    let llvm_config = home.join("bin").join({
        if cfg!(target_os = "windows") {
            "llvm-config.exe"
        } else {
            "llvm-config"
        }
    });
    if llvm_config.exists() {
        use std::process::Stdio;
        let mut cmd = std::process::Command::new(&llvm_config);
        cmd.arg("--version");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        if let Ok(child) = cmd.spawn() {
            if let Ok(output) = child.wait_with_output() {
                if output.status.success() {
                    let ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !ver.is_empty() {
                        return Some(ver);
                    }
                }
            }
        }
    }

    // 备选：从目录名解析
    if let Some(name) = home.file_name().and_then(|n| n.to_str()) {
        if let Some(rest) = name.strip_prefix("clang+llvm-") {
            if let Some(major) = rest.split('.').next() {
                return Some(format!("{}.x.x (from dir name)", major));
            }
        }
    }

    None
}
