# Native 层封装与 `extern interface` 统一方案

> **状态**：设计方案 | **优先级**：P0 | **前置**：当前 `aura/core/aura/lang/` 纯 Aura 实现已可自举
>
> **核心决策**：
> 1. 放弃 Phantom Source Tree 方案（过时），以现有纯 Aura 源码为真相源
> 2. 保留 `aura.lang.native` 实现层，但用户不可直接 import
> 3. 统一 `extern object` → `extern interface`
> 4. `extern interface` 是**统一 FFI 机制**：编译器内置 native + 用户 C FFI + 用户 Aura AOT FFI 共用同一语法
> 5. **`extern interface` 是纯声明边界**：仅 `default fun loadLibrary()` 允许函数体，其余实现全部移到 `object`
> 6. std 层采用**混合模式**：无状态用 `object`，有状态用 `class`（面向对象）
> 7. 仅给用户暴露 `aura.lang.std.*`，不允许 import `aura.lang.native.*`

---

## 一、现状问题

### 1.1 `extern object` 的设计错误

`extern object` 让编译器将**全部成员**按外部 C 符号处理（只 `declare`、不发射函数体），导致三类问题：

| 问题 | 示例 | 后果 |
|------|------|------|
| Aura 实现被丢弃 | `Console.intToStr()`、`Clock.now()`、`ProcessOps.exit()` | AOT 报 `use of undefined value` |
| 外部 C 声明缺 `@native` 注解 | `ProcessOps.aura_process_argCount()` | 符号悬空为未定义 |
| 接收者参数错位 | `ProcessOps.aura_process_arg(i)` | C 侧参数槽错位，恒返回 argv[0] |

### 1.2 根因分析

`extern object` 的根本错误在于**编译器无法区分声明与实现**：它把所有成员一律当作外部 C 符号处理。

正确的设计是 `extern interface`：**纯声明边界**，编译器行为确定性：

| 成员形式 | 编译器行为 | 是否允许 |
|---------|-----------|---------|
| `const val X: T = v` | 编译期常量折叠 | ✅ |
| `fun f(): R`（无函数体） | LLVM `declare` | ✅ |
| `@native(N) fun f(): R`（无函数体） | 包装器 + `declare` | ✅ 仅编译器内部 |
| `default fun loadLibrary() = "x"` | Aura 函数（返回常量） | ✅ 仅 Aura AOT FFI |
| `var X: T = v` | — | ❌ 移到 object |
| `fun f() { body }`（非 loadLibrary） | — | ❌ 移到 object |

**核心规则**：`extern interface` 内**仅 `default fun loadLibrary()` 允许函数体**，其余所有 `fun` 必须无函数体（纯声明）。所有实现代码、状态变量、默认包装全部移到 `object` 块。

### 1.3 受影响的文件清单

| 文件 | 声明类型 | Aura 实现 | 外部 C 声明 | 迁移方式 |
|------|---------|----------|------------|---------|
| `Memory.aura` | `extern object` | ❌ | ❌ | 改关键词即可 |
| `Cpu.aura` | `extern object` | ❌ | ❌ | 改关键词即可 |
| `ThreadOps.aura` | `extern object` | ❌ | ❌ | 改关键词即可 |
| `Syscalls.aura` | `object` | ❌ | ❌ | `object SyscallsUtils` → `extern interface Syscalls`（val → const val） |
| `Console.aura` | `extern object` | ✅ | ❌ | 拆分：`extern interface Console` + `object ConsoleImpl` |
| `FileOps.aura` | `extern object` | ✅ | ❌ | 拆分：`extern interface FileOps` + `object FileOpsImpl` |
| `Runtime.aura` | `extern object` | ✅ | ❌ | 拆分：`extern interface Runtime` + `object RuntimeImpl` |
| `Clock.aura` | `extern object` | ✅ | ❌ | 拆分：`extern interface Clock` + `object ClockImpl` |
| `ProcessOps.aura` | `extern object` | ✅ | ✅ | 拆分：`extern interface ProcessOps` + `object ProcessOpsImpl` + `extern interface "runtime"` |
| `NetworkOps.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式（`object` + 成员级 `@native`） |
| `ProcessNative.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式 |
| `Stdio.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式 |
| `EnvOps.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式 |
| `Allocator.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式 |
| `MathOps.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式 |
| `PlanA.aura` | `object` ✅ | ✅ | ❌ | 已是正确模式 |

**结论**：
- 4 个纯声明文件（Memory/Cpu/ThreadOps/Syscalls）仅需改关键词
- 5 个混合文件（Console/FileOps/Runtime/Clock/ProcessOps）需拆分：声明留在 `extern interface`，实现移到 `object`
- 6 个文件已是正确模式，无需改动

---

## 二、设计原则

### 2.1 `extern interface` 是纯声明边界

`extern interface` 是 Aura 语言的**统一 FFI 声明机制**。它是**纯声明边界**，只有一个例外：

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         extern interface                                        │
│                                                                                │
│  ┌─ 编译器内部（保留名 + aura.lang.native）──────────────────────────────────┐  │
│  │                                                                            │  │
│  │  保留名接口：编译器自动识别，仅允许声明                                    │  │
│  │  extern interface Memory    →  LLVM load/store intrinsic                  │  │
│  │  extern interface Cpu       →  LLVM inline asm                           │  │
│  │  extern interface Syscalls  →  aura_syscall_dispatch()                    │  │
│  │  extern interface Runtime   →  arc_inc/dec, coroutine_yield               │  │
│  │  extern interface ThreadOps →  aura_thread_create/join/sleep              │  │
│  │                                                                            │  │
│  │  native 包接口：仅允许声明 + 常量，实现移到 object                        │  │
│  │  extern interface Console       →  @native(SYS_WRITE) writeStdout         │  │
│  │  extern interface FileOps      →  @native(SYS_OPEN/READ/WRITE/CLOSE/...)  │  │
│  │  extern interface Clock        →  @native(SYS_CLOCK_GETTIME) clockGettime │  │
│  │  extern interface ProcessOps   →  @native(SYS_EXIT/WAIT/FORK/...)         │  │
│  │                                                                            │  │
│  │  允许的成员：                                                              │  │
│  │  - const val X: T = v            → 编译期常量                             │  │
│  │  - fun f(params): RetType         → 纯声明                                 │  │
│  │  - @native(N) fun f(params): R    → 带注解的纯声明                         │  │
│  │                                                                            │  │
│  │  不允许的成员：                                                            │  │
│  │  - var X: T = v                  → 移到 object                            │  │
│  │  - fun f(params) { body }        → 移到 object                            │  │
│  │  - default fun loadLibrary()     → 不适用（编译器内部无 Aura AOT FFI）    │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                                                                │
│  ┌─ 用户 FFI（字符串名）────────────────────────────────────────────────────┐  │
│  │                                                                            │  │
│  │  extern interface "raylib"    { ... }     → C FFI                         │  │
│  │  extern interface "sqlite3"   { ... }     → C FFI                         │  │
│  │  extern interface "aura_image" { ... }   → Aura AOT FFI                   │  │
│  │                                                                            │  │
│  │  允许的成员：                                                              │  │
│  │  - const val X: T = v            → 编译期常量                             │  │
│  │  - fun f(params): RetType         → 纯声明                                 │  │
│  │  - default fun loadLibrary() = "x" → Aura AOT FFI 库名（唯一允许函数体的方法） │  │
│  │                                                                            │  │
│  │  不允许的成员：                                                            │  │
│  │  - var X: T = v                  → 移到 object                            │  │
│  │  - fun f(params) { body }        → 移到 object（除 loadLibrary 外）       │  │
│  │  - @native 注解                   → sema 报错                              │  │
│  │  - 保留名（Memory/Cpu/Syscalls/Runtime/ThreadOps）→ sema 报错             │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                                                                │
├──────────────────────────────────────────────────────────────────────────────┤
│  唯一例外：                                                                     │
│  default fun loadLibrary(): String = "xxx"                                    │
│  — 仅用于 Aura AOT FFI，标识动态库名。                                         │
│  其他所有 fun 必须无函数体（纯声明），实现移到 object 块。                      │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 成员分类与编译器行为

#### 编译器内部接口（保留名 + aura.lang.native）

| 成员形式 | 示例 | 编译器行为 | 允许 |
|---------|------|-----------|------|
| `const val X: T = v` | `const val O_RDONLY: Int = 0` | 编译期常量折叠 | ✅ |
| `fun f(): R` | `fun writeStdout(buf: Long, c: Long): Long` | LLVM `declare` | ✅ |
| `@native(N) fun f(): R` | `@native(SYS_WRITE) fun write(...)` | 包装器 + `declare` | ✅ |
| `var X: T = v` | `var tsBuf: Long = 0` | — | ❌ 移到 object |
| `fun f() { body }` | `fun print(msg: Long) { ... }` | — | ❌ 移到 object |
| `default fun loadLibrary()` | — | — | ❌ 不适用 |

#### 用户接口（字符串名 CFFI + Aura FFI）

| 成员形式 | 示例 | 编译器行为 | 允许 |
|---------|------|-----------|------|
| `const val X: T = v` | `const val WHITE: Int = 0xFFFFFFFF` | 编译期常量折叠 | ✅ |
| `fun f(): R` | `fun InitWindow(w: Int, h: Int, t: CString): Unit` | LLVM `declare` | ✅ |
| `default fun loadLibrary() = "x"` | `default fun loadLibrary() = "aura_image"` | Aura 函数 | ✅ 唯一例外 |
| `var X: T = v` | `var bufferSize: Int = 1024` | — | ❌ 移到 object |
| `fun f() { body }` | `fun clearBackground(color: Int) { ... }` | — | ❌ 移到 object |
| `@native(N) fun f()` | — | — | ❌ sema 报错 |

### 2.3 与 Java/Kotlin 接口的类比

