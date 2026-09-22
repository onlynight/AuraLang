# Photon IR Format Specification v1.0

> **定位**：语言无关的标准中间表示，供所有前端产出，供 Photon 后端消费。
>
> **设计哲学**：借鉴 LLVM IR、Rust MIR、Cranelift IR、Swift SIL、MLIR 等优秀 IR 设计的精华。
>
> **格式**：
> - `.phir` — 伪代码文本格式（人类可读可写）
> - `.phir.bin` — 二进制格式（机器高效）
>
> **关系**：
> ```
> [Any Front-end] (Aura / Rust / C / Python / ...)
>         │
>         ▼
>    [Photon IR]  ← .phir 文本 / .phir.bin 二进制
>         │
>         ▼
>    [Photon Backend] (SSA → LIR → DAG → RegAlloc → Encode)
> ```

---

## 1. 设计原则（从优秀 IR 借鉴）

| 来源 IR | 借鉴特性 | 说明 |
|---------|---------|------|
| **LLVM IR** | SSA、指令属性、模块系统、指令分类 | 基础架构 |
| **Rust MIR** | 基本块+终止符、Place/Rvalue/Operand、Locals、调试作用域 | 结构化 CFG |
| **Cranelift IR** | BB 参数（替代 Phi）、显式栈槽、函数前言、全局值、调用约定、验证器 | 简洁高效 |
| **Swift SIL** | 值所有权（owned/borrowed/guaranteed）、ARC 内置 | 内存安全 |
| **MLIR** | 方言系统、可组合操作 | 可扩展性 |
| **Go SSA** | 逃逸分析、栈着色 | 优化友好 |

### 1.1 核心架构决策

```
┌─────────────────────────────────────────────────────────────┐
│                    Photon IR 架构                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Module                                                     │
│  ├── Globals (常量、全局变量)                                  │
│  ├── Function Declarations (外部函数)                         │
│  ├── Function Definitions                                    │
│  │   ├── Preamble (函数前言: 栈槽、函数签名、函数标志)          │
│  │   ├── Basic Blocks (基本块)                               │
│  │   │   ├── Statements (语句: 单一后继)                      │
│  │   │   └── Terminator (终止符: 多后继, 块末尾)              │
│  │   └── Debug Info (调试信息: 变量作用域)                     │
│  └── Verification (IR 验证)                                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 文本格式 (`.phir`)

### 2.1 模块结构

```phir
# 注释以 # 开头
# module <name> target <triple> flags <hex>
# source <path>

@str.hello = "hello world"          # 全局常量
@counter   = i32 0                   # 全局变量（可写）

native fun write(fd: i32, buf: ptr, len: i64) -> i32   # 原生函数声明
native fun malloc(size: i64) -> ptr                      # 系统调用

fun strlen(s: ptr) -> i64 {
    # ... 函数体
}

fun main() -> i32 {
    # ... 函数体
}
```

### 2.2 完整语法

```
phir_file   = module_header globals* declarations* functions*
module_header = ('#' 'module' ident 'target' ident 'flags' hex)?
               ('#' 'source' string)?

globals     = '@' ident ':' type '=' value
              | '@' ident '=' value

declarations = 'native fun' ident '(' params? ')' '->' type (':' attr)*

functions   = 'fun' ident '(' params? ')' ('->' type)? (':' attr)* '{' preamble blocks '}'

preamble    = ('ss' ident '=' type 'offset' i32)*           # 栈槽声明
              ('fn' ident '=' ident '(' params? ')' '->' type)*  # 函数引用
              ('sig' ident '(' params? ')' '->' type)*      # 签名声明

blocks      = basic_block*
basic_block = 'bb' ident '(' params? ')' ':' '{' statements terminator '}'
              # 或使用缩进语法（见 §2.5）

statements  = statement*
statement   = assign | def | store | load_stmt | dbg | unreachable

terminator  = return_term | br_term | condbr_term | call_term | unreachable_term

assign      = place '=' rvalue
def         = ('let' | 'var') ident (':' type)? '=' expr
store       = '*' place '=' expr
load_stmt   = var_stmt ':' type '=' '*' place

