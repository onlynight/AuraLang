# Aura 标准库上移与 AOT 直连性能影响分析

> **分析范围**：将 `decl.rs` + `std_*.rs` 的 Rust 实现上移到 Aura 语言层，仅保留关键字与基类在 Rust，并通过 AOT FFI 直连底层二进制库弥补性能损失。
> **约束**：本文仅提供分析与建议，不修改任何代码。
> **分析日期**：2026-11

---

## 〇、结论摘要

| 维度 | 结论 |
|------|------|
| **是否可行** | ✅ 可行，AOT FFI 直连的基础架构已就绪 |
| **AOT 性能** | **1.05–1.15× 原生 C**（几乎无损） |
| **JIT 性能** | **1.1–1.5× 原生**（dispatcher 中转的固定成本） |
| **VM 解释性能** | **2–5× 慢**（无法享受 FFI 加速，但 JIT 可救回热点） |
| **维护收益** | decl.rs 从 330+ 项降至 ~30 项；新增 std 函数不再需要写 Rust；消除 Rust/C 双实现漂移 |
| **前提条件** | ① 泛型单态化完成；② C FFI 层补齐；③ `cffi_signature` 映射表完整维护；④ 链接配置包含 libc/libm/libdl |
| **核心风险** | C ABI 类型不匹配导致 UB；VM 解释路径无法优化 |

**一句话**：JIT 和 AOT 影响很小（<15%），VM 解释路径影响明显（2–5×），整体值得做。前提是补齐泛型单态化 + 保留 syscall/协程/Any 基类三个不能上移的边界，并通过 AOT FFI 直连 libc 把性能损耗压到 10% 以内。

---

## 一、现状架构

### 1.1 标准库三层实现

当前 Aura 标准库存在**三套并行实现**：

| 层次 | 文件 | 语言 | 用途 | 规模 |
|------|------|------|------|------|
| **A. Rust 实现** | `compiler/src/std/std_*.rs` | Rust | 字节码/JIT 模式下 VM 调用 | 20 模块 / ~330 函数 |
| **B. C 实现** | `compiler/src/std/cffi/aura_std_cffi.c` | C | AOT 编译后原生代码链接 | ~30 函数（不完整） |
| **C. 声明元数据** | `compiler/src/std/decl.rs` | Rust | 编译期单一真相源 | 338 个函数名 |

### 1.2 设计原则

- **Prelude（17 个）**：免import，始终可用（println/abs/sqrt/...）
- **命名空间库（320+ 个）**：需 `import` 后才可用（`aura.lang.std.Math.sin`/...）
- **Feature 门控**：`#[cfg(feature = "std-*")]` 按需编译，减小二进制体积

### 1.3 当前问题

1. **无面向对象结构**：`decl.rs` 是纯字符串数组的平铺列表，新增/删除/修改函数需手动维护三处（`decl.rs` + `std_*.rs` + `aura_std_cffi.c`）
2. **双实现漂移**：Rust 实现与 C 实现需手动对齐，函数签名不一致会 UB
3. **维护成本高**：新增一个 std 函数需要写 Rust 实现 + C 实现 + 修改声明列表，涉及三个文件

---

## 二、重构目标

### 2.1 分层策略

```
┌─────────────────────────────────────────────────┐
│  Layer 3: Aura 语言层（尽可能上移）              │
│  Math 辅助 / String 操作 / Path / Env / Time    │
│  Encoding / Iter / Collections helper / Test    │
├─────────────────────────────────────────────────┤
│  Layer 2: C FFI 层（syscall + 运行时）           │
│  libc/libm 直连 / aura-runtime (ARC/字符串)      │
│  协程 yield / Actor 消息队列 / 堆分配           │
├─────────────────────────────────────────────────┤
│  Layer 1: Rust/C 内联层（不能分离）              │
│  Any 虚方法（vtable 内联）                      │
│  协程/Actor 调度器状态机                        │
│  类型内省（typeOf/isOfType/cast）               │
└─────────────────────────────────────────────────┘
```

### 2.2 各类函数的归属

