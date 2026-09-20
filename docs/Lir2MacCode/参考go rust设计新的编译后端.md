# Aura 自研编译器后端设计方案（v2）

> **版本**：2.0  
> **日期**：2026-06-24  
> **依据**：`docs/编译器LLVM交互分析与纯Aura化迁移计划.md` Phase 6/6.5/7/9 现状分析 + 参考 Go SSA / Rust HIR-MIR-LLVM 分层设计  
> **目标**：替换 LLVM / Cranelift 外部依赖，实现纯 Aura 自研后端，统一 AOT 与 JIT 两条路径  
> **适用范围**：`aura/compiler/`（纯 Aura 自举编译器），目标架构 x86_64 → aarch64

---

## 一、设计目标

### 1.1 核心目标

1. **替换 LLVM / Cranelift 外部依赖**：不再通过 `llc`/`clang` 子进程生成机器码，也不再依赖 Cranelift 的 FFI 调用
2. **统一 AOT 与 JIT 后端**：两条路径共享同一套 MIR → LIR → Machine DAG → 寄存器分配 → 指令编码管线
3. **保持自举能力**：新后端以纯 Aura 实现（`aura/compiler/` 目录），不引入任何外部编译依赖
4. **保持向前兼容**：与现有 HIR（Phase 2）、MIR（Phase 3）、字节码（Phase 4/5）基础设施对接

### 1.2 非目标

- 不实现跨 6+ 架构的完整覆盖（优先 x86_64，其次 aarch64）
- 不追求 LLVM O3 级别的激进优化（目标是"足够好的优化"）
- 不改动前端（Lexer/Parser/AST/Sema/HIR）的现有实现

### 1.3 核心原则

| 原则 | 说明 |
|------|------|
| **单一 MIR** | VM / AOT / JIT 共享同一份 SSA 化 MIR，禁止各自维护不同表示 |
| **单一后端管线** | AOT 和 JIT 从 LIR 开始走相同的 Machine DAG → 寄存器分配 → 指令编码 |
| **规则驱动 lowering** | 指令选择用模式匹配 + DAG Tiling，而非硬编码 if-else |
| **纯 Aura 实现** | 后端全部以 Aura 源码实现，不依赖 Rust / C / 外部工具链 |

---

## 二、当前现状分析（Phase 6.5 终点）

### 2.1 现有编译管线

```
Source → Lexer → Parser → AST → Sema → HIR (树, List arena)
                                         │
                                         ├──→ MIR (TAC, 寄存器式 CFG) → Codegen → 字节码 → VM 解释
                                         │
                                         └──→ Emit (直接 HIR→LLVM IR 文本) → llc → clang → Native exe
```

**JIT 路径（独立）**：

```
字节码 → JitLower → Cranelift IR 文本 → (Rust Cranelift FFI) → 原生码
```

### 2.2 现有架构的核心问题

| # | 问题 | 现状 | 影响 |
|---|------|------|------|
| 1 | **AOT 绕过 MIR** | `Emit.aura` 直接从 HIR 生成 LLVM IR，不经 MIR | MIR 对 AOT 无用，两套 IR 并存 |
| 2 | **MIR 非 SSA** | TAC 三地址码，变量槽寻址（`LOAD_VAR` / `STORE_VAR`） | 无法做 GVN、CSE 等 SSA 优化 |
| 3 | **无显式内存模型** | 内存操作隐式顺序，无 memory chain | 内存依赖不可分析 |
| 4 | **指令选择硬编码** | `Emit.aura`（9859 行）if-else 直接拼 LLVM IR | 添加新指令需改代码，不可扩展 |
| 5 | **寄存器分配靠外部** | 由 LLVM / Cranelift 完成 | 不可控制溢出策略 |
| 6 | **JIT 仅支持叶子函数** | `CALL` 不在白名单（实际已加入但 Cranelift FFI 未接入） | 循环热点、递归无法加速 |
| 7 | **值/指针表示混用** | 结构体实例时而 `%struct.X` 时而 `i8*` | 字段修改丢失（Phase 6.5 已暴露） |
| 8 | **String 双表示** | AOT 内 `{i8*, i64}` vs 运行时 `i8*` | 字符串方法调用静默错误 |
| 9 | **外部工具链依赖** | AOT 需 `llc`/`clang`/`lld-link` 子进程 | 部署不便，编译速度受限 |
| 10 | **JIT 依赖 Cranelift FFI** | 原生码生成需 Rust 侧 `cranelift` 库 | 无法纯 Aura 自举 |

### 2.3 现有资产（可复用）

| 资产 | 文件 | 可复用性 | 说明 |
|------|------|---------|------|
| HIR 表示与降级 | `hir/Hir.aura` (83KB) | ✅ 直接复用 | 树状、去糖化，前端产出 |
| MIR 基本块框架 | `mir/Mir.aura` (12KB) | 🔧 需重构 | List arena 结构可复用，TAC 需改 SSA |
| HIR→MIR 降级 | `mir/MirLower.aura` (18KB) | 🔧 需重写 | 需产出 SSA 而非 TAC |
| MIR 优化器 | `mir/MirOpt.aura` (8KB) | 🔧 需扩展 | 现有 DCE/常量传播可保留 |
| 类型映射器 | `aot/TypeMapper.aura` (11KB) | 🔧 可复用 | Int→i32、String→`{i8*,i64}` 映射可保留 |
| 目标三元组 | `aot/Target.aura` (8KB) | 🔧 可扩展 | x86_64/aarch64 三元组构造可复用 |
| Runtime 声明 | `aot/Runtime.aura` (16KB) | 🔧 可复用 | ARC/异常/字符串 runtime 声明可保留 |
| 调用点符号表 | `aot/StdSigs.aura` | ✅ 直接复用 | std 函数签名唯一真相源 |
| JIT 热点检测 | `jit/JitState.aura` (13KB) | ✅ 直接复用 | Fix A/B 状态机可保留 |
| JIT 优化传递 | `jit/JitOpt.aura` (24KB) | ✅ 直接复用 | 7 个优化传递可保留 |
| JIT 派发/回退 | `jit/JitDispatch.aura` (12KB) | ✅ 直接复用 | 回退逻辑可保留 |
| JIT 段格式 | `jit/JitRuntime.aura` (7KB) | 🔧 可扩展 | `.auc v4` 段格式可复用 |
| JIT ABI | `jit/JitAbi.aura` (8KB) | ✅ 直接复用 | `JitValue`/`AotEntry` ABI 可保留 |
| 字节码发射器 | `codegen/Codegen.aura` (15KB) | ✅ 保留 | VM 路径不变 |
| VM 解释器 | `vm/Vm.aura` (80KB) | ✅ 保留 | VM 路径不变 |

---

## 三、新后端架构总览

### 3.1 四层 IR 架构

