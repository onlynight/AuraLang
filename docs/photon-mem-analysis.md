# Photon 编译后端内存分析：根因诊断与优化方案（含 20GB 死循环分析）

> **定位**：50k 行源码编译，常规内存 >1 GB，极端情况可达 20 GB。本分析从代码级定位全部根因，给出按投入/产出排序的优化路线。

---

## 一、编译管线总览

Photon 编译管线分为两大执行域，二者通过子进程边界串联：

```
┌─── 域1: Rust 前端 (aura.exe / photon.exe) ─────────────────────────┐
│  源码 → Lexer → Parser(AST) → Sema → HIR → .phir 文本文件          │
│  内存占用：AST + HIR + sema 表（一次性，编译完即释放）               │
└──────────────────────────┬────────────────────────────────────────┘
                           │ 子进程调用
┌──────────────────────────▼────────────────────────────────────────┐
│  域2: Aura VM (PhotonDriver.aura → PhotonPipeline.aura)            │
│  .phir → SSA → LIR → MachineDag → RegAlloc → Encode → COFF → exe  │
│  内存占用：VM 堆对象 + 调用帧 + 指令栈 + 中间表示对象（持续累积）     │
└─────────────────────────────────────────────────────────────────────┘
```

**结论**：
- **1 GB 基线**：VM 堆对象的 `HashMap` 字段存储 + 热路径 `clone` 开销
- **20 GB 峰值**：另有 4 类**死循环级放大效应**，其中 2 类是已知的真实 bug（源码中有注释记录）

---

## 二、VM 内存模型的根本缺陷（1 GB 基线）

### 2.1 对象字段用 `HashMap<u16, Value>` 存储 —— 最大浪费源

**代码位置**：`rust/compiler/src/vm/heap.rs:26`

```rust
pub enum HeapData {
    Object {
        type_tag: u16,
        fields: HashMap<u16, Value>,   // ← 所有对象字段用 HashMap 存储
        vtable: Option<HashMap<u16, usize>>,
    },
    ...
}
```

**量化分析**：

Rust `HashMap<u16, Value>` 的内部布局：
| 组成 | 每条目开销 | 说明 |
|------|-----------|------|
| 桶数组（bucket） | ~40 字节/桶 | `Bucket<Hash, Key, Val>` 含哈希位掩码+键+值指针 |
| 负载因子 | 0.667 | 实际容量 = 条目数 × 1.5 |
| 空槽位（tombstone） | 40 字节 | 删除后留空槽 |

**每个对象的字段表开销**（假设 N 个字段）：
```
N 字段对象 → HashMap 容量 = N × 1.5（向上取 2 的幂）
桶数组开销 = 2^⌈log2(N×1.5)⌉ × 40 字节
每桶开销   = 40 字节（含 Hash(8) + Key(2) + Value(~56) + 对齐）
```

| 字段数 N | 实际桶数 | 桶数组大小 | 有效字段数据 | **空间放大倍数** |
|----------|---------|-----------|-------------|----------------|
| 2 | 8 | 320 B | 112 B | **2.9×** |
| 5 | 16 | 640 B | 280 B | **2.3×** |
| 10 | 32 | 1,280 B | 560 B | **2.3×** |
| 20 | 64 | 2,560 B | 1,120 B | **2.3×** |
| 50 | 128 | 5,120 B | 2,800 B | **1.8×** |

**对比方案**：使用 `Vec<(u16, Value)>` 代替 `HashMap<u16, Value>`：
- 每个字段只需 ~58 字节（2 键 + 56 值）
- 无桶开销、无碎片
- 字段数 ≤ 64 时 `binary_search` 比哈希查找更快
- 空间可降 **60-70%**

**编译器场景影响**：
- Photon 管线每个阶段（SSA/LIR/DAG/RegAlloc）为每条指令/每个节点创建一个 Aura 对象
- 每个对象至少 4-8 个字段（类型、操作数、SSA 编号、块归属、支配关系等）
- 50k 行源码 → 估计 **10-20 万个 SSA 节点 + 10-20 万个 LIR 指令 + 5-10 万个 DAG 节点**
- 仅字段表 HashMap 桶开销就占 **200-400 MB**

### 2.2 `Value` 枚举的递归克隆 —— 隐藏的内存放大器

**代码位置**：`rust/compiler/src/vm/value.rs:14-37`

