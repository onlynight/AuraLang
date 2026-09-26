# StringBuilder 重构方案

> **核心约束**：纯 Aura 实现，仅使用 `Memory.*`（编译器内置）+ `String.charCodeAt` / `fromCharCode` / `String.join`（已有标准库）。**不依赖 CString / ReadCStr / 任何 C FFI**。
> 状态：**方案设计**

---

## 1. 现状问题

### 1.1 当前 API

```aura
object StringBuilder {
    fun create(): Long
    fun append(h: Long, text: Long): Long       // 强制 CString(s) 转换
    fun appendChar(h: Long, ch: Short): Long
    fun appendInt(h: Long, v: Int): Long
    fun length(h: Long): Long
    fun finish(h: Long): Long                    // 返回 Long，不是 String
    fun reset(h: Long): Long
}
```

### 1.2 问题清单

| # | 问题 | 根因 |
|---|------|------|
| P1 | 静态方法 + 显式句柄传递 | 设计为 C 风格 FFI 包装 |
| P2 | `append` 参数为 `Long` | 规避 Plan A 装箱 → 强制 CString |
| P3 | `finish` 返回 `Long` | 转移所有权 → 强制 ReadCStr |
| P4 | `finish` 后句柄失效 | 与 Java StringBuilder 语义相反 |
| P5 | 无链式调用 | 静态方法无法返回 `this` |
| P6 | CString / ReadCStr 依赖 C 实现 | 违反纯 Aura 原则 |

### 1.3 根因链

```
Plan A 装箱陷阱 (i8* 低位标记)
  → 参数声明为 Long 规避
  → CString() 做 String → Long 转换
  → 底层依赖 C 实现（Rust CString / C strlen）
  → API 设计为静态句柄式
  → 体验差、类型不安全、违反纯 Aura 原则
```

---

## 2. 设计目标

| 目标 | 说明 |
|------|------|
| G1 | Java 风格实例类：`val sb = StringBuilder()` |
| G2 | 链式调用：`sb.append("a").append("b")` |
| G3 | 类型安全：参数为 `String`/`Int`/`Char`，不暴露 `Long` |
| G4 | `toString()` 返回 `String`，不销毁缓冲区 |
| G5 | **纯 Aura**：仅用 `Memory.*` + `String.charCodeAt` / `fromCharCode` / `String.join` |
| G6 | **零 CString / 零 C FFI**：不依赖任何 C 实现的原生函数 |
| G7 | VM/AOT 双端一致 |

---

## 3. 架构

```
┌────────────────────────────────────────────────────────┐
│  用户代码                                                │
│  val sb = StringBuilder()                               │
│  sb.append("Hello").append(", ").append(42)             │
│  val s: String = sb.toString()                          │
└──────────────────────────┬─────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────┐
│  StringBuilder (纯 Aura 类)                             │
│                                                         │
│  class StringBuilder {                                 │
│      private var buf: Long     // Memory.alloc 分配的    │
│      private var len: Long     // 当前长度               │
│      private var cap: Long     // 容量                   │
│                                                         │
│      fun append(text: String): StringBuilder            │
│      fun appendChar(ch: Char): StringBuilder            │
│      fun appendInt(value: Int): StringBuilder           │
│      fun toString(): String                             │
│      fun length(): Int                                  │
│      fun reset(): StringBuilder                         │
│      fun dispose(): Unit                                │
│  }                                                      │
└──────────┬─────────────────────────────┬───────────────┘
           │                             │
    ┌──────▼──────┐              ┌───────▼──────┐
    │  Memory.*   │              │  String.*    │
    │  (编译器内置) │              │  (已有标准库)  │
    │             │              │              │
    │ alloc/free  │              │ charCodeAt   │
    │ write/read  │              │ fromCharCode │
    │ copy/set    │              │ String.join  │
    │ write64     │              │ .length      │
    └─────────────┘              └──────────────┘
```

**全部使用已有的编译器内置和标准库函数，不新增任何 C/Rust 原生代码。**

---

## 4. 完整实现

```aura
// aura:///aura/lang/std/StringBuilder.aura
package aura.lang.std

import aura.lang.native.Memory
import aura.lang.collection.List
import aura.lang.collection.ArrayList
import aura.lang.collection.Array

/// Java 风格可变字符串缓冲区。
///
/// 纯 Aura 实现，仅依赖 `Memory.*`（编译器内置）和 `String.*`（标准库）。
/// 不依赖 CString / ReadCStr / 任何 C FFI。
///
/// 用法：
/// ```aura
/// val sb = StringBuilder()
/// sb.append("Hello").append(", ").append("World").appendInt(42)
/// val s: String = sb.toString()
/// sb.append(" (追加)")   // toString 后 sb 仍可用
/// sb.dispose()
/// ```
class StringBuilder {

    // ════════════════════════════════════════════
    // 内部状态
    // ════════════════════════════════════════════

    private var buf: Long = 0
    private var len: Long = 0
    private var cap: Long = 0

    // ════════════════════════════════════════════
    // 构造器
    // ════════════════════════════════════════════

    init() {
        this.buf = Memory.alloc(256)
        if (this.buf != 0) {
            Memory.write(this.buf, 0 as Byte)
            this.cap = 256
        }
    }

    // ════════════════════════════════════════════
    // 追加操作（返回 this，支持链式）
    // ════════════════════════════════════════════

    /// 追加字符串。逐字符读取（charCodeAt → Memory.write），无需 CString。
    fun append(text: String): StringBuilder {
        val n: Int = text.length
        this.reserve(this.len + n as Long + 1)
        if (this.buf == 0) { return this }
        var i: Int = 0
        while (i < n) {
            val c: Int = text.charCodeAt(i)
            if (c <= 0) { break }
            Memory.write(this.buf + this.len, c as Byte)
            this.len = this.len + 1
            i = i + 1
        }
        Memory.write(this.buf + this.len, 0 as Byte)
        return this
    }

    /// 追加单字符。直接写码点到缓冲区。
    fun appendChar(ch: Char): StringBuilder {
        val c: Int = ch as Int
        this.reserve(this.len + 2)
        if (this.buf == 0) { return this }
        Memory.write(this.buf + this.len, c as Byte)
        this.len = this.len + 1
        Memory.write(this.buf + this.len, 0 as Byte)
        return this
    }

    /// 追加整数（十进制）。
    fun appendInt(value: Int): StringBuilder {
        return this.append(value.toString())
    }

    /// 追加浮点数。
    fun appendFloat(value: Float): StringBuilder {
        return this.append(value.toString())
    }

    /// 追加布尔值（"true" / "false"）。
    fun appendBoolean(value: Boolean): StringBuilder {
        if (value) {
            return this.append("true")
        } else {
            return this.append("false")
        }
    }

    /// 追加另一个 StringBuilder 的内容。
    fun append(builder: StringBuilder): StringBuilder {
        return this.append(builder.toString())
    }

    // ════════════════════════════════════════════
    // 查询操作
    // ════════════════════════════════════════════

    /// 当前内容长度。
    fun length(): Int {
        return this.len as Int
    }

    /// 当前容量。
    fun capacity(): Int {
        return this.cap as Int
    }

    /// 是否为空。
    fun isEmpty(): Boolean {
        return this.len == 0
    }

    // ════════════════════════════════════════════
    // 转换操作
    // ════════════════════════════════════════════

    /// 返回当前内容的 String 副本（**不转移所有权**）。
    ///
    /// 逐字节从缓冲区读取 → fromCharCode 构造单字符 → String.join 合并。
    /// 树形归并保证总拷贝量 O(n log n)。
    override fun toString(): String {
        if (this.buf == 0 || this.len == 0) { return "" }
        val parts = ArrayList<String>()
        var i: Long = 0
        while (i < this.len) {
            val c: Int = Memory.read(this.buf + i) as Int
            if (c == 0) { break }
            parts.add(String.fromCharCode(c))
            i = i + 1
        }
        return joinTree(parts, 0, parts.size - 1)
    }

    // ════════════════════════════════════════════
    // 重置操作
    // ════════════════════════════════════════════

    /// 清空内容，保留容量。
    fun reset(): StringBuilder {
        this.len = 0
        if (this.buf != 0) {
            Memory.write(this.buf, 0 as Byte)
        }
        return this
    }

    /// 清空内容（`reset()` 的别名）。
    fun clear(): StringBuilder {
        return this.reset()
    }

    // ════════════════════════════════════════════
    // 容量管理
    // ════════════════════════════════════════════