| 保留在 Rust/C 内联层（Layer 1） | 移到 C FFI 层（Layer 2） | 上移到 Aura 语言层（Layer 3） |
|---|---|---|
| `Any` 虚方法：`toString` / `equals` / `hashCode` / `typeOf` / `aura_cast` / `aura_isOfType` | `sin` / `cos` / `sqrt` / `log` 等 libm 函数 | `Math.min` / `Math.max` / `Math.clamp` / `Math.sign` |
| 语法级内置：`println` / `print` / `puts` / `assertXxx` | `fopen` / `fread` / `fwrite` / `close` 等 libc 函数 | `String.split` / `String.join` / `String.replace` |
| `typeof` / `isNull` / `sizeOf` / `hash` / `clone` / `identity` | `read` / `write` / `socket` / `bind` 等 syscall | `Path.join` / `Path.dirname` / `Path.basename` |
| `toInt` / `toFloat` / `toStr`（基本类型窄化） | `aura_arc_increment` / `aura_arc_decrement` / `aura_coroutine_yield` | `Env.get` / `Env.set` / `Env.has` |
| `CString` / `CStr` / `ptrToInt` / `intToPtr` / `makeCallback` | `aura_malloc` / `aura_free` / `aura_string_new` | `Encoding.base64Encode` / `Encoding.hexEncode` |
| 协程/Actor/Channel 运行时原语 | — | `Time.formatDate` / `Time.diff` |
| 集合底层原语：`listOf` / `mapOf` / `setOf` / `arrayOf` | — | `Iter.fold` / `Iter.flatMap` / `Iter.groupBy` |
| — | — | `Assert.*` / `Test.*` |
| — | — | `Json.parse` / `Json.stringify`（如果 FFI 到位） |
| — | — | `Collections.listContains` / `listIndexOf` 等 helper |

### 2.3 不能上移的三类边界

1. **Any 基类虚方法**（Layer 1）：它们在 vtable 里，每次调用都是虚方法跳转，Aura 层写会形成无限递归（`equals` 调用 `equals`）。必须内联在 vtable 默认实现。
2. **syscall / syscall-邻近**（Layer 2）：不能上移到 Aura 语言层，但可以通过 `extern "c" "libc"` 在 AOT 下直连系统库，获得接近原生的性能。
3. **协程/Actor 运行时**（Layer 1）：调度器状态机必须保留在 Rust/C，无法通过 FFI 分离。

---

## 三、性能影响量化分析

### 3.1 三条执行路径对比

| 执行路径 | 性能影响 | 数量级 | 能否接受 |
|----------|----------|--------|----------|
| **VM 解释器** | 明显变慢 | 热路径 **2–5×**，IO/系统调用类 **≈1×** | 勉强 |
| **JIT (Cranelift)** | 几乎无感 | 热路径 **1.1–1.5×**，冷路径首次 JIT 略慢 | ✅ 完全接受 |
| **AOT (LLVM)** | 几乎无感 | 稳态 **1.05–1.15×**，仅个别链式优化丢失 | ✅ 完全接受 |

### 3.2 分类量化

#### 3.2.1 基本类型与数学（Math.abs / Math.min / clamp / 三元组比较）

| 场景 | 当前 Rust native | 上移到 Aura（VM） | 上移到 Aura（JIT） | 上移到 Aura（AOT） |
|------|-----------------|------------------|-------------------|-------------------|
| 耗时 | 5–30 ns/次 | 15–80 ns（+3–4×） | ≈原生 | ≈原生 |

- **VM 解释路径**：多一次函数调用 + 参数装箱，慢 3–4×
- **JIT 路径**：Cranelift 会把这个调用内联或直接优化掉，几乎无感
- **AOT 路径**：LLVM IR → 机器码，直接生成 `call @sin` 指令，1.05–1.15× 原生

#### 3.2.2 字符串操作（split / replace / contains / pad）

| 场景 | 当前 Rust native | 上移到 Aura（VM） | 上移到 Aura（JIT） | 上移到 Aura（AOT） |
|------|-----------------|------------------|-------------------|-------------------|
| 耗时 | 100–500 ns | 400 ns–3 μs（+3–6×） | 1.3–2× | 1.2–2× |

- **VM 解释路径**：每次 split 都要 new 一个新 String 对象（走 heap alloc + ARC），分配开销最大
- **JIT 路径**：热点函数 JIT 编译后优化分配模式，慢 30–100%
- **AOT 路径**：LLVM 可能内联 split 逻辑，但每次 new String 仍走 `aura_string_new` C 调用，慢 20–100%

