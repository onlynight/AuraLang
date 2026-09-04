# std.builtin — API 文档

> 函数数: 2 | [返回目录](index.md)

## 目录

- [typeof](#stdtypeof) — 返回值的类型名称。
- [toString](#stdtoString) — 将任意值转换为字符串。

### builtin.typeof

返回值的类型名称。

**签名:** `builtin.typeof(value: Value): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `value` | `Value` | 待检查的值

#### 返回值

`String` — 类型名（Int/Float/Boolean/String/List/Map/Null）

#### 示例

```aura
builtin.typeof(42) // → "Int"
```

---

### builtin.toString

将任意值转换为字符串。

**签名:** `builtin.toString(value: Value): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `value` | `Value` | 待转换的值

#### 返回值

`String` — 字符串表示

---

