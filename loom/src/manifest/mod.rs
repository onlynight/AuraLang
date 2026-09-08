//! 配置层：`aura.toml` 完整数据结构
//!
//! B1.1: 扩展 Manifest 数据结构
//! B1.2: SourceSet 数据结构 + 默认源码集
//! B1.3: 依赖配置分组
//! B1.4: 配置优先级
//! B1.5: 完整解析 + 验证

pub mod parse;
pub mod priority;
pub mod validate;

use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// 包元数据
// ═══════════════════════════════════════════════════════════════════════════════

/// loom 构建配置清单（`aura.toml`）
///
/// 对标 Cargo.toml，纯 TOML 声明式配置。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LoomManifest {
    /// 清单格式版本
    #[serde(default = "default_schema_version")]
    pub schema_version: String,

    /// 包名
    pub name: String,
    /// 版本
    pub version: String,

    /// 描述
    #[serde(default)]
    pub description: Option<String>,
    /// 作者列表
    #[serde(default)]
    pub authors: Vec<String>,
    /// 许可证
    #[serde(default)]
    pub license: Option<String>,
    /// 仓库 URL
    #[serde(default)]
    pub repository: Option<String>,

    /// 入口文件（应用必填；库可省略）
    #[serde(default = "default_entry")]
    pub entry: String,

    /// 导出符号列表
    #[serde(default)]
    pub exports: Vec<String>,

    /// 是否为库包
    #[serde(default)]
    pub library: bool,

    /// 包类型: bytecode | hybrid | native
    #[serde(default = "default_kind")]
    pub kind: String,

    /// 编译模式: vm | jit | aot（根目录仅能配置一种）
    #[serde(default = "default_mode")]
    pub mode: CompileMode,

    /// 最低编译器版本
    #[serde(default, rename = "compiler-min-version")]
    pub compiler_min_version: Option<String>,

    /// 最高编译器版本
    #[serde(default, rename = "compiler-max-version")]
    pub compiler_max_version: Option<String>,

    // ── 依赖（B1.3: 5 种配置分组） ──
    /// 编译 + 运行期依赖（默认）
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// 仅编译期依赖（类 compileOnly）
    #[serde(default, rename = "compile-dependencies")]
    pub compile_dependencies: Vec<Dependency>,
    /// 仅运行期依赖（类 runtimeOnly）
    #[serde(default, rename = "runtime-dependencies")]
    pub runtime_dependencies: Vec<Dependency>,
    /// 测试 + 基准依赖（类 testImplementation）
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: Vec<Dependency>,
    /// 构建脚本依赖（类 buildscript）
    #[serde(default, rename = "build-dependencies")]
    pub build_dependencies: Vec<Dependency>,

    // ── 构建配置（B1.2: 源码集 + 编译选项） ──
    /// 构建配置
    #[serde(default)]
    pub build: BuildConfig,

    // ── 插件配置（B1.1: [plugins] 表） ──
    /// 插件配置
    #[serde(default)]
    pub plugins: PluginConfig,

    // ── Profile（B1.1: [profiles] 表） ──
    /// 构建配置切换（类 Maven profiles）
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,

    // ── 仓库（B1.1: [repositories] 表） ──
    /// 仓库声明
    #[serde(default)]
    pub repositories: RepositoryConfig,

    // ── Workspace（B1.1: [workspace] 表） ──
    /// 多项目配置
    #[serde(default)]
    pub workspace: Option<WorkspaceConfig>,

    // ── 资源（已有，保留） ──
    /// 资源配置
    #[serde(default)]
    pub resources: ResourceConfig,

    // ── 制品选项（已有，保留） ──
    /// 制品选项
    #[serde(default)]
    pub package: PackageOptions,

    // ── 自定义任务（B1.1: [[tasks]] 表） ──
    /// 自定义任务定义
    #[serde(default)]
    pub tasks: Vec<TaskDef>,
}

