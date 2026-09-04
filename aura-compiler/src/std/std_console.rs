//! std.console — 终端控制
//!
//! 提供清屏、着色、光标控制、尺寸查询等终端操作。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("console.clear", nat_clear);
    reg.register("console.cursorUp", nat_cursor_up);
    reg.register("console.cursorDown", nat_cursor_down);
    reg.register("console.cursorLeft", nat_cursor_left);
    reg.register("console.cursorRight", nat_cursor_right);
    reg.register("console.cursorShow", nat_cursor_show);
    reg.register("console.cursorHide", nat_cursor_hide);
    reg.register("console.reset", nat_reset);
    reg.register("console.red", nat_red);
    reg.register("console.green", nat_green);
    reg.register("console.yellow", nat_yellow);
    reg.register("console.blue", nat_blue);
    reg.register("console.magenta", nat_magenta);
    reg.register("console.cyan", nat_cyan);
    reg.register("console.white", nat_white);
    reg.register("console.bold", nat_bold);
    reg.register("console.italic", nat_italic);
    reg.register("console.underline", nat_underline);
    reg.register("console.dim", nat_dim);
    reg.register("console.inverse", nat_inverse);
    reg.register("console.size", nat_size);
    reg.register("console.width", nat_width);
    reg.register("console.height", nat_height);
}

fn arg0_str(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}

/// console.clear() → Unit
fn nat_clear(_args: &[Value]) -> Value {
    print!("\x1b[2J\x1b[H");
    Value::Null
}

/// console.cursorUp(n) → Unit
fn nat_cursor_up(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1) as u16;
    print!("\x1b[{}A", n);
    Value::Null
}

/// console.cursorDown(n) → Unit
fn nat_cursor_down(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1) as u16;
    print!("\x1b[{}B", n);
    Value::Null
}

/// console.cursorLeft(n) → Unit
fn nat_cursor_left(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1) as u16;
    print!("\x1b[{}D", n);
    Value::Null
}

/// console.cursorRight(n) → Unit
fn nat_cursor_right(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1) as u16;
    print!("\x1b[{}C", n);
    Value::Null
}

/// console.cursorShow() → Unit
fn nat_cursor_show(_args: &[Value]) -> Value {
    print!("\x1b[?25h");
    Value::Null
}

/// console.cursorHide() → Unit
fn nat_cursor_hide(_args: &[Value]) -> Value {
    print!("\x1b[?25l");
    Value::Null
}

/// console.reset() → Unit
fn nat_reset(_args: &[Value]) -> Value {
    print!("\x1b[0m");
    Value::Null
}

fn color_wrapper(code: &str) -> impl Fn(&[Value]) -> Value {
    move |args: &[Value]| {
        let text = arg0_str(args);
        Value::str_(format!("\x1b[{}m{}\x1b[0m", code, text))
    }
}

/// console.red(text) → String (red text)
fn nat_red(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[31m{}\x1b[0m", text))
}

/// console.green(text) → String
fn nat_green(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[32m{}\x1b[0m", text))
}

/// console.yellow(text) → String
fn nat_yellow(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[33m{}\x1b[0m", text))
}

/// console.blue(text) → String
fn nat_blue(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[34m{}\x1b[0m", text))
}

/// console.magenta(text) → String
fn nat_magenta(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[35m{}\x1b[0m", text))
}

/// console.cyan(text) → String
fn nat_cyan(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[36m{}\x1b[0m", text))
}

/// console.white(text) → String
fn nat_white(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[37m{}\x1b[0m", text))
}

/// console.bold(text) → String
fn nat_bold(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[1m{}\x1b[22m", text))
}

/// console.italic(text) → String
fn nat_italic(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[3m{}\x1b[23m", text))
}

/// console.underline(text) → String
fn nat_underline(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[4m{}\x1b[24m", text))
}

/// console.dim(text) → String
fn nat_dim(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[2m{}\x1b[22m", text))
}

/// console.inverse(text) → String
fn nat_inverse(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[7m{}\x1b[27m", text))
}

/// console.size() → Map { width, height }
fn nat_size(_args: &[Value]) -> Value {
    let (w, h) = terminal_size();
    let mut map = std::collections::HashMap::new();
    map.insert(Value::str_("width"), Value::Int(w as i64));
    map.insert(Value::str_("height"), Value::Int(h as i64));
    Value::Map(map)
}

/// console.width() → Int
fn nat_width(_args: &[Value]) -> Value {
    let (w, _) = terminal_size();
    Value::Int(w as i64)
}

/// console.height() → Int
fn nat_height(_args: &[Value]) -> Value {
    let (_, h) = terminal_size();
    Value::Int(h as i64)
}

/// 获取终端尺寸
fn terminal_size() -> (usize, usize) {
    #[cfg(windows)]
    {
        (120, 30)
    }
    #[cfg(unix)]
    {
        // Try to read from ioctl
        unsafe {
            use std::io::{self, Read};
            let mut termios = std::mem::zeroed();
            if libc::ioctl(io::stdin().as_raw_fd(), 0x5413 /* TIOCGWINSZ */, &mut termios) == 0 {
                (termios.ws_col as usize, termios.ws_row as usize)
            } else {
                (120, 30)
            }
        }
    }
}
