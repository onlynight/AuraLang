---
name: aura-lang
version: 2.0.0
lastModified: 2026-06-18
changes: v2.0.0 — 全面更新：添加 P8-P10 并发/协程/select、Phase D FFI AOT/native、完整语法参考（class/actor/interface/object/try-catch/import/++/-- 等已恢复）
description: Use when writing, reviewing, or debugging Aura programming language code (.aura files). Aura is a system-level scripting language with Kotlin-style syntax. This skill provides complete syntax reference, feature status, and code verification workflow.
---

# Aura Language — Complete Reference (v2.0)

Aura is a **system-level scripting language for NovaOS**. Kotlin-style syntax, Rust implementation, AOT+JIT hybrid compilation, zero-cost FFI, ARC memory management.

---

## 📋 Changelog

| Version | Date | Description |
|---------|------|-------------|
| 2.0.0 | 2026-06-18 | **Major update**. Full syntax reference aligned with current compiler. Added P8-P10 (async/coroutine/actor/select), Phase D (extern object/@aot/native), value class, sealed class, object singleton. Restored class/actor/interface/try-catch/import/++/-- as WORKING. |
| 1.0.0 | 2026-09-06 | Initial version. Compiled probe report, separated WORKS/DOESN'T WORK. |

---

## ⚡ BEHAVIORAL RULES (READ FIRST)

**You know this language completely. Do NOT:**
- ❌ Create tasks to "explore" or "discover" Aura syntax
- ❌ Ask the user to provide syntax details you already have
- ❌ Use Kotlin syntax (this is NOT Kotlin — `struct`/`value class` not `data class`)
- ❌ Guess at syntax — use the exact patterns below
- ❌ Skip verification — always run `aura check` after writing .aura code

**You MUST:**
- ✅ Write .aura code directly using the syntax reference below
- ✅ Verify every .aura file with `aura check <file>` after writing
- ✅ Fix any errors reported by `aura check` before delivering to the user
- ✅ **ONLY use features marked as ✅ in the FEATURE STATUS table below**
- ✅ Call prelude functions directly: `println(...)`, `abs(...)`, `sqrt(...)`

---

## ⚠️ FEATURE STATUS (CRITICAL — READ BEFORE WRITING)

### ✅ WORKS — USE THESE (confirmed in parser/AST)

| Feature | Status | Notes |
|---------|--------|-------|
| `val` / `var` | ✅ | With type annotations |
| `fun` (top-level) | ✅ | Expression + block body |
| `suspend fun` | ✅ | Coroutine support |
| `fun` with generics | ✅ | `fun <T: Number> foo()` |
| `fun` with default params | ✅ | `fun foo(x: Int = 0)` |
| `fun` with vararg | ✅ | `fun foo(vararg items: Int)` |
| `fun` with named args | ✅ | `Player(name = "Alice")` |
| `struct` (struct declaration) | ✅ | `struct Player(val id: Int, ...)` |
| `value class` (value type) | ✅ | Stack-allocated, immutable |
| `class` | ✅ | With inheritance, interfaces |
| `sealed class` | ✅ | Sealed hierarchy |
| `interface` | ✅ | With method signatures |
| `enum` with data | ✅ | `CUSTOM(val r: Int, ...)` |
| `actor` | ✅ | Concurrent entity |
| `object` (singleton) | ✅ | Global unique instance |
| `typealias` | ✅ | Type alias |
| `import` | ✅ | Module imports with aliases |
| `extern "c"` | ✅ | C ABI FFI (inline + block) |
| `extern object` + `@aot` | ✅ | AOT FFI interface |
| `@native(...)` | ✅ | Syscall/asm/builtin |
| `@aot` | ✅ | AOT annotation |
| `if` / `else` | ✅ | Expression or statement |
| `when` (value/range/type) | ✅ | With `is`, `in`, `else`, no subject |
| `for (i in 0..5)` | ✅ | Range iteration |
| `for (i in list)` | ✅ | Iterable iteration |
| `while` / `do-while` | ✅ | |
| `try-catch` | ✅ | With `Exception` type |
| `try-catch-finally` | ✅ | |
| `break` / `continue` | ✅ | Including labeled (`break@outer`) |
| `defer` | ✅ | Defer block for cleanup |
| `async { }` | ✅ | Async block (P8) |
| `await` | ✅ | Await expression (P8) |
| `select` | ✅ | Multiplexing (P10) |
| `throw` | ✅ | Throw expression |
| `return` | ✅ | Return with value |
| String interpolation | ✅ | `$var`, `${expr}`, `"""raw"""` |
| String methods | ✅ | See below |
| Lambda / closure | ✅ | `{ x -> x + 1 }` |
| Method reference | ✅ | `obj::method` |
| All operators | ✅ | `+` `*` `-` `/` `%` `+=` `-=` etc. |
| `++` / `--` | ✅ | `x++` / `x--` / `++x` |
| `toInt()` / `toFloat()` | ✅ | String → number |
| `toString()` | ✅ | Any → String |
| Type cast (`as`, `as?`) | ✅ | |
| Null safety (`?`, `?.`, `!!`, `?:`) | ✅ | |
| Range (`..`) | ✅ | `0..10`, `1..=10` |
| Destructuring | ✅ | `val (a, b) = pair` |
| Annotations (`@`) | ✅ | `@aot`, `@native`, `@Depends` |
| Visibility (`public` `private` `protected` `internal`) | ✅ | |
| Modifiers (`open` `abstract` `sealed` `override` etc.) | ✅ | |
| Memory ops (`malloc` `free` `retain` `release`) | ✅ | P7 |
| `box` / `weak` | ✅ | P7 |
| `this` / `super` | ✅ | |
| `new` | ✅ | New expression |
| **Prelude functions** | ✅ | See list below |
| **Std library modules** | ✅ | See list below |

