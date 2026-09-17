# Native 层封装与 `extern interface` 统一方案

> **状态**：设计方案 | **优先级**：P0 | **前置**：当前 `aura/core/aura/lang/` 纯 Aura 实现已可自举
>
> **核心决策**：
> 1. 放弃 Phantom Source Tree 方案（过时），以现有纯 Aura 源码为真相源
> 2. 保留 `aura.lang.native` 实现层，但用户不可直接 import
> 3. 统一 `extern object` → `extern interface`，消除声明/实现混合
> 4. `extern interface` 是**统一 FFI 机制**：编译器内置 native + 用户 C FFI + 用户 Aura AOT FFI 共用同一语法
> 5. std 层采用**混合模式**：无状态用 `object`，有状态用 `class`（面向对象）
> 6. 仅给用户暴露 `aura.lang.std.*`，不允许 import `aura.lang.native.*`

---

## 一、现状问题

### 1.1 `extern object` 的设计错误

`extern object` 让编译器将**全部成员**按外部 C 符号处理（只 `declare`、不发射函数体），导致三类问题：

| 问题 | 示例 | 后果 |
|------|------|------|
| Aura 实现被丢弃 | `Console.intToStr()`、`Clock.now()`、`ProcessOps.exit()` | AOT 报 `use of undefined value` |
| 外部 C 声明缺 `@native` 注解 | `ProcessOps.aura_process_argCount()` | 符号悬空为未定义 |
| 接收者参数错位 | `ProcessOps.aura_process_arg(i)` | C 侧参数槽错位，恒返回 argv[0] |

### 1.2 受影响的文件清单

| 文件 | 声明类型 | Aura 实现 | 外部 C 声明 | 当前状态 |
|------|---------|----------|------------|---------|
| `Memory.aura` | `extern object` | ❌ | ❌ | ✅ 纯声明，仅需改关键词 |
| `Cpu.aura` | `extern object` | ❌ | ❌ | ✅ 纯声明，仅需改关键词 |
| `ThreadOps.aura` | `extern object` | ❌ | ❌ | ✅ 纯声明，仅需改关键词 |
| `Console.aura` | `extern object` | ✅ `intToStr`/`init`/`getNewlineBuffer` | ❌ | ❌ 混合，需拆分 |
| `FileOps.aura` | `extern object` | ✅ `openFile`/`exists`/`mkdirFile` | ❌ | ❌ 混合，需拆分 |
| `Runtime.aura` | `extern object` | ✅ `init`/`memUsedMb`/`trackAlloc` | ❌ | ❌ 混合，需拆分 |
| `Clock.aura` | `extern object` | ✅ `init`/`now`/`timeMs`/`timeNs`/`sleep` | ❌ | ❌ 混合，需拆分 |
| `ProcessOps.aura` | `extern object` | ✅ `exit`/`wait`/`exec`/`getPid`/`forkProcess` | ✅ `aura_process_argCount`/`args` | ❌ 混合，需拆分 |
| `NetworkOps.aura` | `object` ✅ | ✅ `close`/`tcpSend`/`tcpRecv` | ❌ | ✅ 已是正确模式 |
| `ProcessNative.aura` | `object` ✅ | ✅ 全部 | ❌ | ✅ 已是正确模式 |
| `Stdio.aura` | `object` ✅ | ✅ 全部 | ❌ | ✅ 已是正确模式 |
| `EnvOps.aura` | `object` ✅ | ✅ 全部 | ❌ | ✅ 已是正确模式 |
| `Allocator.aura` | `object` ✅ | ✅ 全部 | ❌ | ✅ 已是正确模式 |
| `MathOps.aura` | `object` ✅ | ✅ 全部 | ❌ | ✅ 已是正确模式 |
| `PlanA.aura` | `object` ✅ | ✅ 全部 | ❌ | ✅ 已是正确模式 |

**结论**：9 个文件使用 `extern object`，其中 6 个是纯声明（仅需改关键词），3 个是混合体（需拆分）。

---

## 二、设计原则

### 2.1 `extern interface` 是唯一 FFI 声明边界

