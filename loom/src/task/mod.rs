//! [Phase L2] 任务引擎：TaskGraph + 拓扑排序 + 并行调度

pub mod builtin;
pub mod compile_stdlib;
pub mod executor;
pub mod graph;
pub mod scheduler;

use std::collections::HashMap;

/// 构建任务定义
#[derive(Debug, Clone)]
pub struct TaskDefinition {
    /// 任务名称（如 "compile-main"）
    pub name: String,
    /// 任务描述
    pub description: String,
    /// 任务类型
    pub kind: TaskKind,
    /// 依赖任务（DAG 前置条件）
    pub depends_on: Vec<String>,
    /// 任务输入（用于增量检查）
    pub inputs: TaskInputs,
    /// 任务输出（产物）
    pub outputs: TaskOutputs,
}

/// 任务类型
#[derive(Debug, Clone)]
pub enum TaskKind {
    /// 清理产物
    Clean,
    /// 解析依赖
    Resolve,
    /// 编译源码集
    Compile(String),
    /// 运行测试
    Test,
    /// 打包制品
    Package,
    /// 验证制品
    Verify,
    /// 语法/语义检查
    Check,
    /// 安装到本地注册表
    Install,
    /// 发布到远程仓库
    Deploy,
    /// 运行应用
    Execute,
    /// 监听源码变化
    Watch,
    /// 插件任务
    Plugin(String),
}

/// 任务输入
#[derive(Debug, Clone, Default)]
pub struct TaskInputs {
    /// 源文件列表
    pub files: Vec<std::path::PathBuf>,
    /// 编译选项
    pub options: HashMap<String, String>,
    /// 依赖任务的 fingerprint 哈希
    pub dep_fingerprints: HashMap<String, String>,
}

/// 任务输出
#[derive(Debug, Clone, Default)]
pub struct TaskOutputs {
    /// 产物文件列表
    pub files: Vec<std::path::PathBuf>,
    /// 产物目录
    pub dir: std::path::PathBuf,
}

/// 任务图
#[derive(Debug, Default)]
pub struct TaskGraph {
    tasks: HashMap<String, TaskDefinition>,
}

impl TaskGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_task(&mut self, task: TaskDefinition) {
        self.tasks.insert(task.name.clone(), task);
    }

    pub fn get(&self, name: &str) -> Option<&TaskDefinition> {
        self.tasks.get(name)
    }

    pub fn all(&self) -> impl Iterator<Item = &TaskDefinition> {
        self.tasks.values()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tasks.contains_key(name)
    }
}
