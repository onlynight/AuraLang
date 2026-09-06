---
name: aura-lang
version: 1.0.0
lastModified: 2026-09-06
changes: 合并编译器探测报告，区分 WORKS/DOESN'T WORK，移除 class/actor/try-catch/import 等不可用特性
description: Use when writing, reviewing, or debugging Aura programming language code (.aura files). Aura is a system-level scripting language with Kotlin-style syntax. This skill provides complete syntax reference, compiler limitations, and code verification workflow.
---

# Aura Language — Complete Reference

Aura is a **system-level scripting language for NovaOS**. Kotlin-style syntax, Rust implementation, AOT+JIT hybrid compilation, zero-cost FFI, ARC memory management.

---

## 📋 Changelog

| Version | Date | Description |
|---------|------|-------------|
| 1.0.0 | 2026-09-06 | 初始版本。合并编译器探测报告，区分 WORKS/DOESN'T WORK，移除 class/actor/try-catch/import 等不可用特性 |

---

## ⚡ BEHAVIORAL RULES (READ FIRST)

**You know this language completely. Do NOT:**
- ❌ Create tasks to "explore" or "discover" Aura syntax
- ❌ Ask the user to provide syntax details you already have
- ❌ Use Kotlin syntax (this is NOT Kotlin — `struct` not `data class`)
- ❌ Guess at syntax — use the exact patterns below
- ❌ Skip verification — always run `aura check` after writing .aura code
- ❌ **Use BROKEN features** (class, actor, break, continue, try-catch, import system, ++, --)

**You MUST:**
- ✅ Write .aura code directly using the syntax reference below
- ✅ Verify every .aura file with `aura check <file>` after writing
- ✅ Fix any errors reported by `aura check` before delivering to the user
- ✅ **ONLY use features marked as ✅ in the COMPILER LIMITATIONS table below**
- ✅ Call prelude functions directly: `println(...)`, `abs(...)`, `sqrt(...)`

---

## ⚠️ COMPILER LIMITATIONS (CRITICAL — READ BEFORE WRITING)

The following features are **BROKEN or NOT YET IMPLEMENTED** in the current Aura compiler (`aura.exe`).
Using them will cause compilation failures. **DO NOT USE.**

### ❌ BROKEN — DO NOT USE

| Feature | Status | Workaround |
|---------|--------|------------|
| `class` declaration | ❌ Compile error | Use top-level `fun` + `struct` |
| Class method calls | ❌ `class not found` | Use top-level functions |
| `this` keyword | ❌ `class not found` | Pass instance as parameter |
| `actor` definition | ❌ `actor not implemented` | Use `struct` + top-level functions |
| `break` / `continue` | ❌ `token not expected` | Use recursion or condition short-circuit |
| Labeled break | ❌ `token not expected` | Use recursion |
| `try-catch` | ❌ `token not expected` | Use return value checks |
| `import` system | ❌ `token not expected` | Only use 17 prelude functions |
| `++` / `--` | ❌ `unresolved function` | Use `x = x + 1` / `x = x - 1` |
| `mutableListOf` | ❌ `unresolved function` | Use string concatenation |
| `mapOf` / `emptyList` | ❌ `unresolved function` | Use string with separators |
| List indexing/size/add | ❌ `not a function` | Split strings with `split(", ")` |
| Nested functions | ❌ `function not expected` | Move to top-level |
| All `import aura.*` | ❌ All fail | Import system is broken |
| All std module calls | ❌ `unresolved function` | Only prelude + string methods |

### ✅ WORKS — USE THESE

| Feature | Status | Notes |
|---------|--------|-------|
| `struct` + field access | ✅ | `Player(1, "Alice", 50)` |
| `fun` (top-level) | ✅ | Expression + block body |
| `enum` with data | ✅ | `CUSTOM(val r: Int, ...)` |
| `val` / `var` | ✅ | With type annotations |
| `main()` | ✅ | Returns `Unit` or `Int` |
| `for (i in 0..5)` | ✅ | Range iteration |
| `while` / `do-while` | ✅ | |
| `if` / `else` | ✅ | Expression or statement |
| `when` (value/range/type) | ✅ | With `is`, `in`, `else` |
| String interpolation | ✅ | `$var`, `${expr}`, `"""raw"""` |
| String methods | ✅ | `length`, `contains`, `indexOf`, `substring`, `trim`, `toUpperCase`, `toLowerCase`, `split`, `toInt` |
| Arithmetic | ✅ | `+`, `*`, `-`, `/` |
| `toInt()` / `toFloat()` | ✅ | String → number |
| `toString()` | ✅ | Any → String |
| `listOf(1)` | ⚠️ **1 arg only** | `listOf(1, 2)` fails! |
| **17 Prelude functions** | ✅ | See list below |

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

