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

## 九、AOT 路径设计：从 MIR 到可执行文件

### 9.1 完整管线（六步）

```
MIR (SSA)
    │
    ▼ [Step 1: Lowering]
LIR (机器无关 SSA)
    │
    ▼ [Step 2: Instruction Selection]
Machine DAG (每个函数一棵 DAG)
    │
    ▼ [Step 3: Register Allocation]
着色 DAG (寄存器已分配)
    │
    ▼ [Step 4: Instruction Encoding]
每个函数 → 字节序列 + 重定位表
    │
    ▼ [Step 5: Object File Assembly]
目标文件 (.o / .obj) ← 包含 .text / .rdata / .data 节
    │
    ▼ [Step 6: System Linking]
可执行文件 (.exe / .out) 或 动态库 (.so / .dll)
```

**核心问题**：Step 4 产出的只是"裸机器码字节"，它还不能运行。下面逐步骤解释每个环节具体怎么把字节变成可执行文件。

---

### 9.2 Step 1-3：函数级代码生成（概要）

Step 1-3（Lowering → InstSelect → RegAlloc）将 MIR 转换为每个函数的着色 DAG。详见第五章至第七章。

**关键产出**：每个函数得到一个 `MachineFunction` 结构，包含：

```aura
class MachineFunction {
    var name: String              // 函数名，如 "main"、"add"
    var instructions: List<Mi>    // 最终指令序列（已分配寄存器）
    var frameLayout: FrameLayout  // 栈帧布局（局部变量 + 溢出槽）
    var relocations: List<Reloc>  // 重定位项（跨函数调用点）
    var stringConstants: List<String>  // 函数引用的字符串常量
    var globalRefs: List<String>  // 引用的全局变量名
}

// 一条最终指令（寄存器已分配）
class Mi {
    var opcode: String       // 如 "ADD_RR"、"MOV_RI"、"CALL_R32"
    var operands: List<Int>  // 物理寄存器编号或立即数
    var relocIndex: Int      // 重定位索引（-1 表示无重定位）
}
```

**举例**：`fun add(a: Int, b: Int): Int { return a + b }` 经过 Step 1-3 后：

```aura
MachineFunction("add") {
    instructions = [
        Mi("PUSH_R",    [RBP]),                          // prologue: push rbp
        Mi("MOV_RR",    [RBP, RSP]),                     // prologue: mov rbp, rsp
        Mi("SUB_RI",    [RSP, 0x28]),                    // prologue: sub rsp, 0x28
        Mi("MOV_RR",    [RAX, RCX]),                     // 将参数 a (RCX) 移到 RAX
        Mi("ADD_RR",    [RAX, RDX]),                     // rax = rax + rdx (a + b)
        Mi("LEAVE"),                                     // epilogue: leave
        Mi("RET")                                         // epilogue: ret
    ],
    frameLayout = FrameLayout(localSize=0x28, spillCount=0),
    relocations = [],
    stringConstants = [],
    globalRefs = []
}
```

---

### 9.3 Step 4：指令编码（每个函数 → 字节序列 + 重定位）

每个 `MachineFunction` 的指令序列通过 `X86Encoder`（第八章）编码为字节序列。

**核心产出**：`EncodedFunction` —— 每个函数的机器码字节 + 偏移量 + 重定位项：

```aura
class EncodedFunction {
    var name: String          // 函数名
    var offset: Int           // 在 .text 节中的偏移（字节）
    var size: Int             // 机器码大小（字节）
    var code: ByteArray       // 机器码字节序列
    var relocations: List<RelocItem>  // 需要链接器修复的跳转/调用点
    var align: Int            // 对齐要求（通常 16）
}

// 一个重定位项：标记某个偏移处有一个跨函数引用，链接器需要填充地址
class RelocItem {
    var offset: Int           // 在 .text 节中的偏移（相对于 .text 节起始）
    var symbol: String        // 目标符号名，如 "print"、"aura_arc_increment"
    var type: String          // 重定位类型："R_X86_64_REL32"（Windows）或 "R_X86_64_32S"（ELF）
    var addend: Int           // 加数（通常为 0）
}
```

**编码过程**：

```aura
fun encodeFunction(func: MachineFunction, encoder: X86Encoder, textSection: Section): EncodedFunction {
    // 1. 记录当前偏移（编码前）
    val startOffset: Int = textSection.size

    // 2. 逐条编码指令
    for (mi in func.instructions) {
        if (mi.relocIndex >= 0) {
            // 该指令包含跨函数引用 → 编码时写入占位符（4 字节 0x00）
            // 并记录重定位项
            encoder.writeByte(mi.opcode)       // 操作码
            encoder.writeInt(0)                // 占位：链接器稍后填充
            textSection.relocations.add(RelocItem(
                offset = startOffset + encoder.size,  // 在 .text 中的绝对偏移
                symbol = func.relocations[mi.relocIndex].symbol,
                type = "R_X86_64_REL32",
                addend = 0
            ))
        } else {
            // 普通指令：直接编码
            encoder.emit(mi)
        }
        textSection.append(encoder.drain())  // 将编码字节追加到 .text 节
    }

    // 3. 对齐到 16 字节（函数边界对齐）
    textSection.alignTo(16)

    return EncodedFunction(
        name = func.name,
        offset = startOffset,
        size = textSection.size - startOffset,
        code = textSection.get(startOffset, textSection.size - startOffset),
        relocations = textSection.relocations,
        align = 16
    )
}
```

**举例**：`fun main(): Int { return add(2, 3) }` 编码后：

```aura
// .text 节内容（十六进制）：
// offset 0x00: 55                    // push rbp
// offset 0x01: 48 89 E5              // mov rbp, rsp
// offset 0x04: 48 83 EC 30           // sub rsp, 0x30
// offset 0x08: B9 02 00 00 00       // mov ecx, 2      (参数 a)
// offset 0x0D: BA 03 00 00 00       // mov edx, 3      (参数 b)
// offset 0x12: E8 ?? ?? ?? ??       // call add        ← 重定位！占位 4 字节
// offset 0x17: 5D                    // pop rbp         (leave)
// offset 0x18: C3                    // ret

// 重定位表：
// [RelocItem(offset=0x13, symbol="add", type="R_X86_64_REL32", addend=0)]
//  ↑ offset 0x13 是 call 指令中 4 字节占位的起始位置
//  链接器会计算 add 函数的地址，减去 call 指令下一条的地址，填入这 4 字节
```

---

### 9.4 Step 5：目标文件组装（字节序列 → .obj / .o）

多个 `EncodedFunction` + 字符串常量 + 全局数据需要组装成**目标文件**。

#### 9.4.1 节（Section）的组织

```aura
// 节：目标文件的基本组成单位
class Section {
    var name: String          // 节名，如 ".text"、".rdata"、".data"
    var data: ByteArray       // 节的二进制内容
    var relocations: List<RelocItem>  // 节内的重定位项
    var flags: Int            // 节标志（可执行/可写/只读）

    fun size(): Int { return this.data.length }
    fun append(bytes: ByteArray): Int { ... }
    fun alignTo(alignment: Int): Int { ... }  // 填充到对齐边界
}
```

