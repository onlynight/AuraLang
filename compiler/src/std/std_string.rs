//! std.string — 字符串操作与格式化
//!
//! 提供 Kotlin 风格的字符串工具：`contains`、`startsWith`、`endsWith`、
//! `split`、`replace`、`trim`、`substring`、`format` 等。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    // ── 纯逻辑函数（已上移到 Aura 层，但 VM 仍需 native 实现）──
    reg.register("aura.lang.std.String.contains", nat_contains);
    reg.register("aura.lang.std.String.startsWith", nat_starts_with);
    reg.register("aura.lang.std.String.endsWith", nat_ends_with);
    reg.register("aura.lang.std.String.split", nat_split);
    reg.register("aura.lang.std.String.join", nat_join);
    reg.register("aura.lang.std.String.replace", nat_replace);
    reg.register("aura.lang.std.String.trim", nat_trim);
    reg.register("aura.lang.std.String.trimStart", nat_trim_start);
    reg.register("aura.lang.std.String.trimEnd", nat_trim_end);
    reg.register("aura.lang.std.String.substring", nat_substring);
    reg.register("aura.lang.std.String.substringBefore", nat_substring_before);
    reg.register("aura.lang.std.String.substringAfter", nat_substring_after);
    reg.register("aura.lang.std.String.toLowerCase", nat_to_lower);
    reg.register("aura.lang.std.String.toUpperCase", nat_to_upper);
    reg.register("aura.lang.std.String.length", nat_length);
    reg.register("aura.lang.std.String.isEmpty", nat_is_empty);
    reg.register("aura.lang.std.String.repeat", nat_repeat);
    reg.register("aura.lang.std.String.indexOf", nat_index_of);
    reg.register("aura.lang.std.String.lastIndexOf", nat_last_index_of);
    reg.register("aura.lang.std.String.padStart", nat_pad_start);
    reg.register("aura.lang.std.String.padEnd", nat_pad_end);
    reg.register("aura.lang.std.String.splitLines", nat_split_lines);
    reg.register("aura.lang.std.String.joinLines", nat_join_lines);
    reg.register("aura.lang.std.String.countChar", nat_count_char);
    reg.register("aura.lang.std.String.first", nat_first);
    reg.register("aura.lang.std.String.last", nat_last);
    reg.register("aura.lang.std.String.isBlank", nat_is_blank);
    reg.register("aura.lang.std.String.containsAny", nat_contains_any);
    reg.register("aura.lang.std.String.containsAll", nat_contains_all);

    // ── 复杂函数（正则/格式化/转义，Rust native 实现）──
    reg.register("aura.lang.std.String.replaceAll", nat_replace_all);
    reg.register("aura.lang.std.String.format", nat_format);
    reg.register("aura.lang.std.String.escape", nat_escape);
    reg.register("aura.lang.std.String.unescape", nat_unescape);
    reg.register("aura.lang.std.String.matches", nat_matches);
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

fn nat_contains(args: &[Value]) -> Value {
    Value::Bool(s0(args).contains(s1(args).as_str()))
}

fn nat_starts_with(args: &[Value]) -> Value {
    Value::Bool(s0(args).starts_with(s1(args).as_str()))
}

fn nat_ends_with(args: &[Value]) -> Value {
    Value::Bool(s0(args).ends_with(s1(args).as_str()))
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
    let start = i0(args) as usize;
    let end = args.get(1).map(|v| v.as_int() as usize).unwrap_or(s0(args).len());
    let s = s0(args);
    if start > s.len() || end > s.len() {
        return Value::str_("");
    }
    Value::str_(s[start..end].to_string())
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
    let n = i0(args) as usize;
    Value::str_(s1(args).repeat(n))
}

fn nat_index_of(args: &[Value]) -> Value {
    let target = s1(args);
    match s0(args).find(&target) {
        Some(pos) => Value::Int(pos as i64),
        None => Value::Int(-1),
    }
}

fn nat_last_index_of(args: &[Value]) -> Value {
    let target = s1(args);
    match s0(args).rfind(&target) {
        Some(pos) => Value::Int(pos as i64),
        None => Value::Int(-1),
    }
}

fn nat_pad_start(args: &[Value]) -> Value {
    let len = i0(args) as usize;
    let pad = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    let s = s1(args);
    let pad_char = if pad.is_empty() { ' ' } else { pad.chars().next().unwrap_or(' ') };
    let pad_len = len.saturating_sub(s.chars().count());
    let padding: String = std::iter::repeat(pad_char).take(pad_len).collect();
    Value::str_(padding + &s)
}

fn nat_pad_end(args: &[Value]) -> Value {
    let len = i0(args) as usize;
    let pad = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    let s = s1(args);
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
