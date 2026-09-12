# 分阶段实施计划

> **最后更新**：Phase S0 语法定义与 IR 发射逻辑已完成，parser/emitter 扩展待实现。

---

## Phase S0：语法与发射器（3-5 天）

### 目标
新增 `@native` 和 `extern object` 语法，完成 IR 翻译。

### 任务

| # | 任务 | 涉及文件 | 状态 |
|---|------|---------|------|
| S0.1 | 新增 `@native` 语法节点（AST/Parser） | `compiler/src/ast.rs` + `parser.rs`（**或** Aura 侧 `parser/Parser.aura`） | ⚠️ 部分（FfiSyntax.aura 已创建，parser 待扩展） |
| S0.2 | 新增 `extern object` 语法节点 | 同上 | ⚠️ 部分（FfiSyntax.aura 已创建，parser 待扩展） |
| S0.3 | 指针类型 `*T`（如未支持） | sema / Type | ❌ 未开始 |
| S0.4 | 指针算术 / 解引用 IR 翻译 | `aura/.../aot/Emit.aura`（或 `compiler/src/codegen/aot/emit.rs`） | ❌ 未开始 |
| S0.5 | `@native(N)` → inline asm syscall | 同上 | ✅ 完成（FfiEmit.aura） |
| S0.6 | `@native`（无参数）→ load/store | 同上 | ✅ 完成（FfiEmit.aura） |
| S0.7 | `@native(asm = "...")` → inline asm | 同上 | ✅ 完成（FfiEmit.aura） |
| S0.8 | `@export` → define external | 同上 | ✅ 完成（FfiEmit.aura） |
| S0.9 | 测试：`tests/phase_s0_native_syntax_tests.aura` | `tests/` | ✅ 完成（待 parser 支持） |

### 已完成文件

| 文件 | 位置 | 说明 |
|---|---|---|
| `FfiSyntax.aura` | `aura/compiler/aura/lang/compiler/ffi/` | FFI 语法节点定义和辅助函数 |
| `FfiEmit.aura` | `aura/compiler/aura/lang/compiler/aot/` | FFI IR 发射逻辑 |
| `phase_s0_syntax_tests.aura` | `tests/pure_aura_cffi/` | Phase S0 语法测试 |

### 待完成

1. **Lexer 扩展**：添加 `@`、`native`、`export`、`extern` 关键词
2. **AST 扩展**：添加 `ExternObject`、`NativeMethod`、`ExportMethod` 节点类型
3. **Parser 扩展**：添加 `parseExternObject`、`parseNativeMethod`、`parseExportMethod` 方法
4. **Emitter 扩展**：调用 `FfiEmit.aura` 处理 FFI 节点
5. **TypeMapper 扩展**：添加 `CString`、`Long`、`Byte` 等类型映射

### 验证标准
- `@native(1) fun write(...)` 能正确翻译为 inline asm ❌
- `@native fun read(addr: Long): Byte` 能正确翻译为 `load i8` ❌
- `@export fun malloc(...)` 能正确翻译为 `define external` ❌
- 单元测试通过 ❌

---

## Phase S1：最小运行库（1 周）

### 目标
实现最小运行库，Hello World 无 libc 跑通。

### 任务

| # | 任务 | 涉及文件 | 状态 |
|---|------|---------|------|
| S1.1 | `Syscalls.aura`（x86_64 Linux） | `aura/core/aura/lang/native/Syscalls.aura` | ✅ 完成 |
| S1.2 | `Memory.aura`（内存操作） | `aura/core/aura/lang/native/Memory.aura` | ✅ 完成 |
| S1.3 | `Cpu.aura`（CPU 级操作） | `aura/core/aura/lang/native/Cpu.aura` | ✅ 完成 |
| S1.4 | 内存分配器（bump allocator） | `aura/core/aura/lang/native/memory/Allocator.aura` | ❌ 未开始 |
| S1.5 | 字符串操作 | `aura/core/aura/lang/native/string/StrOps.aura` | ❌ 未开始 |
| S1.6 | Console（`Console.println`） | `aura/core/aura/lang/native/console/Console.aura` | ❌ 未开始 |
| S1.7 | `Runtime.aura` 入口聚合 | `aura/core/aura/lang/native/Runtime.aura` | ❌ 未开始 |
| S1.8 | 测试：`tests/runtime/allocator_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S1.9 | 测试：`tests/runtime/string_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S1.10 | 测试：`tests/runtime/console_tests.aura` | `tests/runtime/` | ❌ 未开始 |