```rust
#[derive(Clone, Debug)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Rc<str>),           // ← Rc 共享，零拷贝 ✓
    Null,
    Ref(usize),
    Weak(usize),
    Ptr(i64),
    List(Vec<Value>),       // ← Vec<Value> 递归克隆！
    Map(HashMap<Value, Value>),  // ← HashMap 递归克隆！
}
```

**问题**：`Value::List(Vec<Value>)` 和 `Value::Map(HashMap<Value, Value>)` 在克隆时会**递归克隆整个嵌套结构**。

编译器代码中大量使用 `mutableListOf(...)` 和 `mutableMapOf(...)` 构建中间数据结构。当这些值被：
- 压入/弹出操作数栈（`Instr::LoadVar` → `clone()`）
- 写入局部变量槽（`Instr::StoreVar` → 直接移动，无克隆）
- 作为函数参数传递（`pop_n` → `split_off`，无克隆 ✓）
- 在集合操作中传递（`ListPush` → `Value::clone`）

每当克隆一个含 `List` 或 `Map` 的 `Value`，整个嵌套树都被深拷贝。

**编译器场景影响**：
- 寄存器分配的干涉图（interference graph）以 Map 形式存储
- SSA 的支配树以嵌套 List 形式存储
- 每次遍历干涉图节点时克隆其相邻边表 → 指数级复制

### 2.3 `pop()` 无条件克隆函数名 —— 热路径上的无谓分配

**代码位置**：`rust/compiler/src/vm/interp.rs:1957-1966`

```rust
fn pop(&mut self, top: usize) -> Result<Value, VmError> {
    let func_name = self.module.funcs[self.frames[top].func].name.clone();  // ← 每次pop都clone!
    let ip = self.frames[top].ip;
    match self.frames[top].stack.pop() {
        Some(v) => Ok(v),
        None => Err(VmError::Runtime(format!(
            "operand stack underflow in `{}` at ip={}", func_name, ip
        ))),
    }
}
```

`func_name` 是一个 `String`。每次 `pop()` 调用（操作数栈弹出）都会克隆这个字符串，即使它仅在栈下溢错误路径中使用。

**量化**：编译器管线执行数百万条指令，每条指令平均触发 2-3 次 `pop()`。假设 50k 行源码产生 500 万次 `pop()`：
- 每次 clone 一个 ~20 字节函数名 → **100 MB** 无谓分配（虽然立即释放，但增加 GC 压力和碎片）

**`pop_n()` 同样有问题**（`interp.rs:1969-1984`）：使用 `split_off` 创建新 Vec，O(n) 复杂度。

### 2.4 `step()` 每条指令都克隆 Instr —— 解释循环的固有开销

**代码位置**：`rust/compiler/src/vm/interp.rs:147`

```rust
let instr = self.module.funcs[func].code[ip].clone();  // ← 每条指令都clone
self.frames[top].ip += 1;
self.exec_instr(top, func, instr)
```

`Instr` 枚举有 ~70 个变体，最大变体 `PushHandler(usize, u16, u16)` 占 12 字节 + 标签 = **16 字节**。

每次 `step()` 克隆 16 字节看似无害，但 500 万次指令 × 16 字节 = **80 MB** 临时分配。更重要的是，`Instr` 的 `Clone` 实现包含枚举变体判别和分支预测，在热循环中影响 CPU 流水线。

### 2.5 `do_call_native()` 克隆整个原生函数描述符

**代码位置**：`rust/compiler/src/vm/interp.rs:1451`

```rust
let native = self.module.natives[idx].clone();  // ← 含 Vec<u8> + Option<String>
```

`BytecodeNative` 包含 `param_types: Vec<u8>` 和 `ffi_lib: Option<String>`。每次原生调用都克隆这些堆分配。编译器代码中有数千次原生调用（字符串操作、集合操作、文件操作），每次调用都触发堆分配。

### 2.6 `const_to_value()` 双重字符串分配

**代码位置**：`rust/compiler/src/vm/interp.rs:2313-2320`

```rust
fn const_to_value(c: &crate::codegen::opcode::Const) -> Value {
    match c {
        Const::Str(s) => Value::str_(s.clone()),  // ← clone String → Rc<str>，双重分配
        ...
    }
}
```

