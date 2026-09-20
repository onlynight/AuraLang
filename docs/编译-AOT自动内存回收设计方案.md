# AOT 自动内存回收机制设计方案

> 日期：2026-09  
> 范围：Aura 自举编译器 AOT 模式  
> 目标：开发者无需手动管理内存，即时回收，无 GC 暂停  
> 约束：不引入 VM 依赖，纯 AOT 原生执行

---

## 一、现状诊断

### 1.1 AOT 模式内存模型

Aura 自举编译器在 AOT 模式下，所有堆对象通过 `Memory.alloc` 分配（底层 → C `malloc`），但**从不释放**：

```
Memory.alloc(n) → aura_malloc(n) → C malloc + 16B AuraMemHdr → 返回负载指针
Memory.free(addr) → aura_free(addr) → C free

现状：Memory.free 虽已声明，但 AOT emit 从不生成调用
```

C 运行时注释（`aura_std_cffi.c:87`）明确指出：
> "AOT 运行时的字符串/列表/映射/对象目前只分配不释放"

### 1.2 现有 ARC 原语未被使用

`Memory.aura` 已声明 ARC 操作（内联汇编）：

```aura
@native(asm = "lock inc") fun arcIncrement(addr: Long): Long
@native(asm = "lock dec") fun arcDecrement(addr: Long): Long
```

C 运行时也实现了 `aura_arc_increment` / `aura_arc_decrement`，期望对象头部有 `AuraArcHeader`（4B refcount）。但：

- AOT emit 生成 `aura_malloc` 时**不写入 refcount**
- `emit_call` **不生成** `arcIncrement`/`arcDecrement` 调用
- `Memory.arcIncrement` 的内联汇编声明**从未被任何 Aura 源码调用**

### 1.3 根因链

```
AOT emit 不调用 ARC 原语
  → 无引用计数插入
  → 无释放时机
  → 所有堆对象（字符串/列表/映射/闭包/对象）只分配不释放
  → 8 GiB 闸门触发 exit(70)
```

---

## 二、总体架构

### 2.1 三层回收策略

```
┌────────────────────────────────────────────────────────────────┐
│ Layer 1: 逃逸分析（编译期）                                      │
│                                                                │
│ 未逃逸对象 → 栈分配（alloca），零 ARC 开销                         │
│ 消除堆分配 + 消除引用计数开销                                      │
│                                                                │
│ 收益：函数内临时对象全部栈化，无头部分配，无 retain/release 调用     │
└────────────────────────────────────────────────────────────────┘
                           │ 逃逸
                           ▼
┌────────────────────────────────────────────────────────────────┐
│ Layer 2: ARC 自动插入（编译期插入 + 运行时执行）                    │
│                                                                │
│ 逃逸对象 → 堆分配 + 原子引用计数                                    │
│ retain/release 在每个引用转移点自动插入                             │
│ 引用归零 → 立即释放，无 GC 暂停                                    │
│                                                                │
│ 无需开发者调用 Memory.arcIncrement / Memory.arcDecrement          │
│ 编译器自动在赋值/传参/返回/字段写入/作用域退出点插入                   │
└────────────────────────────────────────────────────────────────┘
                           │ 优化
                           ▼
┌────────────────────────────────────────────────────────────────┐
│ Layer 3: ARC 优化 Pass（编译期）                                  │
│                                                                │
│ 冗余 Retain/Release 消除 + Move 消除 + 循环提升                   │
│                                                                │
│ 目标：最小化运行时 ARC 调用次数                                     │
└────────────────────────────────────────────────────────────────┘
```

### 2.2 设计原则

| 原则 | 说明 |
|------|------|
| **开发者无感知** | 不需要写 `Memory.arcIncrement`/`arcDecrement`，编译器自动插入 |
| **即时回收** | 引用归零即释放，无 mark-sweep 暂停 |
| **无 VM 依赖** | 纯 AOT 原生执行，不经过 VM 字节码层 |
| **栈优先** | 未逃逸对象优先栈分配，消除 ARC 开销 |
| **向后兼容** | `Memory.alloc`/`Memory.free` 保持不变，ARC 为增量扩展 |

