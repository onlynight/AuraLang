//! std.fs — 文件系统操作
//!
//! 提供文件与目录的增删改查、遍历、属性查询等功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use std::path::PathBuf;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.fs.exists", nat_exists);
    reg.register("aura.fs.isFile", nat_is_file);
    reg.register("aura.fs.isDirectory", nat_is_directory);
    reg.register("aura.fs.readText", nat_read_text);
    reg.register("aura.fs.writeText", nat_write_text);
    reg.register("aura.fs.readBytes", nat_read_bytes);
    reg.register("aura.fs.writeBytes", nat_write_bytes);
    reg.register("aura.fs.delete", nat_delete);
    reg.register("aura.fs.mkdir", nat_mkdir);
    reg.register("aura.fs.mkdirP", nat_mkdir_p);
    reg.register("aura.fs.rename", nat_rename);
    reg.register("aura.fs.copy", nat_copy);
    reg.register("aura.fs.listDir", nat_list_dir);
    reg.register("aura.fs.listFiles", nat_list_files);
    reg.register("aura.fs.fileSize", nat_file_size);
    reg.register("aura.fs.lastModified", nat_last_modified);
    reg.register("aura.fs.absolutePath", nat_absolute_path);
    reg.register("aura.fs.homeDir", nat_home_dir);
    reg.register("aura.fs.tempDir", nat_temp_dir);
    reg.register("aura.fs.currentDir", nat_current_dir);
    reg.register("aura.fs.walk", nat_walk);
}

fn arg0(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}

fn arg1(args: &[Value]) -> String {
    args.get(1).map(|v| v.as_string()).unwrap_or_default()
}

fn nat_exists(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).exists())
}

fn nat_is_file(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).is_file())
}

fn nat_is_directory(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).is_dir())
}

fn nat_read_text(args: &[Value]) -> Value {
    match std::fs::read_to_string(&arg0(args)) {
        Ok(s) => Value::str_(s),
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_write_text(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    match std::fs::write(&arg0(args), arg1(args).as_str()) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_read_bytes(args: &[Value]) -> Value {
    match std::fs::read(&arg0(args)) {
        Ok(bytes) => Value::List(bytes.into_iter().map(|b: u8| Value::Int(b as i64)).collect()),
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_write_bytes(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let bytes: Vec<u8> = match &args[1] {
        Value::List(items) => items.iter().map(|v| v.as_int() as u8).collect(),
        _ => Vec::new(),
    };
    match std::fs::write(&arg0(args), bytes) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_delete(args: &[Value]) -> Value {
    let path = PathBuf::from(arg0(args));
    let result =
        if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
    match result {
        Ok(_) => Value::Bool(true),
        Err(e) => Value::str_(format!("Delete error: {}", e)),
    }
}

fn nat_mkdir(args: &[Value]) -> Value {
    match std::fs::create_dir(&arg0(args)) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("mkdir error: {}", e)),
    }
}

fn nat_mkdir_p(args: &[Value]) -> Value {
    match std::fs::create_dir_all(&arg0(args)) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("mkdir error: {}", e)),
    }
}

fn nat_rename(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    match std::fs::rename(&arg0(args), &arg1(args)) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("rename error: {}", e)),
    }
}

fn nat_copy(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    match std::fs::copy(&arg0(args), &arg1(args)) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("copy error: {}", e)),
    }
}

fn nat_list_dir(args: &[Value]) -> Value {
    let mut results = Vec::new();
    match std::fs::read_dir(&arg0(args)) {
        Ok(entries) => {
            for entry in entries.flatten() {
                results.push(Value::str_(entry.file_name().to_string_lossy()));
            }
        }
        Err(e) => {
            results.push(Value::str_(format!("error: {}", e)));
        }
    }
    Value::List(results)
}

fn nat_list_files(args: &[Value]) -> Value {
    let mut results = Vec::new();
    match std::fs::read_dir(&arg0(args)) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    results.push(Value::str_(path.to_string_lossy()));
                }
            }
        }
        Err(e) => {
            results.push(Value::str_(format!("error: {}", e)));
        }
    }
    Value::List(results)
}

fn nat_file_size(args: &[Value]) -> Value {
    match std::fs::metadata(&arg0(args)) {
        Ok(m) => Value::Int(m.len() as i64),
        Err(_) => Value::Int(-1),
    }
}

fn nat_last_modified(args: &[Value]) -> Value {
    match std::fs::metadata(&arg0(args)) {
        Ok(m) => match m.modified() {
            Ok(time) => match time.duration_since(std::time::UNIX_EPOCH) {
                Ok(d) => Value::Float(d.as_secs_f64()),
                Err(_) => Value::Float(0.0),
            },
            Err(_) => Value::Float(0.0),
        },
        Err(_) => Value::Float(0.0),
    }
}

fn nat_absolute_path(args: &[Value]) -> Value {
    let path = PathBuf::from(arg0(args));
    match std::fs::canonicalize(&path) {
        Ok(p) => Value::str_(p.to_string_lossy()),
        Err(_) => Value::str_(path.to_string_lossy()),
    }
}

fn nat_home_dir(_args: &[Value]) -> Value {
    match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(h) => Value::str_(h),
        Err(_) => Value::str_("."),
    }
}

fn nat_temp_dir(_args: &[Value]) -> Value {
    Value::str_(std::env::temp_dir().to_string_lossy())
}

fn nat_current_dir(_args: &[Value]) -> Value {
    match std::env::current_dir() {
        Ok(p) => Value::str_(p.to_string_lossy()),
        Err(_) => Value::str_("."),
    }
}

/// fs.walk(root, maxDepth) → List of file paths
fn nat_walk(args: &[Value]) -> Value {
    let root = arg0(args);
    let max_depth = args.get(1).map(|v| v.as_int() as usize).unwrap_or(10);
    let mut results = Vec::new();
    walk_dir(&PathBuf::from(&root), 0, max_depth, &mut results);
    Value::List(results)
}

fn walk_dir(path: &PathBuf, depth: usize, max_depth: usize, results: &mut Vec<Value>) {
    if depth > max_depth {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            results.push(Value::str_(p.to_string_lossy()));
            if p.is_dir() {
                walk_dir(&p, depth + 1, max_depth, results);
            }
        }
    }
}
