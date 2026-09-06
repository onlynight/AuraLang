# AOT 与 FFI 集成设计评估

> **核心问题**：当 Aura 调用 AOT 编译生成的机器码时，是否必须走 FFI？是否要为 Aura 自己再加一层 FFI？还是设计一套更简洁高效的调用方案？
>
> **结论前置**：**不需要为 AOT 调用再加 FFI**。当前 AOT（生成独立可执行文件）与 VM/JIT（运行在同一进程）是两条互不相通的路径；真正要做的是让 AOT 产出的机器码**嵌入到字节码模块里**，由 VM 通过共享调用约定直接调用——这才是"AOT 与 VM/JIT 同一运行时"的正确形态。

---

## 1. 现状分析

### 1.1 四种执行路径

| 路径 | 产出物 | 调用方式 | 现状 |
|------|--------|----------|------|
| VM 解释 | `.auc` 字节码 | `interp.rs` 栈式分发 | ✅ 完成 |
| JIT 编译 | 运行时机器码 | `JitEntry = extern "C" fn(*const JitValue, *mut JitValue, usize, *const ())` | ✅ 完成（Cranelift） |
| AOT 编译 | 独立 `.exe` / ELF | `std::process::Command::spawn` | ✅ 完成（LLVM 后端） |
| C FFI | 外部动态库 | `dlsym` / `GetProcAddress` + i64 装箱 | ✅ 完成（P8） |

### 1.2 关键发现

1. **AOT 当前产出"独立进程"**：`aot/mod.rs::compile` 调用 `link_to_executable`，生成带 `main()` 的可执行文件，通过 `Command::spawn` 启动。它与 VM/JIT 完全无运行时联系。
2. **JIT 已经有共享调用约定**：`JitValue { tag: i64, payload: i64 }` + `JitEntry` 签名（见 `jit.rs:7-12`, `jit.rs:53`）。
3. **FFI 的 `CFuncPtr` 是另一套 ABI**：`ffi.rs:250` 定义为 `fn(i64×8) -> i64`，所有参数强制装箱为 i64，与 JIT 的 `JitValue` 不兼容。
4. **遗留问题已识别**：`docs/遗留问题与风险分析报告.md` #22 指出"所有参数强制转为 i64，f64/指针/结构体参数依赖平台 ABI 巧合"。

### 1.3 关键设计缺口

> AOT 产出的机器码**无法被同一进程内的 VM 直接调用**。
>
> 这意味着 NovaOS 场景下的"热重载"、"Actor 模型跨进程隔离"、"核心库 AOT + 脚本 JIT 混合执行"都无法真正实现——它们都被独立的进程边界切断了。

---

## 2. 问题本质：AOT 调用关系的三种模型

用户问题的本质是：**宿主进程（Host）如何调用 Aura 编译出的机器码？** 业界有三种经典模型。

### 模型 A：独立进程（Standalone Process）

```
┌──────────────────┐         ┌──────────────────┐
│  Host (aura.exe) │ spawn   │  foo.exe (AOT)   │
│                  │ ───────▶ │  main()          │
│                  │  stdio   │  独立运行时       │
└──────────────────┘         └──────────────────┘
```

- **现状**：当前 AOT 走的就是这条路。
- **FFI 需要吗？** ❌ 不需要。进程间通过 stdio/pipe/TCP 通信。
- **优点**：完全隔离、崩溃不影响宿主、交叉编译简单。
- **缺点**：无法共享内存/状态、热重载需重启、Actor 模型被进程边界切断。

### 模型 B：动态库 + FFI（Hosted via FFI）

```
┌──────────────────────────────────────────┐
│  Host (aura.exe)                         │
│  ├─ VM/JIT runtime                       │
│  ├─ dlopen("foo.so")  ──────┐            │
│  └─ dlsym("main") ──────────┤            │
└─────────────────────────────┼────────────┘
                              │ FFI (i64 装箱)
                              ▼
                    ┌─────────────────┐
                    │  foo.so / .dll  │
                    │  AOT 机器码     │
                    └─────────────────┘
```

