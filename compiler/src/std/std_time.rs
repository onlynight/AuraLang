//! std.time — 时间/日期工具
//!
//! 提供时间戳、日期格式化、时长计算等基础功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn register(reg: &mut NativeRegistry) {
    // ── 纯逻辑函数（已上移到 Aura 层，但 VM 仍需 native 实现）──
    reg.register("aura.lang.std.Time.duration", nat_duration);
    reg.register("aura.lang.std.Time.diff", nat_diff);

    // ── 时间戳/睡眠/日期解析（Rust native 实现）──
    reg.register("aura.lang.std.Time.now", nat_now);
    reg.register("aura.lang.std.Time.epoch", nat_epoch);
    reg.register("aura.lang.std.Time.currentTime", nat_current_time);
    reg.register("aura.lang.std.Time.sleep", nat_sleep);
    reg.register("aura.lang.std.Time.toDateString", nat_to_date_string);
    reg.register("aura.lang.std.Time.toTimeString", nat_to_time_string);
    reg.register("aura.lang.std.Time.formatDate", nat_format_date);
    reg.register("aura.lang.std.Time.parseDate", nat_parse_date);
}

/// time.now() → Float (seconds since epoch)
fn nat_now(_args: &[Value]) -> Value {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    Value::Float(now)
}

/// time.epoch() → Float (alias for now)
fn nat_epoch(args: &[Value]) -> Value {
    nat_now(args)
}

/// time.currentTime() → Float (milliseconds since epoch)
fn nat_current_time(_args: &[Value]) -> Value {
    let now =
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    Value::Float(now)
}

/// time.sleep(seconds) → Unit
fn nat_sleep(args: &[Value]) -> Value {
    let secs = args.first().map(|v| v.as_float()).unwrap_or(0.0);
    std::thread::sleep(Duration::from_secs_f64(secs));
    Value::Null
}

/// time.duration(secs) → Float (duration in seconds)
fn nat_duration(args: &[Value]) -> Value {
    Value::Float(args.first().map(|v| v.as_float()).unwrap_or(0.0))
}

/// time.toDateString(seconds) → String (YYYY-MM-DD)
fn nat_to_date_string(args: &[Value]) -> Value {
    let secs = args.first().map(|v| v.as_int()).unwrap_or(0);
    let d = chrono::DateTime::from_timestamp(secs, 0);
    match d {
        Some(dt) => Value::str_(dt.format("%Y-%m-%d").to_string()),
        None => Value::str_("1970-01-01"),
    }
}

/// time.toTimeString(seconds) → String (HH:MM:SS)
fn nat_to_time_string(args: &[Value]) -> Value {
    let secs = args.first().map(|v| v.as_int()).unwrap_or(0);
    let d = chrono::DateTime::from_timestamp(secs, 0);
    match d {
        Some(dt) => Value::str_(dt.format("%H:%M:%S").to_string()),
        None => Value::str_("00:00:00"),
    }
}

/// time.formatDate(seconds, pattern) → String
fn nat_format_date(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let secs = args[0].as_int();
    let pattern = args[1].as_string();
    let d = chrono::DateTime::from_timestamp(secs, 0);
    match d {
        Some(dt) => Value::str_(dt.format(&pattern).to_string()),
        None => Value::str_("1970-01-01T00:00:00Z"),
    }
}

/// time.diff(t1, t2) → Float (seconds between two timestamps)
fn nat_diff(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Float(0.0);
    }
    let t1 = args[0].as_float();
    let t2 = args[1].as_float();
    Value::Float((t2 - t1).abs())
}

/// time.parseDate(text) → Float (timestamp) or error string
fn nat_parse_date(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    // Try parsing ISO 8601
    match chrono::DateTime::parse_from_rfc3339(&text) {
        Ok(dt) => Value::Float(dt.timestamp() as f64),
        Err(_) => {
            // Try date only
            match chrono::NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S") {
                Ok(ndt) => Value::Float(ndt.and_utc().timestamp() as f64),
                Err(_) => match chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
                    Ok(nd) => {
                        Value::Float(nd.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64)
                    }
                    Err(_) => Value::str_(format!("Invalid date: {}", text)),
                },
            }
        }
    }
}
