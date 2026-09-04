# std.assert — API 文档

> 函数数: 1 | [返回目录](index.md)

## 目录

- [assert](#stdassert) — 通用断言，返回 OK/ASSERTION FAILED。

### assert.assert

通用断言，返回 OK/ASSERTION FAILED。

**签名:** `assert.assert(condition: Value, message: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `condition` | `Value` | 断言条件
| `message` | `String` | 消息（可选）

#### 返回值

`String` — "OK: ..." 或 "ASSERTION FAILED: ..."

---