```
Source AST → HIR (去糖化，已有)
               │
               ▼
          MIR (SSA, CFG + memory chain)       ← 新增：替换现有 TAC MIR
               │
               ├──→ VM 字节码 (已有，VM 路径不变)
               │
               ▼
          LIR (机器无关 SSA, 含寻址模式标记)    ← 新增：后端 IR
               │
               ▼
          Machine DAG (架构相关指令选择)         ← 新增：DAG Tiling
               │
               ▼
          寄存器分配 (图着色)                    ← 新增
               │
               ├──→ AOT: 指令编码 → 目标文件 → 链接器 → exe
               │
               └──→ JIT: 指令编码 → W^X mmap → 原生执行
```

### 3.2 各层职责

| 层 | 输入 | 输出 | 职责 | 对应 Go/Rust |
|----|------|------|------|-------------|
| **HIR** | AST | HIR（树） | 去糖化、单态化、内联、常量折叠 | HIR（Rust） |
| **MIR** | HIR | MIR（SSA+CFG） | 表达式平坦化、Phi 插入、memory chain | SSA（Go）+ MIR（Rust） |
| **LIR** | MIR | LIR（机器无关 SSA） | 类型降低、地址模式标记、常量合并 | LLVM IR（Rust） |
| **Machine DAG** | LIR | DAG | 指令选择、DAG Tiling | SelectionDAG（Rust） |
| **寄存器分配** | DAG | 着色 DAG | 图着色、溢出处理 | Go FlagAlloc |
| **指令编码** | 着色 DAG | 字节序列 | 机器码生成 | — |
| **AOT 输出** | 字节序列 | .o / .exe | 目标文件格式 + 链接 | — |
| **JIT 执行** | 字节序列 | 可执行内存 | W^X mmap + 分发表 | — |

### 3.3 AOT 与 JIT 的统一与差异

```
                      MIR (SSA)
                         │
                    [共享管线]
                         │
                         ▼
                      LIR (SSA)
                         │
                    [共享管线]
                         │
                         ▼
                   Machine DAG
                         │
                    [共享管线]
                         │
                         ▼
                   寄存器分配
                         │
                         ▼
                   指令编码 (x86_64/aarch64)
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
         AOT 路径               JIT 路径
    ┌──────────────┐      ┌──────────────────┐
    │ 目标文件生成  │      │ W^X mmap 分配    │
    │ (.o / .obj)  │      │ (mmap/VirtualAlloc)│
    │    ↓          │      │    ↓              │
    │ 系统链接器    │      │ 分发表构建       │
    │ (.exe / .so) │      │ (dispatch_table) │
    │    ↓          │      │    ↓              │
    │ 可执行文件    │      │ 原生执行         │
    └──────────────┘      └──────────────────┘
```

**差异点**（仅最后一步）：

| 维度 | AOT | JIT |
|------|-----|-----|
| **输入** | 完整程序（编译时） | 热点函数（运行时） |
| **输出** | 目标文件 + 可执行文件 | 内存中可执行代码 |
| **代码存放** | 磁盘（.exe / .o） | 内存（W^X mmap） |
| **链接** | 系统链接器（ld/lld） | 自研分发表 |
| **优化级别** | 完整优化管线 | 快速编译（跳过部分 pass） |
| **回退** | 无（编译时失败即报错） | deopt 回退解释器 |
| **递归支持** | 完整（直接 call） | dispatch_table + call_indirect |

---

## 四、MIR 层设计（SSA + CFG + Memory Chain）

### 4.1 设计目标

替换现有 TAC 风格 MIR，引入 SSA 形式以支持高级优化。MIR 是**后端共享的单一真相源**：

- VM 路径：MIR → 字节码（现有 Codegen 管线，保持兼容）
- AOT 路径：MIR → LIR → Machine DAG → ...
- JIT 路径：MIR → LIR → Machine DAG → ...

### 4.2 数据结构

#### 4.2.1 类型注册表（TypeRegistry）

参考 Go `types` 包和 Naga `TypeRegistry`，用**类型句柄 + 去重**管理类型：

```aura
// aura/compiler/aura/lang/compiler/mir/TypeRegistry.aura

/// 类型 ID（整数句柄，避免 IR 节点嵌入复杂类型描述）。
/// 类型比较 = 整数比较，序列化 = 整数写入。
class TypeRegistry {
    private var types: List<MirType> = arrayListOf<MirType>()
    private var dedup: HashMap<String, Int> = hashMapOf<String>()

    /// 注册类型，返回句柄 ID（已去重则返回已有 ID）。
    fun register(t: MirType): Int { ... }

    /// 通过 ID 查找类型。
    fun lookup(id: Int): MirType { ... }

    /// 创建整数类型（i8/i16/i32/i64）。
    fun intType(width: Int): Int { ... }

    /// 创建浮点类型（f32/f64）。
    fun floatType(width: Int): Int { ... }

    /// 创建指针类型。
    fun ptrType(pointee: Int): Int { ... }

    /// 创建结构体类型。
    fun structType(name: String, fields: List<MirField>): Int { ... }

    /// 创建函数类型。
    fun funcType(params: List<Int>, ret: Int): Int { ... }

    /// 创建枚举类型（tagged union）。
    fun enumType(name: String, variants: List<MirVariant>): Int { ... }

    /// 获取类型大小/对齐。
    fun size(id: Int): Int { ... }
    fun align(id: Int): Int { ... }
}

class MirType {
    var kind: String       // "Int"/"Float"/"Ptr"/"Struct"/"Array"/"Func"/"Enum"
    var width: Int         // 位宽（标量）
    var name: String       // 类型名（结构体/枚举）
    var fields: List<Int>  // 字段类型 ID（聚合类型）
    var elem: Int          // 元素类型 ID（数组/指针）
}
```

#### 4.2.2 Value 模型（SSA 核心）

每个 `Value` 只被赋值一次，可被多次使用：

```aura
// aura/compiler/aura/lang/compiler/mir/MirValue.aura

class MirValue {
    var id: Int             // ValueID（全局唯一）
    var op: String          // 操作符（"Add"/"Load"/"Phi"/"Const"等）
    var type: Int           // TypeID（结果类型）
    var args: List<Int>     // 操作数 ValueID（数据依赖）
    var aux: String         // 辅助信息（常量值、内存偏移、Phi 入边等）
    var block: Int          // 所属基本块 BlockID
    var useCount: Int       // 使用计数（DCE 用）
}
```

**Memory Chain（显式内存依赖）**：

```aura
// memory 是一个特殊的 Value，类型为 TypeMem
// Store 产生新的 memory，Load 消费 memory 并产生结果 + 新 memory

// 示例：
//   v_mem0 = entry_memory                    // 函数入口的 memory 状态
//   v_addr = alloca(%struct.Foo, v_mem0)      // 分配栈空间
//   v_mem1 = store(v_addr, v_value, v_mem0)  // 写入
//   v_val  = load(v_addr, v_mem1)             // 读取（依赖 store）
//   v_mem2 = store(v_addr, v_newval, v_mem1) // 再次写入
```