| 特性 | Java `interface` | Kotlin `interface` | Aura `extern interface` |
|------|-----------------|-------------------|------------------------|
| 常量 | ✅ `int CONST = 0` | ✅ `companion object { const val ... }` | ✅ `const val X: T = v` |
| 纯声明 | ✅ `void f()` | ✅ `fun f()`（无函数体） | ✅ `fun f(): R`（无函数体） |
| 默认实现 | ✅ `default void f() { ... }` | ✅ `fun f() { ... }` | ❌ 移到 `object` |
| 状态变量 | ❌ 不支持 | ❌ 不支持 | ❌ 移到 `object` |
| 固定标记方法 | — | — | ✅ `default fun loadLibrary()`（唯一允许函数体） |

Aura 的 `extern interface` 比 Java/Kotlin 更严格：**不允许任何默认实现和状态变量**，仅允许纯声明 + 常量 + 一个固定标记方法。

### 2.4 用户 FFI 示例

#### 用户 C FFI（纯声明，无实现）

```aura
// ═══════════════════════════════════════════════════════
// 用户 C FFI：仅声明 + 常量
// ═══════════════════════════════════════════════════════

// 调用 raylib 图形库（仅声明）
extern interface "raylib" {
    const val WHITE: Int = 0xFFFFFFFF
    const val BLACK: Int = 0xFF000000
    const val RED: Int = 0xFFFF0000
    const val GREEN: Int = 0x00FF0000
    const val BLUE: Int = 0x0000FF00
    const val DEFAULT_FONT_SIZE: Int = 10

    fun InitWindow(width: Int, height: Int, title: CString): Unit
    fun BeginDrawing(): Unit
    fun EndDrawing(): Unit
    fun ClearBackground(r: Int, g: Int, b: Int, a: Int): Unit
    fun DrawText(text: CString, x: Int, y: Int, fontSize: Int, color: Int): Unit
    fun IsWindowClosed(): Boolean
    fun CloseWindow(): Unit
    fun GetScreenWidth(): Int
    fun GetScreenHeight(): Int
}

// 调用 sqlite3 数据库（仅声明）
extern interface "sqlite3" {
    const val SQLITE_OK: Int = 0
    const val SQLITE_ROW: Int = 100
    const val SQLITE_DONE: Int = 101
    const val SQLITE_ERROR: Int = 1

    fun sqlite3_open(path: CString, db: Long): Int
    fun sqlite3_close(db: Long): Int
    fun sqlite3_prepare_v2(db: Long, sql: CString, n: Int, stmt: Long, tail: Long): Int
    fun sqlite3_step(stmt: Long): Int
    fun sqlite3_column_text(stmt: Long, col: Int): Long
    fun sqlite3_finalize(stmt: Long): Int
    fun sqlite3_errmsg(db: Long): CString
}

// ═══════════════════════════════════════════════════════
// 用户 C FFI 实现层：默认包装移到 object
// ═══════════════════════════════════════════════════════

// raylib 高层包装
object RaylibWrapper {
    fun clearBackground(color: Int) {
        val r: Int = (color shr 16) and 255
        val g: Int = (color shr 8) and 255
        val b: Int = color and 255
        Raylib.ClearBackground(r, g, b, 255)
    }

    fun drawCenteredText(text: CString, color: Int) {
        val x: Int = (Raylib.GetScreenWidth() - 100) / 2
        val y: Int = (Raylib.GetScreenHeight() - Raylib.DEFAULT_FONT_SIZE) / 2
        Raylib.DrawText(text, x, y, Raylib.DEFAULT_FONT_SIZE, color)
    }

    fun runGameLoop() {
        Raylib.InitWindow(800, 600, "Game")
        while (!Raylib.IsWindowClosed()) {
            Raylib.BeginDrawing()
            clearBackground(0x31CCAD)
            Raylib.EndDrawing()
        }
        Raylib.CloseWindow()
    }
}

// sqlite3 高层包装
object SqliteWrapper {
    fun checkError(code: Int, db: Long): Unit {
        if (code != Sqlite.SQLITE_OK) {
            IO.println("SQLite error: " + Sqlite.sqlite3_errmsg(db).toString())
        }
    }
}
```

#### 用户 Aura AOT FFI（声明 + loadLibrary）

```aura
// ═══════════════════════════════════════════════════════
// 用户 Aura AOT FFI：声明 + loadLibrary 标记
// ═══════════════════════════════════════════════════════

// 调用 Aura AOT 编译的图像库（仅声明 + loadLibrary）
extern interface "aura_image" {
    default fun loadLibrary(): String = "aura_image"

    fun loadImage(path: String): Long
    fun getImageWidth(img: Long): Int
    fun getImageHeight(img: Long): Int
    fun getPixel(img: Long, x: Int, y: Int): Int
}

// 调用 Aura AOT 编译的加密库（仅声明 + loadLibrary）
extern interface "aura_crypto" {
    default fun loadLibrary(): String = "aura_crypto"

    fun sha256(data: Long, len: Int): Long
    fun hmacSign(key: Long, data: Long, len: Int): Long
}

// ═══════════════════════════════════════════════════════
// 用户 Aura AOT FFI 实现层：默认包装移到 object
// ═══════════════════════════════════════════════════════

object ImageWrapper {
    fun isTransparent(img: Long, x: Int, y: Int): Boolean {
        return AuraImage.getPixel(img, x, y) and 0xFF000000 == 0
    }

    fun getPixelWithAlpha(img: Long, x: Int, y: Int): Int {
        val pixel = AuraImage.getPixel(img, x, y)
        return pixel and 0xFF
    }
}

object CryptoWrapper {
    fun sha256Hex(data: Long, len: Int): String {
        val hash = AuraCrypto.sha256(data, len)
        return longToHex(hash)
    }
}
```

### 2.5 编译器内置识别规则

编译器按以下优先级识别 `extern interface` 的目标：

| 优先级 | 条件 | 处理方式 | 调用 ABI |
|--------|------|---------|---------|
| 1 | 接口名 = 保留名 | 编译器内置 intrinsics | LLVM IR 直接发射 |
| 2 | 字符串名 + `default fun loadLibrary()` | 外部 Aura AOT 库 | JitValue ABI |
| 3 | 字符串名（无 loadLibrary） | 外部 C 库 | C ABI（dlopen） |

### 2.6 规则总结

| # | 规则 | 说明 |
|---|------|------|
| R1 | `extern interface` 内**仅 `loadLibrary` 可有函数体** | 唯一例外 |
| R2 | `extern interface` 内**不允许 var** | 移到 `object` |
| R3 | `extern interface` 内**允许 const val** | 编译期常量 |
| R4 | `extern interface` 内**允许纯声明** | 无函数体 |
| R5 | `extern interface` 内**允许 @native** | 仅编译器内部接口 |
| R6 | `default fun loadLibrary()` | 仅用户 Aura AOT FFI |
| R7 | 用户**不可声明保留名** | sema 报错 |
| R8 | 用户**不可声明 @native** | sema 报错 |

---

## 三、std API 设计分析：object 单例 vs 面向对象实例

### 3.1 两种范式对比

| 维度 | object 单例（当前） | 实例化（Java/Kotlin） |
|------|-------------------|---------------------|
| 语法 | `IO.println("hello")` | `val f = File("a.txt"); f.readText()` |
| 状态 | 无状态（全局） | 有状态（实例字段） |
| 多态 | ❌ 不支持 | ✅ 支持接口/继承 |
| 组合 | ❌ 不支持 | ✅ 可传递实例 |
| 可测试性 | ❌ 难以 mock | ✅ 可注入 mock 实例 |
| 资源管理 | ❌ 无 close/finalize | ✅ 可 close()、try-finally |
| 简洁性 | ✅ 直接调用 | ⚠️ 需先创建实例 |
| 性能 | ✅ 零开销 | ⚠️ 有实例分配 |
| 全局副作用 | ⚠️ 全局状态 | ✅ 局部实例 |

### 3.2 逐模块分析

#### Math — 无状态工具 → `object` 最合适

```aura
Math.sin(3.14)
Math.max(1, 2)
```

**结论**：保持 `object Math`。

#### IO / Console — 系统资源 → `object` + 属性

```aura
object Console {
    var stdout: OutputStream = DefaultConsoleOut()
    var stderr: OutputStream = DefaultConsoleErr()
    var stdin: InputStream = DefaultConsoleIn()

    fun println(msg: String): Unit { stdout.println(msg) }
    fun print(msg: String): Unit { stdout.print(msg) }
}
```

**结论**：`object Console` + 可替换属性 + `class OutputStream/InputStream`。

#### FileSystem / File — 资源对象 → `class` 最合适

```aura
val file = File("/path/to/file")
val content = file.readText()
file.close()

FileSystem.readText("/path")  // 便捷入口保留
```

**结论**：`class File` + `object FileSystem`（便捷入口）。

#### Random — 有状态（种子）→ `class` 最合适

```aura
val rng = Random(seed = 42)
val x = rng.nextInt(100)
```

**结论**：`class Random`。

#### Network / Socket — 资源对象 → `class` 最合适

```aura
val server = ServerSocket(port = 8080)
val client = server.accept()
client.close()
server.close()
```

**结论**：`class ServerSocket` + `class Socket`。

#### Process — 混合模式

```aura
val pid = Process.spawn("ls -la")
val proc = ProcessHandle(pid)
proc.wait()
proc.kill()
```

**结论**：`object Process`（创建）+ `class ProcessHandle`（操作）。

#### Thread — 资源对象 → `class` 最合适

```aura
val t = Thread { doWork() }
t.start()
t.join()
```

**结论**：`class Thread`。

#### Collections — 已有实例 → 保持

```aura
val list = mutableListOf<Int>()
val map = mutableMapOf<String, Int>()
```

当前集合已经是实例化的。保持。

#### Time / Clock — 无状态工具 → `object`

