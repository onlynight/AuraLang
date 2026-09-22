//! std.string — 字符串操作与格式化
//!
//! 提供 Kotlin 风格的字符串工具：`contains`、`startsWith`、`endsWith`、
//! `split`、`replace`、`trim`、`substring`、`format` 等。

use crate::vm::native::{NativeFn, NativeRegistry};
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    // ── 纯逻辑函数（已上移到 Aura 层，但 VM 仍需 native 实现）──
    //
    // 每个方法注册两种**限定名**：
    //   * `aura.lang.std.String.<m>` — stdlib 合并后的全限定符号；
    //   * `String.<m>`               — 编译器对 String 值方法调用发射的名字。
    //
    // 早先只注册了全限定名，导致经 VM 编译的代码（例如编译器自身的 Aura 模块）
    // 里 `s.substring(a, b)` 被发射为 `String.substring` 后**无法解析**，
    // 表现为 `[vm] Unlinked external function 'String.substring', call ignored`，
    // 随后在 null 值上继续调用，最终 `call to undefined function #65535` 崩溃。
    // `String.charCodeAt` 曾被单独补过，正是同一根因的局部修法；这里统一处理。
    let qualified: &[(&str, NativeFn)] = &[
        ("contains", nat_contains),
        ("startsWith", nat_starts_with),
        ("endsWith", nat_ends_with),
        ("split", nat_split),
        ("join", nat_join),
        ("replace", nat_replace),
        ("trim", nat_trim),
        ("trimStart", nat_trim_start),
        ("trimEnd", nat_trim_end),
        ("substring", nat_substring),
        ("substringBefore", nat_substring_before),
        ("substringAfter", nat_substring_after),
        ("toLowerCase", nat_to_lower),
        ("toUpperCase", nat_to_upper),
        ("length", nat_length),
        ("isEmpty", nat_is_empty),
        ("repeat", nat_repeat),
        ("indexOf", nat_index_of),
        ("lastIndexOf", nat_last_index_of),
        ("padStart", nat_pad_start),
        ("padEnd", nat_pad_end),
        ("splitLines", nat_split_lines),
        ("joinLines", nat_join_lines),
        ("countChar", nat_count_char),
        ("first", nat_first),
        ("last", nat_last),
        ("isBlank", nat_is_blank),
        ("containsAny", nat_contains_any),
        ("containsAll", nat_contains_all),
        ("fromCharCode", nat_from_char_code),
        ("charAt", nat_char_at),
        ("charCodeAt", nat_char_at_code),
        ("replaceAll", nat_replace_all),
        ("format", nat_format),
        ("escape", nat_escape),
        ("unescape", nat_unescape),
        ("matches", nat_matches),
    ];
    for (name, f) in qualified {
        reg.register(&format!("aura.lang.std.String.{name}"), *f);
        reg.register(&format!("String.{name}"), *f);
    }

    // ── 短名注册（实例方法调用 `text.substring(i, j)` 解析为 "substring"）──
    // Aura 编译的 companion 方法（如 String.indexOf）内部调用实例方法
    // （如 `text.substring(i, j)`），编译器将短名 "substring" 发射为原生调用。
    // 这些短名不在 stdlib_func_map 中（非 prelu），需在此注册为 native 回退。
    //
    // 注意：只注册**不会与容器/其他类型同名方法冲突**的短名；
    // `length` / `first` / `last` / `trim` / `split` / `join` 等保持不注册短名，
    // 以免覆盖 List / Map 上的同名方法。
    reg.register("substring", nat_substring);
    reg.register("substringBefore", nat_substring_before);
    reg.register("substringAfter", nat_substring_after);
    reg.register("indexOf", nat_index_of);
    reg.register("lastIndexOf", nat_last_index_of);
    reg.register("replace", nat_replace);
    reg.register("contains", nat_contains);
    reg.register("startsWith", nat_starts_with);
    reg.register("endsWith", nat_ends_with);
    reg.register("toLowerCase", nat_to_lower);
    reg.register("toUpperCase", nat_to_upper);
    reg.register("fromCharCode", nat_from_char_code);
    reg.register("charCodeAt", nat_char_at_code);
    reg.register("charAt", nat_char_at);
}

fn s0(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}

fn s1(args: &[Value]) -> String {
    args.get(1).map(|v| v.as_string()).unwrap_or_default()
}

fn i0(args: &[Value]) -> i64 {
    args.first().map(|v| v.as_int()).unwrap_or(0)
}

