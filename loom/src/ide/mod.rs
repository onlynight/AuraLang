//! [Phase B7] IDE 集成
//!
//! 提供 .loom/aura-project.json 导出，支持 IDE 项目识别和导航。
//! 对应设计文档 §17 IDE 集成。

pub mod model;

pub use model::{
    FileScanner, IdeBuildConfig, IdeDependency, IdeFile, IdePlugin, IdeProject, IdeTask,
};
