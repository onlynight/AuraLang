# Aura 调试器设计方案

> **版本**: v1.1  
> **状态**: Phase 1-4 全部完成  
> **日期**: 2026-07-21  

---

## 一、概述

为 Aura 语言实现完整的源码级调试器，覆盖三种执行后端：**VM 解释器**、**JIT（Cranelift）**、**AOT（LLVM）**。

### 1.1 核心定位

`aura-debug` 是独立的调试工具二进制（对标 GDB / LLDB），提供交互式源码调试能力：断点、单步、变量检查、调用栈、源码查看。

### 1.2 现有二进制架构

```
AuraLang workspace
├── compiler/          # 编译器库（lib crate）
├── cli/               # 命令行工具（bin crate）
│   ├── aura           # 编译/运行/检查/包管理
│   ├── aura-lsp       # 独立 LSP 服务器
│   └── aura-debug     # ★ 独立调试器（新增）
└── loom/              # 构建系统
```

---

## 二、三后端现状与调试策略

| 维度 | VM（解释器） | JIT（Cranelift） | AOT（LLVM） |
|------|:-----------:|:----------------:|:-----------:|
| **执行模型** | `step()` 逐条执行 `Instr` | 编译为原生码，`JitEntry` 派发 | LLVM IR → 原生可执行文件 |
| **变量表示** | `Frame.locals: Vec<Value>` | `JitValue { tag, payload }` | LLVM `alloca` + SSA |
| **调试能力** | ✅ 完整（逐条单步） | ⚠️ 部分（跟踪调用，不可单步 JIT 函数） | ⚠️ DWARF + 外部调试器 |

### 2.1 调试策略

```
VM 模式:   DebugSession 直接控制 Vm::step()，完整断点+单步+变量检查
JIT 模式:  跟踪 JIT 编译状态，通过 VM step() 跟踪调用点；JIT 函数入口断点，内部不可单步
AOT 模式:  生成 DWARF 元数据，启动外部 GDB/LLDB；DebugSession 提供辅助信息
```

---

## 三、架构设计

```
┌─────────────────────────────────────────────────────────────────────┐
│                      用户交互层                                       │
│                                                                     │
│  ┌──────────────────────┐    ┌──────────────────────────────────┐  │
│  │  aura-debug          │    │  aura debug (子命令)              │  │
│  │  (debugger_main.rs)  │    │  (main.rs → spawn aura-debug)    │  │
│  └──────────┬───────────┘    └──────────────┬───────────────────┘  │
│             ▼                                ▼                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │               Debug REPL (cli/src/debugger.rs)                │  │
│  │  命令解析 → 命令分发 → 输出格式化                                │  │
│  └────────────────────────────────┬─────────────────────────────┘  │
├───────────────────────────────────┼─────────────────────────────────┤
│                          调试引擎层                                   │
│  ┌────────────────────────────────┴─────────────────────────────┐  │
│  │           DebugSession (compiler/src/vm/debugger.rs)          │  │
│  │  BreakpointManager | Stepper | StateInspector                 │  │
│  │  SourceMapping     | ModeManager | FormatHelper               │  │
│  └───────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────┤
│                       后端执行层                                       │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────────┐  │
│  │ VmBackend    │   │ JitBackend   │   │ AotBackend           │  │
│  │ debug_step() │   │ jit_state()  │   │ generate_dwarf()     │  │
│  │ frames()     │   │ jit_compile()│   │ spawn_external()     │  │
│  └──────────────┘   └──────────────┘   └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 四、核心数据结构

### 4.1 调试模式

```rust
pub enum DebugMode {
    Vm,    // VM 解释执行（默认）— 完整调试
    Jit,   // JIT 编译 — 跟踪 JIT 状态，回退 VM
    Aot,   // AOT 原生 — DWARF + 外部调试器
}
```

### 4.2 断点

```rust
pub enum BreakpointTarget {
    Line { line: usize },
    Function { name: String },
    JitFunc { func_idx: usize },
    AotSymbol { symbol: String },
}

