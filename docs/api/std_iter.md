# std.iter — API 文档

> 函数数: 4 | [返回目录](index.md)

## 目录

- [sum](#stdsum) — 返回列表中所有元素的和。
- [avg](#stdavg) — 返回列表中所有元素的平均值。
- [distinct](#stddistinct) — 返回列表的去重结果（保持顺序）。
- [range](#stdrange) — 生成闭区间 [from, to] 的整数列表。

### iter.sum

返回列表中所有元素的和。

**签名:** `iter.sum(list: List): Int / Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `list` | `List` | 源列表

#### 返回值

`Int / Float` — 元素总和

#### 示例

```aura
iter.sum(listOf(1, 2, 3)) // → 6
```

---

### iter.avg

返回列表中所有元素的平均值。

**签名:** `iter.avg(list: List): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `list` | `List` | 源列表

#### 返回值

`Float` — 平均值

---

### iter.distinct

返回列表的去重结果（保持顺序）。

**签名:** `iter.distinct(list: List): List`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `list` | `List` | 源列表

#### 返回值

`List` — 去重后的列表

---

### iter.range

生成闭区间 [from, to] 的整数列表。

**签名:** `iter.range(from: Int, to: Int): List`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `from` | `Int` | 起始值
| `to` | `Int` | 结束值

#### 返回值

`List` — 整数列表

#### 示例

```aura
iter.range(1, 5) // → [1, 2, 3, 4, 5]
```

---

