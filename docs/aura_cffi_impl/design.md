# 详细设计：语法、类型、架构

---

## 一、新增语法规范

### 1.1 `extern object`

```
Syntax:
  extern object <Name> {
    <MethodDecl>*
  }

MethodDecl:
  'default' 'fun' 'loadLibrary' '(' ')' ':' 'String' '=' STRING   // AOT 库路径
  | '@aot' 'fun' <Name>(<Params>): <ReturnType>                   // AOT 库函数
  | [@NativeAnnotation] fun <Name>(<Params>): <ReturnType>        // 系统调用/内联汇编
  | 'native' fun <Name>(<Params>): <ReturnType>                   // 编译器内置
  | 'export' 'fun' <Name>(<Params>): <ReturnType> '{' <Block> '}'  // 导出符号
```

**语义**：
- `extern object` 是一个外部方法容器，支持系统级绑定和 AOT 库绑定
- **系统级模式**（无 `loadLibrary()`）：包含 `@native`/`native`/`export` 方法
- **AOT 库模式**（有 `loadLibrary()`）：包含 `@aot` 方法，绑定到 Aura AOT 动态库
- `default fun loadLibrary(): String = "libname"` 声明库路径（仅 AOT 库模式需要）
- `@aot fun` 函数实现在 AOT 库中，通过 JitValue ABI 调用

**示例**：
```aura
// 系统级绑定（syscall）
extern object Syscalls {
    @native(1) fun write(fd: Int, buf: Long, count: Long): Long
    @native(0) fun read(fd: Int, buf: Long, count: Long): Long
}

// 编译器内置
extern object Memory {
    native fun read(addr: Long): Byte
    native fun write(addr: Long, v: Byte)
}

// 内联汇编
extern object Cpu {
    @native(asm = "rdtsc") fun rdtsc(): Long
    @native(asm = "mfence") fun memFence()
}

// AOT 库绑定
extern object Utils {
    default fun loadLibrary(): String = "utils"
    @aot fun add(a: Int, b: Int): Int
    @aot fun multiply(a: Int, b: Int): Int
}
```

### 1.2 `@native` 标注

```
@NativeAnnotation:
  @native(<SyscallNumber>)
  @native("libc:<SymbolName>")
  @native(asm = "<AssemblyCode>")
  native

<AssemblyCode>:
  字符串字面量，LLVM inline asm 语法

<SyscallNumber>:
  整数常量（0-511）

<SymbolName>:
  字符串字面量
```

**语义**：
- `@native(N)`：系统调用，N 为 syscall 号
- `@native("libc:name")`：外部库符号（libc 或其他链接库）
- `@native(asm = "...")`：内联汇编
- `native`（无参数）：编译器内置（内存操作、常量等）

### 1.3 `export` 标注

```
export fun <Name>(<Params>): <ReturnType> { <Body> }
```

**语义**：
- 标记方法为导出符号
- 符号名默认为方法名（如 `@export fun malloc` → 符号 `malloc`）
- 可以用 `@export("custom_name")` 指定符号名
- 必须有 Aura 实现体

### 1.4 `@aot` 标注

```
@aot fun <Name>(<Params>): <ReturnType>
```

**语义**：
- 标记方法为 AOT 库函数（JitValue ABI 直调）
- 必须配合 `default fun loadLibrary(): String = "libname"` 使用
- 函数实现在 AOT 编译的动态库中
- 调用约定：JitValue ABI（零参数转换开销）
- 导出符号格式：`aura_aot_<name>!<arg_types>!<ret_type>`

**示例**：
```aura
extern object Math {
    default fun loadLibrary(): String = "aura_std_math"
    @aot fun abs(x: Int): Int
    @aot fun sin(x: Float): Float
    @aot fun sqrt(x: Float): Float
}
```

---

## 二、类型系统

### 2.1 基础类型