impl LoomManifest {
    /// 判断是否为库包
    pub fn is_library(&self) -> bool {
        self.library || self.entry.is_empty()
    }

    /// 获取所有依赖（合并 5 种配置，config 字段自动设置）
    pub fn all_dependencies(&self) -> Vec<Dependency> {
        let mut deps = Vec::new();
        for d in &self.dependencies {
            deps.push(Dependency {
                config: DepConfig::Implementation,
                ..d.clone()
            });
        }
        for d in &self.compile_dependencies {
            deps.push(Dependency {
                config: DepConfig::CompileOnly,
                ..d.clone()
            });
        }
        for d in &self.runtime_dependencies {
            deps.push(Dependency {
                config: DepConfig::RuntimeOnly,
                ..d.clone()
            });
        }
        for d in &self.dev_dependencies {
            deps.push(Dependency {
                config: DepConfig::Test,
                ..d.clone()
            });
        }
        for d in &self.build_dependencies {
            deps.push(Dependency {
                config: DepConfig::Build,
                ..d.clone()
            });
        }
        deps
    }
}

impl Default for LoomManifest {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            name: "unnamed".to_string(),
            version: "0.1.0".to_string(),
            description: None,
            authors: Vec::new(),
            license: None,
            repository: None,
            entry: default_entry(),
            exports: Vec::new(),
            library: false,
            kind: default_kind(),
            mode: CompileMode::default(),
            compiler_min_version: None,
            compiler_max_version: None,
            dependencies: Vec::new(),
            compile_dependencies: Vec::new(),
            runtime_dependencies: Vec::new(),
            dev_dependencies: Vec::new(),
            build_dependencies: Vec::new(),
            build: BuildConfig::default(),
            plugins: PluginConfig::default(),
            profiles: HashMap::new(),
            repositories: RepositoryConfig::default(),
            workspace: None,
            resources: ResourceConfig::default(),
            package: PackageOptions::default(),
            tasks: Vec::new(),
        }
    }
}

fn default_schema_version() -> String {
    "2.0".to_string()
}

fn default_entry() -> String {
    "src/main.aura".to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 依赖声明
// ═══════════════════════════════════════════════════════════════════════════════

/// 依赖声明
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Dependency {
    /// 包名
    pub name: String,
    /// 版本约束（如 "^1.0"、">=2.0"）
    pub version: String,
    /// 依赖配置（implementation / compileOnly / runtimeOnly / test / build）
    #[serde(default = "default_dep_config")]
    pub config: DepConfig,
    /// 是否启用特性
    #[serde(default)]
    pub features: Vec<String>,
    /// 目标平台限制（triple 列表）
    #[serde(default)]
    pub target: Option<String>,
}

impl Dependency {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            config: DepConfig::default(),
            features: Vec::new(),
            target: None,
        }
    }
}

fn default_dep_config() -> DepConfig {
    DepConfig::Implementation
}

/// 依赖配置类型
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum DepConfig {
    /// 编译 + 运行期（默认）
    #[default]
    #[serde(rename = "implementation")]
    Implementation,
    /// 仅编译期
    #[serde(rename = "compile-only")]
    CompileOnly,
    /// 仅运行期
    #[serde(rename = "runtime-only")]
    RuntimeOnly,
    /// 测试 + 基准
    #[serde(rename = "test")]
    Test,
    /// 构建脚本
    #[serde(rename = "build")]
    Build,
}