`extern interface` 不是编译器专有的概念——它是 Aura 语言的**统一 FFI 机制**，面向所有用户开放。编译器内置的 native 操作（Memory/Cpu/Syscalls）只是 `extern interface` 的特殊实例（通过保留名区分）。

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         extern interface                                   │
│                                                                          │
│  ┌─ 编译器内置（保留名，编译器自动识别）──────────────────────────────┐   │
│  │  extern interface Memory    →  LLVM load/store intrinsic           │   │
│  │  extern interface Cpu       →  LLVM inline asm                     │   │
│  │  extern interface Syscalls  →  aura_syscall_dispatch()             │   │
│  │  extern interface Runtime   →  arc_inc/dec, coroutine_yield        │   │
│  │  extern interface ThreadOps →  aura_thread_create/join/sleep       │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  ┌─ 用户 C FFI（用户自定义，字符串名 = 库名）──────────────────────────┐   │
│  │  extern interface "raylib"    { fun InitWindow(...): Unit }        │   │
│  │  extern interface "sqlite3"   { fun open(path, db): Int }          │   │
│  │  extern interface "OpenSSL"   { fun SSL_library_init() }          │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                          │
│  ┌─ 用户 Aura AOT FFI（字符串名 = Aura 动态库名）─────────────────────┐   │
│  │  extern interface "mylib"                                       │   │
│  │    default fun loadLibrary() = "mylib"                          │   │
│  │    fun compute(data: Long, size: Int): Long                      │   │
│  └────────────────────────────────────────────────────────────────────┘   │
│                                                                          │
├──────────────────────────────────────────────────────────────────────────┤
│  规则：                                                                 │
│  - 所有 extern interface 内仅允许纯声明（无函数体 { }）                   │
│  - 保留名（Memory/Cpu/Syscalls/Runtime/ThreadOps）由编译器特殊处理      │
│  - 字符串名 → 用户 FFI（C 或 Aura AOT，由 loadLibrary 区分）            │
│  - 用户**可定义**自己的 extern interface（CFFI + Aura FFI）              │
│  - 用户**不可定义**保留名（sema 报错）                                  │
└──────────────────────────────────────────────────────────────────────────┘
```

### 2.2 三条规则

| # | 规则 | 说明 |
|---|------|------|
| R1 | `extern interface` 内**不允许函数体** | 所有成员是纯声明（无 `{ }`） |
| R2 | `object` / `class` 内**允许** `@native` 注解 | 编译器为每个 `@native` 成员生成包装器 |
| R3 | 用户**可**声明 `extern interface`（CFFI + Aura FFI） | 但不可声明保留名，不可声明 `@native` 注解 |

### 2.3 用户 FFI 示例

```aura
// ═══════════════════════════════════════════════════════
// 用户 C FFI：调用外部 C 库
// ═══════════════════════════════════════════════════════

// 调用 raylib 图形库
extern interface "raylib" {
    fun InitWindow(width: Int, height: Int, title: CString): Unit
    fun BeginDrawing(): Unit
    fun EndDrawing(): Unit
    fun ClearBackground(r: Int, g: Int, b: Int, a: Int): Unit
    fun DrawText(text: CString, x: Int, y: Int, fontSize: Int, color: Int): Unit
    fun IsWindowClosed(): Boolean
    fun CloseWindow(): Unit
}

// 调用 sqlite3 数据库
extern interface "sqlite3" {
    fun sqlite3_open(path: CString, db: Long): Int
    fun sqlite3_close(db: Long): Int
    fun sqlite3_prepare_v2(db: Long, sql: CString, n: Int, stmt: Long, tail: Long): Int
    fun sqlite3_step(stmt: Long): Int
    fun sqlite3_column_text(stmt: Long, col: Int): Long
    fun sqlite3_finalize(stmt: Long): Int
}

// ═══════════════════════════════════════════════════════
// 用户 Aura AOT FFI：调用 Aura 编译的动态库
// ═══════════════════════════════════════════════════════

// 调用 Aura AOT 编译的图像库
extern interface "aura_image" {
    default fun loadLibrary(): String = "aura_image"
    fun loadImage(path: String): Long
    fun getImageWidth(img: Long): Int
    fun getImageHeight(img: Long): Int
    fun getPixel(img: Long, x: Int, y: Int): Int
}

// 调用 Aura AOT 编译的加密库
extern interface "aura_crypto" {
    default fun loadLibrary(): String = "aura_crypto"
    fun sha256(data: Long, len: Int): Long
    fun hmacSign(key: Long, data: Long, len: Int): Long
}
```

### 2.4 编译器内置识别规则

编译器按以下优先级识别 `extern interface` 的目标：

| 优先级 | 条件 | 处理方式 | 调用 ABI |
|--------|------|---------|---------|
| 1 | 接口名 = 保留名 | 编译器内置 intrinsics | LLVM IR 直接发射 |
| 2 | 字符串名 + `default fun loadLibrary()` | 外部 Aura AOT 库 | JitValue ABI |
| 3 | 字符串名（无 loadLibrary） | 外部 C 库 | C ABI（dlopen） |

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
// 当前：object 单例 ✅
Math.sin(3.14)    // 无状态，无需实例
Math.max(1, 2)    // 无状态
```