**关键设计**：memory 链将内存依赖显式编码进数据流，使得：
- 指令重排序必须尊重 memory 依赖
- 死代码消除可安全传播
- 循环不变代码外提可精确判断

#### 4.2.3 基本块（BasicBlock）

```aura
class MirBlock {
    var id: Int             // BlockID
    var phis: List<Int>     // 块首 Phi 节点 ValueID
    var instrs: List<Int>   // 块内普通指令 ValueID
    var term: Int           // 终结指令 ValueID（Jump/Branch/Return/Switch）
    var succs: List<Int>    // 后继块（从 term 推导）
    var preds: List<Int>    // 前驱块（Phi 求解用）
}
```

#### 4.2.4 MIR 函数

```aura
class MirFunction {
    var name: String        // 函数名
    var type: Int           // 函数类型 TypeID
    var blocks: List<Int>   // 基本块列表
    var entryBlock: Int     // 入口块
    var params: List<Int>   // 参数 ValueID（入口块的首个指令）
    var returnType: Int     // 返回类型 TypeID
    var isNative: Boolean   // 是否为原生函数
}
```

### 4.3 操作符分类

| 类别 | 操作符 | 说明 |
|------|--------|------|
| **常量** | `Const` | 整型/浮点/布尔/字符串/空 |
| **内存** | `Alloca` / `Load` / `Store` / `GEP` | 显式 memory chain |
| **算术** | `Add` / `Sub` / `Mul` / `SDiv` / `SRem` / `FAdd` / `FSub` / `FMul` / `FDiv` | 整数/浮点运算 |
| **位运算** | `And` / `Or` / `Xor` / `Shl` / `Shr` / `AShr` | 按位操作 |
| **比较** | `ICmpEq` / `ICmpNe` / `ICmpSlt` / `ICmpSle` / `ICmpSgt` / `ICmpSge` / `FCmpOeq` / ... | 整数/浮点比较 |
| **类型转换** | `ZExt` / `SExt` / `Trunc` / `F2I` / `I2F` / `BitCast` | 宽度/类型转换 |
| **控制流** | `Br` / `CondBr` / `Ret` / `Switch` | 无条件/条件跳转/返回 |
| **调用** | `Call` / `CallIndirect` | 直接/间接调用 |
| **对象** | `New` / `GetField` / `SetField` / `GetMethod` | 对象构造/字段访问 |
| **ARC** | `Retain` / `Release` / `WeakRef` / `WeakGet` | 自动引用计数 |
| **特殊** | `Phi` / `Unreachable` / `FrameAddr` / `GlobalAddr` | Phi 节点/不可达/栈地址/全局地址 |

### 4.4 SSA 构建算法

从 HIR（树状）构建 SSA MIR 的核心算法：

#### 4.4.1 表达式平坦化（HIR → CFG）

```
func foo(a: Int, b: Int): Int {
    val s = a + b * 2    // 表达式树
    if (s > 10) {
        return s
    } else {
        return s * 2
    }
}
```

降级为 CFG：

```
block entry:
    v1 = Param(a, i32)          // 函数参数
    v2 = Param(b, i32)
    v3 = Const(2, i32)
    v4 = Mul(v2, v3)            // b * 2
    v5 = Add(v1, v4)            // a + (b * 2)
    v6 = Alloca(%struct.int, mem0)
    v7 = Store(v5, v6, mem0)
    v8 = Const(10, i32)
    v9 = ICmpSgt(v5, v8)
    CondBr(v9, block.then, block.else)

block then:
    Ret(v5)

block else:
    v10 = Const(2, i32)
    v11 = Mul(v5, v10)
    Ret(v11)
```

#### 4.4.2 Phi 插入（Cytron 算法）

对于控制流合流点，插入 Phi 节点：

```
block merge:
    v_phi = Phi([v_ret_then, block.then], [v_ret_else, block.else])
    // v_ret_then 来自 then 块
    // v_ret_else 来自 else 块
```

### 4.5 优化 Pass（在 SSA MIR 上直接运行）

| Pass | 实现方式 | 收益 |
|------|---------|------|
| **常量传播** | 遇到 `Const` 操作数直接替换 | 编译期计算简化 |
| **死代码消除** | 从根（`Ret`/`Store`/`Call`）反向标记可达 Value | 移除无用指令 |
| **全局值编号 (GVN)** | 对 `(Op, Type, Args)` 做哈希去重 | 消除重复计算 |
| **部分冗余消除 (PRE)** | 基于支配边界计算可用表达式 | 循环中冗余表达式外提 |
| **循环不变代码外提 (LICM)** | 在回边块上识别不变指令 | 减少循环体开销 |
| **强度削弱** | `/2^n` → `Shl`、`*2^n` → `Shl` | 用移位替代乘除 |
| **条件移动** | 简单 if-else 转为条件选择指令 | 减少分支（x86 CMOV） |

---

## 五、LIR 层设计（机器无关 SSA + 寻址模式标记）

### 5.1 设计目标

LIR 是后端 IR，从 MIR 经 lowering 得到。核心差异：

- 保留 SSA 形式
- 增加**寻址模式标记**（让后端知道哪些表达式可融合到寻址）
- 增加**类型降低**（将高位类型拆分为低位操作）
- 常量合并（去重）

### 5.2 LIR 数据结构

LIR 复用 MIR 的 Value/Block 结构，增加：

```aura
class LirValue {
    // ... MirValue 的所有字段 ...

    var addrMode: String   // "Base"/"Base+Index"/"Base+Index*Scale+Offset"（寻址模式标记）
    var cost: Int          // 指令代价（DAG Tiling 用）
    var lowered: Boolean   // 是否已降低（高位类型拆分标记）
}
```

### 5.3 Lowering 规则

采用**规则驱动**的 lowering，每条规则匹配 IR 子树形状并产生替换：

```aura
// lowering 规则定义
class LowerRule {
    var pattern: LowerPattern      // 匹配的 IR 子树形状
    var action: func(machine, v, match) → List<ValueID>  // 替换动作
    var cost: Int                   // 规则代价（优先级）
}

// 规则示例（x86_64）：
// 规则 1: (Add (Load x) (Const y)) → LEA(x.base, x.index, y.imm)
// 匹配：Add 节点，第一个参数是 Load，第二个参数是 Const
// 动作：生成 LEA 指令

// 规则 2: (SDiv x (Const 2^n)) → AShr(x, n)
// 匹配：SDiv 节点，第二个参数是 2 的幂
// 动作：生成 AShr 指令
```

### 5.4 目标架构选择

| 架构 | 状态 | 优先级 |
|------|------|--------|
| **x86_64** | Phase 1 实现 | P0 |
| **aarch64** | Phase 2 实现 | P1 |

---

## 六、Machine DAG 指令选择

### 6.1 设计目标

将 LIR 的表达式树转换为架构相关的 DAG，用**DAG Tiling**（动态规划）寻找最小代价的指令覆盖。

