# Photon 后端编译自举编译器 - 分析报告

## 当前状态

### ✅ Photon 后端已验证功能
1. **Hello World 编译** - 成功编译并运行
   - 输出: `hello world` (68 65 6C 6C 6F 20 77 6F 72 6C 64 0D 0A)
   - 目标文件: hello.obj (230 bytes) + aura_runtime.obj (343 bytes)
   - 可执行文件: hello.exe (1536 bytes)

2. **自举编译器前端编译** - 成功生成 HIR
   - 输入: `aura\compiler\aura\lang\compiler\Main.aura`
   - 输出: 2153 个函数的 HIR
   - HIR 文件: `build\photon-compiler`

### ❌ 当前限制
`aura build -b photon` 命令目前**仅完成前端**（解析+语义分析+HIR 生成），**不实际执行 Photon 后端管线**。

当前输出显示:
```
Photon 后端管线 (S1 阶段):
  Phase A: HIR → SSA MIR (SsaBuilder)
  Phase B: MIR → LIR (Lowering)
  Phase C: LIR → Machine DAG (InstructionSelection)
  Phase D: Register Allocation + Peephole
  Phase E: X86 Encoding → COFF → Link → Executable

注意: Photon 后端管线在 Aura 编译器中实现，
完整 AOT 编译请使用: aura run tests/photon/S1/07_pipeline_integration.aura
```

## 架构分析

### 当前架构
```
Rust 编译器 (前端)
  ↓
  解析 → 语义分析 → HIR 生成
  ↓
  [停止] 仅输出 HIR + 提示消息
  ↓
  [手动] aura run PhotonHelloBuild.aura
  ↓
  Photon 后端 (Aura VM 执行)
  ↓
  COFF hex → .obj → lld-link → .exe
```

### 目标架构
```
Rust 编译器 (前端)
  ↓
  解析 → 语义分析 → HIR 生成
  ↓
  [自动] 调用 Photon 后端 (Aura VM)
  ↓
  HIR → SSA → LIR → DAG → RegAlloc → Encode → COFF
  ↓
  lld-link → .exe
  ↓
  [输出] 可执行文件
```

## 技术挑战

### 1. 编译器复杂度
Main.aura 包含 **2153 个函数**，远超当前 Photon 后端能力：
- 当前仅支持简单函数（hello world）
- 不支持复杂的控制流（循环、条件）
- 不支持类型系统（类、接口、泛型）
- 不支持内存管理（ARC、GC）

### 2. 标准库依赖
Main.aura 依赖大量标准库功能：
- String 操作（substring, indexOf, startsWith）
- 文件系统操作
- 进程管理
- 这些功能在 Photon 后端中尚未实现

### 3. 代码生成能力
当前 Photon 后端仅支持：
- 简单赋值
- 函数调用（println）
- 常量加载

需要支持：
- 复杂表达式
- 内存分配
- 指针操作
- 虚函数调用
- 异常处理

### 4. 链接器支持
当前仅支持：
- Windows COFF
- kernel32.lib

需要支持：
- 跨平台（Linux ELF, macOS Mach-O）
- 多模块链接
- 符号解析

## 实现路径

### 阶段 1: 基础设施 (2-4 周)
1. **修改 `cmd_build_photon`** - 集成 VM 执行 Photon 后端
2. **创建通用驱动** - 支持任意源码编译
3. **实现 HIR 序列化** - Rust HIR → Aura HIR 转换

### 阶段 2: 代码生成完善 (4-8 周)
1. **控制流支持** - 循环、条件、分支
2. **内存管理** - 栈分配、堆分配、ARC
3. **类型系统** - 类、接口、泛型
4. **调用约定** - 多参数、返回值、寄存器分配

### 阶段 3: 标准库实现 (4-8 周)
1. **String 操作** - substring, indexOf, startsWith
2. **文件系统** - 读写、目录操作
3. **进程管理** - exit, spawn
4. **运行时支持** - 内存管理、GC

### 阶段 4: 完整编译 (2-4 周)
1. **编译 Main.aura** - 生成可执行文件
2. **验证功能** - 对比 Rust 编译器行为
3. **自举验证** - 用编译产物重新编译自身

## 预估工作量
- **总计**: 12-24 周 (3-6 个月)
- **人力**: 1-2 名编译器工程师

## 替代方案

### 方案 A: 逐步完善 Photon 后端
- 优点: 完全自主控制，可定制优化
- 缺点: 工作量大，周期长

### 方案 B: 使用 LLVM 后端
- 优点: 功能完整，社区支持
- 缺点: 依赖外部工具，集成复杂

### 方案 C: 混合方案
- 前端用 Rust，后端用 LLVM
- 逐步迁移到 Photon 后端
- 目前采用此方案

## 建议
1. **短期**: 继续使用 LLVM 后端编译自举编译器
2. **中期**: 逐步完善 Photon 后端能力
3. **长期**: 用 Photon 后端编译自举编译器（自举验证）