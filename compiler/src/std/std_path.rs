//! std.path — 路径操作
//!
//! 提供路径拼接、分割、扩展名等路径工具。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.path.join", nat_join);
    reg.register("aura.path.dirname", nat_dirname);
    reg.register("aura.path.basename", nat_basename);
    reg.register("aura.path.extname", nat_extname);
    reg.register("aura.path.relative", nat_relative);
    reg.register("aura.path.resolve", nat_resolve);
    reg.register("aura.path.normalize", nat_normalize);
    reg.register("aura.path.isAbsolute", nat_is_absolute);
    reg.register("aura.path.isRelative", nat_is_relative);
    reg.register("aura.path.split", nat_split);
    reg.register("aura.path.separators", nat_separators);
    reg.register("aura.path.fromUnix", nat_from_unix);
    reg.register("aura.path.fromWindows", nat_from_windows);
}

fn s0(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}

fn s1(args: &[Value]) -> String {
    args.get(1).map(|v| v.as_string()).unwrap_or_default()
}

/// path.join(p1, p2, ...) → String
fn nat_join(args: &[Value]) -> Value {
    let parts: Vec<String> = args.iter().map(|v| v.as_string()).collect();
    let joined = std::path::PathBuf::from_iter(parts.iter());
    Value::str_(joined.to_string_lossy())
}

/// path.dirname(path) → String (directory part)
fn nat_dirname(args: &[Value]) -> Value {
    let path_str = s0(args);
    let p = std::path::Path::new(&path_str);
    match p.parent() {
        Some(parent) => Value::str_(parent.to_string_lossy()),
        None => Value::str_("."),
    }
}

/// path.basename(path) → String (filename without extension)
fn nat_basename(args: &[Value]) -> Value {
    let path_str = s0(args);
    let p = std::path::Path::new(&path_str);
    match p.file_stem() {
        Some(basename) => Value::str_(basename.to_string_lossy()),
        None => Value::str_(""),
    }
}

/// path.extname(path) → String (extension with dot)
fn nat_extname(args: &[Value]) -> Value {
    let path_str = s0(args);
    let p = std::path::Path::new(&path_str);
    match p.extension() {
        Some(ext) => Value::str_(format!(".{}", ext.to_string_lossy())),
        None => Value::str_(""),
    }
}

/// path.relative(from, to) → String (relative path)
fn nat_relative(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::str_("");
    }
    let from = std::path::PathBuf::from(&s0(args));
    let to = std::path::PathBuf::from(&s1(args));
    let rel = to.strip_prefix(&from).unwrap_or(&to);
    Value::str_(rel.to_string_lossy())
}

/// path.resolve(path) → String (absolute path)
fn nat_resolve(args: &[Value]) -> Value {
    let path_str = s0(args);
    let p = std::path::PathBuf::from(&path_str);
    match std::fs::canonicalize(&p) {
        Ok(abs) => Value::str_(abs.to_string_lossy()),
        Err(_) => Value::str_(p.to_string_lossy()),
    }
}

/// path.normalize(path) → String (normalized path)
fn nat_normalize(args: &[Value]) -> Value {
    let path_str = s0(args);
    let p = std::path::PathBuf::from(&path_str);
    // Simple normalization: resolve . and .. components
    let mut parts: Vec<std::path::Component> = Vec::new();
    let mut root_prefix: Option<std::path::Component> = None;
    for component in p.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                if root_prefix.is_none() {
                    root_prefix = Some(component);
                }
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                parts.pop();
            }
            _ => {
                parts.push(component);
            }
        }
    }
    let mut result =
        root_prefix.map(|p| p.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
    for part in parts {
        if !result.is_empty() && !result.ends_with(std::path::MAIN_SEPARATOR) {
            result.push(std::path::MAIN_SEPARATOR);
        }
        result.push_str(part.as_os_str().to_string_lossy().as_ref());
    }
    if result.is_empty() {
        result = ".".to_string();
    }
    Value::str_(result)
}

/// path.isAbsolute(path) → Bool
fn nat_is_absolute(args: &[Value]) -> Value {
    let path_str = s0(args);
    Value::Bool(std::path::Path::new(&path_str).is_absolute())
}

/// path.isRelative(path) → Bool
fn nat_is_relative(args: &[Value]) -> Value {
    let path_str = s0(args);
    Value::Bool(!std::path::Path::new(&path_str).is_absolute())
}

/// path.split(path) → List of path components
fn nat_split(args: &[Value]) -> Value {
    let path_str = s0(args);
    let p = std::path::Path::new(&path_str);
    let parts: Vec<Value> =
        p.components().map(|c| Value::str_(c.as_os_str().to_string_lossy())).collect();
    Value::List(parts)
}

/// path.separators(path) → Int (count of path separators)
fn nat_separators(args: &[Value]) -> Value {
    let s = s0(args);
    Value::Int(s.matches(std::path::MAIN_SEPARATOR).count() as i64)
}

/// path.fromUnix(unixPath) → String (Unix-style path)
fn nat_from_unix(args: &[Value]) -> Value {
    Value::str_(s0(args).replace('\\', "/"))
}

/// path.fromWindows(windowsPath) → String (Windows-style path)
fn nat_from_windows(args: &[Value]) -> Value {
    Value::str_(s0(args).replace('/', "\\"))
}