`Const::Str(String)` 存储的是 `String`，`Value::str_()` 内部调用 `Rc::from(s.into().as_str())` 创建 `Rc<str>`。这意味着：
1. `s.clone()` — 分配一个新 `String`（堆分配 + memcpy）
2. `Value::str_()` — 将 `String` 转为 `Rc<str>`（又一次分配）

**修复**：将 `Const::Str` 改为 `Const::Str(Rc<str>)`，`const_to_value` 改为 `Value::Str(Rc::clone(s))`。

### 2.7 `Value::as_string()` 无条件分配

**代码位置**：`rust/compiler/src/vm/value.rs:118-123`

```rust
pub fn as_string(&self) -> String {
    match self {
        Value::Str(s) => s.to_string(),       // ← String::from(&str) 分配新String
        other => format!("{}", other),         // ← format! 分配新String
    }
}
```

`as_string()` 在几乎所有原生函数签名匹配、类型转换、格式化输出路径上被调用。每次调用都分配新 `String`，即使源值本身就是 `Rc<str>`。

### 2.8 `bin_add()` 字符串拼接用 `format!`

**代码位置**：`rust/compiler/src/vm/interp.rs:2331-2335`

```rust
fn bin_add(a: Value, b: Value) -> Value {
    if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
        return Value::str_(&format!("{}{}", a, b));  // ← format! 分配
    }
    ...
}
```

编译器代码中大量使用 `+` 拼接字符串（构建 SSA 名称、LIR 文本、机器码十六进制表示）。每次拼接都分配新 `String`。

### 2.9 `Heap.allocated: Vec<usize>` 泄漏检测追踪

**代码位置**：`rust/compiler/src/vm/heap.rs:75`

```rust
pub struct Heap {
    slots: Vec<HeapSlot>,
    free: Vec<usize>,
    c_strings: Vec<std::sync::Arc<str>>,
    allocated: Vec<usize>,  // ← 记录所有分配，用于泄漏检测
}
```

`allocated` 记录每个分配槽的索引，**永远不缩容**。编译器在 VM 堆上分配的对象可达数十万，仅追踪向量就消耗 **数十 MB**。

更严重的是：**当槽位被 `dec_ref` 回收后，`allocated` 中的记录并未移除**（`heap.rs:229-239`）。这意味着：
- 已回收的槽位仍在 `allocated` 中
- `allocated` 的增长完全由分配次数决定，而非活跃对象数
- 数百万次分配 → 数百万个 `usize` 条目 → **8-16 MB** 纯追踪开销

---

## 三、导致 20 GB 的死循环级放大效应（关键发现）

### 🔴 3.1 无条件 `eprintln!` 在原生调用热路径 —— 已知 23 GB 事故的同源 bug

**代码位置**：`rust/compiler/src/vm/interp.rs:1559-1562` 和 `1746-1749`

```rust
// 位置1: do_call_native (line 1559)
if let Some((std_func_idx, needs_self)) = std_lookup {
    eprintln!(                                              // ← 无条件！每次都打印！
        "[vm] stdlib-aura: {} → Aura compiled func #{} (self={})",
        native.name, std_func_idx, needs_self
    );

// 位置2: do_call_native_args (line 1746)
if let Some((std_func_idx, needs_self)) = args_std_lookup {
    eprintln!(                                              // ← 无条件！每次都打印！
        "[vm] stdlib-aura: {} (argc={}) → Aura compiled func #{} (self={})",
        native.name, eff_argc, std_func_idx, needs_self
    );
```

**严重程度**：这是 **已知的真实 bug**，源码注释直接记录了 23 GB 事故：

> `interp.rs:13-16`（`warn_unlinked_once` 函数注释）：
> "原实现每次调用都 args.iter().map(|v| v.to_string())：当实参里含大列表时，单条日志就要构造数百 KB 字符串。实测 4 万次 list.get(i)（裸名 get 未注册 → 走本兜底）峰值内存 **23GB**、耗时超过一分钟 —— 「未链接告警」自己变成了 OOM 元凶。"

`warn_unlinked_once` 修复了**未链接函数**的告警问题（去重 + 截断），但 **`eprintln!` 在 stdlib-aura 派发路径上完全没有任何保护**。