## Syntax Quick Reference

### Variables & Types
```aura
val immutable = 42                    // immutable
var count: Int = 0                    // mutable
val name: String = "Alice"            // with type annotation
val flag: Boolean = true

// Type conversion
val n = "123".toInt()                 // String → Int
val f = "1.5".toFloat()               // String → Float
val s = 42.toString()                 // Int → String
```

### Functions
```aura
fun add(a: Int, b: Int): Int = a + b              // expression body
fun add(a: Int, b: Int): Int { return a + b }     // block body
fun power(base: Int, exp: Int = 2): Int { }       // default params
fun <T> identity(x: T): T = x                       // generic
fun main() { println("Hello!") }                    // entry point
fun main(): Int { return 0 }                        // entry point with return
```

### Data Structures
```aura
// Struct with default values
struct Player(val id: Int, val name: String, val health: Int = 100)
struct Point(val x: Int, val y: Int)

// Instantiate
val p = Player(1, "Alice", 50)

// Field access
println(p.name)
println(p.id)
println(p.health)
```

### Enum
```aura
enum Color {
    RED,
    GREEN,
    BLUE,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}
```

### Control Flow
```aura
// for loop (range)
for (i in 0..5) {
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

// if / else
if (x > 0) {
    println("positive")
} else {
    println("non-positive")
}

// when expression (value match)
val result = when (score) {
    0 -> "zero"
    in 1..50 -> "low"
    51..100 -> "high"
    else -> "extreme"
}

// when expression (type match)
val t = when (val) {
    is Int -> "int"
    is String -> "string"
    else -> "other"
}
```

### Strings
```aura
// String methods (call on object)
val s = "Hello World"
s.length                  // Int
s.contains("World")       // Bool
s.indexOf("World")        // Int
s.substring(0, 5)         // String
s.trim()                  // String
s.toUpperCase()           // String
s.toLowerCase()           // String
s.split(" ")              // List<String>
s.toInt()                 // String → Int

// String interpolation
println("Hello $name")
println("Score: ${score * 2}")

// Raw string (triple quotes)
val raw = """
    Multi-line text
    $interpolation
"""

// Concatenation
val msg = "Length: " + s.length.toString()
```

### Workaround Patterns

```aura
// ❌ DO NOT: class
// class Database { fun query() { } }

// ✅ DO: top-level function + struct
struct QueryResult(val success: Bool, val data: String, val error: String)
fun executeQuery(sql: String): QueryResult {
    return QueryResult(true, "result", "")
}

// ❌ DO NOT: ++ / --
// i++

// ✅ DO: explicit assignment
var i = 0
i = i + 1
i = i - 1

// ❌ DO NOT: break / continue
// for (i in 0..10) { if (i == 5) break }

// ✅ DO: recursive function
fun check(i: Int, max: Int) {
    if (i >= max) { return }
    println(i)
    check(i + 1, max)
}

// ❌ DO NOT: try-catch
// try { risky() } catch (e) { handle(e) }

// ✅ DO: check return value
val result = tryParse("123")
if (result.success) { use(result.data) }
else { handleError(result.error) }

// ❌ DO NOT: import
// import aura.io.*

// ✅ DO: only prelude functions
println("Hello")
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

### Example 2: Struct + Enum
```aura
enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

struct Player(val id: Int, val name: String, val health: Int = 100)

fun main() {
    val p = Player(1, "Alice", 50)
    println("Player: " + p.name + ", HP: " + p.health.toString())
    val c = Color.CUSTOM(255, 128, 0)
    println("Color: " + c.toString())
}
```

### Example 3: String Processing
```aura
fun processText(input: String): String {
    val trimmed = input.trim()
    val upper = trimmed.toUpperCase()
    val words = upper.split(" ")
    return upper
}

fun main() {
    val result = processText("  hello aura  ")
    println("Processed: " + result)
}
```

### Example 4: Database Pattern (No class, No import)
```aura
struct DBConfig(val host: String, val port: Int)

struct QueryResult(val success: Bool, val data: String, val error: String)

fun executeQuery(config: DBConfig, sql: String): QueryResult {
    if (sql.length == 0) {
        return QueryResult(false, "", "Empty SQL")
    }
    return QueryResult(true, "result data", "")
}

