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

/// 嵌入的 Ascii 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_ASCII_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Ascii.auc"));

/// 嵌入的 Assert 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_ASSERT_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Assert.auc"));

/// 嵌入的 Encoding 标准库（AOT 机器码）— 纯逻辑（Base64 / Hex）
pub static EMBEDDED_ENCODING_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "Encoding.auc"));

/// 嵌入的 Iter 标准库（AOT 机器码）— 纯逻辑（函数式工具）
pub static EMBEDDED_ITER_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Iter.auc"));

/// 嵌入的 Json 标准库（AOT 机器码）— 纯逻辑（JSON parse/stringify）
pub static EMBEDDED_JSON_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Json.auc"));

/// 嵌入的 StringBuilder 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_SB_AUC: &[u8] = include_bytes!(concat!("../../../build/", "StringBuilder.auc"));

/// 嵌入的 TestHelper 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_TEST_HELPER_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "TestHelper.auc"));

/// 嵌入的 Path 标准库（AOT 机器码）— 纯逻辑（路径操作）
pub static EMBEDDED_PATH_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Path.auc"));

/// 嵌入的 String 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_STRING_AUC: &[u8] = include_bytes!(concat!("../../../build/", "String.auc"));

/// 嵌入的 Actor 标准库（AOT 机器码）— 纯逻辑（单线程 Actor）
pub static EMBEDDED_ACTOR_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Actor.auc"));

/// 嵌入的 Channel 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_CHANNEL_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Channel.auc"));

/// 嵌入的 Coroutine 标准库（AOT 机器码）— 纯逻辑
pub static EMBEDDED_COROUTINE_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "Coroutine.auc"));

/// 所有嵌入的标准库模块（模块名, .auc 字节）
/// 仅包含纯逻辑模块（FFI 模块保持 Rust native）
pub static EMBEDDED_STDLIB_MODULES: &[(&str, &[u8])] = &[
    ("Math", EMBEDDED_MATH_AUC),
    ("Time", EMBEDDED_TIME_AUC),
    ("Collections", EMBEDDED_COLLECTIONS_AUC),
    ("Test", EMBEDDED_TEST_AUC),
    ("Ascii", EMBEDDED_ASCII_AUC),
    ("Assert", EMBEDDED_ASSERT_AUC),
    ("Encoding", EMBEDDED_ENCODING_AUC),
    ("Iter", EMBEDDED_ITER_AUC),
    ("Json", EMBEDDED_JSON_AUC),
    ("StringBuilder", EMBEDDED_SB_AUC),
    ("TestHelper", EMBEDDED_TEST_HELPER_AUC),
    ("Path", EMBEDDED_PATH_AUC),
    ("String", EMBEDDED_STRING_AUC),
    ("Actor", EMBEDDED_ACTOR_AUC),
    ("Channel", EMBEDDED_CHANNEL_AUC),
    ("Coroutine", EMBEDDED_COROUTINE_AUC),
];

/// 标准库模块总数
pub const EMBEDDED_STDLIB_COUNT: usize = EMBEDDED_STDLIB_MODULES.len();
