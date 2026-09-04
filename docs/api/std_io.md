# std.io — API 文档

> 函数数: 6 | [返回目录](index.md)

## 目录

- [println](#stdprintln) — 打印一行文本到标准输出，末尾自动追加换行符。
- [print](#stdprint) — 打印文本到标准输出，不追加换行符。
- [readLine](#stdreadLine) — 从标准输入读取一行文本。
- [fileRead](#stdfileRead) — 读取文件全部内容为字符串。
- [fileWrite](#stdfileWrite) — 将字符串写入文件（覆盖模式）。
- [fileExists](#stdfileExists) — 检查文件或目录是否存在。

### io.println

打印一行文本到标准输出，末尾自动追加换行符。

**签名:** `io.println(msg: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `msg` | `String` | 要输出的文本（可多参数拼接）

#### 返回值

`Unit` — 无返回值

#### 示例

```aura
io.println("Hello, Aura!")
```

---

### io.print

打印文本到标准输出，不追加换行符。

**签名:** `io.print(msg: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `msg` | `String` | 要输出的文本

#### 返回值

`Unit` — 无返回值

#### 示例

```aura
io.print("Hello ") // 不换行
```

---

### io.readLine

从标准输入读取一行文本。

**签名:** `io.readLine: String`

#### 返回值

`String` — 读取到的文本（去除尾部换行），无输入时返回 null

#### 示例

```aura
val line = io.readLine()
```

---

### io.fileRead

读取文件全部内容为字符串。

**签名:** `io.fileRead(path: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`String` — 文件内容，读取失败返回错误信息

#### 示例

```aura
val content = io.fileRead("data.txt")
```

---

### io.fileWrite

将字符串写入文件（覆盖模式）。

**签名:** `io.fileWrite(path: String, content: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目标文件路径
| `content` | `String` | 要写入的内容

#### 返回值

`Unit` — 无返回值，失败时返回错误信息

#### 示例

```aura
io.fileWrite("out.txt", "Hello")
```

---

### io.fileExists

检查文件或目录是否存在。

**签名:** `io.fileExists(path: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 路径

#### 返回值

`Bool` — 存在返回 true

---