```aura
Time.now()
Time.elapsed()
Time.sleep(1000)
```

**结论**：`object Time`。

### 3.3 推荐分类总结

| 模块 | 类型 | 模式 | 理由 |
|------|------|------|------|
| Math | `object` | 静态工具 | 无状态 |
| Console | `object` | 系统资源 + 属性 | stdout/stderr 可替换 |
| IO | `object` | 便捷入口（委托 Console） | 顶层函数便利 |
| Time | `object` | 静态工具 | 无状态 |
| Env | `object` | 静态工具 | 无状态 |
| Random | `class` | 有状态实例 | 内部种子 |
| File | `class` | 资源实例 | 文件句柄 |
| FileSystem | `object` | 便捷入口（委托 File） | 单次操作便利 |
| Socket | `class` | 资源实例 | 网络连接 |
| ServerSocket | `class` | 资源实例 | 监听连接 |
| Process | `object` | 创建工具 | 返回 PID |
| ProcessHandle | `class` | 资源实例 | 操作子进程 |
| Thread | `class` | 资源实例 | 线程句柄 |
| Channel | `class` | 资源实例 | 并发通道 |
| Mutex | `class` | 资源实例 | 同步锁 |
| Actor | `class` | 资源实例 | 演员消息 |
| Future | `class` | 结果容器 | 异步结果 |
| List/Map/Set | `class` | 已有实例 | 已有正确模式 |

### 3.4 接口抽象（可选扩展）

```aura
interface OutputStream {
    fun write(bytes: Array<Byte>): Unit
    fun writeInt(value: Int): Unit
    fun println(msg: String): Unit
    fun print(msg: String): Unit
    fun flush(): Unit
}

interface InputStream {
    fun read(buffer: Long, count: Int): Int
    fun readLine(): String
    fun readAll(): String
    fun close(): Unit
}
```

---

## 四、语法规范

### 4.1 编译器内部 `extern interface`（仅声明 + 常量）

```aura
// ── 编译器内置 native（保留名）──

extern interface Memory {
    fun read(addr: Long): Byte
    fun read16(addr: Long): Short
    fun read32(addr: Long): Int
    fun read64(addr: Long): Long
    fun write(addr: Long, v: Byte)
    fun write16(addr: Long, v: Short)
    fun write32(addr: Long, v: Int)
    fun write64(addr: Long, v: Long)
    fun copy(dst: Long, src: Long, n: Long)
    fun set(addr: Long, v: Byte, n: Long)
    fun alloc(n: Long): Long
    fun free(addr: Long)
    fun mmap(addr: Long, length: Long, prot: Int, flags: Int, fd: Int, offset: Long): Long
    fun munmap(addr: Long, length: Long): Int
    fun mprotect(addr: Long, length: Long, prot: Int): Int
    fun arcIncrement(addr: Long): Long
    fun arcDecrement(addr: Long): Long
}

extern interface Cpu {
    fun rdtsc(): Long
    fun memFence()
    fun cpuid(level: Int): Long
    fun atomicAdd(addr: Long, delta: Long): Long
}

extern interface Syscalls {
    const val SYS_READ: Int = 0
    const val SYS_WRITE: Int = 1
    const val SYS_OPEN: Int = 2
    const val SYS_CLOSE: Int = 3
    const val SYS_FSTAT: Int = 5
    const val SYS_LSEEK: Int = 8
    const val SYS_MMAP: Int = 9
    const val SYS_MUNMAP: Int = 11
    const val SYS_ACCESS: Int = 21
    const val SYS_PIPE: Int = 22
    const val SYS_UNLINK: Int = 87
    const val SYS_MKDIR: Int = 83
    const val SYS_RMDIR: Int = 84
    const val SYS_RENAME: Int = 82
    const val SYS_EXECVE: Int = 59
    const val SYS_EXIT: Int = 60
    const val SYS_EXIT_GROUP: Int = 231
    const val SYS_WAIT4: Int = 61
    const val SYS_FORK: Int = 57
    const val SYS_KILL: Int = 62
    const val SYS_GETPID: Int = 39
    const val SYS_CLOCK_GETTIME: Int = 228
    const val SYS_GETRANDOM: Int = 272
    const val STDIN: Int = 0
    const val STDOUT: Int = 1
    const val STDERR: Int = 2
    const val O_RDONLY: Int = 0
    const val O_WRONLY: Int = 1
    const val O_RDWR: Int = 2
    const val O_CREAT: Int = 0x40
    const val O_EXCL: Int = 0x80
    const val O_TRUNC: Int = 0x200
    const val F_OK: Int = 0
    const val PROT_READ: Int = 1
    const val PROT_WRITE: Int = 2
    const val PROT_EXEC: Int = 4
    const val MAP_PRIVATE: Int = 2
    const val MAP_ANONYMOUS: Int = 0x20
    const val MAP_FAILED: Long = -1
    const val SEEK_SET: Int = 0
    const val SEEK_CUR: Int = 1
    const val SEEK_END: Int = 2
    const val CLOCK_REALTIME: Int = 0
    const val CLOCK_MONOTONIC: Int = 1
    const val CLOCK_PROCESS_CPUTIME_ID: Int = 2
    const val CLOCK_THREAD_CPUTIME_ID: Int = 3
    const val WNOHANG: Int = 0x00000001
    const val INVALID_FD: Int = -1

    @native(SYS_READ) fun read(fd: Long, buf: Long, count: Long): Long
    @native(SYS_WRITE) fun write(fd: Long, buf: Long, count: Long): Long
    @native(SYS_OPEN) fun open(path: Long, flags: Int): Int
    @native(SYS_CLOSE) fun close(fd: Int): Int
    @native(SYS_FSTAT) fun fstat(fd: Int, buf: Long): Long
    @native(SYS_LSEEK) fun lseek(fd: Int, off: Long, whence: Int): Long
    @native(SYS_MMAP) fun mmap(addr: Long, length: Long, prot: Int, flags: Int, fd: Int, offset: Long): Long
    @native(SYS_MUNMAP) fun munmap(addr: Long, length: Long): Int
    @native(SYS_ACCESS) fun access(path: Long, mode: Int): Int
    @native(SYS_UNLINK) fun unlink(path: Long): Int
    @native(SYS_MKDIR) fun mkdir(path: Long, mode: Int): Int
    @native(SYS_RMDIR) fun rmdir(path: Long): Int
    @native(SYS_RENAME) fun rename(old: Long, new: Long): Int
    @native(SYS_EXECVE) fun execve(path: Long, args: Long, env: Long): Long
    @native(SYS_EXIT) fun exit(code: Int)
    @native(SYS_EXIT_GROUP) fun exitGroup(code: Int)
    @native(SYS_WAIT4) fun wait4(pid: Int, status: Long, options: Int, rusage: Long): Int
    @native(SYS_FORK) fun fork(): Int
    @native(SYS_KILL) fun kill(pid: Int, sig: Int): Int
    @native(SYS_GETPID) fun getpid(): Int
    @native(SYS_CLOCK_GETTIME) fun clockGettime(clock: Int, ts: Long): Long
    @native(SYS_GETRANDOM) fun getrandom(buf: Long, len: Long, flags: Int): Long
    @native(SYS_PIPE) fun pipe(pipes: Long): Int
    @native(41) fun socket(domain: Int, type: Int, protocol: Int): Int
    @native(42) fun connect(fd: Int, addr: Long, addrlen: Int): Int
    @native(49) fun bind(fd: Int, addr: Long, addrlen: Int): Int
    @native(50) fun listen(fd: Int, backlog: Int): Int
    @native(43) fun accept(fd: Int, addr: Long, addrlen: Long): Int
    @native(44) fun sendto(fd: Int, buf: Long, len: Int, flags: Int, addr: Long, addrlen: Int): Long
    @native(45) fun recvfrom(fd: Int, buf: Long, len: Int, flags: Int, addr: Long, addrlen: Int): Long
}

extern interface Runtime {
    fun arcIncrement(ptr: Long)
    fun arcDecrement(ptr: Long): Long
    fun coroutineYield(ctx: Long)
}

extern interface ThreadOps {
    fun create(fn_id: Int, arg: Int): Int
    fun join(thread_id: Int): Int
    fun sleepMs(ms: Int): Unit
    fun currentId(): Int
    fun cores(): Int
}

// ── aura.lang.native 包接口（仅声明 + 常量）──

extern interface Console {
    @native(SYS_WRITE) fun writeStdout(buf: Long, count: Long): Long
}

extern interface FileOps {
    const val O_RDONLY: Int = 0
    const val O_WRONLY: Int = 1
    const val O_RDWR: Int = 2
    const val O_CREAT: Int = 0x40
    const val O_EXCL: Int = 0x80
    const val O_TRUNC: Int = 0x200
    const val SEEK_SET: Int = 0
    const val SEEK_CUR: Int = 1
    const val SEEK_END: Int = 2
    const val F_OK: Int = 0

    @native(SYS_OPEN)   fun open(path: Long, flags: Int): Int
    @native(SYS_CLOSE)  fun close(fd: Int): Int
    @native(SYS_READ)   fun read(fd: Int, buf: Long, count: Long): Long
    @native(SYS_WRITE)  fun write(fd: Int, buf: Long, count: Long): Long
    @native(SYS_LSEEK)  fun lseek(fd: Int, off: Long, whence: Int): Long
    @native(SYS_FSTAT)  fun fstat(fd: Int, buf: Long): Long
    @native(SYS_UNLINK) fun unlink(path: Long): Int
    @native(SYS_ACCESS) fun access(path: Long, mode: Int): Int
    @native(SYS_MKDIR)  fun mkdir(path: Long, mode: Int): Int
    @native(SYS_RMDIR)  fun rmdir(path: Long): Int
    @native(SYS_RENAME) fun rename(old: Long, new: Long): Int
}

extern interface Clock {
    const val CLOCK_REALTIME: Int = 0
    const val CLOCK_MONOTONIC: Int = 1
    const val CLOCK_PROCESS_CPUTIME_ID: Int = 2
    const val CLOCK_THREAD_CPUTIME_ID: Int = 3

    @native(SYS_CLOCK_GETTIME) fun clockGettime(clock: Int, ts: Long): Long
}

extern interface ProcessOps {
    @native(SYS_EXIT_GROUP) fun exitGroup(code: Int)
    @native(SYS_WAIT4)      fun wait4(pid: Int, status: Long, options: Int, rusage: Long): Int
    @native(SYS_FORK)       fun fork(): Int
    @native(SYS_EXECVE)     fun execve(path: Long, args: Long, env: Long): Long
    @native(SYS_GETPID)     fun getpid(): Int
    @native(SYS_OPEN)       fun openFile(path: CString, flags: Int): Int
    @native(SYS_CLOSE)      fun closeFile(fd: Int): Int
    @native(SYS_READ)       fun readSyscall(fd: Int, buf: Long, count: Long): Long
}

// ── 外部 C 符号声明 ──
extern interface "runtime" {
    fun aura_process_argCount(): Long
    fun aura_process_args(): String
}
```