### 已完成文件

| 文件 | 位置 | 说明 |
|---|---|---|
| `Syscalls.aura` | `aura/core/aura/lang/native/` | x86_64 Linux 系统调用声明（含常量定义） |
| `Memory.aura` | `aura/core/aura/lang/native/` | 编译器内置内存操作声明 |
| `Cpu.aura` | `aura/core/aura/lang/native/` | CPU 级操作（内联汇编）声明 |

### 验证标准
- `Console.println("Hello, Aura!")` 能正确输出 ❌
- `Allocator.malloc(100)` 能正确分配内存 ❌
- `StrOps.strlen("hello")` 返回 5 ❌
- Hello World 程序无 libc 跑通 ❌

### 示例测试代码

```aura
// tests/runtime/allocator_tests.aura
fun testMalloc(): Boolean {
    val addr: Long = Allocator.malloc(100)
    if (addr == 0) { return false }
    Memory.write(addr, 0x42 as Byte)
    val v: Byte = Memory.read(addr)
    Allocator.free(addr)
    return v == 0x42 as Byte
}

// tests/runtime/string_tests.aura
fun testStrlen(): Boolean {
    val s: String = "hello"
    val n: Long = StrOps.strlen(s as CString)
    return n == 5 as Long
}

// tests/runtime/console_tests.aura
fun testPrintln(): Boolean {
    Console.println("Hello, Aura!")
    return true
}
```

---

## Phase S2：完整运行库（2 周）

### 目标
实现完整运行库，覆盖文件 I/O、进程、时间、随机、数学等。

### 任务

| # | 任务 | 涉及文件 | 状态 |
|---|------|---------|------|
| S2.1 | 文件 I/O | `aura/core/aura/lang/native/file/FileOps.aura` | ❌ 未开始 |
| S2.2 | 进程管理 | `aura/core/aura/lang/native/process/ProcessOps.aura` | ❌ 未开始 |
| S2.3 | 时间 | `aura/core/aura/lang/native/time/Clock.aura` | ❌ 未开始 |
| S2.4 | 随机数 | `aura/core/aura/lang/native/random/XorShift.aura` | ❌ 未开始 |
| S2.5 | 数学 | `aura/core/aura/lang/native/math/MathCore.aura` | ❌ 未开始 |
| S2.6 | Plan A 装箱 | `aura/core/aura/lang/native/boxed/PlanA.aura` | ❌ 未开始 |
| S2.7 | 测试：`tests/runtime/file_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S2.8 | 测试：`tests/runtime/process_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S2.9 | 测试：`tests/runtime/time_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S2.10 | 测试：`tests/runtime/random_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S2.11 | 测试：`tests/runtime/math_tests.aura` | `tests/runtime/` | ❌ 未开始 |
| S2.12 | 测试：`tests/runtime/boxed_tests.aura` | `tests/runtime/` | ❌ 未开始 |

### 验证标准
- 文件读写正常 ❌
- 进程创建/退出正常 ❌
- 时间获取正常 ❌
- 随机数生成正常 ❌
- 数学函数精度可接受 ❌

---

## Phase S3：跨平台（2 周）

### 目标
支持 Windows、macOS、aarch64 平台。

### 任务

| # | 任务 | 涉及文件 | 状态 |
|---|------|---------|------|
| S3.1 | Windows syscall 表 | `aura/core/aura/lang/native/arch/x86_64_windows/Syscalls.aura` | ❌ 未开始 |
| S3.2 | macOS syscall 表 | `aura/core/aura/lang/native/arch/x86_64_darwin/Syscalls.aura` | ❌ 未开始 |
| S3.3 | aarch64 Linux syscall 表 | `aura/core/aura/lang/native/arch/aarch64_linux/Syscalls.aura` | ❌ 未开始 |
| S3.4 | aarch64 macOS syscall 表 | `aura/core/aura/lang/native/arch/aarch64_darwin/Syscalls.aura` | ❌ 未开始 |
| S3.5 | CRT startup（可选） | `aura/core/aura/lang/native/startup/Start.aura` | ❌ 未开始 |
| S3.6 | 平台特性（Windows `NtWriteVirtualMemory` 等） | 各平台 `Syscalls.aura` | ❌ 未开始 |
| S3.7 | 跨平台测试 | `tests/runtime/cross_platform_tests.aura` | ❌ 未开始 |

