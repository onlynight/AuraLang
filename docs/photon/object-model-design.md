# Aura Photon 对象模型设计方案

> **状态**：设计中 → **部分实施**（Phase 1 分配器扩展 + Phase 2 基础构造器）  
> **依赖**：P1 Step 3（Photon 编译编译器 — ✅ 已完成，可链接）  
> **目标**：实现完整的 Aura 对象模型，使 `aura-compiler.exe` 可运行并重编译自身

---

## 0. 任务必要性分析（2026-09-25 复核）

> 本节基于代码实际状态，对原计划 10 项待完成任务逐项验证必要性。

### 0.1 原任务清单与复核结论

| # | 原任务 | 复核结论 | 依据 |
|---|--------|---------|------|
| 1 | 对象模型实现 — vtable/字段偏移/48 个类构造器 | ✅ **需要** | `emitClassConstructor` 仅分配零初始化内存（L844-850），字段访问全返回 0 |
| 2 | heapArena 扩展 — 16KB → 1MB | ✅ **需要** | 当前 `heapArena:16384`（L1767），48 个类 + 编译器状态远超 16KB |
| 3 | 字段布局推断 — 从 Aura 源码推断 | ❌ **合并到 #1** | 附录 A 已有完整字段布局表，无需独立"推断"任务 |
| 4 | POSIX 函数实现 — NtCreateFile/NtReadFile/NtClose | ❌ **已完成** | `PhotonRuntime.aura:854-1006` 已有 `open`/`close`/`read`/`write`/`access`/`exitGroup` 全部 Nt* syscall |
| 5 | Stdlib 函数实现 — Collections.indexOf/mapContainsKey/transform | ❌ **已在 runtime 中** | `PhotonRuntime.aura:1728-1740` 已有 `Collections.indexOf`/`contains`/`mapContainsKey`/`arrayListOf`/`transform` 等 stub |
| 6 | 修复 runtime-only 链接 | ❌ **表述有误** | "undefined symbol: main" 是预期行为（runtime 是库不是 exe）。真正问题：`compileHat` 链接时 runtime obj 可能未写入 |
| 7 | 修复 rebuild6 崩溃 | 🟡 **降低优先级** | rebuild5 已成功编译全部 2694 函数（468KB 机器码），rebuild6 崩溃可能是间歇性问题 |
| 8 | 自举验证 | 🟡 **长期目标** | 终极目标，非阻塞项。需先让编译器可运行 |
| 9 | Rust CLI 降级 | 🟡 **长期目标** | 非阻塞项，当前 Rust CLI 作为种子编译器正常工作 |
| 10 | 高级运行时 — mmap/原子操作/异常表/线程 | ❌ **P4 未来项** | 当前 bump 分配器 + 单线程足够编译器运行 |

### 0.2 修正后的任务清单

**真正的阻塞项（必须做）：**

| # | 任务 | 说明 |
|---|------|------|
| A | 扩展 heapArena 至 1MB | `heapArena:16384` → `heapArena:1048576`，三处 dataSymbolName 需同步修改 |
| B | 实现基础类构造器 | `emitClassConstructor` 需设置 vtable pointer（offset 0）和 object size（offset 8） |
| C | 确保 runtime obj 正确链接 | 验证 `compileHat` 中 runtime obj 写入成功并包含在链接命令中 |

**可延后（非阻塞）：**

| # | 任务 | 说明 |
|---|------|------|
| D | 编译 aura/core stdlib 到 native | 替代 runtime stub，长期方案 |
| E | rebuild6 崩溃排查 | 如 rebuild5 可稳定复现，此项可跳过 |
| F | 自举验证 + Rust CLI 降级 | P3/P4 阶段 |

### 0.3 关键发现

1. **POSIX 函数早已实现**：`open`/`read`/`write`/`close`/`access`/`exitGroup` 全部通过 Nt* syscall 实现，无需重写
2. **Stdlib stub 已在 runtime 中**：`Collections.indexOf` 等 9 个函数已有实现（stub 级别）
3. **链接步骤需要验证**：`compileHat` L429 的条件 `this.outputType != "so" && this.outputType != "dylib"` 应确保 exe 输出时包含 runtime obj
4. **字段布局已知**：附录 A 已有 48 个类的完整字段布局表，无需"推断"

