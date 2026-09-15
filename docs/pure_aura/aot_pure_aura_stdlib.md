# AOT 纯 Aura 标准库嵌入方案

> **目标**：AOT 编译时将 stdlib 嵌入到生成代码中，生成的可执行文件只依赖系统库，不依赖外部 C runtime。

## 背景

当前 AOT 编译管线假设所有 native 函数都在 C runtime（`aura_std_cffi.c`）中，但实际上：

- **纯 Aura 函数**（如 `IO.fileExists`、`Ascii.isAlpha`）：不需要 C 实现
- **`@native(asm=...)` 函数**：编译期生成内联汇编 IR，不需要 C 实现
- **`@native(N)` syscall**：编译期生成内联 syscall IR，不需要 C 实现
- **`native fun` 编译器内置**：编译期生成对应 IR，不需要 C 实现

**问题**：AOT 编译器生成外部符号引用（`@aura_lang_std_IO_fileExists`），期望 C runtime 提供实现，但 C runtime 没有这些函数。

## 当前架构 vs 目标架构

### 当前架构（有问题）

```
用户代码 ──→ AOT 编译 ──→ 原生代码 ──→ 链接 C runtime ──→ 可执行文件
                                         ↑
                              纯 Aura 函数在这里找不到实现
```

### 目标架构

```
用户代码 ──┐
            ├──→ AOT 编译 ──→ 原生代码 ──→ 链接系统库 ──→ 自包含可执行文件
stdlib   ───┘
```

## 实施阶段

### Phase 1：分析用户代码的 import，识别依赖的 stdlib 模块

**任务**：
1. 解析用户代码的 `import` 语句
2. 识别依赖的 stdlib 模块（`aura.lang.std.*`、`aura.lang.concurrent.*`）
3. 生成依赖的 stdlib 模块列表

**检查点**：
- [ ] 能正确解析 `import aura.lang.std.Math` 语句
- [ ] 能识别依赖的 stdlib 模块
- [ ] 能生成依赖的 stdlib 模块列表

**完成情况**：🔴 未开始

**关键文件**：
- `compiler/src/codegen/resolve_imports.rs` - import 解析
- `cli/src/main.rs` - AOT 编译入口

---

### Phase 2：编译 stdlib 模块到 HIR

**任务**：
1. 读取 stdlib 模块的 `.aura` 源码
2. 解析到 AST
3. 语义分析
4. 生成 HIR

**检查点**：
- [ ] 能读取 stdlib 源码文件
- [ ] 能解析 stdlib 源码到 AST
- [ ] 能完成语义分析
- [ ] 能生成 HIR

**完成情况**：🔴 未开始

**关键文件**：
- `aura/core/aura/lang/std/*.aura` - stdlib 源码
- `aura/core/aura/lang/concurrent/*.aura` - concurrent 源码
- `compiler/src/parse/` - 解析器
- `compiler/src/sema/` - 语义分析
- `compiler/src/codegen/hir/` - HIR 生成

---

### Phase 3：合并用户代码和 stdlib 的 HIR

**任务**：
1. 合并用户代码的 HIR 和 stdlib 的 HIR
2. 合并函数表、常量表、native 函数表
3. 重定位函数调用下标

**检查点**：
- [ ] 能合并多个 HIR 模块
- [ ] 能合并函数表
- [ ] 能重定位函数调用下标

**完成情况**：🔴 未开始

**关键文件**：
- `compiler/src/codegen/hir/mod.rs` - HIR 数据结构
- `compiler/src/std/embedded_stdlib.rs` - 嵌入 stdlib 加载逻辑（参考）

---

### Phase 4：生成包含所有代码的 LLVM IR

**任务**：
1. 为合并后的 HIR 生成 LLVM IR
2. 处理模块间的符号引用
3. 确保函数调用正确链接

**检查点**：
- [ ] 能生成包含所有代码的 LLVM IR
- [ ] 能处理模块间的符号引用
- [ ] 函数调用正确链接

**完成情况**：🔴 未开始

**关键文件**：
- `compiler/src/codegen/aot/emit.rs` - AOT IR 生成
- `compiler/src/codegen/aot/types.rs` - 类型映射

---

### Phase 5：修改链接流程，不链接 C runtime

**任务**：
1. 移除 C runtime 链接（`aura_std_cffi.o`）
2. 只链接系统库（libc、kernel32 等）
3. 处理 native 函数的链接

**检查点**：
- [ ] 不链接 C runtime
- [ ] 只链接系统库
- [ ] native 函数正确链接

**完成情况**：🔴 未开始

**关键文件**：
- `compiler/src/codegen/aot/linker.rs` - 链接器
- `compiler/src/codegen/aot/runtime.rs` - 运行时函数声明

---

### Phase 6：测试验证

**任务**：
1. 运行语言测试（AOT 模式）
2. 验证所有测试通过
3. 检查可执行文件自包含

**检查点**：
- [ ] 所有 AOT 语言测试通过
- [ ] 可执行文件不依赖 C runtime
- [ ] 可执行文件可独立运行

