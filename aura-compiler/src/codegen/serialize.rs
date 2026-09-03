//! `.auc` 字节码文件格式：序列化 / 反序列化（对应 P4.12）
//!
//! 文件布局（小端）：
//! ```text
//! "AURA"        魔数 (4 字节)
//! u16           版本 (当前 = 1)
//! u32           常量池数量
//!   每个常量:   u8 标签 + 载荷
//! u16           原生函数数量
//!   每个原生:   u16 名称长度 + 名称(UTF-8) + u16 参数数量
//! u16           函数数量
//!   每个函数:   u16 名称长度 + 名称 + u16 参数数 + u16 局部数 + u8 是否原生 + u32 代码长度 + 代码
//! u16           入口函数索引
//! ```
//!
//! 常量标签：0=Int(i64) 1=Float(f64) 2=Str(u32 len + bytes) 3=Bool(u8) 4=Null

use crate::codegen::opcode::{BytecodeFunction, BytecodeModule, BytecodeNative, Const};
use std::fs;

pub const MAGIC: &[u8; 4] = b"AURA";
pub const VERSION: u16 = 1;

#[derive(Debug)]
pub enum SerializeError {
    Io(String),
    Format(String),
}

impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializeError::Io(m) => write!(f, "io error: {}", m),
            SerializeError::Format(m) => write!(f, "format error: {}", m),
        }
    }
}

impl From<std::io::Error> for SerializeError {
    fn from(e: std::io::Error) -> Self {
        SerializeError::Io(e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 序列化
// ─────────────────────────────────────────────────────────────────────────────

/// 序列化为字节
pub fn to_bytes(module: &BytecodeModule) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());

    // 常量池
    buf.extend_from_slice(&(module.consts.len() as u32).to_le_bytes());
    for c in &module.consts {
        write_const(&mut buf, c);
    }

    // 原生函数
    buf.extend_from_slice(&(module.natives.len() as u16).to_le_bytes());
    for n in &module.natives {
        write_str(&mut buf, &n.name);
        buf.extend_from_slice(&n.param_count.to_le_bytes());
    }

    // 函数
    buf.extend_from_slice(&(module.functions.len() as u16).to_le_bytes());
    for f in &module.functions {
        write_str(&mut buf, &f.name);
        buf.extend_from_slice(&f.param_count.to_le_bytes());
        buf.extend_from_slice(&f.locals.to_le_bytes());
        buf.push(if f.is_native { 1 } else { 0 });
        buf.extend_from_slice(&(f.code.len() as u32).to_le_bytes());
        buf.extend_from_slice(&f.code);
    }

    // 入口
    buf.extend_from_slice(&module.entry.to_le_bytes());
    buf
}

/// 写入 `.auc` 文件
pub fn write_auc(path: &str, module: &BytecodeModule) -> Result<(), SerializeError> {
    let bytes = to_bytes(module);
    fs::write(path, bytes).map_err(|e| SerializeError::Io(e.to_string()))?;
    Ok(())
}

fn write_const(buf: &mut Vec<u8>, c: &Const) {
    match c {
        Const::Int(i) => {
            buf.push(0);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        Const::Float(fl) => {
            buf.push(1);
            buf.extend_from_slice(&fl.to_le_bytes());
        }
        Const::Str(s) => {
            buf.push(2);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        Const::Bool(b) => {
            buf.push(3);
            buf.push(if *b { 1 } else { 0 });
        }
        Const::Null => {
            buf.push(4);
        }
    }
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u16).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

// ─────────────────────────────────────────────────────────────────────────────
// 反序列化
// ─────────────────────────────────────────────────────────────────────────────

/// 字节读取器（小端）
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SerializeError> {
        if self.pos + n > self.bytes.len() {
            return Err(SerializeError::Format("unexpected EOF".into()));
        }
        let s = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, SerializeError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, SerializeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, SerializeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i64(&mut self) -> Result<i64, SerializeError> {
        let b = self.take(8)?;
        Ok(i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn f64(&mut self) -> Result<f64, SerializeError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
    }

    fn str(&mut self) -> Result<String, SerializeError> {
        let len = self.u16()? as usize;
        let b = self.take(len)?;
        Ok(String::from_utf8_lossy(b).into_owned())
    }
}

/// 从字节反序列化
pub fn from_bytes(bytes: &[u8]) -> Result<BytecodeModule, SerializeError> {
    let mut r = Reader::new(bytes);

    let magic = r.take(4)?;
    if magic != MAGIC {
        return Err(SerializeError::Format("bad magic".into()));
    }
    let _version = r.u16()?;

    // 常量池
    let nconsts = r.u32()? as usize;
    let mut consts = Vec::with_capacity(nconsts);
    for _ in 0..nconsts {
        let tag = r.u8()?;
        let c = match tag {
            0 => Const::Int(r.i64()?),
            1 => Const::Float(r.f64()?),
            2 => {
                let len = r.u32()? as usize;
                let b = r.take(len)?;
                Const::Str(String::from_utf8_lossy(b).into_owned())
            }
            3 => Const::Bool(r.u8()? != 0),
            4 => Const::Null,
            _ => return Err(SerializeError::Format("bad const tag".into())),
        };
        consts.push(c);
    }

    // 原生函数
    let nnatives = r.u16()? as usize;
    let mut natives = Vec::with_capacity(nnatives);
    for _ in 0..nnatives {
        let name = r.str()?;
        let pc = r.u16()?;
        natives.push(BytecodeNative {
            name,
            param_count: pc,
        });
    }

    // 函数
    let nfuncs = r.u16()? as usize;
    let mut functions = Vec::with_capacity(nfuncs);
    for _ in 0..nfuncs {
        let name = r.str()?;
        let pc = r.u16()?;
        let locals = r.u16()?;
        let is_native = r.u8()? != 0;
        let code_len = r.u32()? as usize;
        let code = r.take(code_len)?.to_vec();
        functions.push(BytecodeFunction {
            name,
            param_count: pc,
            locals,
            is_native,
            code,
        });
    }

    let entry = r.u16()?;

    Ok(BytecodeModule {
        consts,
        natives,
        functions,
        entry,
    })
}

/// 读取 `.auc` 文件
pub fn read_auc(path: &str) -> Result<BytecodeModule, SerializeError> {
    let bytes = fs::read(path).map_err(|e| SerializeError::Io(e.to_string()))?;
    from_bytes(&bytes)
}