| Aura 类型 | LLVM IR 类型 | 大小 | 说明 |
|---|---|---|---|
| `Int` | `i32` | 4 bytes | 32 位有符号整数 |
| `Long` | `i64` | 8 bytes | 64 位有符号整数 |
| `Byte` | `i8` | 1 byte | 8 位字节 |
| `Short` | `i16` | 2 bytes | 16 位有符号整数 |
| `Float` | `f32` | 4 bytes | 32 位浮点 |
| `Double` | `f64` | 8 bytes | 64 位浮点 |
| `Boolean` | `i1` | 1 byte | 布尔 |
| `String` | `{i8*, i64}` | 16 bytes | 字符串（数据指针 + 长度） |
| `CString` | `i8*` | 8 bytes | C 风格字符串（NUL 结尾） |
| `Unit` | `void` | 0 bytes | 空 |
| `Any` | `i64` | 8 bytes | 装箱值（Plan A） |

### 2.2 内存地址表示

**约定**：内存地址用 `Long` 表示。

```aura
val addr: Long = 0x7FFF1234  // 内存地址
val p: Long = Syscalls.mmap(...)  // mmap 返回地址
Memory.read(p)  // 从地址 p 读取字节
```

**转换**：
```aura
// CString → Long
val addr: Long = cstr as Long

// Long → CString
val cstr: CString = addr as CString

// Long → String
val str: String = addr as String
```

### 2.3 空值表示

**约定**：空指针用 `0` 表示。

```aura
val addr: Long = 0  // 空指针
if (addr == 0) { ... }  // 空指针检查
```

---

## 三、内存操作 API

### 3.1 `Memory` 对象

```aura
extern object Memory {
    // 读取
    native fun read(addr: Long): Byte
    native fun read16(addr: Long): Short
    native fun read32(addr: Long): Int
    native fun read64(addr: Long): Long
    
    // 写入
    native fun write(addr: Long, v: Byte)
    native fun write16(addr: Long, v: Short)
    native fun write32(addr: Long, v: Int)
    native fun write64(addr: Long, v: Long)
    
    // 块操作
    native fun copy(dst: Long, src: Long, n: Long)
    native fun set(addr: Long, v: Byte, n: Long)
    
    // 分配/释放（编译器内置，可替换为 Allocator）
    native fun alloc(n: Long): Long
    native fun free(addr: Long)
}
```

### 3.2 使用示例

```aura
// 读取字符串长度
fun strlen(s: CString): Long {
    val addr: Long = s as Long
    if (addr == 0) { return 0 }
    var i: Long = 0
    var c: Byte = Memory.read(addr + i)
    while (c != 0) {
        i = i + 1
        c = Memory.read(addr + i)
    }
    return i
}

// 写入整数到内存
fun writeInt(addr: Long, v: Int) {
    Memory.write(addr, (v & 0xFF) as Byte)
    Memory.write(addr + 1, ((v >> 8) & 0xFF) as Byte)
    Memory.write(addr + 2, ((v >> 16) & 0xFF) as Byte)
    Memory.write(addr + 3, ((v >> 24) & 0xFF) as Byte)
}
```

---

## 四、系统调用 API

### 4.1 x86_64 Linux Syscalls

```aura
extern object Syscalls {
    @native(0)   fun read(fd: Int, buf: Long, count: Long): Long
    @native(1)   fun write(fd: Int, buf: Long, count: Long): Long
    @native(2)   fun open(path: CString, flags: Int): Int
    @native(3)   fun close(fd: Int): Int
    @native(5)   fun fstat(fd: Int, buf: Long): Long
    @native(6)   fun lseek(fd: Int, off: Long, whence: Int): Long
    @native(9)   fun mmap(addr: Long, len: Long, prot: Int, flags: Int, fd: Int, off: Long): Long
    @native(10)  fun munmap(addr: Long, len: Long): Long
    @native(21)  fun access(path: CString, mode: Int): Int
    @native(39)  fun unlink(path: CString): Int
    @native(59)  fun execve(path: CString, args: Long, env: Long): Long
    @native(60)  fun exitGroup(code: Int)
    @native(61)  fun wait4(pid: Int, status: Long, options: Int, rusage: Long): Int
    @native(228) fun clockGettime(clock: Int, ts: Long): Int
    @native(272) fun getrandom(buf: Long, len: Long, flags: Int): Long
}
```