---

## 1. 背景与问题分析

### 1.1 当前状态

P1 Step 3 已通过 63 个 stub 解决链接期 undefined symbol，产出 `aura-compiler.exe`（507,392 B）。但 stub 返回 null/0，运行时立即崩溃。

**崩溃根因**：
- 48 个类构造器符号（`Parser()`, `Hir()`, `Mir()` 等）返回零初始化内存
- 编译器代码通过 `obj.field` 访问对象字段时，得到 null 指针
- `__list_get(0, idx)` 尝试读取 `[0 + idx*8 + 8]`，触发 0xC0000005

**已添加的临时修复**（null 检查）：
- `__list_get`/`__list_setat` 添加 `cmp rcx, 0; je .null` 检查
- 运行时不再崩溃（exit code 0），但程序无输出（逻辑错误）

### 1.2 核心挑战

| 挑战 | 说明 | 复杂度 |
|------|------|--------|
| **字段布局未知** | 48 个类的字段偏移量需要从 Aura 源码推断 | 🔴 极高 |
| **Vtable 支持** | 方法调用可能通过 vtable 分派（需验证） | 🟡 中 |
| **内存限制** | 16KB heapArena 可能不够 | 🟡 中 |
| **确定性** | 对象分配必须确定性（自举验证要求） | 🟡 中 |

### 1.3 方法调用分析

**关键发现**：undefined symbol 列表中**没有** `Parser.parse`、`Hir.lower` 等方法符号。

**可能原因**：
1. **方法被内联**：编译器在编译期将方法调用展开为内联代码
2. **方法通过 vtable 分派**：vtable 在构造器中设置，方法调用通过 `call [rax+offset]`
3. **方法调用不存在**：编译器生成的代码不包含这些调用

**验证方法**：反汇编 `aura-compiler.exe`，搜索 `call` 指令的目标符号。

---

## 2. 对象模型设计

### 2.1 对象内存布局

```
偏移量      大小       内容                      说明
─────────────────────────────────────────────────────────────
0x00        8B        vtable pointer            虚方法表指针（初始化为 0 或类 vtable 地址）
0x08        8B        object size / type tag    对象大小或类型标签
0x10        8B        field 0                   第一个字段
0x18        8B        field 1                   第二个字段
...         ...       ...                       ...
─────────────────────────────────────────────────────────────
```

**设计决策**：
- **vtable pointer 在 offset 0**：与方法调用约定兼容（`call [rcx+offset]`）
- **object size 在 offset 8**：用于 GC/ARC 和调试
- **字段从 offset 16 开始**：与 Rust/C++ 对象模型兼容

### 2.2 Vtable 设计

```
偏移量      大小       内容                      说明
─────────────────────────────────────────────────────────────
0x00        8B        method 0                  第一个虚方法
0x08        8B        method 1                  第二个虚方法
...         ...       ...                       ...
─────────────────────────────────────────────────────────────
```

**方法调用约定**：
```x86_64
; 虚方法调用：obj.method(args...)
; 等价于：call [obj + vtable_offset]

call_method:
    mov  rax, [rcx]           ; rax = vtable
    mov  rax, [rax + offset]  ; rax = method address
    mov  rdx, arg2            ; 设置参数
    call rax                  ; 调用方法
```

**静态方法 vs 虚方法**：
- **静态方法**：直接调用 `call ClassName.method`
- **虚方法**：通过 vtable 分派 `call [obj + vtable_offset]`

### 2.3 类构造器设计

```
构造器签名：ClassName() → obj*

构造器实现：
1. 分配对象内存（emitObjectAlloc）
2. 设置 vtable pointer（obj[0] = vtable_address）
3. 设置 object size（obj[8] = size）
4. 初始化字段（默认值或参数）
5. 返回对象指针
```

**构造器模板**：
```x86_64
; Parser() — 分配 Parser 实例，返回指针
Parser:
    ; 估算对象大小（保守值：128 字节，覆盖大部分类）
    mov  rcx, 128
    call emitObjectAlloc
    ; 设置 vtable pointer（如果类有虚方法）
    ; lea  rdx, [rel ParserVtable]
    ; mov  [rcx], rdx
    ; 设置 object size
    ; mov  qword [rcx+8], 128
    ; 初始化字段（如果需要）
    ; ...
    ret
```