### ❌ NOT YET IMPLEMENTED (do NOT use)

| Feature | Status | Notes |
|---------|--------|-------|
| `data class` (Kotlin style) | ❌ | Use `value class` instead |
| `mutableListOf` | ❌ | Use string manipulation |
| `mapOf` / `emptyList` | ❌ | Not in prelude |
| List indexing (multi-arg) | ❌ | `listOf(1, 2)` fails — use single arg only |
| `listOf` with 2+ args | ❌ | Use `listOf(1)` single-arg only |
| `emptyList()` | ❌ | Not implemented |

### 📋 Prelude Functions (No Import Needed — Always Available)

```
println, print, puts
abs, sqrt, pow
toInt, toFloat, toStr, toString
clock, strlen
CString, CStr
ptrIsNull, ptrToInt
intToPtr, makeCallback
```

### 📋 Std Library Modules (require import)

```
import aura.lang.std.String
import aura.lang.std.Collections
import aura.lang.std.Math
import aura.lang.std.Iter
import aura.lang.std.FileSystem
import aura.lang.std.Process
import aura.lang.std.IO
import aura.lang.std.Json
import aura.lang.std.Time
import aura.lang.std.Path
import aura.lang.std.Random
import aura.lang.std.Encoding
import aura.lang.std.Test
import aura.lang.std.Net
import aura.lang.std.Env
import aura.lang.std.Console
import aura.lang.std.ASCII
import aura.lang.std.Assert
import aura.lang.std.Builtin
```

---

## 🔧 Code Verification (MANDATORY)

After writing ANY .aura file, run:

```bash
"D:\Code\AuraLang\target\release\aura.exe" check <path-to-file.aura>
```

- ✅ `✓ <file> 检查通过` = success, deliver to user
- ❌ Error output = fix the code and re-run check

**Other commands:**
```bash
aura run <file>              # Compile and run (VM mode)
aura build <file> --aot      # AOT compile (needs LLVM)
aura ast <file>              # Output AST
aura tokens <file>           # Output tokens
aura fmt <file>              # Format code
```

**AOT setup** (needs LLVM):
```powershell
$env:AURA_LLVM_HOME = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc"
$env:Path = "D:\Code\AuraLang\target\release;$env:AURA_LLVM_HOME\bin;$env:Path"
```

---

## Complete Keyword Table

### Hard Keywords (always recognized)

| Category | Keywords |
|----------|----------|
| **Variable** | `val` `var` |
| **Function** | `fun` |
| **Type** | `struct` `class` `interface` `enum` `actor` `object` `typealias` |
| **Control** | `if` `else` `when` `for` `while` `do` `try` `catch` `finally` `throw` `break` `continue` `return` `defer` `select` |
| **Concurrency** | `async` `await` `suspend` |
| **Access** | `public` `private` `protected` `internal` |
| **Type ops** | `is` `as` `in` `to` `it` `by` |
| **Other** | `import` `extern` `native` `null` `this` `super` `unit` |

