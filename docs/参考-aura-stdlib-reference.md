# Aura 标准库参考 (Stdlib Reference) — v2.0

> **数据来源**：`docs/api/index.md`（自动生成的官方 API 文档，2026-09-04）
> **19 个模块，80 个函数**

---

## 1. std.ascii — 字符判断 (3 函数)

```aura
ascii.isAlpha(text: String): Bool      // 首字符是否为字母
ascii.isDigit(text: String): Bool      // 首字符是否为数字
ascii.codeAt(text: String, index: Int): Int  // 位置字符的 Unicode 码点
```

## 2. std.assert — 断言 (1 函数)

```aura
assert.assert(condition: Value, message: String): String  // "OK: ..." 或 "ASSERTION FAILED: ..."
```

## 3. std.builtin — 内置 (2 函数)

```aura
builtin.typeof(value: Value): String   // 返回类型名: Int/Float/Boolean/String/List/Map/Null
builtin.toString(value: Value): String // 任意值转字符串
```

## 4. std.collections — 集合构造 (5 函数)

```aura
collections.listOf(...: Value): List              // 不可变列表
collections.mapOf(...: Value): Map                // 键值映射（交替参数）
collections.setOf(...: Value): List               // 集合（自动去重）
collections.emptyList: List                        // 空列表
collections.listContains(list: List, item: Value): Bool
```

## 5. std.console — 终端颜色 (2 函数)

```aura
console.red(text: String): String     // 红色 ANSI
console.green(text: String): String   // 绿色 ANSI
```

## 6. std.encoding — 编解码 (3 函数)

```aura
encoding.base64Encode(text: String): String
encoding.base64Decode(text: String): String
encoding.hexEncode(text: String): String  // "Hi" → "4869"
```

## 7. std.env — 环境变量 (3 函数)

```aura
env.get(name: String, default: String): String
env.has(name: String): Bool
env.platform: String  // "windows" / "linux" / "macos"
```

## 8. std.fs — 文件系统 (7 函数)

```aura
fs.exists(path: String): Bool
fs.readText(path: String): String
fs.writeText(path: String, content: String): Unit
fs.mkdir(path: String): Unit
fs.mkdirP(path: String): Unit          // 递归创建
fs.listDir(path: String): List<String>
fs.fileSize(path: String): Int         // 字节数，失败返回 -1
```

## 9. std.io — IO 操作 (6 函数)

```aura
io.println(msg: String): Unit          // 换行输出
io.print(msg: String): Unit            // 不换行
io.readLine: String                    // 读取一行
io.fileRead(path: String): String      // 读取文件
io.fileWrite(path: String, content: String): Unit
io.fileExists(path: String): Bool
```

## 10. std.iter — 迭代器 (4 函数)

```aura
iter.sum(list: List): Int / Float
iter.avg(list: List): Float
iter.distinct(list: List): List
iter.range(from: Int, to: Int): List   // 闭区间 [from, to]
```

## 11. std.json — JSON (3 函数)

```aura
json.parse(text: String): Value
json.stringify(value: Value, pretty: Bool): String
json.isValid(text: String): Bool
```

## 12. std.math — 数学 (12 函数)

```aura
math.abs(x: Int / Float): Int / Float
math.min(a: Int, b: Int): Int
math.max(a: Int, b: Int): Int
math.sqrt(x: Float): Float
math.pow(base: Float, exp: Float): Float
math.PI: Float           // 圆周率
math.E: Float            // 自然常数
math.sin(angle: Float): Float
math.cos(angle: Float): Float
math.log(x: Float): Float
math.ceil(x: Float): Float
math.floor(x: Float): Float
```

## 13. std.net — 网络 (2 函数)

```aura
net.getHostname: String
net.getLocalIp: String
```

## 14. std.path — 路径 (3 函数)

```aura
path.join(...: String): String
path.basename(path: String): String   // "dir/file.txt" → "file"
path.extname(path: String): String    // "file.txt" → ".txt"
```

## 15. std.process — 进程 (2 函数)

```aura
process.pid: Int
process.args: List<String>
```

## 16. std.random — 随机 (5 函数)

```aura
random.nextInt: Int
random.nextFloat: Float
random.nextIntRange(min: Int, max: Int): Int  // [min, max)
random.choice(...: Value): Value
random.shuffle(list: List): List
```

## 17. std.string — 字符串 (12 函数)

```aura
string.contains(text: String, substr: String): Bool
string.startsWith(text: String, prefix: String): Bool
string.endsWith(text: String, suffix: String): Bool
string.split(text: String, sep: String): List<String>
string.join(text: String, sep: String): String
string.replace(text: String, target: String, replacement: String): String
string.toUpperCase(text: String): String
string.toLowerCase(text: String): String
string.length(text: String): Int
string.trim(text: String): String
string.format(template: String, ...: Value): String  // "{0}" 占位符
string.matches(text: String, regex: String): Bool     // 正则匹配
```

## 18. std.test — 测试 (2 函数)

```aura
test.assertTrue(condition: Value, message: String): String
test.assertEq(a: Value, b: Value, message: String): String
```

## 19. std.time — 时间 (3 函数)

```aura
time.now: Float                    // Unix 时间戳（秒）
time.sleep(seconds: Float): Unit   // 暂停
time.toDateString(timestamp: Int): String  // "1970-01-01"
```

---

## Prelude（全局可用，无需 import）

```
println, print, puts, abs, sqrt, pow, toInt, toFloat, toStr,
toString, clock, strlen, CString, CStr, ptrIsNull, ptrToInt,
intToPtr, makeCallback
```

---

## 并发 API（compiler 实现，非 std 模块）

```aura
// 在 compiler/src/vm/ 中实现
concurrent.spawnActor(T::class): T
concurrent.send(actor, msg)
concurrent.ask(actor, msg)
concurrent.newChannel<T>(buffer: Int): Channel<T>
concurrent.channelSend(ch, value)
concurrent.channelRecv(ch): T
concurrent.channelClose(ch)
concurrent.spawn(block: () -> T): Future<T>
concurrent.supervise(actor)
```