---

## 三、对象内存布局

### 3.1 扩展头部设计

在现有 `AuraMemHdr`（16B，用于 malloc 大小追踪）基础上，扩展为包含引用计数的 `AuraObjHeader`：

```
┌─────────────────────────────────────────────────────────────┐
│ AuraObjHeader (16 bytes)                                     │
│   [refcount: 4] [pad: 4] [size: 8]                           │
├─────────────────────────────────────────────────────────────┤
│ payload (对象数据 / 字符串数据 / 列表数据 / 映射数据)             │  ← 用户指针
└─────────────────────────────────────────────────────────────┘
```

**与现有布局的关系**：
- 大小不变（16B 头部，与 `AuraMemHdr` 一致），向后兼容
- `refcount` 在偏移 +0，`size` 在偏移 +8（与 `AuraMemHdr.h.size` 对齐）
- `Memory.free` 无需修改（按 `-16` 偏移找到头部，从偏移 +8 读 size）

### 3.2 分配 API 分层

```
┌─────────────────────────────────────────────────────┐
│ 开发者层（Aura 源码）                                  │
│   Memory.alloc(n) → 普通分配（无 ARC，向后兼容）         │
│   Memory.free(addr) → 普通释放                         │
│   编译器内置: new Type(...) → 自动 ARC 分配             │
├─────────────────────────────────────────────────────┤
│ AOT emit 层                                            │
│   new T(args) → call @aura_arc_alloc(size)            │
│   赋值/传参/返回 → call @aura_arc_retain/release       │
├─────────────────────────────────────────────────────┤
│ C 运行时层                                             │
│   aura_arc_alloc(n) → malloc + 写 refcount=1 + size   │
│   aura_arc_retain(ptr) → atomic inc refcount          │
│   aura_arc_release(ptr) → atomic dec, free if 0       │
│   aura_malloc(n) → 现有普通分配（不变）                  │
│   aura_free(ptr) → 现有普通释放（不变）                   │
└─────────────────────────────────────────────────────┘
```

---

## 四、ARC 自动插入规则

### 4.1 所有权模型

每份引用拥有独立的引用计数。编译器根据 HIR 节点类型自动决定插入点：

| HIR 操作 | ARC 动作 | 说明 |
|---------|---------|------|
| `new T(args...)` | `aura_arc_alloc` (refcount=1) | 对象创建，调用者获得所有权 |
| `val x = obj` | `retain(obj)` + `release(old_x)` | 赋值：保留新引用，释放旧引用 |
| `x = obj2` | `release(x)` + `retain(obj2)` | 重新赋值：先释放旧值 |
| `obj.field = val` | `release(old_field)` + `retain(val)` | 字段赋值 |
| `call f(obj)` | `retain(obj)` | 传参：被调方获得引用 |
| `return obj` | `retain(obj)` | 返回：调用方获得引用 |
| 作用域退出 | `release(local)` | 释放所有管理局部 |
| 字符串字面量 | 全局常量段 | 静态驻留，无 ARC |
| 字符串拼接结果 | `retain(result)` | 动态字符串走 ARC |
| 集合 add(item) | `retain(item)` | 列表获得元素引用 |
| 集合 remove(idx) | `release(item)` | 列表释放元素引用 |

### 4.2 插入规则详解

#### 4.2.1 对象创建

```
// Aura 源码
fun f() -> Foo {
    return new Foo(42)
}

// AOT emit 生成的 LLVM IR（概念）
define %struct.Foo* @f() {
    %raw = call i8* @aura_arc_alloc(i64 48)     ; refcount=1
    %foo = bitcast i8* %raw to %struct.Foo*
    store i32 42, i32* %foo, align 4
    ; 无需 retain：refcount=1 已包含调用方的引用
    ret %struct.Foo* %foo
}
```

#### 4.2.2 变量赋值