### Prefix Modifiers

`abstract` `open` `sealed` `data` `value` `override` `lateinit` `inline` `suspend` `comptime` `const` `lazy` `defer` `extern` `box` `weak` `async` `init` `companion` `constructor` `expect` `actual` `malloc` `free` `retain` `release` `native` `operator` `infix` `tailrec`

### Context Keywords (soft, stored as Ident)

`it` `by` `get` `set` `where` `then` `field` `final` `annotation` `inner` `noinline` `crossinline` `vararg` `reified`

### Boolean Literals

`true` `false` `null`

### Type Keywords (stored as Ident but represent types)

`Int` `Long` `Short` `Byte` `Float` `Double` `Boolean` `Char` `String` `Any` `Nothing` `Unit`

---

## Syntax Quick Reference

### Variables & Types
```aura
val immutable = 42                    // immutable
var count: Int = 0                    // mutable
val name: String = "Alice"            // with type annotation
val flag: Boolean = true
var cache: String                     // uninitialized (lateinit)

// Type conversion
val n = "123".toInt()                 // String → Int
val f = "1.5".toFloat()               // String → Float
val s = 42.toString()                 // Int → String

// Null safety
val safe: Int = n ?: 0                // Elvis operator
val len: Int? = p.tag?.length         // Safe call
val forced: Int = n!!                 // Assert non-null
```

### Functions
```aura
// Expression body
fun add(a: Int, b: Int): Int = a + b

// Block body
fun add(a: Int, b: Int): Int { return a + b }

// Default parameters
fun power(base: Int, exp: Int = 2): Int { }

// Vararg
fun join(vararg parts: String): String { }

// Generic with bound
fun <T: Number> first(items: List<T>): T? { }

// Coroutine
suspend fun fetchData(): Int = 0

// Entry point
fun main() { println("Hello!") }
fun main(): Int { return 0 }

// Inline
inline fun max(a: Int, b: Int): Int = a

// Comptime
comptime fun constValue(): Int = 1
```

### Data Structures
```aura
// Value class (value type, stack-allocated)
value class Point(val x: Int, val y: Int)

// Struct (same as value class)
struct Player(val id: Int, val name: String, val health: Int = 100)

// Class (reference type, inheritance)
class Circle : Shape() {
    override fun area(): Float = 0.0f
}

// Sealed class
sealed class Shape {
    fun area(): Float = 0.0f
}

// Interface
interface Drawable {
    fun draw(): Unit
}

// Enum with data variants
enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

// Actor (concurrent entity)
actor Scheduler {
    private var tick: Int = 0
    fun step() { tick += 1 }
}

// Object (singleton)
object Logger {
    fun log(msg: String) { println(msg) }
}

// Type alias
typealias Vec2 = Pair<Int, Int>

// Instantiation
val p = Player(1, "Alice", 50)
println(p.name)
```

### Control Flow
```aura
// if expression
val status = if (hp > 0) "alive" else "dead"

// if statement
if (x > 0) {
    println("positive")
} else {
    println("non-positive")
}

// when with subject
val result = when (score) {
    0 -> "zero"
    in 1..50 -> "low"
    51..100 -> "high"
    else -> "extreme"
}

// when without subject
when {
    cond1 -> result1
    cond2 -> result2
    else -> result3
}

// when with type check
val t = when (val) {
    is Int -> "int"
    is String -> "string"
    else -> "other"
}

// for loop (range)
for (i in 0..5) {
    println(i)
}

// for loop (iterable)
for (item in list) {
    println(item)
}

// C-style for
for (var i = 0; i < 5; i = i + 1) {
    println(i)
}

// while loop
var i = 0
while (i < 3) {
    i = i + 1
}

// do-while
do {
    i = i + 1
} while (i < 3)

// Labeled break
outer@ for (a in 0..3) {
    for (b in 0..3) {
        if (a == b) break@outer
    }
}
```

### Concurrency (P8-P10)
```aura
// Async block
async {
    await fetchData()
}

// Await expression
val result = await fetch()

// Select multiplexing
select {
    case msg = channel -> { handle(msg) }
    case timeout -> { fallback() }
}

// Actor
actor Server {
    fun start() { println("Running") }
}
val server = concurrent.spawnActor(Server::class)
```

