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
use std::sync::{Arc, Mutex};

use anyhow::Result;
use clap::{Parser, Subcommand};

use aura_loom::cache::local::LocalCache;
use aura_loom::cache::remote::{CacheService, RemoteCacheConfig};
use aura_loom::manifest::parse;
use aura_loom::manifest::priority::{CliOverrides, ResolvedBuildConfig, resolve_build_config};
use aura_loom::manifest::LoomManifest;
use aura_loom::plugin::PluginRegistry;
use aura_loom::task::scheduler::{Scheduler, SchedulerConfig};
use aura_loom::lifecycle::phases::{build_standard_task_graph, build_task_graph_with_plugins};

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
    Clean {
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
    },
    /// 解析依赖
    Resolve {
        /// 离线模式
        #[arg(long)]
        offline: bool,
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
    },
    /// 打包为 .auz
    Package {
        /// 输出路径
        #[arg(long)]
        output: Option<String>,
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
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

#[derive(Parser, Debug, Clone, Default)]
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
    /// 禁用缓存
    #[arg(long)]
    no_cache: bool,
    /// 仅显示执行计划
    #[arg(long)]
    dry_run: bool,
    /// 详细输出
    #[arg(long)]
    verbose: bool,
    /// 清理后重编
    #[arg(long)]
    clean: bool,
    /// 指定输出目录
    #[arg(long)]
    out_dir: Option<String>,
    /// 指定缓存目录
    #[arg(long)]
    cache_dir: Option<String>,
}

/// 从 BuildArgs 提取 CLI 覆盖配置
fn build_cli_overrides(args: &BuildArgs) -> CliOverrides {
    CliOverrides {
        opt_level: args.opt,
        debug: args.debug,
        target: args.target.clone(),
        emit_package: None,
        parallel: if args.parallel.is_some() { Some(true) } else { None },
        parallel_jobs: args.parallel,
        out_dir: args.out_dir.clone(),
        cache_dir: args.cache_dir.clone(),
        profile: args.profile.clone(),
    }
}

/// 从 BuildArgs 构建调度配置
fn build_scheduler_config(args: &BuildArgs, resolved: &ResolvedBuildConfig) -> SchedulerConfig {
    SchedulerConfig {
        parallel: args.parallel.is_none() && resolved.parallel,
        max_jobs: args.parallel.unwrap_or(resolved.parallel_jobs),
        use_cache: !args.no_cache,
        dry_run: args.dry_run,
        verbose: args.verbose,
        clean: args.clean,
    }
}

/// 加载项目配置并执行任务
fn load_and_execute(
    dir: &str,
    phase: &str,
    args: &BuildArgs,
) -> Result<()> {
    let project_dir = PathBuf::from(dir);
    let manifest_path = project_dir.join("aura.toml");

    if !manifest_path.exists() {
        anyhow::bail!("未找到 {}（请先运行 loom new 或检查目录）", manifest_path.display());
    }

    // 1. 解析配置
    let manifest = parse::parse_from_file(&manifest_path)?;
    println!("项目: {} v{}", manifest.name, manifest.version);

    // 2. 解析构建配置（应用 CLI 覆盖 + profile）
    let cli_overrides = build_cli_overrides(args);
    let resolved_config = resolve_build_config(&manifest, &cli_overrides);

    // 3. 创建本地缓存
    let cache_dir = project_dir.join(&resolved_config.cache_dir);
    let cache = if args.no_cache {
        None
    } else {
        Some(Arc::new(Mutex::new(LocalCache::new(&cache_dir)?)))
    };

    // 3.5. B3.4: 创建缓存服务（本地 + 远程）
    let cache_service = if args.no_cache {
        None
    } else {
        let local_cache = cache.clone().unwrap();
        let remote_config = RemoteCacheConfig::from_build_config(&resolved_config);
        Some(Arc::new(Mutex::new(CacheService::new(
            local_cache,
            remote_config,
            Arc::new(resolved_config.clone()),
        )?)))
    };

    // 4. 加载插件注册表（B4）
    let plugin_registry = match PluginRegistry::from_manifest(&manifest, &project_dir) {
        Ok(registry) => {
            if registry.len() > 0 {
                println!("插件: {} 个 ({} )", registry.len(), registry.names().join(", "));
            }
            Some(Arc::new(registry))
        }
        Err(e) => {
            eprintln!("⚠ 插件加载失败: {} (继续构建，但不含插件任务)", e);
            None
        }
    };

    // 5. 构建任务图（含插件注册的任务）
    let graph = if let Some(ref registry) = plugin_registry {
        match build_task_graph_with_plugins(&manifest, &project_dir, registry, &resolved_config) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("⚠ 插件任务图构建失败: {} (回退到标准任务图)", e);
                build_standard_task_graph(&manifest, &project_dir)
            }
        }
    } else {
        build_standard_task_graph(&manifest, &project_dir)
    };

    // 5. 确定根任务
    let root_task = match phase {
        "build" => "install",
        "compile" => "compile-main",
        "test" => if graph.contains("run-tests") { "run-tests" } else { "compile-main" },
        "clean" => "clean",
        "resolve" => "resolve",
        "package" => "package",
        "verify" => "verify",
        "install" => "install",
        "publish" => "publish",
        "run" => "run",
        "watch" => "watch",
        _ => "build",
    };

    // 7. 构建调度器并执行（B4.5: 传递插件注册表）
    let scheduler_config = build_scheduler_config(args, &resolved_config);
    let scheduler = Scheduler::with_plugins(
        Arc::new(graph),
        Arc::new(resolved_config),
        cache,
        cache_service,
        scheduler_config,
        plugin_registry,
    );

    scheduler.execute(root_task)?;

    // 8. 打印结果
    print_results(&scheduler);

    Ok(())
}