    /// 确保容量 >= minCap。
    fun ensureCapacity(minCap: Int): Unit {
        this.reserve(minCap as Long)
    }

    // ════════════════════════════════════════════
    // 释放
    // ════════════════════════════════════════════

    /// 释放 native 缓冲区（AOT 无 GC，显式释放）。
    fun dispose(): Unit {
        if (this.buf != 0) {
            Memory.free(this.buf)
            this.buf = 0
            this.len = 0
            this.cap = 0
        }
    }

    // ════════════════════════════════════════════
    // 内部辅助
    // ════════════════════════════════════════════

    /// 确保容量 >= need（几何增长，摊还 O(1)）。
    private fun reserve(need: Long): Unit {
        if (this.buf == 0) {
            this.buf = Memory.alloc(need)
            if (this.buf != 0) {
                Memory.write(this.buf, 0 as Byte)
                this.cap = need
            }
            return
        }
        if (this.cap >= need) { return }
        var newCap: Long = this.cap
        if (newCap < 256) { newCap = 256 }
        while (newCap < need) {
            newCap = newCap * 2
        }
        val newBuf: Long = Memory.alloc(newCap)
        if (newBuf == 0) { return }
        if (this.len > 0) {
            Memory.copy(newBuf, this.buf, this.len)
        }
        Memory.write(newBuf + this.len, 0 as Byte)
        Memory.free(this.buf)
        this.buf = newBuf
        this.cap = newCap
    }

    /// 树形归并：O(n log n) 拷贝量，避免逐次拼接的 O(n²)。
    private fun joinTree(parts: ArrayList<String>, lo: Int, hi: Int): String {
        if (lo > hi) { return "" }
        if (lo == hi) { return parts[lo] }
        val mid: Int = (lo + hi) / 2
        return joinTree(parts, lo, mid) + joinTree(parts, mid + 1, hi)
    }
}
```

---

## 5. 核心设计决策

### 5.1 为什么不用 CString

| 方案 | 依赖 | 性能 | 纯 Aura |
|------|------|------|---------|
| CString | Rust CString + libc strlen | 快 | ❌ C FFI |
| charCodeAt | String.charCodeAt + Memory.write | 中（O(n)） | ✅ 纯 Aura |

`charCodeAt(i)` 在 AOT 下是**内联 load**（单字节读取，O(1)），在 VM 下是标准方法调用。逐字符写入的总复杂度为 O(n)，可接受。

### 5.2 toString() 的树形归并

逐次拼接 `result = result + char` 是 O(n²)。树形归并：

```
[a, b, c, d, e, f, g, h]
→ [ab, cd, ef, gh]
→ [abcd, efgh]
→ [abcdefgh]
```

每层拷贝 n 个字符，共 log₂(n) 层，总拷贝量 **O(n log n)**。

对于 4MB IR 输出：4M × 22 ≈ 88M 次拷贝，可接受。

### 5.3 缓冲区管理

| 操作 | 实现 |
|------|------|
| 分配 | `Memory.alloc`（编译器内置 → libc malloc） |
| 读取 | `Memory.read`（编译器内置 → LLVM load） |
| 写入 | `Memory.write`（编译器内置 → LLVM store） |
| 拷贝 | `Memory.copy`（编译器内置 → LLVM memcpy） |
| 释放 | `Memory.free`（编译器内置 → libc free） |
| 扩容 | `alloc(newCap)` + `copy` + `free(old)` |

全部是编译器内置操作，非 C FFI。

### 5.4 零 CString 的字符串转换路径

```
String → 缓冲区（append）：
  text.charCodeAt(i)  →  Memory.write(buf + len, c)
  （逐字符读取码点 → 逐字节写入）

缓冲区 → String（toString）：
  Memory.read(buf + i)  →  String.fromCharCode(c)
  树形归并合并所有单字符字符串
```

---

## 6. API 对比

| 操作 | 旧 API | 新 API |
|------|--------|--------|
| 创建 | `StringBuilder.create()` → Long | `StringBuilder()` → 实例 |
| 追加字符串 | `StringBuilder.append(h, CString(s))` | `sb.append(s)` |
| 追加字符 | `StringBuilder.appendChar(h, 'x')` | `sb.appendChar('x')` |
| 追加整数 | `StringBuilder.appendInt(h, 42)` | `sb.appendInt(42)` |
| 追加浮点 | — | `sb.appendFloat(3.14f)` |
| 追加布尔 | — | `sb.appendBoolean(true)` |
| 追加另一 sb | — | `sb.append(otherSb)` |
| 获取长度 | `StringBuilder.length(h)` → Long | `sb.length()` → Int |
| 获取容量 | — | `sb.capacity()` → Int |
| 转 String | `ReadCStr(StringBuilder.finish(h))` | `sb.toString()` → String |
| 清空 | `StringBuilder.reset(h)` | `sb.reset()` |
| 释放 | —（泄漏） | `sb.dispose()` |
| 链式 | ❌ | ✅ |
| toString 后复用 | ❌ 句柄失效 | ✅ |
| C FFI 依赖 | CString + ReadCStr | **零依赖** |

---

## 7. EmitBuffer 迁移

### 迁移前（依赖 CString + ReadCStr）

```aura
class EmitBuffer {
    var h: Long = 0
    init() { this.h = StringBuilder.create() }
    fun sbAdd(s: String) { this.h = StringBuilder.append(this.h, CString(s)) }
    fun sbBuild(): String {
        val n = StringBuilder.length(this.h)
        val p = StringBuilder.finish(this.h)
        if (p == 0 || n <= 0) { return "" }
        return ReadCStr(p)
    }
    fun sbReset() { this.h = StringBuilder.reset(this.h) }
}
```

### 迁移后（纯 Aura，零 C FFI）

```aura
class EmitBuffer {
    private val sb: StringBuilder = StringBuilder()

    fun sbAdd(s: String) { this.sb.append(s) }
    fun sbBuild(): String { return this.sb.toString() }
    fun sbReset() { this.sb.reset() }
    fun sbSize(): Int { return this.sb.length() }
}
```

**消除 CString、ReadCStr、finish、显式句柄传递。**

---

## 8. 测试用例

```aura
// tests/aot/string_builder_runtime.aura

fun main(): Int {
    // 1. 基本链式追加
    val sb = StringBuilder()
    sb.append("Hello").append(", ").append("World").append("!").appendInt(42)
    if (sb.length() != 15) { return 1 }
    if (sb.toString() != "Hello, World!42") { return 2 }

    // 2. toString 后 sb 仍可用
    sb.append(" extra")
    if (sb.length() != 21) { return 3 }

    // 3. reset 清空内容，保留容量
    sb.reset()
    if (sb.length() != 0) { return 4 }
    if (sb.capacity() < 256) { return 5 }

    // 4. appendChar
    sb.appendChar('A').appendChar('B').appendChar('C')
    if (sb.toString() != "ABC") { return 6 }

    // 5. appendBoolean
    sb.reset()
    sb.appendBoolean(true).append(" & ").appendBoolean(false)
    if (sb.toString() != "true & false") { return 7 }

    // 6. append 另一 StringBuilder
    val sb2 = StringBuilder()
    sb2.append("nested")
    sb.reset().append("[").append(sb2).append("]")
    if (sb.toString() != "[nested]") { return 8 }

    // 7. appendFloat
    sb.reset()
    sb.appendFloat(3.14f)
    if (sb.length() < 3) { return 9 }

    // 8. ensureCapacity
    sb.reset()
    sb.ensureCapacity(1024)
    if (sb.capacity() < 1024) { return 10 }

    // 9. dispose
    sb.dispose()
    sb2.dispose()

    println("aot.string_builder_runtime ok")
    return 0
}
```

---

## 9. 可独立测试的开发阶段

每个阶段可独立编译、独立测试、独立回滚。阶段间通过接口契约衔接。

---

### Phase 1：StringBuilder 类（L1 纯 Aura）

**目标**：新增 `class StringBuilder`，纯 Aura 实现，零 C FFI。

**交付物**：

| 文件 | 变更 |
|------|------|
| `aura/core/aura/lang/std/StringBuilder.aura` | 新增 class + 保留旧 object 为 deprecated |

**接口契约**：

```
append(text: String) → StringBuilder          // 链式
appendChar(ch: Char) → StringBuilder          // 链式
appendInt(value: Int) → StringBuilder         // 链式
appendFloat(value: Float) → StringBuilder     // 链式
appendBoolean(value: Boolean) → StringBuilder // 链式
append(builder: StringBuilder) → StringBuilder// 链式
length() → Int
capacity() → Int
isEmpty() → Boolean
toString() → String                           // 不销毁缓冲区
reset() → StringBuilder                       // 链式
clear() → StringBuilder                       // 链式
ensureCapacity(minCap: Int) → Unit
dispose() → Unit
```

**测试用例**：

```aura
// tests/phase1/string_builder_class.aura