### Strings
```aura
// Interpolation
val message = "Hello $name"
val expr = "Score: ${score * 2}"

// Raw string (multi-line, no interpolation)
val raw = """
    No interpolation here
    Backslash: \n
"""

// String methods
val s = "Hello World"
s.length              // Int
s.contains("World")   // Bool
s.indexOf("World")    // Int
s.substring(0, 5)     // String
s.trim()              // String
s.toUpperCase()       // String
s.toLowerCase()       // String
s.split(" ")          // List<String>
s.toInt()             // String → Int
```

### FFI (C ABI)
```aura
// Inline declaration
extern "c" fun puts(msg: String): Int
extern "c" fun strlen(s: CString): Int

// Block declaration
extern "c" "raylib" {
    fun DrawCircle(x: Int, y: Int, radius: Float, color: Color)
    fun GetFrameTime(): Float
    val WHITE: Color
}
```

### FFI (AOT — JitValue ABI)
```aura
extern object Utils {
    default fun loadLibrary(): String = "utils"
    @aot fun add(a: Int, b: Int): Int
    @aot fun multiply(a: Int, b: Int): Int
}

// Usage
val sum = Utils.add(3, 4)
```

### Native Annotation
```aura
// Syscall
@native(SYS_READ)
fun read(fd: Int, buf: CString, count: Int): Int

// Inline asm
@native(asm = "rdtsc")
fun rdtsc(): Long

// Compiler builtin
native fun builtinFunc(a: Int): Int
```

### Type System
```aura
// Built-in types
val i: Int = 42
val l: Long = 42L
val f: Float = 3.14f
val d: Double = 3.14
val b: Boolean = true
val c: Char = 'A'
val s: String = "hello"

// Generics
val nums: List<Int> = listOf(1, 2, 3)
fun <T> identity(x: T): T = x

// Nullable
val n: Int? = null
val s: String? = p.tag

// Function types
val f: (Int, Int) -> Int = { a, b -> a + b }
```

### Destructuring
```aura
// Destructuring declaration
val (a, b) = pair

// Destructuring in for
for ((k, v) in map) {
    println("Key: $k, Value: $v")
}
```

---

## Complete Working Examples

### Example 1: Basic Calculator
```aura
fun add(a: Int, b: Int): Int = a + b

fun fib(n: Int): Int {
    if (n < 2) { return n }
    return add(fib(n - 1), fib(n - 2))
}

fun max(a: Int, b: Int): Int {
    if (a > b) { return a }
    else { return b }
}

fun main() {
    println(add(10, 20))
    println(max(10, 20))
    println(fib(6))
}
```

### Example 2: Value Class + Enum
```aura
value class Player(val id: Int, val name: String, val health: Int = 100)

enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

fun main() {
    val p = Player(1, "Alice", 50)
    println("Player: " + p.name + ", HP: " + p.health.toString())
    val c = Color.CUSTOM(255, 128, 0)
    println("Color: " + c.toString())
}
```

### Example 3: Class + Interface
```aura
interface Drawable {
    fun draw(): Unit
}

class Circle(val radius: Float) : Drawable {
    override fun draw() {
        println("Drawing circle with radius: $radius")
    }

    fun area(): Float = 3.14159f * radius * radius
}

fun main() {
    val c = Circle(5.0f)
    c.draw()
    println("Area: " + c.area())
}
```

### Example 4: Sealed Class + When
```aura
sealed class Shape {
    fun area(): Float = 0.0f
}

class Circle(val radius: Float) : Shape() {
    override fun area(): Float = 3.14159f * radius * radius
}

class Rectangle(val width: Float, val height: Float) : Shape() {
    override fun area(): Float = width * height
}

fun describe(shape: Shape): String = when (shape) {
    is Circle -> "Circle(r=${shape.radius})"
    is Rectangle -> "Rect(${shape.width}x${shape.height})"
}

fun main() {
    val shapes: List<Shape> = listOf(Circle(3.0f), Rectangle(4.0f, 5.0f))
    for (s in shapes) {
        println(describe(s) + " area=" + s.area())
    }
}
```

### Example 5: Actor + Select
```aura
actor Counter {
    private var value: Int = 0
    fun increment() { value = value + 1 }
    fun get(): Int { return value }
}

fun main() {
    val counter = concurrent.spawnActor(Counter::class)
    for (i in 0..5) {
        counter.increment()
        println("Count: " + counter.get())
    }
}
```

