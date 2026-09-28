# IO / Console / File 架构重构方案

> **状态**: 待评审  
> **影响范围**: `aura/core/` (6 个 .aura 文件) + `seed/compiler/src/` (10+ .rs 文件) + `aura/` 工具链 (40+ .aura 文件)  
> **目标**: 消除循环依赖、职责重叠、双实现路径；建立清晰的四层分层架构  
> **核心原则**: File.aura = 开发者主 API；FileUtils = 基于 File 的工具类；InputStream/OutputStream = 公共流 API

---

## 1. 当前问题分析

### 1.1 循环依赖链

```
Console.aura ──imports──▶ IO.aura ──imports──▶ Stdio.aura ──imports──▶ native.Console
    │
    └──imports──▶ std/io/DefaultConsoleOut ──imports──▶ IO.aura  (循环！)
```

### 1.2 职责三重冗余

| 功能 | IO.aura | File.aura | FileSystem.aura |
|------|---------|-----------|-----------------|
| 读取文件 | `fileRead(path)` | `readText()` | `readText(path)` |
| 写入文件 | `fileWrite(path, content)` | `writeText(content)` | `writeText(path, content)` |
| 检查存在 | `fileExists(path)` | `exists()` | `exists(path)` |
| 目录操作 | — | — | `mkdir/mkdirP/rename/copy/walk` |

三个模块做同一件事。FileSystem.aura 有 20+ 方法但多数是空实现或简化实现。

### 1.3 InputStream/OutputStream 未暴露为公共 API

`InputStream`/`OutputStream` 接口定义在 `std/io/` 但：
- 仅 `DefaultConsoleIn`/`DefaultConsoleOut` 实现，且功能不完整
- 开发者无法获取文件输入流或输出流
- 无 `FileInputStream`/`FileOutputStream` 等常用实现
- 无 `ByteArrayInputStream`/`ByteArrayOutputStream` 等内存流

### 1.4 Console 职责污染

`Console.aura` 自称"纯逻辑"但 `clear()`/`cursorShow()`/`cursorHide()` 直接调用 `IO.print`。

### 1.5 VM/AOT/Aura 三实现路径

| 路径 | IO.println | Console.print | FileSystem.exists |
|------|-----------|---------------|-------------------|
| **VM (Rust)** | `std_io.rs` → Rust native | `std_console.rs` → Rust native | `std_fs.rs` → Rust native |
| **AOT (LLVM)** | runtime table → C FFI | `emit.rs` 硬编码 → C FFI | C FFI 直接调用 |
| **Aura 源码** | IO → Stdio → Console.writeStdout | Console → stdout → DefaultConsoleOut → IO (循环) | FileSystem → Stdio → FileOps |

### 1.6 C FFI 符号命名不一致

```rust
"IO"      => "io",     // aura_lang_std_IO_println → aura_io_println
"Console" => return name.to_string();  // 不翻译
"FileSystem" => "fs",  // aura_lang_std_FileSystem_* → aura_fs_*
```

---

## 2. 目标架构

### 2.1 四层分层

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  Layer 3: Public API (开发者面向)                                              │
│                                                                              │
│  File.aura     — class File (有状态文件句柄) + object Files (静态工具)          │
│  std/io/       — InputStream/OutputStream 接口 + 默认实现                       │
│    ├─ StreamImpl.aura   — stdin/stdout 流实现                                 │
│    ├─ FileStream.aura   — FileInputStream / FileOutputStream                   │
│    └─ MemoryStream.aura — ByteArrayInputStream / ByteArrayOutputStream        │
│  FileUtils.aura — 基于 File/Files 的工具类（便捷批量操作）                       │
├──────────────────────────────────────────────────────────────────────────────┤
│  Layer 2: Internal Core (核心库内部使用)                                        │
│  IO.aura       — stdin/stdout facade（prelu.aura 的 println/print）            │
│  Console.aura  — 纯 ANSI 转义序列生成（无 I/O）                                 │
│  std/io/Ansi.aura — ANSI 转义序列生成器                                        │
├──────────────────────────────────────────────────────────────────────────────┤
│  Layer 1: Native Bridge                                                       │
│  native/io/Stdio.aura — String↔Buffer 转换 + 高级文件 I/O                      │
├──────────────────────────────────────────────────────────────────────────────┤
│  Layer 0: Native Primitives                                                   │
│  native/console/Console.aura — extern: writeStdout/writeStderr                │
│  native/file/FileOps.aura — extern: open/close/read/write/...                 │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 目标文件结构

```
aura/core/aura/lang/
├── native/
│   ├── console/Console.aura          # [不变]
│   ├── file/FileOps.aura             # [不变]
│   └── io/Stdio.aura                 # [不变]
├── std/
│   ├── io/
│   │   ├── InputStream.aura          # [增强] 公共接口
│   │   ├── OutputStream.aura         # [增强] 公共接口
│   │   ├── StreamImpl.aura           # [新增] stdin/stdout 实现
│   │   ├── FileStream.aura           # [新增] 文件流实现
│   │   ├── MemoryStream.aura         # [新增] 内存流实现
│   │   └── Ansi.aura                 # [新增] ANSI 转义序列
│   ├── IO.aura                       # [重构] 内部 stdin/stdout facade
│   ├── Console.aura                  # [重构] 内部纯 ANSI 逻辑
│   ├── File.aura                     # [丰富] 开发者主 API
│   ├── FileUtils.aura                # [新增] 基于 File 的工具类
│   ├── fs/FileSystem.aura            # [删除] 空壳
│   └── io/StdStreams.aura            # [删除] 被 StreamImpl 替代
├── prelu.aura                        # [小改]
└── ...
```

---

## 3. Aura 标准库重构

### 3.1 `std/File.aura` — 开发者主 API

```aura
// aura:///aura/lang/std/File.aura
package aura.lang.std

import aura.lang.native.Memory
import aura.lang.native.memory.Allocator
import aura.lang.native.file.FileOps
import aura.lang.native.io.Stdio

// ═══════════════════════════════════════════════════════════════
// File — 有状态文件句柄
// ═══════════════════════════════════════════════════════════════

/// 文件资源实例（有状态，绑定到指定路径）。
///
/// ```aura
/// val f: File = File("hello.txt")
/// f.writeText("Hello, World!")
/// val content: String = f.readText()
/// ```
class File {

    private var pathStr: String = ""
    private var pathBuf: Long = 0

    init(path: String) {
        this.pathStr = path
        this.pathBuf = Stdio.stringToBuffer(path)
    }

    // ── 文本 I/O ──
    fun readText(): String {
        val fd: Int = FileOps.open(pathBuf, 0)
        if (fd < 0) { return "" }
        val size: Long = FileOps.lseek(fd, 0, 2)
        FileOps.lseek(fd, 0, 0)
        if (size <= 0) { FileOps.close(fd); return "" }
        val buf: Long = Allocator.malloc(size + 1)
        if (buf == 0) { FileOps.close(fd); return "" }
        val read: Long = FileOps.read(fd, buf, size)
        FileOps.close(fd)
        if (read <= 0) { Allocator.free(buf); return "" }
        val text: String = Stdio.bufferToString(buf, read)
        Allocator.free(buf)
        return text
    }

    fun writeText(content: String): Unit {
        val fd: Int = FileOps.open(pathBuf, 1 | 0x40 | 0x200)
        if (fd < 0) { return }
        val buf: Long = Stdio.stringToBuffer(content)
        if (buf == 0) { FileOps.close(fd); return }
        val len: Long = Stdio.strlen(buf)
        FileOps.write(fd, buf, len)
        Allocator.free(buf)
        FileOps.close(fd)
    }

    // ── 二进制 I/O ──
    fun readBytes(): Array<Byte> {
        val content: String = readText()
        val arr: Array<Byte> = arrayOf<Byte>()
        var i: Int = 0
        while (i < content.length) {
            arr[i] = content.charCodeAt(i)
            i = i + 1
        }
        return arr
    }

    fun writeBytes(bytes: Array<Byte>): Unit {
        var content: String = ""
        var i: Int = 0
        while (i < bytes.size) {
            content = content + String.fromCharCode(bytes[i])
            i = i + 1
        }
        writeText(content)
    }

    // ── 文件信息 ──
    fun exists(): Boolean { return Stdio.fileExists(pathStr) }
    fun isFile(): Boolean { return Stdio.fileExists(pathStr) }
    fun size(): Long {
        val fd: Int = FileOps.open(pathBuf, 0)
        if (fd < 0) { return -1 }
        val n: Long = FileOps.lseek(fd, 0, 2)
        FileOps.close(fd)
        return n
    }

    // ── 文件操作 ──
    fun delete(): Unit { FileOps.unlink(pathBuf) }
    fun renameTo(newPath: String): Unit {
        val newBuf: Long = Stdio.stringToBuffer(newPath)
        if (newBuf != 0) { FileOps.rename(pathBuf, newBuf); Allocator.free(newBuf) }
    }
    fun copyTo(targetPath: String): Unit {
        val target: File = File(targetPath)
        target.writeText(readText())
    }
}

// ═══════════════════════════════════════════════════════════════
// Files — 文件系统静态工具
// ═══════════════════════════════════════════════════════════════

/// 文件系统静态工具对象。
///
/// ```aura
/// val content: String = Files.read("hello.txt")
/// Files.write("out.txt", "Hello!")
/// Files.mkdir("newdir")
/// ```
object Files {

    // ── 文本 I/O ──
    fun read(path: String): String { return Stdio.readFile(path) }
    fun write(path: String, content: String): Unit { Stdio.writeFile(path, content) }

    // ── 文件检查 ──
    fun exists(path: String): Boolean { return Stdio.fileExists(path) }
    fun isFile(path: String): Boolean { return Stdio.fileExists(path) }
    fun isDirectory(path: String): Boolean { return false }

    // ── 文件信息 ──
    fun fileSize(path: String): Long {
        val pathBuf: Long = Stdio.stringToBuffer(path)
        if (pathBuf == 0) { return 0 }
        val fd: Int = FileOps.open(pathBuf, 0)
        Allocator.free(pathBuf)
        if (fd < 0) { return 0 }
        val size: Long = FileOps.lseek(fd, 0, 2)
        FileOps.close(fd)
        return size
    }

    fun lastModified(path: String): Float { return 0.0f }

    // ── 文件操作 ──
    fun delete(path: String): Unit {
        val pathBuf: Long = Stdio.stringToBuffer(path)
        if (pathBuf == 0) { return }
        FileOps.unlink(pathBuf)
        Allocator.free(pathBuf)
    }