### 验证标准
- Windows 上 `Console.println` 正常 ❌
- macOS 上 `Console.println` 正常 ❌
- aarch64 Linux 上 `Console.println` 正常 ❌
- 跨平台测试全部通过 ❌

---

## Phase S4：删除 C 源码（1 天）

### 目标
删除项目中的 C 源码，CI 加门禁。

### 任务

| # | 任务 | 涉及文件 | 状态 |
|---|------|---------|------|
| S4.1 | 删除 `compiler/src/std/cffi/aura_std_cffi.c` | - | ❌ 未开始 |
| S4.2 | 删除 `compiler/src/std/cffi/aura_std_cffi.h` | - | ❌ 未开始 |
| S4.3 | 删除 `examples/ext_ffi_demo/demo_cffi/utils.h` | - | ❌ 未开始 |
| S4.4 | 删除 `build/test_export.c` | - | ❌ 未开始 |
| S4.5 | 更新 CI 配置，加 C 源检查门禁 | `.github/workflows/ci.yml` | ❌ 未开始 |
| S4.6 | 更新文档，说明新的运行库架构 | `docs/aura_cffi_impl/README.md` | ✅ 完成 |

### 验证标准
- `git ls-files '*.c' '*.h' '*.cpp' '*.cc' | grep -v node_modules` 返回空 ❌
- CI 门禁通过 ❌
- 所有测试通过 ❌

### CI 门禁示例

```yaml
# .github/workflows/ci.yml
- name: Verify no C source in project
  run: |
    if git ls-files '*.c' '*.h' '*.cpp' '*.cc' '*.hpp' '*.hh' | grep -v '^ide-extension/' | grep -v '^node_modules/' | grep -q .; then
      echo "ERROR: C source files found in project"
      git ls-files '*.c' '*.h' '*.cpp' '*.cc' '*.hpp' '*.hh' | grep -v '^ide-extension/' | grep -v '^node_modules/'
      exit 1
    fi
    echo "✅ No C source in project"
```

---

## Phase S5：自举（长期，可选）

### 目标
Aura 编译器自身用 Aura AOT 编译，完全无 Rust 参与。

### 任务

| # | 任务 | 涉及文件 | 状态 |
|---|------|---------|------|
| S5.1 | `Emit.aura` 补齐类方法 | `aura/compiler/aura/lang/compiler/aot/Emit.aura` | ❌ 未开始 |
| S5.2 | `Emit.aura` 补齐列表 / 集合 | 同上 | ❌ 未开始 |
| S5.3 | `Emit.aura` 补齐异常 | 同上 | ❌ 未开始 |
| S5.4 | `Emit.aura` 补齐闭包 | 同上 | ❌ 未开始 |
| S5.5 | 编译器自举（Aura 编译器编译自身） | - | ❌ 未开始 |

### 验证标准
- Aura 编译器自身用 Aura AOT 编译 ❌
- 完全无 Rust 参与 ❌
- 所有测试通过 ❌

---

## 进度总览

| Phase | 任务数 | 完成 | 部分 | 未开始 | 进度 |
|---|---|---|---|---|---|
| S0 语法与发射器 | 9 | 4 | 2 | 3 | 44% |
| S1 最小运行库 | 10 | 3 | 0 | 7 | 30% |
| S2 完整运行库 | 12 | 0 | 0 | 12 | 0% |
| S3 跨平台 | 7 | 0 | 0 | 7 | 0% |
| S4 删除 C 源码 | 6 | 1 | 0 | 5 | 17% |
| S5 自举 | 5 | 0 | 0 | 5 | 0% |
| **总计** | **49** | **8** | **2** | **39** | **16%** |