fn i1(args: &[Value]) -> i64 {
    args.get(1).map(|v| v.as_int()).unwrap_or(0)
}

/// 兼容 self 前缀的参数提取：返回 (text, arg1, arg2_opt)
fn str_args(args: &[Value]) -> (String, String, Option<String>) {
    if args.len() >= 3 && !matches!(args.first(), Some(Value::Str(_))) {
        // self 在前：args = [self, text, arg1, arg2]
        (
            args.get(1).map(|v| v.as_string()).unwrap_or_default(),
            args.get(2).map(|v| v.as_string()).unwrap_or_default(),
            args.get(3).map(|v| v.as_string()),
        )
    } else {
        // 无 self：args = [text, arg1, arg2]
        (s0(args), s1(args), args.get(2).map(|v| v.as_string()))
    }
}

fn nat_contains(args: &[Value]) -> Value {
    let (text, target, _) = str_args(args);
    Value::Bool(text.contains(target.as_str()))
}

fn nat_starts_with(args: &[Value]) -> Value {
    let (text, target, _) = str_args(args);
    Value::Bool(text.starts_with(target.as_str()))
}

fn nat_ends_with(args: &[Value]) -> Value {
    let (text, target, _) = str_args(args);
    Value::Bool(text.ends_with(target.as_str()))
}

fn nat_split(args: &[Value]) -> Value {
    let sep = s1(args);
    let parts: Vec<Value> =
        s0(args).split(sep.as_str()).map(|s| Value::str_(s.to_string())).collect();
    Value::List(parts)
}

fn nat_join(args: &[Value]) -> Value {
    let list = s0(args);
    let sep = s1(args);
    Value::str_(list.split_whitespace().collect::<Vec<&str>>().join(sep.as_str()))
}

fn nat_replace(args: &[Value]) -> Value {
    let target = s1(args);
    let replacement = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    Value::str_(s0(args).replacen(&target, &replacement, 1))
}

fn nat_replace_all(args: &[Value]) -> Value {
    let target = s1(args);
    let replacement = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    Value::str_(s0(args).replace(&target, &replacement))
}

fn nat_trim(args: &[Value]) -> Value {
    Value::str_(s0(args).trim().to_string())
}

fn nat_trim_start(args: &[Value]) -> Value {
    Value::str_(s0(args).trim_start().to_string())
}

fn nat_trim_end(args: &[Value]) -> Value {
    Value::str_(s0(args).trim_end().to_string())
}

fn nat_substring(args: &[Value]) -> Value {
    // 兼容两种参数顺序（方法调用展开为「接收者在前」）：
    //   (text, start, end)  ← `s.substring(a, b)`
    //   (start, end, text)  ← 历史静态调用形式
    let first_is_num = matches!(args.first(), Some(Value::Int(_)) | Some(Value::Float(_)));
    let (text, start, end_opt) = if first_is_num && args.len() >= 3 {
        (
            args.get(2).map(|v| v.as_string()).unwrap_or_default(),
            i0(args).max(0) as usize,
            Some(i1(args).max(0) as usize),
        )
    } else {
        (
            s0(args),
            args.get(1).map(|v| v.as_int().max(0) as usize).unwrap_or(0),
            args.get(2).map(|v| v.as_int().max(0) as usize),
        )
    };

    // 按字符（而非字节）切片，避免非 ASCII 边界 panic
    let chars: Vec<char> = text.chars().collect();
    let end = end_opt.unwrap_or(chars.len());
    if start > chars.len() || end > chars.len() || start > end {
        return Value::str_("");
    }
    Value::str_(chars[start..end].iter().collect::<String>())
}

fn nat_substring_before(args: &[Value]) -> Value {
    let sep = s1(args);
    match s0(args).split_once(&sep) {
        Some((before, _)) => Value::str_(before.to_string()),
        None => Value::str_(s0(args)),
    }
}

fn nat_substring_after(args: &[Value]) -> Value {
    let sep = s1(args);
    match s0(args).split_once(&sep) {
        Some((_, after)) => Value::str_(after.to_string()),
        None => Value::str_(""),
    }
}

fn nat_to_lower(args: &[Value]) -> Value {
    Value::str_(s0(args).to_lowercase())
}

fn nat_to_upper(args: &[Value]) -> Value {
    Value::str_(s0(args).to_uppercase())
}

fn nat_length(args: &[Value]) -> Value {
    Value::Int(s0(args).chars().count() as i64)
}

fn nat_is_empty(args: &[Value]) -> Value {
    Value::Bool(s0(args).is_empty())
}