impl std::fmt::Display for DepConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DepConfig::Implementation => write!(f, "implementation"),
            DepConfig::CompileOnly => write!(f, "compile-only"),
            DepConfig::RuntimeOnly => write!(f, "runtime-only"),
            DepConfig::Test => write!(f, "test"),
            DepConfig::Build => write!(f, "build"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 构建配置
// ═══════════════════════════════════════════════════════════════════════════════

/// 构建配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BuildConfig {
    /// 源码集
    #[serde(default)]
    pub source_sets: HashMap<String, SourceSetConfig>,
    /// 优化级别（0-3）
    #[serde(default = "default_opt_level")]
    pub opt_level: u8,
    /// 调试信息
    #[serde(default = "default_debug")]
    pub debug: bool,
    /// 目标三元组
    #[serde(default)]
    pub target: Option<String>,
    /// 输出目录
    #[serde(default = "default_out_dir")]
    pub out_dir: String,
    /// 缓存目录
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    /// 远程缓存 URL
    #[serde(default)]
    pub cache_remote: Option<String>,
    /// 是否启用远程缓存
    #[serde(default)]
    pub cache_remote_enabled: bool,
    /// 远程缓存是否共享
    #[serde(default)]
    pub cache_remote_shared: bool,
    /// 是否生成类型签名
    #[serde(default = "default_emit_signatures")]
    pub emit_signatures: bool,
    /// 是否打包为 .apkg
    #[serde(default)]
    pub emit_package: bool,
    /// 别名映射（类似 tsconfig paths）
    #[serde(default)]
    pub alias: HashMap<String, String>,
    /// 是否启用并行构建
    #[serde(default = "default_parallel")]
    pub parallel: bool,
    /// 并行任务数（0 = 自动，CPU 核心数）
    #[serde(default)]
    pub parallel_jobs: u32,
    /// FFI 模式（库导出方式）：cabi（C ABI）或 aot（AOT Aura 直连）
    #[serde(default)]
    pub ffi_mode: FfiMode,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            source_sets: HashMap::new(),
            opt_level: default_opt_level(),
            debug: default_debug(),
            target: None,
            out_dir: default_out_dir(),
            cache_dir: default_cache_dir(),
            cache_remote: None,
            cache_remote_enabled: false,
            cache_remote_shared: false,
            emit_signatures: default_emit_signatures(),
            emit_package: false,
            alias: HashMap::new(),
            parallel: default_parallel(),
            parallel_jobs: 0,
            ffi_mode: FfiMode::default(),
        }
    }
}

fn default_opt_level() -> u8 {
    2
}

fn default_kind() -> String {
    "bytecode".to_string()
}

fn default_mode() -> CompileMode {
    CompileMode::Vm
}

/// 编译模式（根目录仅能配置一种）
///
/// - `vm`: 字节码 + VM 解释执行
/// - `jit`: 字节码 + JIT 编译执行
/// - `aot`: AOT 编译为原生可执行文件
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompileMode {
    /// VM 解释执行（默认）
    #[default]
    #[serde(rename = "vm")]
    Vm,
    /// JIT 编译执行
    #[serde(rename = "jit")]
    Jit,
    /// AOT 编译为原生可执行文件
    #[serde(rename = "aot")]
    Aot,
}

impl std::fmt::Display for CompileMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileMode::Vm => write!(f, "vm"),
            CompileMode::Jit => write!(f, "jit"),
            CompileMode::Aot => write!(f, "aot"),
        }
    }
}

/// FFI 模式（库导出方式）
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FfiMode {
    /// C ABI 导出（供外部 C/Rust 调用）
    #[serde(rename = "cabi")]
    Cabi,
    /// AOT Aura 直连（JitValue ABI，供 extern interface 调用）
    #[default]
    #[serde(rename = "aot")]
    Aot,
}

impl std::fmt::Display for FfiMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfiMode::Cabi => write!(f, "cabi"),
            FfiMode::Aot => write!(f, "aot"),
        }
    }
}

fn default_debug() -> bool {
    true
}

fn default_out_dir() -> String {
    "target/build".to_string()
}

fn default_cache_dir() -> String {
    "target/cache".to_string()
}

fn default_emit_signatures() -> bool {
    true
}