```
// Aura 源码
fun g() -> Foo {
    var x: Foo = new Foo(1)
    x = new Foo(2)        // 赋值：释放旧 Foo(1)
    return x
}

// AOT emit 生成的 LLVM IR（概念）
define %struct.Foo* @g() {
    %x = alloca %struct.Foo*
    store %struct.Foo* null, %struct.Foo** %x

    ; x = new Foo(1)
    %raw1 = call i8* @aura_arc_alloc(i64 48)
    %foo1 = bitcast i8* %raw1 to %struct.Foo*
    store i32 1, i32* %foo1, align 4
    store %struct.Foo* %foo1, %struct.Foo** %x

    ; x = new Foo(2)  → release old, retain new
    %old = load %struct.Foo*, %struct.Foo** %x
    call void @aura_arc_release(i8* %old)      ; 释放 Foo(1) → refcount 归零 → free
    %raw2 = call i8* @aura_arc_alloc(i64 48)
    %foo2 = bitcast i8* %raw2 to %struct.Foo*
    store i32 2, i32* %foo2, align 4
    store %struct.Foo* %foo2, %struct.Foo** %x

    ; return x  → retain
    %ret = load %struct.Foo*, %struct.Foo** %x
    call void @aura_arc_retain(i8* %ret)       ; refcount: 1 → 2
    call void @aura_arc_release(i8* %ret)       ; 释放局部引用: 2 → 1
    ret %struct.Foo* %ret                       ; 调用方持有 refcount=1
}
```

#### 4.2.3 函数传参

```
// Aura 源码
fun apply(f: Foo) -> Foo {
    return f
}

fun h() -> Foo {
    val a = new Foo(1)
    return apply(a)
}

// AOT emit（概念）
define %struct.Foo* @apply(%struct.Foo* %arg.f) {
    ; 形参已持有引用（调用方 retain 了）
    ; 返回时 retain（转移所有权给调用方）
    call void @aura_arc_retain(i8* %arg.f)       ; refcount: 1 → 2
    call void @aura_arc_release(i8* %arg.f)      ; 释放形参引用: 2 → 1
    ret %struct.Foo* %arg.f                       ; 调用方持有 refcount=1
}

define %struct.Foo* @h() {
    %raw = call i8* @aura_arc_alloc(i64 48)
    %a = bitcast i8* %raw to %struct.Foo*
    call void @aura_arc_retain(i8* %a)            ; 传参前 retain: 1 → 2
    %r = call %struct.Foo* @apply(%struct.Foo* %a)  ; apply 返回时 refcount 仍为 2
    call void @aura_arc_release(i8* %a)           ; 释放局部引用: 2 → 1
    ret %struct.Foo* %r                            ; 调用方持有 refcount=1
}
```

#### 4.2.4 字段赋值

```
// Aura 源码
class Wrapper {
    var inner: Foo
}

fun w() -> Wrapper {
    val w = new Wrapper()
    w.inner = new Foo(1)     // 字段赋值
    w.inner = new Foo(2)     // 覆盖：释放 Foo(1)
    return w
}

// AOT emit（概念）
define %struct.Wrapper* @w() {
    %rawW = call i8* @aura_arc_alloc(i64 24)
    %wrp = bitcast i8* %rawW to %struct.Wrapper*
    store %struct.Foo* null, %struct.Foo** %wrp

    ; w.inner = new Foo(1)
    %raw1 = call i8* @aura_arc_alloc(i64 48)
    %foo1 = bitcast i8* %raw1 to %struct.Foo*
    store %struct.Foo* %foo1, %struct.Foo** %wrp   ; 字段初始为 null，无需 release

    ; w.inner = new Foo(2)
    %old = load %struct.Foo*, %struct.Foo** %wrp
    call void @aura_arc_release(i8* %old)           ; 释放 Foo(1) → refcount 归零 → free
    %raw2 = call i8* @aura_arc_alloc(i64 48)
    %foo2 = bitcast i8* %raw2 to %struct.Foo*
    store %struct.Foo* %foo2, %struct.Foo** %wrp

    call void @aura_arc_retain(i8* %wrp)           ; return retain
    ret %struct.Wrapper* %wrp
}
```

### 4.3 字符串类型的 ARC 处理

字符串在 AOT 模式下有两种表示：

| 类型 | 表示 | ARC 管理 |
|------|------|---------|
| 字面量 `"hello"` | 全局常量段 `@str_data.N` | ❌ 静态驻留，无需 ARC |
| 动态字符串（拼接/substring） | `i8*`（C 字符串指针） | ✅ ARC 管理 |

