//! Phase 3: 嵌入式标准库（预编译 AOT .auc 嵌入二进制）
//!
//! `aura/core/aura/lang/std/*.aura` 是唯一真相源，构建时预编译为 AOT 机器码并嵌入 .auc v4，
//! VM 启动时自动加载，优先执行 AOT 机器码（而非 Rust native 或字节码解释）。
//!
//! 编译管线：
//!   aura/core/aura/lang/std/*.aura → HIR → MIR → AOT (LLVM) → 机器码 → .auc v4 → include_bytes!
//!
//! 调用优先级：
//!   1. AOT 机器码（嵌入 .auc，直接执行）
//!   2. Aura 字节码（嵌入 .auc，VM 解释）
//!   3. Rust native（仅限 Layer 0-A 引导层 + FFI）
//!
//! 注意：仅包含纯逻辑模块（可在 Aura 中完整实现）。
//! FFI 模块（IO/FileSystem/Network/Random）保持 Rust native 实现。
//!
//! 目录结构（v3.1）：
//!   aura/core/aura/lang/std/          — 用户标准库（Layer 1+）
//!   aura/compiler/aura/lang/compiler/ — 编译器基础设施（Layer 0-B）
//!   build/                            — 预编译 .auc 文件（输出目录）

/// 嵌入的 Math 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_MATH_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Math.auc"));

/// 嵌入的 Time 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_TIME_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Time.auc"));

/// 嵌入的 Collections 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_COLLECTIONS_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "Collections.auc"));

/// 嵌入的 Test 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_TEST_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Test.auc"));

/// 所有嵌入的标准库模块（模块名, .auc 字节）
/// 仅包含纯逻辑模块（FFI 模块保持 Rust native）
pub static EMBEDDED_STDLIB_MODULES: &[(&str, &[u8])] = &[
    ("Math", EMBEDDED_MATH_AUC),
    ("Time", EMBEDDED_TIME_AUC),
    ("Collections", EMBEDDED_COLLECTIONS_AUC),
    ("Test", EMBEDDED_TEST_AUC),
];

/// 标准库模块总数
pub const EMBEDDED_STDLIB_COUNT: usize = EMBEDDED_STDLIB_MODULES.len();
