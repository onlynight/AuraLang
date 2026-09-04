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

pub mod std_ascii;
pub mod std_assert;
pub mod std_builtin;
pub mod std_collections;
pub mod std_console;
pub mod std_encoding;
pub mod std_env;
pub mod std_fs;
pub mod std_io;
pub mod std_iter;
pub mod std_json;
pub mod std_math;
pub mod std_net;
pub mod std_path;
pub mod std_process;
pub mod std_random;
pub mod std_string;
pub mod std_test;
pub mod std_time;

use crate::vm::native::NativeRegistry;

/// 将所有标准库函数注册到 `NativeRegistry`
pub fn register_all(reg: &mut NativeRegistry) {
    std_io::register(reg);
    std_math::register(reg);
    std_string::register(reg);
    std_collections::register(reg);
    std_fs::register(reg);
    std_net::register(reg);
    std_json::register(reg);
    std_time::register(reg);
    std_test::register(reg);
    std_builtin::register(reg);
    std_env::register(reg);
    std_process::register(reg);
    std_random::register(reg);
    std_encoding::register(reg);
    std_ascii::register(reg);
    std_console::register(reg);
    std_path::register(reg);
    std_assert::register(reg);
    std_iter::register(reg);
}