**目标文件的典型节**：

| 节名 | 内容 | 标志 | 说明 |
|------|------|------|------|
| `.text` | 机器代码 | 可执行、只读 | 所有函数的编码字节 |
| `.rdata` / `.rodata` | 字符串常量、全局常量 | 只读 | `"hello"` 等 |
| `.data` | 全局变量（有初始值） | 可写 | `var x = 5` |
| `.bss` | 全局变量（无初始值） | 可写 | `var y = 0`（只占符号表，不占空间） |
| `.symtab` | 符号表 | — | 函数名 → 节内偏移 |
| `.reloc` | 重定位表 | — | 偏移 → 目标符号 |

#### 9.4.2 构建流程

```aura
fun buildObjectFile(
    functions: List<MachineFunction>,     // Step 3 产出
    globalVars: List<GlobalVar>,           // HIR 中的全局变量
    stringConsts: List<String>             // HIR 中的字符串常量
): ByteArray {
    // ── 1. 创建节 ──
    val textSec = Section(".text", flags = EXEC_READ)
    val rdataSec = Section(".rdata", flags = READ)
    val dataSec = Section(".data", flags = READ_WRITE)
    val bssSec = Section(".bss", flags = READ_WRITE)

    // ── 2. 编码每个函数 → .text 节 ──
    val encoder = X86Encoder()
    val symbolTable = SymbolTable()

    for (func in functions) {
        textSec.alignTo(16)  // 函数 16 字节对齐

        val funcOffset = textSec.size
        symbolTable.define(func.name, ".text", funcOffset)

        // 逐条编码指令
        for (mi in func.instructions) {
            if (mi.relocIndex >= 0) {
                // 含重定位的指令：写入操作码 + 4 字节占位
                encoder.emitWithReloc(mi, func.relocations[mi.relocIndex].symbol)
                textSec.append(encoder.drain())
                textSec.relocations.add(RelocItem(
                    offset = funcOffset + /* 指令在函数内的偏移 */,
                    symbol = func.relocations[mi.relocIndex].symbol,
                    type = "R_X86_64_REL32"
                ))
            } else {
                encoder.emit(mi)
                textSec.append(encoder.drain())
            }
        }
    }

    // ── 3. 字符串常量 → .rdata 节 ──
    for (s in stringConsts) {
        val strOffset = rdataSec.size
        rdataSec.append(s.toByteArray())
        rdataSec.append(byteArrayOf(0))  // NUL 终止
        // 记录符号：@str.0 → .rdata + 0
        symbolTable.define("@str." + symbolTable.nextStrIdx(), ".rdata", strOffset)
    }

    // ── 4. 全局变量 → .data 或 .bss 节 ──
    for (gv in globalVars) {
        if (gv.hasInitializer) {
            dataSec.append(gv.initializerBytes)
            symbolTable.define(gv.name, ".data", dataSec.size - gv.initializerBytes.length)
        } else {
            // .bss 只记录符号，不写数据
            symbolTable.define(gv.name, ".bss", 0)
        }
    }

    // ── 5. 组装目标文件格式 ──
    return buildCoffFile(textSec, rdataSec, dataSec, bssSec, symbolTable)
}
```

#### 9.4.3 COFF 目标文件格式（Windows .obj）

COFF 文件是目标文件的二进制格式。链接器（`link.exe`、`lld-link`）消费此格式。

```
┌─────────────────────────────────────────────────────┐
│ COFF 文件头 (20 字节)                                │
│   machine, numberOfSections, timestamp, ...          │
├─────────────────────────────────────────────────────┤
│ 节表 (40 字节/节 × numberOfSections)                  │
│   [节1] name(8B) virtualSize(4B) vAddress(4B) ...   │
│   [节2] name(8B) ...                                │
│   [节N] name(8B) ...                                │
├─────────────────────────────────────────────────────┤
│ 字符串表                                            │
│   节名字符串（以 NUL 终止，连续排列）                   │
├─────────────────────────────────────────────────────┤
│ 符号表                                               │
│   [符号1] name(8B) value(4B) section(2B) type(2B)  │
│   [符号2] ...                                       │
│   [符号N] ...                                       │
├─────────────────────────────────────────────────────┤
│ 重定位表 (每个节一个)                                  │
│   [重定位1] offset(4B) symbolTableIndex(4B) type(2B) │
│   [重定位2] ...                                     │
├─────────────────────────────────────────────────────┤
│ 节数据                                               │
│   [.text 节数据: 机器码字节]                          │
│   [.rdata 节数据: 字符串常量]                         │
│   [.data 节数据: 全局变量初始值]                      │
└─────────────────────────────────────────────────────┘
```

**COFF 文件头（20 字节）**：

```aura
class CoffFileHeader {
    var machine: Short          = 0x8664    // AMD64
    var numberOfSections: Short = 0
    var timestamp: Int          = 0
    var pointerToSymbolTable: Int = 0
    var numberOfSymbols: Int    = 0
    var sizeOfOptionalHeader: Short = 0     // 目标文件无可选头
    var characteristics: Short  = 0x0102   // EXECUTABLE_IMAGE | LARGE_ADDRESS_AWARE
}
```

**节表头（40 字节/节）**：

```aura
class CoffSectionHeader {
    var name: ByteArray         // 8 字节节名（不足补 NUL）
    var virtualSize: Int        // 节在内存中的大小
    var virtualAddress: Int     // 节在内存中的虚拟地址
    var sizeOfRawData: Int      // 节在文件中的大小
    var pointerToRawData: Int   // 节数据在文件中的偏移
    var pointerToRelocations: Int  // 重定位表偏移
    var numberOfRelocations: Int   // 重定位项数
    var pointerToLinenumbers: Int  // 行号表偏移（通常为 0）
    var numberOfLinenumbers: Int   // 行号数（通常为 0）
    var characteristics: Int   // 节标志

    // 节标志常量
    // SECTION_CODE = 0x20           可执行
    // SECTION_READ = 0x40000000     可读
    // SECTION_WRITE = 0x80000000    可写
    // SECTION_ALIGN_16BYTES = 0x0010  16 字节对齐
}
```

**COFF 符号表项（18 字节/符号）**：

```aura
class CoffSymbol {
    var name: ByteArray          // 8 字节（短名直接存，长名存字符串表偏移）
    var value: Int               // 符号的值（节内偏移）
    var sectionNumber: Short     // 所属节编号（1-based）
    var type: Short              // 符号类型（0x20 = IMAGE_SYM_TYPE_DWORD）
    var storageClass: Byte       // 存储类别（0x20 = EXTERNAL, 0x03 = STATIC）
    var numberOfAuxEntries: Byte // 辅助项数
    // 后续可能有辅助项（函数/节信息等）
}
```

**COFF 重定位项（10 字节/重定位）**：

```aura
class CoffRelocation {
    var virtualAddress: Int      // 重定位的虚拟地址（节内偏移）
    var symbolTableIndex: Int    // 符号表索引
    var type: Short              // 重定位类型
    // Windows x64: IMAGE_REL_AMD64_REL32 = 0x04
}
```