**放大机制**：
1. Photon 编译器代码调用 `mutableListOf`、`String.split`、`Collections.set` 等标准库函数
2. 每次调用走 `do_call_native` → `find_stdlib_func` → `eprintln!`（无条件）
3. 编译器代码执行 50-100 万次标准库调用 → 50-100 万次 `eprintln!`
4. 每次 `eprintln!` 构造 format buffer + 写入 stderr → 累计 stderr 输出可达 **数 GB**
5. 如果 stderr 重定向到文件或终端缓冲区 → **20 GB 内存被 I/O 缓冲占用**

**修复**：添加条件编译或环境变量开关（`AURA_VM_TRACE_CALL` 已有此模式）。

### 🔴 3.2 `native.name.clone()` 在每次原生调用中执行 —— 隐藏的热路径分配

**代码位置**：`rust/compiler/src/vm/interp.rs:1451` 和 `1649`

```rust
// do_call_native (line 1451)
let native = self.module.natives[idx].clone();  // ← clone Vec<u8> + Option<String> + String

// do_call_native_args (line 1649)
let native = self.module.natives[idx].clone();  // ← 同上
```

**量化**：编译器执行 50 万次原生调用 × 每次 clone ~100 字节（含 String + Vec + Option）= **50 MB** 临时分配。虽然每次立即释放，但高频分配/释放导致内存碎片化，实际占用远超 50 MB。

**修复**：改为借用 `let native = &self.module.natives[idx];`。

### 🔴 3.3 `IncRef`/`DecRef`/`DropRef` 克隆栈顶 —— 每条指令都克隆

**代码位置**：`rust/compiler/src/vm/interp.rs:442-458`

```rust
Instr::DropRef => {
    if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {  // ← clone!
        self.heap.drop_ref(h);
    }
}
Instr::IncRef => {
    if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {  // ← clone!
        self.heap.inc_ref(h);
    }
}
Instr::DecRef => {
    if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {  // ← clone!
        self.heap.dec_ref(h);
    }
}
```

每次 INCREF/DECREF/DROPREF 指令都克隆栈顶的 `Value`。对于 `Value::Ref(usize)`，clone 是廉价的（16 字节），但模式本身浪费：
- 应先模式匹配引用，再取出 handle，而非先 clone 再匹配
- 如果栈顶是 `Value::List(Vec<Value>)` 或 `Value::Map(HashMap<Value, Value>)`，clone 会递归克隆整个嵌套结构

**编译器场景**：ARC 指令在寄存器分配后大量生成（每个引用变量都有对应的 INCREF/DECREF），编译器管线中可能有 **数十万次 ARC 指令**。

**修复**：
```rust
Instr::IncRef => {
    if let Value::Ref(h) = self.frames[top].stack.last().copied() {
        self.heap.inc_ref(h);
    }
}
```
（`Ref(usize)` 是 `Copy`，用 `.copied()` 避免 clone）

### 🔴 3.4 `allocated` 向量永不缩容 —— 单调增长到数百万条

**代码位置**：`rust/compiler/src/vm/heap.rs:199`

```rust
pub fn alloc(&mut self, data: HeapData) -> usize {
    ...
    self.allocated.push(h);  // ← 只追加，永不缩容
    h
}
```

**与 `dec_ref` 的交互**（`heap.rs:229-239`）：
```rust
pub fn dec_ref(&mut self, handle: usize) {
    if slot.rc == 0 {
        slot.data = None;
        self.free.push(handle);  // ← 加入 free list，但 allocated 不变！
    }
}
```

**放大机制**：
1. Photon 编译器管线分配/回收 100 万个对象
2. `allocated` 记录 100 万个 `usize`（8 字节/个 = 8 MB）
3. 但 `allocated` **不是**主要内存来源（8 MB 不足以造成 20 GB）
4. 真正的放大效应是：`allocated` 的存在意味着 `leak_report()` 会遍历所有 100 万条目
5. 如果有人在调试时调用 `leak_report()`，它会为每个条目创建 `LeakDetail` → **瞬间分配 100+ MB**

**修复**：用 `HashSet<usize>` 代替 `Vec<usize>`，在 `dec_ref` 回收时移除条目。或用 `#[cfg(feature = "leak-detection")]` 条件编译。

---

## 四、调用帧与栈的内存开销

### 4.1 `Frame` 每次调用分配两个独立 Vec

**代码位置**：`rust/compiler/src/vm/mod.rs:884-895`

