//! P9 — 标准库模块
//!
//! 对应 开发规划与实现进度.md P9 阶段。
//! 每个子模块将一组函数注册到 `NativeRegistry`，
//! 函数签名统一为 `fn(&[Value]) -> Value`（见 vm::native::NativeFn）。
//!
//! 子模块清单：
//! - std_io       — 标准输入输出（println / readLine / File）
//! - std_math     — 数学函数与常量（sin / cos / PI / ...）
//! - std_string   — 字符串操作与格式化（contains / split / format / ...）
//! - std_collections — 集合辅助（listOf / mutableListOf / mapOf / setOf / ...）
//! - std_fs       — 文件系统操作（exists / read / write / delete / list）
//! - std_net      — 网络 Socket（TCP/UDP 基础操作）
//! - std_json     — JSON 解析与序列化（parse / stringify）
//! - std_time     — 时间/日期（now / epoch / format）
//! - std_test     — 测试断言（assert / assertTrue / assertEq / ...）
//! - std_builtin  — 编译期内置（typeOf / typeof / assert / ...）
//! - std_env      — 环境变量（get / set / remove）
//! - std_process  — 进程管理（exit / spawn / args）
//! - std_random   — 随机数（nextInt / nextFloat / shuffle / ...）
//! - std_encoding — 编码/解码（base64 / hex / ...）
//! - std_ascii    — 字符工具（isAlpha / isDigit / isWhitespace / ...）
//! - std_console  — 终端控制（clear / color / cursor）
//! - std_path     — 路径操作（join / dirname / basename / ext）
//! - std_assert   — 通用断言（assert / debugAssert）
//! - std_iter     — 迭代器/函数式工具（map / filter / reduce / ...）

pub mod decl;
pub mod source_index;

// Phase 3: 嵌入式标准库（预编译 AOT .auc 嵌入二进制）
pub mod embedded_stdlib;

// Phase 3: std 模块按需编译（#[cfg(feature)] 门控）
// 默认不编译任何 std 模块，减小二进制体积。
// 启用方式：cargo build --features "std-math,std-io"

#[cfg(feature = "std-ascii")]
pub mod std_ascii;
#[cfg(feature = "std-assert")]
pub mod std_assert;
#[cfg(feature = "std-builtin")]
pub mod std_builtin;
#[cfg(feature = "std-collections")]
pub mod std_collections;
#[cfg(feature = "std-console")]
pub mod std_console;
#[cfg(feature = "std-encoding")]
pub mod std_encoding;
#[cfg(feature = "std-env")]
pub mod std_env;
#[cfg(feature = "std-fs")]
pub mod std_fs;
#[cfg(feature = "std-io")]
pub mod std_io;
#[cfg(feature = "std-iter")]
pub mod std_iter;
#[cfg(feature = "std-json")]
pub mod std_json;
#[cfg(feature = "std-math")]
pub mod std_math;
#[cfg(feature = "std-net")]
pub mod std_net;
#[cfg(feature = "std-path")]
pub mod std_path;
#[cfg(feature = "std-process")]
pub mod std_process;
#[cfg(feature = "std-random")]
pub mod std_random;
#[cfg(feature = "std-string")]
pub mod std_string;
#[cfg(feature = "std-test")]
pub mod std_test;
#[cfg(feature = "std-time")]
pub mod std_time;

use crate::vm::native::NativeRegistry;

/// 将所有标准库函数注册到 `NativeRegistry`
///
/// 仅注册已启用的模块（由 Cargo feature 控制）。
pub fn register_all(reg: &mut NativeRegistry) {
    #[cfg(feature = "std-io")]
    std_io::register(reg);
    #[cfg(feature = "std-math")]
    std_math::register(reg);
    #[cfg(feature = "std-string")]
    std_string::register(reg);
    #[cfg(feature = "std-collections")]
    std_collections::register(reg);
    #[cfg(feature = "std-fs")]
    std_fs::register(reg);
    #[cfg(feature = "std-net")]
    std_net::register(reg);
    #[cfg(feature = "std-json")]
    std_json::register(reg);
    #[cfg(feature = "std-time")]
    std_time::register(reg);
    #[cfg(feature = "std-test")]
    std_test::register(reg);
    #[cfg(feature = "std-builtin")]
    std_builtin::register(reg);
    #[cfg(feature = "std-env")]
    std_env::register(reg);
    #[cfg(feature = "std-process")]
    std_process::register(reg);
    #[cfg(feature = "std-random")]
    std_random::register(reg);
    #[cfg(feature = "std-encoding")]
    std_encoding::register(reg);
    #[cfg(feature = "std-ascii")]
    std_ascii::register(reg);
    #[cfg(feature = "std-console")]
    std_console::register(reg);
    #[cfg(feature = "std-path")]
    std_path::register(reg);
    #[cfg(feature = "std-assert")]
    std_assert::register(reg);
    #[cfg(feature = "std-iter")]
    std_iter::register(reg);
}