`String.substring` 当前通过 `Memory.alloc` + `Memory.write` 创建新字符串，需改为 ARC 分配：

```
// 现状（无 ARC）
val buf: Long = Memory.alloc((n + 1) as Long)
Memory.write(buf + i, c)
return buf          // 泄漏

// ARC 化后（编译器自动转换）
// Memory.alloc → aura_arc_alloc（编译器 emit 层拦截）
// 无需开发者修改 Aura 源码
```

### 4.4 集合类型的 ARC 处理

集合元素也是引用，增删时需要 ARC 操作：

```
// ArrayList.add(item)
// 现状：Collections.listAppend(data, item) → 无 ARC
// ARC 化后：retain(item) 后传入 listAppend（列表获得引用）

// ArrayList.remove(idx)
// 现状：无释放
// ARC 化后：release(被移除的元素)
```

---

## 五、逃逸分析

### 5.1 判定条件

对象满足以下**所有**条件则**未逃逸**，可栈分配：

1. **不返回**：不包含在 `return` 表达式中
2. **不传参**：不作为函数调用的实参
3. **不存字段**：不写入对象的字段
4. **不入集合**：不追加到 List/Map 中
5. **不跨作用域**：不在嵌套块中声明并在外层引用

### 5.2 栈分配 vs 堆分配

| 条件 | 分配方式 | ARC 开销 |
|------|---------|---------|
| 未逃逸 | `alloca`（栈分配） | **零** |
| 逃逸 | `aura_arc_alloc`（堆分配） | retain/release |

### 5.3 自举编译器场景分析

以 `String.substring` 为例：

```
fun substring(from: Int, to: Int): String {
    val buf: Long = Memory.alloc((n + 1) as Long)
    Memory.write(buf + i, c)
    Memory.write(buf + n, 0)
    return buf          ; buf 作为返回值逃逸 → 堆分配 + ARC
}
```

`substring` 的结果被传递给 `indexOf`、`replace`、`split` 等函数，**逃逸** → 堆分配 + ARC。

再以词法分析器内部的 `token` 为例：

```
fun tokenize(src: String): List<Token> {
    var i = 0
    while (i < src.length) {
        val start = i
        val c = src[i]
        while (i < src.length && !isDelimiter(c)) { i = i + 1 }
        val len = i - start
        val token = new Token(src.substring(start, i), c)  ; token 不入集合，未逃逸 → 栈分配
        ; 但 token 被加入 result 列表 → 逃逸 → 堆分配 + ARC
        result.add(token)
    }
    return result
}
```

逃逸分析正确识别：`token` 被 `result.add` 引用 → 逃逸 → 堆分配 + ARC。

---

## 六、ARC 优化 Pass

### 6.1 Pass 管线

在 LLVM IR 生成后、优化前运行：

```
LLVM IR
    │
    ▼
┌──────────────────────────────────────────────────┐
│ ARC Opt Pass 1: 冗余消除                            │
│   • 连续 retain+release → 删除两者                  │
│   • 连续 release+retain → 删除两者                  │
│   • 连续 retain+retain → 保留一个                   │
│   • 连续 release+release → 保留一个                 │
├──────────────────────────────────────────────────┤
│ ARC Opt Pass 2: Move 消除                           │
│   • 赋值一次 + 释放一次 → 跳过两者                   │
│   • 函数传参后不再使用 → 跳过 retain                 │
├──────────────────────────────────────────────────┤
│ ARC Opt Pass 3: 循环提升                            │
│   • 循环内不变量的 retain/release → 提到循环外        │
│   • 循环内不变量的 release → 移到循环后               │
├──────────────────────────────────────────────────┤
│ ARC Opt Pass 4: 作用域合并                           │
│   • 嵌套作用域中的冗余 retain → 合并                  │
└──────────────────────────────────────────────────┘
    │
    ▼
优化后 IR → LLVM -O
```

### 6.2 冗余消除示例

```
// 优化前
call void @aura_arc_retain(i8* %x)
call void @aura_arc_release(i8* %x)

// 优化后
; （两者抵消，全部删除）
```