#### 9.4.4 ELF 目标文件格式（Linux .o）

```
┌─────────────────────────────────────────────────────┐
│ ELF 文件头 (64 字节)                                  │
│   magic(0x7F ELF), class(64), data(LE), ...          │
├─────────────────────────────────────────────────────┤
│ 节头表 (64 字节/节)                                    │
│   [节0] .null (空节)                                  │
│   [节1] .text  name(0x24) sh_type(1) flags(6) ...    │
│   [节2] .rdata name(0x25) sh_type(1) flags(2) ...    │
│   [节3] .data  ...                                   │
│   [节4] .symtab ...                                  │
│   [节5] .strtab ...                                  │
│   [节6] .shstrtab ...                                │
├─────────────────────────────────────────────────────┤
│ 节数据（按节头表中的 offset 排列）                       │
├─────────────────────────────────────────────────────┤
│ 符号表数据 (.symtab 节)                               │
├─────────────────────────────────────────────────────┤
│ 字符串表数据 (.strtab 节)                              │
└─────────────────────────────────────────────────────┘
```

**ELF 文件头（64 字节）**：

```aura
class Elf64Header {
    var magic: ByteArray          // 0x7F 'E' 'L' 'F'
    var eiClass: Byte             // 2 = 64 位
    var eiData: Byte              // 1 = 小端
    var eType: Short              // 1 = REL (目标文件)
    var eMachine: Short           // 62 = x86_64
    var eVersion: Int             // 1
    var eEntry: Long              // 入口点（目标文件为 0）
    var ePhoff: Long              // 程序头表偏移（目标文件为 0）
    var eShoff: Long              // 节头表偏移
    var eFlags: Int
    var eEhsize: Short            // ELF 头大小 = 64
    var ePhentsize: Short
    var ePhnum: Short
    var eShentsize: Short         // 节头大小 = 64
    var eShnum: Short             // 节数
    var eShstrndx: Short          // 节名字符串表索引
}
```

**ELF 节头（64 字节/节）**：

```aura
class Elf64SectionHeader {
    var shName: Int               // 节名字符串表中的偏移
    var shType: Int               // 1=PROGBITS, 2=SYMTAB, 3=STRTAB, 0=NULL
    var shFlags: Long             // 2=ALLOC(可加载), 1=WRITE, 4=EXECINSTR
    var shAddr: Long              // 虚拟地址
    var shOffset: Long            // 文件偏移
    var shSize: Long              // 节大小
    var shLink: Int               // 关联节索引（如 .symtab → .strtab）
    var shInfo: Int
    var shAddralign: Long         // 对齐要求
    var shEntsize: Long           // 条目大小（符号表项大小等）
}
```

#### 9.4.5 目标文件构建伪代码

```aura
fun buildCoffFile(sections: List<Section>, symbolTable: SymbolTable): ByteArray {
    val buf = ByteCodeBuffer(64 + 40 * sections.size)  // 预分配

    // ── 1. 文件头 (20 字节) ──
    buf.writeShort(0x8664)                // machine = AMD64
    buf.writeShort(sections.size)         // numberOfSections
    buf.writeInt(0)                       // timestamp
    buf.writeInt(0)                       // pointerToSymbolTable（稍后填充）
    buf.writeInt(symbolTable.count)       // numberOfSymbols
    buf.writeShort(0)                     // sizeOfOptionalHeader = 0（目标文件无可选头）
    buf.writeShort(0x0102)                // characteristics

    // ── 2. 计算各节的文件偏移 ──
    var fileOffset = 20 + 40 * sections.size  // 节表之后
    var strTableOffset = fileOffset            // 字符串表起始

    for (sec in sections) {
        // 记录节的文件偏移
        sec.rawDataOffset = fileOffset
        fileOffset += sec.size
        fileOffset = alignTo(fileOffset, 8)  // 8 字节对齐
    }

    val relocOffset = fileOffset
    val symbolTableOffset = relocOffset + /* 重定位表大小 */

    // ── 3. 节表 (40 字节/节) ──
    var sectionIdx = 1  // COFF 节编号从 1 开始
    for (sec in sections) {
        // 写入 8 字节节名（不足补 NUL）
        var nameBytes = sec.name.toByteArray()
        var i = 0
        while (i < 8) {
            if (i < nameBytes.length) buf.writeByte(nameBytes[i]) else buf.writeByte(0)
            i = i + 1
        }
        buf.writeInt(sec.virtualSize)       // 节在内存中的大小
        buf.writeInt(sec.virtualAddress)     // 虚拟地址（链接器决定）
        buf.writeInt(sec.size)               // 文件中的大小
        buf.writeInt(sec.rawDataOffset)      // 文件偏移
        buf.writeInt(sec.relocations.size)  // 重定位项数
        buf.writeInt(0)                      // 行号表偏移（不使用）
        buf.writeInt(0)                      // 行号数
        buf.writeInt(sec.flags)              // 节标志
        sectionIdx = sectionIdx + 1
    }

    // ── 4. 字符串表 ──
    for (sec in sections) {
        buf.writeByteArray(sec.name.toByteArray())
        buf.writeByte(0)  // NUL 终止
    }

    // ── 5. 重定位表 ──
    for (sec in sections) {
        for (reloc in sec.relocations) {
            buf.writeInt(reloc.offset)              // 虚拟地址（节内偏移）
            buf.writeInt(symbolTable.lookup(reloc.symbol))  // 符号表索引
            buf.writeShort(0x04)                    // R_X86_64_REL32
        }
    }

    // ── 6. 符号表 ──
    for (sym in symbolTable.symbols) {
        // 8 字节名 + value + section + type + storage + auxCount
        // 短名（<8 字符）直接写入，长名写字符串表偏移
        buf.writeByteArray(sym.name.toByteArray().take(8))  // 简化：仅短名
        buf.writeInt(sym.value)
        buf.writeShort(sym.sectionNumber)
        buf.writeShort(0x20)           // type = DWORD
        buf.writeByte(0x20)            // storage = EXTERNAL
        buf.writeByte(0)               // numberOfAuxEntries
    }

    // ── 7. 填充文件头中的符号表偏移 ──
    buf.seek(20 + 8)  // 回到 pointerToSymbolTable 字段
    buf.writeInt(symbolTableOffset)

    // ── 8. 写入节数据 ──
    for (sec in sections) {
        buf.seek(sec.rawDataOffset)
        buf.writeByteArray(sec.data)
    }

    return buf.toByteArray()
}
```

#### 9.4.6 具体示例：`fun main() { println("hello") }` 的目标文件