### 4.2 编译器内部 `object`（实现层）

```aura
// ── Console 实现 ──
object ConsoleImpl {
    var newlineBuf: Long = 0
    var intBuf: Long = 0

    fun init(): Boolean {
        newlineBuf = Memory.alloc(2)
        if (newlineBuf == 0) { return false }
        intBuf = Memory.alloc(32)
        if (intBuf == 0) { return false }
        Memory.write(newlineBuf, 10)
        return true
    }

    fun print(msg: Long) {
        val len: Long = StringOps.strlen(msg)
        Console.writeStdout(msg, len)
    }

    fun println(msg: Long) {
        val len: Long = StringOps.strlen(msg)
        Console.writeStdout(msg, len)
        if (newlineBuf != 0) {
            Console.writeStdout(newlineBuf, 1)
        }
    }

    fun printlnInt(n: Long) {
        val buf: Long = intToStr(n)
        if (buf == 0) { return }
        val len: Long = StringOps.strlen(buf)
        Console.writeStdout(buf, len)
        if (newlineBuf != 0) {
            Console.writeStdout(newlineBuf, 1)
        }
    }

    fun printInt(n: Long) {
        val buf: Long = intToStr(n)
        if (buf == 0) { return }
        val len: Long = StringOps.strlen(buf)
        Console.writeStdout(buf, len)
    }

    fun printlnFloat(n: Float) {
        val i: Long = n as Long
        printlnInt(i)
    }

    fun getNewlineBuffer(): Long { return newlineBuf }
    fun getIntBuffer(): Long { return intBuf }

    fun intToStr(n: Long): Long {
        if (intBuf == 0) { return 0 }
        Memory.set(intBuf, 0, 32)
        if (n == 0) {
            Memory.write(intBuf, 48)
            Memory.write(intBuf + 1, 0)
            return intBuf
        }
        var pos: Long = 30
        var num: Long = n
        var negative: Boolean = false
        if (num < 0) {
            negative = true
            num = -num
        }
        while (num > 0) {
            val digit: Byte = (48 + num % 10) as Byte
            pos = pos - 1
            Memory.write(intBuf + pos, digit)
            num = num / 10
        }
        if (negative) {
            pos = pos - 1
            Memory.write(intBuf + pos, 45)
        }
        Memory.write(intBuf + pos + 16, 0)
        return intBuf + pos
    }
}

// ── FileOps 实现 ──
object FileOpsImpl {
    fun openFile(path: Long, flags: Int): Int { return FileOps.open(path, flags) }
    fun closeFile(fd: Int): Int { return FileOps.close(fd) }
    fun readFile(fd: Int, buf: Long, count: Long): Long { return FileOps.read(fd, buf, count) }
    fun writeFile(fd: Int, buf: Long, count: Long): Long { return FileOps.write(fd, buf, count) }
    fun seekFile(fd: Int, offset: Long, whence: Int): Long { return FileOps.lseek(fd, offset, whence) }
    fun statFile(fd: Int, buf: Long): Long { return FileOps.fstat(fd, buf) }
    fun deleteFile(path: Long): Int { return FileOps.unlink(path) }
    fun exists(path: Long): Boolean { return FileOps.access(path, 0) == 0 }
    fun mkdirFile(path: Long, mode: Int): Int { return FileOps.mkdir(path, mode) }
    fun rmdirFile(path: Long): Int { return FileOps.rmdir(path) }
}

// ── Runtime 实现 ──
object RuntimeImpl {
    const val ARC_COUNT_OFFSET: Long = 0

    var allocTotal: Long = 0
    var allocLive: Long = 0
    var allocCount: Int = 0
    var freeCount: Int = 0

    fun init(): Boolean {
        val ok: Boolean = Allocator.init()
        if (!ok) { return false }
        return true
    }

    fun version(): String {
        return "Aura Runtime 0.1.0"
    }

    fun memUsedMb(): Int {
        val used: Long = Allocator.used()
        val mb: Long = used / (1024 * 1024)
        return mb as Int
    }

    fun trackAlloc(size: Long) {
        allocTotal = allocTotal + size
        allocLive = allocLive + size
        allocCount = allocCount + 1
    }

    fun trackFree(size: Long) {
        allocLive = allocLive - size
        freeCount = freeCount + 1
    }
}

// ── Clock 实现 ──
object ClockImpl {
    var tsBuf: Long = 0

    fun init(): Boolean {
        tsBuf = Memory.alloc(16)
        return tsBuf != 0
    }

    fun now(): Time {
        if (tsBuf == 0) { return Time() }
        var t: Time = Time()
        val ret: Long = Clock.clockGettime(0, tsBuf)
        if (ret == 0) {
            t.sec = Memory.read64(tsBuf)
            t.nsec = Memory.read64(tsBuf + 8)
        }
        return t
    }

    fun timeMs(): Long {
        val t: Time = now()
        return t.sec * 1000 + t.nsec / 1000000
    }

    fun timeUs(): Long {
        val t: Time = now()
        return t.sec * 1000000 + t.nsec / 1000
    }

    fun timeNs(): Long {
        val t: Time = now()
        return t.sec * 1000000000 + t.nsec
    }

    fun sleep(ms: Long) {
        if (ms <= 0) { return }
        val startNs: Long = timeNs()
        val endNs: Long = startNs + ms * 1000000
        while (timeNs() < endNs) {
            Cpu.memFence()
        }
    }
}

// ── ProcessOps 实现 ──
object ProcessOpsImpl {
    fun exit(code: Int) { ProcessOps.exitGroup(code) }
    fun wait(pid: Int): Int { return ProcessOps.wait4(pid, 0, 0, 0) }
    fun exec(path: Long, args: Long, env: Long): Long { return ProcessOps.execve(path, args, env) }
    fun getPid(): Int { return ProcessOps.getpid() }
    fun forkProcess(): Int { return ProcessOps.fork() }
}
```

### 4.3 用户 `extern interface`（仅声明 + loadLibrary）

```aura
// ── 用户 C FFI（仅声明 + 常量，无实现）──
extern interface "raylib" {
    const val WHITE: Int = 0xFFFFFFFF
    const val BLACK: Int = 0xFF000000

    fun InitWindow(width: Int, height: Int, title: CString): Unit
    fun BeginDrawing(): Unit
    fun EndDrawing(): Unit
    fun ClearBackground(r: Int, g: Int, b: Int, a: Int): Unit
    fun IsWindowClosed(): Boolean
    fun CloseWindow(): Unit
}

// ── 用户 Aura AOT FFI（声明 + loadLibrary 标记）──
extern interface "aura_image" {
    default fun loadLibrary(): String = "aura_image"

    fun loadImage(path: String): Long
    fun getImageWidth(img: Long): Int
}
```

### 4.4 用户 `object`（实现层）

```aura
// ── 用户 C FFI 实现层 ──
object RaylibWrapper {
    fun clearBackground(color: Int) {
        val r: Int = (color shr 16) and 255
        val g: Int = (color shr 8) and 255
        val b: Int = color and 255
        Raylib.ClearBackground(r, g, b, 255)
    }

    fun drawCenteredText(text: CString, color: Int) {
        val x: Int = (Raylib.GetScreenWidth() - 100) / 2
        val y: Int = (Raylib.GetScreenHeight() - Raylib.DEFAULT_FONT_SIZE) / 2
        Raylib.DrawText(text, x, y, Raylib.DEFAULT_FONT_SIZE, color)
    }

    fun runGameLoop() {
        Raylib.InitWindow(800, 600, "Game")
        while (!Raylib.IsWindowClosed()) {
            Raylib.BeginDrawing()
            clearBackground(0x31CCAD)
            Raylib.EndDrawing()
        }
        Raylib.CloseWindow()
    }
}

// ── 用户 Aura AOT FFI 实现层 ──
object ImageWrapper {
    fun isTransparent(img: Long, x: Int, y: Int): Boolean {
        return AuraImage.getPixel(img, x, y) and 0xFF000000 == 0
    }
}
```

### 4.5 std 层 `object` / `class` 语法

