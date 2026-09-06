//! [Phase L6] 本地路径解析器
//!
//! 第四阶段职责边界收敛后，本模块仅负责将 aura.toml 中声明的依赖
//! 解析为本地路径（供构建系统使用），不再承担远端拉取或注册表交互。
//!
//! 生态层职责（安装、发布、版本冲突解析）由 `aura` 的包管理器承担，
//! 详见 `compiler/src/package.rs`。
//!
//! 本地包缓存目录约定：`~/.aura/cache/packages/`
//! - loom 的 `resolve` 命令仅**读取**该目录，不写入
//! - aura 的 `install` / `update` 命令负责**写入**该目录
//!
//! 后续计划：
//! - [ ] 实现从 `~/.aura/cache/packages/` 读取已安装包路径
//! - [ ] 与 aura 的 DependencyGraph 对接