```rust
pub struct Frame {
    pub func: usize,
    pub ip: usize,
    pub locals: Vec<Value>,   // ← 独立堆分配
    pub stack: Vec<Value>,    // ← 独立堆分配
    pub coroutine_id: usize,
}
```

每次 `push_frame()` 都创建两个新的 `Vec<Value>`。编译器管线调用深度可达 50-100 层，每层 locals 平均 10-20 个 Value，每个 Value 8-56 字节。

**估算**：100 帧 × (20 locals × 56B + 10 stack × 56B) = **16.8 MB** 常驻。

### 4.2 `push_frame` 诊断日志中的 `to_string()` 无界分配

**代码位置**：`rust/compiler/src/vm/mod.rs:1730-1738`

```rust
if let Some(pat) = filter {
    let fname = self.module.funcs[func_idx].name.clone();  // ← clone
    if fname.contains(pat.as_str()) {
        eprintln!(
            "[vm] args: func={} param_count={} argc={} args={:?}",
            fname,
            self.module.funcs[func_idx].param_count,
            args.len(),
            args.iter().map(|v| v.to_string()).collect::<Vec<_>>()  // ← 无界 to_string!
        );
    }
}
```

虽然受 `AURA_VM_ARGS` 环境变量保护，但一旦启用：
- 每次函数调用都 `to_string()` 所有参数
- 如果参数包含大 List/Map（编译器中间数据结构），单次调用可分配 **数百 MB**
- 这与 `warn_unlinked_once` 注释中记录的 23 GB 事故完全相同

### 4.3 `step()` 看门狗中的 `to_string()` 无界分配

**代码位置**：`rust/compiler/src/vm/interp.rs:124-131`

```rust
if let Some(fr) = self.frames.last() {
    let locs: Vec<String> = fr
        .locals
        .iter()
        .take(6)
        .map(|v| format!("{:?}", v))  // ← Debug 格式，无截断！
        .collect();
    eprintln!("[vm]   locals: [{}]", locs.join(", "));
}
```

虽然受 `AURA_VM_WATCH` 环境变量保护，但 Debug 格式对大嵌套结构会输出完整内容。

---

## 五、VM 执行循环本身的潜在无限循环

### 5.1 主执行循环无指令计数上限

**代码位置**：`rust/compiler/src/vm/mod.rs:1494-1496`

```rust
while !self.frames.is_empty() && !self.halt {
    self.step()?;
}
```

**问题**：这个循环没有指令计数上限。如果编译后的 Aura 程序有无限循环（编译器 bug 导致的死循环），VM 会永远执行下去。

**已知案例**（`interp.rs:78-85` 注释记录）：
> "某些死循环完全不调用任何 std 函数（纯 Aura 计算），因此 stderr 上不会有任何 stdlib 派发日志可看 —— 自举编译器里 `EmitBuffer`（AOT 发射）就这类：实测 180s 只有 6 次 `StringBuilder.create`，其余全是空转。"

**如果死循环中还有分配操作**（创建 List/Map/Object），内存会无限增长直到 OOM。20 GB 可能正是这种场景：编译器代码中有一个逻辑 bug 导致无限循环，且循环体内不断创建集合对象。

### 5.2 递归调用深度限制

**代码位置**：`rust/compiler/src/vm/mod.rs:1714-1718`

```rust
fn push_frame(&mut self, func_idx: usize, args: Vec<Value>) -> Result<(), VmError> {
    if self.frames.len() >= self.opts.max_call_depth {
        return Err(VmError::Runtime(format!(
            "call stack overflow (max depth {})", self.opts.max_call_depth
        )));
    }
```

`max_call_depth` 默认 4096，可以有效防止无限递归。但**无限循环**（while loop 不退出）不受此限制。

---

## 六、编译器前端（Rust）的潜在指数级扩展

### 6.1 前端是单次构建、即时释放的 —— 不构成 OOM 风险

Rust 前端的 AST/HIR/sema 表在编译完成后立即释放（子进程边界）。即使 HIR 有指数级扩展，也不会累积到 VM 堆中。

### 6.2 HIR desugar 的递归深度

HIR desugar（`hir.rs:1715-7593`）对 AST 进行递归降级。递归深度等于 AST 深度，对于正常源码是有限的。但以下模式可能导致扩展：