```
┌──────────────────────────────────────────────────────────────────────┐
│ 文件头 (20 字节)                                                       │
│   machine=0x8664 sections=3 timestamp=0 symTab=0x800 syms=2 optsz=0 │
│   chars=0x0102                                                        │
├──────────────────────────────────────────────────────────────────────┤
│ 节表 1: .text (40 字节)                                                │
│   name=".text" vSize=0x2A vAddr=0 rawDataSize=0x30 rawDataOff=0x130 │
│   relocs=1  chars=0x62000040 (CODE|MEM_16|ALIGN_16|READ|EXEC)         │
├──────────────────────────────────────────────────────────────────────┤
│ 节表 2: .rdata (40 字节)                                               │
│   name=".rdata" vSize=0x6 vAddr=0 rawDataSize=0x8 rawDataOff=0x160   │
│   relocs=0  chars=0x40000040 (MEM_16|ALIGN_16|READ)                   │
├──────────────────────────────────────────────────────────────────────┤
│ 节表 3: .data (40 字节)                                                │
│   name=".data" vSize=0 vAddr=0 rawDataSize=0 rawDataOff=0x168        │
│   relocs=0  chars=0xC0000040 (MEM_16|ALIGN_16|READ|WRITE)             │
├──────────────────────────────────────────────────────────────────────┤
│ 字符串表 (0x130 字节)                                                  │
│   ".text\0.rdata\0.data\0"                                            │
├──────────────────────────────────────────────────────────────────────┤
│ 重定位表 (0x140 字节, 1 项 × 10 字节)                                   │
│   [0] offset=0x16 symbolIdx=0 type=0x04 (REL32, 指向 println)         │
├──────────────────────────────────────────────────────────────────────┤
│ 符号表 (0x150 字节, 2 项 × 18 字节)                                     │
│   [0] "println"  value=0  section=0 (外部引用)                        │
│   [1] "main"     value=0    section=1 (函数, .text 偏移 0)            │
├──────────────────────────────────────────────────────────────────────┤
│ .text 节数据 (0x160 字节, 48 字节)                                      │
│   55                    push rbp                                       │
│   48 89 E5              mov rbp, rsp                                   │
│   48 83 EC 30           sub rsp, 0x30                                  │
│   48 B8 00 00 00 00     mov rax, [rip + 0]     ← 加载 println 地址    │
│   ?? ?? ?? ??           (重定位占位：4 字节)                           │
│   48 8D 0D 00 00 00 00  lea rcx, [rip + 0]     ← 加载 "hello" 地址    │
│   ?? ?? ?? ??           (重定位占位：4 字节)                           │
│   E8 ?? ?? ?? ??        call rax               ← 调用 println         │
│   ?? ?? ?? ??           (重定位占位：4 字节)                           │
│   31 C0                 xor eax, eax         (return 0)               │
│   5D                    pop rbp                                       │
│   C3                    ret                                           │
│   (填充到 48 字节对齐)                                                  │
├──────────────────────────────────────────────────────────────────────┤
│ .rdata 节数据 (0x190 字节, 8 字节)                                      │
│   68 65 6C 6C 6F 00     "hello\0"                                     │
└──────────────────────────────────────────────────────────────────────┘
```

---

### 9.5 Step 6：系统链接（目标文件 → 可执行文件 / 库）

#### 9.5.1 链接流程概览

```
输入: main.obj + aura_runtime.obj + [系统库]
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  系统链接器（Windows: link.exe / lld-link）               │
│  （Linux: ld / lld）                                      │
│                                                           │
│  1. 读取所有目标文件，收集节和符号表                         │
│  2. 解析符号引用：                                          │
│     - main.obj 中 println 的符号引用 → 从 aura_runtime.obj 解析 │
│     - main.obj 中 print 的符号引用 → 从 kernel32.dll 解析（导入表）│
│  3. 布局节：分配虚拟地址，合并同类型节                        │
│     - .text 节合并（所有目标文件的机器码拼接）                  │
│     - .rdata 节合并（字符串常量拼接）                        │
│     - .data 节合并（全局变量拼接）                           │
│  4. 应用重定位：                                            │
│     - 将 REL32 重定位的 4 字节占位替换为实际地址              │
│     - call println 的占位 → println 函数的地址              │
│  5. 生成 PE/ELF 可执行文件                                  │
│     - PE 头 + 节表 + 导入表 + 导出表 + 节数据                │
│     - 或 ELF 头 + 节头表 + 程序头表 + 节数据                 │
└─────────────────────────────────────────────────────────┘
    │
    ▼
输出: main.exe (Windows PE) / main (Linux ELF)
```

#### 9.5.2 Windows PE 可执行文件格式

```
┌──────────────────────────────────────────────────────────────┐
│ DOS 头 (64 字节)                                              │
│   magic=0x5A4D ("MZ")                                         │
│   e_lfanew = PE 头偏移（通常为 0x80 或 0x100）                   │
├──────────────────────────────────────────────────────────────┤
│ PE 签名 (4 字节)                                              │
│   "PE\0\0"                                                    │
├──────────────────────────────────────────────────────────────┤
│ 文件头 (20 字节)                                               │
│   machine=0x8664 (AMD64)                                     │
│   numberOfSections=3 (.text, .rdata, .data)                  │
│   characteristics=0x0102                                      │
├──────────────────────────────────────────────────────────────┤
│ 可选头 (240 字节)                                              │
│   magic=0x020B (PE32+)                                       │
│   imageBase=0x140000000 (默认加载地址)                         │
│   sectionAlignment=0x1000 (4KB)                              │
│   subsystem=0x03 (WINDOWS_CUI, 控制台应用)                     │
│   entryPointAddress = .text 中 main 的 RVA                    │
├──────────────────────────────────────────────────────────────┤
│ 节表 (40 字节/节)                                              │
│   [.text]   name=".text"  vSize=0x2A  vAddr=0x1000           │
│            rawDataSize=0x30  rawDataOff=0x600  chars=0x60000020 │
│   [.rdata]  name=".rdata" vSize=0x6   vAddr=0x1000           │
│            rawDataSize=0x8   rawDataOff=0x630  chars=0x40000040 │
│   [.data]   name=".data"  vSize=0     vAddr=0x2000           │
│            rawDataSize=0     rawDataOff=0x638  chars=0xC0000040 │
├──────────────────────────────────────────────────────────────┤
│ 导入表（Import Directory）                                     │
│   记录需要的外部 DLL 和函数                                     │
│   - kernel32.dll: WriteConsoleA, ExitProcess, ...            │
│   - msvcrt.dll: __set_app_type, ...                         │
│   - aura_runtime.dll (如果是动态链接): aura_println, ...      │
├──────────────────────────────────────────────────────────────┤
│ 节数据（对齐到 0x600 = 1536 字节）                              │
│   .text 节数据（机器码，重定位已应用）                            │
│   .rdata 节数据（字符串常量）                                   │
│   .data 节数据（全局变量）                                      │
└──────────────────────────────────────────────────────────────┘
```

#### 9.5.3 链接器调用（纯 Aura 实现）

