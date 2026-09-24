# HAT 格式设计文档 v2.0 (SSA 结构化 IR)

> **版本**: 2.0
> **状态**: 设计提案
> **日期**: 2026-06-25
> **基线**: SSA MIR + LLVM IR + Rust MIR（Phi 节点风格）
>
> **核心变更**: 从 v1.0 的「HIR Arena 线性转储」升级为 **SSA 结构化 IR 文本格式**。
> 采用 **Phi 节点**（与 LLVM/MIR/SsaBuilder 一致），两条管线（PHIR/HAT）在 SSA MIR 处汇合，互不干扰。

---

## 1. 概述

### 1.1 问题背景

当前 PHIR 存在三个结构性缺陷：

| 缺陷 | 根因 | 代价 |
|------|------|------|
| **伪源码语法** | 用 `fun`/`if`/`while` 等源码关键字 | 800+ 行递归下降解析器，~50ms/函数 |
| **类型丢失** | `ty` 字段未序列化 | 解析器重写类型推断（`phirSigLookup`，历史 OOM 20 GB） |
| **Span 全丢** | 6 个 span 分量硬编码 `Span(0,0,1,1,1,1)` | 调试信息为零 |

HAT v2.0 解决这三个问题：直接序列化 SSA 结构，显式标注类型，保留 span 注释。

### 1.2 解决方案

**HAT v2.0**——SSA 结构化 IR 文本格式，使用 **Phi 节点**：

```
; module main target x86_64

@fn while_loop() -> Int
  bb entry:
    @i@0 = @i32_const 0 : Int
    @br @check

  bb check:
    @i@1 = @phi(@i@0, @i@2) : Int       ← Phi 节点
    @t0 = @i32_const 10 : Int
    @t1 = @icmp @slt(@i@1, @t0) : Bool
    @br_if @t1 => @body, @exit

  bb body:
    @t2 = @i32_const 1 : Int
    @i@2 = @add(@i@1, @t2) : Int
    @br @check

  bb exit:
    @ret @i@1 : Int
```

核心特性：
- **SSA 形式**：每个值只赋值一次（`@t0`, `@t1`, `@i@0`, `@i@1`, ...）
- **显式 CFG**：基本块 + 终止符，无隐式控制流
- **Phi 节点**：`@t = @phi(@t0, @t1) : Int`——与 LLVM/MIR/SsaBuilder 一致
- **声明式指令**：`@t0 = @add(@a, @b) : Int`
- **显式类型**：每个值都标注类型，**无需推断**

### 1.3 双管线架构

HAT v2.0 与 PHIR **并行共存**，两条管线在 SSA MIR 处汇合为**同一种表示**（Phi 节点 SSA），之后管线完全共享。

```
PHIR 路径（现有，不动）：
  PHIR text ──→ PHIR Parser(800行) ──→ HIR Arena ──→ SsaBuilder ──→ SSA MIR(Phi) ─┐
                                                                                     │
                                                                                        ▼
                                                                                 Lowering ──→ LIR ──→ DAG ──→ X86 ──→ COFF

HAT 路径（新增）：
  HAT text ──→ HatParser(120行) ──────────────────────────────────→ SSA MIR(Phi) ──┘
```

**关键设计决策**：HAT 也产出 Phi 节点（非 BB 参数），与 SsaBuilder 产出一致。

| 维度 | 选择 | 原因 |
|------|------|------|
| Phi 节点 vs BB 参数 | **Phi 节点** | SsaBuilder 已支持，零改造 |
| 两条管线共存 | **并行** | 互不干扰，PHIR 路径不受影响 |
| SSA MIR 表示 | **统一 Phi** | 两条管线产出同一种 SSA MIR |
| 下游管线 | **共享** | Lowering 及以下完全不改 |

### 1.4 格式定位

| 格式 | 层级 | 角色 | 消费者 | 解析复杂度 |
|------|------|------|--------|-----------|
| **HAT v2.0** | SSA MIR | **主 IR 文本格式** | Photon 管线 | ~120 行 |
| **PHIR** | HIR 伪源码 | 备选/调试 | 人工、IDE 高亮 | ~800 行 |
| **LLVM IR** | SSA MIR | 目标码生成 | LLVM 工具链 | — |

**数据流对比**：

```
PHIR 管线:   PHIR text → HIR Arena → SSA MIR → LIR → DAG → X86 → COFF → Link
                  (parse 800行)  (SsaBuilder)

HAT 管线:    HAT text ──────────────────→ SSA MIR → LIR → DAG → X86 → COFF → Link
                  (parse 120行)
```

---

## 2. 命名

### 2.1 缩写含义

**HAT** = **H**IR-**A**dvanced **T**ext IR

| 字母 | 含义 | 说明 |
|------|------|------|
| **H** | HIR-derived | 源于 HIR，SSA 形式 |
| **A** | Advanced | 高级 IR（SSA + CFG + Phi） |
| **T** | Text | 文本格式 |

### 2.2 标识符约定

| 标识符 | 值 |
|--------|-----|
| **格式名** | HAT |
| **版本** | `HAT/2.0` |
| **文件扩展名** | `.hat` |
| **类名（解析器）** | `HatParser` |
| **类名（序列化器）** | `HatSerializer` |
| **工具对象** | `HatUtils` |
| **包路径** | `aura.lang.compiler.hir.hat` |
| **环境变量（输入）** | `AURA_PHOTON_HAT` |
| **环境变量（格式选择）** | `AURA_PHOTON_FORMAT` (`hat` \| `phir`，默认 `hat`) |

### 2.3 与 PHIR 的关系

```
HAT v2.0 ──primary──→ Photon 管线（SSA MIR → LIR → X86 → COFF → Link）
 ├──serialize──→ PHIR（人工调试、IDE 高亮）
 └──serialize──→ LLVM IR（目标码生成）

PHIR ──secondary──→ Photon 管线（HIR Arena → SSA MIR → LIR → ...）
```

两条管线**并行运行**，由 CLI 参数 `-f hat|phir` 或环境变量 `AURA_PHOTON_FORMAT` 选择。
两者在 SSA MIR 处汇合为同一种表示（Phi 节点 SSA），之后完全共享管线。

---

## 3. 格式规范

### 3.1 整体结构

一个 `.hat` 文件由**四部分**组成：

```
; ① 元数据头（一行）
; ② 外部函数声明（零或多个）
; ③ 函数定义（零或多个）
; ④ 类型声明（零或多个，可选）
```

> 上图中各行首的 `;` 是**文档注释**，不是文件内容。

**`;` 的使用规则**（唯一权威定义）：`;` 只在两处出现——

1. **元数据头三行**（§3.2）：`; module ...` / `; schema=...` / `; source=...`（LLVM IR 注释风格）；
2. **行内 span 标注**（§3.8）：`@t0 = @i32_const 42 : Int ;@span L2:26-35`。

除这两处外，**其余所有行都不带 `;` 前缀**：`@extern` / `@fn` / `@struct` / `@enum` 声明、`bb <label>:` 块头、以及块内指令都直接以内容开头（`bb` 用 2 空格缩进，指令用 4 空格缩进），基本块之间用**空行**分隔。

解析器对行首 `;` 是宽容的（只剥掉一个前导 `;` 加后续空白，见 §7.3），因此历史遗留的 `;@fn` / `;  bb` 写法仍可解析，但**序列化器不再产出这种形式**。

### 3.2 元数据头

第一行，格式固定：

```
; module <name> target <triple>
```

| 字段 | 说明 | 示例 |
|------|------|------|
| `<name>` | 模块名 | `main` |
| `<triple>` | 目标三元组 | `x86_64-pc-windows-msvc` |

可选元数据行（紧随其后）：

```
; schema=HAT/2.0
; source=<path>
```

### 3.3 外部函数声明

以 `@extern` 开头：

```
@extern <name>(<params>) -> <ret>
```

