//! loom CLI 入口
//!
//! loom 是 Aura 语言的**构建系统**（对标 webpack / gradlew），仅负责构建编排。
//! 包生态职责（install / publish / deps / package / verify）由 `aura` 承担。
//! 详见 `docs/多进程与CLI架构分析报告.md` §4 职责边界收敛。
//!
//! 用法：
//!   loom build [--profile <p>] [--target <t>] [--parallel <n>]
//!   loom compile [--profile <p>]
//!   loom test [--profile <p>] [--parallel <n>]
//!   loom run [--profile <p>]
//!   loom clean
//!   loom resolve [--offline]
//!   loom ci [--steps <list>]
//!   loom watch [--profile <p>]
//!   loom check-config [--dir <path>]
//!   loom new <name> [--template <t>]    # 别名，等价于 aura new
//!   loom wrapper install
//!   loom version
//!   loom help

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use clap::{Parser, Subcommand};

use aura_loom::cache::local::LocalCache;
use aura_loom::cache::remote::{CacheService, RemoteCacheConfig};
use aura_loom::lifecycle::phases::{build_standard_task_graph, build_task_graph_with_plugins};
use aura_loom::manifest::parse;
use aura_loom::manifest::priority::{CliOverrides, ResolvedBuildConfig, resolve_build_config};
use aura_loom::plugin::PluginRegistry;
use aura_loom::task::scheduler::{Scheduler, SchedulerConfig};
use aura_loom::workspace::Workspace;