pub struct Breakpoint {
    pub id: usize,
    pub target: BreakpointTarget,
    pub resolved_func_idx: Option<usize>,
    pub resolved_instr_idx: Option<usize>,
    pub enabled: bool,
    pub hit_count: u32,
    pub condition: Option<String>,
    pub mode: DebugMode,
}
```

### 4.3 暂停原因

```rust
pub enum StopReason {
    Breakpoint { bp_id: usize, description: String },
    Step { mode: StepMode },
    Completion { result: Value },
    Error { message: String, at_func: Option<String> },
    JitCompiled { func_name: String, success: bool },
    AotCompiled { path: String },
}
```

### 4.4 单步模式

```rust
pub enum StepMode {
    Off,    // 运行到下一个断点
    In,     // 步入：每条指令暂停
    Over,   // 步过：执行到当前函数返回
    Out,    // 步出：执行到当前函数返回
}
```

### 4.5 调试会话

```rust
pub struct DebugSession {
    pub mode: DebugMode,
    pub vm: Option<Vm>,
    pub module: BytecodeModule,
    pub source: String,
    pub file_name: String,
    pub source_map: SourceMap,
    pub file_id: FileId,
    pub mapping: SourceMapping,
    pub breakpoints: Vec<Breakpoint>,
    pub step_mode: StepMode,
    pub paused: bool,
    pub stop_reason: Option<StopReason>,
    pub step_counter: u64,
}
```

### 4.6 JIT 调试信息

```rust
pub struct JitDebugInfo {
    pub func_states: Vec<JitFuncState>,
    pub total_compile_count: u32,
    pub fallback_count: u32,
}

pub struct JitFuncState {
    pub func_idx: usize,
    pub func_name: String,
    pub is_compiled: bool,
    pub is_skipped: bool,
    pub skip_reason: Option<String>,
    pub call_count: u64,
}
```

### 4.7 AOT 调试信息

```rust
pub struct AotDebugInfo {
    pub ll_path: Option<PathBuf>,
    pub exe_path: Option<PathBuf>,
    pub dwarf_functions: Vec<DwarfFunctionInfo>,
    pub external_debugger: Option<String>,
    pub supports_breakpoints: bool,
}

pub struct DwarfFunctionInfo {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub llvm_name: String,
}
```

---

## 五、VM 调试模式（Phase 1）

### 5.1 源码位置映射

**当前限制**：字节码不携带源码位置信息。

**Phase 1 降级方案**：
- 从 AST 解析函数声明 → 函数名 + 行号范围
- 从 `BytecodeModule.functions` → 函数索引
- 通过函数名匹配建立映射
- 断点精确到函数入口，步进制精确到指令

**Phase 4 增强方案**：
- 在 HIR/MIR 保持 Span 关联
- 在 `BytecodeFunction` 增加 `line_table: Option<Vec<(usize, usize)>>`
- 每条指令记录源码行号

### 5.2 VM 调试循环

```
DebugSession.run()
  │
  ├─ check_breakpoints()     ← 检查断点
  ├─ vm.debug_step()          ← 执行一条指令
  ├─ check_completion()       ← 检查是否完成
  └─ check_step_mode()        ← 检查单步模式