| 字段 | 说明 |
|------|------|
| `<name>` | 函数名（可含 `.` 命名空间） |
| `<params>` | 参数列表，逗号分隔，每个 `@name: Type` |
| `<ret>` | 返回类型 |

**示例**：
```
@extern println(@msg: Any) -> Unit
@extern toStr(@v: Any) -> String
@extern aura.lang.std.Math.abs(@x: Float) -> Float
```

### 3.4 函数定义

以 `@fn` 开头：

```
@fn <name>(<params>) -> <ret>
  bb <label>:
    ...指令...
  bb <label>:
    ...指令...
```

**函数签名**：
| 字段 | 说明 |
|------|------|
| `<name>` | 函数名 |
| `<params>` | 参数列表，`@name: Type`，逗号分隔 |
| `<ret>` | 返回类型 |

**基本块**：
| 字段 | 说明 |
|------|------|
| `<label>` | BB 标签名（`entry`, `check`, `body`, `exit`, `then`, `else`, ...） |

### 3.5 指令

指令以 `@` 开头，前置缩进 4 空格（位于 `bb` 块内）：

```
    @<target> = @<op>(<args>) : <type>
```

或无返回值指令：

```
    @<op>(<args>)
```

或带属性的操作：

```
    @<target> = @<op> { <attrs> } (<args>) : <type>
```

**指令分类**：

#### 常量指令

| 指令 | 语法 | 说明 |
|------|------|------|
| `@i32_const` | `@t = @i32_const <val> : Int` | 32 位整数常量 |
| `@f64_const` | `@t = @f64_const <val> : Float` | 64 位浮点常量 |
| `@const_str` | `@t = @const_str "<str>" : String` | 字符串常量 |
| `@bool_const` | `@t = @bool_const <true\|false> : Bool` | 布尔常量 |
| `@null` | `@t = @null : Any` | 空值 |

**转义规则**：字符串常量中 `|` → `\|`，`\` → `\\`，换行 → `\n`。

#### Phi 节点指令

| 指令 | 语法 | 说明 |
|------|------|------|
| `@phi` | `@t = @phi(@a, @b, ...) : Type` | SSA Phi 节点，合并多条控制流路径 |

Phi 节点是 SSA 形式的核心——在循环入口或分支汇合点，将来自不同前驱 BB 的值合并为一个 SSA 值。

#### 算术指令

| 指令 | 类型 | 语法 |
|------|------|------|
| `@add` | Int/Float | `@t = @add(@a, @b) : Type` |
| `@sub` | Int/Float | `@t = @sub(@a, @b) : Type` |
| `@mul` | Int/Float | `@t = @mul(@a, @b) : Type` |
| `@div` | Int/Float | `@t = @div(@a, @b) : Type` |
| `@rem` | Int | `@t = @rem(@a, @b) : Int` |
| `@neg` | Int/Float | `@t = @neg(@a) : Type` |

#### 比较指令

| 指令 | 语法 | 说明 |
|------|------|------|
| `@icmp @i32_eq` | `@t = @icmp @i32_eq(@a, @b) : Bool` | 整数相等 |
| `@icmp @i32_ne` | `@t = @icmp @i32_ne(@a, @b) : Bool` | 整数不等 |
| `@icmp @i32_slt` | `@t = @icmp @i32_slt(@a, @b) : Bool` | 有符号 < |
| `@icmp @i32_sgt` | `@t = @icmp @i32_sgt(@a, @b) : Bool` | 有符号 > |
| `@icmp @i32_sle` | `@t = @icmp @i32_sle(@a, @b) : Bool` | 有符号 <= |
| `@icmp @i32_sge` | `@t = @icmp @i32_sge(@a, @b) : Bool` | 有符号 >= |
| `@icmp @i32_ult` | `@t = @icmp @i32_ult(@a, @b) : Bool` | 无符号 < |
| `@icmp @i32_ugt` | `@t = @icmp @i32_ugt(@a, @b) : Bool` | 无符号 > |
| `@icmp @f64_eq` | `@t = @icmp @f64_eq(@a, @b) : Bool` | 浮点相等 |
| `@icmp @f64_lt` | `@t = @icmp @f64_lt(@a, @b) : Bool` | 浮点 < |
| `@icmp @str_eq` | `@t = @icmp @str_eq(@a, @b) : Bool` | 字符串相等 |

#### 逻辑指令

| 指令 | 语法 |
|------|------|
| `@and` | `@t = @and(@a, @b) : Bool` |
| `@or` | `@t = @or(@a, @b) : Bool` |
| `@xor` | `@t = @xor(@a, @b) : Bool` |
| `@not` | `@t = @not(@a) : Bool` |

#### 位运算指令

| 指令 | 语法 |
|------|------|
| `@band` | `@t = @band(@a, @b) : Int` |
| `@bor` | `@t = @bor(@a, @b) : Int` |
| `@bxor` | `@t = @bxor(@a, @b) : Int` |
| `@shl` | `@t = @shl(@a, @b) : Int` |
| `@shr` | `@t = @shr(@a, @b) : Int` |
| `@ushr` | `@t = @ushr(@a, @b) : Int` |

#### 类型转换指令

| 指令 | 语法 |
|------|------|
| `@i32_to_f64` | `@t = @i32_to_f64(@a) : Float` |
| `@f64_to_i32` | `@t = @f64_to_i32(@a) : Int` |
| `@i32_to_str` | `@t = @i32_to_str(@a) : String` |
| `@f64_to_str` | `@t = @f64_to_str(@a) : String` |
| `@str_to_i32` | `@t = @str_to_i32(@a) : Int` |

#### 内存指令

| 指令 | 语法 | 说明 |
|------|------|------|
| `@alloc` | `@t = @alloc { size: N, align: M } : !stackslot` | 分配栈槽 |
| `@store` | `@store @val => @slot : Type` | 存储到栈槽 |
| `@load` | `@t = @load @slot : Type` | 从栈槽加载 |

#### 字符串指令

| 指令 | 语法 |
|------|------|
| `@str_concat` | `@t = @str_concat(@a, @b) : String` |
| `@str_len` | `@t = @str_len(@a) : Int` |
| `@str_sub` | `@t = @str_sub(@s, @start, @len) : String` |
| `@str_char` | `@t = @str_char(@s, @i) : Char` |

#### 集合指令

| 指令 | 语法 |
|------|------|
| `@alloc_list` | `@t = @alloc_list { count: N } : !list` |
| `@list_set` | `@t = @list_set(@list, @i, @v) : !list` |
| `@list_get` | `@t = @list_get(@list, @i) : Type` |
| `@list_len` | `@t = @list_len(@list) : Int` |
| `@alloc_map` | `@t = @alloc_map { count: N } : !map` |
| `@map_set` | `@t = @map_set(@map, @k, @v) : !map` |
| `@map_get` | `@t = @map_get(@map, @k) : Type` |

#### 对象指令

| 指令 | 语法 |
|------|------|
| `@new` | `@t = @new @Type { fields: [@f0, @f1, ...] } : !ptr` |
| `@field` | `@t = @field @obj, .name : Type` |
| `@field_set` | `@field_set @obj, .name, @val : Unit` |

#### 调用指令

```
    @t = @call @<fn_name>(@args) : <ret_type>
