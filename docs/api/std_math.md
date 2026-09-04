# std.math — API 文档

> 函数数: 12 | [返回目录](index.md)

## 目录

- [abs](#stdabs) — 返回数值的绝对值。
- [min](#stdmin) — 返回两个整数中的较小值。
- [max](#stdmax) — 返回两个整数中的较大值。
- [sqrt](#stdsqrt) — 返回数的平方根。
- [pow](#stdpow) — 返回 base^exp（幂运算）。
- [PI](#stdPI) — 圆周率 π ≈ 3.141592653589793。
- [E](#stdE) — 自然对数的底 e ≈ 2.718281828459045。
- [sin](#stdsin) — 返回角度的正弦值（弧度）。
- [cos](#stdcos) — 返回角度的余弦值（弧度）。
- [log](#stdlog) — 返回自然对数（以 e 为底）。
- [ceil](#stdceil) — 向上取整。
- [floor](#stdfloor) — 向下取整。

### math.abs

返回数值的绝对值。

**签名:** `math.abs(x: Int / Float): Int / Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `x` | `Int / Float` | 输入值

#### 返回值

`Int / Float` — 绝对值

#### 示例

```aura
math.abs(-42) // → 42
```

---

### math.min

返回两个整数中的较小值。

**签名:** `math.min(a: Int, b: Int): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `a` | `Int` | 第一个数
| `b` | `Int` | 第二个数

#### 返回值

`Int` — 最小值

#### 示例

```aura
math.min(3, 5) // → 3
```

---

### math.max

返回两个整数中的较大值。

**签名:** `math.max(a: Int, b: Int): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `a` | `Int` | 第一个数
| `b` | `Int` | 第二个数

#### 返回值

`Int` — 最大值

#### 示例

```aura
math.max(3, 5) // → 5
```

---

### math.sqrt

返回数的平方根。

**签名:** `math.sqrt(x: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `x` | `Float` | 输入值（≥0）

#### 返回值

`Float` — 平方根

#### 示例

```aura
math.sqrt(16.0) // → 4.0
```

---

### math.pow

返回 base^exp（幂运算）。

**签名:** `math.pow(base: Float, exp: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `base` | `Float` | 底数
| `exp` | `Float` | 指数

#### 返回值

`Float` — 幂运算结果

#### 示例

```aura
math.pow(2.0, 10.0) // → 1024.0
```

---

### math.PI

圆周率 π ≈ 3.141592653589793。

**签名:** `math.PI: Float`

#### 返回值

`Float` — π 的精确值

---

### math.E

自然对数的底 e ≈ 2.718281828459045。

**签名:** `math.E: Float`

#### 返回值

`Float` — e 的精确值

---

### math.sin

返回角度的正弦值（弧度）。

**签名:** `math.sin(angle: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `angle` | `Float` | 弧度值

#### 返回值

`Float` — sin(angle)

---

### math.cos

返回角度的余弦值（弧度）。

**签名:** `math.cos(angle: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `angle` | `Float` | 弧度值

#### 返回值

`Float` — cos(angle)

---

### math.log

返回自然对数（以 e 为底）。

**签名:** `math.log(x: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `x` | `Float` | 输入值（>0）

#### 返回值

`Float` — ln(x)

---

### math.ceil

向上取整。

**签名:** `math.ceil(x: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `x` | `Float` | 输入值

#### 返回值

`Float` — ≥ x 的最小整数

#### 示例

```aura
math.ceil(1.2) // → 2.0
```

---

### math.floor

向下取整。

**签名:** `math.floor(x: Float): Float`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `x` | `Float` | 输入值

#### 返回值

`Float` — ≤ x 的最大整数

#### 示例

```aura
math.floor(1.8) // → 1.0
```

---