    fun rename(oldPath: String, newPath: String): Unit {
        val oldBuf: Long = Stdio.stringToBuffer(oldPath)
        if (oldBuf == 0) { return }
        val newBuf: Long = Stdio.stringToBuffer(newPath)
        if (newBuf == 0) { Allocator.free(oldBuf); return }
        FileOps.rename(oldBuf, newBuf)
        Allocator.free(oldBuf)
        Allocator.free(newBuf)
    }

    fun copy(source: String, target: String): Unit {
        write(target, read(source))
    }

    // ── 目录操作 ──
    fun mkdir(path: String): Unit {
        val pathBuf: Long = Stdio.stringToBuffer(path)
        if (pathBuf == 0) { return }
        FileOps.mkdir(pathBuf, 493)
        Allocator.free(pathBuf)
    }

    fun mkdirP(path: String): Unit {
        val pathBuf: Long = Stdio.stringToBuffer(path)
        if (pathBuf == 0) { return }
        val len: Long = Stdio.strlen(pathBuf)
        if (len <= 0) { Allocator.free(pathBuf); return }
        var i: Long = 1
        while (i < len) {
            val c: Byte = Memory.read(pathBuf + i)
            if (c == '/') {
                Memory.write(pathBuf + i, 0)
                FileOps.mkdir(pathBuf, 493)
                Memory.write(pathBuf + i, '/')
            }
            i = i + 1
        }
        FileOps.mkdir(pathBuf, 493)
        Allocator.free(pathBuf)
    }

    fun listDir(path: String): List<String> { return mutableListOf<String>() }
    fun listFiles(path: String): List<String> { return mutableListOf<String>() }
    fun walk(path: String): List<String> { return mutableListOf<String>() }

    // ── 路径信息 ──
    fun absolutePath(path: String): String { return path }
    val homeDir: String = ""
    val tempDir: String = ""
    val currentDir: String = ""
}
```

### 3.2 `std/FileUtils.aura` — 基于 File 的工具类

```aura
// aura:///aura/lang/std/FileUtils.aura
package aura.lang.std

import aura.lang.std.File
import aura.lang.std.Files
import aura.lang.native.io.Stdio
import aura.lang.native.file.FileOps
import aura.lang.native.memory.Allocator

/// 文件工具类。
///
/// 本类是 `File`/`Files` 的便捷工具层，提供批量操作和高级文件操作。
/// 开发者**首选** `File`/`Files`；本类适用于需要批量处理的场景。
///
/// ```aura
/// FileUtils.batchWrite(["a.txt", "b.txt"], "content")
/// FileUtils.deleteDir("old_dir")
/// FileUtils.copyDir("src/", "dst/")
/// ```
///
/// @since 0.3
object FileUtils {

    // ── 批量操作 ──

    /// 批量写入多个文件（同内容）。
    fun batchWrite(paths: List<String>, content: String): Unit {
        val size: Int = paths.size
        var i: Int = 0
        while (i < size) {
            Files.write(paths[i], content)
            i = i + 1
        }
    }

    /// 批量读取多个文件，返回合并内容（用换行分隔）。
    fun batchRead(paths: List<String>): String {
        val size: Int = paths.size
        var result: String = ""
        var i: Int = 0
        while (i < size) {
            if (i > 0) { result = result + "\n" }
            result = result + Files.read(paths[i])
            i = i + 1
        }
        return result
    }

    // ── 高级文件操作 ──

    /// 删除文件（委托到 Files）。
    fun delete(path: String): Unit { Files.delete(path) }

    /// 重命名（委托到 Files）。
    fun rename(oldPath: String, newPath: String): Unit { Files.rename(oldPath, newPath) }

    /// 复制文件（委托到 Files）。
    fun copy(source: String, target: String): Unit { Files.copy(source, target) }

    // ── 目录操作 ──

    /// 创建目录（委托到 Files）。
    fun mkdir(path: String): Unit { Files.mkdir(path) }

    /// 递归创建目录（委托到 Files）。
    fun mkdirP(path: String): Unit { Files.mkdirP(path) }

    /// 递归删除目录（简化实现：返回空）。
    fun deleteDir(path: String): Unit {
        // TODO: 需要 readdir + unlink/rmdir syscall
    }

    /// 复制目录（简化实现：返回空）。
    fun copyDir(source: String, target: String): Unit {
        // TODO: 需要 readdir + copy
    }

    // ── 文件信息 ──

    /// 获取文件大小（委托到 Files）。
    fun fileSize(path: String): Long { return Files.fileSize(path) }

    /// 检查路径是否存在（委托到 Files）。
    fun exists(path: String): Boolean { return Files.exists(path) }

    /// 检查是否为文件（委托到 Files）。
    fun isFile(path: String): Boolean { return Files.isFile(path) }

    /// 检查是否为目录（委托到 Files）。
    fun isDirectory(path: String): Boolean { return Files.isDirectory(path) }

    // ── 文件 I/O（委托到 Files）──

    fun readText(path: String): String { return Files.read(path) }
    fun writeText(path: String, content: String): Unit { Files.write(path, content) }
    fun readBytes(path: String): Array<Byte> { return File(path).readBytes() }
    fun writeBytes(path: String, bytes: Array<Byte>): Unit { File(path).writeBytes(bytes) }

    // ── 目录列表 ──

    fun listDir(path: String): List<String> { return Files.listDir(path) }
    fun listFiles(path: String): List<String> { return Files.listFiles(path) }
    fun walk(path: String): List<String> { return Files.walk(path) }

    // ── 路径信息 ──

    fun lastModified(path: String): Float { return Files.lastModified(path) }
    fun absolutePath(path: String): String { return Files.absolutePath(path) }
    val homeDir: String { return Files.homeDir }
    val tempDir: String { return Files.tempDir }
    val currentDir: String { return Files.currentDir }
}

// ═══════════════════════════════════════════════════════════════
// FSUtils — 向后兼容别名
// ═══════════════════════════════════════════════════════════════

/// @deprecated 使用 `Files` 替代。
object FSUtils {
    fun fs_exists(path: String): Boolean { return Files.exists(path) }
    fun fs_is_file(path: String): Boolean { return Files.isFile(path) }
    fun fs_read_text(path: String): String { return Files.read(path) }
    fun fs_write_text(path: String, content: String): Boolean {
        Files.write(path, content)
        return true
    }
    fun fs_delete(path: String): Int { Files.delete(path); return 0 }
    fun fs_mkdir_new(path: String): Int { Files.mkdir(path); return 0 }
    fun fs_rename_new(old: String, new: String): Int { Files.rename(old, new); return 0 }
    fun fs_file_size(path: String): Long { return Files.fileSize(path) }
}
```

### 3.3 `std/io/InputStream.aura` — 公共输入流接口（增强）

```aura
// aura:///aura/lang/std/io/InputStream.aura
package aura.lang.std.io

/// 输入流接口（公共 API）。
///
/// 开发者可用于：
/// - 从文件读取：`FileInputStream("file.txt")`
/// - 从标准输入读取：`StdinInputStream()`
/// - 从内存读取：`ByteArrayInputStream("data")`
///
/// ```aura
/// val is: InputStream = FileInputStream("config.txt")
/// val line: String = is.readLine()
/// val all: String = is.readAll()
/// is.close()
/// ```
///
/// @since 0.2
/// @since 0.3 增强为公共 API
interface InputStream {
    /// 从流中读取最多 count 个字节到 buffer，返回实际读取字节数。
    fun read(buffer: Long, count: Int): Int

    /// 读取一行文本（不含换行符）。
    fun readLine(): String

    /// 读取全部内容直到 EOF。
    fun readAll(): String

    /// 关闭流并释放资源。
    fun close(): Unit
}
```

### 3.4 `std/io/OutputStream.aura` — 公共输出流接口（增强）

```aura
// aura:///aura/lang/std/io/OutputStream.aura
package aura.lang.std.io

/// 输出流接口（公共 API）。
///
/// 开发者可用于：
/// - 写入文件：`FileOutputStream("output.txt")`
/// - 写入标准输出：`StdoutOutputStream()`
/// - 写入内存缓冲：`ByteArrayOutputStream()`
///
/// ```aura
/// val os: OutputStream = FileOutputStream("report.txt")
/// os.println("Line 1")
/// os.print("Line 2\n")
/// os.flush()
/// os.close()
/// ```
///
/// @since 0.2
/// @since 0.3 增强为公共 API
interface OutputStream {
    /// 输出字符串（带换行）。
    fun println(msg: String): Unit

    /// 输出字符串（不带换行）。
    fun print(msg: String): Unit

    /// 刷新缓冲区。
    fun flush(): Unit

    /// 关闭流并释放资源。
    fun close(): Unit
}
```

### 3.5 `std/io/StreamImpl.aura` — stdin/stdout 流实现

```aura
// aura:///aura/lang/std/io/StreamImpl.aura
package aura.lang.std.io

import aura.lang.native.console.Console
import aura.lang.native.io.Stdio

/// 标准流实现工厂。
object StreamImpl {

    private fun writeOut(msg: String, addNewline: Boolean): Unit {
        val buf: Long = Stdio.stringToBuffer(msg)
        if (buf == 0) { return }
        val len: Long = Stdio.strlen(buf)
        Console.writeStdout(buf, len)
        if (addNewline) { Stdio.writeNewline() }
    }

    fun defaultStdout(): OutputStream { return StdOutputStream() }
    fun defaultStdin(): InputStream { return StdInputStream() }
}

class StdOutputStream implements OutputStream {
    fun println(msg: String): Unit { StreamImpl.writeOut(msg, true) }
    fun print(msg: String): Unit { StreamImpl.writeOut(msg, false) }
    fun flush(): Unit { /* 无缓冲 */ }
    fun close(): Unit { /* stdout 不可关闭 */ }
}

class StdInputStream implements InputStream {
    fun read(buffer: Long, count: Int): Int { return 0 }
    fun readLine(): String { return Stdio.readLine() }
    fun readAll(): String { return Stdio.readAll() }
    fun close(): Unit { /* stdin 不可关闭 */ }
}
```

### 3.6 `std/io/FileStream.aura` — 文件流实现

```aura
// aura:///aura/lang/std/io/FileStream.aura
package aura.lang.std.io