### 状态图例

- ✅ 完成 - 任务已完成，可验证
- ⚠️ 部分 - 部分完成，需继续
- ❌ 未开始 - 尚未开始

---

## 风险与缓解

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| 运行库 Aura 源码无法被现有 AOT 后端编译 | 中 | 高 | 用 Rust AOT 后端（特性更全）过渡 |
| 符号名不一致导致链接失败 | 中 | 高 | 差分测试符号表 |
| Plan A 装箱语义有 bug | 中 | 高 | 逐函数迁移 + 逐函数测试 |
| 指针类型 / 解引用 IR 翻译错 | 中 | 中 | 单元测试覆盖每种指针操作 |
| libc 符号缺失（平台差异） | 低 | 中 | 按平台分类声明 |
| 编译器自举失败（Phase S5） | 高 | 低 | 不阻塞 S0-S4；S5 可独立推进 |

---

## 工作量评估

| 模块 | 行数（估算） | 难度 | 状态 |
|---|---|---|---|
| `@native` 语法 + IR 翻译 | 200-400 | 中 | ✅ IR 完成，语法待扩展 |
| `extern object` 语法 + IR 翻译 | 100-200 | 低 | ⚠️ IR 完成，语法待扩展 |
| `@export` 语法 + IR 翻译 | 100-200 | 低 | ✅ 完成 |
| 内存分配器（bump） | 100-200 | 低 | ❌ 未开始 |
| 内存分配器（dlmalloc 移植） | 500-1000 | 中 | ❌ 未开始 |
| 字符串 / 内存操作 | 200-300 | 低 | ❌ 未开始 |
| IO / 文件 / 进程 | 300-500 | 低 | ❌ 未开始 |
| 数学（sin/cos/pow/sqrt） | 300-500 | 中 | ❌ 未开始 |
| 随机数 | 50-100 | 低 | ❌ 未开始 |
| 时间 / 时钟 | 100-200 | 低 | ❌ 未开始 |
| CRT startup（可选） | 100-200 | 中 | ❌ 未开始 |
| 跨平台 syscall 表 | 200-300/平台 | 低 | ❌ 未开始 |
| **合计** | **2500-5000** | | **已用 ~500 行** |

**对比**：原 `aura_std_cffi.c` 77 KB ≈ 2500 行 C。Aura 版规模相当。

---

## 决策点

1. **内存地址类型**：用 `Long`（64 位整数）表示地址，还是新增 `MemAddr` 类型？  
   - 推荐：`Long`（与 `ptrToInt`/`intToPtr` 一致）。

2. **CString vs String**：运行库内部用 `CString`（C 风格）还是 `String`（Aura 风格）？  
   - 推荐：内部用 `CString`（与 syscall 接口一致），对外用 `String`。

3. **数学库精度**：Taylor 级数（精度中等）还是查表（快但精度低）？  
   - 推荐：Taylor 级数（无 libc 前提下这是唯一选择）。

4. **内存分配器**：bump（快但简单）还是 dlmalloc（慢但通用）？  
   - 推荐：先 bump，Phase S2 评估是否需要升级。

5. **跨平台优先级**：x86_64 Linux → Windows → macOS → aarch64？  
   - 推荐：按使用频率排序。

6. **是否做 Phase S5（编译器自举）**：是长期目标还是搁置？  
   - 推荐：S0-S4 做完后评估，S5 可独立开 Issue。

---

## 下一步行动

1. **扩展 Lexer**：添加 `@`、`native`、`export`、`extern` 关键词（预计 1-2 天）
2. **扩展 AST**：添加 `ExternObject`、`NativeMethod`、`ExportMethod` 节点类型（预计 1 天）
3. **扩展 Parser**：添加 `parseExternObject`、`parseNativeMethod`、`parseExportMethod` 方法（预计 2-3 天）
4. **扩展 Emitter**：调用 `FfiEmit.aura` 处理 FFI 节点（预计 1-2 天）
5. **扩展 TypeMapper**：添加 `CString`、`Long`、`Byte` 等类型映射（预计 0.5 天）
6. **实现 Phase S1**：内存分配器、字符串操作、Console（预计 1 周）