### 6.2 DAG 节点

```aura
class DagNode {
    var op: String              // 架构指令（"ADD_RR"/"LEA_RR"/"MOV_RI"等）
    var cost: Int               // 指令代价
    var inputs: List<Int>       // 输入节点 DAG ID
    var result: Int             // 结果 ValueID（LIR 层）
    var imm: Int                // 立即数
    var memOperand: String      // 内存操作数描述（寻址模式）
}
```

### 6.3 指令分类

#### x86_64 核心指令集（P0）

**整数运算（R/M 32 和 R/M 64）**：

| 操作 | x86_64 指令 | 编码 |
|------|------------|------|
| `add` | `ADD r32, r32` / `ADD r64, r64` | 01 00 / 03 01 |
| `sub` | `SUB r32, r32` / `SUB r64, r64` | 29 2B / 2B 29 |
| `mul` | `IMUL r32, r32` / `IMUL r64, r64` | 0F AF |
| `div` | `IDIV r32, r32` / `IDIV r64, r64` | 0F BF（需 CQO） |
| `rem` | `IDIV` + 取 EDX/RDX | 同上 |
| `and` | `AND r32, r32` / `AND r64, r64` | 21 25 |
| `or` | `OR r32, r32` / `OR r64, r64` | 09 0D |
| `xor` | `XOR r32, r32` / `XOR r64, r64` | 31 35 |
| `shl` | `SHL r32, r/m32` / `SHL r64, r/m64` | C1 /D3 |
| `shr` | `SHR r32, r/m32` / `SHR r64, r/m64` | C1 /1 / D3 /1 |
| `sar` | `SAR r32, r/m32` / `SAR r64, r/m64` | C1 /1 / D3 /1 |
| `neg` | `NEG r32` / `NEG r64` | F7 /3 |
| `not` | `NOT r32` / `NOT r64` | F7 /2 |

**浮点运算（XMM 寄存器）**：

| 操作 | x86_64 指令 | 编码 |
|------|------------|------|
| `add` | `ADDSS xmm, xmm` / `ADDPD xmm, xmm` | 0F 58 |
| `sub` | `SUBSS xmm, xmm` / `SUBPD xmm, xmm` | 0F 5C |
| `mul` | `MULSS xmm, xmm` / `MULPD xmm, xmm` | 0F 59 |
| `div` | `DIVSS xmm, xmm` / `DIVPD xmm, xmm` | 0F 5E |

**比较与分支**：

| 操作 | x86_64 指令 | 说明 |
|------|------------|------|
| `cmp` | `CMP r32, r/m32` / `CMP r64, r/m64` | 设置标志位 |
| `test` | `TEST r32, r/m32` / `TEST r64, r/m64` | 标志位设置 |
| `je` / `jne` | `JZ rel32` / `JNZ rel32` | 条件跳转 |
| `jl` / `jge` | `JL rel32` / `JGE rel32` | 有符号比较 |
| `jle` / `jg` | `JLE rel32` / `JG rel32` | 有符号比较 |
| `jb` / `ja` | 无符号比较跳转 | 用于 `u32` 比较 |
| `ucomiss` | `UCOMISS xmm, xmm` | 浮点比较 |
| `cmovz` | `CMOVZ r, r/m` | 条件移动（消除分支） |

**函数调用**：

| 操作 | x86_64 指令 | 说明 |
|------|------------|------|
| `call` | `CALL rel32` / `CALL r/m64` | 直接/间接调用 |
| `ret` | `RET` | 返回 |
| `jmp` | `JMP rel32` / `JMP r/m64` | 跳转 |

**内存操作**：

| 操作 | x86_64 指令 | 说明 |
|------|------------|------|
| `mov` | `MOV r32, r/m32` / `MOV r64, r/m64` | 寄存器/内存 |
| `mov` | `MOV r/m32, imm32` | 立即数 |
| `lea` | `LEA r64, [r/m64]` | 取地址 |
| `push` | `PUSH r64` | 入栈 |
| `pop` | `POP r64` | 出栈 |

**栈帧（Prologue/Epilogue）**：

```nasm
; Prologue (Windows x64 MSVC)
push rbp
mov rbp, rsp
sub rsp, 0x30        ; 分配局部空间 + 16 字节对齐
; ... 保留 32 字节 shadow space（MSVC 要求）

; Epilogue
leave                ; mov rsp, rbp; pop rbp
ret
```

### 6.4 DAG Tiling 算法

将表达式树转为 DAG，用动态规划寻找最小代价的 tile 覆盖：

```
输入: 表达式树 DAG
输出: 最小代价指令序列

算法:
1. 将表达式树转为 DAG（公共子表达式共享）
2. 对 DAG 做后序遍历
3. 对每个子树，尝试所有可用的 tile（指令模式）
4. 选择最小代价的 tile 覆盖
5. 递归处理剩余节点

示例: a[i+1] 的地址计算
  表达式树: add(mul(i, 4), add(a, 4))
  DAG 节点: {i, mul_4, add_4, a, addr_result}
  最佳 tile: LEA(r, [a + i*4 + 4])  ← 一条 LEA 覆盖整个子树
```

### 6.5 复杂寻址模式（x86_64）

| 模式 | 编码 | 说明 |
|------|------|------|
| `base` | `[r/m64]` | 单基址 |
| `base + offset` | `[r/m64 + imm32]` | 基址 + 偏移 |
| `base + index` | `[r/m64 + r/m64]` | 基址 + 索引 |
| `base + index*2` | `[r/m64 + r/m64*2]` | 基址 + 索引×2 |
| `base + index*4` | `[r/m64 + r/m64*4]` | 基址 + 索引×4 |
| `base + index*8` | `[r/m64 + r/m64*8]` | 基址 + 索引×8 |
| `base + index*scale + offset` | `[r/m64 + r/m64*scale + imm32]` | 完整寻址 |

---

## 七、寄存器分配

### 7.1 设计目标

采用**图着色**的乐观版本（Optimistic Coloring）：

1. 构建干涉图
2. 简化：移除度数 < K 的节点（K = 物理寄存器数）
3. 乐观着色：对高度数节点假设"可能着色"
4. 若失败则标记溢出
5. 溢出处理：插入 `Store`/`Load`，重新运行

### 7.2 干涉图构建

```aura
// 两个 Value 的活跃区间重叠 → 连边
// 活跃区间 = 从定义点到最后一个使用点（考虑控制流）

class InterferenceGraph {
    var nodes: List<Int>     // ValueID 列表
    var edges: HashMap<Int, List<Int>>  // ValueID → 相邻 ValueID
    var degree: HashMap<Int, Int>        // ValueID → 度数

    fun build(lir: LirFunction) { ... }
}
```

### 7.3 物理寄存器分配（x86_64）