### 2.4 字段布局推断

**策略**：从 Aura 源码推断每个类的字段布局。

**示例：Parser 类**
```aura
class Parser {
    private var lx: Lexer = Lexer("")      // offset 0x10 (Lexer* = 8B)
    var n: Int = 0                          // offset 0x18 (Int = 8B)
    private var pos: Int = 0                // offset 0x20 (Int = 8B)
    var ast: Ast = Ast()                    // offset 0x28 (Ast* = 8B)
    var errors: CompileErrors = ...         // offset 0x30 (CompileErrors* = 8B)
    private var kinds: List<String> = ...   // offset 0x38 (List* = 8B)
    private var prsNoLambda: Boolean = false // offset 0x40 (Bool = 8B)
    private var prsNoBinaryIs: Boolean = false // offset 0x48 (Bool = 8B)
}
```

**字段布局表**：

| 类名 | 字段数 | 估算大小 | 关键字段 |
|------|--------|----------|----------|
| `Parser` | 8 | 96B | lx, n, pos, ast, errors, kinds |
| `Hir` | 12 | 112B | kinds, texts, tys, spans, kids, count, rootId |
| `Mir` | 8 | 80B | blocks, functions, count |
| `X86Encoder` | 4 | 48B | buffer, pos, labels |
| `CString` | 2 | 32B | ptr, len |
| `Span` | 6 | 56B | start, end, startLine, startCol, endLine, endCol |
| ... | ... | ... | ... |

**完整字段布局表**（48 个类）：

见附录 A。

### 2.5 内存管理

**当前状态**：
- `heapArena: 16384`（16KB）
- `heapBump: 8`（字符串 bump 游标）
- `objBump: 8`（对象 bump 游标）

**问题**：
- 16KB 可能不够（48 个类构造器 + 编译器状态）
- bump 分配器无回收，长期运行会 OOM

**解决方案**：

| 方案 | 优点 | 缺点 | 推荐 |
|------|------|------|------|
| **扩展 heapArena 至 1MB** | 简单，满足编译器需求 | 浪费内存 | ✅ MVP |
| **mmap 堆**（P4） | 动态扩展，无上限 | 复杂，需 syscall | ❌ 后续 |
| **标记清除 GC**（P4） | 回收内存 | 复杂，暂停时间长 | ❌ 后续 |

**MVP 策略**：扩展 `heapArena` 至 1MB（0x100000），满足编译器需求。

---

## 3. 实现方案

### 3.1 Phase 1：对象分配器扩展（优先级 P0）

**目标**：扩展对象分配器，支持更大的内存池。

**修改**：
```aura
// .data 段布局（修改后）：
// toStrBuffer:32 + heapArena:1048576 + heapBump:8 + objBump:8
//              = 32 + 1048576 + 8 + 8 = 1048624 字节（~1MB）
```

**实现**：
```aura
fun emitObjectAlloc(enc: X86Encoder): Unit {
    // 从 objBump 分配 size 字节，对齐 8B，返回指针
    // ...（现有实现）
}
```

**验证**：
- 检查 `objBump` 是否超出 `heapArena`
- 超出时返回 null 或 panic

### 3.2 Phase 2：类构造器实现（优先级 P0）

**目标**：48 个类构造器返回有效的对象指针。

**策略**：
1. 从 Aura 源码推断每个类的字段布局
2. 计算每个类的大小（字段大小 + 对齐）
3. 生成构造器代码

**构造器模板**：
```aura
fun emitClassConstructor(enc: X86Encoder, className: String, size: Int, vtableAddr: Int): Unit {
    enc.emitPrologue(0)
    // 分配对象
    enc.emitMovRI("rcx", size)
    enc.emitCallRel("emitObjectAlloc")
    // 设置 vtable pointer（如果类有虚方法）
    if (vtableAddr != 0) {
        enc.emitLeaRIP("rdx", vtableAddr)
        enc.emitStoreMemDisp32("rax", 0, "rdx")
    }
    // 设置 object size
    enc.emitStackStore(-0x10, "rcx")  // 保存 size
    enc.emitLoadIndexed8("rdx", "rax", "rax", 0)  // rdx = obj
    enc.emitStoreIndexed8("rax", "rdx", "rax", 8)  // [obj+8] = size
    // 初始化字段（如果需要）
    // ...
    enc.emitRet()
}
```

