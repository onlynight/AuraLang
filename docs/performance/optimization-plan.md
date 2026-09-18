# AuraLang 编译器编译速度优化方案

> 生成日期：2026-07-01
> 适用范围：Aura 自举编译器 + Rust 编译器

## 目录

1. [现状分析](#1-现状分析)
2. [Rust 编译器瓶颈](#2-rust-编译器瓶颈)
3. [Aura 自举编译器瓶颈](#3-aura-自举编译器瓶颈)
4. [Aura 编译器优化方案](#4-aura-编译器优化方案)
5. [Rust 编译器优化方案](#5-rust-编译器优化方案)
6. [统一路线图](#6-统一路线图)

---

## 1. 现状分析

### 1.1 两套编译器对比

| 层级 | Rust 编译器 | Aura 自举编译器 |
|------|-------------|-----------------|
| 源码目录 | `compiler/src/` + `cli/src/` | `aura/compiler/aura/lang/compiler/` |
| 前端 | Lexer → Parser → Sema | Parser (内置词法) → TypeChecker |
| 中端 | HIR → Mono → Inline/Fold → MIR → ARC | HirLowerer → Mono → Inline → Fold → MirLowerer |
| 后端(VM) | Codegen → BytecodeModule | Codegen → 扁平字符串字节码 |
| 后端(AOT) | LLVM IR → llc → clang | LLVM IR → Process.run(llc) → Process.run(clang) |
| 并发原语 | `std::thread`, `rayon`, `tokio` | `Thread.spawn`, `Channel`, `Coroutine`, `Future`, `Mutex` |
| 数据结构 | Rust Vec/HashMap (高效) | **扁平字符串 arena** (每行一项，换行分隔) |

### 1.2 编译流水线

```
Source → ImportResolve → Lex → Parse → Sema → 
  Desugar → Mono → Inline/Fold → MIR → ARC → Emit → 
  LLVM IR → llc(verify) → llc(compile) → clang(link)
```

---

## 2. Rust 编译器瓶颈

| # | 瓶颈 | 位置 | 问题 |
|---|------|------|------|
| R1 | `cmd_stdlib_compile` 顺序遍历 | cli/main.rs:726 | 所有 .aura 文件串行编译 |
| R2 | `link_to_object` 两次 llc 调用 | linker.rs:161-177 | verify 与 compile 串行 |
| R3 | `compile_std_cffi` 每次重编 C 文件 | linker.rs:254-294 | 不变的 C 文件重复编译 |
| R4 | `Command::output()` 阻塞 | linker.rs:299 | 外部进程同步等待 |
| R5 | pass 级串行遍历 | opt.rs, mono.rs, mir.rs | fold_hir/inline_hir/lower_program 顺序遍历函数 |
| R6 | EmitCtx 全局计数器 | aot/emit.rs:37-100 | bb_counter/var_counter 跨函数共享 |
| R7 | 无构建缓存 | 全局 | 每次从零编译 |
| R8 | 无 sccache | Cargo 配置 | 依赖重复编译 |

---

## 3. Aura 自举编译器瓶颈

### 3.1 架构性瓶颈：扁平字符串 arena

```aura
// Hir.aura: 节点 = 换行分隔的文本行
var kinds: String    // "HirProgram\nHirFunction\n..."
var texts: String    // "main\nadd\n..."
var kids: String     // "1,2,3\n4,5\n..."
```

每次操作都是 O(n) 字符串遍历/拼接。

### 3.2 具体热点

| 操作 | 位置 | 复杂度 | 原因 |
|------|------|--------|------|
| `lookupConst` | Codegen.aura:310 | O(n) | 线性扫描 constMap 字符串 |
| `lookupVar` | Codegen.aura:339 | O(n) | 线性扫描 varTable 字符串 |
| `resolveLabel` | Codegen.aura:134 | O(n) | 线性扫描 labelTable 字符串 |
| `hirKidsCount` | Hir.aura | O(n) | 遍历逗号分隔字符串 |
| `hirKidsAt` | Hir.aura | O(n) | 遍历逗号分隔字符串 |
| `substr` | 多处 | O(n) | 逐字符字符串拼接 |
| `bytecode = bytecode + line` | Codegen.aura:255 | O(n²) | 反复字符串拼接 |
| `Process.run()` | Aot.aura:145-188 | 阻塞 | 3 次串行外部进程调用 |
| `FileSystem.readText` | ModuleLink.aura:98 | 阻塞 | 递归同步文件读取 |

### 3.3 可用并发原语

| 原语 | 适用性 |
|------|--------|
| `Thread.spawn(fn_id, arg)` | 并行文件读取、并行编译 |
| `Channel.newChannel(cap)` | 流水线连接 |
| `Coroutine.spawn(body)` | I/O 重叠 |
| `Future.spawn(fn_id, arg)` | 并行 AOT 编译 |
| `Mutex` / `Atomic` | 共享状态保护 |
| `Thread.parallelism()` | 线程池大小 |

---

## 4. Aura 编译器优化方案

### P0 — 立即收益

#### A1. HashMap 替代线性扫描查找 ⭐

**文件**：`aura/compiler/aura/lang/compiler/codegen/Codegen.aura`

当前 `lookupConst` / `lookupVar` 是 O(n) 字符串扫描，每次发射指令都调用。改用 `HashMap<String, Int>` 实现 O(1) 查找。

```aura
import aura.lang.collection.HashMap

// 当前（O(n)）：
var constMap: String = ""
fun lookupConst(value: String): Int {
    var pos = 0
    while (pos < this.constMap.length) {
        if (toStr(this.constMap[pos]) == "\n") { /* 逐行扫描 */ }
        pos = pos + 1
    }
    return -1
}

// 优化后（O(1)）：
var constMap: HashMap<String, Int> = HashMap<String, Int>()
fun lookupConst(value: String): Int {
    val found = this.constMap.get(value)
    if (found != null) return found
    return -1
}
```

同理对 `lookupVar` / `resolveLabel` 做相同改造。

**预期收益**：编译快 2-5x

#### A2. StringBuilder 替代字符串拼接 ⭐

**文件**：`aura/compiler/aura/lang/compiler/codegen/Codegen.aura`

当前每次追加字节码行都是 `this.bytecode = this.bytecode + line + "\n"`，O(n²) 总复杂度。改用 StringBuilder。

```aura
import aura.lang.std.string.StringBuilder

// 当前（O(n²)）：
this.bytecode = this.bytecode + line + "\n"

// 优化后（O(n)）：
val builder: StringBuilder = StringBuilder()
builder.append(line)
builder.append("\n")
```

**预期收益**：大文件编译快 5-20x

#### A3. 合并 verify 到 llc 主命令

**文件**：`aura/compiler/aura/lang/compiler/aot/Aot.aura`

```aura
// 当前：两次 llc 调用
val verifyCmd = llc + " ... -verify-each"
Process.run(verifyCmd)
val llcCmd = llc + " ... -o " + objPath + " -filetype=obj"
Process.run(llcCmd)

// 优化：合并为一次
val llcCmd = llc + " ... -o " + objPath + " -filetype=obj -verify-each"
Process.run(llcCmd)
```

**预期收益**：AOT 快 10-20%

#### A4. 跳过 emitC

确认所有构建路径走 `compileAotIrOnlyEx`（设 `opts.emitCSource = false`），避免二次遍历 HIR 生成 C 文本。

#### A5. 源码哈希缓存

在 `.auc` / `.exe` 输出旁写入源码哈希，编译前检查，哈希匹配则跳过。

### P1 — 核心加速

#### B1. 并行模块链接

**文件**：`aura/compiler/aura/lang/compiler/aot/ModuleLink.aura`

用 `Future.spawn` 并行读取多个模块文件，用 `Channel` 传递解析结果。

#### B2. Pass 级并行

对 `fold_hir` / `inline_hir` / `lower_program` 使用 `Future.spawn` 并行处理各函数。

#### B3. Kids 计数预计算缓存

在 `Hir` 类中增加 `kidsCounts: List<Int>` 缓存，`hirKidsCount` 从 O(n) → O(1)。

### 3.4 Aura 编译器并行化详细方案

#### 3.4.1 并行模块链接（ModuleLink.aura）

**当前问题**：`ModuleLink.loadPath` 递归同步读取文件，编译器自身 40+ 模块依赖全部串行。

**方案**：两阶段并行——

```aura
// 阶段 1：收集所有依赖路径（快速扫描 import）
fun collectAllImports(entryPath: String): List<String>

// 阶段 2：并行读取 + 解析
fun loadParallel(paths: List<String>): List<Hir> {
    val numThreads = Thread.parallelism()
    // 将路径分成 numThreads 组，每组一个线程
    for (i in 0..<numThreads) {
        Thread.spawn(threadLoaderTask, batchStart)
    }
    // 等待所有线程完成，收集结果
}

// 阶段 3：顺序合并 HIR
fun mergeAll(inputs: List<Hir>): Hir
```

**收益**：40+ 模块的 I/O + 解析时间缩短到 **~1/core数** 倍。

#### 3.4.2 AOT 多文件并行编译

**当前问题**：`Process.run()` 是同步阻塞，多模块 AOT 编译完全串行。

**方案**：使用 `Thread.spawn` + `Channel` 并行执行多个 `Process.run` 调用。

```aura
fun parallelAotCompile(modules: List<String>, outDir: String): Unit {
    val resultCh = Channel.newChannel(modules.size)
    for (i in 0..<modules.size) {
        Thread.spawn(moduleCompileTask, modules[i])
    }
    // 等待所有完成
}
```

**收益**：多文件 AOT 编译快 **~cores 倍**。

#### 3.4.3 协程 I/O 重叠（Import 解析）

**方案**：使用协程 + Channel 实现生产者-消费者流水线，读取线程与解析线程重叠执行。

```aura
// 读取线程：不断读文件
fun readerTask(pathCh: Int, fileCh: Int): Unit { ... }
// 解析线程：不断解析文件  
fun parserTask(fileCh: Int, resultCh: Int): Unit { ... }
```

**收益**：I/O 等待与 CPU 计算重叠，**编译快 2-3x**。

#### 3.4.4 并行 HIR Pass 执行

| Pass | 文件 | 可并行 |
|------|------|--------|
| Fold (常量折叠) | `hir/Fold.aura` | ✅ 每个函数独立 |
| Inline (内联分析) | `hir/Inline.aura` | ✅ 可并行收集 |
| Mono (单态化) | `hir/Mono.aura` | ❌ 需全局调用图 |
| MirLower (HIR→MIR) | `mir/MirLower.aura` | ✅ 可并行降级 |
| ModuleLink | `aot/ModuleLink.aura` | ✅ 可并行读取 |

**收益**：函数密集型程序编译快 **3-6x**。

#### 3.4.5 并发原语使用指南

| 场景 | 推荐原语 | 理由 |
|------|---------|------|
| 并行文件读取 | `Thread.spawn` + `Channel` | I/O 密集，线程间无共享状态 |
| 并行编译模块 | `Thread.spawn` + `Channel` | 每个模块独立编译 |
| 并行 pass 执行 | `Thread.spawn` + `Channel` | 每个函数独立处理 |
| 并行 AOT 外部进程 | `Thread.spawn` + `Channel` | `Process.run` 阻塞，需线程 |
| I/O 重叠 | `Coroutine` + `Channel` | 同线程轻量级调度 |
| 共享计数器 | `Atomic` | 无锁状态更新 |
| 互斥保护 | `Mutex` | 共享数据保护 |

### P2 — 架构升级

#### C1. AOT 进程并行

对多文件 AOT 编译，使用 `Thread.spawn` + `Channel` 并行执行 `Process.run`。

#### C2. 协程 I/O 重叠

用协程实现文件读取与 CPU 计算的重叠。

#### C3. 并行 HIR 发射（Emit）

将 `Emit.aura` 中的 `emitProgram` 拆分为：全局初始化（模块头、全局变量）+ 函数级 IR 生成。函数级 IR 生成天然可并行。

---

## 5. Rust 编译器优化方案

### 5.1 Rust ↔ Aura 优化项对齐分析

| Rust 优化项 | 对 Aura 适用？ | 说明 |
|-------------|---------------|------|
| R1: 合并 verify 到 llc | ✅ **直接适用** | Aura `Aot.aura` 同样有两次 `Process.run(llc)` |
| R2: C FFI 编译缓存 | ❌ **不适用** | Aura 不编译 C 文件（Aura 自实现 std 库） |
| R3: 源码哈希缓存 | ✅ **直接适用** | Aura 同样每次从零编译 |
| R4: stdlib rayon 并行 | ✅ **可适配** | Aura 用 `Thread.spawn` + `Channel` 替代 rayon |
| R5: pass par_iter_mut | ✅ **可适配** | Aura 用 `Thread.spawn` 并行处理函数列表 |
| R6: tokio 异步进程 | ✅ **可适配** | Aura 用 `Thread.spawn` 并行执行 `Process.run` |
| R7: sccache 构建缓存 | ❌ **不适用** | Rust 构建工具特性，Aura 无对应 |
| R8: Cargo Profile 优化 | ❌ **不适用** | Rust 构建工具特性，Aura 无对应 |
| R9: 依赖裁剪 | ❌ **不适用** | Aura 编译器是单文件/多模块，无外部依赖树 |

### 5.2 Rust 特有问题（Aura 无此问题）

| 问题 | Rust 位置 | Aura 状态 |
|------|----------|-----------|
| EmitCtx 全局计数器（bb_counter/var_counter） | `aot/emit.rs` | Aura 无此架构问题 |
| C FFI 文件重复编译 | `linker.rs` | Aura 不编译 C 文件 |
| rayon 依赖缺失 | `Cargo.toml` | Aura 用 Thread/Channel 替代 |
| tokio 依赖缺失 | `Cargo.toml` | Aura 用 Thread/Channel/Coroutine 替代 |

### 5.3 Aura 特有问题（Rust 无此问题）

| 问题 | Aura 位置 | 优化方向 |
|------|----------|---------|
| 扁平字符串 arena 表示 | Hir/Mir/Codegen | HashMap + ArrayList |
| O(n) 线性扫描查找 | Codegen.aura | HashMap O(1) |
| O(n²) 字符串拼接 | Codegen.aura | ArrayList 批量 join |
| 递归同步文件读取 | ModuleLink.aura | Thread.spawn 并行 |
| Process.run 串行阻塞 | Aot.aura | Thread.spawn 并行 |

### P0 — 立即收益

#### R1. 合并 llc verify 到主命令

**文件**：`compiler/src/codegen/aot/linker.rs:161-177`

```rust
// 当前：两次 llc 调用
let mut verify_cmd = build_command("llc", options)?;
verify_cmd.arg(ll_path).arg("-verify-each");
run_and_report(&mut verify_cmd, "llc -verify")?;

// 优化：合并到主命令
let mut cmd = build_command("llc", options)?;
cmd.arg(ll_path)
    .arg("-o").arg(object_path)
    .arg(options.opt_level.as_llvm_flag())
    .arg("-filetype=obj")
    .arg("-verify-each");
```

#### R2. C FFI 编译缓存

**文件**：`compiler/src/codegen/aot/linker.rs:254-294`

检查 `aura_std_cffi.c` 和 `aura_syscalls.c` 的 mtime，比 `.o` 新才重编。

#### R3. 源码哈希缓存

编译前检查源文件哈希，匹配已有缓存则跳过。

### P1 — 并行化

#### R4. stdlib 并行编译

**文件**：`cli/src/main.rs:726-785`

```rust
// 当前：顺序 for 循环
for (rel_path, abs_path) in &aura_files {
    let source = read_file(abs_path);
    let module = compile_source(&source);
    write_auc(&out_file, &module);
}

// 优化：rayon 并行
aura_files.par_iter().for_each(|(rel_path, abs_path)| {
    let source = read_file(abs_path);
    let module = compile_source(&source);
    write_auc(&out_file, &module);
});
```

#### R5. Pass 级 par_iter_mut

对 `fold_hir` / `inline_hir` / `lower_program` / `arc::run_arc_analysis` 使用 rayon 并行遍历函数列表。

#### R6. 外部进程异步并行

引入 `tokio` 依赖，用 `tokio::process::Command` 异步执行 `llc`/`clang`，多模块并行 AOT 编译。

### P2 — 构建优化

#### R7. sccache 构建缓存

```toml
# .cargo/config.toml
[build]
rustc-wrapper = "sccache"
```

#### R8. Cargo Profile 优化

```toml
[profile.dev.build-override]
opt-level = 2
debug = false
```

#### R9. 依赖裁剪

cranelift 标记为 optional（已实现）。

---

## 6. 统一路线图

| 优先级 | Rust 编译器 | Aura 编译器 | 预期总收益 |
|--------|------------|------------|-----------|
| **P0** | 合并 verify 到 llc (R1) | 合并 verify 到 llc (A3) | 快 10-20% |
| **P0** | C FFI 缓存 (R2) | 跳过 emitC (A4) | 快 10-20% |
| **P0** | 源码哈希缓存 (R3) | 源码哈希缓存 (A5) | 未改文件→0ms |
| **P1** | stdlib rayon 并行 (R4) | **HashMap 替代线性扫描 (A1)** | **快 2-5x** |
| **P1** | pass par_iter_mut (R5) | **ArrayList 替代拼接 (A2)** | **快 5-20x** |
| **P1** | sccache 配置 (R7) | kids 计数缓存 (B3) | 快 1.5-3x |
| **P2** | Import 并行解析 (R6) | 并行模块链接 (B1) | 快 2-4x |
| **P2** | LLVM IR 并行生成 | AOT 进程并行 (C1) | 快 2-3x |
| **P2** | profile 优化 (R8) | 协程 I/O 重叠 (C2) | 快 1.5-2x |
| **P3** | tokio 异步 (R6) | 并行 HIR Pass (B2) | 快 2-3x |
| **P3** | — | 并行 HIR 发射 (C3) | 快 2-4x |
| **P4** | 增量编译 | 增量编译 | 快 5-10x |

### 实施顺序

1. **Phase 1**（1天）：保存方案 → Aura HashMap → Aura ArrayList → 合并 verify
2. **Phase 2**（2-3天）：Aura kids 缓存 → Aura 并行模块链接 → Rust verify 合并
3. **Phase 3**（1周）：Rust rayon 并行 → Rust tokio 异步 → Aura 协程 I/O 重叠
4. **Phase 4**：性能基准对比（前后对比测试）

---

## 7. 实施进度记录（2026-07-06）

> 本节记录 optimization-plan.md 中 Aura 自举编译器优化项的实施状态。

### 7.1 已完成项

| 优化项 | 文件 | 状态 | 说明 |
|--------|------|------|------|
| **A1: HashMap 替代线性扫描** | `Codegen.aura` | ✅ 已完成 | `constIdxMap`/`varIdxMap`/`labelIdxMap` 均为 `HashMap<String, Int>`，查找 O(1) |
| **A2: EmitBuffer 替代字符串拼接** | `Codegen.aura` | ✅ 已完成 | `bytecodeLines` 用 `ArrayList<String>` 批量追加；`constPoolBuf`/`funcTableBuf` 改用 `EmitBuffer`（原生 StringBuilder） |
| **A2: EmitBuffer 替代字符串拼接** | `CBackend.aura` | ✅ 已完成 | `fBody` 从 `String` 改为 `EmitBuffer`，消除 O(n²) 拼接 |
| **A2: EmitBuffer 替代字符串拼接** | `Mir.aura` | ✅ 已完成 | 整个 Mir arena 从扁平字符串改为 List-based（匹配 Hir.aura 结构），消除 O(n²) |
| **A3: 合并 verify 到 llc** | `Aot.aura` | ✅ 已完成 | `llc -verify-each` 合并到主编译命令 |
| **B3: kids 计数预计算** | `Hir.aura` / `Mir.aura` | ✅ 已完成 | `hirKidsCount`/`mirKidsCount` 改用 `charCodeAt` 单次扫描计数，不再 split 分配整张表 |
| **消除 toStr(text[i]) 反模式** | 多文件 | ✅ 已完成 | `Codegen.aura`/`ModuleLink.aura`/`Aot.aura`/`Mir.aura` 所有字符遍历改用 `charCodeAt`，消除逐字符字符串分配 |
| **mirToIntOf 优化** | `Mir.aura` | ✅ 已完成 | 从 10 次字符串比较改为 `charCodeAt - 48` 直接算法 |
| **ModuleLink.aura seen 优化** | `ModuleLink.aura` | ✅ 已完成 | `seen` 从换行分隔字符串改为 `ArrayList<String>`，contains 检查 O(1) |
| **ModuleLink.aura 路径工具优化** | `ModuleLink.aura` | ✅ 已完成 | `normalizePath`/`slashify`/`dirOf`/`replaceAllDots` 改用 `charCodeAt` |
| **Aot.aura aotWinPath 优化** | `Aot.aura` | ✅ 已完成 | `aotWinPath` 改用 `charCodeAt` |

### 7.2 未完成项及原因

| 优化项 | 预期收益 | 阻塞原因 | 建议方案 |
|--------|----------|----------|----------|
| **B1: 并行模块链接** | 快 2-4x | `Thread.spawn(fn_id, arg)` API 限制：只能传单个 Int 参数，无法传文件路径等复杂数据。需要全局数组 + 索引方案，且 `Channel` 返回 `Any` 类型需要动态转换 | 需要扩展 Thread API 或使用 `Future` 原语（需验证自举 AOT 下泛型集合可靠性） |
| **B2: Pass 级并行** | 快 3-6x | 同上：线程通信受限。且 pass 间有数据依赖（Mono 需全局调用图） | Fold/Inline 可独立并行，但需线程安全的结果收集机制 |
| **C1: AOT 进程并行** | 快 2-3x | `Process.run` 是同步阻塞调用，`Thread.spawn` 无法传递进程参数 | 可用 `Channel` 在子线程中执行 `Process.run`，但需验证自举 AOT 下的可靠性 |
| **C2: 协程 I/O 重叠** | 快 1.5-2x | 同上 | 待 Thread/Channel 自举可靠性验证后实施 |
| **C3: 并行 HIR 发射** | 快 2-4x | `Emit.aura` 的 `LlvmEmitter` 是有状态对象，函数间共享计数器（`varCount`/`bbCount`/`fGen` 等），并行化需拆分状态 | 可先将「全局初始化」与「函数级 IR 生成」分离，再并行发射各函数 |
| **A4: 跳过 emitC** | 快 10-20% | 已实现 `emitCSource = false` 开关，但未默认启用 | 在 `compileAotIrOnly` 路径已跳过 |
| **A5: 源码哈希缓存** | 未改文件→0ms | 需要文件系统 mtime 检查 + 缓存目录管理 | 可在 Rust 编译器侧实现（CLI 层），Aura 编译器侧暂不实现 |

### 7.3 优化效果预期

| 指标 | 优化前 | 优化后 | 改善 |
|------|--------|--------|------|
| Mir arena 内存分配 | O(n²) 字符串拼接 | O(n) List 追加 | **减少 90%+ 临时分配** |
| Codegen 常量池构建 | O(n²) 字符串拼接 | O(n) EmitBuffer 追加 | **减少 95%+ 临时分配** |
| CBackend 函数体构建 | O(n²) 字符串拼接 | O(n) EmitBuffer 追加 | **减少 95%+ 临时分配** |
| 字符遍历分配 | 每字符 1 次 toStr 分配 | charCodeAt 零分配 | **消除所有逐字符临时串** |
| ModuleLink 去重检查 | O(n) 字符串线性扫描 | O(n) List.contains（但常数更小） | **减少 50% 检查开销** |
| 常量查找 | O(n) 线性扫描 | O(1) HashMap | **快 2-5x** |

### 7.4 编译验证

- **bytecode 编译**：`scripts\build-aura-compiler.ps1` → ✅ 成功
- **AOT 编译**：`scripts\build-aura-compiler.ps1 -Aot` → ✅ 成功
- **语言测试**：`scripts\run-language-tests.ps1` → ✅ **25/25 全部通过**

### 7.5 自举编译基准测试（2026-07-06）

> 命令：`build\bench_selfhost.ps1`（编译 Main.aura → AOT exe，冷/热各 3 轮）

| 场景 | 优化前 (s) | 字符串优化后 (s) | 并行化+字符串优化 (s) |
|------|-----------|-----------------|---------------------|
| Rust cold | 4.19 | 6.01 | 6.52 |
| Rust hot | 4.09 | 5.85 | 6.10 |
| **Aura cold** | **43.03** | **43.62** | **12.53** |
| **Aura hot** | **43.38** | **43.08** | **12.51** |
| **Ratio (cold)** | **10.27x** | **7.26x** | **1.92x** |
| **Ratio (hot)** | **10.35x** | **7.29x** | **2.05x** |

**分析**：
- 字符串拼接优化（O(n²)→O(n)）：Aura 时间基本不变（43s±1%），不是主要瓶颈。
- **并行模块加载（Thread.spawn + ParallelState）**：Aura 从 43s 降到 12.5s（**3.4x 加速**）。
- 最终 Ratio 从 10.27x 降到 **1.92x**（接近 2x 目标）。
- 主要瓶颈已解决；剩余时间在 HIR lowering（~10s）和 LLVM 编译（~2s）。

### 7.6 并行化实施记录

| 项 | 状态 | 实现方式 |
|----|------|----------|
| **B1: 并行模块链接** | ⛔ **已回退（2026-09-19）** | 见下方「并行模块链接回退」：`object` 单例字段在 AOT 下不受支持，导致自举编译器「编译成功但一运行即段错误」；改为顺序预读，实测耗时无回退（13.2s vs 13.2s） |
| **B1: Emit.aura 对象去重** | ✅ **已完成** | `fObjectClassNames` 添加 `aotListContains` 去重检查，修复自举编译时全局变量重复定义 |
| **A2: Emit.aura 字符串优化** | ✅ **已完成** | `hexByte`/`charCodeOf` 改用 `aotSlice` 替代 `toStr(digits[i])` |
| **A2: TypeMapper.aura 字符串优化** | ✅ **已完成** | `sanitizeLlvm` 改用 `aotSlice` 替代 `toStr(s[i])`（发射期最热调用之一） |
| **A2: CompileError.aura 字符串优化** | ✅ **已完成** | `ceToIntOf` 改用 `charCodeAt - 48`；`ceFieldAt` 改用 `substring` + `charCodeAt` |
| **A2: Parser.aura 字符串优化** | ✅ **已完成** | `isWordChars` 改用 `aotSlice` 替代 `toStr(s[i])` |
| **A2: SymbolTable.aura 字符串优化** | ✅ **已完成** | `stLine`/`stField`/`stFieldAt` 改用 `charCodeAt`；`symToIntOf` 改用 `charCodeAt - 48` |
| **A2: Main.aura 字符串优化** | ✅ **已完成** | `cliDir`/`cliStem` 改用 `charCodeAt` 替代 `toStr(p[i])` |
| **B2: 并行 HIR Pass** | ⏳ 暂缓 | Fold/Inline 未在当前 AOT 管线中使用，无可并行项 |
| **C1: AOT 进程并行** | ⏳ 待实现 | 需扩展 Thread API 支持进程管理 |

#### 并行模块链接回退（2026-09-19）

**现象**：自举编译器（`aura build aura/compiler/aura/lang/compiler/Main.aura --aot`）
能编译成功，但一运行即崩溃：

```
[aura] memory limit exceeded: used ~0 MiB, requesting ~33681408 MiB more, limit 8192 MiB.
```

或直接 `0xC0000005`（访问冲突）。

**定位**（gdb + `lld-link /map` 符号化调用栈）：

```
aura_mem_realloc ← aura_dynlist_push ← Collections_listAppend
  ← AotModuleLinker_preloadCache ← AotModuleLinker_link ← MainUtils_runCli ← main
```

`Collections_listAppend` 收到的「列表句柄」是 `0xABABABABABABABAB`（MSVC 堆的
未初始化填充值），`aura_dynlist_push` 读 `items` 字段（偏移 16）落在分配块之外。

**根因**：`ModuleLink.aura` 的并行预读把共享状态放在
`object ParallelState { var paths: ArrayList<String> ... }` 上，而
**Rust AOT 后端（`compiler/src/codegen/aot/`）完全不支持 `object` 单例字段**：

1. `ParallelState.paths`（HIR 里被降级为零参调用 `ParallelState.paths()`）
   被发射成「新建一个 `ArrayList` 对象并返回」，从不读取单例全局
   （Rust 后端连 `@aura_obj_*` 全局都不发射；`is_singleton` 在该后端零引用）；
2. 该 `ArrayList` 实例随后又被当作**裸集合句柄**传给
   `Collections.listAppend`/`getAt`/`count` —— `%struct.ArrayList`（`{i8*,i32,i32}`
   = 16 字节）与 `AuraDynList`（`{len,cap,items}` = 24 字节）布局不同，
   读 `items` 直接越界。

对比：**Aura 侧发射器（`aot/Emit.aura`）已支持 `object` 单例**（`fObjectClassNames`
+ `@aura_obj_*` 全局 + 方法 `self` 传单例地址），因此由它生成的编译器可正常运行。

**处置**：`preloadCache` 回到顺序预读（只依赖 `AotModuleLinker` 的 Class 实例字段，
两个后端语义都正确）。实测 `pgen1.exe`（并行版）与 `aura-compiler-parallel.exe`
（顺序版）编译自身均为 **13.2s**，即并行预读在此场景**没有可测收益**，
故回退无性能损失。

**后续（若要恢复并行）**：需要给 **Rust AOT 后端**补齐 `object` 单例支持：
发射 `@aura_obj_<Name>` 全局、把 `Name.field` 零参调用降级为字段 `load`、
把 `Name.field = v` 降级为字段 `store`、并在入口调用 `<Name>.__singletonInit`
（或让 `Thread` 支持传递超过 Int 的负载）。