- **FFI 需要吗？** ✅ 需要 `dlopen`/`dlsym`（这本身就是 FFI 的一种形态）。
- **优点**：标准、跨平台、调试方便。
- **缺点**：符号命名约束严格、每次调用有 i64 装箱开销、`Value` 类型无法跨边界共享。

### 模型 C：嵌入机器码（Embedded Machine Code）

```
┌──────────────────────────────────────────┐
│  Host (aura.exe)                         │
│  ├─ VM/JIT runtime                       │
│  ├─ .auc 加载器                          │
│  │   ├─ 字节码段                          │
│  │   └─ 机器码段 (mmap PROT_EXEC)        │
│  └─ 函数描述符表 ──▶ VM 分发器           │
└──────────────────────────────────────────┘
```

- **FFI 需要吗？** ❌ 不需要。机器码是模块的一部分，通过共享调用约定直接 `call`。
- **优点**：零 FFI 开销、共享内存、支持热重载、Actor 模型可真正落地。
- **缺点**：实现复杂、需设计共享 ABI、安全风险（PROT_EXEC 内存）。

---

## 3. 方案评估

### 3.1 方案 A：扩展 FFI 到 AOT 调用

让 VM 通过现有 `CFuncPtr` 调用 AOT 函数。

| 维度 | 评估 |
|------|------|
| 实现难度 | 低（复用现有 FFI 基础设施） |
| 性能开销 | 高（每次调用 i64 装箱 + 参数拷贝） |
| 类型安全 | 弱（所有参数强制 i64，与遗留问题 #22 一致） |
| 与 VM/JIT 集成 | 割裂（AOT 函数是"外部函数"，不是"本机函数"） |
| 跨平台 | 中（依赖 dlopen/GetProcAddress） |
| 安全风险 | 低（动态库天然隔离） |

**结论**：能跑，但本质是"用外部库的方式调用自己的代码"——架构异味，且每次调用都有装箱开销。**不推荐**。

### 3.2 方案 B：AOT → 动态库 + dlopen

AOT 增加 `OutputFormat::SharedLibrary`，宿主 `dlopen` 后通过约定入口调用。

| 维度 | 评估 |
|------|------|
| 实现难度 | 中（需要约定入口符号 + 初始化函数） |
| 性能开销 | 中（dlsym 一次，调用仍走 C ABI） |
| 类型安全 | 中（需约定 Aura 侧 ABI） |
| 与 VM/JIT 集成 | 弱（仍是"外部调用"） |
| 跨平台 | 高（动态库标准） |
| 安全风险 | 中（需校验加载的库） |

**结论**：比方案 A 干净，但仍需 FFI 层。**适合作为可选模式**，不是核心路径。**保留为 Tier 2**。

### 3.3 方案 C：AOT 机器码嵌入 `.auc`（推荐）

`.auc` 格式扩展为"字节码 + 机器码"双段，机器码 mmap 后由 VM 分发器直接调用。

| 维度 | 评估 |
|------|------|
| 实现难度 | 中高（需扩展 `.auc` 格式 + 设计共享 ABI + mmap 管理） |
| 性能开销 | **零 FFI 开销**（直接 `call` 指令） |
| 类型安全 | 强（与 JIT 共享 `JitValue` ABI） |
| 与 VM/JIT 集成 | **最强**（AOT 函数 = 预编译 JIT 函数，同一运行时） |
| 跨平台 | 高（mmap/mprotect 是 POSIX/Windows 标准） |
| 安全风险 | 中（PROT_EXEC 内存，但模块来自已签名 `.auc`） |

**结论**：**这是"AOT 与 VM/JIT 同一运行时"的正确形态**，也是 NovaOS 热重载/Actor 模型/混合执行的前提。**推荐作为 Tier 3 核心路径**。