return_term = 'return' expr?
br_term     = 'br' ident '(' args? ')'
condbr_term = 'br if' expr (',' ident '(' args? ')')? (',' ident '(' args? ')')?
call_term   = place? '=' 'call' target '(' args? ')' (',' ident '(' args? ')')? (',' ident '(' args? ')')?
unreachable_term = 'unreachable'

place       = ident | '*' place | place '.' ident | place '[' expr ']' | 'this'
rvalue      = operand | rvalue_op
operand     = ('const' | 'copy' | 'move') value_expr
              | '@' ident        # 全局引用
              | '%' ident        # 局部引用
              | ident            # 隐式 copy

rvalue_op   = binary_expr | unary_expr | call_expr | new_expr
              | borrow_expr | deref_expr | field_expr | index_expr
              | cast_expr | sizeof_expr | typeid_expr | isa_expr

binary_expr = value_expr binop value_expr
unary_expr  = unop value_expr

value_expr  = operand | '(' value_expr ')' | func_call
func_call   = ident '(' args? ')'

binop       = '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '<<' | '>>' | '>>>'
              | '==' | '!=' | '<' | '>' | '<=' | '>=' | '&&' | '||' | 'is' | 'as'

unop        = '!' | '-' | '+' | '*' | '&' | '~' | '~=' | '&mut' | '&'

args        = value_expr (',' value_expr)*
params      = param (',' param)*
param       = ident (':' type)?

type        = type_name ('<' type (',' type)* '>')? ('?' | '*' | '&' ('mut')?)?
type_name   = ident

attr        = ident ('(' value_expr ')')?
```

### 2.3 类型系统

| 类型 | 说明 | 参考 |
|------|------|------|
| `void` | 无返回 | LLVM IR |
| `bool` / `i1` | 布尔 | LLVM IR |
| `i8` / `i16` / `i32` / `i64` / `i128` | 整数 | LLVM IR + Cranelift |
| `f32` / `f64` | 浮点 | LLVM IR + Cranelift |
| `ptr` | 不透明指针 | LLVM IR |
| `ptr<T>` | 类型化指针 | 自定义 |
| `ref<T>` | 不可变引用 | Rust MIR |
| `ref mut<T>` | 可变引用 | Rust MIR |
| `string` | Aura 字符串 | 自定义 |
| `struct<T>` | 匿名结构体 | LLVM IR |
| `T` | 命名类型 | 通用 |
| `list<T>` | 泛型列表 | 自定义 |
| `map<K,V>` | 泛型映射 | 自定义 |
| `(T) -> R` | 函数类型 | Cranelift |
| `vector<N, T>` | SIMD 向量 | Cranelift |
| `owned<T>` | 拥有所有权 | Swift SIL |
| `borrowed<T>` | 借用 | Swift SIL |
| `guaranteed<T>` | 保证 | Swift SIL |

### 2.4 函数结构（Cranelift 风格）

```phir
# 函数前言 (Preamble): 声明函数体中使用的实体
fun average(array: ptr, count: i64) -> f32 {
    # 栈槽声明
    ss0 = f64 8

    # 直接调用的函数引用
    fn0 = @strlen(ptr) -> i64

    # 间接调用的函数签名
    sig0 = (i32, i32) -> i32

    # 全局值
    gv0 = @str.hello

    bb entry(v0: ptr, v1: i64):
        v2 = const f64 0.0
        store f64 v2, ss0
        br if v1, block1, block2

    bb block1:
        v3 = const i64 0
        br block2(v3)

    bb block2(v4: i64):
        v5 = imul v4, 4
        v6 = add v0, v5
        v7 = load.f32 v6
        v8 = fpromote.f64 v7
        v9 = load.f64 ss0
        v10 = fadd v8, v9
        store f64 v10, ss0
        v11 = add v4, 1
        v12 = icmp ult v11, v1
        br if v12, block2(v11), block3

    bb block3:
        v13 = load.f64 ss0
        v14 = fcvt_from_uint.f64 v1
        v15 = fdiv v13, v14
        v16 = fdemote.f32 v15
        return v16

    bb block4:
        v17 = const f32 NaN
        return v17
}
```

**关键设计**：
- **BB 参数**（Cranelift 风格）：`bb block2(v4: i64):` — 替代 Phi 节点，BB 入口声明参数
- **基本块**：每个 BB 以终止符结束，控制流不可穿透
- **栈槽**：`ss0 = f64 8` — 编译期确定的栈空间，便于寄存器分配
- **函数引用**：`fn0 = @strlen(ptr) -> i64` — 前言中声明被调函数
- **全局值**：`gv0 = @str.hello` — 前言中引用全局变量

### 2.5 缩进语法（人类友好模式）

对于简单的函数，可以使用缩进语法代替显式 BB：

```phir
fun max(a: i32, b: i32) -> i32 {
    if a > b {
        return a
    } else {
        return b
    }
}

