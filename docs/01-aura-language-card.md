# Aura Language Card — DSH 系统提示词版 (v2.0)

> **用途**：注入 DSH 系统提示词，让大模型直接理解 Aura 语言语法并生成正确代码。
> **Token 预算**：约 3000 tokens。
> **数据来源**：编译器源码 (`compiler/src/`)、语言教程 (`book/`)、标准库 API 文档 (`docs/api/index.md`)、TextMate 语法定义、36 个实际示例文件。

---

## 1. 语言身份

Aura 是 **NovaOS 的系统级脚本语言**，Kotlin 风格语法，Rust 实现，支持 AOT + JIT 混合编译、零成本 FFI、ARC 内存管理。
与 Kotlin 相似：`val`/`var`、`fun`、`when`、泛型、空安全（`?`/`!!`/`?:`）、`suspend` 协程。
与 Kotlin 不同：**`value class`**（值类型）而非 `data class`（引用类型）、`actor` 并发实体、`extern "c"` FFI、`Result<T,E>` 错误处理。`struct` 是 `value class` 的别名（deprecated）。

**文件扩展名**：`.aura` | **编译器**：`cargo build --release` → `aura` CLI

---

## 2. 语法速查表

### 2.1 变量与类型
```aura
val immutable = 42                   // 只读，类型推断
var count: Int = 0                   // 可变
val lazyVal by lazy { compute() }     // 懒加载
lateinit var cache: String            // 延迟初始化

// 类型
val list: List<Int> = listOf(1, 2, 3)
val map: Map<String, Int> = mapOf("a" to 1)
val opt: Int? = null                  // 可空
typealias Vec2 = Pair<Int, Int>       // 类型别名

// 空安全
val safe: Int = n ?: 0                // Elvis
val len = p.tag?.length               // 安全调用
val forced: Int = n!!                 // 强制解包
```

### 2.2 函数
```aura
fun add(a: Int, b: Int): Int = a + b               // 单表达式
fun power(base: Int, exp: Int = 2): Int { }        // 默认参数
fun join(vararg parts: String): String { }         // 可变参数
fun <T: Number> first(items: List<T>): T? { }      // 泛型 + 约束
suspend fun fetchData(): Int = 0                     // 协程
fun main() { println("Hello!") }                    // 入口
```

### 2.3 数据结构
```aura
// 值类型（两种形式）
value class Point(val x: Int, val y: Int) {
    fun manhattan(): Int = x + y
}
value data class Player(val id: Int, var name: String = "unknown", var health: Int = 100)
// struct 是 value class 的别名（deprecated）：struct Point(val x: Int, val y: Int)

// 引用类型（继承、多态）
class Circle : Drawable { override fun draw() {} }
sealed class Shape { fun area(): Float = 0.0f }
class Dog : Animal() { override fun name(): String = "dog" }

// 接口
interface Drawable { fun draw(): Unit }

// 枚举（可带数据）
enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

// Actor（并发实体）
actor Scheduler {
    var tick: Int = 0
    fun step() { tick += 1 }
}
```

### 2.4 控制流
```aura
val status = if (hp > 0) "alive" else "dead"

val band = when (score) {
    in 90..100 -> "A"
    in 80..89  -> "B"
    else       -> "F"
}

val len = when (val) {
    is String -> val.length
    is Int    -> 1
    else      -> 0
}

for (i in 0..10) { ... }
while (cond) { ... }
do { ... } while (cond)
break@outer / continue@outer

try { ... } catch (e: Exception) { ... }
```

### 2.5 字符串与字面量
```aura
// 插值
println("Hello, $name!")
println("Level: ${hp * 2}")

// 原始字符串（无插值）
val raw = """No $interpolation or \n here"""

// 数字
val hex = 0xFF
val binary = 0b1100
val float = 3.14f
val long = 1_000_000L
```

### 2.6 Lambda 与方法引用
```aura
val f = { x: Int -> x + 1 }
listOf(1,2,3).map { x -> x * 2 }
val fn = obj::method
```

---

## 3. 完整关键字表

