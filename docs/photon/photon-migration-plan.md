# Photon 后端完整迁移方案

## 执行摘要

**目标**: 将编译器从混合架构（Rust 前端 + LLVM 后端）完全迁移到纯 Photon 后端，实现自举编译。

**总周期**: 20-28 周 (5-7 个月)

**人力需求**: 2-3 名编译器工程师

**最终交付**: `aura.exe` 由 Photon 后端编译生成，可自举编译自身。

---

## 当前状态分析

### 已完成 (Phase 0)
```
✅ Rust 前端: 解析器、语义分析器、HIR 生成器
✅ LLVM 后端: AOT 编译，用于编译自举编译器
✅ VM 后端: 字节码解释执行
✅ Photon 后端基础: Hello World 编译成功
✅ HIR 生成: Main.aura 前端编译成功 (2153 函数)
```

### 待完成
```
❌ Photon 后端集成: cmd_build_photon 仅完成前端
❌ 代码生成: 仅支持简单函数，不支持复杂控制流
❌ 原生函数调用: Photon 后端不支持 @native 标记的系统调用
❌ 运行时: 内存管理、GC 未实现
❌ 自举验证: 未能用 Photon 编译产物重新编译自身
```

### 标准库现状

**重要发现**: 标准库已经完整实现，不需要重新实现！

```
✅ 上层 API (纯 Aura 实现):
  - String.aura: 808 行，完整的字符串操作
  - File.aura: 文件读写
  - Process.aura: 进程管理
  - List, Map, HashMap: 集合类型
  - Math, Time, Random: 工具库
  - 共 90+ 个 .aura 文件

✅ 底层原生接口 (使用 @native 标记):
  - FileOps.aura: 文件 I/O 系统调用
  - ProcessOps.aura: 进程管理
  - Memory.aura: 内存管理
  - ThreadOps.aura: 线程操作
  - Syscalls.aura: 系统调用封装

❌ Photon 后端缺口:
  - 不支持 @native 标记的函数调用
  - 无法生成系统调用机器码
  - 无法链接原生函数实现
```

### 标准库架构

```
上层 API (纯 Aura)          底层接口 (@native)
String.aura                 FileOps.aura
  ↓                           ↓
File.aura  ──────────────── FileOps.open()
  ↓                           ↓
Process.aura                ProcessOps.aura
  ↓                           ↓
List, Map, Math...         Memory.aura
                            ThreadOps.aura
```

**关键问题**: Photon 后端需要支持 `@native` 标记的函数调用，或者为这些接口提供纯 Aura 实现。

---

## 迁移阶段总览

```
Phase 0: 基线建立 (当前)          [已完成]
  ↓
Phase 1: 基础设施 (2-3 周)        [必须]
  ↓
Phase 2: 核心代码生成 (4-6 周)    [必须]
  ↓
Phase 3: 原生函数支持 (4-6 周)    [必须] - 标准库已实现，需支持 @native
  ↓
Phase 4: 运行时系统 (3-4 周)      [必须]
  ↓
Phase 5: 集成验证 (2-3 周)        [必须]
  ↓
Phase 6: 自举验证 (2-3 周)        [目标]
```

**总周期**: 17-25 周 (4-6 个月)

### 关键理解

**标准库已经完整实现** (90+ 个 .aura 文件)，不需要重新实现！

**架构分层**:
```
上层 API (纯 Aura)          底层接口 (@native)
String.aura                 FileOps.aura
File.aura  ──────────────── FileOps.open()
Process.aura                ProcessOps.aura
List, Map, Math...         Memory.aura
                            ThreadOps.aura
```

**Photon 后端的核心缺口**:
1. 不支持 `@native` 标记的函数调用
2. 无法生成系统调用机器码
3. 无法链接原生函数实现

---

## Phase 1: 基础设施 (2-3 周)

### 目标
建立 Photon 后端的自动化编译流水线，能够编译任意 Aura 源码文件。

### 任务清单

#### 1.1 修改 `cmd_build_photon` (3-5 天)
**文件**: `rust/cli/src/main.rs`

**当前状态**:
```rust
fn cmd_build_photon(args: &[String]) {
    // 仅完成前端: 解析 → 语义分析 → HIR 生成
    // 输出 HIR 文件 + 提示消息
}
```