fun count_digits(n: i32) -> i32 {
    var count: i32 = 0
    var m: i32 = n
    while m != 0 {
        m = m / 10
        count = count + 1
    }
    return count
}
```

**等价于**：
```phir
fun max(a: i32, b: i32) -> i32 {
    bb entry(v0: i32, v1: i32):
        v2 = icmp sgt v0, v1
        br if v2, block1, block2

    bb block1:
        return v0

    bb block2:
        return v1
}
```

---

## 3. 基本块与终止符（Rust MIR 风格）

### 3.1 语句（Statement）

语句只有一个后继（当前块），以 `;` 结尾：

```phir
v2 = const i64 42;           # 赋值
store i32 v3, ss0;          # 栈存储
load i64 ss1 -> v4;         # 栈加载
debug x => v5;              # 调试映射
```

### 3.2 终止符（Terminator）

终止符结束当前块，可以跳转到其他块：

```phir
return v1;                  # 返回
br block1;                  # 无条件跳转
br if v2, block1, block2;   # 条件跳转
v3 = call fn0(v1, v2);      # 函数调用（可能跳入异常处理）
unreachable;                # 不可达
```

### 3.3 CFG 图（Rust MIR 风格）

```phir
fun fib(n: i32) -> i32 {
    bb entry(v0: i32):
        v1 = icmp slt v0, 2
        br if v1, base, recurse

    bb base:
        return v0

    bb recurse:
        v2 = sub v0, 1
        v3 = sub v0, 2
        v4 = call @fib(i32 v2) -> i32;
        v5 = call @fib(i32 v3) -> i32;
        v6 = add v4, v5
        return v6
}
```

---

## 4. 值与 Place 系统（Rust MIR 风格）

### 4.1 Place（左值）

Place 标识内存中的位置：

```phir
v1              # 局部变量
ss0             # 栈槽
v1.field        # 字段访问
v1[0]           # 下标访问
*v2             # 解引用
```

### 4.2 Operand（操作数）

```phir
const 42        # 常量
copy v1         # 复制（要求 T: Copy）
move v1         # 移动（消耗 v1）
%v1             # 局部引用
@str.hello      # 全局引用
```

### 4.3 Rvalue（右值表达式）

```phir
const i32 42                    # 常量
copy v1                         # 复制
move v1                         # 移动
v1 + v2                         # 二元运算
-v1                             # 一元运算
call fn0(v1, v2)                # 函数调用
new struct { i32, i32 }(v1, v2) # 对象创建
& v1                            # 借用
*v2                             # 解引用
v1.field                        # 字段访问
v1[0]                           # 下标访问
v1 as T                         # 类型转换
sizeof T                        # 类型大小
typeid(v1)                      # 运行时类型
v1 is T                         # 类型检查
```

---

## 5. 显式栈槽（Cranelift 风格）

```phir
fun process_data(buf: ptr) -> i32 {
    # 栈槽声明: 名称 = 类型 对齐字节数
    ss0 = i32 4
    ss1 = f64 8
    ss2 = ptr 8

    bb entry(v0: ptr):
        v1 = load.i32 v0
        store i32 v1, ss0
        v2 = load.i32 ss0
        v3 = mul v2, 2
        store i32 v3, ss0
        return v3
}
```

**优势**：
- 栈空间在编译期确定
- 寄存器分配器可直接分配
- 无需 `alloca` 运行时调用
- 与 LLVM IR 的 `alloca` 不同，不需要内存链追踪

---

## 6. 值所有权（Swift SIL 风格）

```phir
# 函数参数所有权
fun consume(obj: owned(ptr)) -> void {
    # obj 被消费，不能再次使用
    release obj
}

