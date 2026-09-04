# std.fs — API 文档

> 函数数: 7 | [返回目录](index.md)

## 目录

- [exists](#stdexists) — 检查路径是否存在。
- [readText](#stdreadText) — 读取文件文本内容。
- [writeText](#stdwriteText) — 将文本写入文件。
- [mkdir](#stdmkdir) — 创建目录。
- [mkdirP](#stdmkdirP) — 递归创建目录（包含所有父目录）。
- [listDir](#stdlistDir) — 列出目录中的所有条目名称。
- [fileSize](#stdfileSize) — 返回文件大小（字节）。

### fs.exists

检查路径是否存在。

**签名:** `fs.exists(path: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件/目录路径

#### 返回值

`Bool` — 存在返回 true

---

### fs.readText

读取文件文本内容。

**签名:** `fs.readText(path: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`String` — 文件内容，失败返回错误信息

---

### fs.writeText

将文本写入文件。

**签名:** `fs.writeText(path: String, content: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径
| `content` | `String` | 文本内容

#### 返回值

`Unit` — 无返回值，失败返回错误信息

---

### fs.mkdir

创建目录。

**签名:** `fs.mkdir(path: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目录路径

#### 返回值

`Unit` — 无返回值

---

### fs.mkdirP

递归创建目录（包含所有父目录）。

**签名:** `fs.mkdirP(path: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目录路径

#### 返回值

`Unit` — 无返回值

---

### fs.listDir

列出目录中的所有条目名称。

**签名:** `fs.listDir(path: String): List<String>`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目录路径

#### 返回值

`List<String>` — 条目名称列表

---

### fs.fileSize

返回文件大小（字节）。

**签名:** `fs.fileSize(path: String): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`Int` — 字节数，失败返回 -1

---