### 6.3 循环提升示例

```
// 优化前
loop:
    call void @aura_arc_retain(i8* %invariant)
    ; ... 循环体 ...
    call void @aura_arc_release(i8* %invariant)
    br label %loop

// 优化后
call void @aura_arc_retain(i8* %invariant)
loop:
    ; ... 循环体 ...
    br label %loop
loop_end:
    call void @aura_arc_release(i8* %invariant)
```

---

## 七、C 运行时实现

### 7.1 新函数（增量扩展，不影响现有功能）

```c
// 扩展对象头部（16B，与 AuraMemHdr 大小一致）
typedef struct {
    volatile int32_t refcount;   // +0
    int32_t _pad0;               // +4
    int64_t size;                // +8
    int32_t _pad1;               // +12
} AuraObjHeader;                 // 共 16 字节
```

**三个核心 API**：

```c
// ARC 分配：分配 + 初始化 refcount=1
void *aura_arc_alloc(int64_t n);

// ARC 保留：引用计数 +1（原子）
void aura_arc_retain(void *ptr);

// ARC 释放：引用计数 -1，归零时释放（原子）
void aura_arc_release(void *ptr);
```

### 7.2 实现要点

```c
void *aura_arc_alloc(int64_t n) {
    if (n < 0) n = 0;
    // 复用现有内存上限检查
    if (g_aura_mem_limit > 0 && g_aura_mem_used + n > g_aura_mem_limit) {
        aura_mem_oom(n);
    }
    AuraObjHeader *h = (AuraObjHeader *)malloc(sizeof(AuraObjHeader) + (size_t)n);
    if (!h) aura_mem_oom(n);
    h->refcount = 1;
    h->size = n;
    g_aura_mem_used += n;
    return (void *)((char *)h + sizeof(AuraObjHeader));
}

void aura_arc_retain(void *ptr) {
    if (!ptr) return;
    AuraObjHeader *h = (AuraObjHeader *)((char *)ptr - sizeof(AuraObjHeader));
    __sync_fetch_and_add(&h->refcount, 1);
}

void aura_arc_release(void *ptr) {
    if (!ptr) return;
    AuraObjHeader *h = (AuraObjHeader *)((char *)ptr - sizeof(AuraObjHeader));
    int32_t old = __sync_sub_and_fetch(&h->refcount, 1);
    if (old <= 0) {
        g_aura_mem_used -= h->size;
        free((void *)h);
    }
}
```

### 7.3 与现有 AuraMemHdr 的兼容

| 维度 | AuraMemHdr（现有） | AuraObjHeader（新增） |
|------|-------------------|---------------------|
| 大小 | 16B | 16B（兼容） |
| 偏移 +0 | `size` (8B) | `refcount` (4B) + pad (4B) |
| 偏移 +8 | `pad` (8B) | `size` (8B) |
| 用途 | malloc/free 大小追踪 | ARC 计数 + 大小追踪 |

`aura_free` 无需修改：它从 `ptr - 16` 读取 `size`（偏移 +8），与 `AuraObjHeader` 布局一致。

---

## 八、与现有 Aura 源码的兼容性

### 8.1 Memory.aura 无需修改

`Memory.alloc`/`Memory.free` 保持现有语义：
- `Memory.alloc(n)` → 普通分配（无 ARC），用于底层内存操作
- `Memory.free(addr)` → 普通释放

ARC 分配由编译器在 `new Type(...)` 处自动触发，不经过 `Memory.alloc`。

### 8.2 已有 ARC 声明的整合

`Memory.aura` 中已有的 ARC 声明：

```aura
@native(asm = "lock inc") fun arcIncrement(addr: Long): Long
@native(asm = "lock dec") fun arcDecrement(addr: Long): Long
```

这些声明**保留但标记为内部使用**：编译器 emit 层生成的 `aura_arc_retain`/`aura_arc_release` 是独立的 C 函数，不走内联汇编路径。内联汇编版本作为低层原语保留，供手动内存管理场景使用。

### 8.3 集合操作（Collections）的 ARC 集成