**目标状态**:
```rust
fn cmd_build_photon(args: &[String]) {
    // 1. 前端: 解析 → 语义分析 → HIR 生成
    // 2. 序列化 HIR → 传递给 Aura VM
    // 3. 启动 Aura VM 执行 Photon 后端
    // 4. 收集机器码 → 链接 → 输出 exe
}
```

**具体修改**:
1. 实现 HIR 序列化格式 (JSON/二进制)
2. 调用 `aura run PhotonDriver.aura` 执行后端
3. 解析后端输出 (COFF hex)
4. 调用链接器生成 exe

**验证标准**:
```bash
aura build -b photon hello.aura --output hello.exe
# 成功生成 hello.exe，输出 "Hello World"
```

#### 1.2 创建通用编译驱动 (3-5 天)
**文件**: `aura/compiler/aura/lang/compiler/backend/photon/PhotonDriver.aura`

**功能**:
- 读取 HIR 输入 (从文件或 stdin)
- 执行完整编译管线 (HIR → SSA → LIR → DAG → RegAlloc → Encode → COFF)
- 输出 COFF 目标文件

**接口设计**:
```aura
fun main() {
    // 1. 读取参数: HIR 文件路径、输出目录
    // 2. 加载 HIR
    // 3. 执行编译管线
    // 4. 输出 COFF hex 或 .obj 文件
}
```

**验证标准**:
```bash
aura run PhotonDriver.aura --hir main.hir --out build/
# 成功生成 main.obj
```

#### 1.3 HIR 序列化实现 (2-3 天)
**文件**: `aura/compiler/aura/lang/compiler/hir/HirSerializer.aura`

**功能**:
- Rust HIR → JSON/二进制格式
- JSON/二进制格式 → Aura HIR
- 支持所有 HIR 节点类型

**格式设计**:
```json
{
  "version": 1,
  "functions": [
    {
      "name": "main",
      "params": [...],
      "ret": "Unit",
      "body": [...]
    }
  ]
}
```

**验证标准**:
```bash
# Rust 生成 HIR
aura build -b photon test.aura --hir-out test.hir
# Aura 读取 HIR
aura run PhotonDriver.aura --hir test.hir --out build/
```

#### 1.4 构建脚本整合 (1-2 天)
**文件**: `scripts/build-photon-full.ps1`

**功能**:
- 整合前端 + 后端 + 链接
- 支持 --output, --debug 等参数
- 错误处理和状态报告

**验证标准**:
```bash
scripts\build-photon-full.ps1 test.aura --output test.exe
# 成功生成 test.exe
```

### Phase 1 交付物
- [ ] `cmd_build_photon` 完整实现
- [ ] `PhotonDriver.aura` 通用驱动
- [ ] `HirSerializer.aura` HIR 序列化
- [ ] `build-photon-full.ps1` 构建脚本
- [ ] 测试用例: 10+ 简单程序编译成功

---

## Phase 2: 核心代码生成 (4-6 周)

### 目标
完善 Photon 后端的代码生成能力，支持 Aura 语言的核心特性。

### 任务清单

#### 2.1 控制流支持 (1-2 周)
**文件**: `MachineDag.aura`, `InstructionSelection.aura`, `X86Emitter.aura`

**需要支持**:
- 条件分支 (if/else)
- 循环 (while, for, do-while)
- 跳转 (break, continue, return)
- 函数调用和返回

**实现细节**:
```aura
// 条件分支
if (cond) {
    // then 块
} else {
    // else 块
}

// 生成 DAG:
//   %cond = load cond
//   br %cond, %then, %else
//   %then: ...
//   %else: ...
//   %merge: ...
```

**验证标准**:
```aura
fun testBranch(): Int {
    val x: Int = 5
    if (x > 3) {
        return 1
    } else {
        return 0
    }
}
```
编译成功，运行结果正确。

#### 2.2 内存管理 (1-2 周)
**文件**: `RegisterAllocator.aura`, `X86Emitter.aura`

**需要支持**:
- 栈帧分配 (局部变量)
- 堆分配 (malloc/free)
- 指针操作 (load/store)
- 结构体和数组