fn default_parallel() -> bool {
    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// 源码集（B1.2）
// ═══════════════════════════════════════════════════════════════════════════════

/// 源码集配置
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SourceSetConfig {
    /// 源码目录列表
    #[serde(default)]
    pub source_dirs: Vec<String>,
    /// 资源目录列表
    #[serde(default)]
    pub resource_dirs: Vec<String>,
    /// 包含模式（默认 ["**/*.aura"]）
    #[serde(default)]
    pub include: Vec<String>,
    /// 排除模式
    #[serde(default)]
    pub exclude: Vec<String>,
    /// 依赖的其他源码集（如 test 依赖 main）
    #[serde(default)]
    pub depends_on: Vec<String>,
}

impl SourceSetConfig {
    /// 创建 main 源码集默认配置
    pub fn main_default() -> Self {
        Self {
            source_dirs: vec!["src".to_string()],
            resource_dirs: vec!["resources".to_string()],
            include: vec!["**/*.aura".to_string()],
            exclude: vec![
                "**/*.test.aura".to_string(),
                "vendor/**".to_string(),
                "target/**".to_string(),
            ],
            depends_on: Vec::new(),
        }
    }

    /// 创建 test 源码集默认配置
    pub fn test_default() -> Self {
        Self {
            source_dirs: vec!["test".to_string()],
            resource_dirs: vec!["test/resources".to_string()],
            include: vec!["**/*.aura".to_string()],
            exclude: Vec::new(),
            depends_on: vec!["main".to_string()],
        }
    }

    /// 创建 bench 源码集默认配置
    pub fn bench_default() -> Self {
        Self {
            source_dirs: vec!["bench".to_string()],
            resource_dirs: vec!["bench/resources".to_string()],
            include: vec!["**/*.aura".to_string()],
            exclude: Vec::new(),
            depends_on: vec!["main".to_string()],
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 插件配置（B1.1: [plugins] 表）
// ═══════════════════════════════════════════════════════════════════════════════

/// 插件配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PluginConfig {
    /// 标准库插件（自动注册 std 模块）
    #[serde(default = "default_true")]
    pub aura_stdlib: bool,
    /// 测试框架插件
    #[serde(default = "default_true")]
    pub aura_test_harness: bool,
    /// 文档生成插件
    #[serde(default)]
    pub aura_doc_gen: bool,
    /// 格式化插件
    #[serde(default)]
    pub aura_format: bool,
    /// AOT 编译插件
    #[serde(default)]
    pub aura_aot: bool,
    /// Watch 模式插件（自动激活）
    #[serde(default = "default_true")]
    pub aura_watch: bool,
    /// CI 插件
    #[serde(default)]
    pub aura_ci: bool,
    /// 外部插件映射
    #[serde(default)]
    pub external: HashMap<String, ExternalPluginConfig>,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            aura_stdlib: true,
            aura_test_harness: true,
            aura_doc_gen: false,
            aura_format: false,
            aura_aot: false,
            aura_watch: true,
            aura_ci: false,
            external: HashMap::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// 外部插件配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ExternalPluginConfig {
    /// 插件路径
    pub path: String,
    /// 插件版本约束
    #[serde(default)]
    pub version: Option<String>,
    /// 插件配置（任意 key-value）
    #[serde(default)]
    pub config: serde_json::Value,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Profile（B1.1: [profiles] 表）
// ═══════════════════════════════════════════════════════════════════════════════

/// Profile 配置（类 Maven profiles）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProfileConfig {
    /// 是否默认激活
    #[serde(default)]
    pub activate: bool,
    /// 构建配置覆盖
    #[serde(default)]
    pub build: Option<BuildConfigOverride>,
}

/// 构建配置覆盖（Profile 专用，所有字段可选）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BuildConfigOverride {
    /// 优化级别
    #[serde(default)]
    pub opt_level: Option<u8>,
    /// 调试信息
    #[serde(default)]
    pub debug: Option<bool>,
    /// 目标三元组
    #[serde(default)]
    pub target: Option<String>,
    /// 是否打包为 .apkg
    #[serde(default)]
    pub emit_package: Option<bool>,
    /// 并行构建
    #[serde(default)]
    pub parallel: Option<bool>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 仓库配置（B1.1: [repositories] 表）
// ═══════════════════════════════════════════════════════════════════════════════

/// 仓库配置
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryConfig {
    /// 中央仓库 URL
    #[serde(default)]
    pub central: Option<String>,
    /// 发布仓库配置
    #[serde(default)]
    pub publish: Option<PublishConfig>,
    /// 自定义仓库
    #[serde(default)]
    pub custom: HashMap<String, RepositoryEntry>,
}

/// 仓库条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RepositoryEntry {
    /// 仓库 URL
    pub url: String,
}

/// 发布仓库配置
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PublishConfig {
    /// 目标仓库名（"central" 或自定义仓库名）
    pub registry: String,
    /// 认证 token（支持环境变量 ${AURA_REGISTRY_TOKEN}）
    #[serde(default)]
    pub token: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Workspace（B1.1: [workspace] 表）
// ═══════════════════════════════════════════════════════════════════════════════

/// Workspace 配置（多项目，类 Cargo workspace）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WorkspaceConfig {
    /// 成员列表（相对路径）
    #[serde(default)]
    pub members: Vec<String>,
    /// 默认构建成员
    #[serde(default)]
    pub default_members: Vec<String>,
    /// 依赖解析策略（"1" = 默认, "2" = 新版本）
    #[serde(default = "default_resolver")]
    pub resolver: String,
    /// Workspace 级别的依赖管理（BOM）
    #[serde(default, rename = "dependency-management")]
    pub dependency_management: HashMap<String, String>,
    /// Workspace 级别的编译选项
    #[serde(default)]
    pub build: Option<BuildConfig>,
}

fn default_resolver() -> String {
    "2".to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 资源配置（已有，保留）
// ═══════════════════════════════════════════════════════════════════════════════

/// 资源配置
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ResourceConfig {
    /// 包含模式
    #[serde(default)]
    pub include: Vec<String>,
    /// 排除模式
    #[serde(default)]
    pub exclude: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 制品选项（已有，保留）
// ═══════════════════════════════════════════════════════════════════════════════

/// 制品选项
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PackageOptions {
    /// 容器格式："auz"（tar+zstd）| "source"（纯源码）
    #[serde(default = "default_pkg_format")]
    pub format: String,
    /// 是否打包源码附件
    #[serde(default, rename = "include-sources")]
    pub include_sources: bool,
    /// 是否打包文档
    #[serde(default, rename = "include-docs")]
    pub include_docs: bool,
    /// 是否打包 AOT 原生库
    #[serde(default, rename = "include-native")]
    pub include_native: bool,
    /// AOT 目标三元组列表
    #[serde(default, rename = "native-targets")]
    pub native_targets: Vec<String>,
    /// AOT 优化级别
    #[serde(default = "default_aot_opt_level", rename = "aot-opt-level")]
    pub aot_opt_level: u32,
}

impl Default for PackageOptions {
    fn default() -> Self {
        Self {
            format: "auz".to_string(),
            include_sources: false,
            include_docs: false,
            include_native: true,
            native_targets: Vec::new(),
            aot_opt_level: 2,
        }
    }
}

fn default_pkg_format() -> String {
    "auz".to_string()
}

fn default_aot_opt_level() -> u32 {
    2
}

// ═══════════════════════════════════════════════════════════════════════════════
// 自定义任务（B1.1: [[tasks]] 表）
// ═══════════════════════════════════════════════════════════════════════════════

/// 自定义任务定义（类 Gradle tasks 块）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TaskDef {
    /// 任务名称
    pub name: String,
    /// 任务描述
    #[serde(default)]
    pub description: String,
    /// 依赖的任务列表
    #[serde(default, rename = "depends-on")]
    pub depends_on: Vec<String>,
    /// 要执行的命令（可选）
    #[serde(default)]
    pub command: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 源码集运行时结构（B1.2）
// ═══════════════════════════════════════════════════════════════════════════════

/// 源码集运行时实例（解析后 + 目录扫描后）
#[derive(Debug, Clone)]
pub struct SourceSet {
    /// 源码集名称（main / test / bench）
    pub name: String,
    /// 源码目录列表（绝对路径）
    pub source_dirs: Vec<std::path::PathBuf>,
    /// 资源目录列表（绝对路径）
    pub resource_dirs: Vec<std::path::PathBuf>,
    /// 包含模式
    pub include: Vec<String>,
    /// 排除模式
    pub exclude: Vec<String>,
    /// 依赖的其他源码集名称
    pub depends_on: Vec<String>,
    /// 发现的模块列表（编译时填充）
    pub modules: Vec<ModuleInfo>,
}

impl SourceSet {
    /// 从配置创建源码集（解析相对路径为绝对路径）
    pub fn from_config(
        config: &SourceSetConfig,
        name: &str,
        project_dir: &std::path::Path,
    ) -> Self {
        let source_dirs = config.source_dirs.iter().map(|d| project_dir.join(d)).collect();
        let resource_dirs = config.resource_dirs.iter().map(|d| project_dir.join(d)).collect();

        Self {
            name: name.to_string(),
            source_dirs,
            resource_dirs,
            include: config.include.clone(),
            exclude: config.exclude.clone(),
            depends_on: config.depends_on.clone(),
            modules: Vec::new(),
        }
    }

    /// 获取主源码集（默认 src/）
    pub fn main(project_dir: &std::path::Path) -> Self {
        Self::from_config(&SourceSetConfig::main_default(), "main", project_dir)
    }

    /// 获取测试源码集（默认 test/）
    pub fn test(project_dir: &std::path::Path) -> Self {
        Self::from_config(&SourceSetConfig::test_default(), "test", project_dir)
    }

    /// 获取基准源码集（默认 bench/）
    pub fn bench(project_dir: &std::path::Path) -> Self {
        Self::from_config(&SourceSetConfig::bench_default(), "bench", project_dir)
    }
}

/// 模块信息
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    /// 模块名（如 "math.vector"）
    pub name: String,
    /// 源文件路径
    pub path: std::path::PathBuf,
    /// 所属源码集
    pub source_set: String,
    /// 是否为入口
    pub is_entry: bool,
    /// 导入的模块列表
    pub imports: Vec<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // B1.1: Manifest 数据结构测试

    #[test]
    fn test_manifest_minimal() {
        let toml_str = r#"
name = "my-app"
version = "1.0.0"
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "my-app");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.schema_version, "2.0");
        assert_eq!(manifest.entry, "src/main.aura");
        assert!(!manifest.library);
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.compile_dependencies.is_empty());
        assert!(manifest.runtime_dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
        assert!(manifest.build_dependencies.is_empty());
        assert!(manifest.profiles.is_empty());
        assert!(manifest.tasks.is_empty());
        assert!(manifest.workspace.is_none());
    }

    #[test]
    fn test_manifest_full() {
        let toml_str = r#"
schema-version = "2.0"
name = "my-app"
version = "1.0.0"
description = "示例项目"
authors = ["Alice <alice@example.com>"]
license = "MIT"
repository = "https://github.com/user/my-app"
entry = "src/main.aura"
exports = ["main", "utils"]
library = false

[[dependencies]]
name = "aura-json"
version = "^1.0"

[[dependencies]]
name = "aura-http"
version = ">=2.0"

[[compile-dependencies]]
name = "aura-test-macros"
version = "^0.5"

[[runtime-dependencies]]
name = "aura-logger"
version = "^2.0"

[[dev-dependencies]]
name = "aura-test"
version = "^1.0"

[[dev-dependencies]]
name = "aura-bench"
version = "^0.3"

[[build-dependencies]]
name = "aura-build-macro"
version = "^0.1"

[build]
opt-level = 2
debug = true
out-dir = "target/build"

[build.source-sets.main]
source-dirs = ["src"]
resource-dirs = ["resources"]
include = ["**/*.aura"]
exclude = ["**/*.test.aura"]

[build.source-sets.test]
source-dirs = ["test"]
depends-on = ["main"]

[plugins]
aura-stdlib = true
aura-test-harness = true
aura-doc-gen = false

[profiles.debug]
activate = true
[profiles.debug.build]
opt-level = 0
debug = true

[profiles.release]
[profiles.release.build]
opt-level = 3
debug = false
emit-package = true

[repositories]
central = "https://registry.aura-lang.dev"

[repositories.publish]
registry = "central"

[workspace]
members = ["libs/core", "apps/server"]
default-members = ["."]
resolver = "2"

[workspace.dependency-management]
aura-json = "^1.2"

[[tasks]]
name = "deploy"
description = "部署到测试环境"
depends-on = ["package", "verify"]
command = "aura publish --target test-env"
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();

        // 包元数据
        assert_eq!(manifest.name, "my-app");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description.as_deref(), Some("示例项目"));
        assert_eq!(manifest.authors.len(), 1);
        assert_eq!(manifest.entry, "src/main.aura");
        assert_eq!(manifest.exports.len(), 2);
        assert!(!manifest.library);

        // 依赖（5 种配置分组）
        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(manifest.compile_dependencies.len(), 1);
        assert_eq!(manifest.runtime_dependencies.len(), 1);
        assert_eq!(manifest.dev_dependencies.len(), 2);
        assert_eq!(manifest.build_dependencies.len(), 1);
        assert_eq!(manifest.all_dependencies().len(), 7);

        // 构建配置
        assert_eq!(manifest.build.opt_level, 2);
        assert!(manifest.build.debug);
        assert_eq!(manifest.build.out_dir, "target/build");
        assert!(manifest.build.source_sets.contains_key("main"));
        assert!(manifest.build.source_sets.contains_key("test"));

        // 插件
        assert!(manifest.plugins.aura_stdlib);
        assert!(manifest.plugins.aura_test_harness);
        assert!(!manifest.plugins.aura_doc_gen);

        // Profile
        assert!(manifest.profiles.contains_key("debug"));
        assert!(manifest.profiles.contains_key("release"));
        assert!(manifest.profiles["debug"].activate);
        assert!(!manifest.profiles["release"].activate);

        // 仓库
        assert_eq!(
            manifest.repositories.central.as_deref(),
            Some("https://registry.aura-lang.dev")
        );
        assert!(manifest.repositories.publish.is_some());

        // Workspace
        assert!(manifest.workspace.is_some());
        let ws = manifest.workspace.as_ref().unwrap();
        assert_eq!(ws.members.len(), 2);
        assert_eq!(ws.default_members.len(), 1);
        assert_eq!(ws.resolver, "2");
        assert!(ws.dependency_management.contains_key("aura-json"));

        // 自定义任务
        assert_eq!(manifest.tasks.len(), 1);
        assert_eq!(manifest.tasks[0].name, "deploy");
        assert_eq!(manifest.tasks[0].depends_on.len(), 2);
        assert!(manifest.tasks[0].command.is_some());
    }

    // B1.2: SourceSet 测试

    #[test]
    fn test_source_set_config_main_default() {
        let config = SourceSetConfig::main_default();
        assert_eq!(config.source_dirs, vec!["src".to_string()]);
        assert_eq!(config.resource_dirs, vec!["resources".to_string()]);
        assert!(config.include.contains(&"**/*.aura".to_string()));
        assert!(config.exclude.contains(&"**/*.test.aura".to_string()));
        assert!(config.depends_on.is_empty());
    }

    #[test]
    fn test_source_set_config_test_default() {
        let config = SourceSetConfig::test_default();
        assert_eq!(config.source_dirs, vec!["test".to_string()]);
        assert!(config.depends_on.contains(&"main".to_string()));
    }

    #[test]
    fn test_source_set_config_bench_default() {
        let config = SourceSetConfig::bench_default();
        assert_eq!(config.source_dirs, vec!["bench".to_string()]);
        assert!(config.depends_on.contains(&"main".to_string()));
    }

    // B1.3: 依赖配置分组测试

    #[test]
    fn test_dependency_new() {
        let dep = Dependency::new("aura-json", "^1.0");
        assert_eq!(dep.name, "aura-json");
        assert_eq!(dep.version, "^1.0");
        assert_eq!(dep.config, DepConfig::Implementation);
        assert!(dep.features.is_empty());
        assert!(dep.target.is_none());
    }

    #[test]
    fn test_dep_config_display() {
        assert_eq!(DepConfig::Implementation.to_string(), "implementation");
        assert_eq!(DepConfig::CompileOnly.to_string(), "compile-only");
        assert_eq!(DepConfig::RuntimeOnly.to_string(), "runtime-only");
        assert_eq!(DepConfig::Test.to_string(), "test");
        assert_eq!(DepConfig::Build.to_string(), "build");
    }

    #[test]
    fn test_dep_config_serde() {
        let dep: Dependency = toml::from_str(
            r#"
name = "test-dep"
version = "^1.0"
config = "compile-only"
"#,
        )
        .unwrap();
        assert_eq!(dep.config, DepConfig::CompileOnly);
    }

    #[test]
    fn test_all_dependencies_merge() {
        let manifest: LoomManifest = toml::from_str(
            r#"
name = "test"
version = "1.0.0"

[[dependencies]]
name = "dep1"
version = "^1.0"

[[compile-dependencies]]
name = "dep2"
version = "^1.0"

[[runtime-dependencies]]
name = "dep3"
version = "^1.0"

[[dev-dependencies]]
name = "dep4"
version = "^1.0"

[[build-dependencies]]
name = "dep5"
version = "^1.0"
"#,
        )
        .unwrap();

        let all = manifest.all_dependencies();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].name, "dep1");
        assert_eq!(all[0].config, DepConfig::Implementation);
        assert_eq!(all[1].name, "dep2");
        assert_eq!(all[1].config, DepConfig::CompileOnly);
        assert_eq!(all[2].name, "dep3");
        assert_eq!(all[2].config, DepConfig::RuntimeOnly);
        assert_eq!(all[3].name, "dep4");
        assert_eq!(all[3].config, DepConfig::Test);
        assert_eq!(all[4].name, "dep5");
        assert_eq!(all[4].config, DepConfig::Build);
    }

    // B1.1: Workspace 测试

    #[test]
    fn test_workspace_config() {
        let toml_str = r#"
name = "ws-root"
version = "1.0.0"

[workspace]
members = ["libs/core", "libs/utils", "apps/server"]
default-members = ["."]
resolver = "2"

[workspace.dependency-management]
aura-json = "^1.2"
aura-http = ">=2.0"
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();
        let ws = manifest.workspace.unwrap();
        assert_eq!(ws.members.len(), 3);
        assert_eq!(ws.default_members.len(), 1);
        assert_eq!(ws.resolver, "2");
        assert_eq!(ws.dependency_management.len(), 2);
        assert_eq!(ws.dependency_management["aura-json"], "^1.2");
    }

    // B1.1: 自定义任务测试

    #[test]
    fn test_custom_tasks() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[[tasks]]
name = "deploy"
description = "部署到测试环境"
depends-on = ["package", "verify"]
command = "aura publish --target test-env"

[[tasks]]
name = "clean-release"
description = "清理并重新发布"
depends-on = ["clean", "package", "publish"]
"#;
        let manifest: LoomManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.tasks.len(), 2);
        assert_eq!(manifest.tasks[0].name, "deploy");
        assert_eq!(
            manifest.tasks[0].depends_on,
            vec![
                "package", "verify"
            ]
        );
        assert!(manifest.tasks[0].command.is_some());
        assert_eq!(manifest.tasks[1].name, "clean-release");
        assert!(manifest.tasks[1].command.is_none());
    }
}
