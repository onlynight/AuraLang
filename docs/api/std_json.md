# std.json — API 文档

> 函数数: 3 | [返回目录](index.md)

## 目录

- [parse](#stdparse) — 将 JSON 字符串解析为 Aura 值树。
- [stringify](#stdstringify) — 将 Aura 值序列化为 JSON 字符串。
- [isValid](#stdisValid) — 检查字符串是否为合法 JSON。

### json.parse

将 JSON 字符串解析为 Aura 值树。

**签名:** `json.parse(text: String): Value`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | JSON 字符串

#### 返回值

`Value` — 解析后的值（Map/List/Int/Float/Bool/String/Null），失败返回错误信息

#### 示例

```aura
json.parse("{\"name\":\"Aura\"}")
```

---

### json.stringify

将 Aura 值序列化为 JSON 字符串。

**签名:** `json.stringify(value: Value, pretty: Bool): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `value` | `Value` | 要序列化的值
| `pretty` | `Bool` | 是否美化输出（可选）

#### 返回值

`String` — JSON 字符串

#### 示例

```aura
json.stringify({name: "Aura"})
```

---

### json.isValid

检查字符串是否为合法 JSON。

**签名:** `json.isValid(text: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 待检查的字符串

#### 返回值

`Bool` — 合法返回 true

---