### 3.4 方案 D：编译期直接链接

AOT 产出 `.o`，与宿主 Rust 代码在编译期链接为一个二进制。

| 维度 | 评估 |
|------|------|
| 实现难度 | 中（需要 Rust C ABI 桥接层） |
| 性能开销 | 零（编译期链接） |
| 类型安全 | 强（编译期检查） |
| 与 VM/JIT 集成 | 弱（AOT 函数是"本机函数"，但 VM 无法动态发现） |
| 跨平台 | 中（依赖 rustc + 目标平台链接器） |
| 安全风险 | 低（无运行时加载） |

**结论**：适合"纯 AOT 部署"场景，不适合"VM 动态调度"。**保留为 Tier 4 可选模式**。

### 3.5 方案对比矩阵

| 维度 | A: 扩展 FFI | B: 动态库 | **C: 嵌入机器码** | D: 编译期链接 |
|------|:-----------:|:---------:|:-----------------:|:-------------:|
| FFI 开销 | 高 | 中 | **零** | 零 |
| 类型安全 | 弱 | 中 | **强** | 强 |
| 与 VM/JIT 同一运行时 | ❌ | ❌ | **✅** | ❌ |
| 热重载 | ❌ | ⚠️ | **✅** | ❌ |
| Actor 跨进程 | ❌ | ⚠️ | **✅** | ❌ |
| 实现复杂度 | 低 | 中 | **中高** | 中 |
| 跨平台 | 中 | 高 | **高** | 中 |
| 安全风险 | 低 | 中 | 中 | 低 |

**核心差异**：只有方案 C 能让 AOT 函数成为 VM/JIT 运行时的"一等公民"，与 JIT 函数完全对称。

---

## 4. 推荐设计：分层 AOT 模型

采用**四层分层架构**，每层独立可选，互不冲突。

```
                    ┌─────────────────────────────────┐
                    │         Tier 4: 编译期链接       │  ← 纯 AOT 部署
                    │   (AOT .o + Rust 宿主 → 单体)    │
                    ├─────────────────────────────────┤
                    │         Tier 3: 嵌入机器码       │  ← 核心路径 (推荐)
                    │   (.auc = 字节码 + 机器码段)      │
                    ├─────────────────────────────────┤
                    │         Tier 2: 动态库           │  ← 可选扩展
                    │   (.so/.dll + dlopen/dlsym)      │
                    ├─────────────────────────────────┤
                    │         Tier 1: 独立进程         │  ← 当前实现
                    │   (独立 .exe，spawn 启动)        │
                    └─────────────────────────────────┘
```

### 4.1 Tier 1: Standalone AOT（保留当前实现）

- **适用**：独立部署、交叉编译、性能敏感的命令行工具
- **调用方式**：`std::process::Command::spawn`
- **FFI**：不需要
- **现状**：✅ 已实现（`aot/linker.rs`）

### 4.2 Tier 2: Shared Library AOT（新增，可选）

- **适用**：插件系统、第三方扩展、跨语言互操作
- **调用方式**：`dlopen` + `dlsym`
- **FFI**：需要（薄层，仅约定入口符号）
- **产出物**：`.so` / `.dll` / `.dylib`

约定的入口符号（与 Tier 3 一致，方便迁移）：

```c
// 模块初始化：注册函数描述符表到宿主运行时
int aura_module_init(void* runtime, const aura_func_desc* table, size_t count);

// 模块调用：按 func_idx 调用
void* aura_module_call(void* runtime, uint32_t func_idx, void* args, size_t argc);

// 模块反初始化
void aura_module_finalize(void* runtime);
```

### 4.3 Tier 3: Embedded AOT（新增，核心路径）

- **适用**：NovaOS 热重载、Actor 模型、核心库 AOT + 脚本 JIT 混合执行
- **调用方式**：VM 分发器直接 `call` 到 mmap 后的机器码
- **FFI**：**不需要**
- **产出物**：扩展后的 `.auc`（字节码 + 机器码双段）