**实现细节**:
```aura
// 栈帧布局
// [rbp + 0]  = saved rbp
// [rbp - 8]  = param 1
// [rbp - 16] = local var 1
// [rbp - 24] = local var 2

// 局部变量访问
mov rax, [rbp - 16]  // load local var 1
mov [rbp - 16], rax  // store local var 1
```

**验证标准**:
```aura
fun testMemory(): Int {
    val x: Int = 10
    val y: Int = 20
    val z: Int = x + y
    return z
}
```
编译成功，运行结果正确 (z = 30)。

#### 2.3 类型系统支持 (1-2 周)
**文件**: `TypeRegistry.aura`, `MachineDag.aura`

**需要支持**:
- 基础类型 (Int, Float, Bool, String)
- 引用类型 (类、接口)
- 泛型 (单态化)
- 类型转换

**实现细节**:
```aura
// 类定义
class MyClass {
    val x: Int
    fun method(): Int {
        return this.x
    }
}

// 虚函数表
// vtable: [method0, method1, ...]
// this: [vtable ptr, field0, field1, ...]
```

**验证标准**:
```aura
class Foo {
    fun bar(): Int { return 42 }
}

fun testClass(): Int {
    val f: Foo = new Foo()
    return f.bar()
}
```
编译成功，运行结果正确 (42)。

#### 2.4 调用约定完善 (1 周)
**文件**: `X86Emitter.aura`, `RegisterAllocator.aura`

**需要支持**:
- Windows x64 调用约定
- 寄存器参数 (rcx, rdx, r8, r9)
- 栈参数 (超过 4 个)
- 返回值 (rax)
- 影子空间 (32 字节)

**实现细节**:
```asm
; 函数调用
sub rsp, 32       ; 影子空间
mov rcx, arg1     ; 参数 1
mov rdx, arg2     ; 参数 2
mov r8, arg3      ; 参数 3
mov r9, arg4      ; 参数 4
; arg5+ 在栈上
call funcName
add rsp, 32
```

**验证标准**:
```aura
fun testCall(a: Int, b: Int, c: Int, d: Int, e: Int): Int {
    return a + b + c + d + e
}
```
编译成功，运行结果正确。

#### 2.5 优化 Pass (1 周)
**文件**: `PhotonOptPasses.aura`

**需要实现**:
- 死代码消除 (DCE)
- 常量传播
- 公共子表达式消除 (CSE)
- 循环不变量外提 (LICM)

**验证标准**:
```aura
// 优化前
fun test(x: Int): Int {
    val y: Int = x + 1
    val z: Int = x + 1
    return y + z
}

// 优化后 (CSE)
fun test(x: Int): Int {
    val y: Int = x + 1
    return y + y
}
```
优化后代码大小减少，运行结果相同。

### Phase 2 交付物
- [ ] 控制流支持: if/else, while, for, break, continue
- [ ] 内存管理: 栈帧、堆分配、指针操作
- [ ] 类型系统: 类、接口、泛型
- [ ] 调用约定: 多参数、返回值、影子空间
- [ ] 优化 Pass: DCE, CSE, 常量传播
- [ ] 测试用例: 50+ 中等复杂度程序编译成功

---

## Phase 3: 原生函数支持 (4-6 周)

### 目标
让 Photon 后端支持 `@native` 标记的函数调用，使现有的标准库能够工作。

### 背景
标准库已经完整实现（90+ 个 .aura 文件），上层 API 用纯 Aura 编写，底层通过 `@native` 标记的接口调用系统函数。Photon 后端需要支持这些原生函数调用。

### 任务清单

#### 3.1 原生函数调用支持 (2 周)
**文件**: `InstructionSelection.aura`, `X86Emitter.aura`, `MachineDag.aura`

**需要支持**:
- 识别 `@native` 标记的函数
- 生成正确的调用机器码
- 处理系统调用编号

**实现细节**:
```aura
// 当前 @native 标记的函数
@native(SYS_OPEN)   fun open(path: Long, flags: Int): Int
@native(SYS_CLOSE)  fun close(fd: Int): Int

// Photon 后端需要生成:
// mov rax, SYS_OPEN  // 系统调用编号
// mov rdi, path      // 参数 1
// mov rsi, flags     // 参数 2
// syscall            // 执行系统调用
// ret rax            // 返回值
```