```aura
// ── object: 无状态工具 ──
object Math {
    fun sin(x: Float): Float { ... }
    fun cos(x: Float): Float { ... }
}

// ── object: 系统资源 ──
object Console {
    var stdout: OutputStream = DefaultConsoleOut()
    var stderr: OutputStream = DefaultConsoleErr()
    var stdin: InputStream = DefaultConsoleIn()

    fun println(msg: String): Unit { stdout.println(msg) }
    fun print(msg: String): Unit { stdout.print(msg) }
}

// ── class: 有状态实例 ──
class File(path: String) {
    private val pathBuf: Long = Stdio.stringToBuffer(path)

    fun readText(): String {
        val fd = FileOpsImpl.openFile(pathBuf, 0)
        return Stdio.bufferToString(buf, len)
    }

    fun writeText(content: String): Unit {
        val fd = FileOpsImpl.openFile(pathBuf, 1 | 0x40 | 0x200)
        // ... 写入 ...
    }

    fun exists(): Boolean { return Stdio.fileExists(path) }
    fun size(): Long { return FSUtils.fs_file_size(path) }
}

class Random(seed: Long = 0) {
    private var state: Long = seed

    fun nextInt(bound: Int): Int {
        state = state * 6364136223846793005 + 1442695040888963407
        return ((state as Long) % bound as Long) as Int
    }
}

class Socket(host: String, port: Int) {
    private var fd: Int = 0

    fun connect(): Unit {
        val addrBuf: Long = Allocator.malloc(16)
        Memory.write32(addrBuf, 2)
        Memory.write32(addrBuf + 4, port)
        fd = Syscalls.socket(2, 1, 6)
        Syscalls.connect(fd, addrBuf, 16)
    }

    fun close(): Unit { Syscalls.close(fd) }
}

// ── object: 便捷入口 ──
object FileSystem {
    fun readText(path: String): String { return File(path).readText() }
    fun writeText(path: String, content: String): Unit { File(path).writeText(content) }
    fun exists(path: String): Boolean { return File(path).exists() }
    fun delete(path: String): Unit { FSUtils.fs_delete(path) }
    fun mkdir(path: String): Unit { FSUtils.fs_mkdir_new(path) }
}
```

### 4.6 废弃语法

| 旧语法 | 新语法 | 原因 |
|--------|--------|------|
| `extern object X { ... }` | `extern interface X { ... }` | 消除编译器无法区分声明/实现 |
| `extern "c" "lib" { ... }` | `extern interface "lib" { ... }` | 统一 FFI 入口 |
| `extern interface X { default fun loadLibrary() = ...; fun f() }` | 保持不变 | 已有实现，向后兼容 |

---

## 五、文件变更清单

### 5.1 仅需改关键词（纯声明文件）

| 文件 | 变更 | 说明 |
|------|------|------|
| `Memory.aura` | `extern object` → `extern interface` | 纯声明，无需拆分 |
| `Cpu.aura` | `extern object` → `extern interface` | 纯声明，无需拆分 |
| `ThreadOps.aura` | `extern object` → `extern interface` | 纯声明，无需拆分 |
| `Syscalls.aura` | `object SyscallsUtils` → `extern interface Syscalls` | `val` → `const val` |

### 5.2 需拆分（声明留在 `extern interface`，实现移到 `object`）

#### Console.aura

**变更前**：
```aura
extern object Console {
    @native(SYS_WRITE) fun writeStdout(buf: Long, count: Long): Long

    var newlineBuf: Long = 0
    var intBuf: Long = 0

    fun init(): Boolean {
        newlineBuf = Memory.alloc(2)
        // ...
    }

    fun print(msg: Long) { ... }
    fun println(msg: Long) { ... }
    fun printlnInt(n: Long) { ... }
    fun printInt(n: Long) { ... }
    fun intToStr(n: Long): Long { ... }
    fun getNewlineBuffer(): Long { return newlineBuf }
    fun getIntBuffer(): Long { return intBuf }
}
```

**变更后**（声明 → `extern interface`，实现 → `object`）：
```aura
// ── 声明 ──
extern interface Console {
    @native(SYS_WRITE) fun writeStdout(buf: Long, count: Long): Long
}

// ── 实现 ──
object ConsoleImpl {
    var newlineBuf: Long = 0
    var intBuf: Long = 0

    fun init(): Boolean {
        newlineBuf = Memory.alloc(2)
        if (newlineBuf == 0) { return false }
        intBuf = Memory.alloc(32)
        if (intBuf == 0) { return false }
        Memory.write(newlineBuf, 10)
        return true
    }

    fun print(msg: Long) {
        val len: Long = StringOps.strlen(msg)
        Console.writeStdout(msg, len)
    }

    fun println(msg: Long) {
        val len: Long = StringOps.strlen(msg)
        Console.writeStdout(msg, len)
        if (newlineBuf != 0) { Console.writeStdout(newlineBuf, 1) }
    }

    fun printlnInt(n: Long) { ... }
    fun printInt(n: Long) { ... }
    fun intToStr(n: Long): Long { ... }
    fun getNewlineBuffer(): Long { return newlineBuf }
    fun getIntBuffer(): Long { return intBuf }
}
```

#### FileOps.aura

**变更后**：
```aura
// ── 声明 ──
extern interface FileOps {
    const val O_RDONLY: Int = 0
    const val O_WRONLY: Int = 1
    const val O_RDWR: Int = 2
    const val O_CREAT: Int = 0x40
    const val O_EXCL: Int = 0x80
    const val O_TRUNC: Int = 0x200
    const val SEEK_SET: Int = 0
    const val SEEK_CUR: Int = 1
    const val SEEK_END: Int = 2
    const val F_OK: Int = 0

    @native(SYS_OPEN)   fun open(path: Long, flags: Int): Int
    @native(SYS_CLOSE)  fun close(fd: Int): Int
    @native(SYS_READ)   fun read(fd: Int, buf: Long, count: Long): Long
    @native(SYS_WRITE)  fun write(fd: Int, buf: Long, count: Long): Long
    @native(SYS_LSEEK)  fun lseek(fd: Int, off: Long, whence: Int): Long
    @native(SYS_FSTAT)  fun fstat(fd: Int, buf: Long): Long
    @native(SYS_UNLINK) fun unlink(path: Long): Int
    @native(SYS_ACCESS) fun access(path: Long, mode: Int): Int
    @native(SYS_MKDIR)  fun mkdir(path: Long, mode: Int): Int
    @native(SYS_RMDIR)  fun rmdir(path: Long): Int
    @native(SYS_RENAME) fun rename(old: Long, new: Long): Int
}

// ── 实现 ──
object FileOpsImpl {
    fun openFile(path: Long, flags: Int): Int { return FileOps.open(path, flags) }
    fun closeFile(fd: Int): Int { return FileOps.close(fd) }
    fun readFile(fd: Int, buf: Long, count: Long): Long { return FileOps.read(fd, buf, count) }
    fun writeFile(fd: Int, buf: Long, count: Long): Long { return FileOps.write(fd, buf, count) }
    fun seekFile(fd: Int, offset: Long, whence: Int): Long { return FileOps.lseek(fd, offset, whence) }
    fun statFile(fd: Int, buf: Long): Long { return FileOps.fstat(fd, buf) }
    fun deleteFile(path: Long): Int { return FileOps.unlink(path) }
    fun exists(path: Long): Boolean { return FileOps.access(path, 0) == 0 }
    fun mkdirFile(path: Long, mode: Int): Int { return FileOps.mkdir(path, mode) }
    fun rmdirFile(path: Long): Int { return FileOps.rmdir(path) }
}
```

#### Runtime.aura

**变更后**：
```aura
// ── 声明 ──
extern interface Runtime {
    fun arcIncrement(ptr: Long)
    fun arcDecrement(ptr: Long): Long
    fun coroutineYield(ctx: Long)
}

// ── 实现 ──
object RuntimeImpl {
    const val ARC_COUNT_OFFSET: Long = 0

    var allocTotal: Long = 0
    var allocLive: Long = 0
    var allocCount: Int = 0
    var freeCount: Int = 0

    fun init(): Boolean {
        val ok: Boolean = Allocator.init()
        if (!ok) { return false }
        return true
    }

    fun version(): String {
        return "Aura Runtime 0.1.0"
    }

    fun memUsedMb(): Int {
        val used: Long = Allocator.used()
        val mb: Long = used / (1024 * 1024)
        return mb as Int
    }

    fun trackAlloc(size: Long) {
        allocTotal = allocTotal + size
        allocLive = allocLive + size
        allocCount = allocCount + 1
    }

    fun trackFree(size: Long) {
        allocLive = allocLive - size
        freeCount = freeCount + 1
    }
}
```

#### Clock.aura

**变更后**：
```aura
// ── 声明 ──
extern interface Clock {
    const val CLOCK_REALTIME: Int = 0
    const val CLOCK_MONOTONIC: Int = 1
    const val CLOCK_PROCESS_CPUTIME_ID: Int = 2
    const val CLOCK_THREAD_CPUTIME_ID: Int = 3

    @native(SYS_CLOCK_GETTIME) fun clockGettime(clock: Int, ts: Long): Long
}

// ── 实现 ──
object ClockImpl {
    var tsBuf: Long = 0

    fun init(): Boolean {
        tsBuf = Memory.alloc(16)
        return tsBuf != 0
    }

    fun now(): Time {
        if (tsBuf == 0) { return Time() }
        var t: Time = Time()
        val ret: Long = Clock.clockGettime(0, tsBuf)
        if (ret == 0) {
            t.sec = Memory.read64(tsBuf)
            t.nsec = Memory.read64(tsBuf + 8)
        }
        return t
    }

    fun timeMs(): Long {
        val t: Time = now()
        return t.sec * 1000 + t.nsec / 1000000
    }

    fun timeUs(): Long {
        val t: Time = now()
        return t.sec * 1000000 + t.nsec / 1000
    }

    fun timeNs(): Long {
        val t: Time = now()
        return t.sec * 1000000000 + t.nsec
    }

    fun sleep(ms: Long) {
        if (ms <= 0) { return }
        val startNs: Long = timeNs()
        val endNs: Long = startNs + ms * 1000000
        while (timeNs() < endNs) {
            Cpu.memFence()
        }
    }
}
```

#### ProcessOps.aura