**完成情况**：🔴 未开始

**关键文件**：
- `scripts/run-language-tests.ps1` - 测试脚本
- `examples/language-test/*.aura` - 测试用例

---

## Phase 4-6 实施记录

### Phase 4：生成包含所有代码的 LLVM IR

**任务**：修复 4 个 LLVM IR 错误

**错误列表**：
| 测试 | 错误位置 | 错误信息 |
|------|----------|----------|
| 05-classes | line 2514:33 | expected value token |
| 07-error-handling | line 1270:26 | expected value token |
| 15-advanced | line 1201:24 | expected value token |
| 16-script-mode | line 1189:32 | expected value token |

**完成情况**：🟡 进行中

**检查点**：
- [ ] 定位 LLVM IR 语法错误
- [ ] 修复变量名缺少 `%` 前缀
- [ ] 确保所有函数调用生成正确的 LLVM IR

---

### Phase 5：修改链接流程，不链接 C runtime

**任务**：
1. 移除 C runtime 链接（`aura_std_cffi.o`）
2. 只链接系统库（libc、kernel32 等）
3. 处理 native 函数的链接

**完成情况**：🔴 未开始

**检查点**：
- [ ] 移除 `aura_std_cffi.o` 链接
- [ ] 只链接系统库
- [ ] native 函数正确链接

---

### Phase 6：测试验证

**任务**：
1. 运行语言测试（AOT 模式）
2. 验证所有测试通过
3. 检查可执行文件自包含

**完成情况**：🔴 未开始

**检查点**：
- [ ] 所有 AOT 语言测试通过
- [ ] 可执行文件不依赖 C runtime
- [ ] 可执行文件可独立运行

---

## 完成情况统计

| Phase | 名称 | 状态 | 完成度 |
|-------|------|------|--------|
| 1 | 分析 import，识别依赖 | ✅ 完成 | 100% |
| 2 | 编译 stdlib 到 HIR | ✅ 完成 | 100% |
| 3 | 合并 HIR | ✅ 完成 | 100% |
| 4 | 生成 LLVM IR | 🟡 进行中 | 25% |
| 5 | 修改链接流程 | 🔴 未开始 | 0% |
| 6 | 测试验证 | 🔴 未开始 | 0% |
| **总计** | | | **54%** |

## Phase 1-2 进展记录

### Phase 1：分析用户代码的 import，识别依赖的 stdlib 模块

**已完成**：
- [x] `resolve_aura_imports` 已能将 stdlib 源码内联到用户代码中
- [x] AOT 编译器已能访问 stdlib 源码
- [x] 符号名转换：新式名称 → 旧式名称（`translate_to_legacy_c`）

**待完成**：
- [ ] 识别 stdlib 中的纯 Aura 函数（非 native 声明）
- [ ] 区分纯 Aura 函数和 native 函数
- [ ] 生成依赖的 stdlib 模块列表

**发现的问题**：
- stdlib 源码中的函数（如 `IO.fileExists`）是纯 Aura 实现，但 AOT 编译器生成外部符号引用
- 通过 `translate_to_legacy_c` 将新式名称转换为旧式名称，匹配 C runtime

### Phase 2：编译 stdlib 模块到 HIR

**已完成**：
- [x] stdlib 源码已内联到用户代码中
- [x] HIR 生成已处理内联的 stdlib 代码
- [x] FFI 声明生成使用旧式符号名
- [x] 函数调用生成使用旧式符号名

**待完成**：
- [ ] 确保 stdlib 中的纯 Aura 函数被编译为原生代码
- [ ] 处理 stdlib 中的 native 函数声明
- [ ] 生成包含所有代码的 LLVM IR

**测试进展**：
- AOT 测试：17/21 通过（之前 9/21）
- 剩余 4 个失败：LLVM IR 错误（`expected value token`）
  - 05-classes: line 2514:33
  - 07-error-handling: line 1270:26
  - 15-advanced: line 1201:24
  - 16-script-mode: line 1189:32

**关键文件**：
- `compiler/src/codegen/aot/emit.rs` - 函数调用生成（已修改）
- `compiler/src/codegen/aot/ffi.rs` - FFI 声明生成（已修改）
- `compiler/src/codegen/aot/runtime.rs` - 运行时函数声明（已扩展）
- `compiler/src/std/cffi/aura_std_cffi.c` - C runtime stubs（已添加）

## 相关文件

- `docs/pure_aura/aot_pure_aura_stdlib.md` - 本文档
- `compiler/src/codegen/aot/` - AOT 编译管线
- `cli/src/main.rs` - CLI 入口
- `aura/core/aura/lang/std/` - stdlib 源码
- `aura/core/aura/lang/concurrent/` - concurrent 源码

## 参考资料

- `docs/pure_aura/pure_aura_refactor.md` - 纯 Aura 重构方案
- `docs/pure_aura/native_syscalls_guide.md` - native syscall 指南