### Example 6: FFI (C ABI)
```aura
extern "c" "libc" {
    fun strlen(s: CString): Int
    fun sqrt(x: Float): Float
    fun clock(): Long
}

fun main() {
    val msg = "Hello Aura"
    println("Length: " + strlen(msg).toString())
    println("Sqrt: " + sqrt(16.0f))
    println("Clock: " + clock().toString())
}
```

### Example 7: FFI (AOT — extern object)
```aura
extern object Utils {
    default fun loadLibrary(): String = "utils"
    @aot fun add(a: Int, b: Int): Int
    @aot fun factorial(n: Int): Int
}

fun main() {
    val sum = Utils.add(3, 4)
    println("Sum: " + sum.toString())
    val fact = Utils.factorial(5)
    println("5! = " + fact.toString())
}
```

### Example 8: Async + Await
```aura
suspend fun fetchData(): Int = 42

fun main() {
    async {
        val data = await fetchData()
        println("Got: " + data.toString())
    }
}
```

### Example 9: Defer + Try-Catch
```aura
fun readFile(path: String): String {
    defer {
        println("Cleanup: closing file")
    }
    try {
        val content = FileSystem.readText(path)
        return content
    } catch (e: Exception) {
        println("Error: " + e.toString())
        return ""
    } finally {
        println("Finally block")
    }
}

fun main() {
    val result = readFile("config.txt")
    println("Result: " + result)
}
```

### Example 10: Labeled Break + While
```aura
fun main() {
    outer@
    for (i in 0..5) {
        for (j in 0..5) {
            if (i == j) break@outer
            println("($i, $j)")
        }
    }

    var x = 10
    while (x > 0) {
        x = x - 1
        if (x == 3) continue
        println("x = " + x.toString())
    }
}
```

---

## Code Style

| Element | Convention | Example |
|---------|------------|---------|
| Constants/Enums | UPPER_CASE | `GAME_WIDTH`, `RED` |
| Classes/Interfaces/Structs | PascalCase | `Player`, `Drawable` |
| Functions/Variables | camelCase | `loadConfig`, `playerName` |
| Generic params | Single uppercase | `T`, `U`, `K`, `V` |
| Modules | lowercase dots | `std.io`, `std.math` |

**Formatting**: 4-space indent, spaces around operators, Kotlin-style braces

---

## ⚠️ Common Pitfalls

| Pitfall | ✅ Correct | ❌ Wrong |
|---------|-----------|---------|
| Data struct | `value class Name(...)` | `data class Name(...)` |
| Value type | `value class` | `data class` |
| Value type (alt) | `struct Name(...)` | `data class Name(...)` |
| Concurrent entity | `actor Name { }` | `class Name { }` |
| FFI (C ABI) | `extern "c" fun f()` | `extern fun f()` |
| FFI block | `extern "c" "lib" { }` | `extern("c") { }` |
| FFI AOT | `extern object Name { @aot fun f() }` | `extern fun f()` |
| Native | `@native(SYS_READ)` | `@syscall(SYS_READ)` |
| await | `await fetch()` | `await(fetch())` |
| Struct ctor | `Player(1, "Alice")` | `Player(id=1, name="Alice")` |
| String interp | `"${expr}"` | `${expr}` (no quotes) |
| Raw string | `"""..."""` | `r"..."` |
| Increment | `x++` / `++x` / `x = x + 1` | — (all work now) |
| Elvis | `n ?: 0` | `if (n != null) { n } else { 0 }` |
| Safe access | `obj?.field` | `if (obj != null) { obj.field }` |

---

## Reference Documentation

Full source at `D:\Code\AuraLang\`:
- `compiler/src/token.rs` — Complete token/keyword definitions
- `compiler/src/lexer.rs` — Lexer implementation (authoritative keyword table)
- `compiler/src/parser.rs` — Parser implementation (all supported constructs)
- `compiler/src/ast.rs` — AST node definitions
- `book/chapter-01.md` — Language tutorial
- `book/chapter-02.md` — Example projects
- `docs/01-aura-language-card.md` — Complete language card
- `docs/api/index.md` — Auto-generated API docs
- `target/release/aura.exe` — Compiler binary (check/run/fmt)
- `examples/` — Real .aura files