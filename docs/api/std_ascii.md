# std.ascii — API 文档

> 函数数: 3 | [返回目录](index.md)

## 目录

- [isAlpha](#stdisAlpha) — 判断首字符是否为字母。
- [isDigit](#stdisDigit) — 判断首字符是否为数字。
- [codeAt](#stdcodeAt) — 返回指定位置的字符 Unicode 码点。

### ascii.isAlpha

判断首字符是否为字母。

**签名:** `ascii.isAlpha(text: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 字符串

#### 返回值

`Bool` — 是字母返回 true

#### 示例

```aura
ascii.isAlpha("A") // → true
```

---

### ascii.isDigit

判断首字符是否为数字。

**签名:** `ascii.isDigit(text: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 字符串

#### 返回值

`Bool` — 是数字返回 true

---

### ascii.codeAt

返回指定位置的字符 Unicode 码点。

**签名:** `ascii.codeAt(text: String, index: Int): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 字符串
| `index` | `Int` | 字符位置

#### 返回值

`Int` — 码点值，越界返回 0

#### 示例

```aura
ascii.codeAt("A", 0) // → 65
```

---