**变更后**：
```aura
// ── syscall 声明 ──
extern interface ProcessOps {
    @native(SYS_EXIT_GROUP) fun exitGroup(code: Int)
    @native(SYS_WAIT4)      fun wait4(pid: Int, status: Long, options: Int, rusage: Long): Int
    @native(SYS_FORK)       fun fork(): Int
    @native(SYS_EXECVE)     fun execve(path: Long, args: Long, env: Long): Long
    @native(SYS_GETPID)     fun getpid(): Int
    @native(SYS_OPEN)       fun openFile(path: CString, flags: Int): Int
    @native(SYS_CLOSE)      fun closeFile(fd: Int): Int
    @native(SYS_READ)       fun readSyscall(fd: Int, buf: Long, count: Long): Long
}

// ── 外部 C 符号声明 ──
extern interface "runtime" {
    fun aura_process_argCount(): Long
    fun aura_process_args(): String
}

// ── 实现 ──
object ProcessOpsImpl {
    fun exit(code: Int) { ProcessOps.exitGroup(code) }
    fun wait(pid: Int): Int { return ProcessOps.wait4(pid, 0, 0, 0) }
    fun exec(path: Long, args: Long, env: Long): Long { return ProcessOps.execve(path, args, env) }
    fun getPid(): Int { return ProcessOps.getpid() }
    fun forkProcess(): Int { return ProcessOps.fork() }
}
```

### 5.3 std 层重构（object → class）

以下模块需要从 `object` 转为 `class`：

| 当前文件 | 当前类型 | 目标类型 | 理由 |
|---------|---------|---------|------|
| `Random.aura` | `object` | `class Random` | 有状态（种子） |
| `Process.aura` | `object` | `object Process`（创建）+ `class ProcessHandle` | 混合 |
| `Thread.aura` | `object` | `class Thread` | 资源实例 |
| `concurrent/Socket.aura`（新增） | — | `class Socket` | 资源实例 |
| `concurrent/Channel.aura` | `object` | `class Channel` | 资源实例 |
| `concurrent/Mutex.aura` | `object` | `class Mutex` | 资源实例 |

保持 `object` 的模块：

| 模块 | 类型 | 理由 |
|------|------|------|
| `Math.aura` | `object` | 无状态工具 |
| `IO.aura` | `object` | 便捷入口 |
| `Console.aura` | `object` + 属性 | 系统资源 + 可替换 |
| `FileSystem.aura` | `object` | 便捷入口（委托 File） |
| `Time.aura` | `object` | 无状态工具 |
| `Env.aura` | `object` | 无状态工具 |
| `Builtin.aura` | `object` | 工具函数 |
| `StringBuilder.aura` | `object` | 纯逻辑 |
| `Collections.aura` | `object` | 工厂方法 |
| `Json.aura` | `object` | 无状态工具 |

---

## 六、编译器变更

### 6.1 Parser 变更

| 变更 | 文件 | 说明 |
|------|------|------|
| `extern object` → `extern interface` | `parser.rs` | 废弃警告 |
| `extern interface` 支持无参名 | `parser.rs` | `extern interface Memory` 无字符串 |
| `extern interface` 仅允许 `loadLibrary` 有函数体 | `parser.rs` | 非 `loadLibrary` 函数体报错 |
| `extern interface` 允许 `const val` | `parser.rs` | 编译期常量 |
| `extern interface` 禁止 `var` | `parser.rs` | sema 报错 |
| `extern interface` 禁止非 `loadLibrary` 函数体 | `parser.rs` | sema 报错 |
| `extern interface` 允许 `@native` 注解 | `parser.rs` | 编译器生成包装器 |
| `extern "c"` → `extern interface` | `parser.rs` | 废弃警告 |
| `interface` 关键字（新增） | `parser.rs` | 面向对象接口声明 |
| `class` 关键字（增强） | `parser.rs` | 确认已支持 |

### 6.2 Sema 变更

| 变更 | 文件 | 说明 |
|------|------|------|
| `ExternObjectDecl` → `ExternInterfaceDecl` | `sema/checker.rs` | 统一 AST |
| `ExternInterfaceDecl` 成员分类 | `sema/checker.rs` | Constant / Declaration / LoadLibrary |
| 保留名锁定表 | `sema/checker.rs` | `Memory`/`Cpu`/`Syscalls`/`Runtime`/`ThreadOps` |
| `loadLibrary` 唯一函数体检查 | `sema/checker.rs` | 非 `loadLibrary` 函数体在 sema 报错 |
| `var` 禁止检查 | `sema/checker.rs` | `extern interface` 内 `var` 在 ema 报错 |
| 字符串名 → FFI 类型推断 | `sema/checker.rs` | 有 `loadLibrary` → `FfiAbi::Aura`；无 → `FfiAbi::C` |
| 用户可声明 `extern interface` | `sema/checker.rs` | 允许字符串名 |
| 用户不可声明保留名 | `sema/checker.rs` | sema 报错 |
| 用户不可声明 `@native` | `sema/checker.rs` | sema 报错 |
| `aura.lang.native` import 拒绝 | `sema/checker.rs` | sema 报错 |
| 接口继承检查 | `sema/checker.rs` | 类实现接口的方法签名验证 |

### 6.3 Codegen 变更

| 变更 | 文件 | 说明 |
|------|------|------|
| `ExternInterfaceDecl` HIR 统一 | `codegen/hir.rs` | 成员分类：Constant / Declaration / LoadLibrary |
| `Declaration` → LLVM `declare` | `codegen/aot/emit.rs` | 外部 C 符号 |
| `LoadLibrary` → LLVM `define` | `codegen/aot/emit.rs` | 返回常量字符串的函数 |
| `Constant` → 常量折叠 | `codegen/aot/emit.rs` | 编译期消除 |
| `NativeDeclaration` → 包装器 + declare | `codegen/aot/emit.rs` | 编译器生成 |
| 内置 native 发射 | `codegen/aot/emit.rs` | `Memory.read` → `load i8, i8* %addr` |
| syscall 发射 | `codegen/aot/emit.rs` | `Syscalls.write` → `aura_syscall_dispatch` |
| 外部 C 发射 | `codegen/aot/ffi.rs` | `declare @sin(float)` |
| 外部 Aura AOT 发射 | `codegen/aot/ffi.rs` | JitValue ABI |
| class vtable 发射 | `codegen/aot/emit.rs` | 虚方法表（已有） |
| interface 方法验证 | `codegen/hir.rs` | 类实现接口时签名检查 |

### 6.4 Runtime 映射表

```rust
// compiler/src/codegen/intrinsics.rs

pub const BUILTIN_INTERFACES: &[&str] = &[
    "Memory", "Cpu", "Syscalls", "Runtime", "ThreadOps",
];

pub const NATIVE_FUNCTION_MAP: &[(&str, &str, IntrinsicKind)] = &[
    ("Memory", "read", IntrinsicKind::Load8),
    ("Memory", "read16", IntrinsicKind::Load16),
    ("Memory", "read32", IntrinsicKind::Load32),
    ("Memory", "read64", IntrinsicKind::Load64),
    ("Memory", "write", IntrinsicKind::Store8),
    ("Memory", "write16", IntrinsicKind::Store16),
    ("Memory", "write32", IntrinsicKind::Store32),
    ("Memory", "write64", IntrinsicKind::Store64),
    ("Memory", "copy", IntrinsicKind::MemCpy),
    ("Memory", "set", IntrinsicKind::MemSet),
    ("Memory", "alloc", IntrinsicKind::HeapAlloc),
    ("Memory", "free", IntrinsicKind::HeapFree),
    ("Memory", "mmap", IntrinsicKind::Syscall("mmap")),
    ("Memory", "munmap", IntrinsicKind::Syscall("munmap")),
    ("Memory", "mprotect", IntrinsicKind::Syscall("mprotect")),
    ("Memory", "arcIncrement", IntrinsicKind::ArcInc),
    ("Memory", "arcDecrement", IntrinsicKind::ArcDec),
    ("Cpu", "rdtsc", IntrinsicKind::Cpu("rdtsc")),
    ("Cpu", "memFence", IntrinsicKind::Cpu("mfence")),
    ("Cpu", "cpuid", IntrinsicKind::Cpu("cpuid")),
    ("Cpu", "atomicAdd", IntrinsicKind::Cpu("lock xaddq")),
    ("Syscalls", "read", IntrinsicKind::Syscall("read")),
    ("Syscalls", "write", IntrinsicKind::Syscall("write")),
    ("Syscalls", "open", IntrinsicKind::Syscall("open")),
    ("Syscalls", "close", IntrinsicKind::Syscall("close")),
    // ... all syscalls ...
    ("Runtime", "arcIncrement", IntrinsicKind::ArcInc),
    ("Runtime", "arcDecrement", IntrinsicKind::ArcDec),
    ("Runtime", "coroutineYield", IntrinsicKind::CoroYield),
    ("ThreadOps", "create", IntrinsicKind::ThreadCreate),
    ("ThreadOps", "join", IntrinsicKind::ThreadJoin),
    ("ThreadOps", "sleepMs", IntrinsicKind::ThreadSleep),
    ("ThreadOps", "currentId", IntrinsicKind::ThreadId),
    ("ThreadOps", "cores", IntrinsicKind::ThreadCores),
];
```

---

## 七、安全性分析

### 7.1 用户可见 API

| 层 | 用户可见 | 说明 |
|----|---------|------|
| `aura.lang.*` | ✅ | 基础类型（Int/String/Boolean/...） |
| `aura.lang.collection.*` | ✅ | 集合（List/Map/Set/Array/...） |
| `aura.lang.concurrent.*` | ✅ | 并发（Thread/Channel/Mutex/...） |
| `aura.lang.errors.*` | ✅ | 错误（Exception/Throwable/...） |
| `aura.lang.std.*` | ✅ | 标准库（Math/IO/Console/FileSystem/...） |
| `aura.lang.native.*` | ❌ | 底层实现，用户不可 import |
| `extern interface` 声明 | ✅ | 用户可定义（CFFI + Aura FFI） |

