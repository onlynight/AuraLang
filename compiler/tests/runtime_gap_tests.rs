//! 运行时缺口回归测试
//!
//! 覆盖一组「Aura 自举（纯 Aura 化迁移）过程中暴露的 VM / 标准库缺陷」修复：
//!
//! | # | 缺陷 | 修复位置 |
//! |---|------|----------|
//! | 1 | prelude 裸名未注册 → `isNull` 恒假、`listOf` 被忽略 | `std::register_prelude` + `vm::native` |
//! | 2 | `s[i]` 字符串索引返回 `null` | `vm::interp` `Instr::GetIndex` |
//! | 3 | `substring` / `repeat` / `padStart` / `padEnd` 参数顺序错位 | `std::std_string` |
//! | 4 | 集合 / 字符串内建成员（`.size` / `.first` / `.last` / `.isEmpty`）不可用 | `vm::interp` `builtin_member` |
//! | 5 | 字符串字面量后接 `.方法()` / `[索引]` 解析失败 | `parser::parse_postfix_chain` |
//! | 6 | 原生函数与内嵌 Aura 标准库实现同名时表示不一致（`listOf`） | `vm::interp` native 优先 |

use compiler::codegen::compile_source;
use compiler::vm::value::Value;
use compiler::vm::{Vm, VmOptions};

/// 编译并运行源码，返回 `main` 的返回值。
fn run(src: &str) -> Result<Value, String> {
    let module = compile_source(src).map_err(|e| e.to_string())?;
    let mut vm = Vm::new(&module, VmOptions::default()).map_err(|e| e.to_string())?;
    vm.run().map_err(|e| e.to_string())
}

/// 断言返回 `Value::Str`
fn expect_str(src: &str, expected: &str) {
    let got = run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsource:\n{src}"));
    assert_eq!(got, Value::Str(expected.into()), "source:\n{src}");
}

/// 断言返回 `Value::Int`
fn expect_int(src: &str, expected: i64) {
    let got = run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsource:\n{src}"));
    assert_eq!(got, Value::Int(expected), "source:\n{src}");
}

/// 断言返回 `Value::Bool`
fn expect_bool(src: &str, expected: bool) {
    let got = run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsource:\n{src}"));
    assert_eq!(got, Value::Bool(expected), "source:\n{src}");
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. prelude 裸名（isNull / isNotNull / toBool）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_is_null_literal_true() {
    expect_bool("fun main(): Boolean { return isNull(null) }", true);
}

#[test]
fn test_is_null_non_null_false() {
    expect_bool("fun main(): Boolean { return isNull(42) }", false);
}

#[test]
fn test_is_not_null() {
    expect_bool("fun main(): Boolean { return isNotNull(\"x\") }", true);
    expect_bool("fun main(): Boolean { return isNotNull(null) }", false);
}

#[test]
fn test_to_bool() {
    expect_bool("fun main(): Boolean { return toBool(0) }", false);
    expect_bool("fun main(): Boolean { return toBool(1) }", true);
}

#[test]
fn test_is_zero_prelude() {
    expect_bool("fun main(): Boolean { return isZero(0) }", true);
    expect_bool("fun main(): Boolean { return isZero(3) }", false);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. 字符串索引 s[i]
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_string_index() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"hello\"; return s[0] }",
        "h",
    );
}

#[test]
fn test_string_index_last_char() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"hello\"; return s[4] }",
        "o",
    );
}

#[test]
fn test_string_index_out_of_range_is_null() {
    // 越界返回 null（而非 panic）
    let got =
        run("import aura.lang.std.String\nfun main(): Any { val s: String = \"hi\"; return s[9] }")
            .expect("run should succeed");
    assert_eq!(got, Value::Null);
}

#[test]
fn test_string_scan_loop() {
    // 逐字符扫描：统计 'l' 出现次数
    expect_int(
        "import aura.lang.std.String\n\
         fun main(): Int {\n\
           val s: String = \"hello\"\n\
           var i: Int = 0\n\
           var n: Int = 0\n\
           while (i < s.length()) {\n\
             if (s[i] == \"l\") { n = n + 1 }\n\
             i = i + 1\n\
           }\n\
           return n\n\
         }",
        2,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. String 原生函数参数顺序
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_substring_method_form() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"hello\"; return s.substring(1, 3) }",
        "el",
    );
}

#[test]
fn test_substring_full_range() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"hello\"; return s.substring(0, 5) }",
        "hello",
    );
}

#[test]
fn test_repeat_method_form() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"ab\"; return s.repeat(3) }",
        "ababab",
    );
}

#[test]
fn test_pad_start_method_form() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"ab\"; return s.padStart(5, \"0\") }",
        "000ab",
    );
}

#[test]
fn test_pad_end_method_form() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"ab\"; return s.padEnd(5, \"-\") }",
        "ab---",
    );
}