```aura
// aura/compiler/aura/lang/compiler/aot/SystemLinker.aura

class SystemLinker {

    /**
     * 调用系统链接器，将目标文件链接为可执行文件。
     *
     * Windows: lld-link 或 link.exe
     * Linux:   ld 或 lld
     */
    fun linkExecutable(
        objectFiles: List<String>,   // 目标文件路径列表
        outputPath: String,           // 输出路径
        targetTriple: String,         // 目标三元组
        subsystem: String             // "console" / "windows"
    ): Boolean {

        val os: String = detectOS()
        val linker: String = findLinker(os, targetTriple)

        if (os == "windows") {
            return linkWindows(linker, objectFiles, outputPath, subsystem)
        } else {
            return linkLinux(os, linker, objectFiles, outputPath)
        }
    }

    /**
     * Windows 链接：lld-link 或 link.exe
     *
     * 命令示例：
     *   lld-link /SUBSYSTEM:CONSOLE main.obj runtime.obj /OUT:main.exe
     *   link.exe /SUBSYSTEM:CONSOLE main.obj runtime.obj /OUT:main.exe
     */
    private fun linkWindows(
        linker: String,
        objects: List<String>,
        outputPath: String,
        subsystem: String
    ): Boolean {
        // 构建命令
        val args = StringBuilder()
        args.append("/SUBSYSTEM:").append(subsystem)
        args.append(" ").append(outputPath)
        for (obj in objects) {
            args.append(" ").append(obj)
        }
        // 追加标准库（如果需要 CRT）
        args.append(" /DEFAULTLIB:msvcrt")
        args.append(" /DEFAULTLIB:kernel32")

        // 调用链接器
        val result = Process.spawn(linker, args.toString())
        return result.exitCode == 0
    }

    /**
     * Linux 链接：ld 或 lld
     *
     * 命令示例：
     *   ld main.o runtime.o -o main --dynamic-linker /lib64/ld-linux-x86-64.so.2
     *   lld main.o runtime.o -o main
     */
    private fun linkLinux(
        os: String,
        linker: String,
        objects: List<String>,
        outputPath: String
    ): Boolean {
        val args = StringBuilder()
        args.append("-o ").append(outputPath)
        for (obj in objects) {
            args.append(" ").append(obj)
        }
        args.append(" --dynamic-linker /lib64/ld-linux-x86-64.so.2")

        val result = Process.spawn(linker, args.toString())
        return result.exitCode == 0
    }

    /**
     * 查找链接器路径（5 级探测）
     */
    private fun findLinker(os: String, triple: String): String {
        // 1. 环境变量 AURA_LINKER
        // 2. 配置 aura.toml [linker] path
        // 3. PATH 搜索
        // 4. 默认路径
        // 5. 报错
    }
}
```

#### 9.5.4 编译产物示例：完整流程

```bash
# 用户输入：
#   fun main(): Int {
#       println("hello")
#       return 0
#   }

# Step 1-3: MIR → LIR → DAG → RegAlloc
#   产出: MachineFunction("main")

# Step 4: 指令编码
#   产出: EncodedFunction("main")
#   .text 节: 55 48 89 E5 48 83 EC 30 ... (机器码字节)
#   重定位: [RelocItem(offset=0x16, symbol="println")]

# Step 5: 目标文件组装
#   产出: main.obj (COFF 格式)
#   节: .text (机器码) + .rdata ("hello\0") + .data (空)
#   符号: main (函数, .text:0), println (外部引用)

# Step 6: 系统链接
#   输入: main.obj + aura_runtime.obj
#   aura_runtime.obj 提供: println 函数实现 + runtime 符号
#   产出: main.exe (PE 格式)

# 运行:
#   main.exe
#   输出: hello
#   退出码: 0
```

#### 9.5.5 共享库（.so / .dll）生成

```aura
// 生成动态库：与可执行文件的区别仅在于链接器参数

fun linkSharedLibrary(
    objectFiles: List<String>,
    outputPath: String,           // "libaura.so" 或 "aura.dll"
    exportSymbols: List<String>   // 需要导出的函数名列表
): Boolean {
    val os: String = detectOS()

    if (os == "windows") {
        // Windows: lld-link /DLL main.obj runtime.obj /OUT:aura.dll /EXPORT:func1 /EXPORT:func2
        val args = StringBuilder()
        args.append("/DLL ")
        args.append(outputPath)
        for (obj in objectFiles) {
            args.append(" ").append(obj)
        }
        for (sym in exportSymbols) {
            args.append(" /EXPORT:").append(sym)
        }
        return Process.spawn("lld-link", args.toString()).exitCode == 0
    } else {
        // Linux: ld -shared main.o runtime.o -o libaura.so -Wl,--export-dynamic
        val args = StringBuilder()
        args.append("-shared ")
        args.append("-o ").append(outputPath)
        for (obj in objectFiles) {
            args.append(" ").append(obj)
        }
        args.append(" -Wl,--export-dynamic")
        return Process.spawn("ld", args.toString()).exitCode == 0
    }
}
```

---

### 9.6 ARC（自动引用计数）

ARC 在新后端中的实现：

```
MIR 指令:    Retain(src)  →  call @aura_arc_increment(src)
             Release(src) →  call @aura_arc_decrement(src)

在 MIR 中显式插入（已有 MIR 降级阶段完成），后端只负责将 Call 映射为指令。
```

---

### 9.7 异常处理（简化方案）

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

## 十、JIT 路径设计：从字节序列到原生执行

### 10.1 完整管线（五步）

```
运行时热点检测 (JitState, 已有)
    │
    ▼ [Step 1: MIR → LIR, 快速模式]
LIR (机器无关 SSA)
    │
    ▼ [Step 2: Instruction Selection]
Machine DAG
    │
    ▼ [Step 3: Register Allocation, 简化模式]
着色 DAG
    │
    ▼ [Step 4: Instruction Encoding]
每个函数 → 字节序列
    │
    ▼ [Step 5: W^X mmap + 分发表]
可执行内存 → 原生执行 + 回退支持
```

**与 AOT 的关键区别**：JIT 没有 Step 5（目标文件组装）和 Step 6（系统链接）。JIT 的机器码直接写入可执行内存，不经过磁盘文件。

---

### 10.2 Step 1-3：快速 Lowering + InstSelect + RegAlloc

JIT 的优化级别低于 AOT，跳过部分耗时操作：

| 操作 | AOT | JIT |
|------|-----|-----|
| **GVN（全局值编号）** | ✅ | ❌ 跳过 |
| **LICM（循环不变代码外提）** | ✅ | ❌ 跳过 |
| **常量传播** | ✅ | ✅ |
| **DCE（死代码消除）** | ✅ | ✅ |
| **强度削弱** | ✅ | ✅ |
| **图着色寄存器分配** | ✅ | ❌ 简化为线性扫描 |
| **完整栈帧布局** | ✅ | ❌ 最小栈帧 |
| **DAG Tiling 指令选择** | ✅ | ✅（完整，指令选择是质量关键） |

```aura
// JIT 专用快速管线
fun jitCompileFunction(
    mirFunc: MirFunction,
    dispatchTable: DispatchTable
): JitResult {
    // Step 1: 快速 Lowering（跳过 LICM/GVN）
    val lir = Lowering(mirFunc, mode = "fast")

    // Step 2: 指令选择（完整 DAG Tiling）
    val dag = InstSelect(lir, arch = "x86_64")

    // Step 3: 简化寄存器分配（线性扫描）
    val coloredDag = RegAlloc(dag, mode = "simple")

    // Step 4: 编码 + Step 5: 写入可执行内存
    return jitEmitAndExecute(coloredDag, dispatchTable)
}
```