- **当量降级**（`when` → 嵌套 `if`）：如果当量嵌套很深，可能产生指数级 if 树
- **运算符重载展开**：`a + b + c + d` 可能被展开为多次调用
- **隐式 toString 插入**：`"hello" + x` 可能被包装为 `toString(x)`

**风险评估**：对于 50k 行正常源码，HIR 扩展应在 2-5 倍范围内。不会导致 20 GB。

---

## 七、内存占用量化总结

| 内存区域 | 估算占用 | 占比 | 根因 |
|---------|---------|------|------|
| VM 堆对象字段表（HashMap 桶） | 500-800 MB | 50-70% | `HashMap<u16, Value>` 存储对象字段 |
| VM 堆对象数据（Value 本体） | 100-200 MB | 10-20% | `Value::List/Map` 递归嵌套 |
| 调用帧（locals + stack） | 20-50 MB | 2-5% | 每帧两个独立 Vec |
| 指令克隆临时分配 | 50-100 MB | 5-10% | 每条指令 clone + pop 时 clone func_name |
| Heap.allocated 追踪 | 10-30 MB | 1-3% | 泄漏检测向量（永不缩容） |
| Rust 前端残留（AST/HIR） | 20-50 MB | 2-5% | 子进程边界未释放 |
| **20 GB 触发条件** | | | |
| ⚠ `eprintln!` 热路径（已知 23 GB 事故同源） | 数 GB → 20 GB | | `interp.rs:1559, 1746` |
| ⚠ 死循环 + 循环内分配 | 无限增长 | | `mod.rs:1494` 无指令上限 |
| ⚠ `to_string()` 对大嵌套值 | 单次数百 MB | | `mod.rs:1738`（诊断日志） |
| **合计** | **~700-1200 MB (基线) / 20+ GB (极端)** | **100%** | |

---

## 八、优化方案（按优先级排序）

### P0：立即修复（投入 < 1 天，收益 30-50% + 消除 20 GB 风险）

#### 6.1 移除热路径无条件 `eprintln!`

**文件**：`rust/compiler/src/vm/interp.rs:1559, 1746`

```rust
// 修复前：无条件打印
if let Some((std_func_idx, needs_self)) = std_lookup {
    eprintln!("[vm] stdlib-aura: {} → ...", native.name, ...);

// 修复后：受环境变量保护
if trace_call_enabled(&native.name) {
    eprintln!("[vm] stdlib-aura: {} → ...", native.name, ...);
}
```

**收益**：消除已知的 23 GB 事故同源 bug，减少数百万次 I/O 写入。

#### 6.2 `do_call_native()` 避免克隆 BytecodeNative

**文件**：`rust/compiler/src/vm/interp.rs:1451`

```rust
// 修复前
let native = self.module.natives[idx].clone();

// 修复后
let native = &self.module.natives[idx];  // 借用，避免clone
```

**收益**：减少数十万次 `Vec<u8>` 和 `String` clone。

#### 6.3 `pop()` 函数名克隆移至错误路径

**文件**：`rust/compiler/src/vm/interp.rs:1957-1966`

```rust
// 修复前
fn pop(&mut self, top: usize) -> Result<Value, VmError> {
    let func_name = self.module.funcs[self.frames[top].func].name.clone();  // 每次clone
    ...

// 修复后
fn pop(&mut self, top: usize) -> Result<Value, VmError> {
    let ip = self.frames[top].ip;
    match self.frames[top].stack.pop() {
        Some(v) => Ok(v),
        None => {
            let func_name = &self.module.funcs[self.frames[top].func].name;
            Err(VmError::Runtime(format!(...)))
        }
    }
}
```

**收益**：消除 500 万次无效 String clone，减少 ~100 MB 临时分配。

#### 6.4 `const_to_value()` 消除双重字符串分配

**文件**：`rust/compiler/src/vm/interp.rs:2313-2320`

```rust
// 修复前
Const::Str(s) => Value::str_(s.clone()),

// 修复后：Const::Str 改为 Rc<str>
Const::Str(s) => Value::Str(Rc::clone(s)),
```

**收益**：消除每次 `LoadConst` 的双重分配。

#### 6.5 `IncRef`/`DecRef`/`DropRef` 改用 `.copied()` 代替 `.cloned()`

**文件**：`rust/compiler/src/vm/interp.rs:442-458`

