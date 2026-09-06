//! Aura 编译器构建脚本。
//!
//! 职责：
//! 1. 从项目根目录 `Cargo.toml` 的 `[workspace.metadata.aura]` 读取外部工具链配置
//! 2. 当启用 `llvm` feature 时，探测本机 LLVM 安装路径，输出 `cargo:rustc-env`
//!    环境变量，供运行时 `aot` 模块使用。
//! 3. 探测方式按优先级：
//!    - 配置文件中 `llvm-home` 显式指定
//!    - 环境变量 `AURA_LLVM_HOME`（显式指定）
//!    - 环境变量 `LLVM_CONFIG`（指向 llvm-config 二进制）
//!    - 配置文件中 `llvm-search-paths`（按优先级探测）
//!    - 常见系统路径（`/usr/lib/llvm-*`, `C:\Program Files\LLVM`, 等）
//! 4. 输出 LLVM 版本（如果检测到），供编译时检查。
//!
//! 对应 技术方案 §9.7.2 build.rs。

use std::env;
use std::path::{Path, PathBuf};

const AURA_LLVM_HOME_ENV: &str = "AURA_LLVM_HOME";
const LLVM_CONFIG_ENV: &str = "LLVM_CONFIG";

/// 从根 Cargo.toml 读取的 `[workspace.metadata.aura]` 配置
#[derive(Debug, Default)]
struct AuraConfig {
    llvm_home: Option<String>,
    llvm_search_paths: Vec<String>,
    c_compiler_windows: Option<String>,
    c_compiler_unix: Option<String>,
    cross_linker_aarch64: Option<String>,
    cross_linker_armv7: Option<String>,
}

fn main() {
    // 读取根 Cargo.toml 中的 Aura 配置
    let config = read_aura_config();

    // 将配置输出为编译时环境变量，供运行时代码通过 option_env! 读取
    if let Some(ref home) = config.llvm_home {
        println!("cargo:rustc-env=AURA_CONFIG_LLVM_HOME={}", home);
    }
    if !config.llvm_search_paths.is_empty() {
        let joined = config.llvm_search_paths.join(";");
        println!("cargo:rustc-env=AURA_CONFIG_LLVM_SEARCH_PATHS={}", joined);
    }
    if let Some(ref c) = config.c_compiler_windows {
        println!("cargo:rustc-env=AURA_CONFIG_C_COMPILER_WINDOWS={}", c);
    }
    if let Some(ref c) = config.c_compiler_unix {
        println!("cargo:rustc-env=AURA_CONFIG_C_COMPILER_UNIX={}", c);
    }
    if let Some(ref c) = config.cross_linker_aarch64 {
        println!("cargo:rustc-env=AURA_CONFIG_CROSS_LINKER_AARCH64={}", c);
    }
    if let Some(ref c) = config.cross_linker_armv7 {
        println!("cargo:rustc-env=AURA_CONFIG_CROSS_LINKER_ARMV7={}", c);
    }

    // 探测 LLVM 安装路径
    let home = detect_llvm_home(&config);

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
        println!(
            "cargo:warning=Aura: 请在 Cargo.toml 的 [workspace.metadata.aura] 中设置 llvm-home"
        );
        println!("cargo:warning=Aura: 或设置 AURA_LLVM_HOME 环境变量");
    }

    // 常规构建指令
    println!("cargo:rerun-if-env-changed=AURA_LLVM_HOME");
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG");
    println!("cargo:rerun-if-changed=build.rs");
}

/// 从根 Cargo.toml 读取 `[workspace.metadata.aura]` 配置
fn read_aura_config() -> AuraConfig {
    let manifest_dir = match env::var("CARGO_MANIFEST_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => return AuraConfig::default(),
    };

    // 向上查找 workspace 根目录（包含根 Cargo.toml 且有 [workspace] 节）
    let workspace_root = find_workspace_root(&manifest_dir);
    let cargo_toml_path = workspace_root.join("Cargo.toml");

    if !cargo_toml_path.exists() {
        return AuraConfig::default();
    }

    let content = match std::fs::read_to_string(&cargo_toml_path) {
        Ok(c) => c,
        Err(e) => {
            println!("cargo:warning=Aura: 无法读取根 Cargo.toml: {}", e);
            return AuraConfig::default();
        }
    };

    let doc: toml::Value = match content.parse() {
        Ok(d) => d,
        Err(e) => {
            println!("cargo:warning=Aura: 无法解析根 Cargo.toml: {}", e);
            return AuraConfig::default();
        }
    };

    // 导航到 [workspace.metadata.aura]
    let Some(meta) =
        doc.get("workspace").and_then(|w| w.get("metadata")).and_then(|m| m.get("aura"))
    else {
        return AuraConfig::default();
    };

    let string = |key: &str| -> Option<String> {
        meta.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    };

    let string_array = |key: &str| -> Vec<String> {
        meta.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    };

    AuraConfig {
        llvm_home: string("llvm-home"),
        llvm_search_paths: string_array("llvm-search-paths"),
        c_compiler_windows: string("c-compiler-windows"),
        c_compiler_unix: string("c-compiler-unix"),
        cross_linker_aarch64: string("cross-linker-aarch64"),
        cross_linker_armv7: string("cross-linker-armv7"),
    }
}

/// 向上查找 workspace 根目录
fn find_workspace_root(manifest_dir: &Path) -> PathBuf {
    let mut current = manifest_dir.to_path_buf();
    loop {
        let candidate = current.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                if content.contains("[workspace]") {
                    return current.clone();
                }
            }
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    manifest_dir.to_path_buf()
}

/// 探测 LLVM 安装目录，按优先级尝试多种策略。
fn detect_llvm_home(config: &AuraConfig) -> Option<PathBuf> {
    // 1. 配置文件中 llvm-home 显式指定
    if let Some(ref v) = config.llvm_home {
        let p = PathBuf::from(v);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. 环境变量
    if let Ok(v) = env::var(AURA_LLVM_HOME_ENV) {
        let p = PathBuf::from(v);
        if p.exists() {
            return Some(p);
        }
    }

    // 3. 通过 LLVM_CONFIG 反推
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

    // 4. 配置文件中的 llvm-search-paths
    for path_str in &config.llvm_search_paths {
        let p = PathBuf::from(path_str);
        if p.exists() {
            return Some(p);
        }
    }

    // 5. 常见系统路径（Windows）
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
                            if let Some(major) =
                                rest.split('.').next().and_then(|s| s.parse::<u32>().ok())
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

    // 6. 常见系统路径（Linux / macOS）
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
    let llvm_config = home
        .join("bin")
        .join({ if cfg!(target_os = "windows") { "llvm-config.exe" } else { "llvm-config" } });
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