fun use(obj: borrowed(ptr)) -> i32 {
    # obj 是借用的，函数返回后不能再使用
    return getfield obj.value
}

fun ensure(obj: guaranteed(ptr)) -> void {
    # obj 是保证的，ARC 由编译器管理
    retain obj
    release obj
}

# 返回值所有权
fun create() -> owned(ptr) {
    let p = new Foo()
    return p    # 转移所有权给调用者
}

# 移动语义
fun transfer(obj: owned(ptr)) -> void {
    let obj2 = move obj   # 移动所有权
    # obj 不能再使用
    release obj2
}

# 复制语义
fun copy_obj(obj: copy(ptr)) -> void {
    let obj2 = copy obj   # 复制（要求 T: Copy）
    release obj2
    # obj 仍然有效
}
```

---

## 7. 指令属性（LLVM IR 风格）

### 7.1 操作数属性

```phir
# 操作数标记
v1 = add { commutative } v2, v3        # 可交换
v1 = add { associative } v2, v3, v4    # 可结合
v1 = load { readonly } v2              # 只读内存
v1 = store { volatile } v2, v3         # 易失操作
v1 = call { nounwind } fn0(v2)         # 不抛异常
v1 = call { noreturn } fn0(v2)         # 不返回
v1 = call { norecurse } fn0(v2)        # 不递归
v1 = call { tail } fn0(v2)             # 尾调用
v1 = call { inline } fn0(v2)           # 内联提示
v1 = call { speculatable } fn0(v2)     # 可推测
```

### 7.2 函数属性

```phir
fun fast(noinline) -> void { ... }     # 不内联
fun cold() -> void { ... }             # 冷代码
fun noreturn() -> void { ... }         # 不返回
fun nosideeffects() -> void { ... }    # 无副作用
fun noalias() -> void { ... }          # 无别名
fun nobuiltin() -> void { ... }        # 非内建函数
fun always_inline() -> void { ... }    # 总是内联
```

### 7.3 内存属性

```phir
load { readonly } v1                   # 内存只读
store { volatile } v1, v2              # 易失存储
load { atomic { seqcst } } v1          # 原子加载
store { atomic { seqcst } } v1, v2     # 原子存储
fence { seqcst }                       # 内存屏障
```

---

## 8. 异常处理

```phir
# 带异常处理的调用
fun risky(n: i32) -> i32 {
    bb entry(v0: i32):
        v1 = call @divide(i32 100, i32 v0) -> i32
              [return: ok, unwind: catch]

    bb ok:
        return v1

    bb catch:
        v2 = const i32 -1
        return v2
}

# LLVM 风格 invoke
fun safe_div(a: i32, b: i32) -> i32 {
    bb entry(v0: i32, v1: i32):
        v2 = icmp eq i32 v1, 0
        br if v2, error, divide

    bb divide:
        v3 = call @div(i32 v0, i32 v1) -> i32
              [return: done, unwind: catch]

    bb done:
        return v3

    bb catch:
        v4 = const i32 0
        return v4

    bb error:
        v5 = const i32 0
        return v5
}
```

---

## 9. 方言系统（MLIR 风格）

```phir
# 方言前缀
# dialect <name>

# 基础方言（默认）
v1 = add i32 v2, v3

# 内存方言
load.f32 v1                      # load 指令
store.f64 v1, ss0                # store 指令

# 浮点方言
v1 = fpromote.f64 v2             # 浮点提升
v1 = fdemote.f32 v2             # 浮点降级
v1 = fmin v2, v3                 # 浮点最小值
v1 = fmax v2, v3                 # 浮点最大值
v1 = frnd v2                     # 浮点舍入

