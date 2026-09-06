# AOT 机器码嵌入方案 —— 详细设计文档

> **目标**：让 AOT 编译的机器码与 VM/JIT 运行在同一进程内，由 VM 分发器直接调用，零 FFI 开销。
>
> **核心思路**：AOT 机器码 = 预编译的 JIT 函数，复用现有 `JitValue`/`JitEntry` 调用约定，嵌入 `.auc` 文件。
>
> **前置阅读**：[AOT 与 FFI 集成设计评估](./AOT与FFI集成设计评估.md)

---

## 目录

1. [设计目标与约束](#1-设计目标与约束)
2. [总体架构](#2-总体架构)
3. [`.auc` 格式扩展（v4）](#3-auc-格式扩展v4)
4. [共享调用约定（JitValue ABI）](#4-共享调用约定jitvalue-abi)
5. [函数描述符表](#5-函数描述符表)
6. [AOT 后端改动](#6-aot-后端改动)
7. [VM 加载器与分发器改动](#7-vm-加载器与分发器改动)
8. [安全模型](#8-安全模型)
9. [分阶段实施计划](#9-分阶段实施计划)
10. [测试策略](#10-测试策略)
11. [附录：文件改动清单](#11-附录文件改动清单)

---

## 1. 设计目标与约束

### 1.1 功能目标

| 目标 | 说明 |
|------|------|
| G1: 零 FFI 开销 | AOT 机器码由 VM 直接 `call`，不走 dlsym/CFuncPtr |
| G2: 与 JIT 对称 | AOT 函数与 JIT 函数共享同一套调用约定、同一个分发器 |
| G3: 热重载 | 运行时可卸载/重载 AOT 模块，不重启进程 |
| G4: Actor 模型 | AOT 模块可被 Actor 直接引用，无进程边界 |
| G5: 向后兼容 | v3 `.auc` 仍可加载（无机器码段时走原路径） |

### 1.2 非目标（Non-goals）

- ❌ 不替代 Tier 1（独立进程 AOT）—— 保留用于部署场景
- ❌ 不替代现有 FFI（P8）—— FFI 继续用于调用外部 C/Rust 库
- ❌ 不实现 JIT 与 AOT 的混合优化 —— 每个函数要么 JIT 要么 AOT
- ❌ 不改变 Tier 4（编译期链接）—— 与本方案正交

### 1.3 关键约束

1. **C ABI 兼容**：`AuraFuncDesc` 必须 `#[repr(C)]`，字段对齐可预测
2. **跨平台**：mmap/mprotect 在 Windows（VirtualAlloc）和 POSIX 上都要工作
3. **无 LLVM 运行时依赖**：AOT emit 仍输出文本 IR，不引入 inkwell
4. **安全**：机器码段 PROT_EXEC，其他段只读；签名验证可选但推荐

---

## 2. 总体架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                        编译时（compile time）                        │
│                                                                     │
│  .aura 源码                                                         │
│      │                                                              │
│      ▼                                                              │
│  HIR ──┬──▶ MIR ──▶ 字节码 (.auc v4 的 bytecode 段)                 │
│        │                                                             │
│        └──▶ LLVM IR ──▶ llc ──▶ 机器码 blob (.auc v4 的 code 段)    │
│                                    │                                │
│                                    ▼                                │
│                          生成 AuraFuncDesc 描述符表                  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│                        运行时（runtime）                             │
│                                                                     │
│  VM 加载器                                                           │
│      │                                                              │
│      ▼                                                              │
│  解析 .auc v4 头部                                                   │
│      │                                                              │
│      ├──▶ mmap(bytecode 段, PROT_READ)                              │
│      ├──▶ mmap(code 段, PROT_EXEC)                                  │
│      └──▶ 解析 func_desc 表                                          │
│              │                                                      │
│              ▼                                                      │
│  VM 分发器 (interp.rs)                                               │
│      │                                                              │
│      ├──▶ CallNative ──▶ FFI 路径 (现有 P8)                         │
│      ├──▶ CallJit ──────▶ JitEntry (现有 Cranelift)                 │
│      └──▶ CallAot ──────▶ AotEntry (新增, = 预编译 JitEntry)       │
│                              │                                      │
│                              ▼                                      │
│                    共享调用约定 (JitValue ABI)                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.1 调用路径对比

```
现有路径：
VM ──CallNative──▶ FFI (dlsym) ──▶ 外部 C 库
VM ──CallJit─────▶ JitEntry ──▶ JIT 机器码

新增路径：
VM ──CallAot─────▶ AotEntry ──▶ AOT 机器码 (= 预编译 JitEntry)
```

**关键**：`AotEntry` 和 `JitEntry` 是同一个类型别名，VM 分发逻辑零差异。

---

## 3. `.auc` 格式扩展（v4）

### 3.1 向后兼容策略

- v3 格式：`MAGIC = "AURA"` + `VERSION = 3`，无机器码段
- v4 格式：`MAGIC = "AURA"` + `VERSION = 4`，新增机器码段 + 描述符表段
- v3 `.auc` 在 v4 VM 中仍可加载（机器码段 size=0）
- v4 `.auc` 在 v3 VM 中拒绝加载（版本号检查）

### 3.2 完整二进制布局

```
偏移   长度   字段              说明
──────────────────────────────────────────────────────────────
0x00   4     magic             "AURA"
0x04   2     version           u16 = 4
0x06   4     header_flags      u32 bitfield (见 §3.3)
0x0A   2     module_name_len   u16
0x0C   N     module_name       UTF-8 bytes
       2     module_version_len u16
       N     module_version    UTF-8 bytes
       16    uuid              16 bytes
──────────────────────────────────────────────────────────────
       4     consts_count      u32
       ...   consts            常量池 (格式同 v3)
       2     natives_count     u16
       ...   natives           原生函数表 (格式同 v3)
       2     funcs_count       u16
       ...   funcs             函数表 (见 §3.4)
       2     entry             u16
       2     entry_kind_len    u16
       N     entry_kind        UTF-8
       2     exports_count     u16
       ...   exports           导出符号表 (格式同 v3)
       2     imports_count     u16
       ...   imports           导入符号表 (格式同 v3)
       2     deps_count        u16
       ...   deps              依赖列表 (格式同 v3)
       2     sig_ids_count     u16
       ...   sig_ids           签名 ID 列表 (格式同 v3)
       2     enabled_modules_count u16
       ...   enabled_modules   启用的 std 模块 (格式同 v3)
──────────────────────────────────────────────────────────────
  ▼▼▼  v4 新增段  ▼▼▼

       2     segments_count    u16  (0 = 纯字节码模块)
       ...   segments          段表 (见 §3.5)
──────────────────────────────────────────────────────────────
  ▼▼▼  段数据区  ▼▼▼

       ...   segment data      各段数据 (offset/size 由段表指定)
──────────────────────────────────────────────────────────────
  ▼▼▼  尾部签名  ▼▼▼

       64    signature         Ed25519 签名 (可选, 见 §8)
       32    public_key        签名公钥 (可选)
```

### 3.3 header_flags 位定义

```rust
// serialize.rs
pub const HEADER_HAS_MACHINE_CODE: u32 = 1 << 0;   // v4 新增
pub const HEADER_HAS_DEBUG_INFO: u32   = 1 << 1;   // v4 新增
pub const HEADER_SIGNED: u32           = 1 << 2;   // v4 新增
pub const HEADER_AOT_EXPORTS: u32      = 1 << 3;   // v4 新增: 模块有 AOT 导出函数
```

### 3.4 函数表扩展（v4）

v3 的 `BytecodeFunction` 格式：

```
       2     name_len      u16
       N     name          UTF-8
       2     param_count   u16
       2     locals        u16
       1     is_native     u8 (0/1)
       4     code_len      u32
       N     code          字节码
```

v4 扩展（在 `is_native` 之后、`code_len` 之前插入）：

```
       2     name_len      u16
       N     name          UTF-8
       2     param_count   u16
       2     locals        u16
       1     is_native     u8 (0/1)
  ┌─── 1     aot_mode      u8 (v4 新增)
  │     ├── 0 = bytecode only
  │     ├── 1 = aot only (无字节码回退)
  │     └── 2 = bytecode + aot (混合, 见 §3.6)
  │
  ┌─── 4     aot_desc_idx  u32 (v4 新增, aot_mode=0 时写 0)
  │       指向 func_desc 表中的索引, 0 = 无 AOT 版本
  │
       4     code_len      u32 (aot_mode=1 时可写 0)
       N     code          字节码 (aot_mode=1 时无字节码)
```

### 3.5 段表（Segment Table）

每个段条目（16 字节，紧凑对齐）：

```rust
#[repr(C)]
struct AucSegment {
    id:       u32,   // 段 ID (枚举, 见下)
    offset:   u32,   // 相对于段数据区起始的偏移
    size:     u32,   // 段大小 (字节)
    flags:    u32,   // 内存权限标志
}
```

**段 ID 枚举**：

```rust
pub const SEG_BYTECODE:   u32 = 0;   // 字节码段 (PROT_READ)
pub const SEG_MACHINE:    u32 = 1;   // 机器码段 (PROT_EXEC)
pub const SEG_DESC_TABLE: u32 = 2;   // 函数描述符表 (PROT_READ)
pub const SEG_DEBUG:      u32 = 3;   // 调试信息段 (PROT_READ, 可选)
pub const SEG_STRING_POOL:u32 = 4;   // 字符串池 (PROT_READ, 描述符表引用)
pub const SEG_SIGNATURE:  u32 = 5;   // 签名段 (PROT_READ, 可选)
```

**段 flags**：

```rust
pub const SEG_PROT_READ:  u32 = 1 << 0;
pub const SEG_PROT_WRITE: u32 = 1 << 1;   // 仅用于 JIT 写回段 (可选)
pub const SEG_PROT_EXEC:  u32 = 1 << 2;
pub const SEG_READONLY:   u32 = 1 << 3;   // 加载后不可修改
```

### 3.6 混合模式（aot_mode=2）

当 `aot_mode = 2` 时，函数同时有字节码和 AOT 机器码版本。VM 加载时的行为：

1. 注册字节码到 `dispatch_table`
2. 注册 AOT entry 到 `aot_dispatch_table`
3. 首次调用走字节码
4. 调用计数达到阈值后，切换到 AOT 版本
5. 可配置策略：`prefer_bytecode` / `prefer_aot` / `threshold_based`

**Phase 1 不实现混合模式**，仅支持 `aot_mode=0`（纯字节码）和 `aot_mode=1`（纯 AOT）。

---

## 4. 共享调用约定（JitValue ABI）

### 4.1 JitValue 扩展

当前 `JitValue`（`vm/jit.rs:7-12`）仅支持 4 种标签：

```rust
#[repr(C)]
pub struct JitValue {
    pub tag: i64,        // 类型标签
    pub payload: i64,    // 值载荷
}
```

v4 扩展标签值（保持现有 4 个不变，追加新标签）：

```rust
// vm/jit.rs
pub const TAG_INT:    i64 = 0;   // payload = i64 值
pub const TAG_FLOAT:  i64 = 1;   // payload = f64.to_bits()
pub const TAG_BOOL:   i64 = 2;   // payload = 0/1
pub const TAG_NULL:   i64 = 3;   // payload = 0 (unused)
pub const TAG_STR:    i64 = 4;   // payload = 字符串对象指针 (v4 新增)
pub const TAG_PTR:    i64 = 5;   // payload = 原始指针地址 (v4 新增)
pub const TAG_OBJ:    i64 = 6;   // payload = 堆对象指针 (v4 新增)
pub const TAG_FUNC:   i64 = 7;   // payload = 函数索引 (v4 新增)
pub const TAG_ARRAY:  i64 = 8;   // payload = 数组对象指针 (v4 新增)
pub const TAG_LIST:   i64 = 9;   // payload = List 对象指针 (v4 新增)
pub const TAG_MAP:    i64 = 10;  // payload = Map 对象指针 (v4 新增)
pub const TAG_CLOSURE:i64 = 11;  // payload = 闭包对象指针 (v4 新增)
pub const TAG_CSTRING:i64 = 12;  // payload = *const c_char 地址 (v4 新增)
```

### 4.2 AotEntry 类型别名

```rust
// vm/jit.rs (或 vm/aot_runtime.rs)
/// AOT 机器码入口 —— 与 JitEntry 完全相同的签名
pub type AotEntry = unsafe extern "C" fn(
    args: *const JitValue,   // 参数数组 (指向调用者分配的内存)
    ret:  *mut JitValue,     // 返回值 (调用者分配, AOT 写入)
    argc: usize,             // 参数个数
    ctx:  *const (),         // 上下文 (模块 ID / 运行时指针)
);
```

**注意**：`AotEntry` 和 `JitEntry` 是同一个类型的两个别名，VM 分发逻辑完全一致。

### 4.3 上下文（ctx）结构

`ctx` 参数指向一个 `AotCallContext` 结构，提供 AOT 机器码访问 VM 运行时的能力：

```rust
// vm/aot_runtime.rs
#[repr(C)]
pub struct AotCallContext {
    /// VM 运行时指针 (非透明, 仅供 AOT 通过预生成调用使用)
    pub runtime: *mut AuraVm,
    /// 当前模块 ID
    pub module_id: u32,
    /// 当前函数索引 (用于调试/错误报告)
    pub func_idx: u32,
    /// 调用深度 (用于栈溢出检测)
    pub call_depth: u32,
    /// 异常状态 (0 = 正常, 非 0 = 异常码)
    pub exception: i32,
}
```

### 4.4 调用约定规则

1. **参数传递**：调用者分配 `JitValue` 数组，将指针传入 AOT 函数
2. **返回值**：AOT 函数将结果写入 `ret` 指向的 `JitValue`
3. **栈管理**：AOT 机器码自行管理栈（LLVM 生成），不与 VM 栈共享
4. **异常处理**：通过 `ctx.exception` 返回异常状态，VM 检查后抛出
5. **线程安全**：AOT 函数必须是无状态的（共享状态通过 `ctx.runtime` 访问）

---

## 5. 函数描述符表

### 5.1 AuraFuncDesc 结构

```rust
// vm/aot_runtime.rs
#[repr(C)]
pub struct AuraFuncDesc {
    /// 函数名在字符串池中的偏移
    pub name_offset: u32,
    /// 函数名长度 (不含 null 终止符)
    pub name_len: u16,

    /// 机器码段中的入口偏移 (相对于 SEG_MACHINE 段起始)
    pub entry_offset: u64,

    /// 参数个数
    pub num_args: u8,

    /// 参数类型标签位图 (bit-packed, 每参数 4 bit)
    /// 例: 3 参数 (Int, Float, Str) = 0b01_00_00 = 0x10
    pub arg_tags: u8,

    /// 返回类型标签
    pub return_tag: u8,

    /// 标志位 (见 §5.2)
    pub flags: u8,

    /// 源文件行号 (0 = 无调试信息)
    pub source_line: u32,

    /// 源文件名在字符串池中的偏移 (0 = 无调试信息)
    pub source_file_offset: u32,

    /// 保留字段 (对齐到 16 字节边界)
    pub _reserved: u32,
}

impl AuraFuncDesc {
    /// 结构体大小 (编译时断言)
    pub const SIZE: usize = 4 + 2 + 6 + 1 + 1 + 1 + 1 + 4 + 4 + 4 = 28;
    // 实际对齐到 32 字节 (考虑 u64 对齐)
}
```

**字段对齐验证**（x86-64 / ARM64）：

```
偏移  字段              大小  对齐
────  ────────────────  ────  ────
0x00  name_offset       4     4
0x04  name_len          2     2
0x06  entry_offset      8     8  ← 需要 8 字节对齐, 插入 2 字节 padding
0x0E  num_args          1     1
0x0F  arg_tags          1     1
0x10  return_tag        1     1
0x11  flags             1     1
0x12  source_line       4     4  ← 需要 4 字节对齐, 插入 2 字节 padding
0x16  source_file_offset 4    4
0x1A  _reserved         4     4
────
总大小: 30 字节 → 对齐到 32 字节
```

**修正后的布局**（显式 padding）：

```rust
#[repr(C, packed(4))]  // 或手动插入 padding 字段
pub struct AuraFuncDesc {
    pub name_offset: u32,       // 0x00, 4 bytes
    pub name_len: u16,          // 0x04, 2 bytes
    pub _pad1: u16,             // 0x06, 2 bytes (padding for u64 alignment)
    pub entry_offset: u64,      // 0x08, 8 bytes
    pub num_args: u8,           // 0x10, 1 byte
    pub arg_tags: u8,           // 0x11, 1 byte
    pub return_tag: u8,         // 0x12, 1 byte
    pub flags: u8,              // 0x13, 1 byte
    pub source_line: u32,       // 0x14, 4 bytes
    pub source_file_offset: u32,// 0x18, 4 bytes
    pub _pad2: u32,             // 0x1C, 4 bytes (align to 32)
}
// 总大小: 32 字节
```

### 5.2 flags 位定义

```rust
pub const FUNC_EXPORT:     u8 = 1 << 0;   // 模块导出函数
pub const FUNC_INIT:       u8 = 1 << 1;   // 模块初始化函数 (加载时自动调用)
pub const FUNC_FINALIZE:   u8 = 1 << 2;   // 模块反初始化函数 (卸载时调用)
pub const FUNC_SUSPEND:    u8 = 1 << 3;   // 挂起/恢复函数
pub const FUNC_ASYNC:      u8 = 1 << 4;   // 异步函数 (返回 Future)
pub const FUNC_CONST:      u8 = 1 << 5;   // 纯函数 (无副作用, 可内联)
pub const FUNC_THREAD_SAFE: u8 = 1 << 6;  // 线程安全 (无共享可变状态)
pub const FUNC_HOT:        u8 = 1 << 7;   // 热点函数 (JIT 编译候选)
```

### 5.3 arg_tags 位图编码

每个参数占 4 bit（支持 16 种标签），最多 16 个参数：

```
bit 0-3:   arg[0] 的 tag
bit 4-7:   arg[1] 的 tag
...
bit 60-63: arg[15] 的 tag
```

但 `arg_tags` 是 `u8`，只能编码 2 个参数。对于超过 2 个参数的函数，使用变长编码：

```rust
// 描述符中 arg_tags 的低 4 bit 是"参数类型描述符的字节数"
// 如果 arg_tags & 0x0F == 0，表示使用紧凑编码（最多 2 参数）
// 否则，从字符串池中读取完整类型描述符

// 紧凑编码（最多 2 参数，每参数 4 bit）:
//   arg_tags = (tag[1] << 4) | tag[0]
// 例: (Int, Float) = (1 << 4) | 0 = 0x10

// 扩展编码（超过 2 参数）:
//   arg_tags 低 4 bit = 描述符长度 N
//   从字符串池读取 N 字节, 每字节一个 tag
```

**简化方案（推荐 Phase 1）**：限制 AOT 函数最多 4 个参数，每个 2 bit tag（仅支持 Int/Float/Bool/Null）。超出时使用扩展编码。

### 5.4 描述符表序列化

描述符表作为 `SEG_DESC_TABLE` 段存储，格式：

```
       2     count          u16 (描述符数量)
       ...   descriptors    count × 32 bytes (AuraFuncDesc)
```

### 5.5 字符串池

字符串池作为 `SEG_STRING_POOL` 段存储，格式：

```
       4     offset_0       u32 (字符串 0 的偏移)
       4     offset_1       u32 (字符串 1 的偏移)
       ...
       4     offset_N       u32
       N     data           所有字符串数据 (null 终止)
```

函数名、源文件名都通过 `name_offset` / `source_file_offset` 引用字符串池。

---

## 6. AOT 后端改动

### 6.1 新增 OutputFormat::Blob

```rust
// codegen/aot/mod.rs
pub enum OutputFormat {
    LlvmIr,
    Object,
    Executable,       // 现有: 独立可执行文件
    SharedLibrary,    // 新增: 动态库 (Tier 2)
    Blob,             // 新增: 机器码 blob (Tier 3, 嵌入 .auc)
}
```

### 6.2 link_to_blob 接口

```rust
// codegen/aot/linker.rs
/// 将 LLVM IR 编译为机器码 blob (不链接, 不生成可执行文件)
///
/// 输出: 原始机器码字节流, 包含所有函数的入口点
/// 调用约定: 每个函数入口遵循 JitValue ABI
pub fn link_to_blob(
    ll_path: &Path,
    blob_path: &Path,
    options: &AotOptions,
) -> Result<Vec<AotFunctionInfo>, AotError> {
    // 1. 调用 llc 生成目标文件 (与 link_to_object 相同)
    let object_path = blob_path.with_extension(if cfg!(target_os = "windows") { "obj" } else { "o" });
    link_to_object(ll_path, &object_path, options)?;

    // 2. 从目标文件中提取机器码段和符号表
    //    使用 llvm-objdump 或 llvm-readobj 解析目标文件
    //
    // 替代方案: 使用 llc -filetype=asm 生成汇编, 再提取 .text 段
    // 再替代方案: 使用 LLDB/ELF 解析器读取目标文件

    // 3. 返回函数信息列表
    Ok(func_infos)
}

/// 函数信息 (从目标文件中提取)
pub struct AotFunctionInfo {
    pub name: String,
    pub entry_offset: u64,     // 相对于 blob 起始的偏移
    pub size: u64,             // 函数大小
    pub is_export: bool,       // 是否导出
}
```

**实现策略**：

- **方案 A**：使用 `llvm-objdump -h` 解析目标文件的 section 信息
- **方案 B**：使用 `llvm-readobj -symbols` 提取符号表
- **方案 C**：使用 Rust crate `object`（https://crates.io/crates/object）解析 ELF/COFF/PE
- **推荐方案 C**：纯 Rust 实现，无外部依赖

### 6.3 AOT emit 改动：生成 JitValue ABI 入口

当前 `emit.rs` 生成的 LLVM IR 函数使用 Aura 原始类型。需要为每个导出函数生成一个**包装函数**，遵循 JitValue ABI：

```llvm
; 原始 Aura 函数 (内部调用)
define internal i32 @add(i32 %a, i32 %b) {
    %sum = add i32 %a, %b
    ret i32 %sum
}

; JitValue ABI 包装函数 (导出)
; @aura_add = JitEntry 包装
define internal i64 @aura_add(
    i64* %args,      ; 参数数组 (JitValue 数组)
    i64* %ret,       ; 返回值 (JitValue)
    i64 %argc,       ; 参数个数
    i64* %ctx        ; 上下文
) {
    ; 解包参数 0: args[0] = JitValue { tag=0(INT), payload=<a> }
    %arg0 = getelementptr i64, i64* %args, i64 0
    %arg0_tag = load i64, i64* %arg0
    %arg0_val = getelementptr i64, i64* %args, i64 1  ; payload
    %a = load i64, i64* %arg0_val
    %a_i32 = trunc i64 %a to i32

    ; 解包参数 1: args[1] = JitValue { tag=0(INT), payload=<b> }
    %arg1 = getelementptr i64, i64* %args, i64 2
    %arg1_val = getelementptr i64, i64* %args, i64 3
    %b = load i64, i64* %arg1_val
    %b_i32 = trunc i64 %b to i32

    ; 调用原始函数
    %sum = call i32 @add(i32 %a_i32, i32 %b_i32)

    ; 打包返回值: ret = JitValue { tag=0(INT), payload=<sum> }
    %ret_tag = getelementptr i64, i64* %ret, i64 0
    store i64 0, i64* %ret_tag    ; tag = INT
    %ret_val = getelementptr i64, i64* %ret, i64 1
    store i64 %sum, i64* %ret_val  ; payload = sum (zext to i64)

    ret i64 0  ; 返回码 (0 = 正常)
}
```

### 6.4 包装函数生成规则

| Aura 类型 | LLVM 类型 | JitValue tag | payload 编码 |
|-----------|-----------|--------------|-------------|
| Int | i32 / i64 | TAG_INT (0) | zext / trunc 到 i64 |
| Float | float / double | TAG_FLOAT (1) | bitcast 到 i64 |
| Bool | i1 | TAG_BOOL (2) | zext 到 i64 |
| Unit | — | TAG_NULL (3) | 0 |
| String | { i8*, i64 } | TAG_STR (4) | 结构体指针 |
| Char | i32 | TAG_INT (0) | zext 到 i64 |
| Pointer | ptr | TAG_PTR (5) | 地址值 |
| Array/List/Map | struct* | TAG_OBJ (6) | 对象指针 |

### 6.5 emit.rs 改动清单

```rust
// codegen/aot/emit.rs

/// 新增: 为每个导出函数生成 JitValue ABI 包装
pub fn emit_wrappers(program: &HirProgram, type_mapper: &TypeMapper) -> String {
    let mut ir = String::new();

    // JitValue 结构体定义
    ir.push_str(
        "define internal i64 @aura_add(\n",
        "  i64* %args, i64* %ret, i64 %argc, i64* %ctx\n",
        ") {\n",
        "    ; ... 参数解包 + 调用 + 返回值打包 ...\n",
        "    ret i64 0\n",
        "}\n",
    );

    ir
}

/// 新增: 生成 AuraFuncDesc 初始化代码
pub fn emit_func_desc_table(program: &HirProgram) -> Vec<AuraFuncDescInit> {
    program.functions.iter()
        .filter(|f| f.is_export)
        .map(|f| AuraFuncDescInit {
            name: f.name.clone(),
            entry_offset: f.mangled_name_offset,  // 需从目标文件符号表获取
            num_args: f.params.len() as u8,
            arg_tags: encode_arg_tags(&f.params),
            return_tag: encode_return_tag(&f.return_type),
            flags: func_flags(f),
            source_line: f.source_line,
            source_file_offset: f.source_file_offset,
        })
        .collect()
}
```

---

## 7. VM 加载器与分发器改动

### 7.1 新增模块：vm/aot_runtime.rs

```rust
// vm/aot_runtime.rs

use std::collections::HashMap;
use crate::vm::jit::{JitValue, JitEntry};
use crate::vm::interp::AuraVm;

/// AOT 机器码入口 (与 JitEntry 相同签名)
pub type AotEntry = unsafe extern "C" fn(
    args: *const JitValue,
    ret:  *mut JitValue,
    argc: usize,
    ctx:  *const (),
);

/// 上下文结构
#[repr(C)]
pub struct AotCallContext {
    pub runtime: *mut AuraVm,
    pub module_id: u32,
    pub func_idx: u32,
    pub call_depth: u32,
    pub exception: i32,
}

/// AOT 模块
pub struct AotModule {
    pub module_id: u32,
    pub name: String,
    pub segments: Vec<MemorySegment>,
    pub func_descriptors: Vec<AuraFuncDesc>,
    pub func_dispatch_table: Vec<Option<AotEntry>>,
}

/// 内存段
pub struct MemorySegment {
    pub id: u32,
    pub base: usize,
    pub size: usize,
    pub flags: u32,
    #[cfg(unix)]
    pub mapping: mmap::MMap,
    #[cfg(windows)]
    pub handle: usize,  // VirtualAlloc 返回的句柄
}

impl AotModule {
    /// 从 .auc 文件加载 AOT 模块
    pub fn load(auc_bytes: &[u8], module_id: u32) -> Result<Self, AotError> {
        // 1. 解析 .auc v4 头部
        // 2. mmap 机器码段 (PROT_EXEC)
        // 3. 解析函数描述符表
        // 4. 计算每个函数的绝对入口地址
        // 5. 填充 dispatch_table
        todo!()
    }

    /// 卸载模块 (释放 mmap 内存)
    pub fn unload(self) {
        for seg in &self.segments {
            // munmap / VirtualFree
        }
    }

    /// 查找函数的 AOT entry
    pub fn find_entry(&self, func_idx: usize) -> Option<AotEntry> {
        self.func_dispatch_table.get(func_idx).copied().unwrap_or(None)
    }
}

/// AOT 运行时管理器
pub struct AotRuntime {
    modules: HashMap<u32, AotModule>,
    next_module_id: u32,
}

impl AotRuntime {
    pub fn new() -> Self { ... }

    /// 加载模块
    pub fn load_module(&mut self, auc_path: &Path) -> Result<u32, AotError> { ... }

    /// 卸载模块
    pub fn unload_module(&mut self, module_id: u32) -> Result<(), AotError> { ... }

    /// 调用 AOT 函数
    pub unsafe fn call_func(
        &mut self,
        module_id: u32,
        func_idx: usize,
        args: &[JitValue],
    ) -> Result<JitValue, AotError> {
        let module = self.modules.get(&module_id).ok_or(...)?;
        let entry = module.find_entry(func_idx).ok_or(...)?;

        let mut ret = JitValue::null();
        let ctx = AotCallContext {
            runtime: std::ptr::null_mut(),  // Phase 1: 暂不提供运行时访问
            module_id,
            func_idx: func_idx as u32,
            call_depth: 0,
            exception: 0,
        };

        (entry)(
            args.as_ptr(),
            &mut ret as *mut JitValue,
            args.len(),
            &ctx as *const _ as *const (),
        );

        Ok(ret)
    }
}
```

### 7.2 VM 分发器改动 (interp.rs)

当前 `do_call_native`（`interp.rs:576`）处理 FFI 调用。新增 `do_call_aot` 处理 AOT 调用：

```rust
// vm/interp.rs

impl AuraVm {
    // ... 现有分发器 ...

    fn dispatch(&mut self, instr: &Instr) -> Result<(), VmError> {
        match instr.opcode {
            // 现有: FFI 调用
            Instr::CallNative(idx) => self.do_call_native(top, idx as usize)?,
            Instr::CallC(idx) => self.do_call_native(top, idx as usize)?,

            // 现有: JIT 调用
            Instr::CallJit(idx) => self.do_call_jit(top, idx as usize)?,

            // 新增: AOT 调用
            Instr::CallAot(idx) => self.do_call_aot(top, idx as usize)?,

            // ... 其他指令 ...
        }
    }

    /// 调用 AOT 函数
    fn do_call_aot(&mut self, top: usize, idx: usize) -> Result<(), VmError> {
        // 1. 从栈上收集参数 (转换为 JitValue)
        let args = self.collect_args(top, idx);
        let jit_args: Vec<JitValue> = args.iter().map(|v| JitValue::from_value(v)).collect();

        // 2. 调用 AOT entry
        let mut ret = JitValue::null();
        let ctx = AotCallContext { ... };

        unsafe {
            let entry = self.aot_runtime.find_entry(self.current_module, idx);
            (entry)(jit_args.as_ptr(), &mut ret, jit_args.len(), &ctx as *const _ as *const ());
        }

        // 3. 将返回值推入栈
        self.push(ret.to_value());

        Ok(())
    }
}
```

### 7.3 新增指令：OP_CALL_AOT

```rust
// codegen/opcode.rs
pub enum OpCode {
    // ... 现有指令 ...
    CallAot(u16),   // v4 新增: 调用 AOT 编译的函数
}

// opcode 编号分配: 74 (现有最大为 73 CallClosure)
impl OpCode {
    pub fn to_byte(&self) -> u8 {
        match self {
            OpCode::CallAot(_) => 74,
            // ...
        }
    }

    pub fn from_byte(byte: u8) -> OpCode {
        match byte {
            74 => OpCode::CallAot(0),
            // ...
        }
    }
}
```

### 7.4 字节码发射器改动 (emit.rs)

当函数有 AOT 版本时，发射 `OP_CALL_AOT` 指令而非 `OP_CALL_NATIVE`：

```rust
// codegen/emit.rs
fn emit_call_aot(builder: &mut BytecodeBuilder, func_idx: u16, module_id: u16) {
    // 编码: OP_CALL_AOT | func_idx | module_id
    builder.emit(OpCode::CallAot(func_idx));
    builder.emit_u16(module_id);  // 目标模块 ID (0 = 当前模块)
}
```

---

## 8. 安全模型

### 8.1 内存权限隔离

| 段 | 权限 | 说明 |
|----|------|------|
| 字节码段 | PROT_READ | 只读, VM 分发器逐条读取 |
| 机器码段 | PROT_EXEC | 可执行, 包含 AOT 机器码 |
| 描述符表 | PROT_READ | 只读, VM 加载时解析 |
| 字符串池 | PROT_READ | 只读, 描述符表引用 |
| 调试信息 | PROT_READ | 只读, 调试器使用 |

**安全规则**：
- 机器码段映射后不可写（W^X 策略）
- 字节码段映射后不可执行（X^W 策略）
- 两个策略共同确保内存安全

### 8.2 签名验证（可选）

`.auc` v4 支持 Ed25519 签名：

```
签名格式:
  signature: 64 bytes (Ed25519)
  public_key: 32 bytes

签名内容:
  对 .auc 文件除签名段外的所有字节计算 SHA-256,
  然后用私钥签名。
```

**验证流程**：
1. 读取 .auc 文件
2. 分离签名段
3. 对剩余字节计算 SHA-256
4. 用公钥验证签名
5. 验证失败则拒绝加载

**NovaOS 场景**：强制签名验证；开发场景：可选。

### 8.3 调用深度限制

防止栈溢出：

```rust
// AotCallContext.call_depth
// VM 在每次调用前检查:
if ctx.call_depth > MAX_CALL_DEPTH {  // 默认 1024
    return Err(VmError::StackOverflow);
}
```

### 8.4 模块沙箱

Phase 1 不实现细粒度沙箱。后续可扩展：
- 文件 I/O 权限控制
- 网络访问控制
- 内存配额
- 执行时间限制

---

## 9. 分阶段实施计划

### Phase 1: MVP（2 周）

**目标**：简单函数（`fun add(a: Int, b: Int): Int`）能 AOT 编译后在 VM 中直接调用。

| 任务 | 文件 | 工期 | 说明 |
|------|------|------|------|
| 1.1 扩展 JitValue 标签 | `vm/jit.rs` | 0.5d | 添加 TAG_STR/PTR/OBJ/FUNC 等 |
| 1.2 定义 AuraFuncDesc | `vm/aot_runtime.rs` (新增) | 1d | C ABI 结构体 + 对齐验证 |
| 1.3 新增 AotEntry 类型 | `vm/aot_runtime.rs` | 0.5d | = JitEntry 别名 |
| 1.4 新增 AotModule/AotRuntime | `vm/aot_runtime.rs` | 2d | 模块加载/卸载/调用 |
| 1.5 mmap 封装 | `vm/mmap_util.rs` (新增) | 1d | 跨平台 mmap/VirtualAlloc |
| 1.6 扩展 .auc v4 格式 | `codegen/serialize.rs` | 2d | 多段结构 + 段表 |
| 1.7 新增 OutputFormat::Blob | `codegen/aot/mod.rs` | 0.5d | 枚举扩展 |
| 1.8 实现 link_to_blob | `codegen/aot/linker.rs` | 3d | llc → obj → blob 提取 |
| 1.9 生成 JitValue 包装 | `codegen/aot/emit.rs` | 2d | LLVM IR 包装函数 |
| 1.10 新增 OP_CALL_AOT | `codegen/opcode.rs` | 0.5d | 指令定义 |
| 1.11 字节码发射 AOT 调用 | `codegen/emit.rs` | 1d | emit_call_aot |
| 1.12 VM 分发器集成 | `vm/interp.rs` | 1d | do_call_aot |
| 1.13 CLI 集成 | `cli/src/main.rs` | 1d | --aot-embed 标志 |
| 1.14 集成测试 | `tests/aot_embed_tests.rs` | 1d | 端到端测试 |

**里程碑**：
- `aura compile foo.aura --aot-embed` 生成 .auc v4
- `aura run foo.auc` 能执行 AOT 编译的函数
- 简单 Int/Float 函数调用正确

### Phase 2: 完整调用约定（3 周）

**目标**：完整 Aura 程序能 AOT 编译后在 VM 中执行。

| 任务 | 文件 | 工期 | 说明 |
|------|------|------|------|
| 2.1 String 支持 | `codegen/aot/emit.rs` | 2d | TAG_STR 包装 |
| 2.2 Bool/Unit 支持 | `codegen/aot/emit.rs` | 1d | TAG_BOOL/TAG_NULL 包装 |
| 2.3 Pointer 支持 | `codegen/aot/emit.rs` | 1d | TAG_PTR 包装 |
| 2.4 闭包/捕获支持 | `codegen/aot/emit.rs` | 3d | 复用现有闭包表 |
| 2.5 集合操作支持 | `codegen/aot/emit.rs` | 2d | TAG_LIST/MAP/ARRAY |
| 2.6 异常处理 | `vm/aot_runtime.rs` | 2d | ctx.exception 传播 |
| 2.7 混合模式 | `vm/interp.rs` | 2d | aot_mode=2 |
| 2.8 函数描述符生成 | `codegen/aot/emit.rs` | 2d | emit_func_desc_table |
| 2.9 字符串池生成 | `codegen/aot/emit.rs` | 1d | 函数名/源文件名 |
| 2.10 集成测试 | `tests/aot_embed_full_tests.rs` | 2d | 全类型测试 |

**里程碑**：
- 字符串/集合/闭包在 AOT 模式下可用
- 异常处理正确传播
- 混合模式（字节码 + AOT）可用

### Phase 3: 安全与优化（2 周）

**目标**：生产可用，安全加固。

| 任务 | 文件 | 工期 | 说明 |
|------|------|------|------|
| 3.1 Ed25519 签名 | `codegen/serialize.rs` | 2d | 签名生成/验证 |
| 3.2 模块沙箱 | `vm/aot_runtime.rs` | 2d | 权限控制 |
| 3.3 热重载 | `vm/aot_runtime.rs` | 2d | 模块卸载/重载 API |
| 3.4 调试信息 | `codegen/aot/dwarf.rs` | 2d | DWARF + 机器码段关联 |
| 3.5 性能优化 | `vm/interp.rs` | 1d | 缓存 AOT entry 查找 |
| 3.6 错误报告 | `vm/aot_runtime.rs` | 1d | 详细错误信息 |
| 3.7 文档 | `docs/` | 1d | 用户文档 |
| 3.8 集成测试 | `tests/` | 1d | 安全/热重载测试 |

**里程碑**：
- .auc 签名验证可用
- 热重载不重启进程
- NovaOS 集成完成

### Phase 4: 高级特性（可选，2 周）

| 任务 | 文件 | 工期 | 说明 |
|------|------|------|------|
| 4.1 Tier 2 动态库模式 | `codegen/aot/linker.rs` | 3d | OutputFormat::SharedLibrary |
| 4.2 Tier 4 编译期链接 | `codegen/aot/linker.rs` | 3d | 与 Rust 宿主链接 |
| 4.3 跨模块调用 | `vm/aot_runtime.rs` | 2d | 多模块依赖解析 |
| 4.4 性能基准 | `bench/` | 2d | AOT vs JIT vs VM 对比 |
| 4.5 插件系统 | `vm/aot_runtime.rs` | 3d | 第三方模块加载 |

---

## 10. 测试策略

### 10.1 单元测试

| 测试文件 | 覆盖范围 |
|----------|----------|
| `tests/aot_runtime_tests.rs` | AuraFuncDesc 对齐、AotModule 加载/卸载 |
| `tests/aot_mmap_tests.rs` | mmap/VirtualAlloc 封装 |
| `tests/aot_serialize_tests.rs` | .auc v4 序列化/反序列化 |
| `tests/aot_linker_tests.rs` | link_to_blob 生成正确性 |
| `tests/aot_emit_tests.rs` | LLVM IR 包装函数生成 |

### 10.2 集成测试

| 测试文件 | 覆盖范围 |
|----------|----------|
| `tests/aot_embed_tests.rs` | 端到端：源码 → AOT → VM 调用 |
| `tests/aot_embed_full_tests.rs` | 全类型：String/Float/Bool/Pointer/Collection |
| `tests/aot_embed_security_tests.rs` | 签名验证、权限隔离 |
| `tests/aot_embed_hot_reload_tests.rs` | 热重载不重启进程 |
| `tests/aot_embed_perf_tests.rs` | 性能基准：AOT vs JIT vs VM |

### 10.3 回归测试

现有测试必须保持通过：
- `tests/ffi_*_tests.rs`：FFI 功能不变
- `tests/vm_*_tests.rs`：VM 解释器不变
- `tests/jit_*_tests.rs`：JIT 编译不变
- `tests/aot_*_tests.rs`：现有 AOT 测试不变（独立进程模式）

### 10.4 跨平台测试

| 平台 | CI 配置 |
|------|---------|
| Linux x86_64 | GitHub Actions ubuntu-latest |
| macOS aarch64 | GitHub Actions macos-latest |
| Windows x86_64 | GitHub Actions windows-latest |
| Linux aarch64 | GitHub Actions ubuntu-latest (QEMU) |
| Raspberry Pi 4 (aarch64) | 手动测试 |
| Raspberry Pi 3 (armv7) | 手动测试 |

---

## 11. 附录：文件改动清单

### 11.1 新增文件

| 文件 | 说明 |
|------|------|
| `vm/aot_runtime.rs` | AOT 运行时：AotModule, AotRuntime, AuraFuncDesc |
| `vm/mmap_util.rs` | 跨平台 mmap/VirtualAlloc 封装 |
| `codegen/aot/blob.rs` | 机器码 blob 解析（目标文件 → blob） |
| `tests/aot_embed_tests.rs` | 端到端集成测试 |
| `tests/aot_embed_full_tests.rs` | 全类型测试 |
| `tests/aot_embed_security_tests.rs` | 安全测试 |
| `tests/aot_embed_hot_reload_tests.rs` | 热重载测试 |
| `tests/aot_embed_perf_tests.rs` | 性能基准 |

### 11.2 修改文件

| 文件 | 改动 |
|------|------|
| `codegen/serialize.rs` | 扩展 .auc v4 格式（多段结构） |
| `codegen/opcode.rs` | 新增 OP_CALL_AOT 指令 |
| `codegen/emit.rs` | 新增 emit_call_aot |
| `codegen/aot/mod.rs` | 新增 OutputFormat::Blob |
| `codegen/aot/linker.rs` | 新增 link_to_blob |
| `codegen/aot/emit.rs` | 生成 JitValue ABI 包装函数 |
| `vm/jit.rs` | 扩展 JitValue 标签 |
| `vm/interp.rs` | 新增 do_call_aot 分发 |
| `cli/src/main.rs` | 新增 --aot-embed 标志 |
| `Cargo.toml` | 新增 `object` crate 依赖 |

### 11.3 不修改文件

| 文件 | 原因 |
|------|------|
| `vm/ffi.rs` | 现有 FFI 保持不变，用于外部 C 调用 |
| `vm/jit.rs` (JitEntry) | 保持现有签名，仅扩展标签值 |
| `codegen/aot/c_backend.rs` | C 后端不改动 |
| `codegen/aot/dwarf.rs` | DWARF 生成不变（Phase 3 扩展） |

### 11.4 关键不变量

1. **FFI 不变**：`ffi.rs` 的 `CFuncPtr`、`resolve_static_symbol`、`aura_callback_trampoline` 全部保留
2. **JIT 不变**：`JitEntry` 签名不变，仅扩展 `JitValue` 标签值
3. **现有 AOT 不变**：`OutputFormat::Executable` 继续工作，独立进程模式保留
4. **字节码格式向后兼容**：v3 `.auc` 在 v4 VM 中可加载

---

## 附录 A：设计决策记录

### A.1 为什么复用 JitValue 而非新造 ABI？

**问题**：AOT 机器码应该使用什么调用约定？

**选项**：
- A: 复用 JIT 的 `JitValue`/`JitEntry`
- B: 新造一套 `AotValue`/`AotEntry`
- C: 使用 LLVM 原始类型（`i32`/`f64` 直接传递）

**决策**：选择 A。

**理由**：
1. VM 已经会调用 `JitEntry`，零适配成本
2. `JitValue` 是双字段结构，能安全传递 f64（`to_bits()`）、指针（地址值）、对象（对象指针）
3. 避免遗留问题 #22 的"i64 装箱"陷阱
4. AOT 函数 = 预编译 JIT 函数，语义对称

### A.2 为什么嵌入 .auc 而非动态库？

**问题**：AOT 机器码应该存放在哪里？

**选项**：
- A: 嵌入 .auc 文件（多段结构）
- B: 独立动态库（.so/.dll）
- C: 嵌入可执行文件（编译期链接）

**决策**：选择 A。

**理由**：
1. .auc 是 Aura 的模块格式，机器码是模块的一部分
2. 无需 dlopen/dlsym，零 FFI 开销
3. 支持热重载（卸载/重载 .auc 文件）
4. Actor 模型可直接引用 AOT 模块
5. 与 Tier 1（独立进程）和 Tier 2（动态库）正交，分层清晰

### A.3 为什么不使用 inkwell？

**问题**：AOT emit 应该使用 LLVM C API 绑定还是文本 IR？

**决策**：继续使用文本 IR（与现有实现一致）。

**理由**：
1. inkwell 0.10.0 最高支持 LLVM 19，当前项目使用 LLVM 23.1.0
2. 文本 IR 跨 LLVM 版本兼容
3. 实现简单、调试方便
4. 与 C 后端备选方案一致

### A.4 为什么限制参数类型标签为 4 bit？

**问题**：`arg_tags` 是 `u8`，每参数 4 bit 只能编码 16 种标签。

**决策**：Phase 1 限制最多 4 参数，每参数 2 bit tag（Int/Float/Bool/Null）。

**理由**：
1. 简单函数（如 `add`, `mul`）通常只有 2-3 个参数
2. 4 bit 标签足够覆盖常见类型
3. 超过 4 参数时使用扩展编码（Phase 2 实现）
4. 避免过度设计

---

## 附录 B：与现有文档的关系

| 文档 | 关系 |
|------|------|
| `docs/AOT与FFI集成设计评估.md` | 前置评估，本设计是其实施细节 |
| `docs/jit优化指南.md` | JIT 与 AOT 的关系说明，本设计是其自然延伸 |
| `docs/遗留问题与风险分析报告.md` | 遗留问题 #22（i64 装箱）是本设计要规避的 |
| `docs/多文件编译打包方案设计.md` | .auc 格式设计，本设计扩展其多段结构 |
| `docs/RustFFI设计方案.md` | Rust FFI 设计，与本设计正交 |
| `docs/Aura调试器设计方案.md` | 调试器设计，Phase 3 需要关联 DWARF |

---

## 附录 C：术语表

| 术语 | 说明 |
|------|------|
| AOT | Ahead-of-Time 编译，编译时生成机器码 |
| JIT | Just-in-Time 编译，运行时生成机器码 |
| VM | Virtual Machine，字节码虚拟机 |
| FFI | Foreign Function Interface，外部函数接口 |
| JitValue | JIT 调用约定的值类型（双字段结构） |
| JitEntry | JIT 调用约定的函数入口签名 |
| AotEntry | AOT 调用约定的函数入口签名（= JitEntry） |
| .auc | Aura Bytecode 文件格式 |
| mmap | Memory Mapping，内存映射文件 |
| PROT_EXEC | 内存保护标志：可执行 |
| PROT_READ | 内存保护标志：可读 |
| W^X | 内存安全策略：不可同时可写可执行 |
| Ed25519 | 数字签名算法 |