```

| 字段 | 说明 |
|------|------|
| `@<fn_name>` | 被调用函数名 |
| `@args` | 参数列表，SSA 值引用 |
| `<ret_type>` | 返回类型 |

### 3.6 终止符

每个 BB 必须以终止符结尾：

| 指令 | 语法 | 说明 |
|------|------|------|
| `@br` | `@br @<label>` | 无条件跳转 |
| `@br_if` | `@br_if @cond => @<then>, @<else>` | 条件跳转 |
| `@ret` | `@ret @<val> : <type>` | 带返回值返回 |
| `@ret` 无值 | `@ret () : Unit` | 无返回值返回 |

### 3.7 SSA 值引用

| 形式 | 说明 | 示例 |
|------|------|------|
| `@tN` | 临时变量（函数内自动编号） | `@t0`, `@t1`, `@t2`, ... |
| `@name` | 源变量名（首次赋值时绑定） | `@i`, `@sum`, `@x` |
| `@name@N` | 可变变量的后续版本（SSA rename） | `@i@1`, `@i@2`, `@i@3` |
| `@"literal"` | 字面量值 | `@"Hello"` |

**SSA rename 规则**：
- 首次赋值：`@i`（无后缀）
- 第 2 次赋值：`@i@1`
- 第 3 次赋值：`@i@2`
- Phi 节点引用所有版本：`@i@3 = @phi(@i@1, @i@2) : Int`

### 3.8 Span 注释

可选的行内注释，用于调试：

```
    @t0 = @i32_const 42 : Int ;@span L2:26-35
```

格式：`;@span L<line>:<col>-<endCol>`

解析器**可选**处理：忽略 span 注释不影响 IR 语义。

### 3.9 类型声明（可选）

用于结构体、枚举等类型定义：

```
@struct @Point { @x: Int, @y: Int }
@struct @Dog { @name: String, @age: Int }
@enum @Color { RED, GREEN, BLUE }
```

---

## 4. HIR → HAT 映射

### 4.1 节点映射表

| HIR 节点 | HAT 指令 | 说明 |
|----------|----------|------|
| `HirProgram` | 元数据头 | 仅元信息 |
| `HirFunction` | `@fn` | 函数签名 |
| `HirParam` | 函数签名参数 | `@a: Int, @b: Int` |
| `HirBlock` | BB 序列 | 每个 Block → 一个 BB |
| `HirValDecl` | `@let` | `@x = @const 42 : Int` |
| `HirVarDecl` | `@let`（可变） | 后续 `@store` 修改 |
| `HirLit` | `@i32_const` / `@f64_const` / `@const_str` / `@bool_const` | 按字面量类型分派 |
| `HirVar` | SSA 值引用 | `@x`, `@t0`, `@i@1` |
| `HirCall` | `@call` | `@t = @call @fn(@args)` |
| `HirBinary` | 二元算术/比较指令 | `@add`, `@sub`, `@icmp` 等 |
| `HirUnary` | 一元指令 | `@neg`, `@not` |
| `HirIf` | `@br_if` | 条件跳转 + then/else BB |
| `HirWhile` | 循环 BB（entry → check → body → exit） | Phi 节点合并循环携带值 |
| `HirFor` | 降级为 while | 同上 |
| `HirReturn` | `@ret` | 终止符 |
| `HirAssign` | `@store` | `@store @val => @slot : Type` |
| `HirBreak` | `@br` 到循环出口 | 终止符 |
| `HirContinue` | `@br` 到循环头 | 终止符 |
| `HirIndex` | `@list_get` / `@field` | 按下标类型分派 |
| `HirMember` | `@field` | `@t = @field @obj, .name : Type` |
| `HirNew` | `@new` | `@t = @new @Type { fields: [...] }` |
| `HirLambda` | `@fn_ref` 或闭包分配 | 待扩展 |
| `HirTry` | `@br_if` + 异常 BB | 待扩展 |
| `HirStruct` | `@struct` | 类型声明 |
| `HirEnum` | `@enum` | 类型声明 |

### 4.2 SSA 构造规则

HIR 是树结构，HAT 是 SSA + CFG。转换规则：

1. **声明 → SSA 赋值**：
   - `HirValDecl(x: Int = 42)` → `@x = @i32_const 42 : Int`
   - `HirVarDecl(i: Int = 0)` → `@i = @i32_const 0 : Int`

2. **可变赋值 → SSA rename**：
   - `HirAssign(x = expr)` → `@x@1 = @eval(expr) : Int`（SSA rename）
   - 或使用 `@store`：`@store @val => @x : Int`

3. **二元运算 → 内联操作符**：
   - `HirBinary(+, a, b)` → `@t = @add(@a, @b) : Int`
   - `HirBinary(==, a, b)` → `@t = @icmp @i32_eq(@a, @b) : Bool`

4. **条件分支 → @br_if**：
   - `HirIf(cond, then, else)` → `@t = @eval(cond) : Bool` + `@br_if @t => @then, @else`

5. **循环 → Phi 节点循环**：
   - 每个循环产生 3-4 个 BB：`entry`, `check`, `body`, `exit`
   - 循环携带值通过 Phi 节点合并

### 4.3 Phi 节点计算

Phi 节点是标准 SSA 的核心——在循环入口或分支汇合点，将来自不同前驱 BB 的值合并为一个 SSA 值。

**循环中的 Phi 节点计算步骤**：

1. **识别循环**：有回边的 BB 是循环体
2. **计算到达定义**：每个在循环体内被赋值的变量，在循环入口需要 Phi
3. **生成 Phi 节点**：`@i@1 = @phi(@i@0, @i@2) : Int`
   - `@i@0`：来自 entry BB 的初始值
   - `@i@2`：来自 body BB 的更新值
4. **Phi 节点位于循环入口 BB 的第一条指令**

**示例**：

```
// HIR: while (i < 10) { i = i + 1 }
//          ↓ 转换后
bb entry:
  @i@0 = @i32_const 0 : Int    // i = 0
  @br @check

bb check:
  @i@1 = @phi(@i@0, @i@2) : Int  // Phi: entry(@i@0) 或 body(@i@2)
  @t0 = @i32_const 10 : Int
  @t1 = @icmp @slt(@i@1, @t0) : Bool
  @br_if @t1 => @body, @exit

bb body:
  @t2 = @i32_const 1 : Int
  @i@2 = @add(@i@1, @t2) : Int   // i = i + 1
  @br @check

bb exit:
  @ret @i@1 : Int
```

---

## 5. 完整示例

### 5.1 Hello World

**源码**：
```aura
fun main() {
    println("Hello, World!")
}
```

**HAT**：
```
; module main target x86_64
; schema=HAT/2.0

@extern println(@msg: Any) -> Unit

@fn main() -> Unit
  @t0 = @const_str "Hello, World!" : String
  @call @println(@t0) : Unit
  @ret () : Unit
```

> **注意**：简单函数（无分支/循环）可以省略 `bb entry:`，解析器会自动创建隐式 entry 块。
> 仅有控制流合并点（循环入口、分支汇合）才需要显式 BB + Phi 节点。

### 5.2 函数调用

**源码**：
```aura
fun add(a: Int, b: Int): Int {
    return a + b
}
fun main() {
    val x = add(3, 4)
    println(x)
}
```

**HAT**：
```
; module main target x86_64
; schema=HAT/2.0

@extern println(@msg: Any) -> Unit

@fn add(@a: Int, @b: Int) -> Int
  bb entry:
    @t0 = @add(@a, @b) : Int
    @ret @t0 : Int

@fn main() -> Unit
  bb entry:
    @t0 = @i32_const 3 : Int
    @t1 = @i32_const 4 : Int
    @t2 = @call @add(@t0, @t1) : Int
    @t3 = @i32_to_str(@t2) : String
    @t4 = @call @println(@t3) : Unit
    @ret () : Unit
```

### 5.3 条件分支

**源码**：
```aura
fun max(a: Int, b: Int): Int {
    if (a > b) {
        return a
    } else {
        return b
    }
}
```

**HAT**：
```
; module main target x86_64
; schema=HAT/2.0

@fn max(@a: Int, @b: Int) -> Int
  bb entry:
    @t0 = @icmp @i32_sgt(@a, @b) : Bool
    @br_if @t0 => @then, @else

  bb then:
    @ret @a : Int

  bb else:
    @ret @b : Int
```

### 5.4 While 循环（Phi 节点版本）

**源码**：
```aura
fun sum_to(n: Int): Int {
    var sum = 0
    var i = 1
    while (i <= n) {
        sum = sum + i
        i = i + 1
    }
    return sum
}
```

**HAT**：
```
; module main target x86_64
; schema=HAT/2.0