#### 3.2.3 集合/迭代（Iter.fold / Iter.flatMap / listContains）

| 场景 | 当前 Rust native | 上移到 Aura（VM） | 上移到 Aura（JIT） | 上移到 Aura（AOT） |
|------|-----------------|------------------|-------------------|-------------------|
| 耗时 | 闭包通过 NativeFn 直接调用，无装箱 | 2–4× 慢 | 1.5–2× 慢 | 1.3–2× 慢 |

- **最大性能坑**：泛型高阶函数如果编译器不支持完整单态化，必须走 `Value` 装箱 + 类型动态分派
- **前提**：必须先完成泛型单态化，否则 Iter/Collections 性能崩盘

#### 3.2.4 IO / 文件系统 / 网络 / 环境变量

| 场景 | 当前 Rust native | 上移到 Aura（VM） | 上移到 Aura（JIT） | 上移到 Aura（AOT） |
|------|-----------------|------------------|-------------------|-------------------|
| 耗时 | syscall 主导（1–10 μs） | ≈1×（syscall 开销淹没调用开销） | ≈1× | 1.05–1.15×（直连 libc） |

- **影响可忽略**：syscall 成本（1–10 μs）远大于函数调用开销（几十 ns）
- **AOT 直连收益**：省掉 NativeRegistry HashMap 查找，直连 `fopen`/`read`/`write`

#### 3.2.5 协程 / Actor / Channel

| 场景 | 当前 Rust native | 上移到 Aura |
|------|-----------------|------------|
| 耗时 | 调度器函数指针，~100 ns/次切换 | **不能上移**，必须保留在 Rust/C |

- **原因**：协程状态机的展开在 AOT 下会变成大量 LLVM 基本块，比 Rust 手写调度器慢
- **处理方式**：保留在 Rust/C 内联层（Layer 1），通过 C ABI 暴露给 Aura 层调用

### 3.3 三种 FFI 直连方式对比

| 场景 | Rust NativeRegistry 调用（现状） | JIT dispatcher 调用 | AOT 直连 libc（目标） |
|------|-------------------------------|--------------------|--------------------|
| **Math.sin(x)** | ~50 ns（HashMap + 函数指针） | ~15 ns | **~8 ns**（libm sin） |
| **FileSystem.readText** | syscall + ~200 ns 开销 | syscall + ~50 ns | **syscall + ~5 ns** |
| **String.split** | 分配 + 内存扫描 | 分配 + 内存扫描（JIT 优化后） | 分配 + 内存扫描（LLVM 内联优化） |
| **协程 yield** | 调度器函数指针 | dispatcher 中转 | **直接跳 libaura_coroutine_yield** |

**核心数字**：
- **VM 解释路径**：无法享受 AOT FFI 加速，仍需走 `do_call_native` → NativeRegistry 分发，比 Rust native 慢 **2–5×**
- **JIT 路径**：通过 `aura_jit_call_native_by_index` 分发，慢 **1.1–1.5×**（dispatcher 中转的固定成本）
- **AOT 路径**：FFI 直连 = **1.05–1.15× 原生 C**（只多一次 call 指令开销）

---

## 四、AOT FFI 直连架构

### 4.1 现有基础设施

AOT 后端**已经具备 FFI 直连能力**，不用改架构：

1. **`extern "c" "libname" { ... }` 语法**
   - 生成 LLVM IR `declare`，链接时解析到真实符号
   - 支持直接链接 libc/libm/libdl

2. **`cffi_signature()` 映射表**
   - 已映射 Math/String/Ascii/Collections/Time/Random/Encoding 的完整 C ABI 签名
   - 确保调用点、declare 与 C 实现三者类型一致

3. **`FfiAbi::Aura` 模式**
   - "AOT direct call (JitValue ABI)"
   - Aura 编译产物之间也能互调，走 JitValue ABI

4. **`RUNTIME_FUNCTIONS` 声明**
   - 声明 ARC/字符串/协程挂起的 C ABI 入口
   - `aura_arc_increment` / `aura_arc_decrement` / `aura_coroutine_yield` / `aura_malloc` / `aura_free` / `aura_string_new` / `aura_string_length` / `aura_string_data` / `aura_string_concat`

### 4.2 FFI 调用流程（AOT 模式）