# SIMD 方言
v1 = vadd i32x4 v2, v3          # SIMD 加法
v1 = vload.f32x4 v2             # SIMD 加载
v1 = vstore.f32x4 v2, v3       # SIMD 存储
v1 = vsplat i32 v2              # SIMD 广播

# 原子方言
v1 = atom.load { seqcst } v2    # 原子加载
v1 = atom.store { seqcst } v2, v3  # 原子存储
v1 = atom.cas { seqcst } v2, v3, v4  # CAS
fence { seqcst }                # 内存屏障

# 调试方言
debug v1                        # 调试断点
printf v1                       # 调试打印
verify v1                       # 断言
```

---

## 10. IR 验证器（Cranelift 风格）

```phir
# 验证器检查
test verifier

function %test(i32) -> i32 {
    bb entry(v0: i32):
        v1 = add i32 v0, 1
        v2 = icmp sgt i32 v1, 0
        br if v2, block1, block2

    bb block1:
        v3 = sub i32 v1, 1
        return v3

    bb block2:
        v4 = const i32 -1
        return v4
}

# 验证规则:
# 1. SSA: 每个值只定义一次
# 2. 支配: 值的定义必须支配其使用
# 3. 类型: 操作数类型与指令匹配
# 4. CFG: 终止符在块末尾
# 5. 无 UB: 无未定义行为
# 6. 栈槽: 栈槽对齐正确
```

---

## 11. 调用约定（Cranelift 风格）

```phir
# 函数签名中的调用约定
fun add(a: i32, b: i32) -> i32 system_v {
    return add a, b
}

fun fast_func(a: i32, b: i32) -> i32 fast {
    return add a, b
}

fun cold_func(a: i32, b: i32) -> i32 cold {
    return add a, b
}

fun win_func(a: i32, b: i32) -> i32 windows_fastcall {
    return add a, b
}

fun probestack(a: i32) -> i32 probestack {
    return add a, 0
}
```

| 约定 | 说明 |
|------|------|
| `system_v` | System V 调用约定（默认） |
| `fast` | 不 ABI 稳定，最佳性能 |
| `cold` | 不 ABI 稳定，不频繁执行的代码 |
| `windows_fastcall` | Windows x64/ARM |
| `probestack` | 需要栈溢出检查 |
| `apple_aarch64` | Apple ARM64 |
| `winch` | Windows 兼容调用约定 |

---

## 12. 全局值（Cranelift 风格）

```phir
# 全局值表达式（在函数前言中）
fun access_globals() -> i32 {
    # VM 上下文（VM 环境）
    gv0 = vmctx

    # 符号地址
    gv1 = @my_global

    # 符号地址 + 偏移
    gv2 = add gv1, 16

    # 从全局值加载
    gv3 = load.i32 gv0[8]

    # colocated 符号（与当前函数同地址）
    gv4 = @local_helper

    bb entry:
        v1 = load.i32 gv1
        v2 = load.i32 gv3
        v3 = call gv0(v1, v2)
        return v3
}
```

---

## 13. 调试信息（Rust MIR 风格）

```phir
fun main() -> i32 {
    # 调试作用域
    scope 1 {
        debug x => v1;
        debug y => v2;
    }

    bb entry:
        v1 = const i32 10
        v2 = const i32 20
        v3 = add i32 v1, v2
        return v3
}

# 调试信息映射:
# - 源码变量名 → IR 值
# - 源码行号 → IR 指令
# - 作用域 → IR 块
```

---

## 14. 完整示例

### 14.1 Hello World

```phir
# module hello target x86_64-pc-windows-msvc flags 0x01
# source hello.aura

@str.hello = "hello world"

native fun nt_write(fd: i32, buf: ptr, len: i64) -> i32   # syscall 1
native fun nt_exit(code: i32) -> void                        # syscall 0x10