---

### 10.3 Step 4-5：从字节序列到可执行内存（核心）

#### 10.3.1 W^X 内存映射：机器码字节的实际落地

JIT 编译的机器码字节需要被加载到**可执行内存**中。这通过操作系统的内存映射接口完成。

```aura
// aura/compiler/aura/lang/compiler/jit/WxMemory.aura

class WxMemory {

    /**
     * 分配可执行内存。
     *
     * Windows: VirtualAlloc(NULL, size, MEM_COMMIT, PAGE_EXECUTE_READWRITE)
     * Linux:   mmap(NULL, size, PROT_READ|PROT_WRITE|PROT_EXEC,
     *              MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)
     *
     * @param size 需要的字节数
     * @return 内存地址（0 表示失败）
     */
    fun allocate(size: Int): Int {
        val os = detectOS()
        if (os == "windows") {
            return virtualAlloc(size)
        } else {
            return mmap(size)
        }
    }

    /**
     * 将机器码字节写入可执行内存。
     *
     * Windows: 直接 memcpy（因为 VirtualAlloc 分配的内存可读写可执行）
     * Linux:   直接 memcpy（mmap 的 PROT_WRITE 允许写入）
     *
     * @param addr allocate() 返回的地址
     * @param code 机器码字节序列
     * @return 是否写入成功
     */
    fun writeCode(addr: Int, code: ByteArray): Boolean {
        // 使用 FFI 调用系统内存写入函数
        // 或直接使用 Aura 的内存写入接口
        return Ffi.memcpy(addr, code, code.length)
    }

    /**
     * 获取函数入口地址。
     * 每个 JIT 编译的函数在可执行内存中的偏移已知。
     *
     * @param baseAddr 内存映射的基地址
     * @param offset 函数在可执行内存中的偏移
     * @return 函数的入口地址
     */
    fun getEntryAddress(baseAddr: Int, offset: Int): Int {
        return baseAddr + offset
    }

    /**
     * 刷新指令缓存（Linux 需要，x86_64 通常不需要）。
     *
     * Linux x86_64: 写入可执行内存后，需要执行 wbinvd 或调用
     *               __builtin___clear_cache 来确保 I-cache 一致。
     *               但 x86_64 的 I-cache 和 D-cache 是硬件同步的，
     *               通常不需要显式刷新。
     * ARM64: 必须刷新（dcivac + isb 指令）。
     */
    fun flushICache(addr: Int, size: Int): Boolean {
        val os = detectOS()
        if (os == "linux" && detectArch() == "aarch64") {
            return Ffi.aarch64FlushICache(addr, size)
        }
        return true  // x86_64 不需要
    }

    /**
     * 释放可执行内存。
     *
     * Windows: VirtualFree(addr, 0, MEM_RELEASE)
     * Linux:   munmap(addr, size)
     */
    fun free(addr: Int, size: Int): Boolean {
        val os = detectOS()
        if (os == "windows") {
            return virtualFree(addr)
        } else {
            return munmap(addr, size)
        }
    }
}
```

#### 10.3.2 JIT 函数代码组织

JIT 编译的多个函数不是单独映射，而是**共享一块可执行内存**：

```
┌─────────────────────────────────────────────────────────┐
│ 可执行内存映射（W^X mmap 分配的连续区域）                   │
│                                                           │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ Preamble 代码（16 字节对齐）                            │ │
│ │  - 保存调用保存寄存器（RBX, RSI, RDI, R12-R15）         │ │
│ │  - 设置 deopt 入口（push 回退信息到栈）                  │ │
│ │  - 设置 dispatch_table 指针                             │ │
│ ├─────────────────────────────────────────────────────┤ │
│ │ 函数 0 代码（16 字节对齐）                               │ │
│ │  - prologue: push rbp / mov rbp,rsp / sub rsp,...     │ │
│ │  - body: 机器码指令                                     │ │
│ │  - epilogue: leave / ret                              │ │
│ │  - 重定位占位（跨函数调用）                               │ │
│ ├─────────────────────────────────────────────────────┤ │
│ │ 函数 1 代码（16 字节对齐）                               │ │
│ │  - ...                                                │ │
│ ├─────────────────────────────────────────────────────┤ │
│ │ 函数 N 代码（16 字节对齐）                               │ │
│ │  - ...                                                │ │
│ ├─────────────────────────────────────────────────────┤ │
│ │ Deopt 蹦床代码                                         │ │
│ │  - 保存所有寄存器到内存                                   │ │
│ │  - 调用 VM 解释器的 deopt 入口                           │ │
│ │  - 从内存恢复寄存器                                     │ │
│ └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

```aura
class JitCodeLayout {
    var baseAddr: Int         // mmap 返回的基地址
    var totalSize: Int        // 总大小
    var funcEntries: List<Int>  // 每个函数的入口偏移

    // 各段偏移
    var preambleOffset: Int   = 0
    var preambleSize: Int     = 64   // Preamble 大小
    var codeOffset: Int       = 64   // 函数代码起始偏移
    var deoptOffset: Int      = 0    // Deopt 蹦床偏移（末尾）
    var deoptSize: Int        = 256  // Deopt 蹦床大小

    /**
     * 计算需要的总内存大小。
     */
    fun calculateSize(functions: List<EncodedFunction>): Int {
        var size = preambleSize
        for (f in functions) {
            size += alignTo(f.size, 16)
        }
        size += deoptSize
        return size
    }