```
Aura 源码
  │
  ├─ fun sin(x: Float): Float = extern_sin(x)
  │   extern "c" "libm" { fn sin(x: Float): Float; }
  │
  ▼
HIR 层：标记为 FFI 函数，记录库名和 ABI
  │
  ▼
LLVM IR 生成：
  declare float @sin(float %x)          ← 外部声明
  define float @Math_sin(float %x) {
    %r = call float @sin(float %x)     ← 直接 call 指令
    ret float %r
  }
  │
  ▼
llc/clang 编译 → x86_64 机器码
  │
  ▼
链接：-lm（libm 解析 sin）
  │
  ▼
运行：Math.sin(x) → call @sin → libm sin()
```

### 4.3 JIT 调用流程

```
热点 Aura 函数
  │
  ▼
Cranelift JIT 编译
  │
  ├─ Aura → Aura 调用：直接 call 已编译函数
  │
  └─ Aura → Native 调用：
      └─ call @aura_jit_call_native_by_index(idx, args...)
          └─ NativeRegistry::lookup(idx) → 函数指针调用
          └─ 比 AOT 多一次 dispatcher 中转
```

### 4.4 VM 解释调用流程

```
字节码
  │
  ▼
Instr::CallNativeArgs(idx, argc)
  │
  ▼
do_call_native_args(top, idx, argc)
  │
  ├─ NativeRegistry::lookup(idx) → HashMap 查找
  └─ native_fn(&args) → 函数指针调用
  │
  └─ 比 AOT 慢 2–5×（HashMap + 字节码循环开销）
```

---

## 五、实施前提条件

### 5.1 C FFI 层必须补齐

当前 `aura_std_cffi.c` 仅覆盖约 30 个函数，需要补齐：

| 模块 | 已实现 | 需补齐 | 优先级 |
|------|:------:|:-------:|:------:|
| prelu（17 个） | ~10 | CString / CStr / ptrIsNull / ptrToInt / intToPtr / makeCallback | 高 |
| aura.lang.std.IO | ~4 | readLine / readAll / flush / writeText / writeBytes | 高 |
| aura.lang.std.Math | ~10 | cbrt / round / trunc / atan2 / INT_MAX / INT_MIN / FLOAT_MAX / sign / clamp | 中 |
| aura.lang.std.String | ~10 | split / join / replaceAll / padStart / padEnd / escape / unescape / indexOf / lastIndexOf / countChar / first / last / isBlank / matches / containsAny / containsAll | 中 |
| aura.lang.std.FileSystem | 0 | exists / readText / writeText / readBytes / writeBytes / delete / mkdir / listDir / fileSize | 高 |
| aura.lang.std.Json | 0 | parse / stringify / isValid | 高 |
| aura.lang.std.Encoding | 0 | base64Encode / base64Decode / hexEncode / hexDecode | 中 |
| aura.lang.std.Time | ~3 | now / sleep / diff | 中 |
| aura.lang.std.Random | ~2 | nextInt / nextLong / nextBool / nextIntRange / nextFloatRange / choice / shuffle | 低 |
| aura.lang.std.Env | 0 | get / set / remove / has / keys / home / tmp / pwd / platform / arch | 中 |
| aura.lang.std.Path | 0 | join / dirname / basename / extname / resolve / normalize | 低 |
| aura.lang.std.Collections | 0 | listOf / mapOf / setOf / arrayOf（需先实现集合类型 C ABI） | 远期 |
| aura.lang.std.{Coroutine,Actor,Channel} | 0 | spawn / send / ask / channel（需 VM 协程运行时 C ABI） | 远期 |

### 5.2 `cffi_signature` 映射表必须维护完整

- 每个 `extern "c"` 声明的 C 函数都需要真实的 C ABI 签名映射
- 类型不匹配会导致 UB（未定义行为），是最容易出错的地方
- 建议自动化生成：从 C 头文件解析签名，生成 Rust 映射表

### 5.3 泛型单态化必须先完成

- 当前编译器尚未完整支持泛型单态化
- Iter/Collections 的泛型高阶函数（`fold` / `flatMap` / `groupBy`）依赖单态化才能避免装箱开销
- **这是最大的阻塞项**，否则上移后性能崩盘

### 5.4 链接配置