Java 也是 `Math.sin()`（静态方法）。Kotlin 是 `kotlin.math.sin()`（顶层函数）。
**结论**：保持 `object Math`。

#### IO / Console — 系统资源 → `object` + 属性

```aura
// 当前：object 单例
IO.println("hello")    // 简洁

// Java: System.out.println("hello")  — out 是 PrintStream 实例
// Kotlin: println("hello")  — 顶层函数
```

但需要支持输出重定向。推荐：

```aura
// Console: 系统资源单例（stdout/stderr 是属性）
object Console {
    val stdout: OutputStream    // 默认 stdout，可替换
    val stderr: OutputStream    // 默认 stderr，可替换
    val stdin: InputStream      // 默认 stdin

    fun println(msg: String): Unit { stdout.println(msg) }
    fun print(msg: String): Unit { stdout.print(msg) }
}

// 输出重定向
Console.stdout = File("log.txt").writer()   // 重定向到文件
Console.stdout = Console.createConsole()    // 恢复默认
```

**结论**：`object Console` + 可替换属性 + `class OutputStream/InputStream`。

#### FileSystem / File — 资源对象 → `class` 最合适

```aura
// 当前：object 单例（路径作为参数）
FileSystem.readText("/path/to/file")    // 每次传路径

// 推荐：class 实例（路径是构造参数）
val file = File("/path/to/file")
val content = file.readText()
file.close()

// 更简洁的单次调用（通过 object FileSystem 保持便利）
FileSystem.readText("/path")    // 保留便捷 API
```

**结论**：`class File` + `object FileSystem`（便捷入口）。

#### Random — 有状态（种子）→ `class` 最合适

```aura
// 有状态操作（内部维护种子）
val rng = Random()              // 默认种子
val rng2 = Random(seed = 42)   // 指定种子
val x = rng.nextInt(100)       // 每次调用改变状态
```

Java: `new Random().nextInt()`。Kotlin: `Random().nextInt()`。
**结论**：`class Random`。

#### Network / Socket — 资源对象 → `class` 最合适

```aura
// 有状态的连接对象
val server = ServerSocket(port = 8080)
val client = server.accept()   // 阻塞等待连接
val data = client.read(1024)
client.write(data)
client.close()
server.close()
```

Java: `new ServerSocket(port)`。Kotlin: `ServerSocket(port)`。
**结论**：`class ServerSocket` + `class Socket`。

#### Process — 混合模式

```aura
// 创建（无状态操作）→ object
val pid = Process.spawn("ls -la")

// 操作（有状态）→ class 实例
val proc = ProcessHandle(pid)
proc.wait()
proc.kill()
```

**结论**：`object Process`（创建）+ `class ProcessHandle`（操作）。

#### Thread — 资源对象 → `class` 最合适

```aura
val t = Thread {
    // 线程体
    doWork()
}
t.start()
t.join()
```

**结论**：`class Thread`。

#### Collections — 已有实例 → 保持

```aura
val list = mutableListOf<Int>()    // 实例 ✅
val map = mutableMapOf<String, Int>()  // 实例 ✅
```

当前集合已经是实例化的。保持。

#### Time / Clock — 无状态工具 → `object`

```aura
Time.now()           // 无状态
Time.elapsed()       // 无状态
Time.sleep(1000)     // 无状态
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

面向对象的关键优势是**多态**。推荐定义接口让实例可替换：

```aura
// 接口：输出流（类似 Java 的 OutputStream）
interface OutputStream {
    fun write(bytes: Array<Byte>): Unit
    fun writeInt(value: Int): Unit
    fun println(msg: String): Unit
    fun print(msg: String): Unit
    fun flush(): Unit
}

// 接口：输入流
interface InputStream {
    fun read(buffer: Long, count: Int): Int
    fun readLine(): String
    fun readAll(): String
    fun close(): Unit
}