@fn sum_to(@n: Int) -> Int
  bb entry:
    @sum = @i32_const 0 : Int
    @i@0 = @i32_const 1 : Int
    @br @check

  bb check:
    @sum@1 = @phi(@sum, @sum@2) : Int       ← Phi: entry 或 body
    @i@1 = @phi(@i@0, @i@2) : Int           ← Phi: entry 或 body
    @t0 = @icmp @i32_sle(@i@1, @n) : Bool
    @br_if @t0 => @body, @exit

  bb body:
    @sum@2 = @add(@sum@1, @i@1) : Int        // sum = sum + i
    @t1 = @i32_const 1 : Int
    @i@2 = @add(@i@1, @t1) : Int             // i = i + 1
    @br @check

  bb exit:
    @ret @sum@1 : Int
```

**注意**：Phi 节点是 BB 的第一条指令，将来自不同前驱 BB 的值合并为一个 SSA 值。

### 5.5 For 循环

**源码**：
```aura
fun factorial(n: Int): Int {
    var result = 1
    for (var i = 1; i <= n; i = i + 1) {
        result = result * i
    }
    return result
}
```

**HAT**：
```
; module main target x86_64
; schema=HAT/2.0

@fn factorial(@n: Int) -> Int
  bb entry:
    @result = @i32_const 1 : Int
    @i@0 = @i32_const 1 : Int
    @br @check

  bb check:
    @result@1 = @phi(@result, @result@2) : Int  ← Phi: entry 或 body
    @i@1 = @phi(@i@0, @i@2) : Int               ← Phi: entry 或 body
    @t0 = @icmp @i32_sle(@i@1, @n) : Bool
    @br_if @t0 => @body, @exit

  bb body:
    @result@2 = @mul(@result@1, @i@1) : Int       // result = result * i
    @t1 = @i32_const 1 : Int
    @i@2 = @add(@i@1, @t1) : Int                  // i = i + 1
    @br @check

  bb exit:
    @ret @result@1 : Int
```

### 5.6 复杂示例（字符串拼接 + 数组）

**源码**：
```aura
fun greet(name: String): String {
    val msg = "Hello, " + name + "!"
    return msg
}

fun main() {
    val arr = [1, 2, 3, 4, 5]
    val s = greet("World")
    println(s)
    println(arr[2])
}
```

**HAT**：
```
; module main target x86_64
; schema=HAT/2.0

@extern println(@msg: Any) -> Unit

@fn greet(@name: String) -> String
  bb entry:
    @t0 = @const_str "Hello, " : String
    @t1 = @str_concat(@t0, @name) : String
    @t2 = @const_str "!" : String
    @msg = @str_concat(@t1, @t2) : String
    @ret @msg : String

@fn main() -> Unit
  bb entry:
    @t0 = @i32_const 1 : Int
    @t1 = @i32_const 2 : Int
    @t2 = @i32_const 3 : Int
    @t3 = @i32_const 4 : Int
    @t4 = @i32_const 5 : Int
    @arr = @alloc_list { count: 5 } : !list
    @arr@1 = @list_set(@arr, 0, @t0) : !list
    @arr@2 = @list_set(@arr@1, 1, @t1) : !list
    @arr@3 = @list_set(@arr@2, 2, @t2) : !list
    @arr@4 = @list_set(@arr@3, 3, @t3) : !list
    @arr@5 = @list_set(@arr@4, 4, @t4) : !list
    @t5 = @const_str "World" : String
    @s = @call @greet(@t5) : String
    @t6 = @call @println(@s) : Unit
    @t7 = @list_get(@arr@5, 2) : Int
    @t8 = @i32_to_str(@t7) : String
    @t9 = @call @println(@t8) : Unit
    @ret () : Unit
```

### 5.7 同一源码的 PHIR 对比

上述 5.4（while 循环）的 PHIR 版本（供对比）：

```
; module main target x86_64
; source sum_to.aura

fun sum_to(n: Int) -> Int {
    var sum: Int = 0
    var i: Int = 1
    while (i <= n) {
        sum = sum + i
        i = i + 1
    }
    return sum
}
```

**对比**：

| 维度 | PHIR | HAT |
|------|------|-----|
| 格式 | 伪源码语法 | SSA 指令 |
| 控制流 | 隐式（`while` 关键字） | 显式（BB + `@br_if`） |
| 变量 | 可变（`var`） | SSA（`@sum@1`, `@i@1`） |
| 类型 | 显式标注 | 显式标注 |
| Span | 丢失 | 可选注释 |
| 解析器 | ~800 行 | ~120 行 |

---

## 6. 性能分析

### 6.1 解析复杂度对比

| 维度 | 当前 PHIR | HAT v1.0 (Arena) | **HAT v2.0 (SSA)** |
|------|----------|-----------------|-------------------|
| 解析代码量 | ~800 行 | ~30 行 | **~120 行** |
| 时间复杂度 | O(N × D) | O(N) | **O(N)** |
| 空间复杂度 | O(N) + 签名表 + 环境表 | O(N) | **O(N)** |
| 临时分配 | 大量（substring/trim/split） | 极少 | **极少** |
| HIR → SSA 转换 | 需要 | 需要 | **不需要** |

### 6.2 关键开销消除

| PHIR 开销 | 说明 | HAT v2.0 |
|-----------|------|----------|
| `phirSigLookup` | 签名表 O(N) 扫描 | **不需要**——类型显式标注 |
| `phirEnvGet` | 环境表 O(N) 扫描 | **不需要**——类型显式标注 |
| `phirSplitBinary` | 递归下降表达式解析 | **不需要**——值引用直接 |
| 缩进跟踪建树 | 栈 + 深度计算 | **不需要**——BB 结构显式 |
| HIR → SSA 转换 | SsaBuilder 整个阶段 | **不需要**——HAT 直接是 SSA |

### 6.3 预期加速比

| 指标 | PHIR | HAT v2.0 | 加速比 |
|------|------|----------|--------|
| 解析 2345 函数 | ~2 分钟 | **~1.5 秒** | **~80×** |
| 解析后管线启动 | 需 HIR → SSA | **直接 SSA → LIR** | **~2×** |
| 峰值内存 | 155 MB | **~25 MB** | **~6×** |
| 解析器代码 | ~800 行 | **~120 行** | **~7×** |

### 6.4 序列化性能

| 维度 | PHIR | HAT v2.0 |
|------|------|----------|
| 遍历方式 | 递归树遍历 | **线性 SSA 遍历** |
| 字符串操作 | 缩进 + 关键字拼接 | **指令模板拼接** |
| 复杂度 | O(N × D) | **O(N)** |
| 输出大小 | ~1.37 MB / 2345 函数 | **~1.2 MB** |

HAT v2.0 的输出文件比 PHIR 更小（SSA 形式更紧凑），但**解析速度提升 80×**，且**跳过 HIR → SSA 转换**。

### 6.5 综合优势

```
PHIR 管线:   PHIR(parse 800行) → HIR Arena → SSA MIR → LIR → ...
                                  ↑ SsaBuilder

HAT 管线:    HAT(parse 120行) ──────────────────→ SSA MIR → LIR → ...
                                  ↑ 跳过 HIR→SSA

共同点：SSA MIR(Phi) 之后完全共享管线
```

### 6.6 Phi 节点 vs BB 参数性能对比

| 维度 | Phi 节点 | BB 参数 | 差异 |
|------|---------|--------|------|
| 解析复杂度 | O(N) | O(N) | **相同** |
| 解析代码量 | ~120 行 | ~120 行 | **相同** |
| 循环额外代码 | 多一行 `@phi` | 参数在 BB 头 | Phi 多 ~1 行/循环 |
| 内存开销 | 1 条指令/Phi | 1 个参数/BB | Phi 略大（~100 字节） |
| 管线改造 | ✅ 零改造 | ❌ 需改造 | Phi 更安全 |
| 性能差异 | 基准 | 基准 | **< 1%** |

**结论**：Phi 节点 vs BB 参数的性能差异可忽略（< 1%）。HAT 路径的核心性能收益来自解析器和管线简化，与 Phi/BB 参数的选择无关。

---

## 7. 解析器设计

### 7.1 接口定义

```
class HatParser {
    // 从 .hat 文本解析为 SSA MIR
    fun parse(text: String): MirSsaProgram