**这是回答用户问题的核心方案**——AOT 机器码不需要 FFI，因为它与 VM/JIT 共享调用约定，被当作"预编译的 JIT 函数"对待。

### 4.4 Tier 4: Link-time AOT（新增，可选）

- **适用**：纯 AOT 部署、无 VM 依赖的场景
- **调用方式**：编译期链接，宿主直接调用
- **FFI**：不需要
- **产出物**：`.o` 目标文件，与宿主 Rust 代码链接

---

## 5. 关键技术设计（Tier 3 核心）

### 5.1 共享调用约定（Shared Calling Convention）

**关键洞察**：JIT 已经有 `JitEntry` 签名（`jit.rs:53`），AOT 机器码应该**直接复用**这个签名，而不是新造一套。

```rust
// 复用现有 JIT 调用约定
pub type JitEntry = unsafe extern "C" fn(
    args: *const JitValue,      // 参数数组
    ret:  *mut JitValue,        // 返回值
    argc: usize,                // 参数个数
    ctx:  *const (),            // 上下文（模块 ID / 运行时指针）
);

// JitValue 定义（jit.rs:7-12）
#[repr(C)]
pub struct JitValue {
    pub tag: i64,        // 类型标签
    pub payload: i64,    // 值载荷
}
```

**为什么复用 JIT ABI？**

1. VM 已经会调用 `JitEntry`，AOT 函数 = 预编译 JIT 函数，零适配成本
2. 避免遗留问题 #22 的"i64 装箱"陷阱——`JitValue` 是双字段结构，f64 走 `TAG_FLOAT` + `to_bits()`，指针走 `TAG_PTR` + 地址值
3. 与现有 `interp.rs` 的 JIT 派发路径完全对称

**类型标签扩展**（在现有 `TAG_INT/FLOAT/BOOL/NULL` 基础上）：

| Tag | 含义 | Payload |
|-----|------|---------|
| 0 | INT | i64 值 |
| 1 | FLOAT | f64 bits |
| 2 | BOOL | 0/1 |
| 3 | NULL | 0 |
| 4 | STR | 字符串对象指针 |
| 5 | PTR | 原始指针 |
| 6 | OBJ | 堆对象指针 |
| 7 | FUNC | 函数索引 |

### 5.2 `.auc` 格式扩展

当前 `.auc` 是纯字节码格式（`codegen/serialize.rs`）。扩展为多段结构：

```
┌─────────────────────────────────────────────────────┐
│ Magic: "AURABC3" (8 bytes)                           │
│ Version: u32                                          │
│ Flags: u32                                            │
│   bit 0: has_bytecode                                 │
│   bit 1: has_machine_code                             │
│   bit 2: has_debug_info                               │
│   bit 3: signed                                        │
├─────────────────────────────────────────────────────┤
│ Segment Header (N entries)                            │
│   ┌─────────────────────────────────────────────┐    │
│   │ segment_id: u32                              │    │
│   │ offset: u64                                  │    │
│   │ size: u64                                    │    │
│   │ flags: u32  (PROT_READ / PROT_EXEC / ...)    │    │
│   │ crc32: u32                                   │    │
│   └─────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────┤
│ Seg 0: Bytecode segment                               │
│ Seg 1: Machine code segment (PROT_EXEC)               │
│ Seg 2: Function descriptor table                     │
│ Seg 3: Debug info (optional)                          │
│ Seg 4: Signature (optional)                           │
└─────────────────────────────────────────────────────┘
```

### 5.3 函数描述符表（Function Descriptor Table）

每个 AOT 模块导出一个描述符表，VM 加载时注册到分发器：

