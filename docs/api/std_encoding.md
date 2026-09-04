# std.encoding — API 文档

> 函数数: 3 | [返回目录](index.md)

## 目录

- [base64Encode](#stdbase64Encode) — 将字符串编码为 Base64。
- [base64Decode](#stdbase64Decode) — 将 Base64 字符串解码。
- [hexEncode](#stdhexEncode) — 将字符串编码为十六进制。

### encoding.base64Encode

将字符串编码为 Base64。

**签名:** `encoding.base64Encode(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — Base64 编码字符串

#### 示例

```aura
encoding.base64Encode("Hello")
```

---

### encoding.base64Decode

将 Base64 字符串解码。

**签名:** `encoding.base64Decode(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | Base64 字符串

#### 返回值

`String` — 解码后的字符串，失败返回错误信息

---

### encoding.hexEncode

将字符串编码为十六进制。

**签名:** `encoding.hexEncode(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 十六进制字符串

#### 示例

```aura
encoding.hexEncode("Hi") // → "4869"
```

---