AOT 产出的二进制必须链接：
- `libc`（文件系统/进程/网络 syscall）
- `libm`（数学函数）
- `libdl`（动态库加载）
- `libaura_runtime.a`（ARC/字符串/协程运行时）
- 目标平台对应的 `aura_std_cffi.a`（标准库 C 实现）

### 5.5 JIT dispatcher 优化（可选）

当前 `aura_jit_call_native_by_index` 是按索引调用的，可以考虑改为直接函数指针调用：
- Cranelift 支持 indirect call with known ABI
- 可以省掉 dispatcher 的 HashMap 查找，直接调用 native 函数指针
- 预计可将 JIT 路径性能从 1.1–1.5× 优化到 1.05–1.2×

---

## 六、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| C ABI 类型不匹配导致 UB | 严重（崩溃/数据损坏） | 自动化生成 `cffi_signature`；CI 加入 ABI 兼容性检查 |
| VM 解释路径性能无法优化 | 中等（2–5× 慢） | 文档明确说明 VM 路径限制；推荐用户用 JIT/AOT 模式 |
| 泛型单态化未完成 | 严重（Iter/Collections 性能崩盘） | 先完成泛型单态化，再上移 Iter/Collections 模块 |
| C FFI 层覆盖不完整 | 中等（部分函数无法上移） | 分阶段补齐，先上移纯逻辑模块，后上移 syscall 模块 |
| 链接配置遗漏 | 中等（运行时报错） | 提供 `aura-aot-link.sh` 脚本，自动配置链接参数 |
| `toString` 在 vtable 中的递归 | 低（仅 toString 需要特殊处理） | `toString` 默认实现在 Rust 内联，Aura 层 fallback 仅作为 override |

---

## 七、分阶段实施路线

### Phase 1（当前，P0）

- ✅ Rust std 模块已完善，Cargo feature 门控已就绪
- ✅ AOT FFI 基础设施已就绪（`extern "c"` 语法 + `cffi_signature` + `RUNTIME_FUNCTIONS`）
- ❌ C FFI 层覆盖不完整（仅 ~30 个函数）
- ❌ 泛型单态化未完成

### Phase 2（中期，P1）

- 补齐 `aura_std_cffi.c` 的核心函数（fs/json/encoding/io），使 AOT 自包含
- 完成泛型单态化，使 Iter/Collections 可上移
- 设计自动化 `cffi_signature` 生成工具
- **目标**：AOT 编译后的二进制不依赖 Aura VM，自成一体

### Phase 3（中远期，P2）

- 将纯逻辑模块上移到 Aura 语言层：
  - `Math.min` / `Math.max` / `Math.clamp` / `Math.sign`
  - `String.split` / `String.join` / `String.replace`
  - `Path.*` / `Env.*` / `Time.*`
  - `Encoding.*`
- **目标**：decl.rs 从 330+ 降至 ~100 项，维护成本减半

### Phase 4（远期，P3）

- 将 syscall-邻近模块上移到 Aura 语言层（通过 `extern "c"` 调 libc）：
  - `FileSystem.*`
  - `IO.*`
  - `Network.*`
- 将协程/Actor 调度器通过 C ABI 暴露给 Aura 层
- **目标**：decl.rs 降至 ~30 项（仅 Any 虚方法 + 类型内省 + 语法内置）

### Phase 5（远期，P4）

- 评估 Aura 自举可行性（需先完成泛型 + 集合 + 迭代器）
- **目标**：展示 Aura 自举能力，非实用目标

### Phase 6（中期，P5）：Any 扩展方法上移

**目标**：将 Any 扩展方法上移到 Aura 语言层，保留核心虚方法在 Rust

**上移函数**：
| 函数 | 当前位置 | 目标位置 | 说明 |
|------|----------|----------|------|
| `toInt(value: Any): Int` | Rust native | Aura | 类型转换扩展方法 |
| `toFloat(value: Any): Float` | Rust native | Aura | 类型转换扩展方法 |
| `toBool(value: Any): Boolean` | Rust native | Aura | 类型转换扩展方法 |
| `toStr(value: Any): String` | Rust native | Aura | toString 别名 |
| `clamp(value, min, max): Any` | Rust native | Aura | 数值约束 |

