# P1 Step 4：自举验证 — Aura 对象模型实现方案

> **状态**：规划中  
> **依赖**：P1 Step 3（Photon 编译编译器 — ✅ 已完成，可链接）  
> **目标**：`aura-compiler.exe` 可运行，重编译自身后产出字节一致的产物

---

## 1. 背景

Step 3 已通过 63 个 stub（`mov eax,0; ret`）解决链接期 undefined symbol，产出 `aura-compiler.exe`（507,392 B）。但 stub 返回 null/0，运行时立即崩溃。

Step 4 需要编译器**可执行**，才能验证自举字节一致性。核心障碍是 **48 个类构造器符号**——编译器代码大量使用 `Parser()`, `Hir()`, `Mir()` 等实例化，返回的对象指针被用于字段访问和方法调用。

---

## 2. 63 个未定义符号分类

| 类别 | 数量 | 复杂度 | 实现策略 |
|------|------|--------|----------|
| **类构造器** | 48 | 🔴 极高 | 需实现 Aura 对象模型（vtable、字段布局、方法分派） |
| **POSIX 函数** | 6 | 🟡 高 | `open`/`fork`/`execve` 等 → Nt* syscall 重写 |
| **Stdlib 函数** | 9 | 🟢 中 | `Collections.indexOf` 等 → 映射到现有 `__list_alloc`/`__list_get` |

### 2.1 类构造器清单（48 个）

```
ArenaAllocator, AucModule, AucReader, CString, HashMap,
Hir, HirLowerer, Mir, MirBlock, MirFunction, MirSsaProgram, MirValue,
Parser, Span, VmRunner, VmStack, VmStrStack, VmFrameStack,
InstructionSelector, MachineDag, DagNode, DagInstruction,
PeepholeOptimizer, X86Emitter, X86Encoder, EmitBuffer, DebugInfo,
Lowering, TargetTriple, OptimizationLevel, CEmitter, LlvmEmitter,
PhotonPipeline, PhotonObjectWriter, PhotonSystemLinker, PhotonLldConfig,
AotCodeGenerator, AotExeResult, AotLinkResult, AotModuleLinker,
AotOptions, AotResult, BackendResult,
JitBackend, JitCoreCompiler, JitCoreVm, JitLinkResult
```

### 2.2 POSIX 函数清单（6 个）

```
access, open, execve, fork, wait4, exitGroup
```

### 2.3 Stdlib 函数清单（9 个）

```
__throw, ReadCStr, fromCharCode,
Collections.indexOf, Collections.contains, Collections.mapContainsKey,
arrayListOf, transform, aura_mem_used_mb
```

---

## 3. 实现方案

### 3.1 Phase 1：对象分配器基础设施（优先级 P0）

**目标**：提供从 `.data` 段堆 arena 分配对象内存的能力。

**现状**：`.data` 段已有 `heapArena:16384`（16KB）+ `heapBump:8`（字符串 bump 游标）。

**新增**：`objBump:8`（对象分配 bump 游标），与字符串 bump 独立。

```
// .data 段布局（修改后）：
// toStrBuffer:32 + heapArena:16384 + heapBump:8 + objBump:8
//              = 32 + 16384 + 8 + 8 = 16432 字节
```

**新增方法 `emitObjectAlloc(enc, size: Int)`**：

```x86_64
; emitObjectAlloc(size) — 从 objBump 分配 size 字节，对齐 8B，返回指针
; 签名：(size: i64) -> i64*
;   入参：rcx = size
;   返回：rax = 对象指针（对齐 8B）

emitObjectAlloc:
    push rbp
    mov  rbp, rsp
    sub  rsp, 0x20

    ; 保存 size
    mov  [rbp-0x8], rcx

    ; 读取当前 objBump
    lea  rax, [rel objBump]
    mov  rax, [rax]

    ; 对齐 8B：addr = (addr + 7) & ~7
    add  rax, 7
    and  rax, -8

    ; 保存对齐后的地址
    mov  [rbp-0x10], rax

    ; 更新 objBump
    mov  rcx, [rbp-0x8]
    add  rax, rcx
    lea  rdx, [rel objBump]
    mov  [rdx], rax

    ; 零初始化分配的内存（最多 size 字节）
    mov  rcx, [rbp-0x10]      ; rcx = 目标地址
    mov  rsi, [rbp-0x8]       ; rsi = size
    xor  rax, rax             ; rax = 0（要写入的值）
    shr  rsi, 3               ; rsi = size / 8（每次写 8 字节）
    jz   .done_init

.init_loop:
    mov  qword [rcx], rax
    add  rcx, 8
    dec  rsi
    jnz  .init_loop

.done_init:
    mov  rax, [rbp-0x10]      ; 返回对齐后的地址
    mov  rsp, rbp
    pop  rbp
    ret
```