```rust
#[repr(C)]
struct AuraFuncDesc {
    // 基本信息
    name_offset: u32,         // 指向 .auc 字符串池的偏移
    entry_offset: u64,        // 机器码段内的入口偏移
    num_args: u8,             // 参数个数
    arg_tag_mask: u8,         // 参数类型标签（bit-packed）
    return_tag: u8,           // 返回类型标签
    flags: u8,                // 见下
    // 调试信息（可选）
    source_line: u32,         // 源文件行号
    source_file_offset: u32,  // 源文件名偏移
}

// flags
const FUNC_EXPORT: u8 = 1 << 0;     // 模块导出函数
const FUNC_INIT: u8 = 1 << 1;       // 模块初始化函数
const FUNC_FINALIZE: u8 = 1 << 2;   // 模块反初始化函数
const FUNC_SUSPEND: u8 = 1 << 3;    // 挂起/恢复函数
```

### 5.4 内存映射与权限

加载流程：

```rust
// VM 加载 .auc
fn load_module(auc_path: &Path) -> Module {
    let data = fs::read(auc_path)?;
    let header = parse_header(&data);
    
    let mut segments = Vec::new();
    for seg in &header.segments {
        let data = &data[seg.offset as usize..][..seg.size as usize];
        let mut opts = mmap::Options::new();
        opts.prot(mmap::Protection::from_flags(
            seg.flags.contains(PROT_READ),
            seg.flags.contains(PROT_WRITE),
            seg.flags.contains(PROT_EXEC),   // 仅机器码段为 true
        ));
        let mapping = mmap::MMap::map_anon_with_opts(data.len(), opts)?;
        mapping.copy_from(data);
        segments.push((seg.id, mapping));
    }
    
    // 解析函数描述符表
    let funcs = parse_func_descriptors(&segments);
    
    Module { segments, funcs, header }
}
```

**安全措施**：

1. 仅机器码段映射为 `PROT_EXEC`，其他段 `PROT_READ`
2. `.auc` 必须通过签名验证（Tier 3 可选，但 NovaOS 场景强制）
3. 模块加载前校验 CRC32

### 5.5 与 VM/JIT 的集成点

VM 分发器新增分支：

```rust
// interp.rs 分发器
fn dispatch(&mut self, instr: &Instr) -> Result<(), VmError> {
    match instr.opcode {
        OP_CALL_FUNCTION => {
            let func_idx = instr.func_idx();
            
            // 1. 检查是否有 JIT 编译版本
            if let Some(jit_entry) = self.jit.dispatch_table.get(func_idx) {
                return self.call_jit_entry(*jit_entry, args);
            }
            
            // 2. 【新增】检查是否有 AOT 预编译版本
            if let Some(aot_entry) = self.aot.dispatch_table.get(func_idx) {
                return self.call_aot_entry(*aot_entry, args);
            }
            
            // 3. 回退到字节码解释
            self.call_bytecode(func_idx, args)
        }
        _ => ...
    }
}
```

**关键**：`call_aot_entry` 与 `call_jit_entry` 共用同一套 `JitValue` 参数传递逻辑——AOT 函数与 JIT 函数对 VM 而言**完全等价**。

### 5.6 调用约定图

```
VM 字节码                    JIT 机器码                  AOT 机器码
─────────────                ────────────                ────────────
OP_CALL_FUNCTION            
        │                   
        ▼                   
┌─────────────────┐         
│ dispatch_table  │         
│ [func_idx]      │         
└────────┬────────┘         
         │                 
    ┌────┴────┐             
    │ JIT?    │ AOT?       
    ▼         ▼             
JitEntry    AotEntry       
(*const      (*const       
 JitValue,    JitValue,    
 *mut         *mut         
 JitValue,    JitValue,    
 usize,       usize,       
 *const())    *const()))   
```

两者签名完全一致，VM 分发逻辑零差异。

---

## 6. 与现有 FFI 的关系

### 6.1 FFI 的边界（保留）

Aura 的现有 FFI（P8）用于**调用外部 C/Rust 库**，这是单向的"向外调用"：

```
Aura 代码  ──FFI──▶  外部 C 库 (raylib, libc, ...)
```

