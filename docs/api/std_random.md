# std.random — API 文档

> 函数数: 5 | [返回目录](index.md)

## 目录

- [nextInt](#stdnextInt) — 返回 64 位随机整数。
- [nextFloat](#stdnextFloat) — 返回 [0, 1) 区间的随机浮点数。
- [nextIntRange](#stdnextIntRange) — 返回 [min, max) 区间的随机整数。
- [choice](#stdchoice) — 从参数列表中随机选择一个元素。
- [shuffle](#stdshuffle) — 返回列表的随机排列。

### random.nextInt

返回 64 位随机整数。

**签名:** `random.nextInt: Int`

#### 返回值

`Int` — 随机 i64 值

---

### random.nextFloat

返回 [0, 1) 区间的随机浮点数。

**签名:** `random.nextFloat: Float`

#### 返回值

`Float` — 随机 f64 值

---

### random.nextIntRange

返回 [min, max) 区间的随机整数。

**签名:** `random.nextIntRange(min: Int, max: Int): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `min` | `Int` | 下界（含）
| `max` | `Int` | 上界（不含）

#### 返回值

`Int` — 区间内随机整数

#### 示例

```aura
random.nextIntRange(1, 100)
```

---

### random.choice

从参数列表中随机选择一个元素。

**签名:** `random.choice(...: Value): Value`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `...` | `Value` | 候选元素

#### 返回值

`Value` — 随机选中的元素

#### 示例

```aura
random.choice(1, 2, 3)
```

---

### random.shuffle

返回列表的随机排列。

**签名:** `random.shuffle(list: List): List`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `list` | `List` | 源列表

#### 返回值

`List` — 打乱后的列表

---