**注意**：`objBump` 初始值必须指向 `heapArena` 起始。需要在 `.data` 段初始化时设置 `objBump = heapArena + 16384`（或某个起始偏移）。

**替代方案**：由于 COFF `.data` 段的符号地址由链接器分配，无法在编译时确定 `heapArena` 的绝对地址。因此 `objBump` 应初始化为 `heapArena` 的符号引用（通过 relocation 解决），或首次分配时惰性初始化。

### 3.2 Phase 2：类构造器实现（优先级 P0）

**目标**：48 个类构造器返回有效的对象指针。

**关键问题**：每个类构造器的返回对象需要正确的**字段布局**和**vtable**。编译器代码通过偏移量访问字段，如果布局不对会读到垃圾数据。

**策略**：由于无法获取 Aura 源码中每个类的字段布局，采用**通用对象头**策略：

```
对象布局（通用）：
  offset 0:    vtable pointer (8 bytes) — 初始化为 0
  offset 8:    size / type tag (8 bytes) — 记录对象大小
  offset 16:   字段数据开始
  ...
```

**构造器模板**：

```x86_64
; Parser() — 分配 Parser 实例，返回指针
Parser:
    ; 估算对象大小（保守值：128 字节，覆盖大部分类）
    mov  rcx, 128
    call emitObjectAlloc
    ret
```

**风险**：
- 如果实际对象需要更多字段，会越界写入
- 如果编译器访问的字段偏移超出分配区域，会读到相邻对象的内存
- **缓解**：使用 256 字节（甚至 512 字节）的保守分配，减少越界概率

**分类处理**：

| 类 | 建议大小 | 说明 |
|----|---------|------|
| `ArenaAllocator` | 32B | 仅需 base + bump 两个字段 |
| `CString` | 16B | 仅需 ptr + len |
| `Span` | 32B | start + end + file |
| `HashMap` | 64B | capacity + entries |
| `Parser` | 256B | 状态较多 |
| `Hir`/`Mir` | 256B | AST/MIR 节点容器 |
| 其他 | 128B | 默认值 |

### 3.3 Phase 3：POSIX 函数实现（优先级 P1）

**目标**：6 个 POSIX 函数通过 Nt* syscall 实现。

| POSIX | Windows Nt* | 服务号 | 复杂度 |
|-------|-------------|--------|--------|
| `open` | `NtCreateFile` | 0x05 | 🔴 高（需构造 UNICODE_STRING + OBJECT_ATTRIBUTES） |
| `read` | `NtReadFile` | 0x03 | 🟡 中 |
| `write` | `NtWriteFile` | 0x08 | ✅ 已有（println 使用） |
| `close` | `NtClose` | 0x0B | 🟢 低 |
| `access` | `NtQueryInformationFile` | 0x0E | 🟡 中 |
| `fork` | 无直接对应 | — | 🔴 极高（需 `NtCreateUserProcess` + 线程） |
| `execve` | `NtCreateUserProcess` | 0x22 | 🔴 极高 |
| `wait4` | `NtWaitForSingleObject` | 0x2D | 🟡 中 |
| `exitGroup` | `NtTerminateProcess` | 0x2C | 🟢 低（已有 exit 实现） |

**`open` 实现难点**：

POSIX `open(pathname, flags)` 需要构造 `OBJECT_ATTRIBUTES` 结构体：
```c
typedef struct {
    HANDLE           RootDirectory;    // 0: 根目录句柄
    UNICODE_STRING  *ObjectName;       // 8: 文件名（UNICODE_STRING）
    ULONG            Attributes;        // 16: 属性
    void            *ObjectAttributes;  // 24: 扩展属性
} OBJECT_ATTRIBUTES;

typedef struct {
    USHORT           Length;            // 0: 字符串长度（字节）
    USHORT           MaximumLength;     // 2: 最大长度
    // USHORT        Padding;           // 4: 对齐填充
    wchar_t         *Buffer;            // 8: 缓冲区指针
} UNICODE_STRING;
```