- `extern "c" fun DrawCircle(...)` → `declare @DrawCircle in LLVM IR`
- `ffi.rs` 的 `CFuncPtr` 用于 VM/JIT 模式下调用外部函数
- **这部分不变，继续保留**

### 6.2 AOT 调用的边界（新增）

AOT 调用是**宿主调用 Aura 代码**，这是另一条方向：

```
宿主运行时  ──共享调用约定──▶  AOT 机器码 (Aura 代码)
```

- 不走 FFI，不走 dlsym
- 走 `JitEntry` 签名 + 函数描述符表
- 机器码是 `.auc` 的一部分，不是外部库

### 6.3 两者共存

一个 AOT-compiled Aura 模块**本身可以包含 FFI 调用**：

```
宿主运行时 ──▶ AOT 模块 ──FFI──▶ 外部 C 库
```

这是三层调用链，每层职责清晰：
- 第一层（宿主 → AOT）：共享调用约定，无 FFI
- 第二层（AOT → 外部 C）：现有 FFI 机制

**关键结论**：FFI 和 AOT 调用是**正交的两层**，不需要互相扩展。

---

## 7. 实施路线

### 7.1 Phase 1: 基础设施（MVP，约 2 周）

| 任务 | 文件 | 说明 |
|------|------|------|
| 1.1 扩展 `.auc` 格式 | `codegen/serialize.rs` | 多段结构 + 机器码段 |
| 1.2 定义 `AuraFuncDesc` | `codegen/aot/mod.rs` | C ABI 结构体 |
| 1.3 AOT 输出机器码段 | `codegen/aot/linker.rs` | 新增 `link_to_blob` |
| 1.4 VM 加载器 | `vm/module_loader.rs`（新增） | mmap + 权限管理 |
| 1.5 分发器分支 | `vm/interp.rs` | `OP_CALL_FUNCTION` 查 AOT 表 |

**里程碑**：简单函数（`fun add(a: Int, b: Int): Int`）能 AOT 编译后在 VM 中直接调用。

### 7.2 Phase 2: 完整调用约定（约 3 周）

| 任务 | 文件 | 说明 |
|------|------|------|
| 2.1 `JitValue` 类型标签扩展 | `vm/jit.rs` | STR/PTR/OBJ/FUNC tags |
| 2.2 AOT emit 生成 JitValue ABI | `codegen/aot/emit.rs` | 函数入口包装 |
| 2.3 闭包/捕获支持 | `codegen/aot/emit.rs` | 复用现有闭包表 |
| 2.4 并发函数支持 | `codegen/aot/emit.rs` | Actor/Channel 原生调用 |
| 2.5 字符串/集合支持 | `codegen/aot/emit.rs` | 堆对象管理 |

**里程碑**：完整 Aura 程序能 AOT 编译后在 VM 中执行，与 JIT 性能相当。

### 7.3 Phase 3: 安全与优化（约 2 周）

| 任务 | 文件 | 说明 |
|------|------|------|
| 3.1 `.auc` 签名验证 | `codegen/serialize.rs` | Ed25519 签名 |
| 3.2 模块沙箱 | `vm/module_loader.rs` | 段权限隔离 |
| 3.3 热重载支持 | `vm/module_loader.rs` | 模块重载 API |
| 3.4 调试信息 | `codegen/aot/dwarf.rs` | DWARF 与机器码段关联 |
| 3.5 Tier 2 动态库模式 | `codegen/aot/linker.rs` | `OutputFormat::SharedLibrary` |

**里程碑**：NovaOS 热重载、Actor 跨进程、插件系统全部可用。

---

## 8. 风险与权衡

### 8.1 安全风险

**PROT_EXEC 内存**：AOT 机器码段映射为可执行内存，是潜在攻击面。

**缓解**：
- `.auc` 必须签名验证（Tier 3 强制）
- 仅机器码段为 PROT_EXEC，其他段只读
- 加载前校验 CRC32
- NovaOS 场景下，模块来自已签名 `.auz` 包