### 4.2 使用示例

```aura
// 写入标准输出
fun writeStdout(data: Long, len: Long): Long {
    return Syscalls.write(1, data, len)
}

// 分配内存
fun allocMemory(size: Long): Long {
    return Syscalls.mmap(0, size, 3, 0x22, -1, 0)
}

// 退出进程
fun exit(code: Int) {
    Syscalls.exitGroup(code)
}
```

---

## 五、CPU 级操作 API

### 5.1 `Cpu` 对象

```aura
extern object Cpu {
    @native(asm = "rdtsc") fun rdtsc(): Long
    @native(asm = "mfence") fun memFence()
    @native(asm = "cpuid") fun cpuid(level: Int): Long
    @native(asm = "lock xaddq") fun atomicAdd(addr: Long, delta: Long): Long
}
```

### 5.2 使用示例

```aura
// 读取 CPU 时间戳
fun cpuTime(): Long {
    return Cpu.rdtsc()
}

// 内存屏障
fun ensureMemoryOrder() {
    Cpu.memFence()
}

// 原子加法
fun atomicIncrement(addr: Long) {
    Cpu.atomicAdd(addr, 1)
}
```

---

## 六、运行库对象设计

### 6.1 对象结构

每个运行库模块是一个 `object`，包含：
- 状态变量（`var`）
- 导出方法（`@export`）
- 内部方法（普通 `fun`）

**示例**：
```aura
object Allocator {
    var HEAP_SIZE: Long = 1024 * 1024
    var HEAP: Long = Syscalls.mmap(0, HEAP_SIZE, 3, 0x22, -1, 0)
    var HEAP_POS: Long = 0
    var HEAP_USED: Long = 0

    export fun malloc(n: Long): Long {
        // ...
    }

    export fun free(addr: Long) {
        // ...
    }

    fun expandHeap() {
        // ...
    }
}
```

### 6.2 符号命名约定

**导出符号名**：默认与方法名相同。

```aura
export fun malloc(n: Long): Long { ... }  // 符号: malloc
export fun free(addr: Long) { ... }       // 符号: free
export fun println(s: String) { ... }     // 符号: println
```

**带前缀的符号**：可以用 `@export("prefix:name")` 指定。

```aura
@export("aura_malloc") fun malloc(n: Long): Long { ... }  // 符号: aura_malloc
@export("aura_free") fun free(addr: Long) { ... }         // 符号: aura_free
```

---

## 七、跨平台设计

### 7.1 平台特定文件

```
aura/core/aura/lang/runtime/arch/
├── x86_64_linux/Syscalls.aura      # x86_64 Linux syscall 号
├── x86_64_windows/Syscalls.aura    # Windows Nt* 函数
├── aarch64_linux/Syscalls.aura     # aarch64 Linux syscall 号
└── aarch64_darwin/Syscalls.aura    # macOS syscall 号
```

### 7.2 平台选择

**构建时通过 `--target` 参数选择平台**：

```bash
aura build Runtime.aura --aot --target x86_64_linux --obj -o aura_runtime_linux.obj
aura build Runtime.aura --aot --target x86_64_windows --obj -o aura_runtime_windows.obj
```

### 7.3 平台无关代码

大部分运行库代码跨平台（字符串、数学、随机数等），只有 syscall 表需要分平台。

---

## 八、编译期翻译详解

### 8.1 `@native(N)` → inline asm

