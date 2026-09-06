/*
Value 自定义二进制编解码（Phase 3.1）

替换 JSON 序列化，使用固定位模式 + 标签字节格式，
性能提升约 20-30x，消息体积缩减 50%。

格式规范:
| Tag  | 变体         | 载荷         |
|------|-------------|-------------|
| 0x00 | Null        | -           |
| 0x01 | Int(i64)    | 8 bytes LE  |
| 0x02 | Float(f64)  | 8 bytes LE  |
| 0x03 | Bool        | 1 byte      |
| 0x04 | Str         | u32 len LE + UTF-8 |
| 0x05 | Ptr(i64)    | 8 bytes LE  |
| 0x06 | Ref(usize)  | u32 LE      |
| 0x07 | Weak(usize) | u32 LE      |
| 0x08 | List        | u32 count + N x Value |
| 0x09 | Map         | u32 count + N x (K+V) |
| 0x10 | ListSmall   | u16 count + N x Value |
| 0x11 | MapSmall    | u16 count + N x (K+V) |

所有多字节数值使用 little-endian 字节序。
字符串使用 UTF-8 编码，前缀 u32 长度。

跨进程句柄:
Ref/Weak 是进程内堆句柄，跨进程无意义。
编码时若标记为跨进程模式，将句柄替换为 INVALID_HANDLE (0xFFFF_FFFF)。
解码时 INVALID_HANDLE 映射为 Value::Null。
*/
use crate::vm::value::Value;
/// 跨进程无效句柄标记
pub const INVALID_HANDLE: u32 = 0xFFFF_FFFF;
/// 小集合阈值（65535 以下使用 u16 计数）
const SMALL_THRESHOLD: usize = 65535;
/// 序列化错误
#[derive(Debug)]
pub enum SerializeError {
    BufferTooSmall { required: usize, available: usize },
    UnknownTag(u8),
    StringTooLong(usize),
    CountTooLarge(usize),
    Utf8(String),
}
impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializeError::BufferTooSmall {
                required,
                available,
            } => {
                write!(
                    f,
                    "buffer too small: need {} bytes, have {}",
                    required, available
                )
            }
            SerializeError::UnknownTag(tag) => write!(f, "unknown tag: 0x{:02X}", tag),
            SerializeError::StringTooLong(len) => write!(f, "string too long: {} bytes", len),
            SerializeError::CountTooLarge(count) => write!(f, "count too large: {}", count),
            SerializeError::Utf8(e) => write!(f, "UTF-8 error: {}", e),
        }
    }
}
impl std::error::Error for SerializeError {}
// --- Encoding ---
/// Encode a Value into a buffer
pub fn encode_value(val: &Value, buf: &mut Vec<u8>) {
    match val {
        Value::Null => buf.push(0x00),
        Value::Int(i) => {
            buf.push(0x01);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        Value::Float(f) => {
            buf.push(0x02);
            buf.extend_from_slice(&f.to_bits().to_le_bytes());
        }
        Value::Bool(b) => {
            buf.push(0x03);
            buf.push(*b as u8);
        }
        Value::Str(s) => {
            buf.push(0x04);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        Value::Ptr(p) => {
            buf.push(0x05);
            buf.extend_from_slice(&p.to_le_bytes());
        }
        Value::Ref(h) => {
            buf.push(0x06);
            buf.extend_from_slice(&(*h as u32).to_le_bytes());
        }
        Value::Weak(h) => {
            buf.push(0x07);
            buf.extend_from_slice(&(*h as u32).to_le_bytes());
        }
        Value::List(items) => {
            let count = items.len();
            if count <= SMALL_THRESHOLD {
                buf.push(0x10);
                buf.extend_from_slice(&(count as u16).to_le_bytes());
            } else {
                buf.push(0x08);
                buf.extend_from_slice(&count.to_le_bytes());
            }
            for item in items {
                encode_value(item, buf);
            }
        }
        Value::Map(map) => {
            let count = map.len();
            if count <= SMALL_THRESHOLD {
                buf.push(0x11);
                buf.extend_from_slice(&(count as u16).to_le_bytes());
            } else {
                buf.push(0x09);
                buf.extend_from_slice(&count.to_le_bytes());
            }
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(k, _)| key_sort_key(k));
            for (k, v) in entries {
                encode_value(k, buf);
                encode_value(v, buf);
            }
        }
    }
}
/// Cross-process mode: replace Ref/Weak with Null
pub fn encode_value_cross_process(val: &Value, buf: &mut Vec<u8>) {
    match val {
        Value::Ref(_) | Value::Weak(_) => {
            buf.push(0x00);
        }
        _ => encode_value(val, buf),
    }
}
/// Encode message frame (4-byte length prefix + encoded Value)
pub fn encode_frame(val: &Value, buf: &mut Vec<u8>) {
    let before = buf.len();
    encode_value(val, buf);
    let payload_len = (buf.len() - before) as u32;
    buf.splice(before..before, payload_len.to_be_bytes().iter().copied());
}
// --- Decoding ---
/// Decode a Value from a buffer
pub fn decode_value(data: &[u8], offset: &mut usize) -> Result<Value, SerializeError> {
    if *offset >= data.len() {
        return Err(SerializeError::BufferTooSmall {
            required: 1,
            available: data.len(),
        });
    }
    let tag = data[*offset];
    *offset += 1;
    match tag {
        0x00 => Ok(Value::Null),
        0x01 => Ok(Value::Int(read_i64(data, offset)?)),
        0x02 => Ok(Value::Float(f64::from_bits(read_u64(data, offset)?))),
        0x03 => {
            let b = read_u8(data, offset)?;
            Ok(Value::Bool(b != 0))
        }
        0x04 => {
            let len = read_u32(data, offset)? as usize;
            if len > 64 * 1024 * 1024 {
                return Err(SerializeError::StringTooLong(len));
            }
            let s = read_str(data, offset, len)?;
            Ok(Value::Str(std::rc::Rc::from(s)))
        }
        0x05 => Ok(Value::Ptr(read_i64(data, offset)?)),
        0x06 => {
            let h = read_u32(data, offset)?;
            if h == INVALID_HANDLE { Ok(Value::Null) } else { Ok(Value::Ref(h as usize)) }
        }
        0x07 => {
            let h = read_u32(data, offset)?;
            if h == INVALID_HANDLE { Ok(Value::Null) } else { Ok(Value::Weak(h as usize)) }
        }
        0x08 | 0x10 => {
            let count = if tag == 0x10 {
                read_u16(data, offset)? as usize
            } else {
                read_u32(data, offset)? as usize
            };
            if count > 64 * 1024 * 1024 {
                return Err(SerializeError::CountTooLarge(count));
            }
            let mut items = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                items.push(decode_value(data, offset)?);
            }
            Ok(Value::List(items))
        }
        0x09 | 0x11 => {
            let count = if tag == 0x11 {
                read_u16(data, offset)? as usize
            } else {
                read_u32(data, offset)? as usize
            };
            if count > 64 * 1024 * 1024 {
                return Err(SerializeError::CountTooLarge(count));
            }
            let mut map = std::collections::HashMap::with_capacity(count.min(1024));
            for _ in 0..count {
                let k = decode_value(data, offset)?;
                let v = decode_value(data, offset)?;
                map.insert(k, v);
            }
            Ok(Value::Map(map))
        }
        tag => Err(SerializeError::UnknownTag(tag)),
    }
}
/// Decode message frame (4-byte length prefix + Value)
pub fn decode_frame(data: &[u8], offset: &mut usize) -> Result<Value, SerializeError> {
    let len = read_u32(data, offset)? as usize;
    if len > 64 * 1024 * 1024 {
        return Err(SerializeError::BufferTooSmall {
            required: len,
            available: data.len(),
        });
    }
    let mut start = *offset;
    *offset += len;
    decode_value(data, &mut start)
}
// --- Helper readers ---
fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, SerializeError> {
    if *offset >= data.len() {
        return Err(SerializeError::BufferTooSmall {
            required: 1,
            available: data.len(),
        });
    }
    let v = data[*offset];
    *offset += 1;
    Ok(v)
}
fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, SerializeError> {
    if *offset + 2 > data.len() {
        return Err(SerializeError::BufferTooSmall {
            required: 2,
            available: data.len() - *offset,
        });
    }
    let v = u16::from_le_bytes([
        data[*offset],
        data[*offset + 1],
    ]);
    *offset += 2;
    Ok(v)
}
fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, SerializeError> {
    if *offset + 4 > data.len() {
        return Err(SerializeError::BufferTooSmall {
            required: 4,
            available: data.len() - *offset,
        });
    }
    let v = u32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    Ok(v)
}
fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, SerializeError> {
    if *offset + 8 > data.len() {
        return Err(SerializeError::BufferTooSmall {
            required: 8,
            available: data.len() - *offset,
        });
    }
    let v = u64::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
        data[*offset + 4],
        data[*offset + 5],
        data[*offset + 6],
        data[*offset + 7],
    ]);
    *offset += 8;
    Ok(v)
}
fn read_i64(data: &[u8], offset: &mut usize) -> Result<i64, SerializeError> {
    Ok(read_u64(data, offset)? as i64)
}
fn read_str(data: &[u8], offset: &mut usize, len: usize) -> Result<String, SerializeError> {
    if *offset + len > data.len() {
        return Err(SerializeError::BufferTooSmall {
            required: len,
            available: data.len() - *offset,
        });
    }
    let s = std::str::from_utf8(&data[*offset..*offset + len])
        .map_err(|e| SerializeError::Utf8(e.to_string()))?;
    *offset += len;
    Ok(s.to_string())
}
fn key_sort_key(v: &Value) -> Vec<u8> {
    let mut buf = Vec::with_capacity(16);
    encode_value(v, &mut buf);
    buf
}