#[test]
fn test_substring_non_ascii_no_panic() {
    // 按字符切片，非 ASCII 不应因字节边界 panic
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"中文abc\"; return s.substring(0, 2) }",
        "中文",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 集合 / 字符串内建成员
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_list_size() {
    expect_int(
        "fun main(): Int { val xs = listOf(1, 2, 3); return xs.size }",
        3,
    );
}

#[test]
fn test_list_first_last() {
    expect_int(
        "fun main(): Int { val xs = listOf(7, 8, 9); return xs.first }",
        7,
    );
    expect_int(
        "fun main(): Int { val xs = listOf(7, 8, 9); return xs.last }",
        9,
    );
}

#[test]
fn test_list_is_empty() {
    expect_bool(
        "fun main(): Boolean { val xs = listOf(1); return xs.isEmpty }",
        false,
    );
}

#[test]
fn test_list_index() {
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs[1] }",
        5,
    );
}

/// list.get(i) 与 list[i] 等价，返回相同的元素值
#[test]
fn test_list_get() {
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs.get(1) }",
        5,
    );
}

#[test]
fn test_list_get_first_and_last() {
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs.get(0) }",
        4,
    );
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs.get(2) }",
        6,
    );
}

/// list.getAt(i) 与 list[i] 等价（Collection 通用接口）
#[test]
fn test_list_getat() {
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs.getAt(1) }",
        5,
    );
}

/// list.count 成员访问（Collection 通用接口）
#[test]
fn test_list_count() {
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs.count }",
        3,
    );
}

/// list.size 向后兼容（Collection 通用接口别名）
#[test]
fn test_list_size_still_works() {
    expect_int(
        "fun main(): Int { val xs = listOf(4, 5, 6); return xs.size }",
        3,
    );
}

/// list.isEmpty 成员访问（Collection 通用接口）
#[test]
fn test_list_is_empty_collection() {
    expect_bool(
        "fun main(): Boolean { val xs = listOf(1); return xs.isEmpty }",
        false,
    );
}

/// list.contains 方法（Collection 通用接口）
#[test]
fn test_list_contains_collection() {
    expect_bool(
        "fun main(): Boolean { val xs = listOf(1, 2, 3); return xs.contains(2) }",
        true,
    );
}

/// list.indexOf 方法（Collection 通用接口）
#[test]
fn test_list_index_of_collection() {
    expect_int(
        "fun main(): Int { val xs = listOf(1, 2, 3); return xs.indexOf(2) }",
        1,
    );
}

/// array[i] = v 下标赋值（等同于 array.set(i, v)）
#[test]
fn test_array_subscript_assign() {
    expect_int(
        "fun main(): Int { val arr = mutableListOf(10, 20, 30); arr[1] = 99; return arr[1] }",
        99,
    );
}

/// array.set(i, v) 方法赋值
#[test]
fn test_array_set_method() {
    expect_int(
        "fun main(): Int { val arr = mutableListOf(10, 20, 30); arr.set(1, 99); return arr[1] }",
        99,
    );
}

/// array[i] = v 和 array.set(i, v) 等价
#[test]
fn test_array_subscript_assign_equivalence() {
    expect_int(
        "fun main(): Int { val a = mutableListOf(1, 2, 3); a[0] = 10; val b = mutableListOf(1, 2, 3); b.set(0, 10); return a[0] + b[0] }",
        20,
    );
}

#[test]
fn test_string_size_and_length() {
    expect_int(
        "import aura.lang.std.String\nfun main(): Int { val s: String = \"hello\"; return s.size }",
        5,
    );
    expect_int(
        "import aura.lang.std.String\nfun main(): Int { val s: String = \"hello\"; return s.length() }",
        5,
    );
}

#[test]
fn test_string_first_last() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"aura\"; return s.first }",
        "a",
    );
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val s: String = \"aura\"; return s.last }",
        "a",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 字面量后缀链（解析修复）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_string_literal_method_call() {
    expect_int(
        "import aura.lang.std.String\nfun main(): Int { return \"hello\".length() }",
        5,
    );
}

#[test]
fn test_string_literal_index() {
    expect_str(
        "import aura.lang.std.String\nfun main(): String { return \"hello\"[1] }",
        "e",
    );
}

