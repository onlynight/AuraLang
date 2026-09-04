# std.env — API 文档

> 函数数: 3 | [返回目录](index.md)

## 目录

- [get](#stdget) — 获取环境变量值。
- [has](#stdhas) — 检查环境变量是否存在。
- [platform](#stdplatform) — 返回当前操作系统名称。

### env.get

获取环境变量值。

**签名:** `env.get(name: String, default: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `name` | `String` | 变量名
| `default` | `String` | 不存在时的默认值（可选）

#### 返回值

`String` — 变量值或默认值

---

### env.has

检查环境变量是否存在。

**签名:** `env.has(name: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `name` | `String` | 变量名

#### 返回值

`Bool` — 存在返回 true

---

### env.platform

返回当前操作系统名称。

**签名:** `env.platform: String`

#### 返回值

`String` — "windows" / "linux" / "macos"

---