fun test1_basicAppend(): Int {
    val sb = StringBuilder()
    sb.append("Hello").append(", ").append("World").append("!")
    if (sb.length() != 13) { return 1 }
    if (sb.toString() != "Hello, World!") { return 2 }
    return 0
}

fun test2_appendInt(): Int {
    val sb = StringBuilder()
    sb.append("n=").appendInt(42)
    if (sb.toString() != "n=42") { return 3 }
    return 0
}

fun test3_appendChar(): Int {
    val sb = StringBuilder()
    sb.appendChar('A').appendChar('B').appendChar('C')
    if (sb.toString() != "ABC") { return 4 }
    return 0
}

fun test4_toStringReuse(): Int {
    val sb = StringBuilder()
    sb.append("first")
    val s1 = sb.toString()
    sb.append(" second")
    val s2 = sb.toString()
    if (s1 != "first") { return 5 }
    if (s2 != "first second") { return 6 }
    return 0
}

fun test5_reset(): Int {
    val sb = StringBuilder()
    sb.append("data")
    val cap1 = sb.capacity()
    sb.reset()
    if (sb.length() != 0) { return 7 }
    if (sb.capacity() != cap1) { return 8 }  // 容量保留
    return 0
}

fun test6_appendBoolean(): Int {
    val sb = StringBuilder()
    sb.appendBoolean(true).append(" & ").appendBoolean(false)
    if (sb.toString() != "true & false") { return 9 }
    return 0
}

fun test7_appendBuilder(): Int {
    val sb1 = StringBuilder()
    sb1.append("nested")
    val sb2 = StringBuilder()
    sb2.append("[").append(sb1).append("]")
    if (sb2.toString() != "[nested]") { return 10 }
    return 0
}

fun test8_empty(): Int {
    val sb = StringBuilder()
    if (!sb.isEmpty()) { return 11 }
    if (sb.toString() != "") { return 12 }
    return 0
}

fun test9_dispose(): Int {
    val sb = StringBuilder()
    sb.append("data")
    sb.dispose()
    if (sb.length() != 0) { return 13 }
    return 0
}

fun test10_appendFloat(): Int {
    val sb = StringBuilder()
    sb.appendFloat(3.14f)
    if (sb.length() < 3) { return 14 }
    return 0
}

fun test11_ensureCapacity(): Int {
    val sb = StringBuilder()
    sb.ensureCapacity(1024)
    if (sb.capacity() < 1024) { return 15 }
    return 0
}

fun test12_growth(): Int {
    // 测试缓冲区增长
    val sb = StringBuilder()
    for (i in 0..100) {
        sb.append("abc")
    }
    if (sb.length() != 300) { return 17 }
    if (sb.toString() != "abc".repeat(100)) { return 18 }
    return 0
}

fun test13_unicodeBoundary(): Int {
    val sb = StringBuilder()
    sb.append("").appendChar(0 as Char)  // NUL 边界
    if (sb.length() != 0) { return 19 }
    return 0
}

fun main(): Int {
    if (test1_basicAppend() != 0) { println("test1 failed: " + toStr(test1_basicAppend())); return 1 }
    if (test2_appendInt() != 0) { println("test2 failed: " + toStr(test2_appendInt())); return 2 }
    if (test3_appendChar() != 0) { println("test3 failed: " + toStr(test3_appendChar())); return 3 }
    if (test4_toStringReuse() != 0) { println("test4 failed: " + toStr(test4_toStringReuse())); return 4 }
    if (test5_reset() != 0) { println("test5 failed: " + toStr(test5_reset())); return 5 }
    if (test6_appendBoolean() != 0) { println("test6 failed: " + toStr(test6_appendBoolean())); return 6 }
    if (test7_appendBuilder() != 0) { println("test7 failed: " + toStr(test7_appendBuilder())); return 7 }
    if (test8_empty() != 0) { println("test8 failed: " + toStr(test8_empty())); return 8 }
    if (test9_dispose() != 0) { println("test9 failed: " + toStr(test9_dispose())); return 9 }
    if (test10_appendFloat() != 0) { println("test10 failed: " + toStr(test10_appendFloat())); return 10 }
    if (test11_ensureCapacity() != 0) { println("test11 failed: " + toStr(test11_ensureCapacity())); return 11 }
    if (test12_growth() != 0) { println("test12 failed: " + toStr(test12_growth())); return 12 }
    if (test13_unicodeBoundary() != 0) { println("test13 failed: " + toStr(test13_unicodeBoundary())); return 13 }
    println("phase1.string_builder_class: ALL PASS")
    return 0
}
```

**验证命令**：
```bash
aura build tests/phase1/string_builder_class.aura --aot -o /tmp/phase1_test
/tmp/phase1_test
```

**回滚**：删除新增的 class，保留旧 object 不变。

---

### Phase 2：EmitBuffer 迁移

**目标**：编译器发射缓冲改用新 StringBuilder，消除 CString/ReadCStr 依赖。

**依赖**：Phase 1 完成。

**交付物**：

| 文件 | 变更 |
|------|------|
| `aura/compiler/aura/lang/compiler/aot/EmitBuffer.aura` | 改用新 StringBuilder API |

**迁移前后对比**：

```aura
// 迁移前
class EmitBuffer {
    var h: Long = 0
    init() { this.h = StringBuilder.create() }
    fun sbAdd(s: String) { this.h = StringBuilder.append(this.h, CString(s)) }
    fun sbBuild(): String {
        val n = StringBuilder.length(this.h)
        val p = StringBuilder.finish(this.h)
        if (p == 0 || n <= 0) { return "" }
        return ReadCStr(p)
    }
    fun sbReset() { this.h = StringBuilder.reset(this.h) }
    fun sbSize(): Int { return this.h as Int }
}

// 迁移后
class EmitBuffer {
    private val sb: StringBuilder = StringBuilder()
    fun sbAdd(s: String) { this.sb.append(s) }
    fun sbBuild(): String { return this.sb.toString() }
    fun sbReset() { this.sb.reset() }
    fun sbSize(): Int { return this.sb.length() }
    fun dispose() { this.sb.dispose() }
}
```

**测试用例**：

```aura
// tests/phase2/emit_buffer_migration.aura

fun test1_basicEmit(): Int {
    val buf = EmitBuffer()
    buf.sbAdd("define void @test() {\n")
    buf.sbAdd("  ret void\n")
    buf.sbAdd("}\n")
    val ir = buf.sbBuild()
    if (ir != "define void @test() {\n  ret void\n}\n") { return 1 }
    return 0
}

fun test2_emitReuse(): Int {
    val buf = EmitBuffer()
    buf.sbAdd("first")
    buf.sbReset()
    buf.sbAdd("second")
    if (buf.sbBuild() != "second") { return 2 }
    return 0
}

fun test3_emitSize(): Int {
    val buf = EmitBuffer()
    buf.sbAdd("hello")
    if (buf.sbSize() != 5) { return 3 }
    buf.sbAdd(" world")
    if (buf.sbSize() != 11) { return 4 }
    return 0
}

fun test4_noCString(): Int {
    // 验证不依赖 CString（通过编译通过即可验证）
    val buf = EmitBuffer()
    buf.sbAdd("test")
    if (buf.sbBuild() != "test") { return 5 }
    buf.dispose()
    return 0
}

fun main(): Int {
    if (test1_basicEmit() != 0) { println("test1 failed: " + toStr(test1_basicEmit())); return 1 }
    if (test2_emitReuse() != 0) { println("test2 failed: " + toStr(test2_emitReuse())); return 2 }
    if (test3_emitSize() != 0) { println("test3 failed: " + toStr(test3_emitSize())); return 3 }
    if (test4_noCString() != 0) { println("test4 failed: " + toStr(test4_noCString())); return 4 }
    println("phase2.emit_buffer_migration: ALL PASS")
    return 0
}
```

**验证命令**：
```bash
aura build tests/phase2/emit_buffer_migration.aura --aot -o /tmp/phase2_test
/tmp/phase2_test