**x86_64 Linux**：
```llvm
define i64 @write(i32 %fd, i64 %buf, i64 %count) {
  %r = call i64 asm sideeffect
    "mov $1, %rax\n\tmov $1, %rdi\n\tmov $2, %rsi\n\tmov $3, %rdx\n\tsyscall",
    "={rax},r,r,r",
    i32 %fd, i64 %buf, i64 %count
  ret i64 %r
}
```

**x86_64 Windows**（使用 `NtWriteVirtualMemory`）：
```llvm
define i64 @write(i32 %fd, i64 %buf, i64 %count) {
  %r = call i64 asm sideeffect
    "mov $0x3B, %rax\n\tmov $2, %rdi\n\tmov $1, %rsi\n\tmov $3, %rdx\n\tsyscall",
    "={rax},r,r,r",
    i32 %fd, i64 %buf, i64 %count
  ret i64 %r
}
```

### 8.2 `native`（无参数）→ load/store

```llvm
define i8 @Memory.read(i64 %addr) {
  %r = load i8, ptr %addr
  ret i8 %r
}

define void @Memory.write(i64 %addr, i8 %v) {
  store i8 %v, ptr %addr
  ret void
}
```

### 8.3 `@native(asm = "...")` → inline asm

```llvm
define i64 @Cpu.rdtsc() {
  %r = call i64 asm sideeffect "rdtsc", "={eax},{edx}"
  ret i64 %r
}
```

### 8.4 `export` → define external

```llvm
define i64 @malloc(i64 %n) {
  // ... Aura 实现体翻译
  ret i64 %r
}
```

---

## 九、错误处理

### 9.1 syscall 错误

**约定**：syscall 返回负数表示错误。

```aura
val fd: Int = Syscalls.open(path, 0)
if (fd < 0) {
    Console.println("Failed to open file")
    return
}
```

### 9.2 内存分配失败

**约定**：`mmap` 返回 `-1` 表示失败。

```aura
val addr: Long = Syscalls.mmap(0, size, 3, 0x22, -1, 0)
if (addr == -1L) {
    Console.println("Out of memory")
    Syscalls.exitGroup(1)
}
```

### 9.3 空指针检查

**约定**：空指针用 `0` 表示。

```aura
if (addr == 0) {
    Console.println("Null pointer")
    return
}
```

---

## 十、性能考虑

### 10.1 内存分配器

**bump allocator**：
- 优点：极快（O(1) 分配）
- 缺点：不支持自由分配，需要 GC 或手动重置

**dlmalloc**（可选升级）：
- 优点：支持自由分配，通用
- 缺点：慢，复杂

**建议**：先用 bump allocator，Phase S2 评估是否需要升级。

### 10.2 数学函数

**Taylor 级数**：
- 优点：纯 Aura 实现，无外部依赖
- 缺点：精度中等，速度慢

**查表 + 插值**（可选优化）：
- 优点：快
- 缺点：精度低，需要查表数据

**建议**：先用 Taylor 级数，必要时优化。

### 10.3 字符串操作

**逐字节循环**：
- 优点：简单，易理解
- 缺点：慢（未优化）

**向量化**（可选优化）：
- 优点：快（SIMD）
- 缺点：复杂，需要平台特定代码

**建议**：先用逐字节循环，必要时优化。

---

## 十一、安全性

### 11.1 内存安全

**风险**：裸内存操作（`Memory.read`/`Memory.write`）可能越界。

**缓解**：
- 运行库内部代码需要人工审查
- 添加边界检查（可选，调试模式）
- 使用 `@export` 限制外部访问

### 11.2 输入验证

**风险**：外部输入可能导致缓冲区溢出。

**缓解**：
- syscall 前验证参数
- 长度检查
- 边界检查

**示例**：
```aura
fun safeWrite(fd: Int, buf: Long, len: Long): Long {
    if (fd < 0) { return -1 }
    if (buf == 0) { return -1 }
    if (len < 0) { return -1 }
    return Syscalls.write(fd, buf, len)
}
```
