//! std.io — 标准输入输出
//!
//! 提供 `println`、`print`、`readLine`、文件读写等基础 I/O 功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use std::io::{BufRead, Read, Write};

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.IO.println", nat_println);
    reg.register("aura.lang.std.IO.print", nat_print);
    reg.register("aura.lang.std.IO.readLine", nat_readline);
    reg.register("aura.lang.std.IO.readAll", nat_readall);
    reg.register("aura.lang.std.IO.flush", nat_flush);
    reg.register("aura.lang.std.IO.fileRead", nat_file_read);
    reg.register("aura.lang.std.IO.fileWrite", nat_file_write);
    reg.register("aura.lang.std.IO.fileExists", nat_file_exists);
    reg.register("aura.lang.std.IO.writeFile", nat_write_file);
    reg.register("aura.lang.std.IO.readFile", nat_read_file);
}

fn nat_println(args: &[Value]) -> Value {
    let s = join_args(args);
    println!("{}", s);
    Value::Null
}

fn nat_print(args: &[Value]) -> Value {
    let s = join_args(args);
    print!("{}", s);
    let _ = std::io::stdout().flush();
    Value::Null
}

fn nat_readline(_args: &[Value]) -> Value {
    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(n) if n > 0 => Value::str_(line.trim_end()),
        _ => Value::Null,
    }
}

fn nat_readall(_args: &[Value]) -> Value {
    let stdin = std::io::stdin();
    let mut s = String::new();
    match stdin.lock().read_to_string(&mut s) {
        Ok(_) => Value::str_(s.trim_end()),
        Err(_) => Value::Null,
    }
}

fn nat_flush(_args: &[Value]) -> Value {
    let _ = std::io::stdout().flush();
    Value::Null
}

fn nat_file_read(args: &[Value]) -> Value {
    let path = args.first().map(|v| v.as_string()).unwrap_or_default();
    match std::fs::read_to_string(&path) {
        Ok(s) => Value::str_(s),
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_file_write(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let path = args[0].as_string();
    let content = args[1].as_string();
    match std::fs::write(&path, content) {
        Ok(_) => Value::Null,
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_write_file(args: &[Value]) -> Value {
    nat_file_write(args)
}

fn nat_read_file(args: &[Value]) -> Value {
    nat_file_read(args)
}

fn nat_file_exists(args: &[Value]) -> Value {
    let path = args.first().map(|v| v.as_string()).unwrap_or_default();
    Value::Bool(std::path::Path::new(&path).exists())
}

fn join_args(args: &[Value]) -> String {
    let mut s = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&a.to_string());
    }
    s
}