/// string.format(template, arg0, arg1, ...) — 将 `{0}`, `{1}` 替换为参数
fn nat_format(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::str_("");
    }
    let template = s0(args);
    let mut result = template.clone();
    for (i, arg) in args.iter().skip(1).enumerate() {
        result = result.replace(&format!("{{{i}}}"), &arg.to_string());
    }
    Value::str_(result)
}

fn nat_repeat(args: &[Value]) -> Value {
    // 兼容两种参数顺序：`text.repeat(n)` 与历史形式 `repeat(n, text)`
    let first_is_num = matches!(args.first(), Some(Value::Int(_)) | Some(Value::Float(_)));
    if first_is_num && args.len() >= 2 {
        let n = i0(args).max(0) as usize;
        return Value::str_(s1(args).repeat(n));
    }
    let s = s0(args);
    let n = args.get(1).map(|v| v.as_int().max(0) as usize).unwrap_or(0);
    Value::str_(s.repeat(n))
}

fn nat_index_of(args: &[Value]) -> Value {
    // 兼容两种参数顺序：
    //   (text, substring)        ← 独立调用
    //   (self, text, substring)  ← companion 方法调用（VM 注入 self）
    let (text, target) = if args.len() >= 3 && !matches!(args.first(), Some(Value::Str(_))) {
        // self 在前：args = [self, text, substring]
        (
            args.get(1).map(|v| v.as_string()).unwrap_or_default(),
            args.get(2).map(|v| v.as_string()).unwrap_or_default(),
        )
    } else {
        (s0(args), s1(args))
    };
    match text.find(&target) {
        Some(pos) => Value::Int(pos as i64),
        None => Value::Int(-1),
    }
}

fn nat_last_index_of(args: &[Value]) -> Value {
    let (text, target) = if args.len() >= 3 && !matches!(args.first(), Some(Value::Str(_))) {
        (
            args.get(1).map(|v| v.as_string()).unwrap_or_default(),
            args.get(2).map(|v| v.as_string()).unwrap_or_default(),
        )
    } else {
        (s0(args), s1(args))
    };
    match text.rfind(&target) {
        Some(pos) => Value::Int(pos as i64),
        None => Value::Int(-1),
    }
}

/// 解析 padStart/padEnd 的参数：兼容 `text.padStart(len, pad)` 与 `padStart(len, pad, text)`
fn pad_args(args: &[Value]) -> (String, usize, String) {
    let first_is_num = matches!(args.first(), Some(Value::Int(_)) | Some(Value::Float(_)));
    if first_is_num && args.len() >= 3 {
        let len = i0(args).max(0) as usize;
        let pad = s1(args);
        let text = args.get(2).map(|v| v.as_string()).unwrap_or_default();
        return (text, len, pad);
    }
    let text = s0(args);
    let len = args.get(1).map(|v| v.as_int().max(0) as usize).unwrap_or(0);
    let pad = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    (text, len, pad)
}

fn nat_pad_start(args: &[Value]) -> Value {
    let (s, len, pad) = pad_args(args);
    let pad_char = if pad.is_empty() { ' ' } else { pad.chars().next().unwrap_or(' ') };
    let pad_len = len.saturating_sub(s.chars().count());
    let padding: String = std::iter::repeat(pad_char).take(pad_len).collect();
    Value::str_(padding + &s)
}

fn nat_pad_end(args: &[Value]) -> Value {
    let (s, len, pad) = pad_args(args);
    let pad_char = if pad.is_empty() { ' ' } else { pad.chars().next().unwrap_or(' ') };
    let pad_len = len.saturating_sub(s.chars().count());
    let padding: String = std::iter::repeat(pad_char).take(pad_len).collect();
    Value::str_(s + &padding)
}

fn nat_escape(args: &[Value]) -> Value {
    let s = s0(args);
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    Value::str_(escaped)
}

fn nat_unescape(args: &[Value]) -> Value {
    let s = s0(args);
    let unescaped = s
        .replace("\\\\", "\\")
        .replace("\\\"", "\"")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t");
    Value::str_(unescaped)
}

fn nat_split_lines(args: &[Value]) -> Value {
    let lines: Vec<Value> = s0(args).lines().map(|s| Value::str_(s.to_string())).collect();
    Value::List(lines)
}

fn nat_join_lines(args: &[Value]) -> Value {
    let list = s0(args);
    Value::str_(list.lines().collect::<Vec<&str>>().join("\n"))
}

fn nat_count_char(args: &[Value]) -> Value {
    let target = s1(args);
    Value::Int(s0(args).chars().filter(|c| target.contains(*c)).count() as i64)
}

