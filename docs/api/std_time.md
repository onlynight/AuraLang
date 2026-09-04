# std.time — API 文档

> 函数数: 3 | [返回目录](index.md)

## 目录

- [now](#stdnow) — 返回当前 Unix 时间戳（秒）。
- [sleep](#stdsleep) — 暂停执行指定秒数。
- [toDateString](#stdtoDateString) — 将时间戳转换为日期字符串（YYYY-MM-DD）。

### time.now

返回当前 Unix 时间戳（秒）。

**签名:** `time.now: Float`

#### 返回值

`Float` — 自 1970-01-01 以来的秒数

---

### time.sleep

暂停执行指定秒数。

**签名:** `time.sleep(seconds: Float): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `seconds` | `Float` | 暂停秒数

#### 返回值

`Unit` — 无返回值

#### 示例

```aura
time.sleep(1.0)
```

---

### time.toDateString

将时间戳转换为日期字符串（YYYY-MM-DD）。

**签名:** `time.toDateString(timestamp: Int): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `timestamp` | `Int` | Unix 时间戳（秒）

#### 返回值

`String` — 格式化日期

#### 示例

```aura
time.toDateString(0) // → "1970-01-01"
```

---