| 用途 | 寄存器 | 说明 |
|------|--------|------|
| **调用约定参数** | RCX, RDX, R8, R9 | Windows x64 MSVC 前 4 个参数 |
| **返回值** | RAX | 整数返回值 |
| **栈指针** | RSP | 保留 |
| **帧指针** | RBP | 保留（可选：省略以节省寄存器） |
| **通用分配** | RBX, RSI, RDI, R12-R15 | 可分配寄存器 |
| **浮点分配** | XMM0-XMM15 | 浮点寄存器 |

**可用通用寄存器数**：8 个（RBX, RSI, RDI, R12-R15）  
**调用保存寄存器**：RBX, RSI, RDI, R12-R15（调用前后需保存/恢复）  
**调用擦除寄存器**：RAX, RCX, RDX, R8, R9, R10, R11

### 7.4 溢出策略

```aura
// 溢出 = 将 Value 写入栈槽
// 溢出处理流程：
// 1. 将溢出 Value 的所有使用点替换为 Load
// 2. 在溢出 Value 的定义点后插入 Store
// 3. 在溢出 Value 的定义前插入 alloca（分配栈槽）
// 4. 重新构建干涉图，重新着色

// 溢出代价（用于指导选择哪些 Value 溢出）：
//   cost = (溢出次数 × spill_penalty) + (重新着色代价)
```

### 7.5 栈帧布局

```
高地址
┌─────────────────┐
│  返回地址        │
├─────────────────┤
│  被调用者保存寄存器 (RBX, RSI, RDI, R12-R15)
├─────────────────┤
│  溢出寄存器      │  ← 图着色溢出的值
├─────────────────┤
│  局部变量        │  ← alloca 的栈分配
├─────────────────┤
│  Shadow Space (32 bytes, MSVC)
├─────────────────┤
│  参数溢出区      │  ← 第 5 个及后续参数
└─────────────────┘
RSP (16 字节对齐)
低地址
```

---

## 八、指令编码

### 8.1 x86_64 指令编码格式

```
[REX 前缀][MOD R/M][立即数]

REX 前缀:
  4 位: W R X B
    W: 0 = 32 位操作, 1 = 64 位操作
    R: 扩展 ModRM 的 reg 字段
    X: 扩展 SIB 的 index 字段
    B: 扩展 ModRM/SIB 的 base 字段

MOD R/M:
  2 位 MOD: 00 = 内存, 01 = 基址+8位偏移, 10 = 基址+32位偏移, 11 = 寄存器
  3 位 R/M: 寄存器编码
  3 位 Reg: 寄存器编码
```

### 8.2 编码实现

```aura
// aura/compiler/aura/lang/compiler/backend/x86_64/Encoder.aura

class X86Encoder {
    var buffer: ByteCodeBuffer  // 输出字节序列

    // MOV r/m64, imm64
    fun movRI(opReg: Int, imm: Long): Int {
        // REX.W + B + 0xB8 + reg + imm64
        writeByte(0x48 | (opReg << 3))
        writeByte(0xB8 | (opReg & 7))
        writeLong(imm)
    }

    // MOV r/m64, r/m64
    fun movRR(dst: Int, src: Int): Int {
        writeByte(0x48)
        writeModRM(0xC7, dst, src)  // MOD=11 (reg), /0
    }

    // ADD r/m64, r/m64
    fun addRR(dst: Int, src: Int): Int {
        writeByte(0x48)
        writeModRM(0xD3, dst, src)
    }

    // LEA r64, [base + index*scale + offset]
    fun leaRR(dst: Int, base: Int, index: Int, scale: Int, offset: Int): Int {
        writeByte(0x48)
        writeModRM(0x8D, dst, base)
        // ... SIB 编码
    }

    // CALL rel32
    fun callR32(imm: Int): Int {
        writeByte(0xE8)
        writeInt(imm)
    }

    // JMP rel32
    fun jmpR32(imm: Int): Int {
        writeByte(0xE9)
        writeInt(imm)
    }

    // RET
    fun ret(): Int {
        writeByte(0xC3)
    }

    // PUSH r64
    fun pushR(reg: Int): Int {
        writeByte(0x50 | (reg & 7))
    }

    // POP r64
    fun popR(reg: Int): Int {
        writeByte(0x58 | (reg & 7))
    }
}
```

### 8.3 字节码缓冲区

```aura
// 高效的字节码缓冲区（避免 O(n²) 拼接）
class ByteCodeBuffer {
    private var data: ByteArray  // 预分配缓冲区
    private var pos: Int         // 当前写入位置
    private var cap: Int         // 容量

    fun writeByte(b: Int): Int { ... }
    fun writeShort(b: Short): Int { ... }
    fun writeInt(b: Int): Int { ... }
    fun writeLong(b: Long): Int { ... }

    fun toByteArray(): ByteArray { ... }
    fun size(): Int { ... }
}
```

---

## 九、AOT 路径设计

### 9.1 完整管线

```
MIR (SSA)
    │
    ▼ [Lowering]
LIR (机器无关 SSA)
    │
    ▼ [Instruction Selection]
Machine DAG
    │
    ▼ [Register Allocation]
着色 DAG
    │
    ▼ [Instruction Encoding]
字节序列
    │
    ▼ [Object File Generation]
目标文件 (.o / .obj)
    │
    ▼ [System Linker]
可执行文件 (.exe / .out / .so)
```

### 9.2 目标文件格式

#### COFF（Windows）

```aura
class CoffObject {
    var header: CoffHeader      // 64 字节文件头
    var sections: List<CoffSection>  // 节表

    // .text 节：机器代码
    // .rdata 节：只读数据（字符串常量）
    // .data 节：可读写数据
    // .pdata 节：异常处理数据
    // .xdata 节：异常处理描述
}
```

#### ELF（Linux）

```aura
class ElfObject {
    var header: ElfHeader       // 64 字节文件头
    var sections: List<ElfSection>  // 节表

    // .text 节：机器代码
    // .rodata 节：只读数据
    // .data 节：可读写数据
    // .eh_frame 节：异常处理帧
}
```

### 9.3 链接器接口

```aura
// 不再依赖外部 llc/clang，使用系统链接器（ld/lld）直接链接目标文件

class SystemLinker {
    // 链接目标文件为可执行文件
    fun linkExecutable(objects: List<String>, outputPath: String): Boolean {
        // Windows: lld-link / SUBSYSTEM:CONSOLE obj1.obj obj2.obj /OUT:out.exe
        // Linux:   ld obj1.o obj2.o -o out --dynamic-linker /lib64/ld-linux.so.2
    }
}
```

### 9.4 ARC（自动引用计数）

ARC 在新后端中的实现：

```
MIR 指令:    Retain(src)  →  call @aura_arc_increment(src)
             Release(src) →  call @aura_arc_decrement(src)

在 MIR 中显式插入（已有 MIR 降级阶段完成），后端只负责将 Call 映射为指令。
```

### 9.5 异常处理（简化方案）

**方案 A：setjmp/longjmp（最小实现）**