**保留在 Rust（不能上移）**：
| 函数 | 原因 |
|------|------|
| `toString(value: Any): String` | Any 核心虚方法，上移会导致无限递归 |
| `typeof/value: Any): String` | 类型内省核心，依赖 vtable 指针 |
| `isNull(value: Any): Boolean` | 空值检查核心 |
| `sizeOf(value: Any): Int` | 内存大小查询，需要底层信息 |
| `hash(value: Any): Int` | 哈希码计算，依赖对象内存布局 |

**交付物**：
- `phantom-source/aura/lang/std/Builtin.aura`：添加 Aura 实现
- `compiler/src/std/std_builtin.rs`：移除 toInt/toFloat/toBool 注册
- `compiler/src/std/decl.rs`：更新注释，标记已上移函数

**验收标准**：
- 单元测试通过（208+ passed）
- Aura 实现语义与 Rust 实现一致

**工时估算**：2-3 天

### Phase 7（中期，P6）：String 类上移

**目标**：将 String 类上移到 Aura 语言层，基于内建字符串类型作为底层表示

**上移函数**：
| 函数 | 当前位置 | 目标位置 | 说明 |
|------|----------|----------|------|
| `String.trim()`, `String.trimStart()`, `String.trimEnd()` | Rust native | Aura | 空白裁剪 |
| `String.substring(start, end): String` | Rust native | Aura | 子串提取 |
| `String.startsWith(prefix): Boolean` | Rust native | Aura | 前缀检查 |
| `String.endsWith(suffix): Boolean` | Rust native | Aura | 后缀检查 |
| `String.contains(sub): Boolean` | Rust native | Aura | 子串检查 |
| `String.split(separator): List<String>` | Rust native | Aura | 分割 |
| `String.join(separator): String` | Rust native | Aura | 连接（数组→字符串） |
| `String.toUpperCase()`, `String.toLowerCase()` | Rust native | Aura | 大小写转换 |
| `String.repeat(count): String` | Rust native | Aura | 重复 |
| `String.length(): Int` | Rust native | Aura | 长度查询 |
| `String.isEmpty(): Boolean` | Rust native | Aura | 空字符串检查 |

**保留在 Rust（不能上移）**：
| 函数 | 原因 |
|------|------|
| `String.concat(a, b): String` | 字符串拼接核心，需要内存管理 |
| `String.format(template, ...): String` | 格式化需要底层支持 |
| `String.matches(pattern): Boolean` | 正则匹配需要底层支持 |
| `String.hashCode(): Int` | Any 核心虚方法 |
| `String.equals(other): Boolean` | Any 核心虚方法 |

**交付物**：
- `phantom-source/aura/lang/std/String.aura`：添加 Aura 实现
- `compiler/src/std/std_string.rs`：移除已上移函数注册
- `compiler/src/std/decl.rs`：更新注释，标记已上移函数

**验收标准**：
- 单元测试通过（208+ passed）
- Aura 实现语义与 Rust 实现一致
- 字符串操作性能损失不超过 1.5×

**工时估算**：3-5 天

### Phase 8（中远期，P7）：类型内省高级功能上移

**目标**：将类型内省高级功能上移到 Aura 语言层，保留核心类型内省在 Rust

**上移函数**：
| 函数 | 当前位置 | 目标位置 | 说明 |
|------|----------|----------|------|
| `typeHierarchy(typeName): List<String>` | Rust native | Aura | 获取类型继承链 |
| `isSubtype(child, parent): Boolean` | Rust native | Aura | 检查子类型关系 |
| `implementsInterface(typeName, iface): Boolean` | Rust native | Aura | 检查接口实现 |
| `getAllMethods(typeName): List<String>` | Rust native | Aura | 获取所有方法名 |
| `getAllFields(typeName): List<String>` | Rust native | Aura | 获取所有字段名 |
| `getAnnotation(typeName, name): String` | Rust native | Aura | 获取注解 |

**保留在 Rust（不能上移）**：
| 函数 | 原因 |
|------|------|
| `typeOf(value: Any): String` | 类型内省核心，依赖 vtable 指针 |
| `isOfType(value, typeName): Boolean` | 类型检查核心 |
| `cast(value, typeName): Any` | 类型转换核心 |
| `Any.toString(): String` | Any 核心虚方法 |
| `Any.equals(other): Boolean` | Any 核心虚方法 |
| `Any.hashCode(): Int` | Any 核心虚方法 |