/// 按需注册标准库函数（只注册指定模块）
///
/// `modules` 是模块名集合，如 `["math", "io", "string"]`。
/// 未指定的模块不注册，对应代码不编译进二进制。
pub fn register_with_modules(reg: &mut NativeRegistry, modules: &[&str]) {
    for module in modules {
        match *module {
            #[cfg(feature = "std-io")]
            "io" => std_io::register(reg),
            #[cfg(feature = "std-math")]
            "math" => std_math::register(reg),
            #[cfg(feature = "std-string")]
            "string" => std_string::register(reg),
            #[cfg(feature = "std-collections")]
            "collections" => std_collections::register(reg),
            #[cfg(feature = "std-fs")]
            "fs" => std_fs::register(reg),
            #[cfg(feature = "std-net")]
            "net" => std_net::register(reg),
            #[cfg(feature = "std-json")]
            "json" => std_json::register(reg),
            #[cfg(feature = "std-time")]
            "time" => std_time::register(reg),
            #[cfg(feature = "std-test")]
            "test" => std_test::register(reg),
            #[cfg(feature = "std-builtin")]
            "builtin" => std_builtin::register(reg),
            #[cfg(feature = "std-env")]
            "env" => std_env::register(reg),
            #[cfg(feature = "std-process")]
            "process" => std_process::register(reg),
            #[cfg(feature = "std-random")]
            "random" => std_random::register(reg),
            #[cfg(feature = "std-encoding")]
            "encoding" => std_encoding::register(reg),
            #[cfg(feature = "std-ascii")]
            "ascii" => std_ascii::register(reg),
            #[cfg(feature = "std-console")]
            "console" => std_console::register(reg),
            #[cfg(feature = "std-path")]
            "path" => std_path::register(reg),
            #[cfg(feature = "std-assert")]
            "assert" => std_assert::register(reg),
            #[cfg(feature = "std-iter")]
            "iter" => std_iter::register(reg),
            _ => {} // 未知模块或未启用，跳过
        }
    }
}

/// 从 import 声明路径提取模块名
///
/// 例如：`"aura.lang.std.Math"` → `"math"`，`"aura.lang.std.IO"` → `"io"`
///
/// 兼容旧命名（`"aura.math"` → `"math"`）以支持渐进迁移。
pub fn module_name_from_path(path: &str) -> Option<&str> {
    // New scheme: aura.lang.std.<ClassName>
    if let Some(rest) = path.strip_prefix("aura.lang.std.") {
        // Convert PascalCase to lowercase module name
        return match rest {
            "Math" => Some("math"),
            "IO" => Some("io"),
            "Ascii" => Some("ascii"),
            "Assert" => Some("assert"),
            "Builtin" => Some("builtin"),
            "Collections" => Some("collections"),
            "Console" => Some("console"),
            "Encoding" => Some("encoding"),
            "Env" => Some("env"),
            "FileSystem" => Some("fs"),
            "Iter" => Some("iter"),
            "Json" => Some("json"),
            "Network" => Some("net"),
            "Path" => Some("path"),
            "Process" => Some("process"),
            "Random" => Some("random"),
            "String" => Some("string"),
            "Test" => Some("test"),
            "Time" => Some("time"),
            "Coroutine" => Some("concurrent"),
            "Actor" => Some("concurrent"),
            "Channel" => Some("concurrent"),
            _ => None,
        };
    }
    // Old scheme (kept for backwards compatibility during migration)
    if let Some(rest) = path.strip_prefix("aura.") { Some(rest) } else { None }
}