    /**
     * 为每个函数分配偏移量。
     */
    fun assignOffsets(functions: List<EncodedFunction>): List<Int> {
        val offsets = arrayListOf<Int>()
        var offset = codeOffset
        for (f in functions) {
            offsets.add(offset)
            offset += alignTo(f.size, 16)
        }
        return offsets
    }
}
```

#### 10.3.3 完整 JIT 编译流程（含内存映射）

```aura
fun jitCompileAndExecute(
    functions: List<MachineFunction>,     // 需要编译的函数
    dispatchTable: DispatchTable          // 分发表
): JitResult {

    // ── 1. 编码每个函数 → 字节序列 ──
    val encoder = X86Encoder()
    val encoded = ArrayList<EncodedFunction>()
    for (func in functions) {
        encoder.reset()
        val relocs = ArrayList<RelocItem>()

        for (mi in func.instructions) {
            if (mi.relocIndex >= 0) {
                // 含重定位的指令：写入操作码 + 4 字节占位
                encoder.emitWithReloc(mi)
                relocs.add(RelocItem(
                    offset = /* 在函数内的偏移 */,
                    symbol = func.relocations[mi.relocIndex].symbol,
                    type = "R_X86_64_REL32"
                ))
            } else {
                encoder.emit(mi)
            }
        }

        encoded.add(EncodedFunction(
            name = func.name,
            offset = 0,   // 稍后分配
            size = encoder.size,
            code = encoder.drain(),
            relocations = relocs,
            align = 16
        ))
    }

    // ── 2. 计算布局 + 分配偏移 ──
    val layout = JitCodeLayout()
    layout.totalSize = layout.calculateSize(encoded)
    layout.funcEntries = layout.assignOffsets(encoded)

    // ── 3. 分配可执行内存 ──
    val wxMem = WxMemory()
    val baseAddr = wxMem.allocate(layout.totalSize)
    if (baseAddr == 0) {
        return JitResult(failed = true, reason = "mmap 失败")
    }

    // ── 4. 写入 Preamble ──
    val preambleEncoder = X86Encoder()
    // Preamble: 保存调用保存寄存器 + 设置 deopt 上下文
    preambleEncoder.pushR(RBX)
    preambleEncoder.pushR(R15)
    preambleEncoder.pushR(R14)
    preambleEncoder.pushR(R13)
    preambleEncoder.pushR(R12)
    preambleEncoder.pushR(RDI)
    preambleEncoder.pushR(RSI)
    // 设置 deopt 信息到栈顶（函数索引 + IP + 状态）
    preambleEncoder.pushR(RAX)  // 保存函数入口地址
    // ... 更多 preamble 代码
    wxMem.writeCode(baseAddr, preambleEncoder.toByteArray())

    // ── 5. 写入每个函数的机器码 ──
    for (i in 0..encoded.size) {
        val ef = encoded[i]
        val funcAddr = baseAddr + layout.funcEntries[i]

        // 写入机器码字节
        wxMem.writeCode(funcAddr, ef.code)

        // 写入重定位（JIT 中直接计算地址，不需要链接器）
        for (reloc in ef.relocations) {
            // 查找目标函数地址
            val targetOffset = layout.funcEntries.indexOf(reloc.symbol)
            if (targetOffset >= 0) {
                val targetAddr = baseAddr + layout.funcEntries[targetOffset]
                val callAddr = baseAddr + layout.funcEntries[i] + reloc.offset
                // 计算相对偏移（JMP/CALL rel32）
                val relOffset = targetAddr - (callAddr + 4)  // +4: 跳过 call 指令本身
                // 写入 4 字节相对偏移
                writeInt32(callAddr + reloc.offset, relOffset)
            } else {
                // 目标函数不在 JIT 编译集中 → 使用 dispatch_table 间接调用
                // 写入 call_indirect 代码（通过分发表查找地址）
                patchIndirectCall(callAddr + reloc.offset, reloc.symbol, dispatchTable)
            }
        }
    }

    // ── 6. 写入 Deopt 蹦床 ──
    val deoptEncoder = X86Encoder()
    // Deopt 蹦床：保存所有寄存器 → 调用 VM deopt → 恢复
    deoptEncoder.pushR(RBX)
    deoptEncoder.pushR(RSI)
    deoptEncoder.pushR(RDI)
    deoptEncoder.pushR(R12)
    deoptEncoder.pushR(R13)
    deoptEncoder.pushR(R14)
    deoptEncoder.pushR(R15)
    // 将栈上的 deopt 信息（函数索引、IP、状态）传给 VM
    deoptEncoder.emitCall(/* VM deopt 入口地址 */)
    // 恢复寄存器...
    wxMem.writeCode(baseAddr + layout.deoptOffset, deoptEncoder.toByteArray())

    // ── 7. 刷新指令缓存（aarch64 需要） ──
    wxMem.flushICache(baseAddr, layout.totalSize)

    // ── 8. 注册到分发表 ──
    for (i in 0..encoded.size) {
        val entryAddr = baseAddr + layout.funcEntries[i]
        dispatchTable.register(encoded[i].name, entryAddr)
    }

    return JitResult(
        baseAddr = baseAddr,
        totalSize = layout.totalSize,
        funcEntries = layout.funcEntries,
        failed = false
    )
}
```

#### 10.3.4 分发表（Dispatch Table）：递归与互递归

JIT 编译的函数需要支持递归调用。由于 JIT 函数没有链接器生成的地址，跨函数调用通过**分发表**间接查找：

```aura
// 分发表：函数索引 → 入口地址映射
class DispatchTable {
    private var entries: HashMap<String, Int> = hashMapOf<String>()

    /**
     * 注册函数入口地址。
     * JIT 编译完成后，将每个函数的入口地址注册到分发表。
     */
    fun register(name: String, entryAddr: Int): Boolean {
        entries.put(name, entryAddr)
    }

    /**
     * 查找函数入口地址。
     * 被 JIT 代码中的 call_indirect 指令使用。
     */
    fun lookup(name: String): Int {
        return entries.getOrDefault(name, 0)  // 0 表示未找到
    }

    /**
     * 检查函数是否已 JIT 编译。
     */
    fun isCompiled(name: String): Boolean {
        return entries.containsKey(name)
    }
}
```

**间接调用的机器码**：

当 JIT 代码需要调用一个可能不在当前编译集中的函数时：

```nasm
; 假设需要调用函数 "fib"
; 1. 加载分发表地址（从固定寄存器 R14 或栈中获取）
; 2. 通过分发表查找 "fib" 的入口地址
; 3. 跳转到该地址

; 生成的代码：
;   mov rax, [rip + disp32]     ; rax = dispatch_table 指针（重定位）
;   mov rax, [rax + fib_entry_offset]  ; rax = fib 的入口地址
;   call rax                     ; 间接调用
```

```aura
// 生成间接调用代码（写入到 call 指令的位置）
fun patchIndirectCall(
    callAddr: Int,           // call 指令的内存地址
    targetName: String,       // 目标函数名
    dispatchTable: DispatchTable  // 分发表
): Boolean {
    val encoder = X86Encoder()

    // 1. 加载分发表地址（假设分发表地址存在固定位置）
    //    通过 RIP-relative 寻址加载 dispatch_table 指针
    encoder.emitMovRR(RAX, disp32 = /* dispatch_table 地址 */)

    // 2. 通过分发表查找目标函数入口
    //    简化方案：分发表是连续数组，每个函数一个 8 字节槽位
    //    目标函数的索引预先确定
    val funcIndex = dispatchTable.getIndex(targetName)
    encoder.emitMovRR(RAX, RAX)  // rax = [rax + funcIndex * 8]
    // 3. 间接调用
    encoder.emitCallRR(RAX)

    // 写入到内存
    val wxMem = WxMemory()
    return wxMem.writeCode(callAddr, encoder.toByteArray())
}
```

**直接调用 vs 间接调用的选择**：

```aura
// JIT 编译时，决定每个 call 是直接用还是间接
fun decideCallType(
    calleeName: String,
    compiledFunctions: List<String>,
    dispatchTable: DispatchTable
): String {
    if (compiledFunctions.contains(calleeName)) {
        // 被调用者也在 JIT 编译集中 → 直接 call rel32
        return "direct"
    } else if (dispatchTable.isCompiled(calleeName)) {
        // 被调用者已 JIT 编译但在其他编译集中 → 间接 call 通过分发表
        return "indirect"
    } else {
        // 被调用者未 JIT 编译 → deopt 回退解释器
        return "deopt"
    }
}
```

#### 10.3.5 回退（Deoptimization）：JIT → VM 的切换

当 JIT 代码遇到无法处理的情况时，需要**回退到 VM 解释器**继续执行。

```
JIT 代码执行中遇到不可处理的情况
    │
    ▼