fun strlen(s: ptr) -> i64 {
    ss0 = i64 8
    ss1 = ptr 8

    bb entry(v0: ptr):
        v1 = const i64 0
        store i64 v1, ss0
        store ptr v0, ss1
        br loop

    bb loop:
        v2 = load.ptr ss1
        v3 = load.i8 v2
        v4 = icmp eq i8 v3, 0
        br if v4, done, loop_body

    bb loop_body:
        v5 = load.i64 ss0
        v6 = add i64 v5, 1
        store i64 v6, ss0
        v7 = load.ptr ss1
        v8 = add ptr v7, 1
        store ptr v8, ss1
        br loop

    bb done:
        v9 = load.i64 ss0
        return v9
}

fun main() -> i32 {
    bb entry:
        v0 = call @strlen(ptr @str.hello) -> i64
        v1 = call @nt_write(i32 1, ptr @str.hello, i64 v0) -> i32
        call @nt_exit(i32 0)
        v2 = const i32 0
        return v2
}
```

### 14.2 循环（Cranelift 风格，BB 参数替代 Phi）

```phir
fun sum_range(lo: i32, hi: i32) -> i32 {
    ss0 = i32 4

    bb entry(v0: i32, v1: i32):
        v2 = const i32 0
        store i32 v2, ss0
        br loop(v0)

    bb loop(v3: i32):
        v4 = icmp sgt i32 v3, v1
        br if v4, done, body

    bb body:
        v5 = load.i32 ss0
        v6 = add i32 v5, v3
        store i32 v6, ss0
        v7 = add i32 v3, 1
        br loop(v7)

    bb done:
        v8 = load.i32 ss0
        return v8
}
```

### 14.3 异常处理

```phir
native fun div(a: i32, b: i32) -> i32

fun safe_div(a: i32, b: i32) -> i32 {
    bb entry(v0: i32, v1: i32):
        v2 = icmp eq i32 v1, 0
        br if v2, error, divide

    bb divide:
        v3 = call @div(i32 v0, i32 v1) -> i32
              [return: done, unwind: catch]

    bb done:
        return v3

    bb catch:
        v4 = const i32 0
        return v4

    bb error:
        v5 = const i32 0
        return v5
}
```

### 14.4 值所有权（Swift SIL 风格）

```phir
fun consume(obj: owned(ptr)) -> void {
    bb entry(v0: ptr):
        release v0
        return
}

fun use(obj: borrowed(ptr)) -> i32 {
    bb entry(v0: ptr):
        v1 = load.i32 v0
        return v1
}

fun create() -> owned(ptr) {
    bb entry:
        v1 = new Foo()
        return v1
}