// 接口：文件
interface File {
    fun readText(): String
    fun writeText(content: String): Unit
    fun readBytes(): Array<Byte>
    fun exists(): Boolean
    fun size(): Long
}
```

这样用户可以传递抽象接口，实现测试和组合：

```aura
// 测试：注入 mock
fun processInput(input: InputStream): String {
    return input.readLine().trim()
}

fun test(): Unit {
    val mock = MockInputStream(["hello", "world"])
    val result = processInput(mock)
    // assert result == "hello"
}
```

---

## 四、语法规范

### 4.1 `extern interface` 声明语法

```aura
// ── 编译器内置 native（保留名，编译器自动识别）──
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
    fun read(fd: Long, buf: Long, count: Long): Long
    fun write(fd: Long, buf: Long, count: Long): Long
    fun open(path: Long, flags: Int): Int
    fun close(fd: Int): Int
    fun fstat(fd: Int, buf: Long): Long
    fun lseek(fd: Int, off: Long, whence: Int): Long
    fun mmap(addr: Long, length: Long, prot: Int, flags: Int, fd: Int, offset: Long): Long
    fun munmap(addr: Long, length: Long): Int
    fun access(path: Long, mode: Int): Int
    fun unlink(path: Long): Int
    fun mkdir(path: Long, mode: Int): Int
    fun rmdir(path: Long): Int
    fun rename(old: Long, new: Long): Int
    fun execve(path: Long, args: Long, env: Long): Long
    fun exit(code: Int)
    fun exitGroup(code: Int)
    fun wait4(pid: Int, status: Long, options: Int, rusage: Long): Int
    fun fork(): Int
    fun kill(pid: Int, sig: Int): Int
    fun getpid(): Int
    fun clockGettime(clock: Int, ts: Long): Long
    fun getrandom(buf: Long, len: Long, flags: Int): Int
    fun pipe(pipes: Long): Int
    fun socket(domain: Int, type: Int, protocol: Int): Int
    fun connect(fd: Int, addr: Long, addrlen: Int): Int
    fun bind(fd: Int, addr: Long, addrlen: Int): Int
    fun listen(fd: Int, backlog: Int): Int
    fun accept(fd: Int, addr: Long, addrlen: Long): Int
    fun sendto(fd: Int, buf: Long, len: Int, flags: Int, addr: Long, addrlen: Int): Long
    fun recvfrom(fd: Int, buf: Long, len: Int, flags: Int, addr: Long, addrlen: Int): Long
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

// ── 用户 C FFI（用户自定义）──
extern interface "raylib" {
    fun InitWindow(width: Int, height: Int, title: CString): Unit
    fun BeginDrawing(): Unit
    fun EndDrawing(): Unit
    fun IsWindowClosed(): Boolean
    fun CloseWindow(): Unit
}

extern interface "sqlite3" {
    fun sqlite3_open(path: CString, db: Long): Int
    fun sqlite3_close(db: Long): Int
}

// ── 用户 Aura AOT FFI（用户自定义）──
extern interface "aura_image" {
    default fun loadLibrary(): String = "aura_image"
    fun loadImage(path: String): Long
    fun getImageWidth(img: Long): Int
}
```

### 4.2 `object` / `class` 实现语法

```aura
// ── object: 无状态工具 ──
object Math {
    fun sin(x: Float): Float { ... }
    fun cos(x: Float): Float { ... }
}

// ── object: 系统资源（Console）──
object Console {
    var stdout: OutputStream = DefaultConsoleOut()
    var stderr: OutputStream = DefaultConsoleErr()
    var stdin: InputStream = DefaultConsoleIn()

    fun println(msg: String): Unit { stdout.println(msg) }
    fun print(msg: String): Unit { stdout.print(msg) }
}

// ── class: 有状态实例（File）──
class File(path: String) {
    private val pathBuf: Long = Stdio.stringToBuffer(path)

    fun readText(): String {
        val fd = FileOps.openFile(pathBuf, 0)
        // ... 读取 ...
        return Stdio.bufferToString(buf, len)
    }

    fun writeText(content: String): Unit {
        val fd = FileOps.openFile(pathBuf, 1 | 0x40 | 0x200)
        // ... 写入 ...
    }

    fun exists(): Boolean { return Stdio.fileExists(path) }
    fun size(): Long { return FSUtils.fs_file_size(path) }
}

// ── class: 有状态实例（Random）──
class Random(seed: Long = 0) {
    private var state: Long = seed