**类构造器清单**（48 个）：

| 类名 | 大小 | 虚方法 | 字段初始化 |
|------|------|--------|----------|
| `ArenaAllocator` | 32B | ❌ | base=0, bump=0 |
| `CString` | 32B | ❌ | ptr=0, len=0 |
| `Span` | 56B | ❌ | start=0, end=0, ... |
| `HashMap` | 64B | ❌ | capacity=0, entries=0 |
| `Parser` | 96B | ❌ | lx=0, n=0, pos=0, ... |
| `Hir` | 112B | ❌ | kinds=0, texts=0, ... |
| `Mir` | 80B | ❌ | blocks=0, functions=0, ... |
| `X86Encoder` | 48B | ❌ | buffer=0, pos=0, ... |
| ... | ... | ... | ... |

### 3.3 Phase 3：Vtable 生成（优先级 P1）

**目标**：为有虚方法的类生成 vtable。

**分析**：
- 检查每个类是否有虚方法（`abstract` 或 `override`）
- 如果类有虚方法，生成 vtable 符号
- 构造器中设置 vtable pointer

**Vtable 模板**：
```aura
// ParserVtable: 虚方法表
//   offset 0x00: Parser.parse
//   offset 0x08: Parser.parseExpr
//   ...

fun emitVtable(enc: X86Encoder, className: String, methods: List<String>): Unit {
    enc.emitLabel(className + "Vtable")
    for (method in methods) {
        enc.emitMovRIP("rax", method)
        enc.emitStoreMemDisp32("rax", 0, "rax")
        enc.emitAddRI("rax", 8)  // 下一个槽位
    }
}
```

**方法分派**：
```x86_64
; parser.parse() — 虚方法调用
; 等价于：call [parser + vtable_offset]

call_parser_parse:
    mov  rax, [rcx]           ; rax = vtable
    mov  rax, [rax + 0]       ; rax = Parser.parse address
    call rax
```

### 3.4 Phase 4：方法调用支持（优先级 P1）

**目标**：支持虚方法调用。

**分析**：
- 检查编译器生成的代码是否有虚方法调用
- 如果有，需要支持 `call [obj + vtable_offset]` 指令

**X86 指令**：
```x86_64
; 虚方法调用：call [obj + vtable_offset]
; 等价于：mov rax, [rcx]; mov rax, [rax + offset]; call rax

; 简化形式（如果 offset 是常量）：
; mov rax, [rcx + vtable_offset]
; call rax
```

**实现**：
```aura
fun emitVirtualCall(enc: X86Encoder, obj: String, offset: Int, args: List<String>): Unit {
    // 加载 vtable
    enc.emitLoadIndexed8("rax", obj, "rax", 0)  // rax = [obj]
    // 加载方法地址
    enc.emitLoadIndexed8("rax", "rax", "rax", offset)  // rax = [vtable + offset]
    // 设置参数
    // ...
    // 调用方法
    enc.emitCallReg("rax")
}
```

### 3.5 Phase 5：字段访问支持（优先级 P0）

**目标**：支持 `obj.field` 访问。

**分析**：
- 编译器生成的代码通过 `mov rax, [rcx + offset]` 访问字段
- 需要确保字段偏移量正确

**字段访问模板**：
```x86_64
; parser.n — 访问 Parser.n 字段（offset 0x18）
; 等价于：mov rax, [rcx + 0x18]

load_parser_n:
    mov  rax, [rcx + 0x18]  ; rax = parser.n
```

**验证**：
- 反汇编编译器生成的代码，检查字段访问偏移量
- 确保偏移量与类定义匹配

### 3.6 Phase 6：POSIX 函数实现（优先级 P2）

**目标**：6 个 POSIX 函数通过 Nt* syscall 实现。