**系统调用编号映射**:
```aura
// Windows (x64)
const val NtOpenFile = 0x0005
const val NtClose = 0x0014
const val NtWriteFile = 0x0006
const val NtReadFile = 0x0003

// Linux (x86_64)
const val SYS_open = 2
const val SYS_close = 3
const val SYS_write = 1
const val SYS_read = 0
```

**验证标准**:
```aura
// 测试原生函数调用
fun testNative(): Int {
    val fd: Int = FileOps.open("test.txt", 0)
    if (fd >= 0) {
        FileOps.close(fd)
        return 1
    }
    return 0
}
```
编译成功，运行结果正确。

#### 3.2 内存管理接口 (1 周)
**文件**: `Memory.aura`, `Allocator.aura`

**需要支持**:
- `Allocator.malloc(size)` → 返回指针
- `Allocator.free(ptr)` → 释放内存
- `Memory.copy(src, dst, len)` → 内存复制

**实现方式**:
```aura
// 使用系统调用实现内存分配
fun malloc(size: Long): Long {
    // 简单的堆分配器
    // 维护堆指针
    val ptr: Long = heapPtr
    heapPtr = heapPtr + size
    return ptr
}
```

**验证标准**:
```aura
fun testMalloc(): Int {
    val ptr: Long = Allocator.malloc(100)
    if (ptr != 0) {
        Allocator.free(ptr)
        return 1
    }
    return 0
}
```
编译成功，运行结果正确 (1)。

#### 3.3 进程和线程接口 (1-2 周)
**文件**: `ProcessOps.aura`, `ThreadOps.aura`

**需要支持**:
- 进程退出: `ExitProcess(code)`
- 环境变量: `GetEnvironmentVariable`
- 线程创建: `CreateThread`
- 线程同步: `CreateMutex`, `WaitForSingleObject`

**实现方式**:
```aura
// Windows 进程退出
fun exit(code: Int): Unit {
    // NtExitProcess 系统调用
    mov rax, NtExitProcess
    mov rcx, code
    syscall
}
```

**验证标准**:
```aura
fun testProcess(): Int {
    Process.exit(0)
    return 0
}
```
编译成功，运行结果正确。

#### 3.4 标准库集成测试 (1-2 周)
**测试用例**:
- [ ] String 操作 (substring, indexOf, startsWith)
- [ ] 文件读写 (readText, writeText)
- [ ] 进程管理 (exit, getenv)
- [ ] 集合类型 (List, Map)
- [ ] 数学工具 (Math.max, Math.min)

**验证标准**:
```bash
# 测试 String
aura build -b photon test_string.aura --output test_string.exe
test_string.exe  # 输出正确

# 测试 File
aura build -b photon test_file.aura --output test_file.exe
test_file.exe  # 输出正确

# 测试 Process
aura build -b photon test_process.aura --output test_process.exe
test_process.exe  # 正常退出
```

### Phase 3 交付物
- [ ] 原生函数调用支持 (@native)
- [ ] 系统调用编号映射
- [ ] 内存管理接口实现
- [ ] 进程和线程接口实现
- [ ] 标准库集成测试通过 (100+ 函数)

---

## Phase 4: 运行时系统 (3-4 周)

### 目标
实现完整的运行时系统，支持内存管理、垃圾回收和异常处理。

### 任务清单

#### 4.1 内存管理 (1 周)
**文件**: `core/aura/lang/runtime/Memory.aura`

**需要实现**:
- 堆分配 (malloc/free)
- 栈分配
- 内存池

**实现方式**:
```aura
// 简单的堆分配器
var heapStart: Long = 0
var heapEnd: Long = 0
var heapSize: Long = 0

fun malloc(size: Long): Long {
    if (heapEnd + size > heapSize) {
        return 0  // 分配失败
    }
    val ptr: Long = heapEnd
    heapEnd = heapEnd + size
    return ptr
}

fun free(ptr: Long): Unit {
    // 简单实现: 不释放，留给 GC
}
```

**验证标准**:
```aura
fun testMemory(): Int {
    val ptr: Long = malloc(100)
    if (ptr == 0) {
        return 0
    }
    return 1
}
```
编译成功，运行结果正确 (1)。

#### 4.2 垃圾回收 (2 周)
**文件**: `core/aura/lang/runtime/GC.aura`

**需要实现**:
- 引用计数 (ARC)
- 根标记 (根集合)
- 可达性分析
- 内存回收