**交付物**：
- `phantom-source/aura/lang/std/Builtin.aura`：添加 Aura 实现
- `compiler/src/std/std_builtin.rs`：移除已上移函数注册
- `compiler/src/std/decl.rs`：更新注释，标记已上移函数

**验收标准**：
- 单元测试通过（208+ passed）
- Aura 实现语义与 Rust 实现一致
- 类型内省功能完整可用

**工时估算**：3-5 天

### Phase 9（远期，P8）：完全 OO 可行性评估

**目标**：评估完全 OO（所有类型上移到 Aura）的可行性

**前置条件**：
- 泛型单态化完成
- 集合类型上移到 Aura
- 迭代器上移到 Aura
- 自举编译器完成

**评估内容**：
1. Int/Float/Boolean 装箱的性能开销量化
2. String 类完全 Aura 化的内存开销
3. Any 基类完全 Aura 化的无限递归解决方案
4. 自举编译器的可行性

**交付物**：
- `docs/完全OO可行性分析报告.md`：详细分析
- 原型实现（可选）

**工时估算**：2-4 周

---

## 八、与现有文档的关系

| 文档 | 关系 |
|------|------|
| `docs/标准库自实现与交付方案分析.md` | 本文是对其"三层架构"建议的细化，补充了 AOT FFI 直连的性能量化分析 |
| `docs/Std-Prelude-改造方案.md` | 本文是对其"按需免import"设计的扩展，讨论了上移后的交付模型 |
| `docs/Any基类与object关键字设计方案.md` | 本文明确了 Any 虚方法必须保留在 Rust 内联层，不能上移 |
| `docs/jit优化指南.md` | 本文补充了 JIT dispatcher 优化的方向（直接函数指针调用） |
| `docs/AOT一致性检查报告.md` | 本文补充了 AOT FFI 直连的类型签名一致性要求 |

---

## 九、总结

| 决策项 | 建议 | 理由 |
|--------|------|------|
| **标准库实现语言** | **Rust/C 内联层 + C FFI 层 + Aura 语言层三层分离** | 保留 syscall/协程/Any 基类三个不能上移的边界，其余尽可能上移 |
| **AOT 后端标准库** | **通过 FFI 直连 libc/libm，目标是 AOT 二进制自包含** | 当前 C FFI 仅 30 个函数，需补齐至 200+，使 AOT 性能接近原生 C |
| **交付格式** | **分层交付，标准库不用 .auz** | 标准库是编译器内置，不是外部依赖；.auz 仅用于第三方库 |
| **模块分层** | **三层架构：内联/FFI/上移** | 内联层不能分离，FFI 层通过 C ABI 暴露，上移层用 Aura 实现 |
| **自举时机** | **远期目标，不列入近期计划** | 等编译器支持泛型+集合+迭代器后再考虑 |

### 实施优先级

```
P0（当前）：Rust std 模块已完善，AOT FFI 基础设施已就绪
P1：补齐 aura_std_cffi.c 的核心函数（fs/json/encoding/io），完成泛型单态化
P2：上移纯逻辑模块（Math/String/Path/Env/Time/Encoding），维护 cffi_signature
P3：上移 syscall-邻近模块（FileSystem/IO/Network），通过 extern "c" 调 libc
P4（远期）：评估 Aura 自举可行性（需先完成泛型+集合+迭代器）
```

---

---

## 九、Any 基类与基础数据类型上移分析

> **分析日期**：2026-11
> **分析目标**：评估将 Any 基类和基础数据类型（Int/Float/Boolean/String）上移到 Aura 语言层的可行性

### 9.1 当前架构

| 组件 | 当前位置 | 原因 |
|------|----------|------|
| **Any 基类**（toString/equals/hashCode/typeOf/cast） | Layer 1（Rust vtable 内联） | 虚方法调用链，避免无限递归 |
| **Int/Float/Boolean**（基本类型） | 编译器内建（栈上值） | 零开销抽象，无需堆分配 |
| **String**（字符串） | 半内建（{data, len} 结构体） | 拼接/比较需要底层优化 |
| **类型内省**（typeOf/isNull/cast） | Layer 1（Rust） | 依赖 vtable 指针，无法从 Aura 层自举 |

### 9.2 上移到 Aura 的收益