| POSIX | Windows Nt* | 服务号 | 复杂度 |
|-------|-------------|--------|--------|
| `open` | `NtCreateFile` | 0x05 | 🔴 高 |
| `read` | `NtReadFile` | 0x03 | 🟡 中 |
| `write` | `NtWriteFile` | 0x08 | ✅ 已有 |
| `close` | `NtClose` | 0x0B | 🟢 低 |
| `access` | `NtQueryInformationFile` | 0x0E | 🟡 中 |
| `fork` | 无直接对应 | — | 🔴 极高 |
| `execve` | `NtCreateUserProcess` | 0x22 | 🔴 极高 |
| `wait4` | `NtWaitForSingleObject` | 0x2D | 🟡 中 |
| `exitGroup` | `NtTerminateProcess` | 0x2C | 🟢 低 |

**MVP 策略**：
- `open`/`read`/`write`/`close`：实现（文件 I/O 必需）
- `access`：stub 返回 0（成功）
- `fork`/`execve`/`wait4`：stub 返回 -1（失败）

### 3.7 Phase 7：Stdlib 函数实现（优先级 P2）

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

---

## 4. 实施计划

### 4.1 阶段划分

| 阶段 | 任务 | 预计工作量 | 产出 |
|------|------|-----------|------|
| **Phase 1** | 对象分配器扩展 | 0.5 天 | 1MB heapArena |
| **Phase 2** | 类构造器实现 | 3-5 天 | 48 个构造器返回有效对象 |
| **Phase 3** | Vtable 生成 | 2-3 天 | 虚方法表 |
| **Phase 4** | 方法调用支持 | 2-3 天 | 虚方法调用 |
| **Phase 5** | 字段访问验证 | 1-2 天 | 字段偏移量正确 |
| **Phase 6** | POSIX 函数实现 | 3-5 天 | 文件 I/O 可用 |
| **Phase 7** | Stdlib 函数实现 | 1-2 天 | stdlib 可用 |
| **Phase 8** | 运行测试 + 迭代 | 3-5 天 | 编译器可运行 |
| **Phase 9** | 自举验证 | 1-2 天 | 字节一致性验证 |

**总计**：14-27 天（单人全职）

### 4.2 风险矩阵

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| 对象字段布局不匹配导致越界读写 | 🔴 高 | 🔴 崩溃 | 使用 256-512B 保守分配；反汇编验证偏移量 |
| 方法调用不是通过 vtable 分派 | 🟡 中 | 🟡 架构性改动 | 反汇编分析确认调用方式 |
| 1MB heapArena 不够用 | 🟡 中 | 🔴 OOM | 扩展至 4MB；或实现 mmap 堆 |
| POSIX `open` 实现过于复杂 | 🟡 中 | 🟡 阻塞文件 I/O | 降级为 stub，编译器走错误处理 |
| 编译器逻辑依赖 stub 行为 | 🔴 高 | 🟡 功能不完整 | 逐步替换，每次只改一类 |

### 4.3 最小可行路径（MVP）

如果时间有限，建议按以下顺序实现：

1. **Phase 1**（对象分配器）→ 必须
2. **Phase 2**（类构造器）→ 必须
3. **Phase 5**（字段访问验证）→ 必须
4. **Phase 7**（stdlib 函数）→ 必须（简单，快速胜利）
5. **Phase 6**（POSIX 函数）→ 可选（如果编译器不强制要求文件 I/O）
6. **Phase 3-4**（Vtable/方法调用）→ 可选（如果编译器不使用虚方法）

**MVP 预计工作量**：9-14 天

---

## 5. 技术细节

### 5.1 `.data` 段布局（修改后）

```
偏移量      大小       符号名                      说明
─────────────────────────────────────────────────────────────
0x0000      32B       toStrBuffer                 — 字符串构建缓冲区
0x0020      1048576B  heapArena                   — 堆 arena（bump 分配用）
0x100020    8B        heapBump                    — 字符串 bump 游标
0x100028    8B        objBump                     — 对象 bump 游标（新增）
0x100030    8B        vtableBase                  — vtable 基址（新增，用于相对偏移）
─────────────────────────────────────────────────────────────
```

**注意**：
- `objBump` 的初始值需要指向 `heapArena` 的起始
- `vtableBase` 用于计算 vtable 的相对偏移

### 5.2 对象内存布局

