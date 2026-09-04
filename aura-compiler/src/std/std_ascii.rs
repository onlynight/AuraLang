//! std.ascii — 字符工具
//!
//! 提供字符分类、转换等 ASCII 和 Unicode 字符操作。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("ascii.isAlpha", nat_is_alpha);
    reg.register("ascii.isDigit", nat_is_digit);
    reg.register("ascii.isAlphaNumeric", nat_is_alphanumeric);
    reg.register("ascii.isWhitespace", nat_is_whitespace);
    reg.register("ascii.isUpper", nat_is_upper);
    reg.register("ascii.isLower", nat_is_lower);
    reg.register("ascii.toUpper", nat_to_upper);
    reg.register("ascii.toLower", nat_to_lower);
    reg.register("ascii.codeAt", nat_code_at);
    reg.register("ascii.charAt", nat_char_at);
    reg.register("ascii.fromCode", nat_from_code);
    reg.register("ascii.codePointAt", nat_code_point_at);
}

fn s0(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}

fn i0(args: &[Value]) -> i64 {
    args.first().map(|v| v.as_int()).unwrap_or(0)
}

/// ascii.isAlpha(text) → Bool (first char is alphabetic)
fn nat_is_alpha(args: &[Value]) -> Value {
    let s = s0(args);
    match s.chars().next() {
        Some(c) => Value::Bool(c.is_alphabetic()),
        None => Value::Bool(false),
    }
}

/// ascii.isDigit(text) → Bool (first char is digit)
fn nat_is_digit(args: &[Value]) -> Value {
    let s = s0(args);
    match s.chars().next() {
        Some(c) => Value::Bool(c.is_ascii_digit()),
        None => Value::Bool(false),
    }
}

/// ascii.isAlphaNumeric(text) → Bool (all chars are alphanumeric)
fn nat_is_alphanumeric(args: &[Value]) -> Value {
    Value::Bool(s0(args).chars().all(|c| c.is_alphanumeric()))
}

/// ascii.isWhitespace(text) → Bool (all chars are whitespace)
fn nat_is_whitespace(args: &[Value]) -> Value {
    Value::Bool(s0(args).chars().all(|c| c.is_whitespace()))
}

/// ascii.isUpper(text) → Bool (first char is uppercase)
fn nat_is_upper(args: &[Value]) -> Value {
    let s = s0(args);
    match s.chars().next() {
        Some(c) => Value::Bool(c.is_uppercase()),
        None => Value::Bool(false),
    }
}

/// ascii.isLower(text) → Bool (first char is lowercase)
fn nat_is_lower(args: &[Value]) -> Value {
    let s = s0(args);
    match s.chars().next() {
        Some(c) => Value::Bool(c.is_lowercase()),
        None => Value::Bool(false),
    }
}

/// ascii.toUpper(text) → String (first char uppercase)
fn nat_to_upper(args: &[Value]) -> Value {
    let s = s0(args);
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let upper = c.to_uppercase();
            Value::str_(upper.to_string() + &chars.as_str())
        }
        None => Value::str_(""),
    }
}

/// ascii.toLower(text) → String (first char lowercase)
fn nat_to_lower(args: &[Value]) -> Value {
    let s = s0(args);
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let lower = c.to_lowercase();
            Value::str_(lower.to_string() + &chars.as_str())
        }
        None => Value::str_(""),
    }
}

/// ascii.codeAt(text, index) → Int (code point at index)
fn nat_code_at(args: &[Value]) -> Value {
    let s = s0(args);
    let idx = i0(args) as usize;
    match s.chars().nth(idx) {
        Some(c) => Value::Int(c as i64),
        None => Value::Int(0),
    }
}

/// ascii.charAt(text, index) → String (character at index)
fn nat_char_at(args: &[Value]) -> Value {
    let s = s0(args);
    let idx = i0(args) as usize;
    match s.chars().nth(idx) {
        Some(c) => Value::str_(c.to_string()),
        None => Value::Null,
    }
}

/// ascii.fromCode(code) → String (character from code point)
fn nat_from_code(args: &[Value]) -> Value {
    let code = i0(args) as u32;
    match char::from_u32(code) {
        Some(c) => Value::str_(c.to_string()),
        None => Value::Null,
    }
}

/// ascii.codePointAt(text, index) → Int (code point, alias for codeAt)
fn nat_code_point_at(args: &[Value]) -> Value {
    nat_code_at(args)
}
