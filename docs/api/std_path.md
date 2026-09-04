# std.path — API 文档

> 函数数: 3 | [返回目录](index.md)

## 目录

- [join](#stdjoin) — 拼接多个路径组件。
- [basename](#stdbasename) — 返回文件名（不含扩展名）。
- [extname](#stdextname) — 返回文件扩展名（含点）。

### path.join

拼接多个路径组件。

**签名:** `path.join(...: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `...` | `String` | 路径组件

#### 返回值

`String` — 拼接后的路径

#### 示例

```aura
path.join("dir", "file.txt")
```

---

### path.basename

返回文件名（不含扩展名）。

**签名:** `path.basename(path: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`String` — 文件基名

#### 示例

```aura
path.basename("dir/file.txt") // → "file"
```

---

### path.extname

返回文件扩展名（含点）。

**签名:** `path.extname(path: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`String` — 扩展名

#### 示例

```aura
path.extname("file.txt") // → ".txt"
```

---

