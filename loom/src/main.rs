//! loom CLI 入口
//!
//! 用法：
//!   loom build [--profile <p>] [--target <t>] [--parallel <n>]
//!   loom test [--profile <p>] [--parallel <n>]
//!   loom run [--profile <p>]
//!   loom clean
//!   loom resolve [--offline]
//!   loom package [--output <path>]
//!   loom verify <file.auz>
//!   loom install <file.auz>
//!   loom publish [--dir <path>]
//!   loom deps [--tree] [--outdated]
//!   loom ci [--steps <list>]
//!   loom watch [--profile <p>]
//!   loom check-config [--dir <path>]
//!   loom new <name> [--template <t>]
//!   loom wrapper install
//!   loom version
//!   loom help

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use aura_loom::manifest::parse;

#[derive(Parser, Debug)]
#[command(name = "loom", version = "0.1.0", about = "Aura 构建系统")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 完整构建（clean → compile → test → package）
    Build(BuildArgs),
    /// 仅编译
    Compile(BuildArgs),
    /// 编译 + 运行测试
    Test(BuildArgs),
    /// 编译 + 运行
    Run(BuildArgs),
    /// 清理构建产物
    Clean,
    /// 解析依赖
    Resolve {
        /// 离线模式
        #[arg(long)]
        offline: bool,
    },
    /// 打包为 .auz
    Package {
        /// 输出路径
        #[arg(long)]
        output: Option<String>,
    },
    /// 验证制品完整性
    Verify { file: String },
    /// 安装到本地注册表
    Install { file: String },
    /// 发布到远程仓库
    Publish {
        /// 项目目录
        #[arg(long)]
        dir: Option<String>,
    },
    /// 显示依赖树
    Deps {
        /// 树状显示
        #[arg(long)]
        tree: bool,
        /// 显示过期依赖
        #[arg(long)]
        outdated: bool,
    },
    /// CI 流水线
    Ci {
        /// 流水线步骤列表
        #[arg(long)]
        steps: Option<String>,
    },
    /// 监听源码变化，增量重编
    Watch(BuildArgs),
    /// 校验 aura.toml 配置
    CheckConfig {
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
    },
    /// 创建新项目
    New {
        /// 项目名
        name: String,
        /// 模板
        #[arg(long)]
        template: Option<String>,
    },
    /// 构建包装器管理
    Wrapper {
        #[command(subcommand)]
        command: WrapperCommand,
    },
    /// 显示版本
    Version,
}

#[derive(Subcommand, Debug)]
enum WrapperCommand {
    /// 安装当前项目指定版本
    Install,
}

#[derive(Parser, Debug, Clone)]
struct BuildArgs {
    /// 激活的 profile
    #[arg(long)]
    profile: Option<String>,
    /// 目标平台三元组
    #[arg(long)]
    target: Option<String>,
    /// 并行任务数
    #[arg(long)]
    parallel: Option<u32>,
    /// 优化级别
    #[arg(long)]
    opt: Option<u8>,
    /// 调试信息
    #[arg(long)]
    debug: Option<bool>,
    /// 项目目录
    #[arg(long, default_value = ".")]
    dir: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Version => {
            println!("loom 0.1.0");
            println!("Aura 构建系统 — 纯 TOML 配置、任务 DAG、增量构建");
        }
        Command::CheckConfig { dir } => {
            let manifest_path = PathBuf::from(&dir).join("aura.toml");
            if !manifest_path.exists() {
                anyhow::bail!("未找到 {}", manifest_path.display());
            }
            let manifest = parse::parse_from_file(&manifest_path)?;
            let errors = aura_loom::manifest::validate::validate_manifest(&manifest);
            if errors.is_empty() {
                println!("✓ 配置校验通过: {}", manifest.name);
                println!("  版本: {}", manifest.version);
                println!("  入口: {}", manifest.entry);
                println!("  依赖: {} 个", manifest.all_dependencies().len());
                println!("  源码集: {}", manifest.build.source_sets.len());
                println!("  Profile: {}", manifest.profiles.len());
                println!("  自定义任务: {}", manifest.tasks.len());
            } else {
                println!("✗ 配置校验失败:");
                for err in &errors {
                    println!("  - {}", err);
                }
                std::process::exit(1);
            }
        }
        Command::New { name, template } => {
            let manifest = parse::default_manifest(&name);
            let dir = PathBuf::from(&name);
            std::fs::create_dir_all(&dir)?;
            let toml_str = toml::to_string_pretty(&manifest)?;
            std::fs::write(dir.join("aura.toml"), toml_str)?;
            std::fs::create_dir_all(dir.join("src"))?;
            std::fs::write(
                dir.join("src/main.aura"),
                format!("fun main() {{\n    println(\"Hello from {}!\")\n}}\n", name),
            )?;
            println!("✓ 项目创建成功: {}", name);
            println!("  目录: {}", dir.display());
            println!("  模板: {}", template.unwrap_or_else(|| "default".to_string()));
        }
        Command::Build(args) | Command::Compile(args) | Command::Test(args) | Command::Run(args)
        | Command::Watch(args) => {
            let dir = PathBuf::from(&args.dir);
            let manifest_path = dir.join("aura.toml");
            if !manifest_path.exists() {
                anyhow::bail!("未找到 {}（请先运行 loom new 或检查目录）", manifest_path.display());
            }
            let manifest = parse::parse_from_file(&manifest_path)?;
            println!("项目: {} v{}", manifest.name, manifest.version);
            println!("入口: {}", manifest.entry);
            println!("依赖: {} 个", manifest.all_dependencies().len());
            println!("[Phase L2] 任务引擎尚未实现");
        }
        Command::Clean => {
            println!("[Phase L2] clean 尚未实现");
        }
        Command::Resolve { offline } => {
            println!("[Phase L6] resolve 尚未实现 (offline: {})", offline);
        }
        Command::Package { output } => {
            println!("[Phase L2] package 尚未实现 (output: {:?})", output);
        }
        Command::Verify { file } => {
            println!("[Phase L2] verify 尚未实现 (file: {})", file);
        }
        Command::Install { file } => {
            println!("[Phase L6] install 尚未实现 (file: {})", file);
        }
        Command::Publish { dir } => {
            println!("[Phase L6] publish 尚未实现 (dir: {:?})", dir);
        }
        Command::Deps { tree, outdated } => {
            println!("[Phase L1] deps 尚未实现 (tree: {}, outdated: {})", tree, outdated);
        }
        Command::Ci { steps } => {
            println!("[Phase L6] ci 尚未实现 (steps: {:?})", steps);
        }
        Command::Wrapper { command } => match command {
            WrapperCommand::Install => {
                println!("[Phase L5] wrapper install 尚未实现");
            }
        },
    }

    Ok(())
}