#[derive(Parser, Debug)]
#[command(name = "loom", version = "0.1.0", about = "Aura 构建系统")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 完整构建（clean → resolve → compile → test）
    Build(BuildArgs),
    /// 仅编译
    Compile(BuildArgs),
    /// 编译 + 运行测试
    Test(BuildArgs),
    /// 语法/语义检查
    Check {
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
    },
    /// 编译 + 运行
    Run(BuildArgs),
    /// 清理构建产物
    Clean {
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
    },
    /// 解析依赖（解析构建输入：源码集 + 依赖路径）
    Resolve {
        /// 离线模式
        #[arg(long)]
        offline: bool,
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
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
    /// 生成 IDE 项目文件
    Ide {
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
        /// 包含所有文件
        #[arg(long)]
        all_files: bool,
    },
    /// 生成文档
    Doc {
        /// 输出目录
        #[arg(long, default_value = "docs")]
        output: String,
        /// 仅生成指定模块的文档
        #[arg(long)]
        module: Option<String>,
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
    },
    /// 格式化源码
    Fmt {
        /// 项目目录
        #[arg(long, default_value = ".")]
        dir: String,
        /// 仅检查不修改
        #[arg(long)]
        check: bool,
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
    /// JSON 格式输出
    #[arg(long)]
    json: bool,
    /// 跳过指定阶段（逗号分隔）
    #[arg(long)]
    skip: Option<String>,
    /// 仅运行指定阶段（逗号分隔）
    #[arg(long)]
    only: Option<String>,
    /// 从指定阶段开始
    #[arg(long)]
    from: Option<String>,
    /// 清理后重编
    #[arg(long)]
    clean: bool,
    /// 指定输出目录
    #[arg(long)]
    out_dir: Option<String>,
    /// 指定缓存目录
    #[arg(long)]
    cache_dir: Option<String>,
    /// 指定 Workspace 成员
    #[arg(long)]
    member: Option<String>,
    /// 包含依赖成员
    #[arg(long)]
    with_deps: bool,
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
/// 阶段名到任务名的映射
fn phase_to_task_name(phase: &str) -> Option<&'static str> {
    match phase {
        "clean" => Some("clean"),
        "resolve" => Some("resolve"),
        "compile" => Some("compile-main"),
        "test" => Some("run-tests"),
        "package" => Some("package"),
        "verify" => Some("verify"),
        "check" => Some("check"),
        "install" => Some("install"),
        "deploy" | "publish" => Some("publish"),
        "run" => Some("run"),
        "watch" => Some("watch"),
        _ => None,
    }
}

/// 过滤任务图（根据 --skip / --only / --from 参数）
fn filter_task_graph(
    mut graph: aura_loom::task::TaskGraph,
    skip: Option<&str>,
    only: Option<&str>,
    from: Option<&str>,
) -> aura_loom::task::TaskGraph {
    use aura_loom::task::TaskGraph;

    let standard_order = vec![
        "clean",
        "resolve",
        "compile-main",
        "compile-test",
        "compile-bench",
        "run-tests",
        "package",
        "verify",
        "check",
        "install",
        "publish",
        "run",
        "watch",
    ];

    // 计算保留的阶段索引范围
    let keep_from: Option<usize> = from.map(|f| {
        standard_order.iter().position(|t| *t == phase_to_task_name(f).unwrap_or(f)).unwrap_or(0)
    });
    let keep_to: Option<usize> = only.and_then(|o| {
        let phases: Vec<&str> = o.split(',').map(|s| s.trim()).collect();
        let mut max_idx = 0;
        for p in &phases {
            if let Some(task_name) = phase_to_task_name(p) {
                if let Some(idx) = standard_order.iter().position(|t| *t == task_name) {
                    max_idx = max_idx.max(idx);
                }
            }
        }
        Some(max_idx)
    });

    // 收集要跳过的任务名
    let skip_tasks: std::collections::HashSet<&str> = skip
        .map(|s| s.split(',').map(|p| p.trim()).filter_map(|p| phase_to_task_name(p)).collect())
        .unwrap_or_default();

    // 收集要保留的任务名（如果指定了 --only）
    let only_tasks: Option<std::collections::HashSet<&str>> = only
        .map(|s| s.split(',').map(|p| p.trim()).filter_map(|p| phase_to_task_name(p)).collect());

    // 过滤任务图
    let mut new_graph = TaskGraph::new();
    for task in graph.all() {
        let task_name = task.name.as_str();

        // 检查是否在跳过列表中
        if skip_tasks.contains(task_name) {
            continue;
        }

        // 检查是否在 only 列表中
        if let Some(ref only_set) = only_tasks {
            if !only_set.contains(task_name) {
                continue;
            }
        }

        // 检查是否在 from 范围之后
        if let Some(from_idx) = keep_from {
            if let Some(idx) = standard_order.iter().position(|t| *t == task_name) {
                if idx < from_idx {
                    continue;
                }
            }
        }

        // 检查是否在 to 范围之前
        if let Some(to_idx) = keep_to {
            if let Some(idx) = standard_order.iter().position(|t| *t == task_name) {
                if idx > to_idx {
                    continue;
                }
            }
        }

        new_graph.add_task(task.clone());
    }

    new_graph
}

fn load_and_execute(dir: &str, phase: &str, args: &BuildArgs) -> Result<()> {
    let project_dir = PathBuf::from(dir);
    let manifest_path = project_dir.join("aura.toml");

    if !manifest_path.exists() {
        anyhow::bail!(
            "未找到 {}（请先运行 loom new 或检查目录）",
            manifest_path.display()
        );
    }

    // 1. 解析配置
    let manifest = parse::parse_from_file(&manifest_path)?;
    println!("项目: {} v{}", manifest.name, manifest.version);

    // 2. 解析构建配置（应用 CLI 覆盖 + profile）
    let cli_overrides = build_cli_overrides(args);
    let resolved_config = resolve_build_config(&manifest, &cli_overrides);

    // 3. 创建本地缓存
    let cache_dir = project_dir.join(&resolved_config.cache_dir);
    let cache =
        if args.no_cache { None } else { Some(Arc::new(Mutex::new(LocalCache::new(&cache_dir)?))) };

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
                println!(
                    "插件: {} 个 ({} )",
                    registry.len(),
                    registry.names().join(", ")
                );
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

    // 5.5 B5: Workspace 模式处理
    if args.member.is_some() || manifest.workspace.is_some() {
        if let Some(ws_config) = &manifest.workspace {
            if !ws_config.members.is_empty() {
                match Workspace::from_manifest(&manifest, &project_dir) {
                    Ok(workspace) => {
                        let selection =
                            Workspace::resolve_selection(args.member.as_deref(), args.with_deps);
                        let selected = workspace.selected_members(&selection);
                        println!(
                            "Workspace: {} 个成员, 选择 {} 个",
                            workspace.len(),
                            selected.len()
                        );
                        for m in &selected {
                            println!("  → {} v{} {}", m.name, m.version, m.path.display());
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠ Workspace 解析失败: {}", e);
                    }
                }
            }
        }
    }

    // 5.6: 生命周期裁剪（--skip / --only / --from）
    let graph = filter_task_graph(
        graph,
        args.skip.as_deref(),
        args.only.as_deref(),
        args.from.as_deref(),
    );

    // 5. 确定根任务
    //    第四阶段：build 生命周期收缩为 clean → resolve → compile → test，
    //    不再包含 package / verify / install / deploy（已归 aura 生态层）。
    let root_task = match phase {
        "build" => {
            if graph.contains("run-tests") {
                "run-tests"
            } else {
                "compile-main"
            }
        }
        "compile" => "compile-main",
        "test" => {
            if graph.contains("run-tests") {
                "run-tests"
            } else {
                "compile-main"
            }
        }
        "clean" => "clean",
        "resolve" => "resolve",
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
    println!(
        "═══ {} 执行, {} 跳过 ({} 缓存命中), 总计 {}ms ═══",
        executed, skipped, cache_hits, total_ms
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Doc {
            output,
            module,
            dir,
        } => {
            let project_dir = PathBuf::from(&dir);
            let output_dir = project_dir.join(&output);

            std::fs::create_dir_all(&output_dir)
                .map_err(|e| anyhow::anyhow!("创建输出目录失败: {}", e))?;

            // 生成标准库文档
            let std_docs = compiler::docgen::generate_docs(&output_dir)
                .map_err(|e| anyhow::anyhow!("标准库文档生成失败: {}", e))?;
            println!("✓ 标准库文档: {} 个文件", std_docs.len());

            // 从项目源码生成文档
            let src_dir = project_dir.join("src");
            if src_dir.exists() {
                let mut user_doc_count = 0;
                for entry in walkdir::WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().map(|e| e == "aura").unwrap_or(false) {
                        let source = std::fs::read_to_string(path)
                            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", path.display(), e))?;
                        let module_name = path
                            .strip_prefix(&src_dir)
                            .unwrap_or(path)
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/")
                            .replace('/', ".");

                        let doc_content = format!(
                            "# {}\n\nSource: `{}`\n\n```\n```\n",
                            module_name,
                            path.strip_prefix(&project_dir).unwrap_or(path).display()
                        );
                        let doc_path = output_dir.join(format!("{}.md", module_name));
                        if let Some(parent) = doc_path.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        std::fs::write(&doc_path, doc_content).map_err(|e| {
                            anyhow::anyhow!("写入 {} 失败: {}", doc_path.display(), e)
                        })?;
                        user_doc_count += 1;
                    }
                }
                if user_doc_count > 0 {
                    println!("✓ 项目文档: {} 个文件", user_doc_count);
                }
            }

            if let Some(ref m) = module {
                println!("  模块过滤: {}", m);
            }

            println!("✓ 文档生成完成");
            println!("  输出: {}", output_dir.display());
            Ok(())
        }
        Command::Fmt { dir, check } => {
            let project_dir = PathBuf::from(&dir);
            let src_dir = project_dir.join("src");

            if !src_dir.exists() {
                println!("✓ 无源码目录，跳过格式化");
                return Ok(());
            }

            let mut formatted = 0;
            let mut errors = 0;

            for entry in walkdir::WalkDir::new(&src_dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().map(|e| e == "aura").unwrap_or(false) {
                    let source = match std::fs::read_to_string(path) {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("⚠ 读取 {} 失败: {}", path.display(), e);
                            errors += 1;
                            continue;
                        }
                    };

                    let formatted_source = compiler::lsp::format_source(&source);

                    if formatted_source != source {
                        if check {
                            println!(
                                "需要格式化: {}",
                                path.strip_prefix(&project_dir).unwrap_or(path).display()
                            );
                        } else {
                            std::fs::write(path, &formatted_source).map_err(|e| {
                                anyhow::anyhow!("写入 {} 失败: {}", path.display(), e)
                            })?;
                            println!(
                                "✓ 已格式化: {}",
                                path.strip_prefix(&project_dir).unwrap_or(path).display()
                            );
                        }
                        formatted += 1;
                    }
                }
            }

            if check {
                if formatted > 0 {
                    println!("⚠ {} 个文件需要格式化", formatted);
                    std::process::exit(1);
                } else {
                    println!("✓ 所有文件已格式化");
                }
            } else {
                println!("✓ 格式化完成: {} 个文件", formatted);
            }
            if errors > 0 {
                eprintln!("⚠ {} 个错误", errors);
            }

            Ok(())
        }
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
        Command::New {
            name,
            template,
        } => {
            // 第四阶段：loom new 作为 aura new 的薄别名，
            // 帮助新用户从构建系统入口创建项目。
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
            println!(
                "  模板: {}",
                template.unwrap_or_else(|| "default".to_string())
            );
            println!("  提示: loom new 等价于 aura new，包生态操作请使用 aura 命令");
            Ok(())
        }
        Command::Build(args) => load_and_execute(&args.dir, "build", &args),
        Command::Compile(args) => load_and_execute(&args.dir, "compile", &args),
        Command::Test(args) => load_and_execute(&args.dir, "test", &args),
        Command::Check { dir } => {
            let dir = PathBuf::from(dir);
            let manifest_path = dir.join("aura.toml");

            if !manifest_path.exists() {
                println!("✓ 无 aura.toml，跳过检查");
                return Ok(());
            }

            let manifest = parse::parse_from_file(&manifest_path)?;
            let errors = aura_loom::manifest::validate::validate_manifest(&manifest);

            if errors.is_empty() {
                println!(
                    "✓ 语法/语义检查通过（{} 个依赖）",
                    manifest.all_dependencies().len()
                );
            } else {
                println!("⚠ 语法/语义检查发现 {} 个问题:", errors.len());
                for e in &errors {
                    println!("  - {}", e);
                }
            }

            Ok(())
        }
        Command::Run(args) => load_and_execute(&args.dir, "run", &args),
        Command::Watch(args) => load_and_execute(&args.dir, "watch", &args),
        Command::Clean { dir } => {
            let args = BuildArgs {
                dir: dir.clone(),
                ..Default::default()
            };
            load_and_execute(&args.dir, "clean", &args)
        }
        Command::Resolve {
            offline,
            dir,
        } => {
            let args = BuildArgs {
                dir: dir.clone(),
                ..Default::default()
            };
            let _ = offline; // TODO: 实现离线模式
            load_and_execute(&args.dir, "resolve", &args)
        }
        Command::Ci { steps: _ } => {
            let project_dir = PathBuf::from(".");
            let ci_path = aura_loom::ci::CiConfig::config_path(&project_dir);

            if !ci_path.exists() {
                let default_path = aura_loom::ci::CiConfig::default_config_path(&project_dir);
                println!("⚠ 未找到 CI 配置 (.loom/.aura-ci.yml)");
                println!("  使用示例配置生成:");

                // 创建 .loom/ 目录
                if let Some(parent) = default_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| anyhow::anyhow!("创建 .loom/ 目录失败: {}", e))?;
                }

                let example = aura_loom::ci::CiConfig::example();
                let yaml = example.to_yaml()?;
                std::fs::write(&default_path, &yaml)?;
                println!("  ✓ 已生成示例配置: {}", default_path.display());
            } else {
                let config = aura_loom::ci::CiConfig::from_file(&ci_path)?;
                let warnings = config.validate()?;
                for w in &warnings {
                    eprintln!("⚠ {}", w);
                }

                let executor = aura_loom::ci::CiExecutor::new(config, &project_dir, true);
                let result = executor.execute()?;
                println!(
                    "\nCI dry-run 完成: {} 成功, {} 失败, {} 跳过",
                    result.success_count, result.failure_count, result.skip_count
                );
            }

            Ok(())
        }
        Command::Wrapper { command } => match command {
            WrapperCommand::Install => {
                let project_dir = PathBuf::from(".");
                let manifest_path = project_dir.join("aura.toml");
                if manifest_path.exists() {
                    let installer =
                        aura_loom::wrapper::installer::WrapperInstaller::from_project(&project_dir);
                    match installer {
                        Ok(installer) => {
                            let result = installer.install()?;
                            println!("✓ Wrapper 安装完成");
                            println!("  版本: {}", result.version);
                            println!("  路径: {}", result.executable_path.display());
                            println!("  新安装: {}", result.installed);
                        }
                        Err(e) => {
                            eprintln!("⚠ Wrapper 加载失败: {}", e);
                        }
                    }
                } else {
                    println!("⚠ 当前目录无 aura.toml，使用默认配置");
                    let installer = aura_loom::wrapper::installer::WrapperInstaller::new(
                        aura_loom::wrapper::WrapperConfig::default(),
                        &project_dir,
                    );
                    let scripts = installer.generate_wrapper_scripts()?;
                    std::fs::write("aura-wrapper", &scripts.bash_script)?;
                    std::fs::write("aura-wrapper.bat", &scripts.bat_script)?;
                    println!("✓ 生成 wrapper 脚本: aura-wrapper, aura-wrapper.bat");
                }
                Ok(())
            }
        },
        Command::Ide {
            dir,
            all_files,
        } => {
            let project_dir = PathBuf::from(&dir);
            let manifest_path = project_dir.join("aura.toml");

            if !manifest_path.exists() {
                eprintln!("错误: 当前目录无 aura.toml");
                return Err(anyhow::anyhow!("aura.toml 不存在"));
            }

            let manifest = parse::parse_from_file(&manifest_path)?;
            let mut project = aura_loom::ide::IdeProject::from_manifest(&manifest, &project_dir);

            if all_files {
                let scanner = aura_loom::ide::FileScanner::new(&project_dir, true);
                if let Ok(files) = scanner.scan() {
                    project = project.with_files(files);
                }
            }

            let output_path = aura_loom::ide::IdeProject::default_path(&project_dir);
            project.write_to(&output_path)?;

            println!("✓ IDE 项目文件已生成");
            println!("  路径: {}", output_path.display());
            println!("  项目: {} v{}", project.name, project.version);
            println!("  依赖: {} 个", project.dependencies.len());
            println!("  任务: {} 个", project.tasks.len());
            println!("  插件: {} 个", project.plugins.len());
            if let Some(ref files) = project.files {
                println!("  文件: {} 个", files.len());
            }
            Ok(())
        }
    }
}
