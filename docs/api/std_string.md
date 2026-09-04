# std.string — API 文档

> 函数数: 12 | [返回目录](index.md)

## 目录

- [contains](#stdcontains) — 判断字符串是否包含子串。
- [startsWith](#stdstartsWith) — 判断字符串是否以指定前缀开头。
- [endsWith](#stdendsWith) — 判断字符串是否以指定后缀结尾。
- [split](#stdsplit) — 按分隔符拆分字符串，返回 List。
- [join](#stdjoin) — 将空格分隔的文本合并，用指定分隔符连接。
- [replace](#stdreplace) — 替换第一个匹配的子串。
- [toUpperCase](#stdtoUpperCase) — 将所有字母转换为大写。
- [toLowerCase](#stdtoLowerCase) — 将所有字母转换为小写。
- [length](#stdlength) — 返回字符串的字符数。
- [trim](#stdtrim) — 去除首尾空白字符。
- [format](#stdformat) — 将 `{0}`, `{1}` 等占位符替换为参数值。
- [matches](#stdmatches) — 正则表达式匹配。

### string.contains

判断字符串是否包含子串。

**签名:** `string.contains(text: String, substr: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `substr` | `String` | 子串

#### 返回值

`Bool` — 包含返回 true

#### 示例

```aura
string.contains("hello", "ell") // → true
```

---

### string.startsWith

判断字符串是否以指定前缀开头。

**签名:** `string.startsWith(text: String, prefix: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `prefix` | `String` | 前缀

#### 返回值

`Bool` — 匹配返回 true

---

### string.endsWith

判断字符串是否以指定后缀结尾。

**签名:** `string.endsWith(text: String, suffix: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `suffix` | `String` | 后缀

#### 返回值

`Bool` — 匹配返回 true

---

### string.split

按分隔符拆分字符串，返回 List。

**签名:** `string.split(text: String, sep: String): List<String>`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `sep` | `String` | 分隔符

#### 返回值

`List<String>` — 拆分后的子串列表

#### 示例

```aura
string.split("a,b,c", ",") // → ["a", "b", "c"]
```

---

### string.join

将空格分隔的文本合并，用指定分隔符连接。

**签名:** `string.join(text: String, sep: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源文本（空格分隔）
| `sep` | `String` | 分隔符

#### 返回值

`String` — 合并后的字符串

---

### string.replace

替换第一个匹配的子串。

**签名:** `string.replace(text: String, target: String, replacement: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `target` | `String` | 要替换的子串
| `replacement` | `String` | 替换文本

#### 返回值

`String` — 替换后的字符串

#### 示例

```aura
string.replace("hello world", "world", "Aura")
```

---

### string.toUpperCase

将所有字母转换为大写。

**签名:** `string.toUpperCase(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 大写字符串

#### 示例

```aura
string.toUpperCase("hello") // → "HELLO"
```

---

### string.toLowerCase

将所有字母转换为小写。

**签名:** `string.toLowerCase(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 小写字符串

#### 示例

```aura
string.toLowerCase("HELLO") // → "hello"
```

---

### string.length

返回字符串的字符数。

**签名:** `string.length(text: String): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`Int` — 字符数量

---

### string.trim

去除首尾空白字符。

**签名:** `string.trim(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 修剪后的字符串

#### 示例

```aura
string.trim("  hello  ") // → "hello"
```

---

### string.format

将 `{0}`, `{1}` 等占位符替换为参数值。

**签名:** `string.format(template: String, ...: Value): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `template` | `String` | 模板字符串
| `...` | `Value` | 任意数量参数

#### 返回值

`String` — 格式化后的字符串

#### 示例

```aura
string.format("Hello {0}", "Aura") // → "Hello Aura"
```

---

### string.matches

正则表达式匹配。

**签名:** `string.matches(text: String, regex: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `regex` | `String` | 正则表达式

#### 返回值

`Bool` — 匹配返回 true

---