**实现方式**:
```aura
// 引用计数
class Object {
    val rc: Int = 1  // 引用计数
}

fun retain(obj: Object): Unit {
    obj.rc = obj.rc + 1
}

fun release(obj: Object): Unit {
    obj.rc = obj.rc - 1
    if (obj.rc == 0) {
        free(obj)
    }
}

// GC 触发
fun gc(): Unit {
    var objects: List<Object> = getAllObjects()
    var freed: Int = 0
    for (obj: Object in objects) {
        if (obj.rc == 0) {
            free(obj)
            freed = freed + 1
        }
    }
}
```

**验证标准**:
```aura
fun testGC(): Int {
    val obj1: Object = new Object()
    val obj2: Object = new Object()
    obj1 = null  // 引用计数减少
    gc()
    return 1  // 应能正常返回
}
```
编译成功，运行结果正确 (1)。

#### 4.3 异常处理 (1-2 周)
**文件**: `core/aura/lang/runtime/Exception.aura`

**需要实现**:
- try/catch/finally
- 异常抛出
- 异常表 (Windows SEH)

**实现方式** (Windows SEH):
```aura
// Windows x64 SEH 异常处理
// .pdata 节: 异常处理程序信息
// .xdata 节: 异常处理数据

fun setupExceptionHandler(): Unit {
    // 注册异常处理程序
    AddVectoredExceptionHandler(0, handler)
}

fun handler(info: Long): Long {
    // 处理异常
    return 1  // EXCEPTION_CONTINUE_SEARCH
}
```

**验证标准**:
```aura
fun testException(): Int {
    try {
        val x: Int = 10 / 0  // 除零异常
        return 0
    } catch (e: Exception) {
        return 1
    } finally {
        // 清理
    }
}
```
编译成功，运行结果正确 (1)。

#### 4.4 线程支持 (1 周)
**文件**: `core/aura/lang/runtime/Thread.aura`

**需要实现**:
- 线程创建
- 线程同步 (Mutex, ConditionVariable)
- 线程局部存储

**实现方式**:
```aura
// Windows 线程 API
// CreateThread, WaitForSingleObject, CreateMutex

fun createThread(func: Func): Long {
    return CreateThread(nil, 0, func, nil, 0, nil)
}

class Mutex {
    val handle: Long = CreateMutex(nil, false, nil)

    fun lock(): Unit {
        WaitForSingleObject(handle, INFINITE)
    }

    fun unlock(): Unit {
        ReleaseMutex(handle)
    }
}
```

**验证标准**:
```aura
fun testThread(): Int {
    val mutex: Mutex = new Mutex()
    mutex.lock()
    mutex.unlock()
    return 1
}
```
编译成功，运行结果正确 (1)。

### Phase 4 交付物
- [ ] 内存管理: malloc, free, 内存池
- [ ] 垃圾回收: 引用计数、根标记、可达性分析
- [ ] 异常处理: try/catch/finally, SEH
- [ ] 线程支持: 创建、同步、TLS
- [ ] 测试用例: 50+ 运行时功能编译成功

---

## Phase 5: 集成验证 (2-3 周)

### 目标
将 Phase 1-4 的所有功能集成，编译完整的自举编译器。

### 任务清单

#### 5.1 完整编译管线集成 (1 周)
**文件**: `PhotonPipeline.aura`, `PhotonDriver.aura`

**功能**:
- 整合所有编译阶段
- 错误处理和状态报告
- 性能优化

**验证标准**:
```bash
aura build -b photon Main.aura --output aura-photon.exe
# 成功生成 aura-photon.exe
```

#### 5.2 功能验证 (1 周)
**测试用例**:
- [ ] 编译简单程序 (Hello World)
- [ ] 编译中等复杂度程序 (100+ 函数)
- [ ] 编译复杂程序 (1000+ 函数)
- [ ] 编译自举编译器 (2153 函数)

**验证标准**:
```bash
# 测试用例 1
aura build -b photon test1.aura --output test1.exe
test1.exe  # 输出正确

# 测试用例 2
aura build -b photon test2.aura --output test2.exe
test2.exe  # 输出正确

# 测试用例 3
aura build -b photon Main.aura --output aura-photon.exe
aura-photon.exe  # 能正常运行
```

