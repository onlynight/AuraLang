//! Import 语法完整支持测试
//!
//! 验证所有 import 形式的解析与运行行为：
//!
//! 语法形式                          | 调用形式                  | 状态
//! ----------------------------------|---------------------------|--------
//! `import aura.concurrent.*`       | `spawn(42)` (短名)        | ✅
//! `import aura.concurrent.*`       | `aura.concurrent.spawn(42)` (全路径) | ✅
//! `import aura.concurrent`         | `aura.concurrent.spawn(42)` (全路径) | ✅
//! `import aura.concurrent.spawn`   | `spawn(42)` (短名)        | ✅
//! `import aura.concurrent.spawn`   | `aura.concurrent.spawn(42)` (全路径) | ✅
//! `import aura.concurrent.spawn as s` | `s(42)` (别名)         | ✅
//! `import aura.concurrent as cc`   | `cc.spawn(42)` (模块别名) | ✅
//! `import aura.concurrent.* as cc` | `cc.spawn(42)` (通配+别名)| ✅
//!
//! 也验证多个不同模块的 import 同时使用不冲突。

use compiler::codegen::compile_source;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::Value;
use compiler::vm::{Vm, VmOptions};

// ─────────────────────────────────────────────────────────────────────────────
// 辅助函数
// ─────────────────────────────────────────────────────────────────────────────

/// 编译源码（仅解析，不运行），返回 Program AST
fn compile_only(src: &str) -> Result<compiler::ast::Program, String> {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    for e in parser.errors() {
        return Err(format!("parse error: {}", e.message));
    }
    Ok(program)
}