### 7.2 用户 FFI 安全边界

| 操作 | 是否允许 | 限制 |
|------|---------|------|
| `extern interface "name" { ... }` | ✅ | 用户可声明外部 C/Aura 库 |
| `extern interface "name" { const val X = ...; ... }` | ✅ | 常量允许 |
| `extern interface "name" { fun f(): R }` | ✅ | 纯声明允许 |
| `extern interface "name" { default fun loadLibrary() = ... }` | ✅ | 唯一允许函数体 |
| `extern interface "name" { var X = ...; ... }` | ❌ | var 移到 object |
| `extern interface "name" { fun f() { body } }` | ❌ | 实现移到 object |
| `extern interface Memory { ... }` | ❌ | 保留名，sema 报错 |
| `@native(N) fun f()` | ❌ | sema 报错，仅编译器可发射 |
| import `aura.lang.native` | ❌ | sema 报错 |

### 7.3 编译器保护机制

| 机制 | 说明 |
|------|------|
| 保留名锁定 | `Memory`/`Cpu`/`Syscalls`/`Runtime`/`ThreadOps` 不可被用户声明 |
| 非 `loadLibrary` 函数体禁止 | `extern interface` 内非 `loadLibrary` 函数体在 sema 报错 |
| `var` 禁止 | `extern interface` 内 `var` 在 sema 报错 |
| `@native` 仅编译器可发射 | 用户源码中的 `@native` 在 sema 阶段报错 |
| `aura.lang.native` 包设为内部 | sema 拒绝用户 import |
| 外部 FFI 运行时加载 | `dlopen` / `LoadLibraryW` 在运行库中执行 |

---

## 八、实施路线

### Phase 1：语法迁移（1 周）

```
Step 1.1  编译器支持 extern interface 无参名语法（parser.rs）
Step 1.2  编译器仅允许 loadLibrary 有函数体（parser.rs）
Step 1.3  编译器允许 extern interface 内的 const val（常量）
Step 1.4  编译器禁止 extern interface 内的 var（sema 报错）
Step 1.5  编译器禁止非 loadLibrary 函数体（sema 报错）
Step 1.6  编译器 deprecate extern object（parser.rs 警告）
Step 1.7  编译器 deprecate extern "c"（parser.rs 警告）
Step 1.8  新增 interface 关键字解析（面向对象接口）
Step 1.9  编译现有代码，确认无回归
```

### Phase 2：纯声明迁移（0.5 周）

```
Step 2.1  Memory.aura: extern object → extern interface
Step 2.2  Cpu.aura: extern object → extern interface
Step 2.3  ThreadOps.aura: extern object → extern interface
Step 2.4  Syscalls.aura: object SyscallsUtils → extern interface Syscalls（val → const val）
Step 2.5  编译测试，确认 AOT/VM 均可运行
```

### Phase 3：混合文件拆分（1 周）

```
Step 3.1  Console.aura → extern interface Console + object ConsoleImpl
Step 3.2  FileOps.aura → extern interface FileOps + object FileOpsImpl
Step 3.3  Runtime.aura → extern interface Runtime + object RuntimeImpl
Step 3.4  Clock.aura → extern interface Clock + object ClockImpl
Step 3.5  ProcessOps.aura → extern interface ProcessOps + object ProcessOpsImpl + extern interface "runtime"
Step 3.6  编译测试 + 自举验证
```

### Phase 4：Syscalls 统一（0.5 周）

```
Step 4.1  native/arch/ 各平台 Syscalls 统一为 extern interface
Step 4.2  编译器 NATIVE_FUNCTION_MAP 完善
Step 4.3  编译测试 + 全量回归
```

### Phase 5：std 面向对象重构（2 周）

```
Step 5.1  Random.aura: object → class Random（有状态实例）
Step 5.2  Thread.aura: object → class Thread
Step 5.3  Process.aura: object → object Process（创建）+ class ProcessHandle（操作）
Step 5.4  新增 File.aura: class File（资源实例）
Step 5.5  新增 Socket.aura / ServerSocket.aura: class（资源实例）
Step 5.6  Console.aura: object + stdout/stderr 可替换属性
Step 5.7  定义 OutputStream / InputStream 接口
Step 5.8  编译测试 + 自举验证
```

### Phase 6：安全加固（0.5 周）

```
Step 6.1  sema: 禁止用户 import aura.lang.native
Step 6.2  sema: 禁止用户声明 @native 注解
Step 6.3  sema: 禁止用户声明 Memory/Cpu/Syscalls/Runtime/ThreadOps 保留名
Step 6.4  sema: 禁止 extern interface 内非 loadLibrary 函数体
Step 6.5  sema: 禁止 extern interface 内 var
Step 6.6  编译测试 + 安全审计
```

### Phase 7：清理（0.5 周）

```
Step 7.1  删除 extern object 语法（parser 报错）
Step 7.2  删除 extern "c" 语法（parser 报错）
Step 7.3  删除 aura.lang.native 中的 deprecated 标记
Step 7.4  更新文档
Step 7.5  自举验证（Aura 编译器编译 Aura 编译器）
```

**总计**：约 6 周

---

## 九、迁移检查清单

### 编译器端

- [ ] `parser.rs`：`extern object` 解析改为警告
- [ ] `parser.rs`：`extern interface` 支持无字符串参数名
- [ ] `parser.rs`：`extern interface` 仅允许 `loadLibrary` 有函数体
- [ ] `parser.rs`：`extern interface` 允许 `const val`（常量）
- [ ] `parser.rs`：`extern interface` 禁止 `var`（sema 报错）
- [ ] `parser.rs`：`extern interface` 禁止非 `loadLibrary` 函数体（sema 报错）
- [ ] `parser.rs`：`extern interface` 允许 `@native` 注解
- [ ] `parser.rs`：`extern "c"` 解析改为警告
- [ ] `parser.rs`：新增 `interface` 关键字（面向对象接口）
- [ ] `sema/checker.rs`：`ExternObjectDecl` 节点废弃
- [ ] `sema/checker.rs`：`ExternInterfaceDecl` 统一处理（成员分类：Constant/Declaration/LoadLibrary）
- [ ] `sema/checker.rs`：保留名锁定表
- [ ] `sema/checker.rs`：非 `loadLibrary` 函数体禁止
- [ ] `sema/checker.rs`：`var` 禁止
- [ ] `sema/checker.rs`：用户可声明 `extern interface "name"`
- [ ] `sema/checker.rs`：用户不可声明保留名
- [ ] `sema/checker.rs`：`aura.lang.native` import 拒绝
- [ ] `sema/checker.rs`：`@native` 用户声明拒绝
- [ ] `sema/checker.rs`：接口继承方法签名验证
- [ ] `codegen/hir.rs`：`ExternInterfaceDecl` HIR 统一（成员分类）
- [ ] `codegen/aot/emit.rs`：内置 native 发射路径
- [ ] `codegen/aot/emit.rs`：`loadLibrary` 发射（define）
- [ ] `codegen/aot/emit.rs`：纯声明发射（declare）
- [ ] `codegen/aot/emit.rs`：常量折叠
- [ ] `codegen/aot/ffi.rs`：外部 FFI 发射路径（用户 CFFI + Aura FFI）
- [ ] `codegen/intrinsics.rs`（新增）：NATIVE_FUNCTION_MAP
- [ ] `codegen/aot/runtime.rs`：更新 legacy 符号映射
- [ ] `std/cffi/aura_std_cffi.c`：无需改动
- [ ] `std/cffi/aura_std_cffi.h`：无需改动

### 源码端

- [ ] `Memory.aura`：`extern object` → `extern interface`
- [ ] `Cpu.aura`：`extern object` → `extern interface`
- [ ] `ThreadOps.aura`：`extern object` → `extern interface`
- [ ] `Syscalls.aura`：`object SyscallsUtils` → `extern interface Syscalls`（val → const val）
- [ ] `Console.aura`：拆分 `extern interface Console` + `object ConsoleImpl`
- [ ] `FileOps.aura`：拆分 `extern interface FileOps` + `object FileOpsImpl`
- [ ] `Runtime.aura`：拆分 `extern interface Runtime` + `object RuntimeImpl`
- [ ] `Clock.aura`：拆分 `extern interface Clock` + `object ClockImpl`
- [ ] `ProcessOps.aura`：拆分 `extern interface ProcessOps` + `object ProcessOpsImpl` + `extern interface "runtime"`
- [ ] `native/arch/*/Syscalls.aura`：统一为 `extern interface`
- [ ] `Random.aura`：`object` → `class Random`
- [ ] `Thread.aura`：`object` → `class Thread`
- [ ] `Process.aura`：`object` → `object Process` + `class ProcessHandle`
- [ ] 新增 `File.aura`：`class File`
- [ ] 新增 `Socket.aura` / `ServerSocket.aura`：`class`
- [ ] `Console.aura`（std 层）：增加 `stdout`/`stderr`/`stdin` 属性
- [ ] 新增 `OutputStream.aura` / `InputStream.aura`：接口定义
- [ ] `FileSystem.aura`：便捷入口委托 `File` 实例
- [ ] 所有 import 路径更新
- [ ] `prelu.aura`：确认无需改动

### 验证

- [ ] `cargo build` 编译通过
- [ ] `cargo test` 全部测试通过
- [ ] `aura --self-host` 自举验证通过
- [ ] AOT 编译 + 执行通过
- [ ] VM 编译 + 执行通过
- [ ] JIT 编译 + 执行通过
- [ ] 用户 C FFI 测试（`extern interface "math" { ... }`）通过
- [ ] 用户 Aura AOT FFI 测试通过
- [ ] `extern interface` 内 const val 常量折叠验证
- [ ] `extern interface` 内非 `loadLibrary` 函数体禁止验证（sema 报错）
- [ ] `extern interface` 内 var 禁止验证（sema 报错）
- [ ] `loadLibrary` 函数体验证（仅 Aura AOT FFI 可用）
- [ ] `class Random` 实例化 + 多次调用通过
- [ ] `class File` 实例化 + readText/writeText 通过
- [ ] 接口多态测试（`OutputStream` 注入 mock）通过
- [ ] 安全审计：用户无法 import `aura.lang.native`
- [ ] 安全审计：用户无法声明 `@native`
- [ ] 安全审计：用户无法声明保留名
- [ ] 安全审计：用户可声明 `extern interface "name"`（仅声明 + 常量 + loadLibrary）