```

### 5.3 VM 状态检查

- `show_locals(depth)` — 局部变量（参数 + 局部槽）
- `show_stack(depth)` — 操作数栈
- `show_backtrace()` — 调用栈（函数名 + 行号 + 指令）
- `show_source(line, context)` — 源码片段
- `show_current_instr()` — 当前指令反汇编
- `show_info()` — VM 状态摘要

---

## 六、JIT 调试模式（Phase 2）✅ 完成

### 6.1 实现概要

JIT 调试通过 VM 的 `jit_state()` 和 `call_counts()` 查询实时 JIT 编译状态。`refresh_jit_info()` 在每次 `jit` 命令执行时刷新，构建 `JitDebugInfo` 结构体。

### 6.2 核心方法

| 方法 | 说明 |
|------|------|
| `refresh_jit_info()` | 从 VM 查询 JIT 状态，构建 `JitDebugInfo` |
| `show_jit_state()` | 显示所有函数的 JIT 编译状态（已编译/已跳过/待编译） |
| `show_jit_fallbacks()` | 显示回退到 VM 的函数及原因 |
| `show_jit_compiled()` | 显示已 JIT 编译的函数列表 |

### 6.3 JIT 独有命令

| 命令 | 说明 |
|------|------|
| `jit state` | 每个函数的 JIT 编译状态表 |
| `jit fallbacks` | 回退到 VM 的函数及原因 |
| `jit compiled` | 已 JIT 编译的函数列表 |

### 6.4 实现文件

- `compiler/src/vm/debugger.rs` — `refresh_jit_info()`, `show_jit_state()`, `show_jit_fallbacks()`, `show_jit_compiled()`
- `compiler/src/vm/mod.rs` — 新增 `jit_skip_reason()` 方法
- `cli/src/debugger.rs` — `jit` 子命令分发

---

## 七、AOT 调试模式（Phase 3）✅ 完成

### 7.1 实现概要

AOT 调试通过 `aot_compile()` 调用 AOT 编译管线生成 `.ll` + `.exe`，然后从 `.ll` 文本解析 `DISubprogram` 元数据提取 DWARF 函数列表。`aot_launch()` 自动检测并启动 `lldb`/`gdb`/`windbg`。

### 7.2 核心方法

| 方法 | 说明 |
|------|------|
| `aot_compile()` | 执行 AOT 编译，生成 .ll + .exe，解析 DWARF 函数 |
| `parse_dwarf_functions()` | 从 LLVM IR 文本解析 `!DISubprogram` 元数据 |
| `show_aot_dwarf()` | 显示 DWARF 调试信息摘要 |
| `show_aot_path()` | 显示 .ll / .exe 输出路径 |
| `aot_launch()` | 检测并启动外部调试器 (lldb/gdb) |

### 7.3 AOT 独有命令

| 命令 | 说明 |
|------|------|
| `aot compile` | 执行 AOT 编译 |
| `aot dwarf` | DWARF 元数据摘要 |
| `aot launch` | 启动外部调试器 |
| `aot path` | 输出路径 |

### 7.4 依赖

AOT 功能需要 `llvm` feature: `cargo build --features llvm`

### 7.5 实现文件

- `compiler/src/vm/debugger.rs` — `aot_compile()`, `parse_dwarf_functions()`, `show_aot_dwarf()`, `show_aot_path()`, `aot_launch()`
- `compiler/src/codegen/aot/linker.rs` — 修复 `-g` 标志（仅传给 clang，不传给 llc）
- `cli/src/debugger.rs` — `aot` 子命令分发
- `cli/src/debugger_main.rs` — AOT 模式自动编译

---

## 八、字节码 line_table（Phase 4）✅ 完成

### 8.1 实现概要

在 `BytecodeFunction` 中新增 `line_table: Option<Vec<(usize, usize)>>` 字段，记录每条指令对应的源码行号。`SourceMapping` 利用 line_table 建立指令级行号映射，使断点可以精确到具体指令而非仅函数入口。

### 8.2 数据结构

```rust
pub struct BytecodeFunction {
    // ... 已有字段 ...
    /// Phase 4: 指令索引 → 源码行号映射表
    /// 每个条目 (instr_index, source_line) 记录一条指令对应的源码行号
    pub line_table: Option<Vec<(usize, usize)>>,
}
```

### 8.3 核心改进

| 改进 | 说明 |
|------|------|
| `SourceMapping.instr_line_map` | 新增指令级行号映射 HashMap |
| `line_to_instr(line)` | 查找行号对应的 (func_idx, instr_idx) |
| `resolve_breakpoint()` | 优先使用 line_table 精确定位 |
| `current_line()` | 优先使用 line_table 获取当前行号 |

### 8.4 向后兼容

`line_table` 为 `Option`，为 `None` 时回退到函数入口断点（Phase 1 行为）。现有 `.auc` 文件和代码生成流水线不受影响。

### 8.5 实现文件

- `compiler/src/codegen/opcode.rs` — `BytecodeFunction.line_table` + `Default` impl
- `compiler/src/vm/debugger.rs` — `SourceMapping.instr_line_map`, `line_to_instr()`, 精确断点
- `compiler/src/codegen/emit.rs` — `line_table: None` 占位
- `compiler/src/codegen/serialize.rs` — `line_table: None` 占位
- `compiler/tests/vm_tests.rs` — 测试用例更新
- `compiler/tests/p7_memory_tests.rs` — 测试用例更新

---

## 九、CLI 命令设计

### 8.1 通用命令

| 命令 | 别名 | VM | JIT | AOT | 说明 |
|------|------|:--:|:---:|:---:|------|
| `break <target>` | `b` | ✅ | ✅ | ✅ | 设置断点 |
| `continue` | `c` | ✅ | ✅ | ❌ | 继续执行 |
| `step` | `s` / `si` | ✅ | ✅ | ❌ | 步入 |
| `next` | `n` / `so` | ✅ | ✅ | ❌ | 步过 |
| `out` | | ✅ | ✅ | ❌ | 步出 |
| `backtrace` | `bt` | ✅ | ✅ | ❌ | 调用栈 |
| `list [line]` | `l` | ✅ | ✅ | ✅ | 源码 |
| `print <expr>` | `p` | ✅ | ✅ | ❌ | 打印值 |
| `locals [depth]` | | ✅ | ✅ | ❌ | 局部变量 |
| `stack [depth]` | | ✅ | ✅ | ❌ | 操作数栈 |
| `info` | | ✅ | ✅ | ✅ | 调试信息 |
| `del <id>` | `d` | ✅ | ✅ | ✅ | 删除断点 |
| `mode <vm\|jit\|aot>` | `m` | ✅ | ✅ | ✅ | 切换模式 |
| `help` | `h` / `?` | ✅ | ✅ | ✅ | 帮助 |
| `quit` | `q` / `exit` | ✅ | ✅ | ✅ | 退出 |

### 8.2 模式专属命令

| 命令 | 模式 | 说明 |
|------|------|------|
| `jit state` | JIT | JIT 编译状态 |
| `jit fallbacks` | JIT | JIT 回退详情 |
| `jit compiled` | JIT | 已编译函数列表 |
| `aot dwarf` | AOT | DWARF 摘要 |
| `aot launch` | AOT | 启动外部调试器 |
| `aot path` | AOT | 输出路径 |

### 8.3 交互示例

```text
$ aura-debug examples/demo.aura

