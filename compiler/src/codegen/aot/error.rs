//! AOT 后端错误类型
//!
//! 对应 技术方案 §9.8 CodegenError 枚举。

use thiserror::Error;

/// AOT 代码生成错误
#[derive(Debug, Error)]
pub enum AotError {
    #[error("LLVM initialization failed: {0}")]
    InitializationFailed(String),

    #[error("invalid target triple: {0}")]
    InvalidTarget(String),

    #[error("target not found: {0}")]
    TargetNotFound(String),

    #[error("IR verification failed: {0}")]
    VerificationFailed(String),

    #[error("code generation failed: {0}")]
    CodeGenerationFailed(String),

    #[error("linker failed: {0}")]
    LinkerFailed(String),

    #[error("undefined function: {0}")]
    UndefinedFunction(String),

    #[error("type mismatch: {0}")]
    TypeMismatch(String),

    #[error("unsupported expression: {0}")]
    UnsupportedExpr(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("LLVM tool invocation failed: {0}")]
    ToolError(String),

    #[error("{0}")]
    Other(String),
}