| 类别 | 关键字 |
|------|--------|
| **前缀修饰符** | `abstract` `final` `enum` `open` `annotation` `sealed` `data` `override` `lateinit` `private` `protected` `public` `internal` `inner` `noinline` `crossinline` `vararg` `reified` `tailrec` `operator` `infix` `inline` `external` `const` `suspend` `comptime` `value` `defer` `extern` `lazy` `box` `weak` `async` |
| **后置修饰符** | `where` `by` `get` `set` |
| **软关键字** | `catch` `finally` `field` `else` `then` `unit` |
| **硬关键字** | `as` `is` `in` `to` `it` |
| **控制关键字** | `if` `while` `do` `when` `try` `throw` `break` `continue` `return` `for` `select` `await` |

---

## 4. 标准库 (19 模块，80 函数)

### 4.1 模块概览

| 模块 | 函数数 | 功能 |
|------|--------|------|
| `std.ascii` | 3 | 字符判断 |
| `std.assert` | 1 | 断言 |
| `std.builtin` | 2 | typeof, toString |
| `std.collections` | 5 | list, map, set 构造 |
| `std.console` | 2 | 终端颜色 |
| `std.encoding` | 3 | base64, hex 编解码 |
| `std.env` | 3 | 环境变量 |
| `std.fs` | 7 | 文件系统 |
| `std.io` | 6 | IO 操作 |
| `std.iter` | 4 | 迭代器 (sum/avg/distinct/range) |
| `std.json` | 3 | JSON 解析 |
| `std.math` | 12 | 数学函数 |
| `std.net` | 2 | 网络信息 |
| `std.path` | 3 | 路径操作 |
| `std.process` | 2 | 进程信息 |
| `std.random` | 5 | 随机数 |
| `std.string` | 12 | 字符串操作 |
| `std.test` | 2 | 测试断言 |
| `std.time` | 3 | 时间 |

### 4.2 常用函数签名

```aura
// IO
import aura.io.*
import aura.fs.*
import aura.math.*
import aura.string.*
import aura.collections.*
import aura.json.*
import aura.concurrent.*
import aura.time.*
import aura.path.*
import aura.random.*
import aura.encoding.*
import aura.test.*

// Prelude (no import needed — call directly)
println("Hello")
println("Hello ")
val line = println()
val content = aura.io.fileRead("data.txt")
aura.io.fileWrite("out.txt", "Hello")

// File operations
aura.fs.exists("path")
val text = aura.fs.readText("file.txt")
aura.fs.writeText("file.txt", "content")
aura.fs.mkdir("dir")
aura.fs.mkdirP("nested/dir")
val files = aura.fs.listDir("dir")
val size = aura.fs.fileSize("file.txt")

// Math
aura.math.abs(-42)          // → 42
aura.math.sqrt(16.0)        // → 4.0
aura.math.pow(2.0, 10.0)    // → 1024.0
aura.math.PI / aura.math.E  // constants
aura.math.sin(x) / aura.math.cos(x) / aura.math.log(x)
aura.math.ceil(1.2) / aura.math.floor(1.8)
aura.math.min(3, 5) / aura.math.max(3, 5)

// String methods (call on object)
val s = "hello"
s.toUpperCase()          // "HELLO"
s.toLowerCase()          // "hello"
s.length()               // character count
s.trim()                 // remove whitespace
s.split(" ").join(", ")  // split and rejoin
s.contains("ell")
s.startsWith("he")
s.endsWith("lo")
s.replace("he", "H")
s.format("Hello {0}", "Aura")
s.matches("a.c")

// Collections (prelude — no import needed)
listOf(1, 2, 3)
mapOf("k", "v")
setOf(1, 2, 1, 3)

// JSON
import aura.json.*
val data = aura.json.parse("""{"name":"Aura"}""")
val out = aura.json.stringify(data)
aura.json.isValid("...")

// Concurrency
import aura.concurrent.*
val actor = aura.concurrent.spawnActor("Worker")
aura.concurrent.send(actor, msg)
val ch = aura.concurrent.newChannel(0)
aura.concurrent.channelSend(ch, val)
aura.concurrent.channelRecv(ch)
aura.concurrent.channelClose(ch)
aura.concurrent.spawn(fn)

// Time
import aura.time.*
aura.time.now                    // Unix timestamp (seconds)
aura.time.sleep(1.0)            // pause
aura.time.toDateString(timestamp)

// Path
import aura.path.*
aura.path.join("dir", "file.txt")
aura.path.basename("dir/file.txt")  // → "file"
aura.path.extname("file.txt")        // → ".txt"

// Random
import aura.random.*
aura.random.nextInt
aura.random.nextFloat
aura.random.nextIntRange(1, 100)
aura.random.choice(1, 2, 3)
aura.random.shuffle(list)

// Encoding
import aura.encoding.*
aura.encoding.base64Encode("Hello")
aura.encoding.base64Decode("SGVsbG8=")
aura.encoding.hexEncode("Hi")      // → "4869"

// Testing
import aura.test.*
aura.test.assertTrue(cond, "msg")
aura.test.assertEq(a, b, "msg")
```