import aura.lang.native.console.Console
import aura.lang.native.file.FileOps
import aura.lang.native.memory.Allocator
import aura.lang.native.io.Stdio

/// 文件输入流。
///
/// ```aura
/// val is: InputStream = FileInputStream("data.txt")
/// val line: String = is.readLine()
/// is.close()
/// ```
class FileInputStream implements InputStream {
    private var pathBuf: Long = 0
    private var fd: Int = -1

    init(path: String) {
        this.pathBuf = Stdio.stringToBuffer(path)
        this.fd = FileOps.open(pathBuf, 0)  // O_RDONLY
    }

    fun read(buffer: Long, count: Int): Int {
        if (fd < 0) { return -1 }
        return FileOps.read(fd, buffer, count)
    }

    fun readLine(): String {
        if (fd < 0) { return "" }
        val buf: Long = Allocator.malloc(4096)
        if (buf == 0) { return "" }
        val bytesRead: Long = FileOps.read(fd, buf, 4096)
        if (bytesRead <= 0) { Allocator.free(buf); return "" }
        var end: Long = bytesRead
        var i: Long = 0
        while (i < bytesRead) {
            val c: Byte = Memory.read(buf + i)
            if (c == 10) { end = i; break }
            i = i + 1
        }
        return Stdio.bufferToString(buf, end)
    }

    fun readAll(): String {
        if (fd < 0) { return "" }
        val size: Long = FileOps.lseek(fd, 0, 2)
        FileOps.lseek(fd, 0, 0)
        if (size <= 0) { return "" }
        val buf: Long = Allocator.malloc(size + 1)
        if (buf == 0) { return "" }
        val read: Long = FileOps.read(fd, buf, size)
        if (read <= 0) { Allocator.free(buf); return "" }
        return Stdio.bufferToString(buf, read)
    }

    fun close(): Unit {
        if (fd >= 0) { FileOps.close(fd); fd = -1 }
        if (pathBuf != 0) { Allocator.free(pathBuf); pathBuf = 0 }
    }
}

/// 文件输出流。
///
/// ```aura
/// val os: OutputStream = FileOutputStream("output.txt")
/// os.println("Hello")
/// os.close()
/// ```
class FileOutputStream implements OutputStream {
    private var fd: Int = -1

    init(path: String) {
        val pathBuf: Long = Stdio.stringToBuffer(path)
        if (pathBuf == 0) { return }
        this.fd = FileOps.open(pathBuf, 1 | 0x40 | 0x200)
        Allocator.free(pathBuf)
    }

    fun println(msg: String): Unit {
        if (fd < 0) { return }
        val buf: Long = Stdio.stringToBuffer(msg)
        if (buf == 0) { return }
        val len: Long = Stdio.strlen(buf)
        FileOps.write(fd, buf, len)
        FileOps.write(fd, buf + len, 1)
        Allocator.free(buf)
    }

    fun print(msg: String): Unit {
        if (fd < 0) { return }
        val buf: Long = Stdio.stringToBuffer(msg)
        if (buf == 0) { return }
        val len: Long = Stdio.strlen(buf)
        FileOps.write(fd, buf, len)
        Allocator.free(buf)
    }

    fun flush(): Unit { /* 无缓冲 */ }

    fun close(): Unit {
        if (fd >= 0) { FileOps.close(fd); fd = -1 }
    }
}
```

### 3.7 `std/io/MemoryStream.aura` — 内存流实现

```aura
// aura:///aura/lang/std/io/MemoryStream.aura
package aura.lang.std.io

/// 字节数组输入流（从内存读取）。
///
/// ```aura
/// val is: InputStream = ByteArrayInputStream("Hello, World!")
/// val line: String = is.readLine()
/// ```
class ByteArrayInputStream implements InputStream {
    private var data: String = ""
    private var pos: Int = 0

    init(data: String) {
        this.data = data
        this.pos = 0
    }

    fun read(buffer: Long, count: Int): Int {
        val remaining: Int = data.length - pos
        if (remaining <= 0) { return 0 }
        val toRead: Int = if (count < remaining) { count } else { remaining }
        var i: Int = 0
        while (i < toRead) {
            Memory.write(buffer + i, data.charCodeAt(pos + i))
            pos = pos + 1
            i = i + 1
        }
        return toRead
    }

    fun readLine(): String {
        var end: Int = pos
        while (end < data.length) {
            if (data.charCodeAt(end) == 10) { break }
            end = end + 1
        }
        val line: String = data.substring(pos, end - pos)
        pos = end + 1
        return line
    }

    fun readAll(): String {
        val remaining: String = data.substring(pos, data.length - pos)
        pos = data.length
        return remaining
    }

    fun close(): Unit {
        this.data = ""
        this.pos = 0
    }
}

/// 字节数组输出流（写入内存缓冲）。
///
/// ```aura
/// val os: OutputStream = ByteArrayOutputStream()
/// os.println("Line 1")
/// os.println("Line 2")
/// val content: String = os.toString()
/// ```
class ByteArrayOutputStream implements OutputStream {
    private var buffer: String = ""

    fun println(msg: String): Unit {
        buffer = buffer + msg + "\n"
    }

    fun print(msg: String): Unit {
        buffer = buffer + msg
    }

    fun flush(): Unit { /* 无缓冲 */ }

    fun close(): Unit {
        // 不重置 buffer，允许调用方继续读取
    }

    /// 获取已写入的全部内容。
    fun toString(): String {
        return buffer
    }
}
```

### 3.8 `std/io/Ansi.aura` — ANSI 转义序列

从 `Console.aura` 提取所有 ANSI 转义序列生成逻辑，**完全无 I/O**。

```aura
package aura.lang.std.io

object Ansi {
    private val ESC: String = String.fromCharCode(27)

    fun cursorUp(lines: Int): String { ... }
    fun cursorDown(lines: Int): String { ... }
    fun cursorLeft(cols: Int): String { ... }
    fun cursorRight(cols: Int): String { ... }
    fun cursorTo(line: Int, column: Int): String { ... }
    fun cursorShow(): String { return ESC + "[?25h" }
    fun cursorHide(): String { return ESC + "[?25l" }
    fun clearScreen(): String { return ESC + "[2J" + ESC + "[H" }
    fun reset(): String { return ESC + "[0m" }
    fun red(text: String): String { return ESC + "[31m" + text + reset() }
    fun green(text: String): String { ... }
    fun bold(text: String): String { ... }
    fun italic(text: String): String { ... }
    fun underline(text: String): String { ... }
    fun dim(text: String): String { ... }
    fun inverse(text: String): String { ... }
    fun strikethrough(text: String): String { ... }
    fun color256(text: String, index: Int): String { ... }
    fun rgb(text: String, r: Int, g: Int, b: Int): String { ... }
    fun onRed(text: String): String { ... }
    fun onGreen(text: String): String { ... }
    fun onYellow(text: String): String { ... }
    fun onBlue(text: String): String { ... }
    fun black(text: String): String { ... }
    fun yellow(text: String): String { ... }
    fun blue(text: String): String { ... }
    fun magenta(text: String): String { ... }
    fun cyan(text: String): String { ... }
    fun white(text: String): String { ... }
}
```

### 3.9 `std/Console.aura` — 内部纯 ANSI 逻辑

```aura
package aura.lang.std

import aura.lang.std.io.Ansi
import aura.lang.std.io.StreamImpl
import aura.lang.std.io.OutputStream
import aura.lang.std.io.InputStream

object Console {
    const val DEFAULT_WIDTH: Int = 80
    const val DEFAULT_HEIGHT: Int = 24

    val size: List<Int> { ... }
    val width: Int { return 80 }
    val height: Int { return 24 }

    val stdout: OutputStream = StreamImpl.defaultStdout()
    val stderr: OutputStream = StreamImpl.defaultStdout()
    val stdin: InputStream = StreamImpl.defaultStdin()

    fun println(msg: String): Unit { stdout.println(msg) }
    fun print(msg: String): Unit { stdout.print(msg) }
    fun clear(): Unit { stdout.print(Ansi.clearScreen()) }
    fun cursorUp(lines: Int): String { return Ansi.cursorUp(lines) }
    fun cursorDown(lines: Int): String { return Ansi.cursorDown(lines) }
    fun cursorLeft(cols: Int): String { return Ansi.cursorLeft(cols) }
    fun cursorRight(cols: Int): String { return Ansi.cursorRight(cols) }
    fun cursorTo(line: Int, column: Int): String { return Ansi.cursorTo(line, column) }
    fun cursorShow(): String { return Ansi.cursorShow() }
    fun cursorHide(): String { return Ansi.cursorHide() }
    fun reset(): String { return Ansi.reset() }
    fun red(text: String): String { return Ansi.red(text) }
    fun green(text: String): String { return Ansi.green(text) }
    fun yellow(text: String): String { return Ansi.yellow(text) }
    fun blue(text: String): String { return Ansi.blue(text) }
    fun magenta(text: String): String { return Ansi.magenta(text) }
    fun cyan(text: String): String { return Ansi.cyan(text) }
    fun white(text: String): String { return Ansi.white(text) }
    fun black(text: String): String { return Ansi.black(text) }
    fun bold(text: String): String { return Ansi.bold(text) }
    fun italic(text: String): String { return Ansi.italic(text) }
    fun underline(text: String): String { return Ansi.underline(text) }
    fun dim(text: String): String { return Ansi.dim(text) }
    fun inverse(text: String): String { return Ansi.inverse(text) }
    fun strikethrough(text: String): String { return Ansi.strikethrough(text) }
    fun color256(text: String, index: Int): String { return Ansi.color256(text, index) }
    fun rgb(text: String, r: Int, g: Int, b: Int): String { return Ansi.rgb(text, r, g, b) }
    fun onRed(text: String): String { return Ansi.onRed(text) }
    fun onGreen(text: String): String { return Ansi.onGreen(text) }
    fun onYellow(text: String): String { return Ansi.onYellow(text) }
    fun onBlue(text: String): String { return Ansi.onBlue(text) }
}
```

### 3.10 `std/IO.aura` — 内部 stdin/stdout facade

```aura
package aura.lang.std

import aura.lang.std.io.StreamImpl