#[test]
fn test_string_literal_split() {
    // 字面量后接 .split(...) 应可用
    expect_int(
        "import aura.lang.std.String\n\
         fun main(): Int {\n\
           val parts = \"a,b,c\".split(\",\")\n\
           return parts.size\n\
         }",
        3,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. listOf 原生与内嵌 Aura 实现的一致性
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_list_of_two_args_is_indexable() {
    // 2 个参数曾命中内嵌 Aura 版 listOf（返回 ArrayList 实例）导致索引/大小为 null
    expect_str(
        "import aura.lang.std.String\nfun main(): String { val xs = listOf(\"a\", \"b\"); return xs[1] }",
        "b",
    );
}

#[test]
fn test_list_of_many_args() {
    expect_int(
        "fun main(): Int { val xs = listOf(1, 2, 3, 4, 5); return xs.size }",
        5,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. object 单例字段默认值
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_object_field_default_value() {
    // 单例字段的声明默认值必须生效（此前统一为 null）
    expect_int(
        "object Config {\n\
           var retries: Int = 5\n\
           fun readR(): Int { return retries }\n\
         }\n\
         fun main(): Int { return Config.readR() }",
        5,
    );
}

#[test]
fn test_object_field_default_zero() {
    expect_int(
        "object CounterX {\n\
           var n: Int = 0\n\
           fun readN(): Int { return n }\n\
         }\n\
         fun main(): Int { return CounterX.readN() }",
        0,
    );
}

#[test]
fn test_object_field_accumulates_as_int() {
    // 默认值生效后，累加应保持 Int（此前 null 参与加法会退化为 Float，7 → 7.0）
    expect_int(
        "object Acc {\n\
           var n: Int = 0\n\
           fun add(x: Int) { n = n + x }\n\
           fun readA(): Int { return n }\n\
         }\n\
         fun main(): Int {\n\
           Acc.add(3)\n\
           Acc.add(4)\n\
           return Acc.readA()\n\
         }",
        7,
    );
}

#[test]
fn test_object_multiple_fields_defaults() {
    expect_int(
        "object Pair2 {\n\
           var a: Int = 1\n\
           var b: Int = 2\n\
           fun readA(): Int { return a }\n\
           fun readB(): Int { return b }\n\
         }\n\
         fun main(): Int { return Pair2.readA() * 10 + Pair2.readB() }",
        12,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. 异常传播（try / catch / finally / throw）
// ─────────────────────────────────────────────────────────────────────────────

/// 编译运行并返回 `(返回值, Process.exit 请求的退出码)`。
fn run_with_exit(src: &str) -> (Value, Option<i32>) {
    let module = compile_source(src).unwrap_or_else(|e| panic!("compilation failed: {e}\n{src}"));
    let mut vm = Vm::new(&module, VmOptions::default()).expect("vm init");
    let v = vm.run().unwrap_or(Value::Null);
    (v, vm.requested_exit_code())
}

#[test]
fn test_throw_caught_binds_value() {
    expect_int(
        "fun main(): Int {\n\
           var r: Int = 0\n\
           try { throw 42 } catch (e: Int) { r = e }\n\
           return r\n\
         }",
        42,
    );
}

#[test]
fn test_catch_skipped_when_no_throw() {
    expect_int(
        "fun main(): Int {\n\
           var r: Int = 1\n\
           try { r = 2 } catch (e: Int) { r = 99 }\n\
           return r\n\
         }",
        2,
    );
}

#[test]
fn test_finally_runs_on_success_path() {
    expect_int(
        "fun main(): Int {\n\
           var r: Int = 0\n\
           try { r = r + 1 } catch (e: Int) { r = r + 10 } finally { r = r + 100 }\n\
           return r\n\
         }",
        101,
    );
}

#[test]
fn test_finally_runs_on_throw_path() {
    expect_int(
        "fun main(): Int {\n\
           var r: Int = 0\n\
           try { throw 1 } catch (e: Int) { r = r + 10 } finally { r = r + 100 }\n\
           return r\n\
         }",
        110,
    );
}

#[test]
fn test_throw_unwinds_across_frames() {
    expect_int(
        "fun boom(): Int { throw 7 }\n\
         fun main(): Int {\n\
           var r: Int = 0\n\
           try { val v: Int = boom()\n  r = 99 } catch (e: Int) { r = e }\n\
           return r\n\
         }",
        7,
    );
}

#[test]
fn test_handler_popped_after_normal_completion() {
    // 第一个 try 的处理器必须已注销，第二个 try 的异常不应进入旧处理器
    expect_int(
        "fun main(): Int {\n\
           var r: Int = 0\n\
           try { r = 1 } catch (e: Int) { r = 99 }\n\
           try { throw 5 } catch (e: Int) { r = r + e }\n\
           return r\n\
         }",
        6,
    );
}

#[test]
fn test_handler_re_registered_per_iteration() {
    expect_int(
        "fun main(): Int {\n\
           var sum: Int = 0\n\
           var i: Int = 0\n\
           while (i < 3) {\n\
             try {\n\
               if (i == 1) { throw 10 }\n\
               sum = sum + i\n\
             } catch (e: Int) { sum = sum + e }\n\
             i = i + 1\n\
           }\n\
           return sum\n\
         }",
        12,
    );
}

#[test]
fn test_finally_only_rethrows_to_outer_handler() {
    expect_int(
        "fun main(): Int {\n\
           var r: Int = 0\n\
           try {\n\
             try { throw 3 } finally { r = r + 1 }\n\
           } catch (e: Int) { r = r + 100 + e }\n\
           return r\n\
         }",
        104,
    );
}

#[test]
fn test_uncaught_throw_is_runtime_error() {
    let err = run("fun main(): Int { throw 1 }").unwrap_err();
    assert!(
        err.contains("uncaught exception"),
        "错误信息应含 uncaught exception，实际: {err}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Process.exit 退出码
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_process_exit_records_code() {
    let (_, code) = run_with_exit("fun main(): Int { Process.exit(3)\n return 0 }");
    assert_eq!(code, Some(3), "Process.exit(3) should record exit code 3");
}

#[test]
fn test_process_exit_zero_is_recorded() {
    let (_, code) = run_with_exit("fun main(): Int { Process.exit(0)\n return 7 }");
    assert_eq!(
        code,
        Some(0),
        "explicit exit(0) should record 0 and not execute subsequent statements"
    );
}

#[test]
fn test_no_exit_request_by_default() {
    let (v, code) = run_with_exit("fun main(): Int { return 5 }");
    assert_eq!(v, Value::Int(5));
    assert_eq!(
        code, None,
        "no exit code request when Process.exit not called"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. 字符串插值词法（与 Rust 参考实现的 token 协议一致）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_lexer_string_interp_kinds() {
    use compiler::lexer::Lexer;
    let toks = Lexer::new("\"a $name b ${n + 1} c\"").tokenize();
    let kinds: Vec<String> = toks.iter().map(|t| format!("{:?}", t.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            "StringLiteral",
            "StringInterpStart",
            "StringLiteral",
            "StringInterpStart",
            "StringLiteral",
            "EOF"
        ],
        "插值应拆分为 StringLiteral / StringInterpStart 交替序列"
    );
    assert_eq!(toks[1].literal, "$name");
    assert_eq!(toks[3].literal, "${n + 1}");
}

#[test]
fn test_lexer_literal_dollar_not_interp() {
    use compiler::lexer::Lexer;
    // `$5` 后接数字 → 非插值，整体作为字面量
    let toks = Lexer::new("\"cost: $5\"").tokenize();
    let kinds: Vec<String> = toks.iter().map(|t| format!("{:?}", t.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            "StringLiteral",
            "EOF"
        ]
    );
    assert_eq!(toks[0].literal, "cost: $5");
}

#[test]
fn test_str_interp_expression_evaluates() {
    // 端到端：插值片段被降级为 toString + 加法链
    expect_str(
        "fun main(): String {\n\
           val name: String = \"aura\"\n\
           val n: Int = 2\n\
           return \"hi $name/${n + 1}\"\n\
         }",
        "hi aura/3",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. 无 import 的程序也必须获得全量 std 原生注册
// ─────────────────────────────────────────────────────────────────────────────

/// `Vm::new` 在 `enabled_modules` 为空（无 import）时走 `NativeRegistry::new()`。
///
/// 此前 `new()` 只注册了一个硬编码的 prelude 子集，并未调用 `std::register_all()`，
/// 导致 `aura.lang.std.String.length` / `Math.sin` 等模块原生函数**未注册**，
/// 调用落入「未链接的外部函数」兜底分支被静默忽略并返回 `Int(0)`
/// ——表现为 `"abc".length() == 0` 这类错值。
#[test]
fn test_full_registry_without_imports_includes_std_modules() {
    let module =
        compile_source("fun main(): Int { return 0 }").expect("compilation should succeed");
    assert!(
        module.enabled_modules.is_empty(),
        "无 import 时 enabled_modules 应为空"
    );
    let vm = Vm::new(&module, VmOptions::default()).expect("VM creation should succeed");
    assert!(
        vm.contains_native("aura.lang.std.String.length"),
        "全量注册应包含 aura.lang.std.String.length"
    );
    assert!(
        vm.contains_native("aura.lang.std.Math.sin"),
        "全量注册应包含 aura.lang.std.Math.sin"
    );
}

#[test]
fn test_string_length_works_without_import() {
    // 不写 import 时 `"abc".length()` 必须得到 3，而不是静默的 0
    expect_int(
        "fun main(): Int {\n\
           val s: String = \"abc\"\n\
           return s.length()\n\
         }",
        3,
    );
}

#[test]
fn test_string_helpers_work_without_import() {
    expect_bool(
        "fun main(): Boolean {\n\
           val s: String = \"abc\"\n\
           return s.contains(\"b\")\n\
         }",
        true,
    );
    expect_int(
        "fun main(): Int {\n\
           val s: String = \"a-b-c\"\n\
           return s.indexOf(\"-\")\n\
         }",
        1,
    );
}