### 8.2 复杂度风险

**实现复杂度**：方案 C 需要扩展 `.auc` 格式、设计共享 ABI、管理 mmap 内存。

**缓解**：
- 分三阶段实施，每阶段独立可用
- Phase 1 仅支持简单函数，Phase 2 逐步扩展
- 复用现有 JIT 的 `JitValue`/`JitEntry` 签名，不新造 ABI

### 8.3 兼容性风险

**`.auc` 格式变更**：扩展后旧版 VM 无法加载新版 `.auc`。

**缓解**：
- 格式版本号 `AURABC3`（当前是 `AURABC2`？）
- VM 加载器检查版本，不支持时给出明确错误
- Tier 1/2 路径保持向后兼容

### 8.4 为什么不直接复用现有 FFI？

用户问"是不是要给 Aura 自己也加上 FFI 才行"——答案是**已经有 FFI（P8），但它是为"调用外部 C"设计的，不适合"宿主调用自己的 AOT 代码"**。

如果强行复用 FFI：
1. 每次调用都有 i64 装箱开销（遗留问题 #22）
2. AOT 函数被当作"外部函数"，无法享受 VM/JIT 的一等公民待遇
3. 与 JIT 的 `JitValue` ABI 不兼容，两套调用约定并存

正确做法是：**AOT 调用走共享调用约定（JitValue ABI），FFI 保留用于调用外部 C**。两者正交，互不干扰。

---

## 9. 总结

| 问题 | 回答 |
|------|------|
| 调用 AOT 机器码需要 FFI 吗？ | **不需要**。通过共享调用约定（复用 JIT 的 `JitValue`/`JitEntry` 签名）+ 嵌入机器码到 `.auc`，VM 直接 `call`。 |
| 要给 Aura 自己加 FFI 吗？ | **不需要**。现有 FFI（P8）保留用于调用外部 C/Rust 库；AOT 调用走另一条路径。 |
| 更简洁高效的方案？ | **Tier 3 嵌入机器码**：`.auc` 扩展为"字节码 + 机器码"双段，机器码 mmap 后由 VM 分发器直接调用，零 FFI 开销，与 JIT 完全对称。 |
| 分层设计？ | Tier 1 独立进程（保留）+ Tier 2 动态库（可选）+ Tier 3 嵌入机器码（核心）+ Tier 4 编译期链接（可选）。 |

**核心理念**：AOT 机器码不是"外部代码"，而是"预编译的 JIT 代码"。它与 JIT 函数共享同一套调用约定、同一个 VM 分发器、同一个运行时——这才是"AOT + JIT 混合编译"的真正含义。

---

## 附录：相关文件索引

| 文件 | 相关度 | 说明 |
|------|--------|------|
| `compiler/src/vm/jit.rs` | ⭐⭐⭐ | `JitValue` / `JitEntry` 定义，AOT 复用此 ABI |
| `compiler/src/vm/ffi.rs` | ⭐⭐ | 现有 FFI 实现，保留用于外部 C 调用 |
| `compiler/src/codegen/aot/mod.rs` | ⭐⭐⭐ | AOT 编译入口，需扩展 `OutputFormat` |
| `compiler/src/codegen/aot/linker.rs` | ⭐⭐⭐ | 链接器，需新增 `link_to_blob` |
| `compiler/src/codegen/serialize.rs` | ⭐⭐⭐ | `.auc` 格式，需扩展为多段结构 |
| `compiler/src/vm/interp.rs` | ⭐⭐⭐ | VM 分发器，需新增 AOT 分支 |
| `docs/遗留问题与风险分析报告.md` | ⭐⭐ | 遗留问题 #22（i64 装箱）是本设计要规避的 |
| `docs/jit优化指南.md` | ⭐⭐ | JIT 与 AOT 的关系说明，本设计是其自然延伸 |