fn nat_first(args: &[Value]) -> Value {
    match s0(args).chars().next() {
        Some(c) => Value::str_(c.to_string()),
        None => Value::Null,
    }
}

fn nat_last(args: &[Value]) -> Value {
    match s0(args).chars().next_back() {
        Some(c) => Value::str_(c.to_string()),
        None => Value::Null,
    }
}

fn nat_is_blank(args: &[Value]) -> Value {
    Value::Bool(s0(args).trim().is_empty())
}

/// string.matches(pattern, regex) — 正则匹配
fn nat_matches(args: &[Value]) -> Value {
    let pattern = s0(args);
    let regex_str = s1(args);
    match regex::Regex::new(&regex_str) {
        Ok(re) => Value::Bool(re.is_match(&pattern)),
        Err(_) => Value::Bool(false),
    }
}

fn nat_contains_any(args: &[Value]) -> Value {
    let s = s0(args);
    let sep = s1(args);
    let result = sep.split('|').any(|p| s.contains(p));
    Value::Bool(result)
}

fn nat_contains_all(args: &[Value]) -> Value {
    let s = s0(args);
    let sep = s1(args);
    Value::Bool(sep.split('|').all(|p| s.contains(p)))
}

/// String.fromCharCode(code) — 从 Unicode 码点创建单字符字符串
fn nat_from_char_code(args: &[Value]) -> Value {
    let code = i0(args) as u32;
    match char::from_u32(code) {
        Some(c) => Value::str_(c.to_string()),
        None => Value::Null,
    }
}

/// String.charAt(index) — 取位置 index 的字符（越界返回空串）
///
/// 兼容两种参数布局：
///   * `(text, index)`         — 实例方法 / 静态方法调用
///   * `(self, text, index)`   — companion 方法调用（VM 注入 self）
fn nat_char_at(args: &[Value]) -> Value {
    let (text, idx) = if args.len() >= 3 && !matches!(args.first(), Some(Value::Str(_))) {
        (
            args.get(1).map(|v| v.as_string()).unwrap_or_default(),
            args.get(2).map(|v| v.as_int()).unwrap_or(0),
        )
    } else if args.len() >= 2 && matches!(args.get(1), Some(Value::Int(_))) {
        (s0(args), args.get(1).map(|v| v.as_int()).unwrap_or(0))
    } else {
        (s0(args), i0(args))
    };
    match text.chars().nth(idx as usize) {
        Some(c) => Value::str_(c.to_string()),
        None => Value::str_(""),
    }
}

/// String.charCodeAt(index) — 获取位置 index 字符的 Unicode 码点（越界返回 -1）
///
/// ⚠ 历史缺陷（2026-09-23 修复）：此处曾写成 `let idx = i0(args)`，而 `i0`
/// 取的是 **args[0]（接收者字符串本身）**，于是索引恒为 0 —— **任何**
/// `s.charCodeAt(i)` 都返回**首字符**的码点。后果极广（VM 解释路径下所有
/// 「按字符码驱动」的逻辑同时错）：
///   * `HirUtils.hirFieldAt` 拿 `,` 的码点（44）去比较 → 分隔符永不命中，
///     整串被当成**一个字段**返回；
///   * `HirUtils.hirKidsCount` 恒为 1 → **所有** kids 列表被截成 1 个元素；
///   * `HirUtils.hirToIntOf("41")` 得 44（每位都取到首字符 `'4'` → 4*10+4）；
///   表象则是一连串「看不出关联」的症状：自举编译器发射的字节码**只剩函数头**、
///   photon `.phir` 解析出 **0 个函数**、`splitComma` 切不开寄存器表……
/// 参数布局与 `nat_char_at` 保持一致（兼容 VM 注入 self 的 companion 调用）。
fn nat_char_at_code(args: &[Value]) -> Value {
    let (text, idx) = if args.len() >= 3 && !matches!(args.first(), Some(Value::Str(_))) {
        (
            args.get(1).map(|v| v.as_string()).unwrap_or_default(),
            args.get(2).map(|v| v.as_int()).unwrap_or(0),
        )
    } else if args.len() >= 2 && matches!(args.get(1), Some(Value::Int(_))) {
        (s0(args), args.get(1).map(|v| v.as_int()).unwrap_or(0))
    } else {
        (s0(args), i0(args))
    };
    match text.chars().nth(idx as usize) {
        Some(c) => Value::Int(c as i64),
        None => Value::Int(-1),
    }
}