object IO {
    fun println(msg: String): Unit { StreamImpl.defaultStdout().println(msg) }
    fun print(msg: String): Unit { StreamImpl.defaultStdout().print(msg) }
    fun readLine(): String { return StreamImpl.defaultStdin().readLine() }
    fun readAll(): String { return StreamImpl.defaultStdin().readAll() }
    fun flush(): Unit { StreamImpl.defaultStdout().flush() }
}
```

### 3.11 删除的文件

- `std/fs/FileSystem.aura` — 空壳，删除
- `std/io/StdStreams.aura` — 被 StreamImpl.aura 替代，删除

---

## 4. FileSystem → File/Files 迁移清单

### 4.1 API 映射表

| 旧 FileSystem API | 新 API | 说明 |
|---|---|---|
| `FileSystem.exists(path)` | `Files.exists(path)` | |
| `FileSystem.isFile(path)` | `Files.isFile(path)` | |
| `FileSystem.isDirectory(path)` | `Files.isDirectory(path)` | |
| `FileSystem.readText(path)` | `Files.read(path)` | |
| `FileSystem.writeText(path, content)` | `Files.write(path, content)` | |
| `FileSystem.readBytes(path)` | `FileInputStream(path).readBytes()` | 或 `File(path).readBytes()` |
| `FileSystem.writeBytes(path, bytes)` | `FileOutputStream(path).writeBytes(bytes)` | 或 `File(path).writeBytes(bytes)` |
| `FileSystem.delete(path)` | `Files.delete(path)` | |
| `FileSystem.mkdir(path)` | `Files.mkdir(path)` | |
| `FileSystem.mkdirP(path)` | `Files.mkdirP(path)` | |
| `FileSystem.rename(old, new)` | `Files.rename(old, new)` | |
| `FileSystem.copy(src, tgt)` | `Files.copy(src, tgt)` | |
| `FileSystem.listDir(path)` | `Files.listDir(path)` | |
| `FileSystem.listFiles(path)` | `Files.listFiles(path)` | |
| `FileSystem.fileSize(path)` | `Files.fileSize(path)` | |
| `FileSystem.lastModified(path)` | `Files.lastModified(path)` | |
| `FileSystem.absolutePath(path)` | `Files.absolutePath(path)` | |
| `FileSystem.homeDir` | `Files.homeDir` | |
| `FileSystem.tempDir` | `Files.tempDir` | |
| `FileSystem.currentDir` | `Files.currentDir` | |
| `FileSystem.walk(path)` | `Files.walk(path)` | |
| `FileSystem.readFile(path)` | `Files.read(path)` | 旧别名 |
| `FileSystem.writeFile(path, data)` | `Files.write(path, data)` | 旧别名 |
| `FSUtils.fs_exists(path)` | `Files.exists(path)` | |
| `FSUtils.fs_read_text(path)` | `Files.read(path)` | |
| `FSUtils.fs_write_text(path, content)` | `Files.write(path, content)` | |
| `FSUtils.fs_delete(path)` | `Files.delete(path)` | |
| `FSUtils.fs_mkdir_new(path)` | `Files.mkdir(path)` | |
| `FSUtils.fs_rename_new(old, new)` | `Files.rename(old, new)` | |
| `FSUtils.fs_file_size(path)` | `Files.fileSize(path)` | |

### 4.2 Aura 文件迁移清单

#### `aura/compiler/` (21 个文件)

| 文件 | 引用方式 | 迁移操作 |
|------|----------|----------|
| `Main.aura` | `FileSystem.exists/readText` | → `Files.exists/read` |
| `package/Package.aura` | `FileSystem.writeFile` | → `Files.write` |
| `sourcemap/SourceMap.aura` | `FileSystem.readFile` | → `Files.read` |
| `auz/Auz.aura` | `FileSystem.writeFile/readFile` | → `Files.write/read` |
| `signing/Signing.aura` | `FileSystem.readFile` | → `Files.read` |
| `signature/Signature.aura` | `FileSystem.writeFile/readFile` | → `Files.write/read` |
| `aot/AotUtil.aura` | `"FileSystem"` 字符串匹配 | → 保留（编译器内部标识） |
| `aot/Aot.aura` | `FileSystem.exists/readText/writeText/mkdirP` | → `Files.exists/read/write/mkdirP` |
| `aot/Emit.aura` | `FileSystem.writeText/readText/exists` | → `Files.write/read/exists` |
| `serialize/AucLoader.aura` | `FileSystem.exists/fileSize` | → `Files.exists/fileSize` |
| `serialize/AucSerializer.aura` | `FileSystem` | → `Files` |
| `aot/ModuleLink.aura` | `FileSystem.exists/readText` + 硬编码路径 | → `Files.exists/read` |
| `vm/Vm.aura` | `FileSystem` | → `Files` |
| `aot/Runtime.aura` | `FileSystem.exists/readText` | → `Files.exists/read` |
| `hir/HirSerializerUtils.aura` | `FileSystem()` 构造 | → `File("")` 或 `Files` |
| `hir/hat/HatParser.aura` | `FileSystem.readText` | → `Files.read` |

#### `aura/toolchain/loom/` (19 个文件)

| 文件 | 引用方式 | 迁移操作 |
|------|----------|----------|
| `TaskExecutor.aura` | `import FileSystem` | → `import Files` |
| `LoomMain.aura` | `import FileSystem` | → `import Files` |
| `plugin/ConventionPlugin.aura` | `FileSystem.exists` | → `Files.exists` |
| `pkg/RegistryClient.aura` | `FileSystem.exists/mkdirP` | → `Files.exists/mkdirP` |
| `pkg/PackageReader.aura` | `FileSystem.exists/readText` | → `Files.exists/read` |
| `pkg/PackageInstaller.aura` | `FileSystem.exists/mkdirP/delete` | → `Files.exists/mkdirP/delete` |
| `pkg/PackageBuilder.aura` | `FileSystem.exists/readText/writeText/mkdirP/listFiles` | → `Files.*` |
| `pkg/LocalRegistry.aura` | `FileSystem.exists` | → `Files.exists` |
| `dep/LockFile.aura` | `FileSystem.exists/readText/writeText` | → `Files.exists/read/write` |
| `dep/DepResolver.aura` | `FileSystem.exists/readText` | → `Files.exists/read` |
| `dep/Bom.aura` | `FileSystem.exists/readText` | → `Files.exists/read` |
| `FileIO.aura` | `FileSystem.exists/readText/writeText/mkdirP/listFiles/delete` | → `Files.*` |
| `Package.aura` | `import FileSystem` | → `import Files` |
| `Main.aura` | `FileSystem.exists` | → `Files.exists` |
| `advanced/Workspace.aura` | `FileSystem.exists` | → `Files.exists` |
| `advanced/Watcher.aura` | `FileSystem.exists` | → `Files.exists` |
| `advanced/Utils.aura` | `FileSystem.exists/listFiles` | → `Files.exists/listFiles` |
| `advanced/StdlibBuilder.aura` | `FileSystem.exists` | → `Files.exists` |
| `advanced/SourceSet.aura` | `FileSystem.exists/listFiles` | → `Files.exists/listFiles` |

#### `aura/toolchain/cli/` (3 个文件)

| 文件 | 引用方式 | 迁移操作 |
|------|----------|----------|
| `Repl.aura` | `import FileSystem` | → `import Files` |
| `CompilerApi.aura` | `import FileSystem` | → `import Files` |
| `FileIO.aura` | `FileSystem.readText/writeText` | → `Files.read/write` |

#### `aura/toolchain/docgen/` (1 个文件)

| 文件 | 引用方式 | 迁移操作 |
|------|----------|----------|
| `Docgen.aura` | `FileSystem.exists/mkdir` + 文档字符串引用 | → `Files.exists/mkdir` |

#### `aura/photon/` (9 个文件)

| 文件 | 引用方式 | 迁移操作 |
|------|----------|----------|
| `InstructionSelection.aura` | `FileSystem.writeText/mkdir` | → `Files.write/mkdir` |
| `PhotonLldConfig.aura` | `FileSystem.exists/readText` | → `Files.exists/read` |
| `PhotonHatCompile.aura` | `FileSystem.exists/writeText/mkdirP` | → `Files.exists/write/mkdirP` |
| `X86Emitter.aura` | `FileSystem.writeText` | → `Files.write` |
| `PhotonBootstrap.aura` | `FileSystem.exists/size` | → `Files.exists/fileSize` |
| `PhirSerializer.aura` | `import FileSystem` | → `import Files` |
| `PhotonPipeline.aura` | `FileSystem.writeText/readText/mkdirP` | → `Files.write/read/mkdirP` |
| `PhotonObjectWriter.aura` | `FileSystem.writeText/readText/exists/writeBytes` | → `Files.write/read/exists` + `FileOutputStream` |
| `RegisterAllocator.aura` | `FileSystem.writeText` | → `Files.write` |

#### `tests/` (9 个文件)

| 文件 | 引用方式 | 迁移操作 |
|------|----------|----------|
| `tests/language-test/13-stdlib.aura` | `FileSystem.writeText/readText/exists` | → `Files.write/read/exists` |
| `tests/phase6_5_aot_tests.aura` | `FileSystem.exists/mkdirP/writeText` | → `Files.exists/mkdirP/write` |
| `tests/phase9_compiler_tests.aura` | `import FileSystem` | → `import Files` |
| `tests/phase_e_loom_tests.aura` | `import FileSystem` | → `import Files` |
| `tests/phase_e_verify_tests.aura` | `FileSystem.exists` | → `Files.exists` |
| `tests/bootstrap_c4_tests.aura` | `FileSystem.readText` | → `Files.read` |
| `tests/native_c2_aot_tests.aura` | `import FileSystem` | → `import Files` |
| `tests/photon/test_phir_to_exe.aura` | `FileSystem.readText` | → `Files.read` |
| `tests/photon/S3/*.aura` (4 files) | `FileSystem.writeText/exists` | → `Files.write/exists` |

### 4.3 Rust 编译器迁移清单

#### `seed/compiler/src/` (10 个文件)

| 文件 | 行号 | 内容 | 迁移操作 |
|------|------|------|----------|
| `std/std_fs.rs` | 全文件 | 注册 `FileSystem.*` native | 新增 `Files.*` 注册，保留 `FileSystem.*` 向后兼容 |
| `std/decl.rs` | 652-677 | `FileSystem.*` 声明列表 | 新增 `Files.*` 声明，保留 `FileSystem.*` |
| `std/decl.rs` | 1121 | 断言 `FileSystem.` | 新增 `Files.` 断言 |
| `codegen/hir.rs` | 6247 | `"fs" => "FileSystem"` | 新增 `"fs" => "Files"` 映射 |
| `codegen/hir.rs` | 6883-6949 | `FileSystem.*` 类型签名 | 新增 `Files.*` 签名，保留 `FileSystem.*` |
| `codegen/mod.rs` | 689 | `"FileSystem" => "fs"` | 新增 `"Files" => "fs"` |
| `codegen/aot/runtime.rs` | 743 | `"FileSystem" => "fs"` | 新增 `"Files" => "fs"` |
| `std/mod.rs` | 224 | `"FileSystem" => "fs"` | 新增 `"Files" => "fs"` |
| `std/source_index.rs` | 406, 642 | `"fs" => "FileSystem.aura"` | 新增 `"fs" => "File.aura"` |
| `docgen.rs` | 460-523 | 文档生成 FileSystem 条目 | 新增 Files 条目 |
| `sema/checker.rs` | 27 | `"FileSystem"` 关键字列表 | 新增 `"Files"`, `"File"` |
| `vm/interp.rs` | 1663 | 注释引用 | 更新注释 |

#### `seed/compiler/src/std/cffi/` (1 个文件)

| 文件 | 行号 | 内容 | 迁移操作 |
|------|------|------|----------|
| `aura_std_cffi.c` | 2716-2862 | `FileSystem.*` C FFI 函数 | 新增 `Files.*` C FFI 函数，保留 `FileSystem.*` |
| `aura_std_cffi.c` | 3587-3692 | `AURA_FN` 注册 | 新增 `Files.*` 注册 |

### 4.4 迁移原则

1. **Rust native 注册**：同时注册 `FileSystem.*` 和 `Files.*`，旧代码不破坏
2. **C FFI 符号**：同时保留 `aura_lang_std_FileSystem_*` 和新增 `aura_lang_std_Files_*`
3. **HIR 类型签名**：同时注册两套签名
4. **语义检查器**：同时识别 `FileSystem`、`Files`、`File` 三个关键字
5. **符号翻译**：`translate_to_legacy_c` 同时处理 `"FileSystem" => "fs"` 和 `"Files" => "fs"`
6. **文档生成**：同时生成 `FileSystem` 和 `Files` 的文档条目

---

## 5. Rust 编译器重构

### 5.1 `seed/compiler/src/std/std_fs.rs` → `std_file.rs` (重构)

重命名文件，新增 `Files.*` 和 `File.*` 注册，保留 `FileSystem.*` 向后兼容。

```rust
//! std.file — 文件/文件系统操作

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    // ── Files.* 静态工具（新 API）──
    reg.register("aura.lang.std.Files.read", nat_files_read);
    reg.register("aura.lang.std.Files.write", nat_files_write);
    reg.register("aura.lang.std.Files.exists", nat_files_exists);
    reg.register("aura.lang.std.Files.isFile", nat_files_is_file);
    reg.register("aura.lang.std.Files.isDirectory", nat_files_is_directory);
    reg.register("aura.lang.std.Files.delete", nat_files_delete);
    reg.register("aura.lang.std.Files.mkdir", nat_files_mkdir);
    reg.register("aura.lang.std.Files.mkdirP", nat_files_mkdir_p);
    reg.register("aura.lang.std.Files.rename", nat_files_rename);
    reg.register("aura.lang.std.Files.copy", nat_files_copy);
    reg.register("aura.lang.std.Files.listDir", nat_files_list_dir);
    reg.register("aura.lang.std.Files.listFiles", nat_files_list_files);
    reg.register("aura.lang.std.Files.fileSize", nat_files_file_size);
    reg.register("aura.lang.std.Files.lastModified", nat_files_last_modified);
    reg.register("aura.lang.std.Files.absolutePath", nat_files_absolute_path);
    reg.register("aura.lang.std.Files.walk", nat_files_walk);

    // ── File.* 实例方法 ──
    reg.register("aura.lang.std.File.readText", nat_file_read_text);
    reg.register("aura.lang.std.File.writeText", nat_file_write_text);
    reg.register("aura.lang.std.File.exists", nat_file_exists);
    reg.register("aura.lang.std.File.isFile", nat_file_is_file);
    reg.register("aura.lang.std.File.size", nat_file_size);
    reg.register("aura.lang.std.File.delete", nat_file_delete);
    reg.register("aura.lang.std.File.renameTo", nat_file_rename_to);
    reg.register("aura.lang.std.File.copyTo", nat_file_copy_to);
    reg.register("aura.lang.std.File.readBytes", nat_file_read_bytes);
    reg.register("aura.lang.std.File.writeBytes", nat_file_write_bytes);

    // ── FileSystem.* 向后兼容 ──
    reg.register("aura.lang.std.FileSystem.exists", nat_files_exists);
    reg.register("aura.lang.std.FileSystem.isFile", nat_files_is_file);
    reg.register("aura.lang.std.FileSystem.isDirectory", nat_files_is_directory);
    reg.register("aura.lang.std.FileSystem.readText", nat_files_read);
    reg.register("aura.lang.std.FileSystem.writeText", nat_files_write);
    reg.register("aura.lang.std.FileSystem.readBytes", nat_files_read_bytes);
    reg.register("aura.lang.std.FileSystem.writeBytes", nat_files_write_bytes);
    reg.register("aura.lang.std.FileSystem.delete", nat_files_delete);
    reg.register("aura.lang.std.FileSystem.mkdir", nat_files_mkdir);
    reg.register("aura.lang.std.FileSystem.mkdirP", nat_files_mkdir_p);
    reg.register("aura.lang.std.FileSystem.rename", nat_files_rename);
    reg.register("aura.lang.std.FileSystem.copy", nat_files_copy);
    reg.register("aura.lang.std.FileSystem.listDir", nat_files_list_dir);
    reg.register("aura.lang.std.FileSystem.listFiles", nat_files_list_files);
    reg.register("aura.lang.std.FileSystem.fileSize", nat_files_file_size);
    reg.register("aura.lang.std.FileSystem.lastModified", nat_files_last_modified);
    reg.register("aura.lang.std.FileSystem.absolutePath", nat_files_absolute_path);
    reg.register("aura.lang.std.FileSystem.homeDir", nat_files_home_dir);
    reg.register("aura.lang.std.FileSystem.tempDir", nat_files_temp_dir);
    reg.register("aura.lang.std.FileSystem.currentDir", nat_files_current_dir);
    reg.register("aura.lang.std.FileSystem.walk", nat_files_walk);

    // ── FSUtils.* 向后兼容 ──
    reg.register("aura.lang.std.FSUtils.fs_exists", nat_fs_utils_exists);
    reg.register("aura.lang.std.FSUtils.fs_is_file", nat_fs_utils_is_file);
    reg.register("aura.lang.std.FSUtils.fs_read_text", nat_fs_utils_read_text);
    reg.register("aura.lang.std.FSUtils.fs_write_text", nat_fs_utils_write_text);
    reg.register("aura.lang.std.FSUtils.fs_delete", nat_fs_utils_delete);
    reg.register("aura.lang.std.FSUtils.fs_mkdir_new", nat_fs_utils_mkdir);
    reg.register("aura.lang.std.FSUtils.fs_rename_new", nat_fs_utils_rename);
    reg.register("aura.lang.std.FSUtils.fs_file_size", nat_fs_utils_file_size);

    // ── 文件流（新 API）──
    reg.register("aura.lang.std.io.FileInputStream.init", nat_fileinputstream_init);
    reg.register("aura.lang.std.io.FileInputStream.read", nat_fileinputstream_read);
    reg.register("aura.lang.std.io.FileInputStream.readLine", nat_fileinputstream_readline);
    reg.register("aura.lang.std.io.FileInputStream.readAll", nat_fileinputstream_readall);
    reg.register("aura.lang.std.io.FileInputStream.close", nat_fileinputstream_close);
    reg.register("aura.lang.std.io.FileOutputStream.init", nat_fileoutputstream_init);
    reg.register("aura.lang.std.io.FileOutputStream.println", nat_fileoutputstream_println);
    reg.register("aura.lang.std.io.FileOutputStream.print", nat_fileoutputstream_print);
    reg.register("aura.lang.std.io.FileOutputStream.flush", nat_fileoutputstream_flush);
    reg.register("aura.lang.std.io.FileOutputStream.close", nat_fileoutputstream_close);
}

fn arg0(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}
fn arg1(args: &[Value]) -> String {
    args.get(1).map(|v| v.as_string()).unwrap_or_default()
}

// ═══════════════════════════════════════════════════════════════
// Files.* 实现
// ═══════════════════════════════════════════════════════════════

fn nat_files_read(args: &[Value]) -> Value {
    match std::fs::read_to_string(&arg0(args)) {
        Ok(s) => Value::str_(s),
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_files_write(args: &[Value]) -> Value {
    let _ = std::fs::write(&arg0(args), &arg1(args));
    Value::Null
}

fn nat_files_exists(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).exists())
}

fn nat_files_is_file(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).is_file())
}

fn nat_files_is_directory(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).is_dir())
}

fn nat_files_delete(args: &[Value]) -> Value {
    let _ = std::fs::remove_file(&arg0(args));
    Value::Null
}

fn nat_files_mkdir(args: &[Value]) -> Value {
    let _ = std::fs::create_dir(&arg0(args));
    Value::Null
}

fn nat_files_mkdir_p(args: &[Value]) -> Value {
    let _ = std::fs::create_dir_all(&arg0(args));
    Value::Null
}

fn nat_files_rename(args: &[Value]) -> Value {
    let _ = std::fs::rename(&arg0(args), &arg1(args));
    Value::Null
}

fn nat_files_copy(args: &[Value]) -> Value {
    let _ = std::fs::copy(&arg0(args), &arg1(args));
    Value::Null
}

fn nat_files_list_dir(_args: &[Value]) -> Value { Value::str_("") }
fn nat_files_list_files(_args: &[Value]) -> Value { Value::str_("") }

fn nat_files_file_size(args: &[Value]) -> Value {
    match std::fs::metadata(&arg0(args)) {
        Ok(m) => Value::Int(m.len() as i64),
        Err(_) => Value::Int(0),
    }
}

fn nat_files_last_modified(_args: &[Value]) -> Value { Value::Float(0.0) }
fn nat_files_absolute_path(args: &[Value]) -> Value { Value::str_(&arg0(args)) }
fn nat_files_walk(_args: &[Value]) -> Value { Value::str_("") }
fn nat_files_home_dir(_args: &[Value]) -> Value { Value::str_("") }
fn nat_files_temp_dir(_args: &[Value]) -> Value { Value::str_("") }
fn nat_files_current_dir(_args: &[Value]) -> Value { Value::str_("") }

fn nat_files_read_bytes(_args: &[Value]) -> Value { Value::str_("") }
fn nat_files_write_bytes(_args: &[Value]) -> Value { Value::Null }

// ═══════════════════════════════════════════════════════════════
// File.* 实现
// ═══════════════════════════════════════════════════════════════

fn nat_file_read_text(args: &[Value]) -> Value {
    match std::fs::read_to_string(&arg0(args)) {
        Ok(s) => Value::str_(s),
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_file_write_text(args: &[Value]) -> Value {
    let _ = std::fs::write(&arg0(args), &arg1(args));
    Value::Null
}

fn nat_file_exists(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).exists())
}

fn nat_file_is_file(args: &[Value]) -> Value {
    Value::Bool(std::path::Path::new(&arg0(args)).is_file())
}

fn nat_file_size(args: &[Value]) -> Value {
    match std::fs::metadata(&arg0(args)) {
        Ok(m) => Value::Int(m.len() as i64),
        Err(_) => Value::Int(-1),
    }
}

fn nat_file_delete(args: &[Value]) -> Value {
    let _ = std::fs::remove_file(&arg0(args));
    Value::Null
}

fn nat_file_rename_to(args: &[Value]) -> Value {
    let _ = std::fs::rename(&arg0(args), &arg1(args));
    Value::Null
}

fn nat_file_copy_to(args: &[Value]) -> Value {
    let _ = std::fs::copy(&arg0(args), &arg1(args));
    Value::Null
}

fn nat_file_read_bytes(_args: &[Value]) -> Value { Value::str_("") }
fn nat_file_write_bytes(_args: &[Value]) -> Value { Value::Null }

// ═══════════════════════════════════════════════════════════════
// FSUtils.* 向后兼容
// ═══════════════════════════════════════════════════════════════

fn nat_fs_utils_exists(args: &[Value]) -> Value { nat_files_exists(args) }
fn nat_fs_utils_is_file(args: &[Value]) -> Value { nat_files_is_file(args) }
fn nat_fs_utils_read_text(args: &[Value]) -> Value { nat_files_read(args) }
fn nat_fs_utils_write_text(args: &[Value]) -> Value { nat_files_write(args) }
fn nat_fs_utils_delete(args: &[Value]) -> Value { nat_files_delete(args) }
fn nat_fs_utils_mkdir(args: &[Value]) -> Value { nat_files_mkdir(args) }
fn nat_fs_utils_rename(args: &[Value]) -> Value { nat_files_rename(args) }
fn nat_fs_utils_file_size(args: &[Value]) -> Value { nat_files_file_size(args) }

// ═══════════════════════════════════════════════════════════════
// 文件流实现
// ═══════════════════════════════════════════════════════════════

fn nat_fileinputstream_init(args: &[Value]) -> Value {
    Value::Int(0)
}
fn nat_fileinputstream_read(_args: &[Value]) -> Value { Value::Int(0) }
fn nat_fileinputstream_readline(_args: &[Value]) -> Value { Value::str_("") }
fn nat_fileinputstream_readall(_args: &[Value]) -> Value { Value::str_("") }
fn nat_fileinputstream_close(_args: &[Value]) -> Value { Value::Null }

fn nat_fileoutputstream_init(args: &[Value]) -> Value { Value::Int(0) }
fn nat_fileoutputstream_println(_args: &[Value]) -> Value { Value::Null }
fn nat_fileoutputstream_print(_args: &[Value]) -> Value { Value::Null }
fn nat_fileoutputstream_flush(_args: &[Value]) -> Value { Value::Null }
fn nat_fileoutputstream_close(_args: &[Value]) -> Value { Value::Null }
```

### 5.2 `std_io.rs` (重构)

注册底层 native，移除 IO.fileRead 等。

```rust
pub fn register(reg: &mut NativeRegistry) {
    // ── 底层 Console 输出 ──
    reg.register("aura.lang.native.console.Console.writeStdout", nat_write_stdout);
    reg.register("aura.lang.native.console.Console.writeStderr", nat_write_stderr);

    // ── 高级文件 I/O（native.io.Stdio）──
    reg.register("aura.lang.native.io.Stdio.readLine", nat_read_line);
    reg.register("aura.lang.native.io.Stdio.readAll", nat_read_all);
    reg.register("aura.lang.native.io.Stdio.readFile", nat_file_read);
    reg.register("aura.lang.native.io.Stdio.writeFile", nat_file_write);
    reg.register("aura.lang.native.io.Stdio.fileExists", nat_file_exists);
    reg.register("aura.lang.native.io.Stdio.stringToBuffer", nat_string_to_buffer);
    reg.register("aura.lang.native.io.Stdio.bufferToString", nat_buffer_to_string);
    reg.register("aura.lang.native.io.Stdio.strlen", nat_strlen);
    reg.register("aura.lang.native.io.Stdio.writeNewline", nat_write_newline);

    // ── 保留旧 IO.* 注册（向后兼容）──
    reg.register("aura.lang.std.IO.println", nat_legacy_println);
    reg.register("aura.lang.std.IO.print", nat_legacy_print);
    reg.register("aura.lang.std.IO.readLine", nat_legacy_readline);
    reg.register("aura.lang.std.IO.readAll", nat_legacy_readall);
    reg.register("aura.lang.std.IO.flush", nat_legacy_flush);
}

fn nat_write_stdout(args: &[Value]) -> Value {
    if let (Some(&Value::Int(buf)), Some(&Value::Int(count))) = (args.first(), args.get(1)) {
        unsafe {
            let slice = std::slice::from_raw_parts(buf as *const u8, count as usize);
            let _ = std::io::stdout().lock().write_all(slice);
        }
    }
    Value::Null
}

fn nat_write_stderr(args: &[Value]) -> Value {
    if let (Some(&Value::Int(buf)), Some(&Value::Int(count))) = (args.first(), args.get(1)) {
        unsafe {
            let slice = std::slice::from_raw_parts(buf as *const u8, count as usize);
            let _ = std::io::stderr().lock().write_all(slice);
        }
    }
    Value::Null
}

fn nat_read_line(_args: &[Value]) -> Value {
    let stdin = std::io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(n) if n > 0 => Value::str_(line.trim_end()),
        _ => Value::Null,
    }
}

fn nat_read_all(_args: &[Value]) -> Value {
    let stdin = std::io::stdin();
    let mut s = String::new();
    match stdin.lock().read_to_string(&mut s) {
        Ok(_) => Value::str_(s.trim_end()),
        Err(_) => Value::Null,
    }
}

fn nat_file_read(args: &[Value]) -> Value {
    let path = args.first().map(|v| v.as_string()).unwrap_or_default();
    match std::fs::read_to_string(&path) {
        Ok(s) => Value::str_(s),
        Err(e) => Value::str_(format!("IO error: {}", e)),
    }
}

fn nat_file_write(args: &[Value]) -> Value {
    if args.len() < 2 { return Value::Bool(false); }
    let path = args[0].as_string();
    let content = args[1].as_string();
    Value::Bool(std::fs::write(&path, content).is_ok())
}

fn nat_file_exists(args: &[Value]) -> Value {
    let path = args.first().map(|v| v.as_string()).unwrap_or_default();
    Value::Bool(std::path::Path::new(&path).exists())
}

fn nat_string_to_buffer(args: &[Value]) -> Value {
    let s = args.first().map(|v| v.as_string()).unwrap_or_default();
    let cstr = std::ffi::CString::new(s.as_bytes()).unwrap();
    Value::Int(cstr.into_raw() as i64)
}

fn nat_buffer_to_string(args: &[Value]) -> Value {
    if let Some(&Value::Int(buf)) = args.first() {
        if buf != 0 {
            unsafe {
                let c_str = std::ffi::CStr::from_ptr(buf as *const std::ffi::c_char);
                if let Ok(s) = c_str.to_str() { return Value::str_(s); }
            }
        }
    }
    Value::str_("")
}

fn nat_strlen(_args: &[Value]) -> Value { Value::Int(0) }

fn nat_write_newline(_args: &[Value]) -> Value {
    let _ = std::io::stdout().write_all(b"\n");
    Value::Null
}

fn nat_legacy_println(args: &[Value]) -> Value {
    let s = join_args(args);
    println!("{}", s);
    Value::Null
}

fn nat_legacy_print(args: &[Value]) -> Value {
    let s = join_args(args);
    print!("{}", s);
    let _ = std::io::stdout().flush();
    Value::Null
}

fn nat_legacy_readline(_args: &[Value]) -> Value { nat_read_line(_args) }
fn nat_legacy_readall(_args: &[Value]) -> Value { nat_read_all(_args) }

fn nat_legacy_flush(_args: &[Value]) -> Value {
    let _ = std::io::stdout().flush();
    Value::Null
}

fn join_args(args: &[Value]) -> String {
    let mut s = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 { s.push(' '); }
        s.push_str(&a.to_string());
    }
    s
}
```

### 5.3 `std_console.rs` (重构)

ANSI 函数改返回 String（纯逻辑），不再直接输出。

```rust
pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.Console.clear", nat_clear);
    reg.register("aura.lang.std.Console.cursorUp", nat_cursor_up);
    reg.register("aura.lang.std.Console.cursorDown", nat_cursor_down);
    reg.register("aura.lang.std.Console.cursorLeft", nat_cursor_left);
    reg.register("aura.lang.std.Console.cursorRight", nat_cursor_right);
    reg.register("aura.lang.std.Console.cursorShow", nat_cursor_show);
    reg.register("aura.lang.std.Console.cursorHide", nat_cursor_hide);
    reg.register("aura.lang.std.Console.reset", nat_reset);
    reg.register("aura.lang.std.Console.red", nat_red);
    reg.register("aura.lang.std.Console.green", nat_green);
    reg.register("aura.lang.std.Console.yellow", nat_yellow);
    reg.register("aura.lang.std.Console.blue", nat_blue);
    reg.register("aura.lang.std.Console.magenta", nat_magenta);
    reg.register("aura.lang.std.Console.cyan", nat_cyan);
    reg.register("aura.lang.std.Console.white", nat_white);
    reg.register("aura.lang.std.Console.bold", nat_bold);
    reg.register("aura.lang.std.Console.italic", nat_italic);
    reg.register("aura.lang.std.Console.underline", nat_underline);
    reg.register("aura.lang.std.Console.dim", nat_dim);
    reg.register("aura.lang.std.Console.inverse", nat_inverse);
    reg.register("aura.lang.std.Console.size", nat_size);
    reg.register("aura.lang.std.Console.width", nat_width);
    reg.register("aura.lang.std.Console.height", nat_height);
}

fn arg0_str(args: &[Value]) -> String {
    args.first().map(|v| v.as_string()).unwrap_or_default()
}

fn nat_clear(_args: &[Value]) -> Value {
    print!("\x1b[2J\x1b[H");
    Value::Null
}

fn nat_cursor_up(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1);
    if n <= 0 { return Value::str_(""); }
    Value::str_(format!("\x1b[{}A", n))
}

fn nat_cursor_down(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1);
    if n <= 0 { return Value::str_(""); }
    Value::str_(format!("\x1b[{}B", n))
}

fn nat_cursor_left(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1);
    if n <= 0 { return Value::str_(""); }
    Value::str_(format!("\x1b[{}D", n))
}

fn nat_cursor_right(args: &[Value]) -> Value {
    let n = args.first().map(|v| v.as_int()).unwrap_or(1);
    if n <= 0 { return Value::str_(""); }
    Value::str_(format!("\x1b[{}C", n))
}

fn nat_cursor_show(_args: &[Value]) -> Value { Value::str_("\x1b[?25h") }
fn nat_cursor_hide(_args: &[Value]) -> Value { Value::str_("\x1b[?25l") }
fn nat_reset(_args: &[Value]) -> Value { Value::str_("\x1b[0m") }

fn nat_red(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[31m{}\x1b[0m", text))
}

fn nat_green(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[32m{}\x1b[0m", text))
}

fn nat_yellow(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[33m{}\x1b[0m", text))
}

fn nat_blue(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[34m{}\x1b[0m", text))
}

fn nat_magenta(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[35m{}\x1b[0m", text))
}

fn nat_cyan(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[36m{}\x1b[0m", text))
}

fn nat_white(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[37m{}\x1b[0m", text))
}

fn nat_bold(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[1m{}\x1b[22m", text))
}

fn nat_italic(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[3m{}\x1b[23m", text))
}

fn nat_underline(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[4m{}\x1b[24m", text))
}

fn nat_dim(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[2m{}\x1b[22m", text))
}

fn nat_inverse(args: &[Value]) -> Value {
    let text = arg0_str(args);
    Value::str_(format!("\x1b[7m{}\x1b[27m", text))
}

fn nat_size(_args: &[Value]) -> Value {
    let (w, h) = terminal_size();
    let mut map = std::collections::HashMap::new();
    map.insert(Value::str_("width"), Value::Int(w as i64));
    map.insert(Value::str_("height"), Value::Int(h as i64));
    Value::Map(map)
}

fn nat_width(_args: &[Value]) -> Value {
    let (w, _) = terminal_size();
    Value::Int(w as i64)
}

fn nat_height(_args: &[Value]) -> Value {
    let (_, h) = terminal_size();
    Value::Int(h as i64)
}

fn terminal_size() -> (usize, usize) {
    #[cfg(windows)]
    { (120, 30) }
    #[cfg(unix)]
    {
        unsafe {
            use std::io::{self, Read};
            let mut termios = std::mem::zeroed();
            if libc::ioctl(io::stdin().as_raw_fd(), 0x5413, &mut termios) == 0 {
                (termios.ws_col as usize, termios.ws_row as usize)
            } else { (120, 30) }
        }
    }
}
```

### 5.4 `runtime.rs` (修改)

```rust
// translate_to_legacy_c 函数中：
// 新增 Files 映射（与 FileSystem 一致）
"FileSystem" => "fs",
"Files" => "fs",    // 新增

// 新增 Console 统一翻译
"Console" => "console",  // 从 "return name.to_string()" 改为翻译

// RUNTIME_FUNCTIONS 中：
// 删除旧 Console 条目:
//   "aura_lang_std_Console_print" / "aura_lang_std_Console_println"

// 新增:
//   RuntimeFn {
//       name: "aura_console_writeStdout",
//       ret: "i64",
//       params: &[("buf", "i8*"), ("count", "i64")],
//   },
//   RuntimeFn {
//       name: "aura_console_writeStderr",
//       ret: "i64",
//       params: &[("buf", "i8*"), ("count", "i64")],
//   },
```

### 5.5 `emit.rs` (修改)

移除硬编码的 `Console.println`/`Console.print` → `aura_lang_std_Console_println`/`print` 特例。

```rust
// 删除以下代码（约第 920-925 行）:
// else if sym_lower.contains("console") && sym_lower.contains("println") {
//     "call void @aura_lang_std_Console_println(i64 %arg.0)".to_string()
// } else if sym_lower.contains("console") && sym_lower.contains("print") {
//     "call void @aura_lang_std_Console_print(i64 %arg.0)".to_string()
// }
```

### 5.6 `decl.rs` (修改)

新增 `Files.*`、`File.*`、`native.console.Console.*`、`native.io.Stdio.*` 声明，保留 `FileSystem.*`。

### 5.7 `hir.rs` (修改)

新增 `Files.*`、`File.*` 类型签名，保留 `FileSystem.*`。

### 5.8 `aura_std_cffi.c` (修改)

```c
// ── Console I/O — 统一命名 ──

int64_t aura_console_writeStdout(const char *buf, int64_t count) {
    if (!buf || count <= 0) return 0;
    return (int64_t)fwrite(buf, 1, (size_t)count, stdout);
}

int64_t aura_console_writeStderr(const char *buf, int64_t count) {
    if (!buf || count <= 0) return 0;
    return (int64_t)fwrite(buf, 1, (size_t)count, stderr);
}

// 保留旧别名（向后兼容 AOT 产物）:
void aura_lang_std_Console_print(int64_t msg) {
    if (msg) {
        const char *s = (const char *)msg;
        size_t len = strlen(s);
        if (len > 0) fwrite(s, 1, len, stdout);
    }
}
void aura_lang_std_Console_println(int64_t msg) {
    aura_lang_std_Console_print(msg);
    fputc('\n', stdout);
    fflush(stdout);
}

// IO 函数别名（保持不变）:
void aura_lang_std_IO_println(const char *s) { aura_println(s); }
void aura_lang_std_IO_print(const char *s) { aura_print(s); }
const char *aura_lang_std_IO_readLine(void) { return aura_io_readLine(); }
const char *aura_lang_std_IO_readAll(void) { return aura_io_readAll(); }
int aura_lang_std_IO_fileExists(const char *path) { return aura_io_fileExists(path); }
```

---

## 6. 迁移步骤

### Phase 1: 新增文件（无破坏性）

1. 创建 `std/io/Ansi.aura`
2. 创建 `std/io/StreamImpl.aura`
3. 创建 `std/io/FileStream.aura` — FileInputStream/FileOutputStream
4. 创建 `std/io/MemoryStream.aura` — ByteArrayInputStream/ByteArrayOutputStream

### Phase 2: 重构 Aura 标准库

5. 丰富 `std/File.aura` — 新增 Files object + readBytes/writeBytes/isFile/renameTo/copyTo
6. 重命名 `std/FileSystem.aura` → `std/FileUtils.aura` — 改为基于 File/Files 的工具类
7. 增强 `std/io/InputStream.aura` — 公共 API 文档
8. 增强 `std/io/OutputStream.aura` — 公共 API 文档
9. 重构 `std/IO.aura` — 移除文件操作
10. 重构 `std/Console.aura` — 委托到 Ansi
11. 删除 `std/fs/FileSystem.aura`
12. 删除 `std/io/StdStreams.aura`

### Phase 3: Rust 编译器适配

13. 重命名 `std_fs.rs` → `std_file.rs`，新增 Files/File/FileSystem 注册
14. 重构 `std_io.rs`
15. 重构 `std_console.rs`
16. 修改 `runtime.rs`
17. 修改 `emit.rs`
18. 修改 `decl.rs`
19. 修改 `hir.rs`
20. 修改 `mod.rs` (codegen + std)
21. 修改 `source_index.rs`
22. 修改 `checker.rs`
23. 修改 `docgen.rs`
24. 修改 `interp.rs` (注释)
25. 修改 `aura_std_cffi.c`

### Phase 4: 工具链迁移（Aura 文件）

26. `aura/compiler/` — 21 个文件 FileSystem → Files
27. `aura/toolchain/loom/` — 19 个文件 FileSystem → Files
28. `aura/toolchain/cli/` — 3 个文件 FileSystem → Files
29. `aura/toolchain/docgen/` — 1 个文件 FileSystem → Files
30. `aura/photon/` — 9 个文件 FileSystem → Files

### Phase 5: 测试迁移

31. `tests/` — 9 个文件 FileSystem → Files

### Phase 6: 清理（向后兼容期结束后）

32. 移除 `FileSystem.*` native 注册
33. 移除 C FFI 中 `aura_lang_std_FileSystem_*` 旧符号
34. 移除 `FSUtils` deprecated 对象

---

## 7. 兼容性保证

### 7.1 向后兼容

| 旧 API | 新 API | 兼容方式 |
|--------|--------|----------|
| `FileSystem.exists(path)` | `Files.exists(path)` | VM 保留注册 |
| `FileSystem.readText(path)` | `Files.read(path)` | VM 保留注册 |
| `FileSystem.writeText(path, c)` | `Files.write(path, c)` | VM 保留注册 |
| `FSUtils.fs_exists(path)` | `Files.exists(path)` | FSUtils @deprecated |
| `IO.println(msg)` | 不变 | IO.aura facade 保留 |
| `IO.print(msg)` | 不变 | 同上 |
| `Console.red(text)` | `Ansi.red(text)` | Console.aura 委托到 Ansi |
| `Console.clear()` | 不变 | 通过 stdout.print 输出 |
| `Console.cursorShow()` | `Ansi.cursorShow()` | Console.aura 保留别名 |
| `File(path).readText()` | 不变 | 保留 |
| `File(path).writeText(c)` | 不变 | 保留 |

### 7.2 新增公共 API

| 新 API | 说明 |
|--------|------|
| `Files.read(path)` | 静态读取文件 |
| `Files.write(path, content)` | 静态写入文件 |
| `File(path).readBytes()` | 二进制读取 |
| `File(path).writeBytes(bytes)` | 二进制写入 |
| `FileInputStream(path)` | 文件输入流 |
| `FileOutputStream(path)` | 文件输出流 |
| `ByteArrayInputStream(data)` | 内存输入流 |
| `ByteArrayOutputStream()` | 内存输出流 |
| `FileUtils.batchWrite(paths, content)` | 批量写入 |
| `FileUtils.batchRead(paths)` | 批量读取 |
| `Ansi.red(text)` | ANSI 红色文本 |
| `Ansi.bold(text)` | ANSI 粗体文本 |

### 7.3 测试矩阵

| 测试项 | 旧代码 | 新代码 |
|--------|--------|--------|
| `FileSystem.exists("f")` | ✅ | ✅ (deprecated) |
| `FileSystem.readText("f")` | ✅ | ✅ (deprecated) |
| `FileSystem.writeText("f", "hi")` | ✅ | ✅ (deprecated) |
| `FSUtils.fs_exists("f")` | ✅ | ✅ (deprecated) |
| `Files.exists("f")` | — | ✅ (新) |
| `Files.read("f")` | — | ✅ (新) |
| `Files.write("f", "hi")` | — | ✅ (新) |
| `File("f").readText()` | ✅ | ✅ |
| `File("f").readBytes()` | — | ✅ (新) |
| `FileInputStream("f").readAll()` | — | ✅ (新) |
| `FileOutputStream("f").println("hi")` | — | ✅ (新) |
| `ByteArrayInputStream("hi").readAll()` | — | ✅ (新) |
| `ByteArrayOutputStream().println("hi")` | — | ✅ (新) |
| `FileUtils.batchWrite(paths, "hi")` | — | ✅ (新) |
| `FileUtils.batchRead(paths)` | — | ✅ (新) |
| `IO.println("hello")` | ✅ | ✅ |
| `Console.red("error")` | ✅ | ✅ |
| `Ansi.red("error")` | — | ✅ (新) |
| `Console.clear()` | ✅ | ✅ |

---

## 8. 变更影响矩阵

| 文件 | 操作 | 影响级别 |
|------|------|----------|
| `std/File.aura` | 丰富 | 低 |
| `std/FileUtils.aura` | 新增 | 中 |
| `std/IO.aura` | 重构 | 中 |
| `std/Console.aura` | 重构 | 中 |
| `std/io/InputStream.aura` | 增强 | 低 |
| `std/io/OutputStream.aura` | 增强 | 低 |
| `std/io/Ansi.aura` | 新增 | — |
| `std/io/StreamImpl.aura` | 新增 | — |
| `std/io/FileStream.aura` | 新增 | — |
| `std/io/MemoryStream.aura` | 新增 | — |
| `std/fs/FileSystem.aura` | 删除 | 低 |
| `std/io/StdStreams.aura` | 删除 | 中 |
| `std_fs.rs` → `std_file.rs` | 重命名+重构 | 低 |
| `std_io.rs` | 重构 | 低 |
| `std_console.rs` | 重构 | 低 |
| `runtime.rs` | 修改 | 低 |
| `emit.rs` | 修改 | 低 |
| `decl.rs` | 修改 | 低 |
| `hir.rs` | 修改 | 低 |
| `mod.rs` (codegen + std) | 修改 | 低 |
| `source_index.rs` | 修改 | 低 |
| `checker.rs` | 修改 | 低 |
| `docgen.rs` | 修改 | 低 |
| `interp.rs` | 注释 | 低 |
| `aura_std_cffi.c` | 修改 | 低 |
| `aura/compiler/` (21 files) | 迁移 | 中 |
| `aura/toolchain/` (23 files) | 迁移 | 中 |
| `aura/photon/` (9 files) | 迁移 | 中 |
| `tests/` (9 files) | 迁移 | 中 |

---

## 9. 架构改善总结

### 重构前

```
┌─ Console.aura ──┐     ┌─ IO.aura ──┐     ┌─ Stdio.aura ──┐
│ clear()         │────▶│ println() │────▶│ stringToBuffer│
│ cursorShow()    │     │ print()   │     │ bufferToString│
│ cursorHide()    │     │ readLine()│     │ readFile      │
│ red(text)→Str   │     │ fileRead()│     │ writeFile     │
└────────┬────────┘     └───────────┘     └───────┬───────┘
         │                                        │
         ▼                                        ▼
┌─ StdStreams ──┐                    ┌─ native.Console ─┐
│ DefaultOut    │                   │ writeStdout       │
│ DefaultErr    │                   └───────────────────┘
│ DefaultIn     │
└────────┬──────┘
         │ (循环!)
         ▼
      IO.aura

┌─ FileSystem.aura ──┐     ┌─ File.aura ──┐
│ exists/readText    │     │ readText()    │  (功能不全)
│ writeText/delete   │     │ writeText()   │
│ mkdir/rename/copy  │     │ exists()      │
│ listDir/walk       │     │ size()        │
│ fileSize/mkdirP    │     │ delete()      │
└────────────────────┘     └───────────────┘
 (20+ 方法，多数空实现)     (5 个方法)

┌─ InputStream/OutputStream ──┐
│ 接口已定义但未暴露为公共 API  │
│ 无 FileInputStream/FileOutputStream │
│ 无 ByteArrayInputStream/ByteArrayOutputStream │
└──────────────────────────────┘
```

### 重构后

```
┌─ File.aura ──────────────────────────────────────────────────┐
│                                                               │
│  class File (开发者 API — 有状态文件句柄)                       │
│  ─────────────────────────────────────────────────           │
│  readText() / writeText() / readBytes() / writeBytes()       │
│  exists() / isFile() / size() / delete()                      │
│  renameTo() / copyTo()                                        │
│                                                               │
│  object Files (开发者 API — 静态工具)                           │
│  ─────────────────────────────────────────────────           │
│  read() / write() / exists() / isFile() / isDirectory()       │
│  delete() / mkdir() / mkdirP() / rename() / copy()           │
│  listDir() / listFiles() / fileSize() / walk()               │
│  absolutePath() / homeDir / tempDir / currentDir              │
│                                                               │
│  全部委托到 ──▶ native.Stdio + native.FileOps                   │
└───────────────────────────────────────────────────────────────┘

┌─ FileUtils.aura (基于 File 的工具类) ───────────────────────┐
│                                                               │
│  batchWrite() / batchRead() — 批量操作                         │
│  deleteDir() / copyDir() — 高级目录操作                         │
│  exists/readText/writeText/... — 委托到 Files                 │
│                                                               │
│  FSUtils (@deprecated，委托到 Files)                           │
└───────────────────────────────────────────────────────────────┘

┌─ std/io/ (公共流 API) ───────────────────────────────────────┐
│                                                               │
│  InputStream (interface)                                       │
│  OutputStream (interface)                                      │
│                                                               │
│  FileInputStream / FileOutputStream — 文件流                   │
│  StdInputStream / StdOutputStream — 标准流                     │
│  ByteArrayInputStream / ByteArrayOutputStream — 内存流        │
└───────────────────────────────────────────────────────────────┘

┌─ IO.aura (内部) ─────────────┐   ┌─ Console.aura (内部) ────┐
│ println/print/readLine/readAll│   │ println/print             │
│ flush                         │   │ clear() (生成+输出)      │
│                               │   │ red/green/bold → Ansi    │
│ prelu.aura 使用               │   │ cursorShow/Hide → String │
└───────────────────────────────┘   │ size/width/height        │
                                     └──────────────────────────┘

依赖关系（无循环）:
  prelu.aura ──▶ IO.aura ──▶ StreamImpl ──▶ native.Console
  Console.aura ──▶ Ansi.aura (无依赖)
  Console.aura ──▶ StreamImpl ──▶ native.Console
  File.aura ──▶ native.Stdio + native.FileOps
  FileUtils.aura ──▶ Files ──▶ native.Stdio + native.FileOps
  FileInputStream ──▶ native.FileOps + native.Stdio
  FileOutputStream ──▶ native.FileOps + native.Stdio
```

**改善**:
1. ✅ **消除循环依赖**
2. ✅ **统一开发者 API** — File.aura 是唯一的文件操作入口
3. ✅ **FileUtils 降级为工具类** — 基于 File/Files 构建，提供批量操作
4. ✅ **流 API 公共化** — InputStream/OutputStream + 4 种默认实现
5. ✅ **Console 纯逻辑** — ANSI 生成不依赖 IO
6. ✅ **IO 职责单一** — 仅 stdin/stdout
7. ✅ **一致的 C FFI 命名**
8. ✅ **完整迁移清单** — 64 个 Aura 文件 + 10+ Rust 文件

---

## 10. 验证清单

- [ ] `cargo test -p compiler` 全部通过
- [ ] `File("test.txt").writeText("hello")` 在 VM 和 AOT 下均正常
- [ ] `File("test.txt").readText()` 在 VM 和 AOT 下均正常
- [ ] `Files.read("test.txt")` 在 VM 和 AOT 下均正常
- [ ] `Files.write("out.txt", "hi")` 在 VM 和 AOT 下均正常
- [ ] `FileInputStream("test.txt").readAll()` 在 VM 下正常
- [ ] `FileOutputStream("out.txt").println("hi")` 在 VM 下正常
- [ ] `ByteArrayInputStream("hi").readAll()` 在 VM 下正常
- [ ] `ByteArrayOutputStream().println("hi")` + `.toString()` 在 VM 下正常
- [ ] `FileUtils.batchWrite(paths, "hi")` 在 VM 下正常
- [ ] `FileUtils.batchRead(paths)` 在 VM 下正常
- [ ] `FileSystem.exists("f")` 在 VM 下仍正常（deprecated）
- [ ] `FileSystem.readText("f")` 在 VM 下仍正常（deprecated）
- [ ] `FSUtils.fs_exists("f")` 在 VM 下仍正常（deprecated）
- [ ] `IO.println("hello")` 在 VM 和 AOT 下均正常
- [ ] `println("hello")` (prelu) 在 VM 和 AOT 下均正常
- [ ] `Console.red("error")` 在 VM 和 AOT 下均正常
- [ ] `Ansi.red("error")` 在 VM 和 AOT 下均正常
- [ ] `Console.clear()` 在 VM 和 AOT 下均正常
- [ ] 所有 64 个 Aura 文件已迁移 FileSystem → Files
- [ ] `stdlib-compile` 后嵌入 .auc 正确
- [ ] `aura_lang_std_Console_print` 旧符号在 C FFI 中仍可用
- [ ] `aura_lang_std_FileSystem_*` 旧符号在 C FFI 中仍可用