#### 5.3 性能基准 (2-3 天)
**测试项目**:
- 编译时间
- 二进制大小
- 运行性能

**对比**:
| 指标 | LLVM 后端 | Photon 后端 | 目标 |
|------|-----------|-------------|------|
| 编译时间 | 10s | 30s | < 60s |
| 二进制大小 | 200KB | 500KB | < 1MB |
| 运行性能 | 100% | 80% | > 70% |

**验证标准**:
```bash
# 编译时间
time aura build -b photon Main.aura --output aura-photon.exe
# 应 < 60s

# 二进制大小
ls -l aura-photon.exe
# 应 < 1MB

# 运行性能
aura-photon.exe --bench
# 应 > 70% of LLVM
```

### Phase 5 交付物
- [ ] 完整编译管线
- [ ] 功能验证报告
- [ ] 性能基准报告
- [ ] 编译 Main.aura 成功

---

## Phase 6: 自举验证 (2-3 周)

### 目标
用 Photon 编译产物重新编译自身，实现自举。

### 任务清单

#### 6.1 自举编译 (1 周)
**流程**:
```
aura.exe (LLVM 编译)
  ↓ 编译
Main.aura → aura-photon.exe (Photon 编译)
  ↓ 编译
Main.aura → aura-photon2.exe (Photon2 编译)
  ↓ 验证
aura-photon.exe == aura-photon2.exe ?
```

**命令**:
```bash
# 第 1 次: 用 LLVM 编译自举编译器
aura build -b llvm Main.aura --output aura-llvm.exe

# 第 2 次: 用 Photon 编译自举编译器
aura-llvm.exe build -b photon Main.aura --output aura-photon.exe

# 第 3 次: 用 Photon 编译产物重新编译
aura-photon.exe build -b photon Main.aura --output aura-photon2.exe

# 验证
cmp aura-photon.exe aura-photon2.exe
```

**验证标准**:
```bash
# 两个文件应完全相同
cmp aura-photon.exe aura-photon2.exe
# 无输出表示成功
```

#### 6.2 行为一致性验证 (1 周)
**测试用例**:
- [ ] 编译简单程序
- [ ] 编译中等复杂度程序
- [ ] 编译自举编译器
- [ ] 运行编译产物

**验证标准**:
```bash
# 测试 1: 编译 hello world
aura-llvm.exe build -b photon hello.aura --output hello-llvm.exe
aura-photon.exe build -b photon hello.aura --output hello-photon.exe
hello-llvm.exe  # 输出 "Hello"
hello-photon.exe  # 输出 "Hello"

# 测试 2: 编译自举编译器
aura-photon.exe build -b photon Main.aura --output aura-photon3.exe
cmp aura-photon.exe aura-photon3.exe  # 应完全相同
```

#### 6.3 发布准备 (2-3 天)
**任务**:
- [ ] 文档更新
- [ ] 测试套件完善
- [ ] 性能优化
- [ ] Bug 修复

**验证标准**:
```bash
# 完整测试套件
scripts\run-all-tests.ps1
# 所有测试通过

# 自举验证
scripts\bootstrap-verify.ps1
# 自举验证成功
```

### Phase 6 交付物
- [ ] 自举编译成功
- [ ] 行为一致性验证通过
- [ ] 文档更新
- [ ] 测试套件完善

---

## 风险评估与缓解

### 高风险项

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 代码生成复杂性超出预期 | 高 | 高 | 分阶段实现，逐步验证 |
| 标准库实现工作量过大 | 中 | 高 | 优先实现核心功能 |
| 运行时系统调试困难 | 高 | 中 | 使用调试器，逐步完善 |
| 性能不达标 | 中 | 中 | 优化关键路径 |

### 中风险项

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| HIR 序列化格式设计不当 | 中 | 中 | 使用成熟格式 (JSON) |
| 跨平台支持增加工作量 | 高 | 中 | 优先支持 Windows |
| 社区支持不足 | 低 | 中 | 内部团队独立完成 |

### 低风险项

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 工具链问题 | 低 | 低 | 使用稳定版本 |
| 文档不完整 | 中 | 低 | 边开发边文档化 |

---

## 资源需求

### 人力
- **编译器工程师**: 2-3 人 (全职)
- **测试工程师**: 1 人 (兼职)
- **项目经理**: 1 人 (兼职)