fun main() {
    val cfg = DBConfig("localhost", 5432)
    val result = executeQuery(cfg, "SELECT * FROM users")
    if (result.success) {
        println("Query OK: " + result.data)
    } else {
        println("Error: " + result.error)
    }
}
```

### Example 5: String-based Collection (No list/map)
```aura
struct Record(val id: Int, val name: String, val value: Int)

fun addRecord(records: String, id: Int, name: String, value: Int): String {
    val sep = ", "
    val newEntry = id.toString() + ":" + name + "=" + value.toString()
    if (records.length == 0) {
        return newEntry
    }
    return records + sep + newEntry
}

fun getRecordCount(records: String): Int {
    if (records.length == 0) { return 0 }
    return records.split(", ").length
}

fun main() {
    var records = ""
    records = addRecord(records, 1, "Alice", 100)
    records = addRecord(records, 2, "Bob", 200)
    println("Records: " + records)
    println("Count: " + getRecordCount(records).toString())
}
```

### Example 6: Error Handling Without try-catch
```aura
struct ParseResult(val success: Bool, val value: Int, val error: String)

fun safeParse(text: String): ParseResult {
    if (text.length == 0) {
        return ParseResult(false, 0, "Empty input")
    }
    return ParseResult(true, text.toInt(), "")
}

fun main() {
    val result = safeParse("123")
    if (result.success) {
        println("Parsed: " + result.value.toString())
    } else {
        println("Parse failed: " + result.error)
    }
    
    val empty = safeParse("")
    if (!empty.success) {
        println("Caught: " + empty.error)
    }
}
```

### Example 7: Recursive Loop (No break/continue)
```aura
fun printRange(from: Int, to: Int) {
    if (from >= to) { return }
    println(from)
    printRange(from + 1, to)
}

fun sumRange(from: Int, to: Int): Int {
    if (from >= to) { return 0 }
    return from + sumRange(from + 1, to)
}

fun main() {
    printRange(0, 5)
    println("Sum: " + sumRange(1, 10).toString())
}
```

---

## Complete Keyword Table

| Category | Keywords |
|----------|----------|
| Prefix modifiers | `abstract` `final` `enum` `open` `annotation` `sealed` `data` `override` `lateinit` `private` `protected` `public` `internal` `inner` `noinline` `crossinline` `vararg` `reified` `tailrec` `operator` `infix` `inline` `external` `const` `suspend` `comptime` `value` `defer` `extern` `lazy` `box` `weak` `async` |
| Postfix modifiers | `where` `by` `get` `set` |
| Soft keywords | `catch` `finally` `field` `else` `then` `unit` |
| Hard keywords | `as` `is` `in` `to` `it` |
| Control keywords | `if` `while` `do` `when` `throw` `return` `for` `select` `await` |

---

## Code Style

| Element | Convention | Example |
|---------|------------|---------|
| Constants/Enums | UPPER_CASE | `GAME_WIDTH`, `RED` |
| Classes/Interfaces/Structs | PascalCase | `Player`, `Drawable` |
| Functions/Variables | camelCase | `loadConfig`, `playerName` |
| Generic params | Single uppercase | `T`, `U`, `K`, `V` |

**Formatting**: 4-space indent, spaces around operators, Kotlin-style braces

---

## ⚠️ Common Pitfalls

| Pitfall | ✅ Correct | ❌ Wrong |
|---------|-----------|---------|
| Data struct | `struct Name(...)` | `data class Name(...)` |
| Class | ❌ Not supported | `class Name { }` |
| Actor | ❌ Not implemented | `actor Name { }` |
| Increment | `x = x + 1` | `x++` / `++x` |
| Break | ❌ Not supported | `break` |
| Try-catch | Check return values | `try { } catch { }` |
| Import | ❌ System broken | `import aura.*` |
| Collections | String with separators | `listOf(1,2)` / `mapOf` |
| Nested fun | Move to top-level | `fun main() { fun inner() { } }` |

---

## Reference Documentation

Full source at `D:\Code\AuraLang\`:
- `docs/01-aura-language-card.md` — Full language card
- `docs/02-aura-stdlib-reference.md` — Complete stdlib API
- `docs/04-aura-style-guide.md` — Style guide
- `book/chapter-01.md` — Language tutorial
- `book/chapter-02.md` — Example projects
- `examples/` — 36 real .aura files
- `docs/api/index.md` — Auto-generated API docs
- `target/release/aura.exe` — Compiler binary (check/run/fmt)
- `D:\Code\AuraProjs\SQLura\docs\Aura编码模式参考.md` — Compiler boundary reference