### 4.3 Prelude（无需 import）
```
println, print, puts, abs, sqrt, pow, toInt, toFloat, toStr,
toString, clock, strlen, CString, CStr, ptrIsNull, ptrToInt,
intToPtr, makeCallback
```

---

## 5. 并发模型

### 5.1 Actor
```aura
actor Server {
    private var running: Boolean = true
    
    fun start() { println("Running") }
    fun handle(msg: String) { println("Got: $msg") }
}

// 创建并启动
val server = concurrent.spawnActor(Server::class)
concurrent.send(server, "hello")
```

### 5.2 Channel
```aura
val ch = concurrent.newChannel<Int>(0)
concurrent.spawn { concurrent.channelSend(ch, 42) }
val val = concurrent.channelRecv(ch)
concurrent.channelClose(ch)
```

### 5.3 协程
```aura
suspend fun fetch(): Int = 42
val result = concurrent.spawn(fetch())  // spawn 启动协程
```

### 5.4 select 多路复用
```aura
select {
    case msg = channel -> { handle(msg) }
    case timeout       -> { fallback() }
}
```

---

## 6. FFI (C 互操作)

```aura
// 形式 1：行内声明
extern "c" fun puts(msg: String): Int
extern "c" fun strlen(s: CString): Int

// 形式 2：块级声明
extern "c" "raylib" {
    fun DrawCircle(x: Int, y: Int, radius: Float, color: Color)
    fun GetFrameTime(): Float
    val WHITE: Color
}

// 形式 3：带库名
extern "c" "libc" {
    fun strlen(s: CString): Int
    fun sqrt(x: Float): Float
    fun clock(): Long
}
```

---

## 7. 代码风格

| 元素 | 约定 | 示例 |
|------|------|------|
| 常量/枚举 | UPPER_CASE | `GAME_WIDTH`, `RED` |
| 类/接口/结构体 | PascalCase | `Player`, `Drawable` |
| 函数/变量 | camelCase | `loadConfig`, `playerName` |
| 泛型参数 | 单大写字母 | `T`, `U`, `K`, `V` |
| 包/模块 | 小写点分 | `std.io`, `std.math` |

**格式化**：4 空格缩进、运算符前后空格、Kotlin 风格花括号

---

## 8. ⚠️ 常见陷阱（LLM 易错点）

| 陷阱 | ✅ 正确 | ❌ 错误 |
|------|---------|---------|
| 数据结构体 | `value class Name(...)` | `data class Name(...)` |
| 并发实体 | `actor Name { }` | `class Name { }` |
| FFI 声明 | `extern "c" fun f()` | `extern fun f()` |
| FFI 块 | `extern "c" "lib" { }` | `extern("c") { }` |
| Result | `Result<T, E>` 双参数 | `Result<T>` 单参数 |
| await | `await fetch()` | `await(fetch())` |
| 结构体构造 | `Player(1, "Alice")` | `Player(id=1, name="Alice")` |
| select | `select { case ch -> ... }` | `select(ch) { }` |
| 字符串插值 | `"${expr}"` | `${expr}` (无引号) |
| 原始字符串 | `"""..."""` 三引号 | 不支持 `r"..."` |

---

## 9. 完整示例

```aura
import aura.io.*
import aura.concurrent.*
import aura.fs.*

value data class Config(val port: Int = 8080, var debug: Boolean = false)

actor Server(config: Config) {
    private var running: Boolean = false
    
    fun start() {
        io.println("Server starting on port ${config.port}")
        running = true
    }
    
    fun stop() { running = false }
}

fun main() {
    val config = Config(port = 3000)
    val server = concurrent.spawnActor(Server::class)
    io.println("Server ready")
}
```
