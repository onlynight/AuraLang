//! std.encoding — 编码/解码工具
//!
//! 提供 Base64、Hex、URL 等编解码功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    // ── 纯逻辑函数（已上移到 Aura 层，但 VM 仍需 native 实现）──
    reg.register("aura.lang.std.Encoding.base64Encode", nat_base64_encode);
    reg.register("aura.lang.std.Encoding.base64Decode", nat_base64_decode);
    reg.register("aura.lang.std.Encoding.hexEncode", nat_hex_encode);
    reg.register("aura.lang.std.Encoding.hexDecode", nat_hex_decode);
    reg.register("aura.lang.std.Encoding.urlEncode", nat_url_encode);
    reg.register("aura.lang.std.Encoding.urlDecode", nat_url_decode);
    reg.register("aura.lang.std.Encoding.byteToHex", nat_byte_to_hex);
    reg.register("aura.lang.std.Encoding.hexToByte", nat_hex_to_byte);
    // ── Phase D.2 P4: 密码学哈希（native，Aura 侧 HMAC 调用）──
    reg.register("aura.lang.std.Encoding.sha256", nat_sha256);
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

// ────────────────────────────────────────────────────────────────────────────
// Phase D.2 P4: SHA256 密码学哈希（Rust 实现，VM 路径）
// ────────────────────────────────────────────────────────────────────────────

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_compress(h: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = ((block[i * 4] as u32) << 24)
            | ((block[i * 4 + 1] as u32) << 16)
            | ((block[i * 4 + 2] as u32) << 8)
            | (block[i * 4 + 3] as u32);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }

    let mut a = h[0];
    let mut b = h[1];
    let mut c = h[2];
    let mut d = h[3];
    let mut e = h[4];
    let mut f = h[5];
    let mut g = h[6];
    let mut hh = h[7];

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 =
            hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

/// 计算 SHA256 哈希，返回 64 字符十六进制字符串
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];

    let orig_len = data.len();
    let bit_len = orig_len as u64 * 8;

    // Padding: append 0x80, then zeros until len ≡ 448 mod 512, then 8 bytes length
    let orig_len_mod = (orig_len + 1) % 64;
    let pad_zeros = if orig_len_mod <= 56 { 56 - orig_len_mod } else { 120 - orig_len_mod };
    let total_len = orig_len + 1 + pad_zeros + 8;
    let mut padded = vec![0u8; total_len];
    padded[..orig_len].copy_from_slice(data);
    padded[orig_len] = 0x80;
    // 写入 64 位长度（大端序）
    let len_bytes = bit_len.to_be_bytes();
    for i in 0..8 {
        padded[total_len - 8 + i] = len_bytes[i];
    }

    // 处理每个 512 位块
    for off in (0..total_len).step_by(64) {
        sha256_compress(&mut h, &padded[off..off + 64]);
    }

    // 输出十六进制
    let mut result = String::with_capacity(64);
    for word in &h {
        result.push_str(&format!("{:08x}", word));
    }
    result
}

/// encoding.sha256(text) → String (64-char hex)
fn nat_sha256(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    Value::str_(sha256_hex(text.as_bytes()))
}