`Collections.listAppend` / `listGet` / `listRemove` 等函数在 C 运行时中实现。ARC 集成方式：

| 函数 | ARC 行为 |
|------|---------|
| `listAppend(list, item)` | 调用方负责 retain(item)，列表持有引用 |
| `listGet(list, idx)` | 返回引用，调用方不增加 retain（读取） |
| `listRemove(list, idx)` | 返回引用，列表释放引用 |
| `listSet(list, idx, value)` | 调用方负责 retain(value) + release(old_value) |

---

## 九、性能预期

### 9.1 内存行为

| 场景 | 现有（无回收） | 新增（ARC + 逃逸分析） | 改善 |
|------|--------------|----------------------|------|
| 循环内创建临时对象 | 每次迭代泄漏 | 栈分配，零泄漏 | **完全消除** |
| 函数间传递对象 | 泄漏 | ARC 自动回收 | **完全消除** |
| 深层对象图 | 泄漏 | 逐引用释放 | **完全消除** |
| 长生命周期缓存 | 泄漏（但合理） | 保持存活（refcount > 0） | 无差异 |

### 9.2 性能开销

| 维度 | 开销 | 说明 |
|------|------|------|
| 逃逸对象 retain | ~1ns/次 | 内存对齐的原子加法 |
| 逃逸对象 release | ~1-2ns/次 | 原子减 + 条件 free |
| 栈分配对象 | **零开销** | alloca，无 ARC 操作 |
| 头部大小 | +16B/对象 | refcount + pad + size |
| 编译时间 | +5-10% | ARC 插入 + 优化 Pass |

### 9.3 自举编译器收益预估

自举编译器编译自身时（约 300 KB 源码），内存峰值从 **~500 MB** 降至 **~80 MB**：

| 阶段 | 现有峰值 | ARC 后峰值 | 节省 |
|------|---------|-----------|------|
| 词法分析 | ~50 MB | ~10 MB | 80% |
| 语法分析 | ~80 MB | ~15 MB | 81% |
| 语义分析 | ~100 MB | ~20 MB | 80% |
| HIR 生成 | ~120 MB | ~25 MB | 79% |
| LLVM IR emit | ~150 MB | ~10 MB | 93% |
| 链接/优化 | ~80 MB | ~10 MB | 88% |
| **总计** | **~500 MB** | **~80 MB** | **84%** |

---

## 十、实施路径

### Phase 1: C 运行时 ARC 原语（Week 1）

- [ ] 定义 `AuraObjHeader` 结构体
- [ ] 实现 `aura_arc_alloc` / `aura_arc_retain` / `aura_arc_release`
- [ ] 在 `aura_std_cffi.h` 中声明
- [ ] 验证：分配 → retain → release → 内存回收（ASan 验证）

### Phase 2: AOT emit ARC 插入（Week 2-3）

- [ ] 修改 emit 的 `new` 路径：`aura_malloc` → `aura_arc_alloc`
- [ ] 在赋值点插入 retain/release
- [ ] 在函数调用传参点插入 retain
- [ ] 在 return 点插入 retain
- [ ] 在字段赋值点插入 release+retain
- [ ] 在作用域退出时插入局部变量 release
- [ ] 验证：引用图正确性

### Phase 3: 逃逸分析（Week 4-5）

- [ ] 实现逃逸分析 Pass（HIR 级）
- [ ] 未逃逸对象使用 alloca
- [ ] 验证：栈分配对象无 ARC 开销

### Phase 4: ARC 优化 Pass（Week 6-7）

- [ ] 实现冗余消除 Pass
- [ ] 实现 Move 消除 Pass
- [ ] 实现循环提升 Pass
- [ ] 集成到 LLVM IR 后处理管线
- [ ] 性能基准测试

### Phase 5: 自举验证（Week 8-9）

- [ ] 用 Rust 编译器编译修改后的 Aura 编译器源码
- [ ] 用修改后的 Aura 编译器 AOT 编译自身
- [ ] 内存泄漏基准测试（ASan + 长跑）
- [ ] 性能基准测试（vs VM ARC / vs 无 ARC）

### Phase 6: 文档与收尾（Week 10）

