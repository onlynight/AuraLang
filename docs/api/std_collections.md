# std.collections — API 文档

> 函数数: 5 | [返回目录](index.md)

## 目录

- [listOf](#stdlistOf) — 构造不可变列表。
- [mapOf](#stdmapOf) — 构造键值映射（参数成对出现）。
- [setOf](#stdsetOf) — 构造集合（自动去重）。
- [emptyList](#stdemptyList) — 构造空列表。
- [listContains](#stdlistContains) — 检查列表是否包含指定元素。

### collections.listOf

构造不可变列表。

**签名:** `collections.listOf(...: Value): List`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `...` | `Value` | 任意数量元素

#### 返回值

`List` — 包含所有参数的列表

#### 示例

```aura
val nums = collections.listOf(1, 2, 3)
```

---

### collections.mapOf

构造键值映射（参数成对出现）。

**签名:** `collections.mapOf(...: Value): Map`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `...` | `Value` | 键值对（交替出现）

#### 返回值

`Map` — 包含所有键值对的映射

#### 示例

```aura
val m = collections.mapOf("name", "Aura", "v", 1)
```

---

### collections.setOf

构造集合（自动去重）。

**签名:** `collections.setOf(...: Value): List`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `...` | `Value` | 任意数量元素

#### 返回值

`List` — 去重后的元素列表

#### 示例

```aura
val s = collections.setOf(1, 2, 1, 3) // → [1, 2, 3]
```

---

### collections.emptyList

构造空列表。

**签名:** `collections.emptyList: List`

#### 返回值

`List` — 空列表

---

### collections.listContains

检查列表是否包含指定元素。

**签名:** `collections.listContains(list: List, item: Value): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `list` | `List` | 源列表
| `item` | `Value` | 要查找的元素

#### 返回值

`Bool` — 包含返回 true

---