    // 从文件路径读取并解析
    fun parseFile(path: String): MirSsaProgram
}

object HatUtils {
    fun emptyParser(): HatParser
    fun parseHat(text: String): MirSsaProgram
    fun loadHat(filePath: String): MirSsaProgram
}
```

### 7.2 解析流程

```
输入: .hat 文本
  │
  ├─ 1. 按 \n 分行
  │
  ├─ 2. 扫描元数据头
  │     → ; module <name> target <triple>
  │     → ; schema=HAT/2.0
  │     → ; source=<path>
  │
  ├─ 3. 遍历剩余行
  │     → @extern → 解析外部函数声明
  │     → @fn → 解析函数定义
  │     → @struct / @enum → 解析类型声明
  │
  ├─ 4. 函数定义解析
  │     → 解析签名: @fn name(@params) -> ret
  │     → 遍历 BB:
  │       → bb label:
  │       → 解析 Phi: @t = @phi(@a, @b) : Type
  │       → 解析指令: @t = @op(@args) : Type
  │       → 解析终止符: @br / @br_if / @ret
  │
  └─ 5. 构建 MirSsaProgram
      → 填充函数、BB、指令、值、符号表
      │
  输出: MirSsaProgram（与 SsaBuilder 产出一致）
```

### 7.3 伪代码（Aura 风格，~120 行）

```
fun parseHat(text: String): MirSsaProgram {
    val prog: MirSsaProgram = MirSsaProgram()
    val lines: List<String> = text.split("\n")

    var i: Int = 0
    while (i < lines.size) {
        val line: String = lines[i].trim()

        if (line.startsWith("; module")) {
            prog.parseHeader(line)
            i = i + 1
        } else if (line.startsWith("@extern")) {
            prog.addExtern(line)
            i = i + 1
        } else if (line.startsWith("@fn")) {
            i = this.parseFunction(lines, i, prog)
        } else if (line.startsWith("@struct")) {
            prog.addStruct(line)
            i = i + 1
        } else if (line.startsWith("@enum")) {
            prog.addEnum(line)
            i = i + 1
        } else if (line == "") {
            i = i + 1
        } else {
            i = i + 1
        }
    }

    return prog
}

fun parseFunction(lines: List<String>, i: Int, prog: MirSsaProgram): Int {
    val sigLine: String = lines[i].trim()
    val func: MirFunction = prog.parseFuncSig(sigLine)
    i = i + 1

    while (i < lines.size) {
        val line: String = lines[i].trim()
        if (line.startsWith("bb ")) {
            val bb: MirBlock = func.parseBbHeader(line)
            func.addBlock(bb)
            i = i + 1
            i = this.parseBbBody(lines, i, func, bb)
        } else if (line == "") {
            i = i + 1
        } else {
            break
        }
    }

    prog.addFunction(func)
    return i
}

fun parseBbBody(lines: List<String>, i: Int, func: MirFunction, bb: MirBlock): Int {
    while (i < lines.size) {
        val line: String = lines[i].trim()
        if (line.startsWith("bb ") || line == "") {
            break
        }

        if (line.startsWith("@br_if")) {
            func.addBrIf(bb, line)
        } else if (line.startsWith("@br ")) {
            func.addBr(bb, line)
        } else if (line.startsWith("@ret")) {
            func.addRet(bb, line)
        } else if (line.startsWith("@phi")) {
            func.addPhi(bb, line)
        } else if (line.startsWith("@")) {
            func.addInstruction(bb, line)
        }

        i = i + 1
    }
    return i
}
```

### 7.4 错误处理

| 错误 | 行为 |
|------|------|
| 缺少终止符 | 警告 + 自动追加 `@ret () : Unit` |
| 未知操作符 | 警告 + 跳过该指令 |
| 无效 BB 标签 | 警告 + 使用 `bbN` 默认标签 |
| Phi 前驱不匹配 | 警告 + 回退为普通指令 |
| 类型不匹配 | 警告 + 按 `Any` 处理 |
| 空文件 | 返回空 `MirSsaProgram` |

---

## 8. 序列化工具设计

### 8.1 接口定义

```
class HatSerializer {
    var moduleName: String = ""
    var targetTriple: String = "x86_64-pc-windows-msvc"

    // 从 HIR 序列化为 HAT 文本
    fun serializeFromHir(hir: Hir): String

    // 从 SSA MIR 序列化为 HAT 文本
    fun serializeFromSsa(mir: MirSsaProgram): String

    // 写入文件
    fun writeFile(mir: MirSsaProgram, path: String): Unit
}

object HatUtils {
    fun emptySerializer(): HatSerializer
    fun serializeHat(mir: MirSsaProgram, module: String): String
    fun saveHat(mir: MirSsaProgram, path: String, module: String): Unit
}
```

### 8.2 序列化流程

```
输入: HIR Arena 或 MirSsaProgram
  │
  ├─ 如果是 HIR：HIR → SSA MIR（SsaBuilder）
  │
  ├─ 1. 写元数据头
  │     → ; module <name> target <triple>
  │     → ; schema=HAT/2.0
  │
  ├─ 2. 遍历外部函数
  │     → @extern name(@params) -> ret
  │
  ├─ 3. 遍历函数定义
  │     → @fn name(@params) -> ret
  │     → 遍历 BB：
  │       → bb label:
  │       → 遍历指令：
  │         → @t = @phi(@a, @b) : Type      ← Phi 节点
  │         → @t = @op(@args) : Type
  │       → 终止符：
  │         → @br / @br_if / @ret
  │
  输出: .hat 文本
```

### 8.3 伪代码

```
fun serializeFromHir(hir: Hir): String {
    // HIR → SSA MIR
    val ssaBuilder: SsaBuilder = SsaBuilderUtils.build(hir)
    return this.serializeFromSsa(ssaBuilder)
}

fun serializeFromSsa(mir: MirSsaProgram): String {
    var out: String = ""

    // 元数据头
    out = out + "; module " + this.moduleName + " target " + this.targetTriple + "\n"
    out = out + "; schema=HAT/2.0\n"
    out = out + "\n"

    // 外部函数
    var i: Int = 0
    while (i < mir.externCount) {
        val ext: MirExtern = mir.externAt(i)
        out = out + "@extern " + ext.toSignature() + "\n"
        i = i + 1
    }
    if (i > 0) { out = out + "\n" }

    // 函数定义
    i = 0
    while (i < mir.functionCount) {
        val func: MirFunction = mir.functionAt(i)
        out = out + "@fn " + func.toSignature() + "\n"
        out = out + this.serializeFunctionBody(func)
        out = out + "\n"
        i = i + 1
    }

    return out
}

fun serializeFunctionBody(func: MirFunction): String {
    var out: String = ""
    var bi: Int = 0
    while (bi < func.blockCount) {
        val bb: MirBlock = func.blockAt(bi)
        if (bi > 0) { out = out + "\n" }            // 基本块之间用空行分隔
        out = out + "  bb " + bb.label + ":\n"
        out = out + this.serializeBlockInstructions(bb)
        bi = bi + 1
    }
    return out
}