JIT 代码调用 deopt 蹦床
    │
    ▼
Deopt 蹦床：保存所有寄存器状态 → 写入内存
    │
    ▼
调用 VM 解释器的 deopt 入口
    │
    ▼
VM 解释器：读取寄存器状态 → 恢复栈帧 → 从当前 IP 继续执行
    │
    ▼
VM 继续执行（语义与 JIT 执行一致）
```

**Deopt 触发条件**：

| 条件 | 说明 |
|------|------|
| **不支持的指令** | 如 `try/catch`、`lambda`、`new object` 等 JIT 白名单外的指令 |
| **调用未编译函数** | 被调用者不在 JIT 编译集中 |
| **栈溢出** | 递归深度超过阈值 |
| **类型不匹配** | 运行期类型与编译期假设不一致 |

**Deopt 蹦床代码**（x86_64）：

```nasm
; Deopt 蹦床：保存到栈上，然后调用 VM 的 deopt 入口
; 输入：当前 RIP（自动在栈上）、RBX/RSI/RDI/R12-R15（调用保存寄存器）

; 1. 保存调用保存寄存器
push rbx
push rsi
push rdi
push r12
push r13
push r14
push r15

; 2. 将 deopt 信息写入内存（VM 可读）
;    deopt_info = { funcIndex, ip, stackState, localsState }
mov rax, [rip + disp32]      ; rax = deopt_info 指针
mov [rax + 0], rcx           ; funcIndex = RCX（约定：RCX 中存函数索引）
lea rax, [rsp]
mov [rax + 8], rax           ; stackState = RSP（当前栈顶）

; 3. 调用 VM 的 deopt 入口
lea rax, [rip + disp32]      ; rax = VM deopt 入口地址
call rax

; 4. VM 返回后，恢复寄存器
pop r15
pop r14
pop r13
pop r12
pop rdi
pop rsi
pop rbx

; 5. 返回到 JIT 代码继续执行
jmp rax
```

```aura
// 从 VM 解释器调用 JIT 编译后的函数
class JitBridge {
    var dispatchTable: DispatchTable
    var wxMemory: WxMemory

    /**
     * 调用 JIT 编译的函数。
     *
     * @param funcName 函数名
     * @param args 参数列表（作为栈传递）
     * @return 返回值
     */
    fun callJitFunction(funcName: String, args: List<Any>): Any {
        val entryAddr = dispatchTable.lookup(funcName)
        if (entryAddr == 0) {
            // 未 JIT 编译 → 回退 VM 解释器
            return vmInterpret(funcName, args)
        }

        // 设置栈帧（符合 x86_64 调用约定）
        setupStackFrame(args)

        // 通过 FFI 调用原生地址
        val result = Ffi.callNative(entryAddr)

        // 清理栈帧
        cleanupStackFrame()

        return result
    }
}
```

---

### 10.4 JIT 与 AOT 的差异处理

| 维度 | AOT | JIT |
|------|-----|-----|
| **优化级别** | 完整（GVN/LICM/内联/循环展开） | 快速（常量传播/DCE/强度削弱） |
| **寄存器分配** | 完整图着色 | 简化（线性扫描 + 少量溢出） |
| **栈帧** | 完整（局部变量 + 溢出 + 对齐） | 最小（仅必要寄存器保存） |
| **调用** | 直接 call（链接器解析地址） | dispatch_table + call_indirect（支持递归） |
| **回退** | 无（编译时失败即报错） | deopt → 回退 VM 解释器 |
| **内存映射** | 磁盘文件（.obj → .exe） | W^X mmap（mmap/VirtualAlloc） |
| **描述符表** | 无 | AuraFuncDesc（32 字节）+ 段格式 |
| **重定位** | 链接器解析（系统链接器） | JIT 内部解析（分发表 + 直接计算） |

---

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

// 新增：JIT 编译批次管理
class JitBatch {
    var compiled: List<String> = arrayListOf<String>()  // 已编译函数名
    var pending: List<String> = arrayListOf<String>()   // 待编译函数名

    /**
     * 收集需要编译的函数集。
     * 入口函数 + 其调用图中可编译的函数。
     */
    fun collectBatch(entryFunc: String, callGraph: Map<String, List<String>>): List<String> {
        val result = arrayListOf<String>()
        result.add(entryFunc)
        // 递归收集可编译的被调用者
        collectCallees(entryFunc, callGraph, result)
        return result
    }

    /**
     * 执行批量 JIT 编译。
     * 所有函数一次性编译到同一块可执行内存中。
     */
    fun executeBatch(functions: List<String>): Boolean {
        // 1. 收集 MIR 函数
        val mirFuncs = collectMirFunctions(functions)

        // 2. 编译并映射（见 10.3.3）
        val result = jitCompileAndExecute(mirFuncs, dispatchTable)

        // 3. 更新状态
        if (!result.failed) {
            compiled.addAll(functions)
        }
        return !result.failed
    }
}
```

---

### 10.6 完整 JIT 执行流程示例

```
// 用户程序：
//   fun main(): Int {
//       return fib(10)
//   }
//
//   fun fib(n: Int): Int {
//       if (n <= 1) { return n }
//       return fib(n-1) + fib(n-2)
//   }

// ── 1. VM 解释执行 fib，调用计数达到阈值 ──
//   fib 被调用 10000 次 → JitState 触发编译

// ── 2. JIT 收集编译批次 ──
//   入口函数: fib
//   调用图: fib → fib（递归）
//   编译批次: [fib]

// ── 3. MIR → LIR → DAG → RegAlloc → Encode ──
//   fib 的机器码:
//   ┌────────────────────────────────────────────────────┐
//   │ 55                    push rbp                      │
//   │ 48 89 E5              mov rbp, rsp                  │
//   │ 48 83 EC 20           sub rsp, 0x20                 │
//   │ 83 FA 01              cmp edx, 1                    │
//   │ 7E 0A                 jle +0x0A (base case)         │
//   │ E8 00 00 00 00        call fib (直接调用，递归)      │
//   │ ?? ?? ?? ??           (重定位: 自身地址)              │
//   │ E8 00 00 00 00        call fib (递归调用 n-2)       │
//   │ ?? ?? ?? ??           (重定位: 自身地址)              │
//   │ 48 01 C0              add rax, rax                  │
//   │ 5D                    pop rbp                       │
//   │ C3                    ret                           │
//   └────────────────────────────────────────────────────┘

// ── 4. W^X mmap + 写入机器码 ──
//   baseAddr = VirtualAlloc(256 bytes)
//   写入 fib 的机器码到 baseAddr + 0x00
//   重定位：将 call 的 4 字节占位替换为 fib 自身的相对地址

// ── 5. 注册到分发表 ──
//   dispatchTable.register("fib", baseAddr + 0x00)

// ── 6. VM 继续执行 main ──
//   main 调用 fib → JitBridge 查分发表 → 找到 fib 入口 → 直接调用原生代码
//   fib 内部递归调用 → 通过直接 call 自身（地址已解析）→ 原生递归执行
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