    fun nextInt(bound: Int): Int {
        state = state * 6364136223846793005 + 1442695040888963407
        return ((state as Long) % bound as Long) as Int
    }
}

// ── class: 有状态实例（Socket）──
class Socket(host: String, port: Int) {
    private var fd: Int = 0

    fun connect(): Unit {
        val addrBuf: Long = Allocator.malloc(16)
        Memory.write32(addrBuf, 2)          // AF_INET
        Memory.write32(addrBuf + 4, port)   // port (network order)
        fd = NetworkOps.socket(2, 1, 6)     // AF_INET, SOCK_STREAM, TCP
        NetworkOps.connect(fd, addrBuf, 16)
    }

    fun close(): Unit { NetworkOps.close(fd) }
}

// ── object: 便捷入口（FileSystem）──
object FileSystem {
    fun readText(path: String): String { return File(path).readText() }
    fun writeText(path: String, content: String): Unit { File(path).writeText(content) }
    fun exists(path: String): Boolean { return File(path).exists() }
    fun delete(path: String): Unit { FSUtils.fs_delete(path) }
    fun mkdir(path: String): Unit { FSUtils.fs_mkdir_new(path) }
}
```

### 4.3 废弃语法

| 旧语法 | 新语法 | 原因 |
|--------|--------|------|
| `extern object X { ... }` | `extern interface X { ... }` | 消除声明/实现混合 |
| `extern "c" "lib" { ... }` | `extern interface "lib" { ... }` | 统一 FFI 入口 |
| `extern interface X { default fun loadLibrary() = ...; fun f() }` | 保持不变 | 已有实现，向后兼容 |

---

## 五、文件变更清单

### 5.1 仅需改关键词（`extern object` → `extern interface`）

| 文件 | 变更 | 影响 |
|------|------|------|
| `Memory.aura` | `extern object` → `extern interface` | 编译器识别为内置 native |
| `Cpu.aura` | `extern object` → `extern interface` | 编译器识别为内置 native |
| `ThreadOps.aura` | `extern object` → `extern interface` | 编译器识别为内置 native |

### 5.2 拆分（声明移入 `extern interface`，实现移入 `object`）

#### Console.aura

**变更后**：

```aura
// ── 声明 ──
extern interface Console {
    fun writeStdout(buf: Long, count: Long): Long
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
extern interface FileOpsDecl {
    fun open(path: Long, flags: Int): Int
    fun close(fd: Int): Int
    fun read(fd: Int, buf: Long, count: Long): Long
    fun write(fd: Int, buf: Long, count: Long): Long
    fun lseek(fd: Int, off: Long, whence: Int): Long
    fun fstat(fd: Int, buf: Long): Long
    fun unlink(path: Long): Int
    fun access(path: Long, mode: Int): Int
    fun mkdir(path: Long, mode: Int): Int
    fun rmdir(path: Long): Int
    fun rename(old: Long, new: Long): Int
}

// ── 实现 ──
object FileOps {
    const val O_RDONLY: Int = 0
    const val O_WRONLY: Int = 1
    // ... constants ...

    fun openFile(path: Long, flags: Int): Int { return FileOpsDecl.open(path, flags) }
    fun closeFile(fd: Int): Int { return FileOpsDecl.close(fd) }
    fun readFile(fd: Int, buf: Long, count: Long): Long { return FileOpsDecl.read(fd, buf, count) }
    fun writeFile(fd: Int, buf: Long, count: Long): Long { return FileOpsDecl.write(fd, buf, count) }
    fun seekFile(fd: Int, offset: Long, whence: Int): Long { return FileOpsDecl.lseek(fd, offset, whence) }
    fun statFile(fd: Int, buf: Long): Long { return FileOpsDecl.fstat(fd, buf) }
    fun deleteFile(path: Long): Int { return FileOpsDecl.unlink(path) }
    fun exists(path: Long): Boolean { return FileOpsDecl.access(path, 0) == 0 }
    fun mkdirFile(path: Long, mode: Int): Int { return FileOpsDecl.mkdir(path, mode) }
    fun rmdirFile(path: Long): Int { return FileOpsDecl.rmdir(path) }
}
```

#### Runtime.aura

**变更后**：

```aura
// ── 声明 ──
extern interface RuntimeDecl {
    fun arcIncrement(ptr: Long)
    fun arcDecrement(ptr: Long): Long
    fun coroutineYield(ctx: Long)
}

// ── 实现 ──
object Runtime {
    const val ARC_COUNT_OFFSET: Long = 0
    var allocTotal: Long = 0
    var allocLive: Long = 0
    var allocCount: Int = 0
    var freeCount: Int = 0

    fun init(): Boolean { ... }
    fun version(): String { return "Aura Runtime 0.1.0" }
    fun memUsedMb(): Int { ... }
    fun trackAlloc(size: Long) { ... }
    fun trackFree(size: Long) { ... }
}
```

#### Clock.aura

**变更后**：

```aura
// ── 声明 ──
extern interface ClockDecl {
    fun clockGettime(clock: Int, ts: Long): Long
}

// ── 实现 ──
object Clock {
    const val CLOCK_REALTIME: Int = 0
    const val CLOCK_MONOTONIC: Int = 1
    const val CLOCK_PROCESS_CPUTIME_ID: Int = 2
    const val CLOCK_THREAD_CPUTIME_ID: Int = 3

    var tsBuf: Long = 0

    fun init(): Boolean { ... }
    fun now(): Time { ... }
    fun timeMs(): Long { ... }
    fun timeUs(): Long { ... }
    fun timeNs(): Long { ... }
    fun sleep(ms: Long) { ... }
}
```

#### ProcessOps.aura

**变更后**：

```aura
// ── syscall 声明（归入 Syscalls）──

// ── 外部 C 声明 ──
extern interface "runtime" {
    fun aura_process_argCount(): Long
    fun aura_process_args(): String
}

// ── 实现 ──
object ProcessOps {
    fun exit(code: Int) { Syscalls.exitGroup(code) }
    fun wait(pid: Int): Int { return Syscalls.wait4(pid, 0, 0, 0) }
    fun exec(path: Long, args: Long, env: Long): Long { return Syscalls.execve(path, args, env) }
    fun getPid(): Int { return Syscalls.getpid() }
    fun forkProcess(): Int { return Syscalls.fork() }
}
```

### 5.3 Syscalls.aura 重构

**变更后**：

```aura
// ── syscall 常量 ──
object SyscallConsts {
    const val SYS_READ: Int = 0
    const val SYS_WRITE: Int = 1
    const val SYS_OPEN: Int = 2
    const val SYS_CLOSE: Int = 3
    // ... (all existing constants)
}

// ── syscall 声明 ──
extern interface Syscalls {
    fun read(fd: Long, buf: Long, count: Long): Long
    fun write(fd: Long, buf: Long, count: Long): Long
    fun open(path: Long, flags: Int): Int
    fun close(fd: Int): Int
    // ... (all syscalls)
}
```

### 5.4 std 层重构（object → class）

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
| `extern interface` 拒绝函数体 | `parser.rs` | 遇到 `{` 报错 |
| `extern "c"` → `extern interface` | `parser.rs` | 废弃警告 |
| `interface` 关键字（新增） | `parser.rs` | 面向对象接口声明 |
| `class` 关键字（增强） | `parser.rs` | 确认已支持 |

### 6.2 Sema 变更

| 变更 | 文件 | 说明 |
|------|------|------|
| `ExternObjectDecl` → `ExternInterfaceDecl` | `sema/checker.rs` | 统一 AST |
| 保留名锁定表 | `sema/checker.rs` | `Memory`/`Cpu`/`Syscalls`/`Runtime`/`ThreadOps` |
| 字符串名 → FFI 类型推断 | `sema/checker.rs` | 有 `loadLibrary` → `FfiAbi::Aura`；无 → `FfiAbi::C` |
| 用户可声明 `extern interface` | `sema/checker.rs` | 允许字符串名 |
| 用户不可声明保留名 | `sema/checker.rs` | sema 报错 |
| 用户不可声明 `@native` | `sema/checker.rs` | sema 报错 |
| `aura.lang.native` import 拒绝 | `sema/checker.rs` | sema 报错 |
| 接口继承检查 | `sema/checker.rs` | 类实现接口的方法签名验证 |

### 6.3 Codegen 变更

| 变更 | 文件 | 说明 |
|------|------|------|
| `ExternInterfaceDecl` HIR 统一 | `codegen/hir.rs` | |
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
| `extern interface Memory { ... }` | ❌ | 保留名，sema 报错 |
| `@native(N) fun f()` | ❌ | sema 报错，仅编译器可发射 |
| import `aura.lang.native` | ❌ | sema 报错 |

### 7.3 编译器保护机制

| 机制 | 说明 |
|------|------|
| 保留名锁定 | `Memory`/`Cpu`/`Syscalls`/`Runtime`/`ThreadOps` 不可被用户声明 |
| `@native` 仅编译器可发射 | 用户源码中的 `@native` 在 sema 阶段报错 |
| `aura.lang.native` 包设为内部 | sema 拒绝用户 import |
| 外部 FFI 运行时加载 | `dlopen` / `LoadLibraryW` 在运行库中执行 |

---

## 八、实施路线

### Phase 1：语法迁移（1 周）

```
Step 1.1  编译器支持 extern interface 无参名语法（parser.rs）
Step 1.2  编译器拒绝 extern interface 内的函数体（parser.rs）
Step 1.3  编译器 deprecate extern object（parser.rs 警告）
Step 1.4  编译器 deprecate extern "c"（parser.rs 警告）
Step 1.5  新增 interface 关键字解析（面向对象接口）
Step 1.6  编译现有代码，确认无回归
```

### Phase 2：纯声明迁移（0.5 周）

```
Step 2.1  Memory.aura: extern object → extern interface
Step 2.2  Cpu.aura: extern object → extern interface
Step 2.3  ThreadOps.aura: extern object → extern interface
Step 2.4  编译测试，确认 AOT/VM 均可运行
```

### Phase 3：混合文件拆分（2 周）

```
Step 3.1  Console.aura → extern interface Console + object ConsoleImpl
Step 3.2  FileOps.aura → extern interface FileOpsDecl + object FileOps
Step 3.3  Runtime.aura → extern interface RuntimeDecl + object Runtime
Step 3.4  Clock.aura → extern interface ClockDecl + object Clock
Step 3.5  ProcessOps.aura → 迁移 syscall 到 Syscalls + extern interface "runtime" + object ProcessOps
Step 3.6  编译测试 + 自举验证
```

### Phase 4：Syscalls 统一（1 周）

```
Step 4.1  Syscalls.aura → extern interface Syscalls + object SyscallConsts
Step 4.2  native/arch/ 各平台 Syscalls 统一为 extern interface
Step 4.3  编译器 NATIVE_FUNCTION_MAP 完善
Step 4.4  编译测试 + 全量回归
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
Step 6.3  ema: 禁止用户声明 Memory/Cpu/Syscalls/Runtime/ThreadOps 保留名
Step 6.4  编译测试 + 安全审计
```

### Phase 7：清理（0.5 周）

```
Step 7.1  删除 extern object 语法（parser 报错）
Step 7.2  删除 extern "c" 语法（parser 报错）
Step 7.3  删除 aura.lang.native 中的 deprecated 标记
Step 7.4  更新文档
Step 7.5  自举验证（Aura 编译器编译 Aura 编译器）
```

**总计**：约 7.5 周

---

## 九、迁移检查清单

### 编译器端

- [ ] `parser.rs`：`extern object` 解析改为警告
- [ ] `parser.rs`：`extern interface` 支持无字符串参数名
- [ ] `parser.rs`：`extern interface` 拒绝函数体
- [ ] `parser.rs`：`extern "c"` 解析改为警告
- [ ] `parser.rs`：新增 `interface` 关键字（面向对象接口）
- [ ] `sema/checker.rs`：`ExternObjectDecl` 节点废弃
- [ ] `sema/checker.rs`：`ExternInterfaceDecl` 统一处理
- [ ] `sema/checker.rs`：保留名锁定表
- [ ] `sema/checker.rs`：用户可声明 `extern interface "name"`
- [ ] `sema/checker.rs`：用户不可声明保留名
- [ ] `sema/checker.rs`：`aura.lang.native` import 拒绝
- [ ] `sema/checker.rs`：`@native` 用户声明拒绝
- [ ] `sema/checker.rs`：接口继承方法签名验证
- [ ] `codegen/hir.rs`：`ExternInterfaceDecl` HIR 统一
- [ ] `codegen/aot/emit.rs`：内置 native 发射路径
- [ ] `codegen/aot/ffi.rs`：外部 FFI 发射路径（用户 CFFI + Aura FFI）
- [ ] `codegen/intrinsics.rs`（新增）：NATIVE_FUNCTION_MAP
- [ ] `codegen/aot/runtime.rs`：更新 legacy 符号映射
- [ ] `std/cffi/aura_std_cffi.c`：无需改动
- [ ] `std/cffi/aura_std_cffi.h`：无需改动

### 源码端

- [ ] `Memory.aura`：`extern object` → `extern interface`
- [ ] `Cpu.aura`：`extern object` → `extern interface`
- [ ] `ThreadOps.aura`：`extern object` → `extern interface`
- [ ] `Syscalls.aura`：拆分常量 + `extern interface`
- [ ] `Console.aura`：拆分 `extern interface` + `object`
- [ ] `FileOps.aura`：拆分 `extern interface` + `object`
- [ ] `Runtime.aura`：拆分 `extern interface` + `object`
- [ ] `Clock.aura`：拆分 `extern interface` + `object`
- [ ] `ProcessOps.aura`：拆分 `extern interface` + `object`
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
- [ ] `class Random` 实例化 + 多次调用通过
- [ ] `class File` 实例化 + readText/writeText 通过
- [ ] 接口多态测试（`OutputStream` 注入 mock）通过
- [ ] 安全审计：用户无法 import `aura.lang.native`
- [ ] 安全审计：用户无法声明 `@native`
- [ ] 安全审计：用户无法声明保留名
- [ ] 安全审计：用户可声明 `extern interface "name"`

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
│  // 用户 FFI                                                             │
│  extern interface "raylib" { fun InitWindow(...) }                      │
│  extern interface "aura_image" { default fun loadLibrary() = ... }     │
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
│  extern interface Cpu          编译器内置 → LLVM inline asm              │
│  extern interface Syscalls     编译器内置 → aura_syscall_dispatch       │
│  extern interface Runtime      编译器内置 → arc_inc/dec, coro_yield     │
│  extern interface ThreadOps    编译器内置 → aura_thread_create/join     │
│  extern interface "runtime"    外部 C → aura_process_argCount/args      │
│                                                                          │
│  object ConsoleImpl            Aura 实现（组合 Syscalls.write）           │
│  object FileOps                Aura 实现（组合 Syscalls.open/read/...）   │
│  object Stdio                  Aura 实现（组合 Console + FileOps）       │
│  object Clock                  Aura 实现（组合 Syscalls.clockGettime）   │
│  object ProcessNative          Aura 实现（组合 Syscalls + Memory）       │
│  object NetworkOps             Aura 实现（组合 Syscalls + FileOps）       │
│  object EnvOps                 Aura 实现（组合 FileOps + Memory）         │
│  object Allocator              Aura 实现（委托 Memory.alloc/free）        │
│  object MathOps                纯 Aura（Taylor 级数 / 牛顿法）            │
│  object PlanA                  纯 Aura（装箱/拆箱）                       │
│  object SyscallConsts          纯 Aura（syscall 号常量）                  │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│              编译器 / 运行库  (Rust + C)                                  │
│                                                                          │
│  AOT: LLVM IR 发射 + aura_std_cffi.c 链接                              │
│  VM:  NativeRegistry + aura_syscall_dispatch()                          │
│  JIT: Cranelift 直接指令生成                                            │
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
| native 暴露 | `extern object` 混合声明/实现 | `extern interface` 纯声明 |
| FFI 入口 | `extern "c"` / `extern object` / `extern interface` 三套 | `extern interface` 一套 |
| 用户 FFI | 受限 | 用户可定义 CFFI + Aura FFI |
| std 设计 | 全部 `object` 单例 | `object`（无状态）+ `class`（有状态） |
| 多态支持 | ❌ 无 | ✅ 接口 + 继承 |
| 可测试性 | ❌ 全局状态 | ✅ 实例注入 mock |
| 资源管理 | ❌ 无 close | ✅ class 可 close/finalize |
| 运行时开销 | 零开销 | 零开销 |

---

## 十二、风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| `extern interface` 无参名与现有语法冲突 | 解析器 | 逐步废弃 `extern object`，先警告后报错 |
| 用户声明 `extern interface "name"` 与保留名冲突 | 解析器 | sema 保留名检查 |
| 拆分文件后引用路径变化 | 现有代码 | 保持 object 名称不变，仅移动声明 |
| `class` 转换带来 ARC 开销 | 运行时 | `value class` 用于轻量值，`class` 用于资源 |
| `class File` 与 `object FileSystem` 并存 | 设计一致性 | FileSystem 委托 File，保持便捷入口 |
| 接口继承在 VM 模式下的 vtable 支持 | 运行时 | 确认 VM vtable 支持接口多态 |
| `interface` 关键字与 `extern interface` 命名冲突 | 解析器 | `interface` 和 `extern interface` 是不同语法 |