fun serializeBlockInstructions(bb: MirBlock): String {
    var out: String = ""
    var ii: Int = 0
    while (ii < bb.instrCount) {
        val ins: MirInstruction = bb.instrAt(ii)
        out = out + "    " + ins.toHatText() + "\n"
        ii = ii + 1
    }
    return out
}
```

---

## 9. 工具集成

### 9.1 CLI 参数

```
aura build -b photon -f hat <src>.aura -o build/hello.exe      # HAT（默认）
aura build -b photon -f phir <src>.aura -o build/hello.exe     # PHIR（备选）
aura build -b photon -f hat build/hello.hat                    # 从 .hat 编译
aura build -b photon -f phir build/hello.phir                  # 从 .phir 编译
aura build -b photon <src>.aura -o build/hello.hat             # 仅生成 HAT
aura build -b photon -v 2.0 <src>.aura                         # 指定 HAT 版本
```

### 9.2 环境变量

```
AURA_PHOTON_FORMAT=hat|phir          # 格式选择（默认 hat）
AURA_PHOTON_HAT=<path>              # .hat 文件路径
AURA_PHOTON_PHIR=<path>             # .phir 文件路径（备选）
AURA_PHOTON_HAT_VERSION=2.0         # HAT 版本
AURA_PHOTON_OUT=<dir>               # 输出目录
AURA_PHOTON_MODULE=<name>           # 模块名
AURA_PHOTON_DEBUG_HAT=1             # 解析后输出调试信息
```

### 9.3 PhotonDriver 集成

```
fun main() {
    val format: String = Env.get("AURA_PHOTON_FORMAT", "hat")
    val outDir: String = Env.get("AURA_PHOTON_OUT", "build/photon_test")
    val moduleName: String = Env.get("AURA_PHOTON_MODULE", "main")

    val pipeline: PhotonPipeline = PhotonPipelineUtils.emptyPipeline()
    pipeline.outDir = outDir
    pipeline.moduleName = moduleName

    val result: BackendResult
    if (format == "hat") {
        val hatPath: String = Env.get("AURA_PHOTON_HAT", "")
        if (hatPath == "") {
            println("Error: AURA_PHOTON_HAT not set")
            Process.exit(1)
        }
        result = pipeline.compileHat(hatPath)
    } else {
        val phirPath: String = Env.get("AURA_PHOTON_PHIR", "")
        if (phirPath == "") {
            println("Error: AURA_PHOTON_PHIR not set")
            Process.exit(1)
        }
        result = pipeline.compilePhir(phirPath)
    }

    if (result.success) {
        println("Success: " + result.executablePath)
        Process.exit(0)
    } else {
        println("Failed: " + result.errorMessage)
        Process.exit(1)
    }
}
```

### 9.4 Rust CLI 集成

```rust
// main.rs 中新增 HAT 序列化

fn hir_to_hat(hir: &HirProgram, module_name: &str, source_path: &str) -> String {
    // 1. HIR → SSA MIR（使用 Phi 节点，与 SsaBuilder 一致）
    let ssa = build_ssa_from_hir(hir);

    // 2. SSA MIR → HAT 文本
    let mut out = String::new();
    out.push_str(&format!("; module {} target x86_64-pc-windows-msvc\n", module_name));
    out.push_str("; schema=HAT/2.0\n");
    if !source_path.is_empty() {
        out.push_str(&format!("; source={}\n", source_path));
    }
    out.push('\n');

    // 外部函数
    for ext in &ssa.externs {
        out.push_str(&format!("@extern {}\n", ext.to_signature()));
    }
    if !ssa.externs.is_empty() {
        out.push('\n');
    }

    // 函数定义
    for func in &ssa.functions {
        out.push_str(&format!("@fn {}\n", func.to_signature()));
        for bb in &func.blocks {
            out.push_str(&format!("  bb {}\n", bb.to_header()));
            for instr in &bb.instructions {
                out.push_str(&format!("    {}\n", instr.to_hat_text()));
            }
        }
        out.push('\n');
    }

    out
}
```

### 9.5 PhotonPipeline 接口

```
// 新增方法：HAT 路径
fun compileHat(hatPath: String): BackendResult {
    val hatText: String = FileSystem.readText(hatPath)
    val mir: MirSsaProgram = HatUtils.parseHat(hatText)

    // 直接从 SSA MIR 开始管线（与 PHIR 路径汇合）
    val lowering = Lowering()
    val lir = lowering.lower(mir)
    val selector = InstructionSelectorUtils.emptySelector()
    val dag = selector.select(lir)
    // ... 后续管线不变
    return this.compileFromDag(dag)
}

// 现有方法：PHIR 路径（不改）
fun compilePhir(phirPath: String): BackendResult {
    val phirText: String = FileSystem.readText(phirPath)
    val hir: Hir = this.parsePhirText(phirText)

    // HIR → SSA MIR → LIR → ...
    val mir: MirSsaProgram = SsaBuilderUtils.build(hir)
    val lowering = Lowering()
    val lir = lowering.lower(mir)
    // ... 后续管线与 compileHat 共享
    return this.compileFromDag(dag)
}
```

**两条管线在 SSA MIR 处汇合**：

```
compilePhir:  PHIR text → HIR Arena → SsaBuilder → SSA MIR → Lowering → ...
compileHat:   HAT text ───────────────────────────→ SSA MIR → Lowering → ...
                                              ↑ 汇合点
```

---

## 10. 迁移计划

### 阶段 1：HAT v2.0 格式规范（本文档）

| 任务 | 产出 |
|------|------|
| 格式定义 | 本文档 |
| HIR → HAT 映射表 | 第 4 节 |
| 指令集定义 | 第 3.5 节 |

### 阶段 2：HAT 解析器实现（1-2 天）

| 任务 | 文件 | 行数 |
|------|------|------|
| `HatParser.aura` | `aura/lang/compiler/hir/hat/HatParser.aura` | ~120 |
| `HatUtils.aura` | `aura/lang/compiler/hir/hat/HatUtils.aura` | ~30 |
| 解析器单测 | `tests/hat_parser_tests.aura` | ~100 |

### 阶段 3：HAT 序列化器实现（2-3 天）

| 任务 | 文件 |
|------|------|
| `HatSerializer.aura`（Aura 侧） | `aura/lang/compiler/hir/hat/HatSerializer.aura` |
| `hir_to_hat()`（Rust 侧） | `rust/cli/src/main.rs` |
| Rust SSA 构造器 | `rust/compiler/src/codegen/ssa.rs` |
| 序列化器单测 | Rust + Aura 双向测试 |

### 阶段 4：管线集成（1 天）

| 任务 | 文件 | 改动 |
|------|------|------|
| `PhotonPipeline.compileHat()` | `PhotonPipeline.aura` | +~10 行 |
| `PhotonDriver` 格式分发 | `PhotonDriver.aura` | +~15 行 |
| CLI `-f hat\|phir` | `Main.aura` | +~10 行 |

**注意**：阶段 4 无需修改 SsaBuilder、SSA MIR、Lowering 或以下任何组件。

### 阶段 5：验证（2 天）

| 验证项 | 方法 |
|--------|------|
| P1 差分测试 | `scripts/photon-suite.ps1 -Phase P1` |
| P2/P3 差分测试 | `scripts/photon-suite.ps1 -Phase P2,P3` |
| HAT vs PHIR 一致性 | 同源编译，SHA256 对比 |
| 性能基准 | HAT vs PHIR 解析时间 |
| 自举验证 | `aura build -b photon Main.aura` |

### 阶段 6：文档与 IDE（2-3 天，可选）

| 任务 | 文件 |
|------|------|
| HAT 语法高亮 | `tools/ide-extension/phir-vscode-extension/syntaxes/hat.tmLanguage.json` |
| HAT 代码片段 | `snippets/hat.json` |
| 格式说明 | `docs/photon/hat-format.md`（从本文档简化） |

### 总工作量

| 阶段 | 代码量 | 周期 |
|------|--------|------|
| 阶段 1（规范） | 本文档 | 已完成 |
| 阶段 2（解析器） | ~250 行 | 1-2 天 |
| 阶段 3（序列化器） | ~300 行 | 2-3 天 |
| 阶段 4（管线集成） | ~35 行 | 1 天 |
| 阶段 5（验证） | 测试用例 | 2 天 |
| 阶段 6（文档/IDE） | ~200 行 | 2-3 天 |
| **总计** | **~785 行** | **8-11 天** |

**零改造**：SsaBuilder、SSA MIR、Lowering、InstructionSelector、RegAlloc、X86Emitter——全部不动。

---

## 11. PHIR 关系

### 11.1 PHIR 定位

PHIR 与 HAT **并行共存**，由用户选择：

```
aura build -b photon -f phir <src>.aura    # PHIR 路径（现有行为）
aura build -b photon -f hat <src>.aura     # HAT 路径（新）
```

PHIR 的用途：
1. **人工调试**：`AURA_PHOTON_FORMAT=phir` 生成可读的伪源码
2. **IDE 语法高亮**：`phir.tmLanguage.json` 继续使用
3. **教学/文档**：PHIR 格式更像源码，适合教学
4. **向后兼容**：现有 PHIR 文件无需修改

### 11.2 HAT → PHIR 转换

```
// 将 HAT 文本转换为 PHIR 文本（需要中间经 HIR）
fun hatToPhir(hatText: String): String {
    val mir: MirSsaProgram = HatUtils.parseHat(hatText)
    val hir: Hir = MirToHir(mir)          // SSA MIR → HIR
    val ser: PhirSerializer = PhirSerializerUtils.emptySerializer()
    return ser.serializeFromHir(hir)
}
```

### 11.3 PHIR → HAT 转换

```
// 将 PHIR 文本转换为 HAT 文本
fun phirToHat(phirText: String): String {
    val hir: Hir = PhotonPipelineUtils.parsePhirText(phirText)
    val mir: MirSsaProgram = SsaBuilderUtils.build(hir)
    val ser: HatSerializer = HatUtils.emptySerializer()
    return ser.serializeFromSsa(mir)
}
```

---

## 12. 双管线共存设计

### 12.1 架构总览

```
                    ┌─────────────────────────────────────────┐
                    │           PHOTON PIPELINE                │
                    │                                         │
  PHIR text ──────→ │  PHIR Parser (800行)                     │
                    │        │                                │
                    │        ▼                                │
                    │  HIR Arena                              │
                    │        │                                │
                    │        ▼                                │
                    │  SsaBuilder ──→ SSA MIR (Phi) ──────────┤
                    │                            │            │
                    │                            │            │
  HAT text ────────→ │  HatParser (120行) ──→ SSA MIR (Phi) ──┤
                    │                            │            │
                    │                            ▼            │
                    │                     Lowering ──→ LIR    │
                    │                            │            │
                    │                            ▼            │
                    │                     InstructionSelector  │
                    │                            │            │
                    │                            ▼            │
                    │                     MachineDag           │
                    │                            │            │
                    │                            ▼            │
                    │                     RegAlloc + Peephole  │
                    │                            │            │
                    │                            ▼            │
                    │                     X86Emitter           │
                    │                            │            │
                    │                            ▼            │
                    │                     COFF → Link          │
                    │                                         │
                    └─────────────────────────────────────────┘