```
try {
    body
} catch (e) {
    handler
}

发射为：
1. 在 try 前分配异常栈槽
2. 在 try 体开始调用 setjmp（保存上下文）
3. 异常路径调用 longjmp 跳回 setjmp 点
4. setjmp 返回非零表示从 longjmp 返回
```

**优点**：实现简单，无需栈展开表  
**缺点**：跨线程不安全，资源清理不完整

---

## 十、JIT 路径设计

### 10.1 完整管线

```
运行时热点检测 (JitState, 已有)
    │
    ▼
MIR (SSA, 该函数的子集)
    │
    ▼ [Lowering, 快速模式]
LIR (机器无关 SSA)
    │
    ▼ [Instruction Selection]
Machine DAG
    │
    ▼ [Register Allocation, 简化模式]
着色 DAG
    │
    ▼ [Instruction Encoding]
字节序列
    │
    ▼ [W^X mmap 分配]
可执行内存
    │
    ▼ [分发表构建]
原生执行 + 回退支持
```

### 10.2 JIT 与 AOT 的差异处理

| 维度 | AOT | JIT |
|------|-----|-----|
| **优化级别** | 完整（GVN/LICM/内联/循环展开） | 快速（常量传播/DCE/强度削弱） |
| **寄存器分配** | 完整图着色 | 简化（线性扫描 + 少量溢出） |
| **栈帧** | 完整（局部变量 + 溢出 + 对齐） | 最小（仅必要寄存器保存） |
| **调用** | 直接 call | dispatch_table + call_indirect（支持递归） |
| **回退** | 无 | deopt → 回退 VM 解释器 |
| **内存映射** | 磁盘文件 | W^X mmap（mmap/VirtualAlloc） |
| **描述符表** | 无 | AuraFuncDesc（32 字节）+ 段格式 |

### 10.3 W^X 内存映射

```aura
// JIT 原生码需要可执行内存

class WxMemory {
    // 分配可执行内存（mmap PROT_READ|PROT_WRITE|PROT_EXEC 或 VirtualAlloc）
    fun allocate(size: Int): Int {
        // 返回内存地址
    }

    // 写入机器码
    fun write(addr: Int, code: ByteCodeBuffer): Boolean {
        // 写入字节序列
    }

    // 获取函数入口地址
    fun getEntry(addr: Int, offset: Int): Int {
        return addr + offset
    }

    // 释放
    fun free(addr: Int): Boolean {
        // munmap 或 VirtualFree
    }
}
```

### 10.4 分发表（Dispatch Table）

```aura
// 用于递归函数和互递归的间接调用
// 与现有 JitAbi.aura 的 dispatch_table 格式兼容

class DispatchTable {
    private var entries: List<Int>  // 函数入口地址列表

    // 获取函数入口地址
    fun lookup(funcIndex: Int): Int {
        return entries[funcIndex]
    }

    // 注册函数入口
    fun register(funcIndex: Int, entryAddr: Int): Boolean {
        entries[funcIndex] = entryAddr
    }
}
```

### 10.5 JIT 编译时机

沿用现有 JitState.aura 的热点检测逻辑：

```aura
// 已有逻辑，保持不变：
// 1. 调用计数（call_counts）
// 2. 阈值检测（hotspot_threshold = 10000）
// 3. Fix A: 入口函数强制编译（jitForceEntry）
// 4. Fix B: 递归函数支持（dispatch_table + call_indirect）
// 5. 白名单检测（is_jit_compilable）
// 6. 回退解释器（不可编译函数）
```

### 10.6 回退（Deoptimization）

```aura
// JIT 执行中遇到不可处理的指令 → deopt
// 回退到 VM 解释器，从当前 IP 继续执行

class DeoptInfo {
    var funcIndex: Int       // 函数索引
    var ip: Int              // 回退时的指令位置
    var stackState: String   // 栈状态快照
    var localsState: String  // 局部变量状态
}
```

---

## 十一、调用约定

### 11.1 Windows x64 (MSVC)

```
参数:   RCX, RDX, R8, R9（前 4 个）
栈:     第 5 个及后续参数
对齐:   RSP % 16 == 8（调用时）/ 0（返回时）
Shadow Space: 每个被调函数预留 32 字节
返回值: RAX（整数）/ XMM0（浮点）/ RAX:RDX（128 位）
清理:   调用者负责（caller cleanup）
```

### 11.2 System V AMD64 (Linux/macOS)

```
参数:   RDI, RSI, RDX, RCX, R8, R9（前 6 个）
栈:     第 7 个及后续参数
对齐:   RSP % 16 == 0（调用前）
返回值: RAX（整数）/ XMM0（浮点）/ RAX:RDX（128 位）
清理:   被调方负责（callee cleanup）
```

### 11.3 AAPCS64 (aarch64)

```
参数:   X0-X7
栈:     第 9 个及后续参数
对齐:   SP % 16 == 0
返回值: X0（整数）/ D0（浮点）/ X0:X1（128 位）
```

---

## 十二、模块结构

```
aura/compiler/aura/lang/compiler/
├── mir/                          # MIR 层（SSA 重构）
│   ├── Mir.aura                 # MIR 定义（替换 TAC，新增 SSA Value/Block）
│   ├── MirLower.aura            # HIR → MIR（SSA 构建 + Phi 插入）
│   ├── MirOpt.aura              # MIR 优化（扩展：GVN/PRE/LICM）
│   ├── TypeRegistry.aura        # 类型注册表（新增）
│   └── SsaBuilder.aura          # SSA 构建算法（新增，Cytron 算法）
│
├── backend/                      # 后端（新增目录）
│   ├── Lir.aura                 # LIR 定义（机器无关 SSA）
│   ├── Lowering.aura            # MIR → LIR lowering（规则驱动）
│   ├── Dag.aura                 # Machine DAG 定义
│   ├── InstSelect.aura          # 指令选择（DAG Tiling）
│   ├── RegAlloc.aura            # 寄存器分配（图着色）
│   ├── Encoder.aura             # 指令编码接口
│   ├── FrameLayout.aura         # 栈帧布局
│   └── abi/
│       ├── CallingConv.aura     # 调用约定接口
│       └── WindowsX64.aura      # Windows x64 实现
│
├── aot/                          # AOT 路径（重构）
│   ├── Aot.aura                 # AOT 编排器（重构：MIR → LIR → DAG → RegAlloc → Encode → .o）
│   ├── ObjectFormat.aura        # 目标文件格式（COFF/ELF）
│   ├── SystemLinker.aura        # 系统链接器接口
│   └── Runtime.aura             # Runtime 声明（保留，扩展）
│
├── jit/                          # JIT 路径（重构）
│   ├── JitCore.aura             # JIT 核心（重构：MIR → LIR → DAG → RegAlloc → Encode → mmap）
│   ├── JitRuntime.aura          # W^X 内存映射 + 段加载（保留，扩展）
│   ├── JitDispatch.aura         # 派发/回退（保留）
│   ├── JitState.aura            # 热点检测（保留）
│   ├── JitOpt.aura              # JIT 优化传递（保留）
│   └── DispatchTable.aura       # 分发表（新增，与 JitAbi 集成）
│
└── x86_64/                       # x86_64 架构（新增目录）
    ├── X86Inst.aura             # x86_64 指令定义
    ├── X86Encoder.aura          # x86_64 指令编码
    ├── X86Abi.aura              # x86_64 调用约定
    └── X86Flags.aura            # 标志寄存器管理（x86 特殊处理）
```