```rust
// 修复前
if let Some(Value::Ref(h)) = self.frames[top].stack.last().cloned() {

// 修复后（Ref(usize) 是 Copy，用 copied 避免 clone）
if let Value::Ref(h) = self.frames[top].stack.last().copied() {
```

**收益**：消除每条 ARC 指令的 clone 开销。

---

### P1：短期优化（投入 1-3 天，收益 20-30%）

#### 6.6 对象字段从 `HashMap<u16, Value>` 改为 `Vec<(u16, Value)>`

**文件**：`rust/compiler/src/vm/heap.rs:26`

```rust
// 修复前
fields: HashMap<u16, Value>,

// 修复后
fields: Vec<(u16, Value)>,
```

字段数 ≤ 64 时，`binary_search` 比哈希查找更快。查找/插入复杂度从 O(1) 平均变为 O(log N)，但对编译器工作负载（字段数通常 4-20）几乎无影响。

**空间收益**：对象字段存储从 ~2.3× 放大约降至 1.0×，节省 **60-70% 对象字段内存**。

#### 6.7 `Value::List/Map` 改为堆对象引用

```rust
// 修复前
List(Vec<Value>),       // 内联，克隆时递归拷贝
Map(HashMap<Value, Value>),  // 内联，克隆时递归拷贝

// 修复后
List(usize),  // 堆句柄，指向 HeapData::List
Map(usize),   // 堆句柄，指向 HeapData::Map
```

消除递归克隆。所有 Value 克隆变为 O(1)。

#### 6.8 `Heap.allocated` 改为按需启用

```rust
// 修复前
allocated: Vec<usize>,  // 总是记录

// 修复后
#[cfg(feature = "leak-detection")]
allocated: Vec<usize>,
```

**收益**：减少 ~20-30 MB 常驻内存。

#### 6.9 添加 VM 执行指令计数上限

**文件**：`rust/compiler/src/vm/mod.rs:1494-1496`

```rust
// 修复前
while !self.frames.is_empty() && !self.halt {
    self.step()?;
}

// 修复后
const MAX_INSTRUCTIONS: u64 = 100_000_000;  // 1 亿条指令上限
let mut instr_count = 0u64;
while !self.frames.is_empty() && !self.halt {
    instr_count += 1;
    if instr_count > MAX_INSTRUCTIONS {
        return Err(VmError::Runtime(
            format!("instruction limit exceeded ({} instructions), possible infinite loop", instr_count)
        ));
    }
    self.step()?;
}
```

**收益**：防止编译器代码中的死循环导致无限内存增长。

---

### P2：中期优化（投入 3-7 天，收益 20-30%）

#### 6.10 引入 arena allocator 管理 VM 堆

编译器管线中大量短生命周期中间对象（SSA 节点、LIR 指令、DAG 节点）按线性顺序分配和释放。Arena allocator 可以：
- 分配时 O(1)（bump pointer）
- 释放时 O(1)（drop chunk）
- 消除 HashMap 桶碎片
- 减少 40-60% 的堆内存

#### 6.11 指令分派改为 `match` 而非 `clone + match`

```rust
fn step(&mut self) -> Result<(), VmError> {
    let top = self.frames.len() - 1;
    let func = self.frames[top].func;
    let ip = self.frames[top].ip;
    let code = &self.module.funcs[func].code;
    
    match &code[ip] {  // ← 直接借用，不 clone
        Instr::LoadConst(ci) => { ... }
        Instr::Add => { ... }
        ...
    }
    self.frames[top].ip += 1;
    Ok(())
}
```

#### 6.12 所有诊断日志统一用 `trace_call_enabled` 保护

扫描 `interp.rs` 中所有无条件 `eprintln!`，统一加上环境变量开关保护。

---

### P3：长期架构改进（投入 2-4 周）

#### 6.13 分离 Photon 后端为原生 Rust 管线

当前 Photon 后端以 Aura 源码形式运行在 VM 中，所有中间对象都通过 VM 的 `HashMap<u16, Value>` 存储。如果将 SSA/LIR/DAG/RegAlloc 管线改为 Rust 原生实现，可以：
- 使用 Rust 的 `Vec<NamedStruct>` 代替 HashMap
- 使用 `Box<[Value]>` 代替 `Vec<Value>`
- 使用 arena allocator 管理中间表示
- 预期内存降低 **5-10×**