- [ ] 文档更新
- [ ] 全量回归测试
- [ ] 标准库适配（Collections ARC 集成）

---

## 十一、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 循环引用导致内存不回收 | 循环引用的对象永远不释放 | 文档标注限制；后续可扩展 Weak 引用打破循环 |
| ARC 开销影响性能 | 每个引用转移 ~1-2ns | 逃逸分析 + 栈分配消除大部分；优化 Pass 消除冗余 |
| 与现有 malloc 内存不兼容 | 混用导致 double-free 或 UAF | 所有 AOT 对象统一走 `aura_arc_alloc`；C 运行时内部使用不受影响 |
| 优化 Pass 引入 bug | 错误消除必要的 retain/release | 分阶段实施，每步验证；ASan 回归测试 |
| 闭包/高阶函数的 ARC 复杂性 | 闭包捕获变量的引用计数 | 闭包环境作为整体 ARC 管理 |
| 自举循环依赖 | 修改编译器后无法编译自身 | Phase 5 先验证 Rust 编译通过，再验证自举 |

---

## 附录 A: 关键 LLVM IR 示例

### A.1 对象创建与返回值

```llvm
; fun f() -> Foo
define %struct.Foo* @f() {
entry:
    %alloc = call i8* @aura_arc_alloc(i64 48)
    %foo = bitcast i8* %alloc to %struct.Foo*
    store i32 0, i32* %foo, align 4
    ret %struct.Foo* %foo
}
```

### A.2 函数间传递

```llvm
; fun apply(f: Foo) -> Foo
define %struct.Foo* @apply(i8* %arg.f) {
entry:
    ; 形参已持有引用
    call void @aura_arc_retain(i8* %arg.f)
    ret i8* %arg.f
}

; fun h() -> Foo
define %struct.Foo* @h() {
entry:
    %alloc = call i8* @aura_arc_alloc(i64 48)
    call void @aura_arc_retain(i8* %alloc)
    %r = call i8* @apply(i8* %alloc)
    call void @aura_arc_release(i8* %alloc)
    ret i8* %r
}
```

### A.3 字段赋值

```llvm
; obj.inner = new_val
%old = load i8*, i8** %field_addr
call void @aura_arc_release(i8* %old)
call void @aura_arc_retain(i8* %new_val)
store i8* %new_val, i8** %field_addr
```

---

## 附录 B: 与 VM ARC 的对应关系

| VM ARC | AOT ARC（本方案） | 位置 |
|--------|------------------|------|
| `HeapSlot.rc`（Rust） | `AuraObjHeader.refcount`（C） | C 运行时 |
| `Heap::inc_ref/dec_ref`（Rust） | `aura_arc_retain/release`（C） | C 运行时 |
| `MirInstr::Retain`（字节码） | `call @aura_arc_retain`（LLVM IR） | AOT emit |
| `MirInstr::Release`（字节码） | `call @aura_arc_release`（LLVM IR） | AOT emit |
| `escape_analysis`（MIR 级） | `escape_analysis`（HIR 级） | 编译器 |
| `optimize_arc`（MIR 级） | `arc_optimize`（LLVM IR 级） | 编译器 |
| VM 堆固定（无栈分配） | 逃逸分析 → 栈分配 | AOT emit |
| `Memory.arcIncrement`（内联汇编） | 保留，内部使用 | Memory.aura |

---

## 附录 C: 诊断 API

### C.1 运行时诊断

C 运行时提供以下诊断函数：

```c
// 当前存活分配字节数
int64_t aura_mem_used_bytes(void);

// 当前生效的内存上限字节数
int64_t aura_mem_limit_bytes(void);

// ARC 统计：当前存活 ARC 对象数
int64_t aura_arc_live_count(void);

// ARC 统计：累计释放对象数
int64_t aura_arc_total_freed(void);
```

### C.2 环境变量

```
AURA_MEM_LIMIT_MB   — 内存上限（MiB），默认 8192
AURA_ARC_ENABLED    — ARC 开关（1=启用，0=禁用，默认 1）
AURA_ARC_DEBUG      — ARC 调试日志（1=打印每次 retain/release）
```