/// 打印执行结果
fn print_results(scheduler: &Scheduler) {
    let results = scheduler.results();
    if results.is_empty() {
        return;
    }

    println!();
    println!("═══ 构建结果 ═══");
    for result in &results {
        if !result.executed {
            if result.cache_hit {
                let source = if result.cache_source.is_empty() {
                    String::new()
                } else {
                    format!(" (来源: {})", result.cache_source)
                };
                println!("  ⏭  {} — {}{}", result.task_name, result.message, source);
            } else {
                println!("  ⏭  {} (跳过)", result.task_name);
            }
        } else if result.success {
            let elapsed = if result.elapsed_ms > 0 {
                format!(" [{}ms]", result.elapsed_ms)
            } else {
                String::new()
            };
            println!("  ✓ {} — {}{}", result.task_name, result.message, elapsed);
        } else {
            println!("  ✗ {} — {}", result.task_name, result.message);
        }
    }

    let total_ms: u64 = results.iter().map(|r| r.elapsed_ms).sum();
    let executed = results.iter().filter(|r| r.executed).count();
    let skipped = results.iter().filter(|r| !r.executed).count();
    let cache_hits = results.iter().filter(|r| r.cache_hit).count();
    println!("═══ {} 执行, {} 跳过 ({} 缓存命中), 总计 {}ms ═══", executed, skipped, cache_hits, total_ms);
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Version => {
            println!("loom 0.1.0");
            println!("Aura 构建系统 — 纯 TOML 配置、任务 DAG、增量构建");
            Ok(())
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
            Ok(())
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
            Ok(())
        }
        Command::Build(args) => load_and_execute(&args.dir, "build", &args),
        Command::Compile(args) => load_and_execute(&args.dir, "compile", &args),
        Command::Test(args) => load_and_execute(&args.dir, "test", &args),
        Command::Run(args) => load_and_execute(&args.dir, "run", &args),
        Command::Watch(args) => load_and_execute(&args.dir, "watch", &args),
        Command::Clean { dir } => {
            let args = BuildArgs { dir: dir.clone(), ..Default::default() };
            load_and_execute(&args.dir, "clean", &args)
        }
        Command::Resolve { offline, dir } => {
            let args = BuildArgs { dir: dir.clone(), ..Default::default() };
            let _ = offline; // TODO: 实现离线模式
            load_and_execute(&args.dir, "resolve", &args)
        }
        Command::Package { output, dir } => {
            let args = BuildArgs { dir: dir.clone(), ..Default::default() };
            let _ = output; // TODO: 使用输出路径
            load_and_execute(&args.dir, "package", &args)
        }
        Command::Verify { file } => {
            println!("[Phase B6] verify 尚未实现 (file: {})", file);
            Ok(())
        }
        Command::Install { file } => {
            println!("[Phase B6] install 尚未实现 (file: {})", file);
            Ok(())
        }
        Command::Publish { dir } => {
            println!("[Phase B6] publish 尚未实现 (dir: {:?})", dir);
            Ok(())
        }
        Command::Deps { tree, outdated } => {
            println!("[Phase B1] deps 尚未实现 (tree: {}, outdated: {})", tree, outdated);
            Ok(())
        }
        Command::Ci { steps } => {
            println!("[Phase B6] ci 尚未实现 (steps: {:?})", steps);
            Ok(())
        }
        Command::Wrapper { command } => match command {
            WrapperCommand::Install => {
                println!("[Phase B5] wrapper install 尚未实现");
                Ok(())
            }
        },
    }
}