#### 6.14 引入 GC 暂停式回收替代 ARC

编译器管线中大量对象的生命周期具有明确阶段（SSA 构建完 → 释放 SSA → LIR 构建完 → 释放 LIR）。ARC 要求每个引用变更都维护计数，且对象只有在所有引用归零后才能回收。GC 暂停式回收可以在阶段边界批量释放。

---

## 九、优化效果预估

| 阶段 | 优化项 | 内存节省 | 投入 |
|------|--------|---------|------|
| **P0** | 6.1-6.5 热路径 clone/eprintln 消除 | 200-300 MB + 消除 20 GB 风险 | 1 天 |
| **P1** | 6.6-6.9 数据结构优化 + 指令上限 | 300-500 MB + 防死循环 | 1-3 天 |
| **P2** | 6.10-6.12 Arena + 指令共享 | 100-200 MB | 3-7 天 |
| **P3** | 6.13-6.14 架构重构 | 500+ MB（至 <200 MB） | 2-4 周 |
| **合计** | | **700-1000+ MB** | |

**目标**：将 50k 行源码编译的内存峰值从 >1 GB 降至 <300 MB，并消除 20 GB 风险。

---

## 十、验证方法

### 10.1 快速验证 P0 修复

```bash
# 编译前后对比（使用 /usr/bin/time）
/usr/bin/time -v photon build target.aura 2>&1 | grep "Maximum resident"

# Windows 下观察内存曲线
wmic process where "name='photon.exe'" get WorkingSetSize
```

### 10.2 死循环检测验证

```bash
# 启用看门狗（AURA_VM_WATCH=5000000 表示每 500 万条指令打印一次）
AURA_VM_WATCH=5000000 photon build target.aura

# 检查 stderr 输出：如果 watch 行持续打印且 IP 不变，说明死循环
```

### 10.3 逐步验证

1. 应用 P0 修复 → 测量内存基线
2. 应用 P1 修复 → 测量内存基线
3. 确认编译器输出正确性（`photon run` 结果一致）
4. 测量时间影响（HashMap → Vec 可能导致查找变慢）

### 10.4 回归测试

```bash
cargo test -p compiler
cargo test -p cli
```

---

## 十一、附录：关键代码位置索引

| 问题 | 文件 | 行号 |
|------|------|------|
| 对象字段 HashMap | `rust/compiler/src/vm/heap.rs` | 26 |
| Value::List/Map 内联 | `rust/compiler/src/vm/value.rs` | 34-36 |
| pop() clone func_name | `rust/compiler/src/vm/interp.rs` | 1957-1966 |
| step() clone Instr | `rust/compiler/src/vm/interp.rs` | 147 |
| do_call_native() clone | `rust/compiler/src/vm/interp.rs` | 1451 |
| **无条件 eprintln (热路径)** | **`rust/compiler/src/vm/interp.rs`** | **1559, 1746** |
| const_to_value() double alloc | `rust/compiler/src/vm/interp.rs` | 2313-2320 |
| Value::as_string() alloc | `rust/compiler/src/vm/value.rs` | 118-123 |
| bin_add() format! alloc | `rust/compiler/src/vm/interp.rs` | 2331-2335 |
| Heap.allocated tracking | `rust/compiler/src/vm/heap.rs` | 75, 199 |
| Frame locals/stack Vec | `rust/compiler/src/vm/mod.rs` | 884-895 |
| pop_n() split_off | `rust/compiler/src/vm/interp.rs` | 1969-1984 |
| Const::Str(String) | `rust/compiler/src/vm/opcode.rs` | 21 |
| IncRef/DecRef clone stack top | `rust/compiler/src/vm/interp.rs` | 442-458 |
| **主循环无指令上限** | **`rust/compiler/src/vm/mod.rs`** | **1494-1496** |
| push_frame 诊断 to_string | `rust/compiler/src/vm/mod.rs` | 1738 |
| step() watchdog to_string | `rust/compiler/src/vm/interp.rs` | 129-131 |
| warn_unlinked_once 23GB 注释 | `rust/compiler/src/vm/interp.rs` | 13-16 |
| max_call_depth=4096 | `rust/compiler/src/vm/mod.rs` | 164 |
| dec_ref 不更新 allocated | `rust/compiler/src/vm/heap.rs` | 229-239 |