fun transfer(obj: owned(ptr)) -> owned(ptr) {
    bb entry(v0: ptr):
        return v0    # 转移所有权
}
```

---

## 15. 二进制格式 (`.phir.bin`)

### 15.1 文件头 (128 字节)

```
Offset  Size  Field
────── ──── ─────────────────────────
0x00    4B    magic = "PHIR"
0x04    2B    version (u16 LE) = 1
0x06    2B    flags (u16 LE)
0x08    4B    header_size = 128
0x0C    4B    module_name_offset
0x10    4B    module_name_len
0x14    4B    source_file_offset
0x18    4B    source_file_len
0x1C    4B    target_triple_offset
0x20    4B    target_triple_len
0x24    4B    str_pool_offset
0x28    4B    str_pool_count
0x2C    4B    str_pool_size
0x30    4B    type_table_offset
0x34    4B    type_table_count
0x38    4B    global_table_offset
0x3C    4B    global_table_count
0x40    4B    func_table_offset
0x44    4B    func_table_count
0x48    4B    bb_table_offset
0x4C    4B    bb_table_count
0x50    4B    value_table_offset
0x54    4B    value_table_count
0x58    4B    instr_table_offset
0x5C    4B    instr_table_count
0x60    4B    dbg_table_offset
0x64    4B    dbg_table_count
0x68    4B    attr_table_offset
0x6C    4B    attr_table_count
0x70    4B    dialect_table_offset
0x74    4B    dialect_table_count
0x78    4B    sig_table_offset
0x7C    4B    sig_table_count
```

### 15.2 表结构

| 表 | 内容 | 参考 |
|----|------|------|
| **字符串池** | `u32 len + utf8 bytes` | LLVM Bitcode |
| **类型表** | `u8 tag + type-specific` | LLVM IR |
| **全局表** | `name_idx, type_idx, flags, size, data_offset` | LLVM IR |
| **函数表** | `name_idx, sig_idx, preamble_idx, bb_range, attr_idx` | Cranelift |
| **BB 表** | `name_idx, params, instr_range, pred/succ, attr_idx` | Rust MIR |
| **值表** | `type_idx, kind, block_idx, use/def count` | LLVM IR |
| **指令表** | `opcode, dialect, type_idx, operands, attr_idx` | Cranelift |
| **调试表** | `var_name, place, scope, line, col` | Rust MIR |
| **属性表** | `attr_type, attr_value` | LLVM IR |
| **方言表** | `dialect_name, dialect_version` | MLIR |
| **签名表** | `params, returns, callconv` | Cranelift |

---

## 16. 指令操作码

| Opcode | 名称 | 方言 | 文本对应 |
|--------|------|------|---------|
| `0x01` | `CONST` | base | `const` |
| `0x10` | `DEF` | base | `let` / `var` |
| `0x11` | `LOAD` | mem | `load` / `*` |
| `0x12` | `STORE` | mem | `store` / `*=` |
| `0x13` | `STACK_LOAD` | mem | `ss_load` |
| `0x14` | `STACK_STORE` | mem | `ss_store` |
| `0x20` | `ADD` | base | `+` |
| `0x21` | `SUB` | base | `-` |
| `0x22` | `MUL` | base | `*` |
| `0x23` | `DIV` | base | `/` |
| `0x24` | `REM` | base | `%` |
| `0x25` | `AND` | base | `&` |
| `0x26` | `OR` | base | `|` |
| `0x27` | `XOR` | base | `^` |
| `0x28` | `SHL` | base | `<<` |
| `0x29` | `SHR` | base | `>>` |
| `0x2A` | `ASHR` | base | `>>>` |
| `0x30` | `ICMP` | base | `icmp` |
| `0x31` | `FCMP` | float | `fcmp` |
| `0x40` | `BR` | base | `br` |
| `0x41` | `COND_BR` | base | `br if` |
| `0x42` | `BR_TABLE` | base | `br table` |
| `0x43` | `RET` | base | `return` |
| `0x48` | `CALL` | base | `call` |
| `0x49` | `INVOKE` | base | `call [return: , unwind: ]` |
| `0x4A` | `VCALL` | obj | `vcall` |
| `0x50` | `NEW` | obj | `new` |
| `0x51` | `RETAIN` | obj | `retain` |
| `0x52` | `RELEASE` | obj | `release` |
| `0x53` | `GETFIELD` | obj | `getfield` / `.field` |
| `0x54` | `SETFIELD` | obj | `setfield` / `.field=` |
| `0x55` | `GEP` | mem | `gep` |
| `0x60` | `PHI` | base | `phi` |
| `0x61` | `BR_PARAM` | base | BB 参数 |
| `0x70` | `SYSCALL` | syscall | `syscall` |
| `0x71` | `FENCE` | atom | `fence` |
| `0x72` | `ATOMIC_LOAD` | atom | `atom.load` |
| `0x73` | `ATOMIC_STORE` | atom | `atom.store` |
| `0x74` | `ATOMIC_CAS` | atom | `atom.cas` |
| `0x80` | `DEBUG` | dbg | `debug` |
| `0x81` | `PRINTF` | dbg | `printf` |
| `0x82` | `VERIFY` | dbg | `verify` |

---

## 17. 与优秀 IR 的对比

| 特性 | LLVM IR | Rust MIR | Cranelift IR | Swift SIL | MLIR | **Photon IR** |
|------|---------|----------|-------------|-----------|------|--------------|
| **SSA** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **Phi 节点** | ✅ | ❌ (BB 参数) | ❌ (BB 参数) | ✅ | ✅ | ✅ + BB 参数 |
| **终止符** | ❌ (无嵌套) | ✅ | ✅ | ❌ | ❌ | ✅ |
| **Place 系统** | ❌ | ✅ | ❌ | ❌ | ❌ | ✅ |
| **栈槽** | ❌ (alloca) | ❌ | ✅ | ❌ | ❌ | ✅ |
| **内存链** | ❌ (别名分析) | ❌ | ❌ | ❌ | ❌ | ✅ 可选 |
| **值所有权** | ❌ | ❌ | ❌ | ✅ | ❌ | ✅ |
| **方言系统** | ❌ | ❌ | ❌ | ❌ | ✅ | ✅ |
| **指令属性** | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ |
| **验证器** | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **调用约定** | ✅ | ✅ | ✅ | ✅ | ❌ | ✅ |
| **全局值** | ✅ | ❌ | ✅ | ❌ | ✅ | ✅ |
| **调试信息** | ✅ | ✅ | ❌ | ✅ | ✅ | ✅ |
| **文本格式** | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **二进制格式** | ✅ | ❌ | ✅ | ✅ | ✅ | ✅ |
| **伪代码可读** | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| **无 UB** | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## 18. 实施路线

### Phase 1: 核心解析器 (1 周)

| 任务 | 产出 | 参考 |
|------|------|------|
| 1.1 PhirParser (Aura 侧) | `phir/PhirParser.aura` | LLVM Bitcode Reader |
| 1.2 PhirPrinter (Aura 侧) | `phir/PhirPrinter.aura` | LLVM IR Print |
| 1.3 PhirVerify (Aura 侧) | `phir/PhirVerify.aura` | Cranelift Verifier |

### Phase 2: 前端集成 (1 周)

| 任务 | 产出 | 参考 |
|------|------|------|
| 2.1 Rust 前端产出 .phir | `cmd_build_photon` 改造 | Cranelift Frontend |
| 2.2 --emit-phir CLI 参数 | `main.rs` | LLVM --emit-llvm |
| 2.3 从 .phir 加载管线 | `PhotonPipeline.compilePhir()` | Cranelift Codegen |

### Phase 3: 二进制格式 (1 周)

| 任务 | 产出 | 参考 |
|------|------|------|
| 3.1 二进制写入器 (Rust) | `phir_bin_writer.rs` | LLVM Bitcode Writer |
| 3.2 二进制读取器 (Aura) | `phir/PhirBinReader.aura` | Cranelift CLIF Reader |
| 3.3 往返测试 | `phir_roundtrip_tests` | LLVM Roundtrip |

### Phase 4: 工具链 (1 周)

| 任务 | 产出 | 参考 |
|------|------|------|
| 4.1 phir-opt (优化 pass) | `phir/PhirOpt.aura` | LLVM Optimization |
| 4.2 phir-link (模块链接) | `phir/PhirLink.aura` | LLVM Link |
| 4.3 phir-disasm (反汇编) | `phir/PhirDisasm.aura` | LLVM Disasm |
| 4.4 llvm-phir (转换器) | `tools/llvm-phir/` | LLVM Transcode |

---

## 19. 设计决策记录

| # | 决策 | 原因 | 参考 |
|---|------|------|------|
| D1 | BB 参数替代 Phi | 更清晰的 SSA，无支配检查 | Cranelift IR |
| D2 | 显式终止符 | 结构化 CFG，每块以终止符结束 | Rust MIR |
| D3 | Place 系统 | 左值表达式，支持借用/移动 | Rust MIR |
| D4 | 显式栈槽 | 编译期栈分配，无需 alloca | Cranelift IR |
| D5 | 值所有权 | owned/borrowed/guaranteed | Swift SIL |
| D6 | 方言系统 | 可扩展指令集 | MLIR |
| D7 | 指令属性 | 优化提示（commutative 等） | LLVM IR |
| D8 | 调用约定 | 显式 ABI | Cranelift IR |
| D9 | 全局值表达式 | VM 上下文、符号偏移 | Cranelift IR |
| D10 | IR 验证器 | 编译时检查正确性 | Cranelift Verifier |
| D11 | 伪代码语法 | 人类可读，缩进作用域 | 自定义 |
| D12 | 无 UB | 所有指令有定义行为 | Cranelift IR |
