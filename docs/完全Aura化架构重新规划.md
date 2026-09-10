# 完全 Aura 化架构重新规划

> **版本**: 3.0  
> **日期**: 2026-07-05  
> **状态**: 重新规划中  
> **核心修正**: `core/*.aura` 必须是唯一真相源，Rust native 仅限 Layer 0-A 引导层

---

## 1. 当前架构问题（方向性错误）

### 1.1 当前实现（错误方向）

```
当前架构（双层漂移）：
┌─────────────────────────────────────────────────────────────────┐
│ core/aura/lang/std/*.aura                                       │
│ ├── 22 个 .aura 文件存在                                         │
│ ├── ~60% 函数仅签名（依赖 Rust native 兜底）                      │
│ ├── vm/gc/memory 等是占位骨架代码                                  │
│ ├── ✗ 不参与编译                                                  │
│ ├── ✗ 不参与运行时                                                 │
│ └── 仅用于 IDE SourceIndex 生成（phantom source）                  │
│                                                                  │
│ compiler/src/std/std_*.rs                                       │
│ ├── 20 个 Rust native 实现文件                                     │
│ ├── ✓ 实际运行时真相源                                             │
│ ├── ✓ VM 通过 CallNative 直接调用                                  │
│ └── 与 core/*.aura 存在双层漂移                                    │
│                                                                  │
│ 结果：core/*.aura 是"文档"，不是"实现"                               │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 方向性错误分析

| 方面 | 方案要求 | 当前实现 | 偏差 |
|------|---------|---------|------|
| 真相源 | `core/*.aura` 是唯一源码 | `compiler/src/std/std_*.rs` 是真相源 | ❌ 完全相反 |
| 编译路径 | `core/*.aura` → `.auc` → VM 执行 | `core/*.aura` → SourceIndex（仅 IDE） | ❌ 未参与编译 |
| 运行时 | VM 优先调用 Aura 编译版本 | VM 只调用 Rust native | ❌ 未实现优先 |
| Rust native | 仅限 Layer 0-A 引导层 | Layer 1 标准库仍在 Rust | ❌ 未上移 |
| AOT 预编译 | 可预编译进二进制，但源码必须保留 | 无预编译机制 | ❌ 缺失 |

---

## 2. 正确架构设计

### 2.1 核心原则（修正版）

1. **`core/*.aura` 是唯一真相源**：所有标准库函数的源码实现必须在 `core/*.aura` 中
2. **Rust native 仅限 Layer 0-A**：VM 核心、JIT、AOT、编译器核心、FFI 操作（syscall）
3. **AOT 预编译可选**：如果某些函数需要 AOT 性能优化，可以预编译进二进制，但：
   - `core/*.aura` 必须保留完整源码
   - VM 优先调用 Aura 编译版本（字节码解释）
   - AOT 版本仅作为性能优化路径，不是必需路径
4. **单一编译管线**：`core/*.aura` → 编译 → `.auc` → 嵌入二进制 → VM 执行

### 2.2 分层架构（修正版）

```
最终架构（正确方向）：
┌─────────────────────────────────────────────────────────────────────────┐
│  Layer 0-A: 最小引导层（Rust，不能上移）                                   │
│  ├── compiler/src/vm/          — VM 核心（字节码解释器、栈帧管理）        │
│  ├── compiler/src/codegen/     — 代码生成（lexer→parser→HIR→MIR→字节码） │
│  ├── compiler/src/jit/         — JIT 编译器核心                          │
│  ├── compiler/src/aot/         — AOT 编译器核心（LLVM）                   │
│  └── compiler/src/bootstrap/   — 最小引导（Any 核心、类型内省）           │
│                                                                         │
│  Layer 0-B: 编译器基础设施（Aura 编写，自举）                              │
│  └── aura/compiler/aura/lang/compiler/                                │
│      ├── vm/              — VM 上层逻辑（指令分发、异常处理）              │
│      ├── gc/              — 垃圾收集器（标记-清除、增量、并发）            │
│      ├── memory/          — 内存管理（ARC、内存池）                       │
│      └── runtime/         — 运行时支持（协程调度、GC 触发）                │
│                                                                         │
│  Layer 1+: 用户标准库（全部 Aura 编译，aura/core/*.aura 为唯一真相源）      │
│  └── aura/core/aura/lang/std/                                       │
│      ├── Math.aura        — 数学函数（纯逻辑 + libm 签名）               │
│      ├── String.aura      — 字符串操作（纯逻辑）                         │
│      ├── Path.aura        — 路径操作（纯逻辑）                           │
│      ├── Encoding.aura    — 编码/解码（纯逻辑）                          │
│      ├── Time.aura        — 时间函数（纯逻辑）                           │
│      ├── Collections.aura — 集合操作（纯逻辑）                           │
│      ├── Builtin.aura     — 类型转换（纯逻辑 + native 签名）             │
│      ├── FileSystem.aura  — 文件系统（FFI 签名，Aura 调用）              │
│      ├── IO.aura          — IO 操作（FFI 签名）                         │
│      └── Network.aura     — 网络操作（FFI 签名）                        │
│                                                                         │
│  FFI 层: C/Rust native（通过 extern interface 声明）                    │
│  └── compiler/src/std/cffi/aura_std_cffi.c — syscall 实现              │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2.1 目录结构（v3.0）

```
aura/
├── core/                          # 用户标准库（Layer 1+）
│   └── lang/
│       └── std/                   # 标准库模块
│           ├── Math.aura
│           ├── String.aura
│           ├── Time.aura
│           ├── Collections.aura
│           ├── Builtin.aura
│           ├── Encoding.aura
│           ├── Path.aura
│           └── ...
├── compiler/                      # 编译器基础设施（Layer 0-B）
│   └── aura/
│       └── lang/
│           └── compiler/          # 编译器专用包
│               ├── vm/            # VM 上层逻辑
│               │   ├── Vm.aura
│               │   ├── Frames.aura
│               │   └── Opcodes.aura
│               ├── gc/            # 垃圾收集器
│               │   ├── Gc.aura
│               │   ├── MarkSweep.aura
│               │   ├── Concurrent.aura
│               │   └── Incremental.aura
│               ├── memory/        # 内存管理
│               │   ├── Memory.aura
│               │   ├── MemoryPool.aura
│               │   └── Arc.aura
│               └── runtime/       # 运行时支持
│                   ├── Coroutine.aura
│                   └── GcTrigger.aura
```

**命名规范**：
- 文件与类/单例/enum 使用 PascalCase（驼峰命名法），参考 Java/Kotlin
- 方法名使用 camelCase
- 文件名与主对象名一致（如 `Math.aura` 包含 `internal object Math`）

### 2.3 编译管线（修正版）

```
编译管线（单一真相源）：
┌─────────────────────────────────────────────────────────────────────────┐
│  构建阶段（cargo build）                                                │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │ 1. 编译 core/*.aura → .auc（预编译）                             │  │
│  │    - 使用当前 Rust 编译器（鸡生蛋问题）                            │  │
│  │    - 输出：std-auc/*.auc                                         │  │
│  │                                                                  │  │
│  │ 2. 嵌入 .auc 到二进制（include_bytes!）                           │  │
│  │    - compiler/src/std/embedded_stdlib.rs                        │  │
│  │    - 存储所有预编译的 .auc 字节                                    │  │
│  │                                                                  │  │
│  │ 3. 编译 Rust 二进制                                              │  │
│  │    - 包含 embedded stdlib                                        │  │
│  │    - Rust native 仅限 Layer 0-A                                  │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                                                                         │
│  运行时（VM 启动）                                                      │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │ 1. VM 加载 embedded stdlib（从二进制中提取 .auc）                │  │
│  │ 2. 注册 stdlib 函数到函数表                                       │  │
│  │ 3. 执行用户代码                                                  │  │
│  │    - CallNative → 优先检查 Aura 编译版本                         │  │
│  │    - 有 Aura 版本 → 调用 Aura 编译函数（字节码解释）              │  │
│  │    - 无 Aura 版本 → 回退到 Rust native（仅限 Layer 0-A + FFI）   │  │
│  └─────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.4 AOT 预编译策略（可选优化）

```
AOT 预编译（性能优化，非必需）：
┌─────────────────────────────────────────────────────────────────────────┐
│  场景：某些函数需要极致性能（如 Math.abs）                               │
│                                                                         │
│  策略：                                                                  │
│  1. core/Math.aura 保留源码（真相源）                                   │
│  2. 构建时可选编译 AOT 版本（aura build --aot）                          │
│  3. AOT 机器码嵌入 .auc v4                                              │
│  4. VM 优先检查 AOT 版本，无则回退字节码解释                            │
│                                                                         │
│  调用优先级：                                                            │
│  1. AOT 机器码（嵌入 .auc）                                              │
│  2. Aura 字节码（嵌入 .auc）                                             │
│  3. Rust native（仅限 Layer 0-A + FFI）                                 │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. 实施计划

### Phase 1: 基础设施（立即执行）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 1.1 | 修复 core/Math.aura 语法（`then` → `()` 语法） | `core/aura/lang/std/Math.aura` | 1h |
| 1.2 | 添加 `embed_stdlib.rs`（include_bytes! 嵌入 .auc） | `compiler/src/std/embedded_stdlib.rs` | 2h |
| 1.3 | 添加 `load_embedded_stdlib()` 到 VM | `compiler/src/vm/mod.rs` | 3h |
| 1.4 | 修改 `do_call_native` 优先调用 Aura 编译版本 | `compiler/src/vm/interp.rs` | 2h |
| 1.5 | 添加 build.rs 预编译 core/*.aura → .auc | `compiler/build.rs` | 4h |

### Phase 2: 修复 core/*.aura 语法（1-2 周）

| # | 任务 | 文件数 | 工作量 |
|---|------|--------|--------|
| 2.1 | 修复 vm/*.aura 语法错误 | 3 | 2d |
| 2.2 | 修复 gc/*.aura 语法错误 | 4 | 2d |
| 2.3 | 修复 memory/*.aura 语法错误 | 3 | 1d |
| 2.4 | 修复 runtime/*.aura 语法错误 | 2 | 1d |
| 2.5 | 修复 Builtin/Encoding/Path/String.aura | 4 | 2d |
| 2.6 | 确保所有 34 个文件可编译 | 34 | 1d |

### Phase 3: 移除 Rust native 冗余（2-3 周）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 3.1 | 移除 std_math.rs 中已有 Aura 实现的函数 | `compiler/src/std/std_math.rs` | 1d |
| 3.2 | 移除 std_string.rs 中已有 Aura 实现的函数 | `compiler/src/std/std_string.rs` | 1d |
| 3.3 | 移除 std_path.rs 中已有 Aura 实现的函数 | `compiler/src/std/std_path.rs` | 1d |
| 3.4 | 移除 std_collections.rs 中已有 Aura 实现的函数 | `compiler/src/std/std_collections.rs` | 1d |
| 3.5 | 添加一致性检查 CI（core/*.aura ↔ native registry） | `compiler/src/std/consistency_check.rs` | 1d |

### Phase 4: AOT 预编译（可选，3-4 周）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 4.1 | 添加 `--embed-aot` 构建选项 | `compiler/build.rs` | 2d |
| 4.2 | 修改 VM 支持 AOT 版本优先 | `compiler/src/vm/mod.rs` | 2d |
| 4.3 | 性能基准测试（AOT vs 字节码 vs Rust native） | `compiler/benches/` | 2d |

---

## 4. 当前实现修正

### 4.1 已完成的修正

- ✅ `core/Math.aura` 语法修复（`then` → `()` 语法）
- ✅ `embedded_stdlib.rs` 创建（`include_bytes!` 嵌入预编译 .auc）
- ✅ VM 启动时自动加载 embedded stdlib（`load_embedded_stdlib()`）
- ✅ `do_call_native` 优先检查 Aura 编译版本（`find_stdlib_func`）
- ✅ Self 参数注入（`Value::Null` 作为 fallback）
- ✅ `call_counts` 自动扩展（支持新增函数）
- ✅ 纯逻辑模块嵌入（Math/Time/Collections/Test）
- ✅ FFI 模块保持 Rust native（IO/FileSystem/Network/Random）

### 4.2 验证结果

```
$ aura run examples/language-test/test_stdlib_aura.aura
[vm] stdlib: loaded Math (31 functions merged)
[vm] stdlib: loaded Time (10 functions merged)
[vm] stdlib: loaded Collections (25 functions merged)
[vm] stdlib: loaded Test (18 functions merged)
[vm] stdlib: total 84 Aura-compiled functions ready
[vm] stdlib-aura: abs (argc=1) → Aura compiled func #2 (self=true)
[vm] stdlib-aura: abs (argc=1) → Aura compiled func #2 (self=true)
[vm] stdlib-aura: abs (argc=1) → Aura compiled func #2 (self=true)
FAIL: abs(-5) == null
FAIL: abs(3) == null
FAIL: abs(0) == null
```

**结论**：
- ✅ Embedded stdlib 加载成功（84 个 Aura 编译函数）
- ✅ `abs` 函数被正确派发到 Aura 编译路径（func #2, self=true）
- ❌ Aura 编译的 `Math.abs` 返回 `null`（编译器 bug）

### 4.3 已知编译器 Bug

**Bug 1**: 对象方法局部变量索引偏移错误（已修复）

**修复**: MIR `lower_function` 参数槽位从 1 开始（0 是函数指针）
```rust
let param_slots: Vec<usize> = (1..=f.params.len()).collect();
for (i, p) in f.params.iter().enumerate() {
    builder.declare(&p.name, i + 1);  // slot 从 1 开始
}
```

**Bug 2**: `if` 表达式返回值丢失（已修复）

**修复**: HIR `desugar_fn_with_self` 处理末尾 `HirStmt::If`，转换为 `Return` 语句
```rust
else if let HirStmt::If { cond, then_b, else_b } = &body.stmts[0] {
    // 重建 HirExpr::If 并包装为 Return
    body.stmts = vec![HirStmt::Return(Some(HirExpr::If { ... }))];
}
```

**Bug 3**: VM Frame 参数放置位置错误（已修复）

**修复**: `Frame::new` 参数从 `locals[1]` 开始（`locals[0]` 是函数指针）
```rust
for (i, a) in args.into_iter().take(n).enumerate() {
    locals[i + 1] = a;  // 参数从 locals[1] 开始
}
```

### 4.4 build.rs 嵌入式标准库检查

**新增**: `check_embedded_stdlib()` 函数检查 .auc 文件是否存在
```rust
fn check_embedded_stdlib() {
    // 检查 Math/Time/Collections/Test 的 .auc 文件
    // 如果缺失，提示运行 `aura stdlib-compile`
}
```

### 4.4 下一步行动

1. **本周**：修复 MIR emit 局部变量索引 bug（使对象方法可用）
2. **下周**：修复 core/*.aura 语法错误（vm/gc/memory/runtime）
3. **后续**：添加 build.rs 自动预编译 core/*.aura → .auc

---

## 5. 关键代码变更

### 5.1 embedded_stdlib.rs（新增）

```rust
// compiler/src/std/embedded_stdlib.rs
//! Phase 3: 嵌入式标准库（预编译 .auc 嵌入二进制）

/// 嵌入的 Math.aura 编译结果
pub static EMBEDDED_MATH_AUC: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"), "/std-auc/Math.auc"
));

/// 嵌入的 String.aura 编译结果
pub static EMBEDDED_STRING_AUC: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"), "/std-auc/String.auc"
));

// ... 其他模块 ...

/// 所有嵌入的标准库模块
pub static EMBEDDED_STDLIB_MODULES: &[(&str, &[u8])] = &[
    ("Math", EMBEDDED_MATH_AUC),
    ("String", EMBEDDED_STRING_AUC),
    // ...
];
```

### 5.2 build.rs 预编译（修改）

```rust
// compiler/build.rs
fn main() {
    // ... 现有逻辑 ...
    
    // Phase 3: 预编译 core/*.aura → .auc
    compile_stdlib_sources();
}

fn compile_stdlib_sources() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let std_dir = out_dir.join("std-auc");
    std::fs::create_dir_all(&std_dir).unwrap();
    
    // 编译每个 core/*.aura 文件
    for aura_file in scan_aura_files("core/aura/lang/std") {
        let source = fs::read_to_string(&aura_file).unwrap();
        let module = compile_source(&source).unwrap();
        let out_path = std_dir.join(format!("{}.auc", module_name));
        write_auc(&out_path, &module).unwrap();
    }
}
```

### 5.3 VM 启动加载（修改）

```rust
// compiler/src/vm/mod.rs
impl Vm {
    pub fn new(module: &BytecodeModule, opts: VmOptions) -> Result<Self, VmError> {
        // ... 现有逻辑 ...
        
        // Phase 3: 自动加载 embedded stdlib
        vm.load_embedded_stdlib();
        
        Ok(vm)
    }
    
    fn load_embedded_stdlib(&mut self) {
        for (name, auc_bytes) in EMBEDDED_STDLIB_MODULES {
            if let Ok(module) = from_bytes(auc_bytes) {
                self.merge_stdlib_module(name, &module);
            }
        }
    }
}
```

---

## 6. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 鸡生蛋问题 | core/*.aura 需要 Rust 编译器编译 | 保留最小 Rust 编译器作为引导层 |
| 性能回退 | 字节码解释比 Rust native 慢 | AOT 预编译作为可选优化 |
| 语法错误 | 部分 core/*.aura 有语法问题 | 逐一修复，添加 CI 检查 |
| 自举失败 | VM 自举可能失败 | 渐进式迁移，保留 Rust 回退 |

---

## 7. 里程碑

| 里程碑 | 目标 | 预计完成 |
|--------|------|---------|
| M1 | embedded_stdlib 基础设施 | 2026-07-06 |
| M2 | 所有 core/*.aura 可编译 | 2026-07-15 |
| M3 | Math.abs 通过 Aura 路径执行 | 2026-07-08 |
| M4 | 移除 Rust native 冗余 | 2026-07-30 |
| M5 | AOT 预编译可选 | 2026-08-15 |

---

*本文档修正了之前的方向性错误：`core/*.aura` 必须是唯一真相源，Rust native 仅限 Layer 0-A 引导层。*
