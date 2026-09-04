//! AOT 后端错误类型
//!
//! 对应 技术方案 §9.8 CodegenError 枚举。

use thiserror::Error;

/// AOT 代码生成错误
#[derive(Debug, Error)]
pub enum AotError {
    #[error("LLVM 初始化失败: {0}")]
    InitializationFailed(String),

    #[error("无效的目标三元组: {0}")]
    InvalidTarget(String),

    #[error("目标未找到: {0}")]
    TargetNotFound(String),

    #[error("IR 验证失败: {0}")]
    VerificationFailed(String),

    #[error("代码生成失败: {0}")]
    CodeGenerationFailed(String),

    #[error("链接器失败: {0}")]
    LinkerFailed(String),

    #[error("未定义函数: {0}")]
    UndefinedFunction(String),

    #[error("类型不匹配: {0}")]
    TypeMismatch(String),

    #[error("不支持的表达式: {0}")]
    UnsupportedExpr(String),

    #[error("IO 错误: {0}")]
    Io(String),

    #[error("LLVM 工具调用失败: {0}")]
    ToolError(String),

    #[error("{0}")]
    Other(String),
}