这需要：
1. 在栈上构造 `UNICODE_STRING`（16 字节）
2. 将 pathname 从 ASCII 转为 UTF-16（宽字符）
3. 构造 `OBJECT_ATTRIBUTES`（24 字节）
4. 调用 `NtCreateFile`（11 个参数，前 4 个走寄存器，后 7 个走栈）

**简化方案**：使用 Windows API（kernel32.dll）的 `CreateFileW` 替代 `NtCreateFile`，避免手动构造 UNICODE_STRING。但这会引入外部依赖，违反 P2 零依赖目标。

**替代方案**：将 `open` 实现为 stub（返回 -1），让编译器走错误处理路径。如果编译器对文件 I/O 失败有容错，这可能可行。

### 3.4 Phase 4：Stdlib 函数实现（优先级 P2）

**目标**：9 个 stdlib 函数映射到现有 runtime 功能。

| Stdlib 函数 | 映射策略 | 复杂度 |
|-------------|---------|--------|
| `Collections.indexOf` | 调用 `__list_get` 循环查找 | 🟡 中 |
| `Collections.contains` | 同 indexOf | 🟡 中 |
| `Collections.mapContainsKey` | stub 返回 0 | 🟢 低 |
| `arrayListOf` | 调用 `__list_alloc` | 🟢 低 |
| `transform` | stub 返回入参 | 🟢 低 |
| `fromCharCode` | 查表返回字符 | 🟢 低 |
| `ReadCStr` | 读取 C 字符串 | 🟡 中 |
| `__throw` | 打印错误消息后 exit | 🟢 低 |
| `aura_mem_used_mb` | 返回 `heapBump` 值 | 🟢 低 |

### 3.5 Phase 5：方法分派（优先级 P3 — 可能不需要）

**关键问题**：编译器代码中的方法调用（如 `parser.parse()`）是否通过 vtable 分派？

**分析**：undefined symbol 列表中**没有** `Parser.parse`、`Hir.lower` 等方法符号。这暗示：
1. **方法被内联**：编译器在编译期将方法调用展开为内联代码
2. **方法通过 vtable 分派**：vtable 在构造器中设置，方法调用通过 `call [rax+offset]`
3. **方法调用不存在**：编译器生成的代码不包含这些调用

**验证方法**：反汇编 `aura-compiler.exe`，搜索 `call` 指令的目标符号，确认是否存在方法调用。

---

## 4. 实施计划

### 4.1 阶段划分

| 阶段 | 任务 | 预计工作量 | 产出 |
|------|------|-----------|------|
| **Phase 1** | 对象分配器基础设施 | 1-2 天 | `emitObjectAlloc` + `objBump` 字段 |
| **Phase 2** | 48 个类构造器 | 2-3 天 | 构造器返回有效对象指针 |
| **Phase 3** | 6 个 POSIX 函数 | 3-5 天 | 文件 I/O 可用 |
| **Phase 4** | 9 个 stdlib 函数 | 1-2 天 | stdlib 可用 |
| **Phase 5** | 运行测试 + 迭代 | 3-5 天 | 编译器可运行 |
| **Phase 6** | 自举验证 | 1-2 天 | 字节一致性验证 |

**总计**：11-19 天（单人全职）

### 4.2 风险矩阵

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| 对象字段布局不匹配导致越界读写 | 🔴 高 | 🔴 崩溃 | 使用 256-512B 保守分配 |
| POSIX `open` 实现过于复杂 | 🟡 中 | 🟡 阻塞文件 I/O | 降级为 stub，编译器走错误处理 |
| 方法分派需要 vtable 支持 | 🟡 中 | 🔴 架构性改动 | 先验证是否需要（反汇编分析） |
| 16KB heapArena 不够用 | 🟡 中 | 🔴 OOM | 扩展至 1-4MB |
| 编译器逻辑依赖 stub 行为 | 🔴 高 | 🟡 功能不完整 | 逐步替换，每次只改一类 |

### 4.3 最小可行路径（MVP）

如果时间有限，建议按以下顺序实现：

1. **Phase 1**（对象分配器）→ 必须
2. **Phase 2**（类构造器）→ 必须
3. **Phase 4**（stdlib 函数）→ 必须（简单，快速胜利）
4. **Phase 3**（POSIX 函数）→ 可选（如果编译器不强制要求文件 I/O）
5. **Phase 5**（运行测试）→ 必须

---

## 5. 技术细节

### 5.1 `.data` 段布局

