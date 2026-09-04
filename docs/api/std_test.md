# std.test — API 文档

> 函数数: 2 | [返回目录](index.md)

## 目录

- [assertTrue](#stdassertTrue) — 断言条件为 true，返回 PASS/FAIL 字符串。
- [assertEq](#stdassertEq) — 断言两个值相等。

### test.assertTrue

断言条件为 true，返回 PASS/FAIL 字符串。

**签名:** `test.assertTrue(condition: Value, message: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `condition` | `Value` | 断言条件
| `message` | `String` | 断言消息（可选）

#### 返回值

`String` — "PASS: ..." 或 "FAIL: ..."

#### 示例

```aura
test.assertTrue(true, "all good")
```

---

### test.assertEq

断言两个值相等。

**签名:** `test.assertEq(a: Value, b: Value, message: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `a` | `Value` | 实际值
| `b` | `Value` | 期望值
| `message` | `String` | 消息（可选）

#### 返回值

`String` — "PASS: ..." 或 "FAIL: ..."

#### 示例

```aura
test.assertEq(42, 42, "answer")
```

---