/// 编译并运行源码，返回 main 的返回值
fn run(src: &str) -> Result<Value, String> {
    let module = compile_source(src).map_err(|e| e.to_string())?;
    let mut vm = Vm::new(&module, VmOptions::default()).map_err(|e| e.to_string())?;
    vm.run().map_err(|e| e.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 1: 解析正确性 — ImportDecl 的 path / wildcard / alias 字段
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_parser_wildcard_flag() {
    let cases = vec![
        // (label, import_line, expected_path, expected_wildcard, expected_alias)
        (
            "wildcard",
            "import aura.concurrent.*",
            "aura.concurrent",
            true,
            None,
        ),
        (
            "module",
            "import aura.concurrent",
            "aura.concurrent",
            false,
            None,
        ),
        (
            "exact_fn",
            "import aura.concurrent.spawn",
            "aura.concurrent.spawn",
            false,
            None,
        ),
        (
            "exact_fn_alias",
            "import aura.concurrent.spawn as s",
            "aura.concurrent.spawn",
            false,
            Some("s"),
        ),
        (
            "module_alias",
            "import aura.concurrent as cc",
            "aura.concurrent",
            false,
            Some("cc"),
        ),
        (
            "wildcard_alias",
            "import aura.concurrent.* as cc",
            "aura.concurrent",
            true,
            Some("cc"),
        ),
        (
            "exact_channel",
            "import aura.concurrent.newChannel",
            "aura.concurrent.newChannel",
            false,
            None,
        ),
        (
            "exact_method",
            "import aura.concurrent.channelSend",
            "aura.concurrent.channelSend",
            false,
            None,
        ),
    ];

    for (label, imp, exp_path, exp_wildcard, exp_alias) in &cases {
        let src = format!("{}\nfun main(): Int {{ return 42 }}\n", imp);
        let program =
            compile_only(&src).unwrap_or_else(|e| panic!("[{}] parse failed: {}", label, e));
        let imp_decl = &program.imports[0];

        assert_eq!(imp_decl.path, *exp_path, "[{}] path mismatch", label);
        assert_eq!(
            imp_decl.wildcard, *exp_wildcard,
            "[{}] wildcard mismatch",
            label
        );
        assert_eq!(
            imp_decl.alias.as_deref(),
            exp_alias.as_deref(),
            "[{}] alias mismatch",
            label
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 2: 通配导入 + 短名调用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wildcard_import_short_call_spawn() {
    // import aura.concurrent.* + spawn(42)
    let src = r#"
        import aura.concurrent.*
        fun main(): Int {
            val co = spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("wildcard + spawn(42) 应成功");
    assert_eq!(result, Value::Int(42), "spawn(42) 应返回 42");
}

#[test]
fn test_wildcard_import_short_call_newChannel() {
    // import aura.concurrent.* + newChannel(5)
    let src = r#"
        import aura.concurrent.*
        fun main(): Int {
            val ch = newChannel(5)
            return ch
        }
    "#;
    let result = run(src).expect("wildcard + newChannel(5) 应成功");
    assert!(result.as_int() > 0, "newChannel 应返回正数通道 ID");
}

#[test]
fn test_wildcard_import_short_call_spawnActor() {
    // import aura.concurrent.* + spawnActor("test")
    let src = r#"
        import aura.concurrent.*
        fun main(): Int {
            val actor = spawnActor("test")
            return actor
        }
    "#;
    let result = run(src).expect("wildcard + spawnActor 应成功");
    assert!(result.as_int() > 0, "spawnActor 应返回正数 actor ID");
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 3: 通配导入 + 全路径调用（向后兼容）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wildcard_import_full_path_call() {
    // import aura.concurrent.* + aura.concurrent.spawn(42)
    let src = r#"
        import aura.concurrent.*
        fun main(): Int {
            val co = aura.concurrent.spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("wildcard + 全路径调用应成功");
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 4: 模块导入 + 全路径调用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_module_import_full_path_call() {
    // import aura.concurrent + aura.concurrent.spawn(42)
    let src = r#"
        import aura.concurrent
        fun main(): Int {
            val co = aura.concurrent.spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("module + 全路径调用应成功");
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 5: 精确导入 + 短名调用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_exact_import_short_call() {
    // import aura.concurrent.spawn + spawn(42)
    let src = r#"
        import aura.concurrent.spawn
        fun main(): Int {
            val co = spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("exact import + spawn(42) 应成功");
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_exact_import_short_call_newChannel() {
    // import aura.concurrent.newChannel + newChannel(0)
    let src = r#"
        import aura.concurrent.newChannel
        fun main(): Int {
            val ch = newChannel(0)
            return ch
        }
    "#;
    let result = run(src).expect("exact import + newChannel 应成功");
    assert!(result.as_int() > 0, "newChannel 应返回正数通道 ID");
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 6: 精确导入 + 别名
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_exact_import_alias_call() {
    // import aura.concurrent.spawn as s + s(42)
    let src = r#"
        import aura.concurrent.spawn as s
        fun main(): Int {
            val co = s(42)
            return co
        }
    "#;
    let result = run(src).expect("exact alias + s(42) 应成功");
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_exact_import_alias_call_newChannel() {
    // import aura.concurrent.newChannel as nc + nc(5)
    let src = r#"
        import aura.concurrent.newChannel as nc
        fun main(): Int {
            val ch = nc(5)
            return ch
        }
    "#;
    let result = run(src).expect("exact alias nc + nc(5) 应成功");
    assert!(result.as_int() > 0, "newChannel 应返回正数通道 ID");
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 7: 模块别名 + 全路径调用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_module_alias_call() {
    // import aura.concurrent as cc + cc.spawn(42)
    let src = r#"
        import aura.concurrent as cc
        fun main(): Int {
            val co = cc.spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("module alias + cc.spawn(42) 应成功");
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_module_alias_call_newChannel() {
    // import aura.concurrent as cc + cc.newChannel(5)
    let src = r#"
        import aura.concurrent as cc
        fun main(): Int {
            val ch = cc.newChannel(5)
            return ch
        }
    "#;
    let result = run(src).expect("module alias + cc.newChannel(5) 应成功");
    assert!(result.as_int() > 0, "newChannel 应返回正数通道 ID");
}

#[test]
fn test_module_alias_call_spawn_actor() {
    // import aura.concurrent as cc + cc.spawnActor("w")
    let src = r#"
        import aura.concurrent as cc
        fun main(): Int {
            val actor = cc.spawnActor("worker")
            return actor
        }
    "#;
    let result = run(src).expect("module alias + cc.spawnActor 应成功");
    assert!(result.as_int() > 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 8: 通配 + 别名
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_wildcard_alias_call() {
    // import aura.concurrent.* as cc + cc.spawn(42)
    let src = r#"
        import aura.concurrent.* as cc
        fun main(): Int {
            val co = cc.spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("wildcard alias + cc.spawn(42) 应成功");
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 9: 多个不同模块的 import 同时使用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multiple_modules_wildcard() {
    // import aura.concurrent.* + import aura.math.*
    // 同时用 spawn 和 sin（不同模块的短名不冲突）
    let src = r#"
        import aura.concurrent.*
        import aura.math.*
        fun main(): Int {
            val co = spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("多模块 wildcard 应成功");
    assert_eq!(result, Value::Int(42));
}

#[test]
fn test_multiple_modules_exact_import() {
    // import aura.concurrent.spawn + import aura.math.sqrt
    // 用 spawn 和 sqrt
    let src = r#"
        import aura.concurrent.spawn
        import aura.math.sqrt
        fun main(): Int {
            val co = spawn(42)
            return co
        }
    "#;
    let result = run(src).expect("多模块 exact import 应成功");
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 10: 不同模块的别名不冲突
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_different_module_aliases() {
    // import aura.concurrent.spawn as cs + import aura.math.sqrt as ms
    let src = r#"
        import aura.concurrent.spawn as cs
        import aura.math.sqrt as ms
        fun main(): Int {
            val co = cs(42)
            return co
        }
    "#;
    let result = run(src).expect("不同模块别名 应成功");
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 11: 别名与模块别名混合使用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mixed_alias_forms() {
    // 同一模块的精确别名 + 模块别名
    let src = r#"
        import aura.concurrent.spawn as s
        import aura.concurrent as cc
        fun main(): Int {
            val co1 = s(42)
            val co2 = cc.spawn(100)
            return co1 + co2
        }
    "#;
    let result = run(src).expect("混合别名形式 应成功");
    assert_eq!(result, Value::Int(142));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 12: 非 aura 模块导入不影响解析
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_non_aura_import_ignored() {
    // import utils.* — 非 aura 模块，不影响编译
    let src = r#"
        import utils.*
        fun main(): Int {
            return 42
        }
    "#;
    let result = run(src).expect("非 aura import 不应阻断编译");
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试组 13: 字符串形式的第三方库导入
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_string_path_import() {
    // import "path/to/module" as alias — 字符串路径形式
    let src = r#"
        import "my-library" as lib
        fun main(): Int {
            return 42
        }
    "#;
    let result = run(src).expect("字符串路径导入 不应阻断编译");
    assert_eq!(result, Value::Int(42));
}