```
偏移量      大小       符号名
0x0000      32B       toStrBuffer     — 字符串构建缓冲区
0x0020      16384B    heapArena       — 堆 arena（bump 分配用）
0x4020      8B        heapBump        — 字符串 bump 游标
0x4028      8B        objBump         — 对象 bump 游标（新增）
```

**注意**：`objBump` 的初始值需要指向 `heapArena` 的起始。由于 COFF relocation 在链接期解决，`objBump` 的初始值应设置为 `heapArena` 的符号引用。

### 5.2 对象内存布局

```
偏移量      大小       内容
0x00        8B        vtable pointer（初始化为 0）
0x08        8B        object size（记录分配大小）
0x10        ...       字段数据（按类定义布局）
```

### 5.3 bump 分配器算法

```
alloc(size):
    addr = objBump
    addr = align(addr, 8)          // 8 字节对齐
    objBump = addr + size
    zero_init(addr, size)          // 零初始化
    return addr
```

### 5.4 NtCreateFile 参数布局

```
NtCreateFile(
    out PHANDLE     FileHandle,          // rcx (r10)
    in  ACCESS_MASK DesiredAccess,       // rdx
    in  POBJECT_ATTRIBUTES ObjectAttributes,  // r8
    out PIO_STATUS_BLOCK IoStatusBlock,  // r9
    in  PLARGE_INTEGER AllocationSize,   // [rsp+0x28]
    in  ULONG        FileAttributes,     // [rsp+0x30]
    in  ULONG        ShareAccess,        // [rsp+0x38]
    in  ULONG        CreateDisposition,  // [rsp+0x40]
    in  ULONG        CreateOptions,      // [rsp+0x48]
    in  PVOID        EaBuffer,           // [rsp+0x50]
    in  ULONG        EaLength            // [rsp+0x58]
)
```

**关键**：`OBJECT_ATTRIBUTES` 需要 `UNICODE_STRING`，需要将 ASCII pathname 转为 UTF-16。

---

## 6. 验证策略

### 6.1 编译验证

```powershell
# 1. 重建 driver
.\rust\target\release\aura.exe build --aot aura\compiler\aura\lang\compiler\backend\photon\PhotonHatCompile.aura --output build\hat-native\PhotonHatCompile.exe

# 2. 运行 diag（含链接）
& .\scripts\photon-c-chain-diag.ps1 -TripKB 8000 -TimeoutSecs 900 -MemMB 8192
```

### 6.2 运行验证

```powershell
# 3. 运行编译后的编译器
.\build\hat-bootstrap\aura-compiler.exe --version

# 4. 如果可运行，尝试编译一个简单的 Aura 文件
.\build\hat-bootstrap\aura-compiler.exe build tests\hello.aura --output tests\hello.exe
```

### 6.3 自举验证

```powershell
# 5. 用编译后的编译器重编译自身
.\build\hat-bootstrap\aura-compiler.exe build --aot aura\compiler\Main.aura --output build\hat-bootstrap\aura-compiler-v2.exe

# 6. 比较字节一致性
$hash1 = (Get-FileHash build\hat-bootstrap\aura-compiler.exe).Hash
$hash2 = (Get-FileHash build\hat-bootstrap\aura-compiler-v2.exe).Hash
"SHA256 match: $($hash1 -eq $hash2)"
```

---

## 7. 与现有基线的兼容性

| 基线 | 当前值 | Step 4 影响 |
|------|--------|------------|
| HAT suite 1 | 30/30 | ⚠️ 可能受影响（runtime obj 变化） |
| HAT suite 2 | 10/10 | ⚠️ 可能受影响 |
| HAT suite 3 | 10/10 | ⚠️ 可能受影响 |
| Native suite | 13/15 | ⚠️ 可能受影响 |

**注意**：Step 4 修改 runtime obj（添加 `objBump` 字段 + 构造器实现），可能影响现有 HAT suite。每次修改后需要运行完整回归测试。

---

## 8. 总结

| 维度 | 评估 |
|------|------|
| 工作量 | 11-19 天（单人全职） |
| 技术风险 | 🔴 高（对象模型布局未知） |
| 最大障碍 | 48 个类构造器的字段布局 + vtable 支持 |
| 快速胜利 | Phase 1（分配器）+ Phase 4（stdlib）可快速完成 |
| 降级方案 | POSIX 函数降级为 stub，编译器走错误处理路径 |

**建议**：先完成 Phase 1 + Phase 2 + Phase 4（约 4-7 天），测试编译器是否可运行。如果不可运行，再评估 Phase 3（POSIX）的必要性。