```

### 12.2 互不干扰保证

| 组件 | PHIR 路径 | HAT 路径 | 干扰 |
|------|----------|---------|------|
| PHIR Parser | ✅ 使用 | ❌ 不使用 | 无 |
| HIR Arena | ✅ 使用 | ❌ 不使用 | 无 |
| SsaBuilder | ✅ 使用 | ❌ 不使用 | 无 |
| HatParser | ❌ 不使用 | ✅ 使用 | 无 |
| **SSA MIR (Phi)** | **✅ 产出** | **✅ 产出** | **汇合点** |
| Lowering | ✅ 使用 | ✅ 使用 | 共享 |
| InstructionSelector | ✅ 使用 | ✅ 使用 | 共享 |
| RegAlloc | ✅ 使用 | ✅ 使用 | 共享 |
| X86Emitter | ✅ 使用 | ✅ 使用 | 共享 |

**关键**：两条管线在 SSA MIR 处汇合为**同一种表示**（Phi 节点 SSA），之后完全共享。

### 12.3 选择机制

```
CLI 参数 -f hat|phir          → 主选择机制
环境变量 AURA_PHOTON_FORMAT   → 环境变量选择
默认值: hat                   → 新安装默认使用 HAT
```

### 12.4 数据流选择逻辑

```
fun selectPipeline(format: String): String {
    if (format == "hat") {
        return "compileHat"      // HAT 路径
    } else {
        return "compilePhir"     // PHIR 路径
    }
}
```

---

## 13. 未来考虑

### 13.1 压缩支持

当前 HAT 是纯文本。未来可考虑：
- **zlib/zstd 压缩**：`.hat` → `.hat.zst`（压缩率 ~5-10×）
- **二进制变体**：`.hat.bin`（定长记录，无文本开销）

### 13.2 MLIR 方言扩展

HAT v2.0 的指令命名（`@op`）天然支持方言扩展：

```
; 当前（默认方言）
    @t = @add(@a, @b) : Int

; 未来（方言前缀）
    @t = @arith.add(@a, @b) : Int
    @t = @mem.alloc { size: 8 } : !stackslot
    @t = @mem.load @slot : Int
```

### 13.3 调试信息增强

HAT v2.0 的 `;@span` 注释可扩展为完整的调试元数据：

```
    @t0 = @i32_const 42 : Int ;@span L2:26-35 ;@debug x, "val"
```

### 13.4 增量编译

HAT 的 SSA 形式天然支持增量编译：
- 每个函数的 HAT 片段可独立解析
- 修改一个函数不影响其他函数
- 可缓存已编译的 HAT 片段

### 13.5 多目标后端

HAT 是目标无关的。同一 HAT 文件可编译到：
- x86_64（当前）
- ARM64（未来）
- RISC-V（未来）

### 13.6 可选升级：BB 参数（未来）

如果未来需要 Cranelift 风格的 BB 参数，可分阶段迁移：

```
阶段 1（当前）：Phi 节点
  → 两条管线都用 Phi 节点
  → 零改造

阶段 2（可选）：BB 参数
  → 改造 SsaBuilder → 产出 BB 参数
  → 升级 HAT 格式 → bb check(@i: Int):
  → 升级 SSA MIR → 支持 BB 参数
  → 两条管线同步升级
  → 性能收益：~1%（微不足道）
```

**当前不推荐迁移到 BB 参数**：性能收益 < 1%，改造代价 2-3 周，收益/代价比极低。

---

## 附录 A：与 v1.0 对比

| 维度 | HAT v1.0 (Arena) | HAT v2.0 (SSA) |
|------|-----------------|-----------------|
| 格式 | 线性管分隔转储 | SSA 指令 |
| 层级 | HIR 树结构 | SSA + CFG |
| 解析器 | ~30 行 | ~120 行 |
| 输出大小 | ~2.5 MB | **~1.2 MB** |
| 管线步骤 | HAT → HIR → SSA → LIR | **HAT → SSA → LIR** |
| Phi 节点 | 隐式（在 SsaBuilder 中） | **显式（`@phi` 指令）** |
| 可读性 | 中等（管道字段） | **高（SSA 指令）** |
| 优化友好 | 需额外转换 | **原生 SSA** |
| 管线共存 | 不支持 | **支持（PHIR/HAT 并行）** |
| 成熟度参考 | 无 | **LLVM + Rust MIR** |

**关键改进**：v2.0 跳过 HIR → SSA 转换，节省一个完整阶段。

---

## 附录 B：指令速查表

```
; ── 常量 ──
@t = @i32_const <val> : Int
@t = @f64_const <val> : Float
@t = @const_str "<str>" : String
@t = @bool_const <true|false> : Bool
@t = @null : Any

; ── Phi 节点 ──
@t = @phi(@a, @b, ...) : Type        ← 循环入口/分支汇合点

; ── 算术 ──
@t = @add(@a, @b) : Type
@t = @sub(@a, @b) : Type
@t = @mul(@a, @b) : Type
@t = @div(@a, @b) : Type
@t = @rem(@a, @b) : Int
@t = @neg(@a) : Type