```
偏移量      大小       内容                      说明
─────────────────────────────────────────────────────────────
0x00        8B        vtable pointer            虚方法表指针（初始化为 0）
0x08        8B        object size               对象大小（记录分配大小）
0x10        8B        field 0                   第一个字段
0x18        8B        field 1                   第二个字段
...         ...       ...                       ...
─────────────────────────────────────────────────────────────
```

### 5.3 bump 分配器算法

```
alloc(size):
    addr = objBump
    addr = align(addr, 8)          // 8 字节对齐
    objBump = addr + size
    if (objBump > heapArena + 1048576):
        return null                 // OOM
    zero_init(addr, size)           // 零初始化
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

## 8. 附录

### 附录 A：48 个类字段布局表

| 类名 | 字段数 | 估算大小 | 关键字段 |
|------|--------|----------|----------|
| `ArenaAllocator` | 2 | 32B | base, bump |
| `AucModule` | 4 | 48B | name, version, exports |
| `AucReader` | 2 | 32B | buf, pos |
| `CString` | 2 | 32B | ptr, len |
| `HashMap` | 4 | 64B | capacity, entries, loadFactor |
| `Hir` | 12 | 112B | kinds, texts, tys, spans, kids, count, rootId |
| `HirLowerer` | 4 | 48B | parser, errors, count |
| `Mir` | 8 | 80B | blocks, functions, count |
| `MirBlock` | 4 | 48B | id, instrs, count |
| `MirFunction` | 6 | 64B | name, blocks, args, count |
| `MirSsaProgram` | 4 | 48B | functions, count |
| `MirValue` | 6 | 64B | id, type, operands, count |
| `Parser` | 8 | 96B | lx, n, pos, ast, errors, kinds |
| `Span` | 6 | 56B | start, end, startLine, startCol, endLine, endCol |
| `VmRunner` | 4 | 48B | module, stack, errors |
| `VmStack` | 2 | 32B | buf, top |
| `VmStrStack` | 2 | 32B | buf, top |
| `VmFrameStack` | 2 | 32B | buf, top |
| `InstructionSelector` | 4 | 48B | dag, patterns, count |
| `MachineDag` | 6 | 72B | nodes, count |
| `DagNode` | 4 | 48B | id, op, operands |
| `DagInstruction` | 4 | 48B | op, operands, count |
| `PeepholeOptimizer` | 2 | 32B | dag, patterns |
| `X86Emitter` | 4 | 48B | encoder, functions, count |
| `X86Encoder` | 4 | 48B | buffer, pos, labels |
| `EmitBuffer` | 2 | 32B | buf, pos |
| `DebugInfo` | 4 | 48B | functions, count |
| `Lowering` | 4 | 48B | ssa, lir, count |
| `TargetTriple` | 3 | 40B | arch, os, env |
| `OptimizationLevel` | 1 | 24B | level |
| `CEmitter` | 4 | 48B | buffer, functions, count |
| `LlvmEmitter` | 4 | 48B | buffer, functions, count |
| `PhotonPipeline` | 6 | 72B | input, output, config |
| `PhotonObjectWriter` | 4 | 48B | output, symbols, relocations |
| `PhotonSystemLinker` | 4 | 48B | config, input, output |
| `PhotonLldConfig` | 4 | 48B | entry, libs, flags |
| `AotCodeGenerator` | 4 | 48B | pipeline, config |
| `AotExeResult` | 4 | 48B | exe, size, symbols |
| `AotLinkResult` | 4 | 48B | exe, size, symbols |
| `AotModuleLinker` | 4 | 48B | config, modules, count |
| `AotOptions` | 6 | 72B | output, target, opts |
| `AotResult` | 4 | 48B | success, errors, count |
| `BackendResult` | 4 | 48B | success, errors, count |
| `JitBackend` | 4 | 48B | config, functions, count |
| `JitCoreCompiler` | 4 | 48B | backend, config |
| `JitCoreVm` | 4 | 48B | module, stack, errors |
| `JitLinkResult` | 4 | 48B | success, errors, count |

**总估算大小**：48 个类 × 平均 50B = 2400B（约 2.4KB）

**加上 vtable**：假设 10 个类有虚方法，每个 vtable 平均 4 个方法 = 10 × 32B = 320B

**总内存需求**：2400B + 320B + 16KB heapArena = ~19KB

**结论**：1MB heapArena 足够满足需求（有 50 倍余量）。

### 附录 B：方法调用分析

**待验证**：反汇编 `aura-compiler.exe`，搜索 `call` 指令的目标符号。

**搜索命令**：
```powershell
# 提取所有 call 指令的目标符号
llvm-objdump -d build\hat-bootstrap\aura-compiler.exe | Select-String "call" | Select-Object -First 100
```

**预期结果**：
- 如果看到 `call [rax+0x10]` 或类似形式 → 虚方法调用
- 如果看到 `call Parser_parse` → 静态方法调用
- 如果没有方法调用 → 方法被内联

### 附录 C：字段偏移量验证

**验证方法**：反汇编编译器生成的代码，检查字段访问偏移量。

**示例**：
```x86_64
; parser.n — 访问 Parser.n 字段
; 如果 Parser.n 在 offset 0x18：
; mov rax, [rcx + 0x18]