---

## 十三、与现有系统的集成

### 13.1 后端选择器

```aura
// 后端选择器：根据目标/优化级别路由

class BackendSelector {
    fun selectBackend(target: String, optLevel: String, mode: String): String {
        // mode: "aot" / "jit"
        // target: "x86_64-pc-windows-msvc" / "aarch64-unknown-linux-gnu"
        // optLevel: "0" / "1" / "2"

        if (mode == "aot") {
            return "AotBackend"
        } else if (mode == "jit") {
            return "JitBackend"
        } else {
            return "VmBackend"  // 字节码解释
        }
    }
}
```

### 13.2 与 VM 路径的兼容

MIR 重构后，VM 路径仍需支持：

```
新 MIR (SSA) → 线性化（SSA → TAC）→ Codegen → 字节码 → VM
```

**线性化策略**：
1. 将 SSA Value 分配为栈槽（类似 TAC 的寄存器槽）
2. Phi 节点转化为分支 + 赋值
3. memory chain 线性化为 load/store 序列
4. 复用现有 Codegen.aura 的字节码发射器

### 13.3 与现有 HIR 的对接

HIR 不变，新增 MIRLowerer 从 HIR 构建 SSA MIR：

```
HIR (树, List arena, 已有)
    │
    ▼ [新建 SsaBuilder]
MIR (SSA, CFG, memory chain, 新增)
```

---

## 十四、性能预期

### 14.1 编译速度

| 场景 | 当前（LLVM llc 子进程） | 新后端（纯 Aura） | 预期 |
|------|----------------------|------------------|------|
| 简单函数（10 行） | 200-800ms | 50-200ms | 2-4x 快 |
| 中等函数（100 行） | 500-2000ms | 100-400ms | 2-5x 快 |
| 大型程序（1000 行） | 2-8s | 0.5-2s | 2-4x 快 |

### 14.2 运行性能

| 场景 | LLVM O2 | 新后端（无优化） | 新后端（+基础优化） |
|------|---------|-----------------|-------------------|
| 整数密集 | 基线 1.0x | 1.5-2.5x 慢 | 1.1-1.5x 慢 |
| 浮点密集 | 基线 1.0x | 1.3-2.0x 慢 | 1.0-1.3x 慢 |
| 内存密集（ARC） | 基线 1.0x | 1.3-1.8x 慢 | 1.1-1.4x 慢 |
| 循环密集 | 基线 1.0x | 2.0-3.0x 慢 | 1.3-1.8x 慢 |

### 14.3 JIT 性能

| 场景 | VM 解释器 | JIT（新后端） | 预期 |
|------|----------|-------------|------|
| 整数循环 | 基线 1.0x | 30-50x 快 | 接近原生 |
| 递归函数 | 基线 1.0x | 20-40x 快 | 接近原生 |
| 方法调用 | 基线 1.0x | 5-15x 快 | 接近原生 |

---

## 十五、实施路线图

### 15.1 Phase A：MIR SSA 重构（2-3 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| TypeRegistry | `mir/TypeRegistry.aura` | 2d |
| MirValue/MirBlock 定义 | `mir/Mir.aura`（重构） | 3d |
| HIR → SSA MIR 构建 | `mir/SsaBuilder.aura` | 5d |
| Phi 插入（Cytron 算法） | `mir/SsaBuilder.aura` | 3d |
| Memory chain 插入 | `mir/MirLower.aura`（扩展） | 3d |
| 线性化（SSA → TAC → 字节码） | `mir/Linearizer.aura` | 3d |
| VM 路径回归测试 | 测试用例 | 2d |

### 15.2 Phase B：LIR + Lowering（2 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| LIR 定义 | `backend/Lir.aura` | 2d |
| Lowering 规则框架 | `backend/Lowering.aura` | 3d |
| 整数/浮点运算 lowering | `backend/Lowering.aura` | 2d |
| 内存操作 lowering | `backend/Lowering.aura` | 2d |
| 调用约定 lowering | `backend/Lowering.aura` | 2d |
| MIR → LIR 回归测试 | 测试用例 | 1d |

### 15.3 Phase C：Machine DAG + 指令选择（2-3 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| DAG 数据结构 | `backend/Dag.aura` | 2d |
| DAG Tiling 算法 | `backend/InstSelect.aura` | 5d |
| x86_64 指令定义 | `x86_64/X86Inst.aura` | 3d |
| 寻址模式融合 | `backend/InstSelect.aura` | 3d |
| 指令选择测试 | 测试用例 | 2d |

### 15.4 Phase D：寄存器分配（2 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| 干涉图构建 | `backend/RegAlloc.aura` | 3d |
| 图着色算法 | `backend/RegAlloc.aura` | 3d |
| 溢出处理 | `backend/RegAlloc.aura` | 3d |
| 栈帧布局 | `backend/FrameLayout.aura` | 2d |
| 寄存器分配测试 | 测试用例 | 1d |

### 15.5 Phase E：指令编码 + AOT 输出（2 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| x86_64 编码器 | `x86_64/X86Encoder.aura` | 4d |
| COFF 目标文件格式 | `aot/ObjectFormat.aura` | 3d |
| 系统链接器接口 | `aot/SystemLinker.aura` | 2d |
| AOT 编排器重构 | `aot/Aot.aura`（重构） | 3d |
| AOT 端到端测试 | 测试用例 | 2d |

### 15.6 Phase F：JIT 路径（2-3 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| JIT 核心重构 | `jit/JitCore.aura`（重构） | 3d |
| W^X 内存映射 | `jit/JitRuntime.aura`（扩展） | 2d |
| 分发表集成 | `jit/DispatchTable.aura` | 2d |
| 回退（deopt）支持 | `jit/JitDispatch.aura`（扩展） | 2d |
| JIT 端到端测试 | 测试用例 | 2d |

### 15.7 Phase G：优化 Pass 扩展（2 周）

| 任务 | 文件 | 预估 |
|------|------|------|
| GVN（全局值编号） | `mir/MirOpt.aura`（扩展） | 3d |
| LICM（循环不变代码外提） | `mir/MirOpt.aura`（扩展） | 3d |
| 条件移动（x86 CMOV） | `backend/Lowering.aura`（扩展） | 2d |
| 简单循环展开 | `mir/MirOpt.aura`（扩展） | 2d |
| 优化测试 | 测试用例 | 2d |

### 15.8 Phase H：aarch64 后端（3-4 周，可选）