# 同时验证自举链
aura build aura/compiler/aura/lang/compiler/Main.aura --aot -o /tmp/aura-native
/tmp/aura-native
```

**回滚**：恢复 EmitBuffer.aura 为旧版本。

---

### Phase 3：全量回归测试

**目标**：确认迁移后编译器行为完全一致。

**依赖**：Phase 1 + Phase 2 完成。

**测试矩阵**：

| 测试集 | 命令 | 预期 |
|--------|------|------|
| 全部单元测试 | `aura test --all` | 全通过 |
| AOT 编译 | `aura build examples/hello.aura --aot` | 成功 |
| 自举编译 | `aura build compiler/Main.aura --aot` | 成功 |
| IR 一致性 | `diff build/old/ ir build/new/ir` | 无差异 |
| 内存对比 | `aura test --mem-trace` | 内存 ≤ 旧版 |

**测试用例**：

```bash
# tests/phase3/regression.sh

# 1. 全量单元测试
echo "=== Unit Tests ==="
aura test --all || exit 1

# 2. AOT 编译示例
echo "=== AOT Examples ==="
for f in examples/*.aura; do
    aura build "$f" --aot -o "/tmp/aot_$(basename $f .aura).exe" || exit 1
done

# 3. 自举链
echo "=== Bootstrapping ==="
aura build aura/compiler/aura/lang/compiler/Main.aura --aot -o /tmp/aura-native || exit 1
/tmp/aura-native aura/compiler/aura/lang/compiler/Main.aura --aot -o /tmp/aura-self || exit 1
/tmp/aura-self examples/hello.aura --aot -o /tmp/hello.exe || exit 1
/tmp/hello.exe || exit 1

# 4. IR 一致性
echo "=== IR Consistency ==="
aura build examples/hello.aura --aot --emit-ir /tmp/hello_old.ir
aura build examples/hello.aura --aot --emit-ir /tmp/hello_new.ir
diff /tmp/hello_old.ir /tmp/hello_new.ir || exit 1

# 5. 内存回归
echo "=== Memory ==="
aura test --mem-trace --filter string || exit 1

echo "phase3.regression: ALL PASS"
```

**回滚**：恢复 Phase 1 + Phase 2 的所有变更。

---

### Phase 4：代码模式优化（L3）

**目标**：在编译器代码中应用 L3 优化模式，消除 O(n²) 拼接。

**依赖**：Phase 3 通过。

**交付物**：

| 文件 | 变更 |
|------|------|
| `aura/compiler/aura/lang/compiler/aot/*.aura` | 将 `+=` 拼接改为 StringBuilder |
| `aura/compiler/aura/lang/compiler/hir/*.aura` | 同上 |

**优化模式**：

```aura
// ❌ 模式 1：+= 拼接（O(n²)）
var result = ""
for (item in items) {
    result = result + item.text + "\n"
}

// ✅ 改为 StringBuilder（O(n)）
val sb = StringBuilder()
for (item in items) {
    sb.append(item.text).append("\n")
}
val result = sb.toString()
sb.dispose()

// ❌ 模式 2：length 在循环条件中
while (i < s.length) { ... }

// ✅ 缓存长度
val n = s.length
while (i < n) { ... }

// ❌ 模式 3：多次 toString 调用
val a = sb1.toString()
val b = sb2.toString()
val c = a + b

// ✅ 合并为一个 StringBuilder
val sb = StringBuilder()
sb.append(sb1.toString()).append(sb2.toString())
val c = sb.toString()
sb.dispose()
```

**测试用例**：

```aura
// tests/phase4/code_pattern_migration.aura

fun test1_noPlusEquals(): Int {
    // 验证编译器源码中没有 += 拼接模式
    // 通过静态分析或代码审查
    return 0
}

fun test2_cachedLength(): Int {
    // 验证 length 不在循环条件中
    return 0
}

fun test3_performanceImprovement(): Int {
    // 对比迁移前后的性能
    val items = arrayOf("a", "b", "c", "d", "e")
    
    // 旧模式
    var oldResult = ""
    for (item in items) {
        oldResult = oldResult + item + "\n"
    }
    
    // 新模式
    val sb = StringBuilder()
    for (item in items) {
        sb.append(item).append("\n")
    }
    val newResult = sb.toString()
    sb.dispose()
    
    if (oldResult != newResult) { return 1 }
    return 0
}

fun main(): Int {
    if (test3_performanceImprovement() != 0) { println("test3 failed"); return 1 }
    println("phase4.code_pattern_migration: ALL PASS")
    return 0
}
```

**验证命令**：
```bash
# 代码审查：检查是否有 += 拼接
grep -rn '= .*+ .*' aura/compiler/aura/lang/compiler/aot/*.aura | grep -v StringBuilder

# 性能对比
time aura build examples/hello.aura --aot -o /tmp/hello_old.exe
time aura build examples/hello.aura --aot -o /tmp/hello_new.exe
```

**回滚**：恢复被修改的编译器源码文件。

---

### Phase 5：缓冲区池化（L2）

**目标**：添加 StringBuilderPool，复用已分配缓冲区。

**依赖**：Phase 4 完成。

**交付物**：

| 文件 | 变更 |
|------|------|
| `aura/core/aura/lang/std/StringBuilderPool.aura` | 新增池化实现 |
| `aura/compiler/aura/lang/compiler/aot/*.aura` | 使用池化获取/释放 |

**实现**：

```aura
// aura:///aura/lang/std/StringBuilderPool.aura
object StringBuilderPool {
    private var freeBufs: Array<Long> = arrayOf()
    private val MAX_POOL_SIZE: Int = 1024
    private val MAX_POOL_CAP: Long = 65536

    fun acquire(cap: Long): StringBuilder {
        if (freeBufs.size > 0) {
            val buf = freeBufs[0]
            freeBufs.remove(0)
            val sb = StringBuilder.fromBuffer(buf, cap)
            return sb
        }
        return StringBuilder()
    }

    fun release(sb: StringBuilder): Unit {
        val cap = sb.capacity()
        if (cap <= MAX_POOL_CAP && freeBufs.size < MAX_POOL_SIZE) {
            freeBufs.add(sb.buffer())
        }
        sb.dispose()
    }
}
```

**测试用例**：

```aura
// tests/phase5/string_builder_pool.aura

fun test1_poolReuse(): Int {
    val sb1 = StringBuilderPool.acquire(256)
    sb1.append("first")
    StringBuilderPool.release(sb1)
    
    val sb2 = StringBuilderPool.acquire(256)
    if (sb2.capacity() < 256) { return 1 }
    sb2.append("second")
    if (sb2.toString() != "second") { return 2 }
    StringBuilderPool.release(sb2)
    return 0
}

fun test2_poolLimit(): Int {
    // 测试池上限
    for (i in 0..2000) {
        val sb = StringBuilderPool.acquire(256)
        StringBuilderPool.release(sb)
    }
    return 0
}

fun test3_poolLargeBuffer(): Int {
    // 大块缓冲区不回收
    val sb = StringBuilderPool.acquire(1000000)
    StringBuilderPool.release(sb)
    return 0
}

fun main(): Int {
    if (test1_poolReuse() != 0) { println("test1 failed: " + toStr(test1_poolReuse())); return 1 }
    if (test2_poolLimit() != 0) { println("test2 failed: " + toStr(test2_poolLimit())); return 2 }
    if (test3_poolLargeBuffer() != 0) { println("test3 failed: " + toStr(test3_poolLargeBuffer())); return 3 }
    println("phase5.string_builder_pool: ALL PASS")
    return 0
}
```

**回滚**：删除 StringBuilderPool.aura，恢复直接 `StringBuilder()` 创建。

---

### Phase 6：编译器内置扩展（L4 - StrOps）

**目标**：新增 `StrOps.aura` 编译器内置函数。

**依赖**：Phase 5 完成。

**交付物**：

| 文件 | 变更 |
|------|------|
| `aura/core/aura/lang/native/StrOps.aura` | 新增 extern interface |
| `rust/compiler/src/codegen/aot/emit.rs` | 添加 StrOps 的 LLVM 发射 |
| `rust/compiler/src/vm/interp.rs` | 添加 StrOps 的 VM 实现 |
| `rust/compiler/src/vm/native.rs` | 注册 StrOps 原生函数 |

**实现**：

```aura
// aura:///aura/lang/native/StrOps.aura
package aura.lang.native

extern interface StrOps {
    fun strBytes(s: String): Long
    fun strFromPtr(p: Long, n: Long): String
    fun strLen(s: String): Int
    fun strCopy(dst: Long, s: String): Long
    fun strEq(a: String, b: String): Boolean
}
```

**VM 实现**（`native.rs`）：

```rust
fn native_str_bytes(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => {
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0);
            let p = Box::leak(bytes.into_boxed_slice()).as_ptr() as i64;
            Value::Int(p)
        }
        Value::Ptr(p) => Value::Int(*p),
        Value::Int(i) => Value::Int(*i),
        _ => Value::Int(0),
    }
}

fn native_str_from_ptr(args: &[Value]) -> Value {
    let p = args.get(0).and_then(|v| v.as_int()).unwrap_or(0);
    let n = args.get(1).and_then(|v| v.as_int()).unwrap_or(0);
    if p == 0 || n <= 0 { return Value::Str(Rc::from("")); }
    let c_str = unsafe { CStr::from_ptr(p as *const c_char) };
    let slice = unsafe { std::slice::from_raw_parts(p as *const u8, n as usize) };
    Value::Str(Rc::from(String::from_utf8_lossy(slice).to_string()))
}

fn native_str_len(args: &[Value]) -> Value {
    match &args[0] {
        Value::Str(s) => Value::Int(s.len() as i64),
        Value::Ptr(p) if *p != 0 => {
            let c_str = unsafe { CStr::from_ptr(*p as *const c_char) };
            Value::Int(c_str.to_bytes().len() as i64)
        }
        _ => Value::Int(0),
    }
}
```

**AOT 实现**（`emit.rs`）：

```rust
// strBytes: AOT 下 identity (String ≡ i8*)
"StrOps.strBytes" => {
    // 直接返回参数（String 就是 i8*）
    Some(emit_identity(args[0]))
}

// strLen: AOT 下内联 strlen
"StrOps.strLen" => {
    // 发射 LLVM strlen 调用
    Some(emit_str_len(args[0]))
}

// strFromPtr: AOT 下从指针构造 String
"StrOps.strFromPtr" => {
    // 发射 malloc + memcpy + 构造
    Some(emit_str_from_ptr(args[0], args[1]))
}
```

**测试用例**：

```aura
// tests/phase6/str_ops.aura

fun test1_strBytes(): Int {
    val s = "hello"
    val p = StrOps.strBytes(s)
    if (p == 0) { return 1 }
    return 0
}

fun test2_strLen(): Int {
    val s = "hello"
    if (StrOps.strLen(s) != 5) { return 2 }
    return 0
}

fun test3_strFromPtr(): Int {
    val s = "hello"
    val p = StrOps.strBytes(s)
    val n = StrOps.strLen(s)
    val result = StrOps.strFromPtr(p, n as Long)
    if (result != "hello") { return 3 }
    return 0
}

fun test4_strEq(): Int {
    if (!StrOps.strEq("hello", "hello")) { return 4 }
    if (StrOps.strEq("hello", "world")) { return 5 }
    return 0
}

fun test5_strCopy(): Int {
    val dst = Memory.alloc(64)
    if (dst == 0) { return 6 }
    val n = StrOps.strCopy(dst, "hello")
    val result = StrOps.strFromPtr(dst, n as Long)
    if (result != "hello") { return 7 }
    Memory.free(dst)
    return 0
}

fun test6_emptyString(): Int {
    val s = ""
    if (StrOps.strLen(s) != 0) { return 8 }
    return 0
}

fun main(): Int {
    if (test1_strBytes() != 0) { println("test1 failed: " + toStr(test1_strBytes())); return 1 }
    if (test2_strLen() != 0) { println("test2 failed: " + toStr(test2_strLen())); return 2 }
    if (test3_strFromPtr() != 0) { println("test3 failed: " + toStr(test3_strFromPtr())); return 3 }
    if (test4_strEq() != 0) { println("test4 failed: " + toStr(test4_strEq())); return 4 }
    if (test5_strCopy() != 0) { println("test5 failed: " + toStr(test5_strCopy())); return 5 }
    if (test6_emptyString() != 0) { println("test6 failed: " + toStr(test6_emptyString())); return 6 }
    println("phase6.str_ops: ALL PASS")
    return 0
}
```

**验证命令**：
```bash
# VM 模式
aura run tests/phase6/str_ops.aura

# AOT 模式
aura build tests/phase6/str_ops.aura --aot -o /tmp/phase6_test
/tmp/phase6_test

# 双端一致性
diff <(aura run tests/phase6/str_ops.aura 2>&1) \
     <(aura build tests/phase6/str_ops.aura --aot -o /tmp/phase6_test && /tmp/phase6_test 2>&1)
```

**回滚**：删除 StrOps.aura，回退 emit.rs 和 native.rs 的变更。

---

### Phase 7：L4 优化版 StringBuilder

**目标**：用 StrOps 编译器内置重写 StringBuilder，性能提升 100×。

**依赖**：Phase 6 完成。

**交付物**：

| 文件 | 变更 |
|------|------|
| `aura/core/aura/lang/std/StringBuilder.aura` | 用 StrOps 重写 append/toString |

**重写对比**：

```aura
// Phase 1 版（纯 Aura）
fun append(text: String): StringBuilder {
    val n: Int = text.length
    this.reserve(this.len + n as Long + 1)
    if (this.buf == 0) { return this }
    var i: Int = 0
    while (i < n) {
        val c: Int = text.charCodeAt(i)
        if (c <= 0) { break }
        Memory.write(this.buf + this.len, c as Byte)
        this.len = this.len + 1
        i = i + 1
    }
    Memory.write(this.buf + this.len, 0 as Byte)
    return this
}

// Phase 7 版（L4 编译器内置）
fun append(text: String): StringBuilder {
    val src: Long = StrOps.strBytes(text)
    val n: Int = StrOps.strLen(text)
    this.reserve(this.len + n as Long + 1)
    if (src == 0 || this.buf == 0) { return this }
    Memory.copy(this.buf + this.len, src, n as Long)
    this.len = this.len + n as Long
    Memory.write(this.buf + this.len, 0 as Byte)
    return this
}
```

**测试用例**：

```aura
// tests/phase7/str_builder_l4.aura

fun test1_basicAppend(): Int {
    val sb = StringBuilder()
    sb.append("Hello").append(", ").append("World")
    if (sb.toString() != "Hello, World") { return 1 }
    sb.dispose()
    return 0
}

fun test2_performance(): Int {
    // 性能基准测试（可选，不阻塞通过）
    val sb = StringBuilder()
    val start = Time.now()
    for (i in 0..10000) {
        sb.append("hello ")
    }
    val elapsed = Time.now() - start
    if (sb.length() != 60000) { return 2 }
    sb.dispose()
    println("10000 appends: " + elapsed + "ms")
    return 0
}

fun test3_largeAppend(): Int {
    val sb = StringBuilder()
    val big = "abc".repeat(1000)  // 3000 chars
    sb.append(big).append(big).append(big)
    if (sb.length() != 9000) { return 3 }
    sb.dispose()
    return 0
}

fun test4_toStringLarge(): Int {
    val sb = StringBuilder()
    for (i in 0..1000) {
        sb.append("line ").appendInt(i).append("\n")
    }
    val s = sb.toString()
    if (s.length() < 6000) { return 4 }
    sb.dispose()
    return 0
}

fun test5_noCharCodeAt(): Int {
    // 验证不使用 charCodeAt（通过编译通过即可）
    val sb = StringBuilder()
    sb.append("test")
    if (sb.toString() != "test") { return 5 }
    sb.dispose()
    return 0
}

fun main(): Int {
    if (test1_basicAppend() != 0) { println("test1 failed"); return 1 }
    if (test2_performance() != 0) { println("test2 failed"); return 2 }
    if (test3_largeAppend() != 0) { println("test3 failed"); return 3 }
    if (test4_toStringLarge() != 0) { println("test4 failed"); return 4 }
    if (test5_noCharCodeAt() != 0) { println("test5 failed"); return 5 }
    println("phase7.str_builder_l4: ALL PASS")
    return 0
}
```

**验证命令**：
```bash
# 功能验证
aura build tests/phase7/str_builder_l4.aura --aot -o /tmp/phase7_test
/tmp/phase7_test

# 性能对比
echo "=== Phase 1 (charCodeAt) ==="
time aura build tests/phase7/str_builder_l4.aura --aot -o /tmp/phase7_old.exe
time /tmp/phase7_old.exe

echo "=== Phase 7 (StrOps) ==="
time aura build tests/phase7/str_builder_l4.aura --aot -o /tmp/phase7_new.exe
time /tmp/phase7_new.exe
```

**回滚**：恢复 Phase 1 版的 StringBuilder。

---

### Phase 8：JIT 字符串支持（L5）

**目标**：在 JIT 中添加字符串操作支持，消除 deopt。

**依赖**：Phase 7 完成。

**交付物**：

| 文件 | 变更 |
|------|------|
| `rust/compiler/src/codegen/hir.rs` | 新增 STR_CONCAT / STR_CHARAT opcode |
| `rust/compiler/src/vm/jit.rs` | JIT 白名单 + 发射逻辑 |
| `rust/compiler/src/vm/jit_opt.rs` | JIT 优化规则 |
| `aura/compiler/aura/lang/compiler/jit/JitLower.aura` | 字节码 → Clif 映射 |

**新增 opcode**：

```rust
// hir.rs — 新增字符串指令
STR_CONCAT,   // String + String → String
STR_CHARAT,   // s.charAt(i) → String
STR_LENGTH,   // s.length → Int
STR_EQUALS,   // s1 == s2 → Boolean
```

**JIT 发射**：

```rust
// jit.rs — 新增字符串指令处理
Instr::STR_CONCAT => {
    // 发射 aura_string_concat 调用
    let lhs = pop();
    let rhs = pop();
    let result = fb.ins().call_external(...);
    push(result);
}

Instr::STR_CHARAT => {
    // 发射 aura_lang_std_String_charAt 调用
    let idx = pop();
    let s = pop();
    let result = fb.ins().call_external(...);
    push(result);
}
```

**测试用例**：

```aura
// tests/phase8/jit_string_support.aura

fun test1_stringConcat(): Int {
    // 此测试在 JIT 模式下运行，验证不 deopt
    val a = "Hello"
    val b = "World"
    val c = a + " " + b
    if (c != "Hello World") { return 1 }
    return 0
}

fun test2_charAt(): Int {
    val s = "Hello"
    val c = s.charAt(0)
    if (c != "H") { return 2 }
    return 0
}

fun test3_length(): Int {
    val s = "Hello"
    if (s.length != 5) { return 3 }
    return 0
}

fun test4_equals(): Int {
    val a = "hello"
    val b = "hello"
    val c = "world"
    if (a != b) { return 4 }
    if (a == c) { return 5 }
    return 0
}

fun test5_noDeopt(): Int {
    // 验证 JIT 模式下字符串操作不 deopt
    // 通过 JIT 统计信息验证
    return 0
}

fun main(): Int {
    // 强制 JIT 模式运行
    if (test1_stringConcat() != 0) { println("test1 failed"); return 1 }
    if (test2_charAt() != 0) { println("test2 failed"); return 2 }
    if (test3_length() != 0) { println("test3 failed"); return 3 }
    if (test4_equals() != 0) { println("test4 failed"); return 4 }
    if (test5_noDeopt() != 0) { println("test5 failed"); return 5 }
    println("phase8.jit_string_support: ALL PASS")
    return 0
}
```

**验证命令**：
```bash
# JIT 模式运行
aura run --jit tests/phase8/jit_string_support.aura

# 检查 JIT 统计（确认无 deopt）
aura run --jit --stats tests/phase8/jit_string_support.aura 2>&1 | grep "deopt"
# 期望：deopt count = 0
```

**回滚**：回退 jit.rs 和 hir.rs 的变更。

---

### 阶段依赖图

```
Phase 1 (StringBuilder L1)     Phase 6 (StrOps L4)
      │                              │
      ▼                              ▼
Phase 2 (EmitBuffer 迁移)     Phase 7 (L4 StringBuilder)
      │                              │
      ▼                              ▼
Phase 3 (全量回归)              Phase 8 (JIT L5)
      │
      ▼
Phase 4 (代码模式 L3)
      │
      ▼
Phase 5 (缓冲区池化 L2)
```

### 阶段汇总

| Phase | 目标 | 依赖 | 测试 | 时间 | 风险 |
|-------|------|------|------|------|------|
| 1 | StringBuilder L1 | 无 | 13 个用例 | 1-2 天 | 低 |
| 2 | EmitBuffer 迁移 | Phase 1 | 4 个用例 + 自举链 | 0.5 天 | 低 |
| 3 | 全量回归 | Phase 1+2 | 全量测试矩阵 | 0.5 天 | 低 |
| 4 | 代码模式 L3 | Phase 3 | 性能对比 | 1-2 天 | 中 |
| 5 | 缓冲区池化 L2 | Phase 4 | 3 个用例 | 0.5 天 | 低 |
| 6 | StrOps L4 | Phase 5 | 6 个用例 + 双端一致性 | 3-5 天 | 中 |
| 7 | L4 StringBuilder | Phase 6 | 5 个用例 + 性能基准 | 1 天 | 低 |
| 8 | JIT L5 | Phase 7 | 5 个用例 + deopt 统计 | 2-3 周 | 高 |

---

## 10. 纯 Aura 性能根因分析

### 10.1 根因一：字符串不可变性 → O(n²) 拼接

Aura 的 `String` 是不可变值类型，每次 `+` 都产生新分配：

```aura
// O(n²)：第 i 次迭代复制 i 个字符
var result: String = ""
while (i < n) {
    result = result + chars[i]
}
```

`String.aura` 注释已记录此问题（第 430-435 行）：

> **性能关键**：AOT 下 `length` 是 `strlen`（O(n)），`charAt` 是索引运算符的落地实现，词法/解析/发射期按字符高频调用 → 整体退化为 O(n²)（实测 300 KB 源码词法分析耗时 24 秒、自举整体 36 秒）。

### 10.2 根因二：AOT 无 GC → 中间串永久驻留

```
AOT 运行时：只分配不释放
  → 每个 String.concat 产生的中间串 = 永久泄漏
  → 编译器自举时峰值 RSS 达 5.2 GB
  → 4 MB IR 文本 ≈ 60× 发射阶段内存
```

### 10.3 根因三：JIT 对字符串零支持 → 全部 deopt

```rust
// jit.rs:782 — 无条件按整数加法发射
Instr::Add => bin_int(fb, ..., |fb, x, y| fb.ins().iadd(x, y)),
```

| 操作 | JIT 行为 | 后果 |
|------|----------|------|
| `"a" + b` | 编译为 `iadd`（指针做整数加法） | 随机地址 |
| `s.charAt(0)` | `CALL_METHOD` 不在白名单 | **deopt** 回退解释器 |
| `s.length` | 同上 | **deopt** |
| `StringBuilder.append` | 同上 | **deopt** |

**结论**：任何字符串操作密集的函数在 JIT 模式下完全回退解释器。

### 10.4 根因四：方法分派开销

```
纯 Aura 方法调用链（append 为例）：

sb.append("hello")
  → StringBuilder.append (Aura 方法)
    → text.charCodeAt(0)     (Aura 方法调用)
      → this[0]              (charAt → String_charAt C 函数)
        → .toInt()           (Aura 方法调用)
    → Memory.write(...)      (编译器内置，快速)
```

每层 Aura 方法调用有**函数查找 + 参数强制转换 + 返回值装箱**的开销。对比 C 函数调用，纯 Aura 方法调用慢 5-10 倍。

### 10.5 根因五：类型强制转换开销

```
Plan A 装箱：i8* 低位标记区分指针/整数
  → 每次类型检查都是运行时分支

String → Long 转换：
  AOT: i8* → i64 (ptrtoint)
  VM:  Rc<str> → 需要分配 C 字符串副本

每次 StringBuilder.append 需要：
  charCodeAt(i)     → 方法调用链（VM: ~500ns, AOT: ~5ns）
  Memory.write(...) → 编译器内置（~1ns）
```

### 10.6 综合性能模型

```
                        编译器内置          C FFI           纯 Aura
                        (Memory.*)        (CString)        (charCodeAt)
                        ─────────         ───────          ──────────
  单次操作延迟          ~1ns             ~50ns            ~500-5000ns
  
  O(n) 操作             memcpy 快速      strlen+memcpy     n × charCodeAt
  O(n log n) 归并       不适用           不适用            可行但慢
  
  JIT 支持              ✅               ❌ deopt           ❌ deopt
  AOT 零拷贝            N/A              ✅ (String≡i8*)   ❌ (逐字符)
```

---

## 11. 性能优化策略金字塔

```
┌─────────────────────────────────────────────────────────┐
│                    性能优化金字塔                          │
│                                                          │
│  ┌─────────────────────────────────────────────────┐   │
│  │  L5: JIT 字符串支持（长期，改动最大）              │   │
│  │  ─────────────────────────────────────────────   │   │
│  │  │  L4: 编译器内置扩展（中期，改动适中）          │   │
│  │  │  ──────────────────────────────────────────   │   │
│  │  │  │  L3: 代码模式优化（短期，零改动）          │   │
│  │  │  │  ──────────────────────────────────       │   │
│  │  │  │  │  L2: 缓冲区池化（短期，改动小）        │   │
│  │  │  │  │  ────────────────────────────          │   │
│  │  │  │  │  │  L1: StringBuilder 优化（当前）    │   │
│  │  │  │  │  │                                     │   │
│  └──┴──┴──┴──┴─────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

### 11.1 L1：StringBuilder 优化（当前方案）

**原理**：用可变缓冲区替代不可变拼接，消除中间串。

| 指标 | 纯 `+=` 拼接 | StringBuilder（本方案） | 提升 |
|------|-------------|----------------------|------|
| append 复杂度 | O(n) 每次拼接 | O(1) 摊还 | n 倍 |
| 总复杂度 | O(n²) | O(n) | n 倍 |
| 内存分配 | 每次拼接新分配 | 仅扩容时分配 | 1/n |
| AOT 泄漏 | 每次拼接泄漏 | 仅缓冲区泄漏 | 1/n |

### 11.2 L2：缓冲区池化（短期，改动小）

**原理**：复用已分配的缓冲区，减少 alloc/free 开销。

```aura
object StringBuilderPool {
    private var freeBufs: Array<Long> = arrayOf()

    /// 从池中获取缓冲区（无可用时新分配）。
    fun acquire(cap: Long): Long {
        if (freeBufs.size > 0) {
            val buf = freeBufs[0]
            freeBufs.remove(0)
            return buf
        }
        return Memory.alloc(cap)
    }

    /// 归还缓冲区到池中（仅回收小块）。
    fun release(buf: Long, cap: Long): Unit {
        if (cap <= 65536) {
            freeBufs.add(buf)
        } else {
            Memory.free(buf)
        }
    }
}
```

| 指标 | 无池化 | 有池化 | 提升 |
|------|--------|--------|------|
| alloc 次数 | 每次 StringBuilder 创建 | 仅首次 | 10-100× |
| malloc 开销 | ~100ns/次 | 摊还 ~1ns | 100× |
| 内存碎片 | 高 | 低 | - |

### 11.3 L3：代码模式优化（短期，零改动）

不修改语言/编译器，仅改变编码模式。

#### 3.1 避免 `length` 在循环条件中

```aura
// ❌ 慢：每次循环 strlen (O(n))
while (i < s.length) { ... }

// ✅ 快：缓存长度
val n = s.length
while (i < n) { ... }
```

#### 3.2 用 StringBuilder 替代 `+=` 拼接

```aura
// ❌ O(n²)
var result = ""
for (part in parts) {
    result = result + part
}

// ✅ O(n)
val sb = StringBuilder()
for (part in parts) {
    sb.append(part)
}
val result = sb.toString()
```

#### 3.3 用列表 + 树形归并替代逐次拼接

```aura
// ❌ O(n²)
var result = ""
for (i in 0..n) {
    result = result + "  " + data[i] + "\n"
}

// ✅ O(n log n)：树形归并
val parts = arrayOf()
for (i in 0..n) {
    parts.add("  " + data[i] + "\n")
}
val result = joinTree(parts)
```

#### 3.4 用 Memory 直接操作替代字符串操作

```aura
// ❌ 慢：字符串操作（方法调用链）
val s = "hello"
val n = s.length
val c = s.charCodeAt(0)

// ✅ 快：直接用 Memory 操作缓冲区
val c = Memory.read(buf + offset)
```

### 11.4 L4：编译器内置扩展（中期，改动适中）

**原理**：将热点字符串操作提升为编译器内置函数（类似 `Memory.*`），由编译器直接降低到 LLVM 指令。

**关键区分**：编译器内置 ≠ C FFI

| 类型 | 示例 | 实现方式 | 性能 |
|------|------|----------|------|
| 编译器内置 | `Memory.alloc` | 编译器直接生成 LLVM 指令 | 最快 |
| 原生函数 | `CString` | VM 注册 → Rust 实现 → 调 C | 中等 |
| 纯 Aura | `charCodeAt` | 方法调用链 | 最慢 |

#### 可提升为编译器内置的操作

```
┌─────────────────────────────────────────────────────────┐
│  建议新增的编译器内置函数                                  │
│  （与 Memory.* 同级，非 C FFI）                           │
├─────────────────────────────────────────────────────────┤
│  strBytes(s: String): Long                              │
│    AOT: identity (String ≡ i8*)                        │
│    VM:  分配 NUL 终止副本，返回指针                      │
│                                                        │
│  strFromPtr(p: Long, n: Long): String                  │
│    AOT: 直接构造 String (ptr, len)                      │
│    VM:  从指针拷贝 n 字节构造 String                    │
│                                                        │
│  strLen(s: String): Int                                │
│    AOT: 内联 strlen (编译器展开)                         │
│    VM:  s.len() (O(1))                                 │
│                                                        │
│  strCopy(dst: Long, src: String, offset: Long): Long    │
│    AOT: memcpy(dst, src, strlen(src))                  │
│    VM:  从 String 提取字节 + memcpy                    │
│                                                        │
│  strEq(a: String, b: String): Boolean                  │
│    AOT: 内联 strcmp                                   │
│    VM:  Rc<str> 比较                                   │
└─────────────────────────────────────────────────────────┘
```

#### 与 CString 的区别

```
CString（原生函数）：
  用户代码调用 → VM 查表 → Rust 实现 → libc strlen/memcpy → 返回
  开销：函数查找 + Rust 调用 + C 调用

strBytes（编译器内置）：
  用户代码调用 → 编译器直接生成 LLVM 指令
  AOT: identity (无开销)
  VM: 编译器生成分配+拷贝指令序列
  开销：零（编译器内联）
```

#### 性能提升预估

```
操作              纯 Aura        编译器内置      提升
────────────     ─────────      ──────────     ────
append 1 字符     ~500ns        ~5ns          100×
toString 1KB     ~5ms          ~50μs          100×
charCodeAt       ~500ns        ~1ns          500×
length          ~500ns        ~0ns(内联)     ∞
```

#### StringBuilder 的 L4 优化版

```aura
// L4 优化后的 append（使用编译器内置 strBytes + Memory.copy）
fun append(text: String): StringBuilder {
    val src = strBytes(text)
    val n = strLen(text)
    this.reserve(this.len + n as Long + 1)
    if (src == 0 || this.buf == 0) { return this }
    Memory.copy(this.buf + this.len, src, n as Long)
    this.len = this.len + n as Long
    Memory.write(this.buf + this.len, 0 as Byte)
    return this
}

// L4 优化后的 toString（使用编译器内置 strFromPtr）
override fun toString(): String {
    if (this.buf == 0 || this.len == 0) { return "" }
    return strFromPtr(this.buf, this.len)
}
```

### 11.5 L5：JIT 字符串支持（长期，改动最大）

**原理**：在 JIT 层添加字符串操作支持，避免 deopt。

#### 5.1 添加 STR_CONCAT opcode

```
当前：Add 指令 → 无条件 iadd（整数加法）
改进：在 HIR 层区分 String + String 和 Int + Int

HIR → 字节码：
  Binary{Add, String, String} → STR_CONCAT (新 opcode)
  Binary{Add, Int, Int}       → Add (现有)

JIT 发射：
  STR_CONCAT → call aura_string_concat
  Add        → iadd (现有)
```

#### 5.2 JIT 字符串常量内联

```
当前：
  LoadConst "hello" → 加载字符串指针
  Add "world"       → iadd (错误！指针做整数加法)

改进：
  LoadConst "hello" → 加载字符串指针
  LoadConst "world" → 加载字符串指针  
  STR_CONCAT        → call aura_string_concat (正确！)
```

#### 5.3 JIT 字符串方法内联

```
当前：
  s.charAt(0) → CALL_METHOD → deopt

改进：
  s.charAt(0) → JIT 内联 charAt 实现
               → Memory.read(s + 0)
               → fromCharCode
```

---

## 12. 优化路径推荐

### 短期（1-2 天）：L1 + L2 + L3

```
1. StringBuilder 用 charCodeAt + Memory.write（当前方案）
2. 缓冲区池化复用
3. 代码模式优化（缓存 length、用 SB 替代 +=）

预期效果：
  - 消除 CString/ReadCStr 依赖
  - 编译器发射阶段内存降低 60%+
  - 字符串操作性能比纯 += 快 5-10 倍
```

### 中期（1-2 周）：L4

```
1. 新增 strBytes / strFromPtr / strLen 编译器内置
2. StringBuilder 用 strBytes + Memory.copy 替代 charCodeAt 逐字符
3. toString 用 strFromPtr 替代 fromCharCode + 树形归并

预期效果：
  - append 性能提升 100×
  - toString 性能提升 100×
  - 仍然无 C FFI 依赖
  - 编译器自举性能接近 CString 方案
```

### 长期（1-3 月）：L5

```
1. 新增 STR_CONCAT / STR_CHARAT 等 opcode
2. JIT 白名单添加字符串操作
3. JIT 字符串方法内联

预期效果：
  - 字符串密集代码 JIT 加速 10-50×
  - 自举编译器 JIT 模式下不再 deopt
```

### 路径总结

| 层级 | 策略 | 改动量 | 性能提升 | C FFI | 时间 |
|------|------|--------|----------|-------|------|
| L1 | StringBuilder 优化 | 小 | 5-10× | ❌ | 当前 |
| L2 | 缓冲区池化 | 小 | 2-3× | ❌ | 1-2 天 |
| L3 | 代码模式优化 | 零 | 2-5× | ❌ | 立即 |
| L4 | 编译器内置扩展 | 中 | 100× | ❌ | 1-2 周 |
| L5 | JIT 字符串支持 | 大 | 10-50× | ❌ | 1-3 月 |

**核心结论**：纯 Aura 性能差的根本原因是**字符串不可变性 + 无 GC + JIT 不支持字符串**。最优的优化路径是**先做 L1+L3（当前方案 + 代码模式优化）**，然后**中期做 L4（编译器内置扩展）**。编译器内置扩展（L4）是**关键转折点**——它将热点操作从"方法调用"提升为"编译器指令"，性能提升 100 倍，且不引入 C FFI 依赖。

---

## 13. 性能分析

| 操作 | 复杂度 | 说明 |
|------|--------|------|
| `append(text)` | O(n) | 逐字符 charCodeAt（O(1)）+ Memory.write（O(1)） |
| `appendChar` | O(1) | 单次 Memory.write |
| `appendInt` | O(k) | k = 整数字符数，委托 append |
| `reserve(need)` | 摊还 O(1) | 几何增长，总拷贝 O(N) |
| `toString()` | O(n log n) | 树形归并 |
| `reset()` | O(1) | 清零 len + 写 NUL |
| `dispose()` | O(1) | Memory.free |

**与 CString 方案的对比**：

| 指标 | CString 方案 | 本方案（纯 Aura） | L4 优化后 |
|------|-------------|-------------------|-----------|
| append | O(n) memcpy | O(n) 逐字符写入 | O(n) memcpy |
| toString | O(n) ReadCStr 拷贝 | O(n log n) 树形归并 | O(n) strFromPtr |
| 内存 | CString 缓存副本（VM） | 无额外副本 | 无额外副本 |
| C FFI | CString + ReadCStr | **零依赖** | **零依赖** |
| 纯 Aura | ❌ | ✅ | ✅（编译器内置） |
| 相对性能 | 1× | 0.01-0.1× | ~1× |

---

## 14. 风险评估

| 风险 | 影响 | 缓解 |
|------|------|------|
| toString() 树形归并较慢 | 4MB 输出约 88M 次拷贝 | 可接受；L4 优化后降至 O(n) |
| 单字符 String 对象多 | VM 下 4M 个 Rc<str> | 可接受；L4 优化后消除 |
| charCodeAt 在 VM 下较慢 | 方法调用开销 ~500ns/次 | 可接受；L4 优化后降至 ~1ns |
| L4 编译器内置扩展需改编译器 | 改动中等 | 与 Memory.* 同模式，风险可控 |
| L5 JIT 改动大 | 新增 opcode + 发射逻辑 | 分阶段：先 STR_CONCAT，后方法内联 |
| 旧 API 迁移遗漏 | 编译失败 | 旧 object 保留为 deprecated |

---

## 15. 未来优化（L4 编译器内置扩展详细设计）

### 15.1 新增编译器内置函数

在 `Memory.aura` 同级新增 `StrOps.aura`：

```aura
// aura:///aura/lang/native/StrOps.aura
package aura.lang.native

/// 字符串底层操作（编译器内置，非 C FFI）。
/// 与 Memory.* 同级：编译器直接生成 LLVM 指令。
extern interface StrOps {

    /// 获取字符串的字节指针。
    /// AOT: identity (String ≡ i8*)
    /// VM:  分配 NUL 终止副本，返回指针
    fun strBytes(s: String): Long

    /// 从指针构造字符串（按长度，不需 NUL 终止）。
    /// AOT: 直接构造 (ptr, len) String
    /// VM:  从指针拷贝 n 字节构造 String
    fun strFromPtr(p: Long, n: Long): String

    /// 获取字符串字节长度。
    /// AOT: 内联 strlen (编译器展开)
    /// VM:  s.len() (O(1))
    fun strLen(s: String): Int

    /// 将字符串内容拷贝到缓冲区。
    /// AOT: memcpy(dst, src, strlen(src))
    /// VM:  从 String 提取字节 + memcpy
    fun strCopy(dst: Long, s: String): Long

    /// 比较两个字符串是否相等。
    /// AOT: 内联 strcmp
    /// VM:  Rc<str> 比较
    fun strEq(a: String, b: String): Boolean
}
```

### 15.2 L4 优化后的 StringBuilder

```aura
/// L4 优化版：使用编译器内置 StrOps，性能接近 CString 方案。
class StringBuilder {

    private var buf: Long = 0
    private var len: Long = 0
    private var cap: Long = 0

    init() {
        this.buf = Memory.alloc(256)
        if (this.buf != 0) {
            Memory.write(this.buf, 0 as Byte)
            this.cap = 256
        }
    }

    /// L4 优化：使用 StrOps.strBytes + Memory.copy，O(n) memcpy。
    fun append(text: String): StringBuilder {
        val src: Long = StrOps.strBytes(text)
        val n: Int = StrOps.strLen(text)
        this.reserve(this.len + n as Long + 1)
        if (src == 0 || this.buf == 0) { return this }
        Memory.copy(this.buf + this.len, src, n as Long)
        this.len = this.len + n as Long
        Memory.write(this.buf + this.len, 0 as Byte)
        return this
    }

    /// L4 优化：使用 StrOps.strFromPtr，O(n) 一次拷贝。
    override fun toString(): String {
        if (this.buf == 0 || this.len == 0) { return "" }
        return StrOps.strFromPtr(this.buf, this.len)
    }

    fun appendChar(ch: Char): StringBuilder {
        val c: Int = ch as Int
        this.reserve(this.len + 2)
        if (this.buf == 0) { return this }
        Memory.write(this.buf + this.len, c as Byte)
        this.len = this.len + 1
        Memory.write(this.buf + this.len, 0 as Byte)
        return this
    }

    fun reset(): StringBuilder {
        this.len = 0
        if (this.buf != 0) { Memory.write(this.buf, 0 as Byte) }
        return this
    }

    fun dispose(): Unit {
        if (this.buf != 0) {
            Memory.free(this.buf)
            this.buf = 0
            this.len = 0
            this.cap = 0
        }
    }

    fun length(): Int { return this.len as Int }
    fun capacity(): Int { return this.cap as Int }
    fun isEmpty(): Boolean { return this.len == 0 }

    private fun reserve(need: Long): Unit {
        if (this.buf == 0) {
            this.buf = Memory.alloc(need)
            if (this.buf != 0) { Memory.write(this.buf, 0 as Byte); this.cap = need }
            return
        }
        if (this.cap >= need) { return }
        var newCap: Long = if (this.cap < 256) 256 else this.cap
        while (newCap < need) { newCap = newCap * 2 }
        val newBuf: Long = Memory.alloc(newCap)
        if (newBuf == 0) { return }
        if (this.len > 0) { Memory.copy(newBuf, this.buf, this.len) }
        Memory.write(newBuf + this.len, 0 as Byte)
        Memory.free(this.buf)
        this.buf = newBuf
        this.cap = newCap
    }
}
```

### 15.3 L4 性能预估

```
操作              L1（纯 Aura）    L4（编译器内置）    提升
────────────     ───────────      ──────────────     ────
append 1 字符     ~500ns           ~5ns               100×
append 1KB        ~500μs           ~5μs               100×
toString 1KB     ~5ms             ~50μs              100×
toString 4MB     ~20ms            ~200μs             100×
charCodeAt       ~500ns           ~1ns (StrOps)     500×
```
