# Aura 标准库 API 文档

> 自动生成于 2026-09-04 21:23 | 共 19 个模块，80 个函数

---

## 目录

- [std.ascii](#stdascii) — 3 个函数
- [std.assert](#stdassert) — 1 个函数
- [std.builtin](#stdbuiltin) — 2 个函数
- [std.collections](#stdcollections) — 5 个函数
- [std.console](#stdconsole) — 2 个函数
- [std.encoding](#stdencoding) — 3 个函数
- [std.env](#stdenv) — 3 个函数
- [std.fs](#stdfs) — 7 个函数
- [std.io](#stdio) — 6 个函数
- [std.iter](#stditer) — 4 个函数
- [std.json](#stdjson) — 3 个函数
- [std.math](#stdmath) — 12 个函数
- [std.net](#stdnet) — 2 个函数
- [std.path](#stdpath) — 3 个函数
- [std.process](#stdprocess) — 2 个函数
- [std.random](#stdrandom) — 5 个函数
- [std.string](#stdstring) — 12 个函数
- [std.test](#stdtest) — 2 个函数
- [std.time](#stdtime) — 3 个函数

## std.ascii

> 模块名称: `std.ascii` | 函数数: 3

### ascii.isAlpha

判断首字符是否为字母。

**签名:** `ascii.isAlpha(text: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 字符串

#### 返回值

`Bool` — 是字母返回 true

#### 示例

```aura
ascii.isAlpha("A") // → true
```

---

### ascii.isDigit

判断首字符是否为数字。

**签名:** `ascii.isDigit(text: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 字符串

#### 返回值

`Bool` — 是数字返回 true

---

### ascii.codeAt

返回指定位置的字符 Unicode 码点。

**签名:** `ascii.codeAt(text: String, index: Int): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 字符串
| `index` | `Int` | 字符位置

#### 返回值

`Int` — 码点值，越界返回 0

#### 示例

```aura
ascii.codeAt("A", 0) // → 65
```

---


## std.assert

> 模块名称: `std.assert` | 函数数: 1

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


## std.builtin

> 模块名称: `std.builtin` | 函数数: 2

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


## std.collections

> 模块名称: `std.collections` | 函数数: 5

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


## std.console

> 模块名称: `std.console` | 函数数: 2

### console.red

将文本包装为红色 ANSI 颜色代码。

**签名:** `console.red(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 文本

#### 返回值

`String` — 带颜色代码的字符串

#### 示例

```aura
io.println(console.red("Error!"))
```

---

### console.green

将文本包装为绿色 ANSI 颜色代码。

**签名:** `console.green(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 文本

#### 返回值

`String` — 带颜色代码的字符串

---


## std.encoding

> 模块名称: `std.encoding` | 函数数: 3

### encoding.base64Encode

将字符串编码为 Base64。

**签名:** `encoding.base64Encode(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — Base64 编码字符串

#### 示例

```aura
encoding.base64Encode("Hello")
```

---

### encoding.base64Decode

将 Base64 字符串解码。

**签名:** `encoding.base64Decode(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | Base64 字符串

#### 返回值

`String` — 解码后的字符串，失败返回错误信息

---

### encoding.hexEncode

将字符串编码为十六进制。

**签名:** `encoding.hexEncode(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 十六进制字符串

#### 示例

```aura
encoding.hexEncode("Hi") // → "4869"
```

---


## std.env

> 模块名称: `std.env` | 函数数: 3

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


## std.fs

> 模块名称: `std.fs` | 函数数: 7

### fs.exists

检查路径是否存在。

**签名:** `fs.exists(path: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件/目录路径

#### 返回值

`Bool` — 存在返回 true

---

### fs.readText

读取文件文本内容。

**签名:** `fs.readText(path: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`String` — 文件内容，失败返回错误信息

---

### fs.writeText

将文本写入文件。

**签名:** `fs.writeText(path: String, content: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径
| `content` | `String` | 文本内容

#### 返回值

`Unit` — 无返回值，失败返回错误信息

---

### fs.mkdir

创建目录。

**签名:** `fs.mkdir(path: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目录路径

#### 返回值

`Unit` — 无返回值

---

### fs.mkdirP

递归创建目录（包含所有父目录）。

**签名:** `fs.mkdirP(path: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目录路径

#### 返回值

`Unit` — 无返回值

---

### fs.listDir

列出目录中的所有条目名称。

**签名:** `fs.listDir(path: String): List<String>`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目录路径

#### 返回值

`List<String>` — 条目名称列表

---

### fs.fileSize

返回文件大小（字节）。

**签名:** `fs.fileSize(path: String): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`Int` — 字节数，失败返回 -1

---


## std.io

> 模块名称: `std.io` | 函数数: 6

### io.println

打印一行文本到标准输出，末尾自动追加换行符。

**签名:** `io.println(msg: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `msg` | `String` | 要输出的文本（可多参数拼接）

#### 返回值

`Unit` — 无返回值

#### 示例

```aura
io.println("Hello, Aura!")
```

---

### io.print

打印文本到标准输出，不追加换行符。

**签名:** `io.print(msg: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `msg` | `String` | 要输出的文本

#### 返回值

`Unit` — 无返回值

#### 示例

```aura
io.print("Hello ") // 不换行
```

---

### io.readLine

从标准输入读取一行文本。

**签名:** `io.readLine: String`

#### 返回值

`String` — 读取到的文本（去除尾部换行），无输入时返回 null

#### 示例

```aura
val line = io.readLine()
```

---

### io.fileRead

读取文件全部内容为字符串。

**签名:** `io.fileRead(path: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 文件路径

#### 返回值

`String` — 文件内容，读取失败返回错误信息

#### 示例

```aura
val content = io.fileRead("data.txt")
```

---

### io.fileWrite

将字符串写入文件（覆盖模式）。

**签名:** `io.fileWrite(path: String, content: String): Unit`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 目标文件路径
| `content` | `String` | 要写入的内容

#### 返回值

`Unit` — 无返回值，失败时返回错误信息

#### 示例

```aura
io.fileWrite("out.txt", "Hello")
```

---

### io.fileExists

检查文件或目录是否存在。

**签名:** `io.fileExists(path: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `path` | `String` | 路径

#### 返回值

`Bool` — 存在返回 true

---


## std.iter

> 模块名称: `std.iter` | 函数数: 4

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


## std.json

> 模块名称: `std.json` | 函数数: 3

### json.parse

将 JSON 字符串解析为 Aura 值树。

**签名:** `json.parse(text: String): Value`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | JSON 字符串

#### 返回值

`Value` — 解析后的值（Map/List/Int/Float/Bool/String/Null），失败返回错误信息

#### 示例

```aura
json.parse("{\"name\":\"Aura\"}")
```

---

### json.stringify

将 Aura 值序列化为 JSON 字符串。

**签名:** `json.stringify(value: Value, pretty: Bool): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `value` | `Value` | 要序列化的值
| `pretty` | `Bool` | 是否美化输出（可选）

#### 返回值

`String` — JSON 字符串

#### 示例

```aura
json.stringify({name: "Aura"})
```

---

### json.isValid

检查字符串是否为合法 JSON。

**签名:** `json.isValid(text: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 待检查的字符串

#### 返回值

`Bool` — 合法返回 true

---


## std.math

> 模块名称: `std.math` | 函数数: 12

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


## std.net

> 模块名称: `std.net` | 函数数: 2

### net.getHostname

返回本机主机名。

**签名:** `net.getHostname: String`

#### 返回值

`String` — 主机名字符串

---

### net.getLocalIp

返回本机 IP 地址。

**签名:** `net.getLocalIp: String`

#### 返回值

`String` — IP 地址字符串

---


## std.path

> 模块名称: `std.path` | 函数数: 3

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


## std.process

> 模块名称: `std.process` | 函数数: 2

### process.pid

返回当前进程 ID。

**签名:** `process.pid: Int`

#### 返回值

`Int` — 进程 ID

---

### process.args

返回命令行参数列表。

**签名:** `process.args: List<String>`

#### 返回值

`List<String>` — 参数列表

---


## std.random

> 模块名称: `std.random` | 函数数: 5

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


## std.string

> 模块名称: `std.string` | 函数数: 12

### string.contains

判断字符串是否包含子串。

**签名:** `string.contains(text: String, substr: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `substr` | `String` | 子串

#### 返回值

`Bool` — 包含返回 true

#### 示例

```aura
string.contains("hello", "ell") // → true
```

---

### string.startsWith

判断字符串是否以指定前缀开头。

**签名:** `string.startsWith(text: String, prefix: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `prefix` | `String` | 前缀

#### 返回值

`Bool` — 匹配返回 true

---

### string.endsWith

判断字符串是否以指定后缀结尾。

**签名:** `string.endsWith(text: String, suffix: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `suffix` | `String` | 后缀

#### 返回值

`Bool` — 匹配返回 true

---

### string.split

按分隔符拆分字符串，返回 List。

**签名:** `string.split(text: String, sep: String): List<String>`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `sep` | `String` | 分隔符

#### 返回值

`List<String>` — 拆分后的子串列表

#### 示例

```aura
string.split("a,b,c", ",") // → ["a", "b", "c"]
```

---

### string.join

将空格分隔的文本合并，用指定分隔符连接。

**签名:** `string.join(text: String, sep: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源文本（空格分隔）
| `sep` | `String` | 分隔符

#### 返回值

`String` — 合并后的字符串

---

### string.replace

替换第一个匹配的子串。

**签名:** `string.replace(text: String, target: String, replacement: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `target` | `String` | 要替换的子串
| `replacement` | `String` | 替换文本

#### 返回值

`String` — 替换后的字符串

#### 示例

```aura
string.replace("hello world", "world", "Aura")
```

---

### string.toUpperCase

将所有字母转换为大写。

**签名:** `string.toUpperCase(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 大写字符串

#### 示例

```aura
string.toUpperCase("hello") // → "HELLO"
```

---

### string.toLowerCase

将所有字母转换为小写。

**签名:** `string.toLowerCase(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 小写字符串

#### 示例

```aura
string.toLowerCase("HELLO") // → "hello"
```

---

### string.length

返回字符串的字符数。

**签名:** `string.length(text: String): Int`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`Int` — 字符数量

---

### string.trim

去除首尾空白字符。

**签名:** `string.trim(text: String): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串

#### 返回值

`String` — 修剪后的字符串

#### 示例

```aura
string.trim("  hello  ") // → "hello"
```

---

### string.format

将 `{0}`, `{1}` 等占位符替换为参数值。

**签名:** `string.format(template: String, ...: Value): String`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `template` | `String` | 模板字符串
| `...` | `Value` | 任意数量参数

#### 返回值

`String` — 格式化后的字符串

#### 示例

```aura
string.format("Hello {0}", "Aura") // → "Hello Aura"
```

---

### string.matches

正则表达式匹配。

**签名:** `string.matches(text: String, regex: String): Bool`

#### 参数

| 参数 | 类型 | 描述 |
|------|------|------|
| `text` | `String` | 源字符串
| `regex` | `String` | 正则表达式

#### 返回值

`Bool` — 匹配返回 true

---


## std.test

> 模块名称: `std.test` | 函数数: 2

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


## std.time

> 模块名称: `std.time` | 函数数: 3

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