| 任务 | 文件 | 预估 |
|------|------|------|
| aarch64 指令定义 | `aarch64/Aarch64Inst.aura` | 5d |
| aarch64 编码器 | `aarch64/Aarch64Encoder.aura` | 5d |
| aarch64 调用约定 | `aarch64/Aarch64Abi.aura` | 3d |
| aarch64 端到端测试 | 测试用例 | 2d |

### 15.9 总工期

| Phase | 内容 | 预估 |
|-------|------|------|
| A | MIR SSA 重构 | 2-3 周 |
| B | LIR + Lowering | 2 周 |
| C | Machine DAG + 指令选择 | 2-3 周 |
| D | 寄存器分配 | 2 周 |
| E | 指令编码 + AOT 输出 | 2 周 |
| F | JIT 路径 | 2-3 周 |
| G | 优化 Pass 扩展 | 2 周 |
| H | aarch64（可选） | 3-4 周 |
| **总计** | | **15-19 周（约 4-5 个月）** |

---

## 十六、风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| SSA MIR 重构破坏 VM 路径 | 中 | 高 | 先实现线性化器（SSA→TAC），确保 VM 路径不受影响 |
| x86_64 指令编码错误 | 高 | 高 | 与 LLVM 生成代码逐字节对比 + 单元测试 |
| 寄存器分配溢出过多 | 中 | 中 | 先实现线性扫描（简单），再升级为图着色 |
| JIT 原生码执行崩溃 | 中 | 高 | W^X mmap 错误保护 + 回退解释器 |
| 栈帧布局错误导致崩溃 | 中 | 高 | 对齐检查 + shadow space 严格遵循调用约定 |
| 异常处理不完整 | 高 | 中 | 初始采用 setjmp/longjmp 简化方案 |
| 自举失败（新后端编译自身） | 中 | 高 | 保留 LLVM 路径作为 fallback；逐步替换 |
| 优化 Pass 引入语义错误 | 中 | 高 | 每个 Pass 配差分测试（优化前 vs 优化后结果一致） |

---

## 十七、验证策略

### 17.1 差分测试

```
同一份 Aura 源码 → 三条路径编译 → 对比执行结果

VM 路径:    MIR → 线性化 → 字节码 → VM 解释 → 结果 A
AOT 路径:   MIR → LIR → DAG → RegAlloc → Encode → .o → .exe → 结果 B
JIT 路径:   MIR → LIR → DAG → RegAlloc → Encode → mmap → 结果 C

断言: A == B == C
```

### 17.2 指令编码验证

```
同一表达式 → 新后端编码 vs LLVM 编码 → 逐字节对比

// 新后端
val encoder = X86Encoder()
encoder.addRR(RAX, RBX)
val bytes1 = encoder.toByteArray()

// LLVM
// llvm-objdump -d --no-show-raw-insn <(llc -filetype=obj test.ll)
// 提取 ADD 指令字节

// 对比
assert(bytes1 == expectedLlvmBytes)
```

### 17.3 寄存器分配验证

```
同一函数 → 新后端寄存器分配 → 检查：
1. 无未分配的虚拟寄存器
2. 调用擦除寄存器未跨 call 使用
3. 调用保存寄存器正确保存/恢复
4. 溢出代码正确（load/store 配对）
```

---

## 十八、与 Go / Rust 的对应关系

| 新后端设计 | Go 对应 | Rust 对应 | 说明 |
|-----------|---------|-----------|------|
| **HIR** | noder 阶段 | HIR | 去糖化、单态化 |
| **MIR (SSA)** | SSA 初始形式 | MIR + Borrow Check | CFG + SSA + memory chain |
| **LIR** | Lowering 后的 SSA | LLVM IR | 机器无关 SSA |
| **Machine DAG** | 架构 lowering 规则 | SelectionDAG | 指令选择 |
| **指令选择** | rewrite rules | ISel patterns | 规则驱动 |
| **寄存器分配** | Optimistic Coloring | LLVM RegAlloc | 图着色 |
| **标志寄存器** | FlagAlloc | x86 Flags | x86 特殊处理 |
| **AOT 输出** | — | LLVM TargetMachine | 目标文件生成 |
| **JIT 执行** | — | Cranelift | W^X mmap + 分发表 |

---

## 十九、一句话总结

> **以 SSA 化 MIR 为单一真相源，通过 LIR → Machine DAG → 图着色寄存器分配 → 指令编码的统一后端管线，实现纯 Aura 自研的 AOT + JIT 双路径代码生成，完全替换 LLVM 与 Cranelift 外部依赖。**

---

## 附录 A：x86_64 寄存器编号

| 寄存器 | 编号 | 用途 |
|--------|------|------|
| RAX | 0 | 通用/返回值 |
| RCX | 1 | 参数1/通用 |
| RDX | 2 | 参数2/通用 |
| RBX | 3 | 调用保存/通用 |
| RSP | 4 | 栈指针（保留） |
| RBP | 5 | 帧指针（保留） |
| RSI | 6 | 调用保存/通用 |
| RDI | 7 | 调用保存/通用 |
| R8 | 8 | 参数3/通用 |
| R9 | 9 | 参数4/通用 |
| R10 | 10 | 调用擦除 |
| R11 | 11 | 调用擦除 |
| R12 | 12 | 调用保存/通用 |
| R13 | 13 | 调用保存/通用 |
| R14 | 14 | 调用保存/通用 |
| R15 | 15 | 调用保存/通用 |

## 附录 B：x86_64 ModRM 编码表

| 寄存器 | R/M 编码 |
|--------|---------|
| RAX | 000 |
| RCX | 001 |
| RDX | 010 |
| RBX | 011 |
| RSP | 100 |
| RBP | 101 |
| RSI | 110 |
| RDI | 111 |
| R8-R15 | 需 REX.X/B |

## 附录 C：关键指令编码速查

| 指令 | 操作码 | 说明 |
|------|--------|------|
| `NOP` | 0x90 | 空操作 |
| `RET` | 0xC3 | 返回 |
| `CALL rel32` | 0xE8 | 直接调用 |
| `CALL r/m64` | 0xFF /2 | 间接调用 |
| `JMP rel32` | 0xE9 | 跳转 |
| `JMP r/m64` | 0xFF /4 | 间接跳转 |
| `TEST r/m, r/m` | 0x85 | 测试 |
| `CMOVZ r, r/m` | 0x44 0x0F 0x44 | 条件移动 |
| `LEA r, [r/m]` | 0x8D | 取地址 |
| `MOV r, imm32` | 0xB8+reg | 立即数 |
| `MOV r/m, r/m` | 0x89 | 移动 |
| `ADD r/m, r/m` | 0x01 | 加法 |
| `SUB r/m, r/m` | 0x29 | 减法 |
| `IMUL r, r/m` | 0x0F 0xAF | 乘法 |
| `PUSH r/m` | 0x50+reg | 入栈 |
| `POP r/m` | 0x58+reg | 出栈 |