---

## 十、架构总览（变更后）

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           用户代码                                        │
│                                                                          │
│  // 无状态工具                                                           │
│  Math.sin(3.14)     Time.now()      Console.println("hello")            │
│                                                                          │
│  // 有状态实例（面向对象）                                                │
│  val f = File("data.txt"); f.readText()                                  │
│  val rng = Random(seed = 42); rng.nextInt(100)                           │
│  val sock = Socket("example.com", 80); sock.connect()                   │
│  val t = Thread { doWork() }; t.start(); t.join()                       │
│                                                                          │
│  // 接口多态                                                             │
│  fun writeTo(out: OutputStream, msg: String) { out.println(msg) }       │
│  val file = File("log.txt"); writeTo(file, "entry")                     │
│                                                                          │
│  // 用户 FFI（仅声明 + 常量 + loadLibrary，实现移到 object）              │
│  extern interface "raylib" {                                            │
│      const val WHITE: Int = 0xFFFFFFFF                                   │
│      fun InitWindow(w: Int, h: Int, t: CString): Unit                   │
│      fun BeginDrawing(): Unit                                            │
│  }                                                                       │
│                                                                          │
│  object RaylibWrapper {                                                  │
│      fun clearBackground(color: Int) { Raylib.ClearBackground(...) }    │
│      fun runGameLoop() { ... }                                          │
│  }                                                                       │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│              aura.lang.std.*  (object + class，纯 Aura)                    │
│                                                                          │
│  object Math         无状态工具                                          │
│  object Console      系统资源（stdout/stderr 属性可替换）                  │
│  object Time         无状态工具                                          │
│  object Env          无状态工具                                          │
│  object FileSystem   便捷入口（委托 File）                               │
│  object IO           便捷入口（委托 Console）                            │
│  object Builtin      工具函数                                            │
│                                                                          │
│  class File          资源实例（路径构造）                                │
│  class Random        有状态实例（种子构造）                              │
│  class Socket        资源实例（host/port 构造）                          │
│  class ServerSocket  资源实例（port 构造）                               │
│  class Thread        资源实例（线程体构造）                              │
│  class ProcessHandle 资源实例（pid 构造）                               │
│  class Channel       资源实例                                            │
│  class Mutex         资源实例                                            │
│  class Actor         资源实例                                            │
│  class Future        结果容器                                            │
│                                                                          │
│  interface OutputStream  多态接口                                        │
│  interface InputStream   多态接口                                        │
│                                                                          │
│  ─── 用户不可 import 以下包 ───                                           │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│            aura.lang.native.*  (用户不可见)                               │
│                                                                          │
│  extern interface Memory       编译器内置 → LLVM load/store              │
│    ├── fun read(addr: Long): Byte   纯声明                              │
│    └── fun write(addr: Long, v: Byte)   纯声明                         │
│                                                                          │
│  extern interface Cpu          编译器内置 → LLVM inline asm              │
│    ├── fun rdtsc(): Long           纯声明                                │
│    └── fun memFence()             纯声明                                 │
│                                                                          │
│  extern interface Syscalls     编译器内置 → aura_syscall_dispatch       │
│    ├── const val SYS_READ: Int = 0    编译期常量                         │
│    ├── @native(SYS_READ) fun read(...)   纯声明 → declare              │
│    └── ...                                                               │
│                                                                          │
│  extern interface Runtime      编译器内置 → arc_inc/dec, coroutine_yield │
│    ├── fun arcIncrement(ptr: Long)   纯声明                              │
│    └── fun arcDecrement(ptr: Long): Long   纯声明                       │
│                                                                          │
│  extern interface ThreadOps    编译器内置 → aura_thread_create/join     │
│    ├── fun create(fn_id: Int, arg: Int): Int   纯声明                   │
│    └── fun join(thread_id: Int): Int   纯声明                            │
│                                                                          │
│  extern interface Console      仅声明 → @native(SYS_WRITE) writeStdout  │
│  extern interface FileOps      仅声明 → @native(SYS_OPEN/READ/WRITE/...)│
│  extern interface Clock        仅声明 → @native(SYS_CLOCK_GETTIME)      │
│  extern interface ProcessOps   仅声明 → @native(SYS_EXIT/WAIT/FORK/...)  │
│  extern interface "runtime"    外部 C → aura_process_argCount/args      │
│                                                                          │
│  object ConsoleImpl            实现层（组合 Console.writeStdout）        │
│  object FileOpsImpl            实现层（组合 FileOps.open/read/...）       │
│  object RuntimeImpl            实现层（ARC 计数 + 内存诊断）              │
│  object ClockImpl              实现层（组合 Clock.clockGettime）          │
│  object ProcessOpsImpl         实现层（组合 ProcessOps.exit/wait/...）   │
│  object Stdio                  实现层（组合 ConsoleImpl + FileOpsImpl）  │
│  object ProcessNative          实现层（组合 Syscalls + Memory）           │
│  object NetworkOps             实现层（组合 Syscalls + FileOps）          │
│  object EnvOps                 实现层（组合 FileOps + Memory）            │
│  object Allocator              实现层（委托 Memory.alloc/free）           │
│  object MathOps                纯 Aura（Taylor 级数 / 牛顿法）            │
│  object PlanA                  纯 Aura（装箱/拆箱）                       │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│              编译器 / 运行库  (Rust + C)                                  │
│                                                                          │
│  AOT: LLVM IR 发射 + aura_std_cffi.c 链接                              │
│  VM:  NativeRegistry + aura_syscall_dispatch()                          │
│  JIT: Cranelift 直接指令生成                                            │
│                                                                          │
│  extern interface 编译规则：                                             │
│  - const val → 常量折叠（所有接口）                                      │
│  - fun f(): R（无函数体）→ LLVM declare（所有接口）                      │
│  - @native(N) fun f(): R（无函数体）→ 包装器 + declare（编译器内部）    │
│  - default fun loadLibrary() → LLVM define（仅 Aura AOT FFI）           │
│  - 非 loadLibrary 函数体 → sema 报错                                    │
│  - var → sema 报错（移到 object）                                       │
│                                                                          │
│  保留名检查：                                                            │
│  - Memory/Cpu/Syscalls/Runtime/ThreadOps → 编译器内置                   │
│  - 用户声明保留名 → sema 报错                                           │
│  - 用户声明 @native → sema 报错                                         │
│                                                                          │
│  用户 FFI:                                                               │
│  - C FFI: dlopen("libraylib.so") + GetProcAddress                      │
│  - Aura AOT FFI: JitValue ABI 直调                                      │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 十一、与旧方案对比

| 维度 | 旧方案（Phantom Source Tree） | 新方案（纯 Aura + extern interface） |
|------|-----------------------------|-----------------------------------|
| 源码真相源 | 虚拟源码 + 元数据 | 实际 `.aura` 文件 |
| IDE 导航 | LSP 虚拟文件协议 | IDE 直接打开 `.aura` 文件 |
| 自举能力 | 需要 Rust/C 实现 | 纯 Aura 实现，可自举 |
| native 暴露 | `extern object` 混合声明/实现 | `extern interface` 仅声明 + `object` 实现 |
| 迁移方式 | 拆分文件 | 拆分文件（声明 → interface，实现 → object） |
| FFI 入口 | `extern "c"` / `extern object` / `extern interface` 三套 | `extern interface` 一套 |
| 用户 FFI | 受限 | 用户可定义 CFFI + Aura FFI（仅声明 + 常量 + loadLibrary） |
| extern interface 常量 | ❌ 不支持 | ✅ `const val` 编译期常量 |
| extern interface 默认实现 | ❌ 不支持 | ❌ 不允许（除 `loadLibrary`），移到 `object` |
| extern interface var | ❌ 不支持 | ❌ 不允许，移到 `object` |
| 编译器内部接口 | `extern object` 全部成员当外部符号 | 仅声明 + 常量，实现移到 `object` |
| std 设计 | 全部 `object` 单例 | `object`（无状态）+ `class`（有状态） |
| 多态支持 | ❌ 无 | ✅ 接口 + 继承 |
| 可测试性 | ❌ 全局状态 | ✅ 实例注入 mock |
| 资源管理 | ❌ 无 close | ✅ class 可 close/finalize |
| 运行时开销 | 零开销 | 零开销 |

---

## 十二、风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| 拆分文件后引用路径变化 | 现有代码 | 保持 object 名称一致（ConsoleImpl 等），仅移动声明 |
| `@native` 在 `extern interface` 内 vs `object` 内的行为差异 | 编译器 | 统一处理：`@native` 成员始终生成包装器 |
| 非 `loadLibrary` 函数体检查可能遗漏 | 编译器 | sema 阶段检查所有 `extern interface` 成员 |
| `var` 检查可能遗漏 | 编译器 | sema 阶段检查所有 `extern interface` 成员 |
| `class` 转换带来 ARC 开销 | 运行时 | `value class` 用于轻量值，`class` 用于资源 |
| `class File` 与 `object FileSystem` 并存 | 设计一致性 | FileSystem 委托 File，保持便捷入口 |
| 接口继承在 VM 模式下的 vtable 支持 | 运行时 | 确认 VM vtable 支持接口多态 |
| `interface` 关键字与 `extern interface` 命名冲突 | 解析器 | `interface` 和 `extern interface` 是不同语法 |