### 工具
- **IDE**: VS Code / IntelliJ IDEA
- **调试器**: GDB / LLDB
- **性能分析**: perf / VTune
- **版本控制**: Git

### 环境
- **开发环境**: Windows 10/11, WSL2
- **构建工具**: Cargo, Make, CMake
- **链接器**: lld-link, ld.lld

---

## 里程碑计划

### 里程碑 1: 基础设施完成 (第 3 周)
- [ ] `cmd_build_photon` 完整实现
- [ ] `PhotonDriver.aura` 通用驱动
- [ ] 10+ 简单程序编译成功

### 里程碑 2: 核心代码生成完成 (第 9 周)
- [ ] 控制流支持
- [ ] 内存管理
- [ ] 类型系统
- [ ] 50+ 中等复杂度程序编译成功

### 里程碑 3: 原生函数支持完成 (第 15 周)
- [ ] @native 函数调用支持
- [ ] 系统调用编号映射
- [ ] 内存管理接口
- [ ] 进程和线程接口
- [ ] 100+ 标准库函数编译成功

### 里程碑 4: 运行时完成 (第 19 周)
- [ ] 内存管理
- [ ] 垃圾回收
- [ ] 异常处理
- [ ] 线程支持
- [ ] 50+ 运行时功能编译成功

### 里程碑 5: 集成验证完成 (第 22 周)
- [ ] 编译 Main.aura 成功
- [ ] 性能基准达标
- [ ] 功能验证通过

### 里程碑 6: 自举验证完成 (第 25 周)
- [ ] 自举编译成功
- [ ] 行为一致性验证通过
- [ ] 文档和测试完善

---

## 成功标准

### 功能标准
- [ ] `aura build -b photon Main.aura --output aura-photon.exe` 成功
- [ ] `aura-photon.exe build -b photon Main.aura --output aura-photon2.exe` 成功
- [ ] `cmp aura-photon.exe aura-photon2.exe` 无差异

### 性能标准
- [ ] 编译时间 < 60s (Main.aura)
- [ ] 二进制大小 < 1MB
- [ ] 运行性能 > 70% of LLVM

### 质量标准
- [ ] 代码覆盖率 > 80%
- [ ] Bug 数 < 10 (严重)
- [ ] 文档完整

---

## 附录

### A. 文件结构
```
aura/
├── compiler/
│   └── aura/lang/compiler/backend/photon/
│       ├── PhotonPipeline.aura
│       ├── PhotonDriver.aura
│       ├── HirSerializer.aura
│       ├── MachineDag.aura
│       ├── InstructionSelection.aura
│       ├── RegisterAllocator.aura
│       ├── X86Emitter.aura
│       ├── PhotonObjectWriter.aura
│       ├── PhotonSystemLinker.aura
│       └── ...
├── core/
│   └── aura/lang/
│       ├── std/
│       │   ├── String.aura
│       │   ├── File.aura
│       │   ├── Process.aura
│       │   ├── List.aura
│       │   ├── Map.aura
│       │   └── Math.aura
│       └── runtime/
│           ├── Memory.aura
│           ├── GC.aura
│           ├── Exception.aura
│           └── Thread.aura
├── scripts/
│   ├── build-photon-full.ps1
│   ├── bootstrap-verify.ps1
│   └── run-all-tests.ps1
└── tests/
    └── photon/
        ├── S1/
        ├── S2/
        ├── S3/
        └── S4/
```

### B. 命令参考
```bash
# 编译单个文件
aura build -b photon test.aura --output test.exe

# 编译自举编译器
aura build -b photon Main.aura --output aura-photon.exe

# 自举验证
aura-photon.exe build -b photon Main.aura --output aura-photon2.exe
cmp aura-photon.exe aura-photon2.exe

# 运行测试
scripts\run-all-tests.ps1
```

### C. 参考资源
- [LLVM 文档](https://llvm.org/docs/)
- [x86_64 调用约定](https://docs.microsoft.com/en-us/cpp/build/x64-calling-convention)
- [Windows SEH](https://docs.microsoft.com/en-us/windows/win32/seh/)
- [COFF 格式](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format)

---

**文档版本**: 1.0

**最后更新**: 2024-01-01

**作者**: Aura 编译器团队