; ── 比较 ──
@t = @icmp @i32_eq(@a, @b) : Bool
@t = @icmp @i32_ne(@a, @b) : Bool
@t = @icmp @i32_slt(@a, @b) : Bool
@t = @icmp @i32_sgt(@a, @b) : Bool
@t = @icmp @i32_sle(@a, @b) : Bool
@t = @icmp @i32_sge(@a, @b) : Bool
@t = @icmp @i32_ult(@a, @b) : Bool
@t = @icmp @i32_ugt(@a, @b) : Bool
@t = @icmp @f64_eq(@a, @b) : Bool
@t = @icmp @f64_lt(@a, @b) : Bool
@t = @icmp @str_eq(@a, @b) : Bool

; ── 逻辑 ──
@t = @and(@a, @b) : Bool
@t = @or(@a, @b) : Bool
@t = @xor(@a, @b) : Bool
@t = @not(@a) : Bool

; ── 位运算 ──
@t = @band(@a, @b) : Int
@t = @bor(@a, @b) : Int
@t = @bxor(@a, @b) : Int
@t = @shl(@a, @b) : Int
@t = @shr(@a, @b) : Int
@t = @ushr(@a, @b) : Int

; ── 类型转换 ──
@t = @i32_to_f64(@a) : Float
@t = @f64_to_i32(@a) : Int
@t = @i32_to_str(@a) : String
@t = @f64_to_str(@a) : String
@t = @str_to_i32(@a) : Int

; ── 内存 ──
@t = @alloc { size: N, align: M } : !stackslot
@store @val => @slot : Type
@t = @load @slot : Type

; ── 字符串 ──
@t = @str_concat(@a, @b) : String
@t = @str_len(@a) : Int
@t = @str_sub(@s, @start, @len) : String
@t = @str_char(@s, @i) : Char

; ── 集合 ──
@t = @alloc_list { count: N } : !list
@t = @list_set(@list, @i, @v) : !list
@t = @list_get(@list, @i) : Type
@t = @list_len(@list) : Int
@t = @alloc_map { count: N } : !map
@t = @map_set(@map, @k, @v) : !map
@t = @map_get(@map, @k) : Type

; ── 对象 ──
@t = @new @Type { fields: [@f0, @f1] } : !ptr
@t = @field @obj, .name : Type
@field_set @obj, .name, @val : Unit

; ── 调用 ──
@t = @call @fn_name(@args) : Type

; ── 终止符 ──
@br @label
@br_if @cond => @then, @else
@ret @val : Type
@ret () : Unit
```

---

## 附录 C：与成熟 IR 的对比

| 特性 | HAT v2.0 | Cranelift IR | LLVM IR | Rust MIR | GIMPLE |
|------|----------|-------------|---------|----------|--------|
| SSA | ✅ | ✅ | ✅ | ✅ | ❌ |
| Phi 节点 | ✅（标准 SSA） | ✅ | ✅ | ✅ | ❌ |
| BB 参数替代 Phi | ❌（当前用 Phi） | ✅ | ❌ | ❌ | ❌ |
| 显式类型标注 | ✅ | ✅ | ✅ | ✅ | ❌ |
| 方言扩展 | ⚠️（未来） | ❌ | ✅ | ❌ | ❌ |
| 嵌套控制流 | ❌（显式 CFG） | ❌ | ❌ | ❌ | ❌ |
| 解析器复杂度 | ~120 行 | ~500 行 | ~3000 行 | ~1000 行 | ~200 行 |
| 输出紧凑性 | ★★★★☆ | ★★★★★ | ★★★☆☆ | ★★★☆☆ | ★★★★☆ |
| 管线共存 | ✅（PHIR/HAT 并行） | ❌ | ❌ | ❌ | ❌ |

HAT v2.0 定位：**LLVM IR 的简化版**——保留核心 SSA + Phi 节点特性，但指令集更小、解析器更轻量、支持双管线共存。

**与 Cranelift 的区别**：Cranelift 使用 BB 参数替代 Phi，HAT 使用 Phi 节点（与 LLVM/MIR 一致）。这是为了与现有 SsaBuilder 兼容，零改造成本。

---

## 附录 D：性能验证方案

```powershell
# 基准测试脚本

$src = "tests/photon/P3/06_syscall_exit.aura"

# PHIR 解析
$env:AURA_PHOTON_FORMAT = "phir"
$phirTime = Measure-Command {
    aura run aura/compiler/aura/lang/compiler/backend/photon/PhotonDriver.aura
}

# HAT v2.0 解析
$env:AURA_PHOTON_FORMAT = "hat"
$hatTime = Measure-Command {
    aura run aura/compiler/aura/lang/compiler/backend/photon/PhotonDriver.aura
}

# 预期
Write-Host "PHIR parse: $($phirTime.TotalSeconds)s"
Write-Host "HAT parse:  $($hatTime.TotalSeconds)s"
Write-Host "Speedup:    $($phirTime.TotalSeconds / $hatTime.TotalSeconds)x"
# 预期 Speedup > 50x
```

---

## 附录 E：错误恢复策略

```
; 解析器错误处理流程：

; 1. 跳过坏行并记录
; [WARN] hat_parse: line 42: unknown instruction "@foo"
;
; 2. 缺少终止符时自动追加
; [WARN] hat_parse: bb "check" at line 55: missing terminator, added @ret () : Unit
;
; 3. 无效 BB 标签时生成默认标签
; [WARN] hat_parse: bb "" at line 60: empty label, using "bb3"
;
; 4. Phi 前驱不匹配时回退
; [WARN] hat_parse: line 65: phi has mismatched predecessors, treating as instruction
;
; 5. 类型不匹配时回退为 Any
; [WARN] hat_parse: line 65: type mismatch in @add, using Any
;
; 6. 汇总报告
; [INFO] hat_parse: parsed 45 functions, 12 warnings, 0 errors
```

---

## 附录 F：包结构与文件清单

```
aura/lang/compiler/hir/
├── Hir.aura                    # HIR Arena 定义（已有，PHIR 路径使用）
├── HirLowerer.aura             # AST → HIR 降级（已有）
├── hat/                        # ← HAT v2.0（新增）
│   ├── HatParser.aura          # HAT → SSA MIR 解析（~120 行）
│   ├── HatSerializer.aura      # HIR/SSA → HAT 序列化（~80 行）
│   ├── HatUtils.aura           # 工具对象（~30 行）
│   └── HatInstruction.aura     # 指令定义与发射（~60 行）
├── phir/                       # PHIR（已有，保留不动）
│   ├── PhirSerializer.aura     # HIR → PHIR 序列化（已有）
│   └── PhirParser.aura         # PHIR → HIR 解析（已有）
```

**新增文件**：4 个（~290 行）
**修改文件**：3 个（~35 行）
**不动文件**：SsaBuilder、SSA MIR、Lowering、InstructionSelector、RegAlloc、X86Emitter

---

## 附录 G：双管线一致性验证

```powershell
# 验证 HAT 和 PHIR 编译同一源码产出相同的 exe

$src = "tests/photon/P1/03_arithmetic.aura"

# PHIR 路径
$env:AURA_PHOTON_FORMAT = "phir"
aura build -b photon -f phir $src -o build/phir_test/test.exe
$phirHash = (Get-FileHash build/phir_test/test.exe).Hash

# HAT 路径
$env:AURA_PHOTON_FORMAT = "hat"
aura build -b photon -f hat $src -o build/hat_test/test.exe
$hatHash = (Get-FileHash build/hat_test/test.exe).Hash

# 对比
if ($phirHash -eq $hatHash) {
    Write-Host "PASS: HAT and PHIR produce identical executables"
} else {
    Write-Host "FAIL: HAT and PHIR produce different executables"
    Write-Host "PHIR: $phirHash"
    Write-Host "HAT:  $hatHash"
}
```

---

*文档结束 — HAT v2.0 设计文档*
