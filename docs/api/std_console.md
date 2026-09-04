# std.console — API 文档

> 函数数: 2 | [返回目录](index.md)

## 目录

- [red](#stdred) — 将文本包装为红色 ANSI 颜色代码。
- [green](#stdgreen) — 将文本包装为绿色 ANSI 颜色代码。

### console.red

将文本包装为红色 ANSI 颜色代码。

**签名:** `console.red(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 文本

#### 返回值

`String` — 带颜色代码的字符串

#### 示例

```aura
io.println(console.red("Error!"))
```

---

### console.green

将文本包装为绿色 ANSI 颜色代码。

**签名:** `console.green(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 文本

#### 返回值

`String` — 带颜色代码的字符串

---

