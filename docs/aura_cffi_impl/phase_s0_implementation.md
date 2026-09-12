# Phase S0 实现文档

> 本文件记录 Phase S0 的实现进度和待完成的工作。

---

## 已完成

### 1. FFI 语法定义

**文件**：`aura/compiler/aura/lang/compiler/ffi/FfiSyntax.aura`

定义了 FFI 相关的 AST 节点种类常量和辅助函数：
- `AST_EXTERN_OBJECT` - extern object 节点
- `AST_NATIVE_METHOD` - @native 方法节点
- `AST_EXPORT_METHOD` - @export 方法节点
- 辅助函数：`ffiIsNode`, `ffiParseSyscallNum`, `ffiParseAsmCode`, `ffiParseLibcSymbol` 等

### 2. FFI IR 发射逻辑

**文件**：`aura/compiler/aura/lang/compiler/aot/FfiEmit.aura`

实现了 FFI 语法到 LLVM IR 的翻译逻辑：
- `ffiEmitSyscall` - 发射 @native(N) 系统调用函数（inline asm）
- `ffiEmitBuiltinRead` - 发射 @native 内存读取函数（load 指令）
- `ffiEmitBuiltinWrite` - 发射 @native 内存写入函数（store 指令）
- `ffiEmitBuiltinMemCopy` - 发射 @native memcpy 函数（LLVM intrinsic）
- `ffiEmitBuiltinMemSet` - 发射 @native memset 函数（LLVM intrinsic）
- `ffiEmitAsm` - 发射 @native(asm = "...") 内联汇编函数
- `ffiEmitExport` - 发射 @export 函数定义
- `ffiEmitAll` - 批量发射 FFI 函数定义

### 3. 运行时核心声明

**文件**：`aura/core/aura/lang/ffi/`

创建了三个核心声明文件：

#### Syscalls.aura
x86_64 Linux 系统调用声明：
- `read`, `write`, `open`, `close`, `fstat`, `lseek`
- `mmap`, `munmap`, `access`, `unlink`
- `execve`, `exitGroup`, `wait4`
- `clockGettime`, `getrandom`

#### Memory.aura
编译器内置内存操作：
- `read`, `read16`, `read32`, `read64`
- `write`, `write16`, `write32`, `write64`
- `copy`, `set`, `alloc`, `free`

#### Cpu.aura
CPU 级操作（内联汇编）：
- `rdtsc` - CPU 时间戳
- `memFence` - 内存屏障
- `cpuid` - CPU 信息
- `atomicAdd` - 原子加法

### 4. 测试文件

**文件**：`tests/phase_s0_syntax_tests.aura`

创建了 Phase S0 语法测试文件，展示预期行为。

---

## 待完成

### 1. Lexer 扩展

**文件**：`aura/compiler/aura/lang/compiler/lexer/Lexer.aura`

需要添加的关键词和符号：
- `@` - 注解前缀
- `native` - @native 标注
- `export` - @export 标注
- `extern` - extern object 关键词

**修改点**：
1. 在关键词表中添加 `native`, `export`, `extern`
2. 添加 `@` 符号的识别逻辑
3. 确保 `@native(1)`, `@native(asm = "...")`, `@export` 等语法能被正确词法分析

### 2. AST 扩展

**文件**：`aura/compiler/aura/lang/compiler/ast/Ast.aura`

需要添加的节点类型：
- `ExternObject` - extern object 容器节点
- `NativeMethod` - @native 方法节点
- `ExportMethod` - @export 方法节点

**修改点**：
1. 在 AST 节点种类中添加新类型
2. 定义节点的字段结构（名称、参数、返回类型、native 类型、实现体等）

### 3. Parser 扩展

**文件**：`aura/compiler/aura/lang/compiler/parser/Parser.aura`

需要添加的解析逻辑：
- 解析 `extern object Name { ... }` 语法
- 解析 `@native(N) fun name(...)` 语法
- 解析 `@native(asm = "...") fun name(...)` 语法
- 解析 `@export fun name(...) { ... }` 语法

**修改点**：
1. 添加 `parseExternObject` 方法
2. 添加 `parseNativeMethod` 方法
3. 添加 `parseExportMethod` 方法
4. 在主解析循环中处理新的声明类型

### 4. Emitter 扩展

**文件**：`aura/compiler/aura/lang/compiler/aot/Emit.aura`

需要添加的发射逻辑：
- 调用 `FfiEmit.aura` 中的函数发射 FFI 节点
- 在 `emitProgram` 中处理 FFI 节点

**修改点**：
1. 在 `emitProgram` 中添加 FFI 节点的处理分支
2. 导入 `FfiEmit` 模块
3. 将 FFI 函数的 LLVM IR 添加到输出中

### 5. 类型映射扩展

**文件**：`aura/compiler/aura/lang/compiler/aot/TypeMapper.aura`

需要支持的新类型：
- `CString` → `i8*` (ptr)
- `Long` → `i64`
- `Byte` → `i8`
- `Short` → `i16`
- `Int` → `i32`
- `Float` → `f32`
- `Double` → `f64`
- `Boolean` → `i1`

**修改点**：
1. 在类型映射表中添加新类型
2. 确保 FFI 函数的参数和返回类型能被正确映射

---

## 实施顺序建议

1. **第一步**：扩展 Lexer（添加关键词和符号）
2. **第二步**：扩展 AST（添加节点类型）
3. **第三步**：扩展 Parser（添加解析逻辑）
4. **第四步**：扩展 TypeMapper（添加类型映射）
5. **第五步**：扩展 Emitter（调用 FfiEmit）
6. **第六步**：运行测试验证

---

## 验证方法

### 词法测试
```bash
aura tokens tests/phase_s0_syntax_tests.aura
```

### 语法测试
```bash
aura ast tests/phase_s0_syntax_tests.aura
```

### AOT 测试
```bash
aura build tests/phase_s0_syntax_tests.aura --aot -o build/bin/phase_s0.exe
build/bin/phase_s0.exe
```

### 差分测试
```bash
# 对比 Rust 编译器和 Aura 编译器的输出
aura build tests/phase_s0_syntax_tests.aura --aot -o build/bin/phase_s0_aura.exe
cargo build --release -p compiler && target/release/aura build tests/phase_s0_syntax_tests.aura --aot -o build/bin/phase_s0_rust.exe
```

---

## 风险与注意事项

1. **VM 兼容性**：新的 AST 节点需要在 VM 中支持，可能需要修改 VM 解释器
2. **符号冲突**：FFI 函数名需要全局唯一，避免与现有函数冲突
3. **平台差异**：syscall 号是平台特定的，需要为每个平台维护单独的 Syscalls.aura
4. **内存安全**：裸内存操作需要仔细测试，避免越界访问

---

## 下一步

1. 开始扩展 Lexer（预计 1-2 天）
2. 开始扩展 AST（预计 1 天）
3. 开始扩展 Parser（预计 2-3 天）
4. 开始扩展 Emitter（预计 1-2 天）
5. 运行测试并修复问题（预计 2-3 天）

**总计**：7-11 天