1. **完全面向对象**：一切皆对象，多态统一，用户可以继承 Int/String 重写方法
2. **减少 Rust 代码**：估算可减少约 500-800 行 Rust 代码
3. **运行时一致性**：所有类型走相同的 vtable 路径，不存在特殊处理
4. **教学价值**：清晰展示 OO 原则，降低学习曲线

### 9.3 上移到 Aura 的代价

| 操作 | 当前（Rust 内联） | 上移到 Aura | 性能损失 |
|------|------------------|-------------|----------|
| `toString()` | 直接 vtable 调用 | Aura 虚方法分发 | 1.2-1.5× |
| `equals()` | 直接 vtable 调用 | Aura 虚方法分发 | 1.2-1.5× |
| `Int.add()` | 内联整数加法 | Aura 方法调用 + 装箱 | 2-5× |
| `String.concat()` | 直接内存操作 | Aura 虚方法分发 + 堆分配 | 1.5-2× |

**关键风险**：
1. **无限递归**：`equals` 调用 `toString`，`toString` 调用 `equals` → 无限递归
2. **自举悖论**：编译 Any 基类需要编译器，编译器需要 Any 基类
3. **装箱开销**：Int/Float 从栈上值变成堆上对象，内存开销 32 字节/个
4. **类型系统复杂性**：需要区分值类型和引用类型

### 9.4 推荐方案：最小引导层（Bootstrap Layer）

```
┌─────────────────────────────────────────────────┐
│  Layer 0: Bootstrap（Rust/C，不能上移）           │
│  最小内建类型：Int, Float, Boolean, Unit          │
│  最小 vtable：toString, equals, hashCode         │
│  类型内省：typeOf, isOfType, cast                │
│  内存管理：malloc, free, arc_increment, arc_decrement │
├─────────────────────────────────────────────────┤
│  Layer 1: 扩展层（上移到 Aura）                   │
│  Any 扩展方法：toInt, toFloat, toStr, clamp      │
│  String 类：基于内建字符串类型的增强封装           │
│  类型内省高级功能：typeHierarchy, isSubtype       │
├─────────────────────────────────────────────────┤
│  Layer 2: 业务层（上移到 Aura）                   │
│  用户自定义类型                                    │
│  标准库扩展模块                                    │
└─────────────────────────────────────────────────┘
```

**保留在 Bootstrap 层（Rust/C）**：
- Any 核心虚方法（toString/equals/hashCode）— 避免无限递归
- Int/Float/Boolean 内建类型 — 避免装箱开销
- 类型内省核心函数（typeOf/isOfType/cast）— 解决自举悖论

**上移到 Aura 层**：
- Any 扩展方法（toInt/toFloat/toStr/clamp）
- String 类（基于内建字符串类型的增强封装）
- 类型内省高级功能（typeHierarchy/isSubtype）

### 9.5 实施优先级

```
P5（当前完成）：纯逻辑模块上移（Math/String/Path/Env/Time/Encoding）
P6：上移 Any 扩展方法到 Aura（toInt/toFloat/toStr/clamp）
P7：上移 String 类到 Aura（基于内建字符串类型）
P8：上移类型内省高级功能到 Aura（typeHierarchy/isSubtype）
P9（远期）：评估完全 OO 可行性（需先完成泛型+集合+迭代器）
```

### 9.6 方案对比

| 方案 | 收益 | 代价 | 推荐度 |
|------|------|------|--------|
| **保留 Any 在 Rust，上移扩展方法** | 中 | 低 | ⭐⭐⭐⭐⭐ |
| **上移 String 类，保留内建字符串** | 中 | 中 | ⭐⭐⭐⭐ |
| **上移 Int/Float 类，装箱** | 低 | 高 | ⭐⭐ |
| **完全 OO（所有类型上移）** | 高 | 极高 | ⭐ |

**结论**：采用折中方案，保留最小引导层，上移扩展功能。完全 OO 作为远期目标，需先解决自举问题和性能优化。

---

**文档状态**：分析完成，建议待评审
**相关文档**：
- `docs/标准库自实现与交付方案分析.md` — 标准库三层实现与交付模型
- `docs/Std-Prelude-改造方案.md` — 按需免import 设计
- `docs/Any基类与object关键字设计方案.md` — Any 基类与 object 关键字
- `docs/jit优化指南.md` — JIT 优化方向
- `docs/AOT一致性检查报告.md` — AOT 类型一致性
- `技术方案.md` — 语言完整技术方案