═══════════════════════════════════════════════════════════
  Aura 调试器 v0.1
  源码: examples/demo.aura
  模式: VM  |  函数: 5  |  断点: 0
═══════════════════════════════════════════════════════════

(demo:10)  10 | fun main() {
>        11 |     val x = 42
(demo:10)  12 |     println("x = $x")
(demo:10)  13 | }

  (b)reak  (c)ontinue  (s)tep  (n)ext  (l)ist  (h)elp  (q)uit

> b 12
  断点 #1 设置: 行 12 → main (instr 0)
> c
  运行中...
  断点 #1 命中 (行 12)

(demo:12)  12 |     println("x = $x")
> p x
  x = 42i
> bt
  #1  main (line 12, instr 8/15)
> n
  步过...

(demo:13)  13 | }
> c
  执行完成: "x = 42"
> q
  退出调试器
```

---

## 九、文件结构

### 9.1 新增文件

| 文件 | 职责 |
|------|------|
| `docs/Aura调试器设计方案.md` | 本文档 |
| `compiler/src/vm/debugger.rs` | 调试引擎核心库 |
| `cli/src/debugger.rs` | Debug REPL 交互逻辑 |
| `cli/src/debugger_main.rs` | `aura-debug` 二进制入口 |

### 9.2 修改文件

| 文件 | 修改内容 |
|------|---------|
| `cli/Cargo.toml` | 新增 `[[bin]] aura-debug` |
| `compiler/src/vm/mod.rs` | `pub mod debugger` + VM 调试访问器 |
| `cli/src/main.rs` | `debug` 子命令转发到 `aura-debug` |

---

## 十、实施计划

| 阶段 | 内容 | 状态 |
|------|------|------|
| **Phase 1** | VM 调试引擎 + `aura-debug` 独立二进制 + REPL | ✅ 完成 |
| **Phase 2** | JIT 状态跟踪 + JIT 断点 + `jit` 命令 | ✅ 完成 |
| **Phase 3** | AOT DWARF 增强 + 外部调试器集成 | ✅ 完成 |
| **Phase 4** | 字节码 `line_table` + 指令级行号映射 | ✅ 完成 |

---

## 十一、风险与缓解

| 风险 | 影响 | 缓解方案 | 状态 |
|------|------|---------|------|
| 字节码无源码位置信息 | 断点行号映射不精确 | Phase 4 已添加 `line_table` 字段，`SourceMapping` 支持指令级映射 | ✅ 已缓解 |
| JIT 函数不可单步 | 无法调试 JIT 热点 | `jit fallbacks` 显示回退原因；引导 VM 模式调试 | ✅ 已缓解 |
| AOT 外部调试器不可用 | AOT 模式无法使用 | 降级为"仅生成 DWARF 摘要"模式 | ✅ 已缓解 |
| `step()` 是 `pub(crate)` | 外部 crate 无法调用 | `debug_step()` 包装方法 | ✅ 已解决 |
| `llc` 不支持 `-g` | AOT DWARF 编译失败 | 已修复：`-g` 仅传给 `clang`，不传给 `llc` | ✅ 已解决 |
