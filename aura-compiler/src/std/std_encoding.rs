//! std.encoding — 编码/解码工具
//!
//! 提供 Base64、Hex、URL 等编解码功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.encoding.base64Encode", nat_base64_encode);
    reg.register("aura.encoding.base64Decode", nat_base64_decode);
    reg.register("aura.encoding.hexEncode", nat_hex_encode);
    reg.register("aura.encoding.hexDecode", nat_hex_decode);
    reg.register("aura.encoding.urlEncode", nat_url_encode);
    reg.register("aura.encoding.urlDecode", nat_url_decode);
    reg.register("aura.encoding.byteToHex", nat_byte_to_hex);
    reg.register("aura.encoding.hexToByte", nat_hex_to_byte);
}

/// encoding.base64Encode(data) → String
fn nat_base64_encode(args: &[Value]) -> Value {
    let data = args.first().map(|v| v.as_string()).unwrap_or_default();
    let encoded = base64::encode(data.as_bytes());
    Value::str_(encoded)
}

/// encoding.base64Decode(text) → String
fn nat_base64_decode(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    match base64::decode(text.as_bytes()) {
        Ok(bytes) => Value::str_(String::from_utf8_lossy(&bytes).to_string()),
        Err(e) => Value::str_(format!("Base64 decode error: {}", e)),
    }
}

/// encoding.hexEncode(data) → String
fn nat_hex_encode(args: &[Value]) -> Value {
    let data = args.first().map(|v| v.as_string()).unwrap_or_default();
    Value::str_(hex::encode(data.as_bytes()))
}

/// encoding.hexDecode(hex) → String
fn nat_hex_decode(args: &[Value]) -> Value {
    let hex = args.first().map(|v| v.as_string()).unwrap_or_default();
    match hex::decode(&hex) {
        Ok(bytes) => Value::str_(String::from_utf8_lossy(&bytes).to_string()),
        Err(e) => Value::str_(format!("Hex decode error: {}", e)),
    }
}

/// encoding.urlEncode(text) → String
fn nat_url_encode(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    let encoded: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect();
    Value::str_(encoded)
}

/// encoding.urlDecode(text) → String
fn nat_url_decode(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    let decoded = decode_url(&text);
    Value::str_(decoded)
}

/// encoding.byteToHex(byte) → String (2-char hex)
fn nat_byte_to_hex(args: &[Value]) -> Value {
    let b = args.first().map(|v| v.as_int() as u8).unwrap_or(0);
    Value::str_(format!("{:02x}", b))
}

/// encoding.hexToByte(hex) → Int
fn nat_hex_to_byte(args: &[Value]) -> Value {
    let hex = args.first().map(|v| v.as_string()).unwrap_or_default();
    match u8::from_str_radix(&hex, 16) {
        Ok(b) => Value::Int(b as i64),
        Err(_) => Value::Int(0),
    }
}

fn decode_url(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex_str = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
            if let Ok(b) = u8::from_str_radix(hex_str, 16) {
                result.push(b);
                i += 3;
            } else {
                result.push(b'%');
                i += 1;
            }
        } else if bytes[i] == b'+' {
            result.push(b' ');
            i += 1;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&result).to_string()
}