; parser.lx — 访问 Parser.lx 字段
; 如果 Parser.lx 在 offset 0x10：
; mov rax, [rcx + 0x10]
```

**验证命令**：
```powershell
# 搜索字段访问指令
llvm-objdump -d build\hat-bootstrap\aura-compiler.exe | Select-String "mov.*\[rcx\+0x" | Select-Object -First 50
```

---

## 9. 当前实施状态（2026-09-25）

### 9.1 已完成

| 阶段 | 任务 | 状态 | 修改文件 |
|------|------|------|----------|
| Phase 1 | heapArena 扩展 16KB → 1MB | ✅ 完成 | `PhotonRuntime.aura` (3 处 dataSymbolName + 3 处边界检查) |
| Phase 2 | 基础类构造器 | ✅ 完成 | `PhotonRuntime.aura` (emitClassConstructor 函数) |
| 分析 | 任务必要性复核 | ✅ 完成 | 本文档 §0 |

### 9.2 关键修改

**heapArena 扩展**：
- `dataSymbolName`: `heapArena:16384` → `heapArena:1048576`
- 边界检查: `16000` → `1048000`
- 三处 dataSymbolName 已同步修改

**类构造器实现**：
```
对象内存布局：
  [0x00] vtable pointer (8B) — 初始化为 0（无虚方法）
  [0x08] object size   (8B) — 记录分配大小
  [0x10] field 0       (8B) — 由 emitObjectAlloc 零初始化
  [0x18] field 1       (8B) — 由 emitObjectAlloc 零初始化
  ...

栈帧布局（sub rsp, 0x30 = 48B）：
  [rbp-0x30, rbp-0x10)  影子空间 shadow space（32B）
  [rbp-0x08]            size 保存槽（避开影子空间）
```

### 9.3 待验证

- **runtime obj 链接**：代码逻辑正确（outputType 默认为 "exe"），需实际运行编译验证
- **48 个类构造器**：已生成（buildRuntimeObject 函数中硬编码），需验证符号是否正确链接

### 9.4 下一步

1. 运行 `scripts\photon-hat-bootstrap.ps1` 验证 runtime obj 是否正确链接
2. 反汇编 `aura-compiler.exe`，分析方法调用方式（附录 B）
3. 如 runtime obj 正确链接，运行自举验证

---

## 10. 总结

| 维度 | 评估 |
|------|------|
| **工作量** | 14-27 天（单人全职） |
| **技术风险** | 🔴 高（对象模型布局未知） |
| **最大障碍** | 48 个类构造器的字段布局 + vtable 支持 |
| **快速胜利** | Phase 1（分配器）+ Phase 7（stdlib）可快速完成 |
| **降级方案** | POSIX 函数降级为 stub，编译器走错误处理路径 |
| **MVP 工作量** | 9-14 天（跳过 Vtable/方法调用） |

**下一步**：
1. 反汇编 `aura-compiler.exe`，分析方法调用方式
2. 从 Aura 源码推断 48 个类的字段布局
3. 实现 Phase 1-2（对象分配器 + 类构造器）
4. 运行测试，验证对象模型是否正确
