# AOT 后端与技术方案一致性检查报告

> 日期：2026-09 · 范围：`技术方案.md` 第九章（LLVM 后端深度设计）vs `compiler/src/codegen/aot/`
> 方法：逐节对照 + llc/链接/运行三层验证

## 一、结论摘要

实现与设计**整体一致**，核心架构、类型映射、控制流、FFI、Runtime、目标平台抽象
均按 §9 落地。检查中发现并修复了 **4 个实现缺陷**（其中 3 个会导致生成非法 IR
或链接失败），并在过程中确认了若干有意的设计差异与 HIR 层简化边界。

## 二、一致性对照表

| 方案章节 | 设计要求 | 实现状态 | 验证 |
|---------|---------|---------|------|
| §9.1.2 整体架构 | HIR→LLVM IR→优化→目标码 | ✅ emit.rs 完整流水线 | llc 编译通过 |
| §9.2.1 模块结构 | LlvmCodeGenerator + 模块头/全局/函数/FFI 分步 | ✅ `mod.rs` + `emit.rs` 分步生成 | IR 结构正确 |
| §9.2.2 TypeMapper | Int→i32 等基本类型 | ✅ 全部正确映射 | 单元测试 10 项 |
| §9.2.2 Enum→tagged union | 枚举映射为 tag+union | ⚠️ HIR 层丢弃枚举（P4 边界） | 非 P6 缺陷 |
| §9.2.2 Function→函数指针 | 函数类型 | ⚠️ HIR 无函数类型（P4 边界） | 非 P6 缺陷 |
| §9.2.3 函数生成 | entry+参数+body+return | ✅ alloca 参数/body/兜底 return | 运行正确 |
| §9.2.4 控制流 if | then/else/merge + PHI | ✅ 修复后三块齐全 | llc 验证 |
| §9.2.4 控制流 loop | cond/body/end 三块 | ✅ loop.cond/body/end | 运行正确 |
| §9.3 FFI 声明 | extern→declare + C ABI | ✅ declare + 默认 ccc | 测试通过 |
| §9.3 FFI 常量 | Constant 声明 | ⚠️ HIR 层未保留（P4 边界） | 非 P6 缺陷 |
| §9.4.1 优化级别 | O0~Oz 六档 | ✅ OptimizationLevel 映射 `-O` | 测试通过 |
| §9.4.2 Pass 管理 | PassManager 进程内 | ⚠️ 委托 llc `-O`（选型差异） | 已记录 |
| §9.5.1 TargetTriple | arch/vendor/os/abi | ✅ 全实现 + from_str | 测试通过 |
| §9.5.2 目标码生成 | obj 生成 + 链接 | ✅ llc→obj + clang/lld 链接 | 运行验证 |
| §9.5.3 交叉编译 | sysroot/linker/c_stdlib | ✅ CrossCompilationConfig | aarch64 ELF 验证 |
| §9.6 Runtime 集成 | ARC/协程声明注入 | ✅ 8 个 runtime 函数 | 测试通过 |
| §9.7 构建系统 | Cargo + build.rs 检测 | ✅ llvm feature + build.rs | 版本检测 23.1.0 |
| §9.8 错误处理 | CodegenError 枚举 | ✅ AotError 全变体 | 测试通过 |

## 三、检查发现并修复的缺陷

### 1. `emit_if_stmt` 缺 merge 块（§9.2.4 不一致 → 非法 IR）

**现象**：`if (x > 3) { x = x + 1 }`（分支不 return）生成 `br label %bb.merge.N`
但未创建 merge 块 → llc 报 `use of undefined value '%bb.merge.N'`。

**修复**：`add_block_named(&merge_name)` 创建 merge 块，后续语句在 merge 中
继续生成（§9.2.4 的三块模型）。

**验证**：llc 编译通过；`aura build --aot` 运行返回正确值 6。

### 2. 字符串全局常量位置错误（§9.2.1 不一致 → 非法 IR）

**现象**：字符串 `private constant` 被 push 进函数体基本块，LLVM 要求全局量为
模块顶层实体 → llc 报 `expected instruction opcode`。

**连带缺陷**：全局常量使用 `%` 前缀（LLVM 全局必须 `@`）；`c"..."` 无隐式
NUL（需显式 `\00`），数组长度 = s.len()+1。

**修复**：`EmitCtx.globals` 收集 + `@str_data.N` 命名 + 去重 + 正确的长度计算。

**验证**：llc 编译通过，运行正确。

### 3. DWARF 元数据被注释掉（§9.5 未真正实现）

**现象**：`emit_subprogram` 结果以 `; ` 注释形式输出，从未成为真实 LLVM 元数据。

**修复**：DIFile/DICompileUnit(distinct)/DISubprogram(distinct)/DILocation 真实
输出；函数体首指令 `, !dbg !N` 关联；`!llvm.dbg.cu = !{!1}` 注册；CLI 加 `--debug`。

**验证**：llc 零警告编译；对象含 `.debug$S`（COFF DWARF）节。

### 4. Windows 大栈帧链接失败（§9.5.2 边界 → 链接错误）

**现象**：栈帧 >4KB 时 llc 生成对 `__chkstk` 的引用，`lld-link` 无 MSVC CRT
提供该符号 → `undefined symbol: __chkstk`。

**修复**：Windows 链接优先用 `clang`（自带 MSVC 运行库解析栈探测），
lld-link 降级回退。

**验证**：组合程序（递归+while+字符串，大栈帧）链接并运行返回 88。

## 四、有意的设计差异（非缺陷）

| 差异 | 方案 | 实现 | 理由 |
|------|------|------|------|
| LLVM 绑定 | inkwell（LLVM 19-） | 文本 IR + 外部 llc/clang/lld | inkwell 不支持 LLVM 23 |
| 优化 pass | 进程内 PassManager | llc `-O<N>` 参数 | 文本 IR 路线等价替代 |
| String 类型 | 裸 `i8*` | `{i8*, i64}` 结构（可配回 `i8*`） | 长度感知字符串（§4.3 需 const char* 转换） |
| 链接器 | write_to_memory_buffer | 外部工具链 | 文本 IR 路线 |

## 五、设计未覆盖项（HIR 层简化边界，P4 遗留）

这些在 `hir.rs` 降级阶段即被丢弃，LLVM 后端（P6）输入中没有，故无法映射：

| 方案特性 | HIR 现状 |
|---------|---------|
| Enum → tagged union | `Decl::Enum => {}`（丢弃） |
| Function 类型 → 函数指针 | HIR 无函数类型；lambda 降级为 `__lambda` 占位 |
| Pointer\<T\> | HIR 无指针变体 |
| FFI 常量（`FfiDeclaration::Constant`） | extern 降级仅保留函数 |

> 修复路径：P4 HIR 层补全这些结构后，P6 的 TypeMapper/emit 即可扩展映射。

## 六、验证命令

```bash
# 全量测试
cargo test --features llvm -p compiler

# 组合回归（递归+while+字符串+大栈帧）
aura build combo.aura --aot --output combo.exe && combo.exe   # 期望 88

# DWARF
aura build demo.aura --aot --debug --emit-llvm && llc demo.ll -filetype=obj -O0
llvm-readobj --sections demo.o | grep debug    # 期望 .debug$S

# aarch64 交叉编译
aura build demo.aura --aot --target aarch64-unknown-linux-gnu --emit-llvm
llc demo.ll -filetype=obj -mtriple=aarch64-unknown-linux-gnu  # ELF64-aarch64
```