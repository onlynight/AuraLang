# Aura 标准库与基础类型「源码可见性」设计方案

> **状态**：设计方案 | **优先级**：P0（开发者体验） | **建议版本**：v0.5 引入 → v1.0 完善
> **范围**：编译期元数据、字节码格式、LSP 协议、IDE 集成、文档生成
> **前置阅读**：[Any基类引入方案分析.md](Any基类引入方案分析.md)、[Std-Prelude-改造方案.md](Std-Prelude-改造方案.md)、[库导出与包格式设计方案.md](库导出与包格式设计方案.md)

---

## 〇、结论摘要

| 维度 | 结论 |
|------|------|
| **核心策略** | **Phantom Source Tree（虚拟源码树）** + **Source Metadata（源码元数据）**——源码作为元数据存在，永不参与编译执行 |
| **基础类型** | 为 `Int`/`Float`/`String` 等基本类型提供虚拟 `.aura` 源码，IDE 可跳转查看签名与文档 |
| **标准库** | 19 个模块、320+ 函数全部提供虚拟 `.aura` 源码，与 Rust/C native 实现**严格分离** |
| **性能代价** | **零运行时代价**（元数据仅存于 `.auc`/`.auz` 静态段，VM/JIT/AOT 均不读取） |
| **编译期代价** | `.auc` 文件增长 ~30-80KB（含源码引用索引），编译时间不变 |
| **实现周期** | 分 4 阶段，约 2-3 周 |
| **风险等级** | 低（纯元数据扩展，不改运行时路径，向后兼容） |
| **阻塞项** | 无——可与任何现有开发并行 |

**一句话**：引入 Phantom Source Tree + Source Metadata 机制，让 IDE 能像 Java/Kotlin 一样跳转查看 stdlib 和基础类型的"源码"，但运行时完全不感知这些元数据——**源码可见性是编译期的奢侈品，执行期不支付任何代价**。

---

## 一、现状分析

### 1.1 基础类型的「不可见性」

基础类型在编译器中有**双重硬编码**：

#### 层 1：语义类型枚举（`compiler/src/sema/ty.rs`）

```rust
// ty.rs:17-63
pub enum Ty {
    Int, Long, Short, Byte, Float, Double, Boolean, Char, String,
    Any, Nothing, Unit,
    // ... 复合类型
}
```

- `Ty::Int` 是 Rust 枚举变体，不是 Aura 源码中的 `class` 声明
- 用户写 `val x: Int = 42` 时，`Int` 被 parser 解析为 `Type::Int`（关键字），不经过符号表查找
- **没有 `Int` 的源文件、没有方法签名、没有文档注释**

#### 层 2：运行时值枚举（`compiler/src/vm/value.rs`）

```rust
// value.rs:14-37
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Rc<str>),
    Null,
    Ref(usize),
    // ...
}
```

- `Value::Int(42)` 直接是 i64 内联存储
- **没有对象头、没有 vtable、没有 toString 虚方法**（`Any基类引入方案分析.md` 正在解决此问题）

#### 层 3：LSP 符号表（`compiler/src/lsp.rs`）

```rust
// lsp.rs:558-584
fn handle_definition(&self, params: &serde_json::Value) -> serde_json::Value {
    // 只在当前文档的 symbols 中查找
    doc.symbols.values().find(|s| line >= s.span.start_line && line <= s.span.end_line)
}
```

- `handle_definition` 仅遍历**用户文档的符号表**
- 基本类型不在任何文档的符号表中（它们是关键字，不是声明）
- **IDE 中对 `Int` 按 F12 无响应**

### 1.2 标准库的「不可见性」

标准库有**三层硬编码**：

#### 层 1：函数名注册表（`compiler/src/std/decl.rs`）

```rust
// decl.rs:97-570
fn build_all_names() -> HashSet<&'static str> {
    let mut s = HashSet::new();
    for n in ["aura.lang.std.Math.abs", "aura.lang.std.Math.sin", ...] {
        s.insert(n);
    }
    s  // 338 个名字
}
```

- 所有 stdlib 函数名是**硬编码的 Rust 字符串数组**
- 没有对应的 `.aura` 源文件
- 没有参数类型、返回类型、文档注释（这些在 `docgen.rs` 中另行维护）

#### 层 2：Rust/C 实现（`compiler/src/std/std_*.rs`）

```rust
// std_math.rs
pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.Math.sin", |args: &[Value]| -> Value {
        Value::Float(args[0].as_float().sin())
    });
    // ... 29 个函数
}
```

- 实现是 Rust 闭包，**不是 Aura 源码**
- 用户无法看到 `sin` 的实现逻辑
- 文档信息在 `docgen.rs` 中以 `StdDoc` Rust 结构体维护——**与实现分离、与源码分离**

#### 层 3：文档生成器（`compiler/src/docgen.rs`）

```rust
// docgen.rs:18-36
pub struct StdDoc {
    pub module: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub params: &'static [(&'static str, &'static str, &'static str)],
    pub returns: &'static str,
    pub example: Option<&'static str>,
    pub rust_fn: &'static str,  // ← 指向 Rust 函数名
}
```

- 文档数据是**声明式 Rust 结构体**，不是从 `.aura` 源文件提取
- 生成 Markdown 文档（`docs/api/index.md`），但**不是用户可跳转的源码**

### 1.3 当前 IDE 体验

| 操作 | 用户代码 | stdlib/基础类型 |
|------|---------|----------------|
| 代码补全 | ✅ 显示用户定义 + 已 import 的 stdlib | ❌ 不显示未 import 的 stdlib |
| 悬停提示 | ✅ 显示类型和可见性 | ❌ 基本类型无响应 |
| 跳转定义 (F12) | ✅ 跳转到用户代码声明 | ❌ 无响应 |
| 查看实现 (⌥⌘B) | ❌ 不支持 | ❌ 无响应 |
| 文档查看 | ❌ 不支持 | ❌ 需查阅 `docs/api/` |

### 1.4 与 Kotlin/Java 的对比

| 特性 | Java | Kotlin | Swift | Aura（当前） | Aura（本方案） |
|------|------|--------|-------|-------------|---------------|
| 基础类型有源码？ | ✅ `Integer.class` → 反编译 | ✅ `kotlin/Integer.kt` | ✅ `Swift/Int.swift` | ❌ 硬编码 | ✅ Phantom Source |
| stdlib 有源码？ | ✅ `rt.jar` 含 `.class` | ✅ `stdlib.jar` 含 `.kt` | ✅ `Swift stdlib` 含 `.swift` | ❌ Rust 实现 | ✅ Phantom Source |
| IDE 可跳转？ | ✅ JDK sources 附加 | ✅ 内建源码导航 | ✅ SourceKit 支持 | ❌ | ✅ LSP VFS |
| 源码在制品中？ | ✅ `.jar` 含 `.class` | ✅ `.jar` 含 `.kt`（sources） | ✅ `.swiftmodule` 含接口 | ❌ 仅 Rust | ✅ `.auz` 含 phantom source |
| 源码影响性能？ | ❌ 不影响 | ❌ 不影响 | ❌ 不影响 | N/A | ❌ 不影响（零运行时代价） |

---

## 二、设计目标与约束

### 2.1 设计目标

| # | 目标 | 度量标准 |
|---|------|---------|
| G1 | 用户可对基本类型（Int/Float/String 等）按 F12 跳转 | IDE 跳转到 phantom source 文件 |
| G2 | 用户可对 stdlib 函数/类按 F12 跳转 | IDE 跳转到 phantom source 文件 |
| G3 | 用户可查看方法签名、参数类型、返回类型、文档注释 | phantom source 含完整 API 表面 |
| G4 | 源码信息在 `.auz` 制品中可分发 | 下游项目无需源码即可跳转 |
| G5 | 向后兼容：现有 `.auc`/`.auz` 文件仍可运行 | 新增段为可选，旧版本忽略 |
| G6 | **零运行时性能代价** | VM/JIT/AOT 不读取源码元数据段 |

### 2.2 硬约束

| 约束 | 含义 | 设计影响 |
|------|------|---------|
| C1：不改运行时路径 | VM/JIT/AOT 执行时不访问源码元数据 | 元数据仅在编译期生成、LSP 阶段读取 |
| C2：不引入新编译阶段 | 不增加 lexer→parser→...→codegen 之外的新阶段 | phantom source 生成嵌入现有 docgen 阶段 |
| C3：不改变现有字节码语义 | `.auc` 现有段保持不变 | 新增可选段，版本号递增但向后兼容 |
| C4：不暴露实现细节 | phantom source 仅展示 API 签名 + 文档 | 不含 Rust/C 实现，不含内部辅助函数 |
| C5：不增加安装体积 | phantom source 应可单独下载或按需获取 | `.auz` 中可选段，LSP 可仅存索引 |
| C6：与 Any 基类方案兼容 | phantom source 遵循 `Any基类引入方案分析.md` 的类型层级 | phantom source 中 `class Dog : Animal()` 等声明 |
| C7：与 Std-Prelude 改造兼容 | phantom source 遵循 prelu/import 分层 | prelude 函数免 import，命名空间函数需 import |

### 2.3 非目标

- ❌ 不实现可修改的 stdlib 源码（用户不能修改 phantom source）
- ❌ 不实现 stdlib 源码编译（phantom source 是只读的）
- ❌ 不改变基本类型的运行时表示（仍内联存储）
- ❌ 不实现完整的 reflection API（仅 `typeOf()` 等基础反射）
- ❌ 不实现热重载或 JIT 重编译 stdlib

---

## 三、总体架构

### 3.1 架构总览

```
┌─────────────────────────────────────────────────────────────────────┐
│                         源码层（Phantom Source Tree）                  │
│                                                                       │
│   aura://builtin/                                                    │
│   ├── Any.aura          ← 基础类型/类层级根                        │
│   ├── Any.aura             ← 顶级类型                                │
│   ├── Int.aura             ← 基本类型                                │
│   ├── Float.aura                                                                 │
│   ├── String.aura                                                                 │
│   ├── Boolean.aura                                                                 │
│   ├── List.aura           ← 集合类型（泛型）                           │
│   └── Map.aura                                                                 │
│                                                                       │
│   aura://stdlib/                                                       │
│   ├── aura/math/Math.aura        ← 19 个模块                          │
│   ├── aura/string/String.aura                                        │
│   ├── aura/io/IO.aura                                                   │
│   ├── aura/collections/List.aura                                       │
│   └── ...                                                               │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ 编译期生成
                               │ （docgen 阶段）
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     元数据层（Source Metadata）                        │
│                                                                       │
│   SourceIndex（编译产物）                                              │
│   ├── type_defs: { "Int" → { uri, line_range } }                     │
│   ├── function_defs: { "aura.lang.std.Math.sin" → { uri, line_range } }       │
│   ├── module_defs: { "aura.lang.std.Math" → { uri, file_path } }              │
│   └── source_archive: 可选，内嵌 phantom source 文本                  │
│                                                                       │
│   .auc 字节码新增段（可选）                                            │
│   ├── class_defs 段（Any基类方案已规划）                             │
│   └── source_index 段（本方案新增）                                     │
│                                                                       │
│   .auz 制品新增文件                                                    │
│   ├── SOURCE/                      ← phantom source 归档              │
│   │   ├── builtin/Int.aura                                                │
│   │   └── stdlib/aura/math/Math.aura                                    │
│   └── META/source-index.aum          ← 二进制索引                      │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ 编译期嵌入
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     工具层（LSP / IDE / DocGen）                       │
│                                                                       │
│   LSP 服务器                                                          │
│   ├── handle_definition: 查 SourceIndex → 返回 phantom URI           │
│   ├── handle_hover: 查 SourceIndex → 返回类型 + 文档                  │
│   ├── handle_completion: 查 SourceIndex → 含 stdlib 补全              │
│   ├── textDocument/didOpen (virtual): LSP 虚拟文件服务                 │
│   └── workspace/symbol: 全局符号索引（含 stdlib）                      │
│                                                                       │
│   IDE 扩展（VS Code / Sublime）                                       │
│   ├── aura:// 协议处理：从 LSP 获取 phantom source 内容                │
│   ├── 语法高亮：phantom source 与普通 .aura 一致                       │
│   └── "只读" 标记：phantom source 标记为只读                           │
│                                                                       │
│   文档生成器（docgen.rs）                                              │
│   ├── render_markdown: 从 SourceIndex 渲染（替代 StdDoc Rust 结构体）  │
│   └── 输出与 phantom source 一致                                      │
└─────────────────────────────────────────────────────────────────────┘
                               │ 运行时
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     执行层（VM / JIT / AOT）                           │
│                                                                       │
│   ★ 不读取 SourceIndex ★                                             │
│   ★ 不读取 phantom source ★                                          │
│   ★ 源码元数据对执行路径完全不可见 ★                                   │
│                                                                       │
│   原生函数注册表（NativeRegistry）                                     │
│   ├── prelude（17 个，始终存在）                                      │
│   └── imported modules（按需注册）                                     │
│                                                                       │
│   字节码执行器（interp.rs / jit.rs / aot/emit.rs）                     │
│   └── 仅读取 functions / vtables / consts 段                          │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 数据流

```
用户代码 → 编译期 → .auc（含 SourceIndex 段）
                        │
                        ├── LSP 读取 → 虚拟文件 → IDE 展示
                        ├── docgen 读取 → Markdown → docs/api/
                        └── 打包进 .auz → 下游 LSP 读取

用户代码 → 运行时 → .auc（仅 functions / vtables / consts）
                        │
                        └── VM / JIT / AOT 执行
                            （SourceIndex 段被跳过/忽略）
```

---

## 四、详细设计

### 4.1 Phantom Source Tree（虚拟源码树）

#### 4.1.1 设计理念

Phantom Source Tree 是一组**只读的 `.aura` 源码文件**，描述基础类型和 stdlib 的 API 表面。它们：

- **永不参与编译**：编译器不会解析、不生成字节码
- **永不参与执行**：VM/JIT/AOT 不知道它们的存在
- **永不参与链接**：不影响 `NativeRegistry`、`enabled_modules` 等
- **仅用于 IDE 导航**：LSP 通过虚拟文件协议服务

#### 4.1.2 目录结构

```
core/
├── builtin/                     ← 基础类型（编译器内建）
│   ├── Any.aura                 ← 运行时基类/顶级类型
│   ├── Nothing.aura             ← 底部类型
│   ├── Unit.aura                ← 无返回值类型
│   ├── Int.aura                 ← 32 位整数
│   ├── Long.aura                ← 64 位整数
│   ├── Short.aura               ← 16 位整数
│   ├── Byte.aura                ← 8 位整数
│   ├── Float.aura               ← 32 位浮点
│   ├── Double.aura              ← 64 位浮点
│   ├── Boolean.aura             ← 布尔
│   ├── Char.aura                ← 字符
│   ├── String.aura              ← 字符串
│   ├── List.aura                ← List<T> 集合
│   ├── Map.aura                 ← Map<K, V> 映射
│   ├── Array.aura               ← Array<T> 数组
│   ├── Function.aura            ← 函数类型
│   ├── Type.aura                ← 反射类型（typeOf() 返回）
│   └── prelude.aura             ← 17 个 prelude 函数声明
│
└── stdlib/                      ← 标准库（19 个模块）
    ├── aura/math/Math.aura
    ├── aura/string/String.aura
    ├── aura/io/IO.aura
    ├── aura/collections/Collections.aura
    ├── aura/fs/FileSystem.aura
    ├── aura/net/Network.aura
    ├── aura/json/Json.aura
    ├── aura/time/Time.aura
    ├── aura/test/Test.aura
    ├── aura/builtin/Builtin.aura
    ├── aura/env/Env.aura
    ├── aura/process/Process.aura
    ├── aura/random/Random.aura
    ├── aura/encoding/Encoding.aura
    ├── aura/ascii/Ascii.aura
    ├── aura/console/Console.aura
    ├── aura/path/Path.aura
    ├── aura/assert/Assert.aura
    └── aura/iter/Iter.aura
```

#### 4.1.3 基础类型 Phantom Source 示例

##### `Int.aura`

```aura
// aura://builtin/Int.aura
// 语言内置类型，不可修改

/// 32 位有符号整数类型。
///
/// - 范围：-2,147,483,648 到 2,147,483,647
/// - 存储：VM 中内联存储为 i64（Value::Int），AOT 中映射为 i32
/// - 性能：零开销，无堆分配，无 ARC 引用计数
///
/// 基本类型不参与 Any 类层级（参见 `Any` 类型）。
/// 使用 `is Int` 通过 Value tag 匹配判断。
///
/// @see Any, Long, Float, Double
/// @since 0.1
internal value class Int {
    // ── 常量 ──
    static val MIN_VALUE: Int = -2147483648
    static val MAX_VALUE: Int = 2147483647
    static val BITS: Int = 32
    static val SIZE: Int = 32
    static val MIN_LONG: Long = -9223372036854775808L
    static val MAX_LONG: Long = 9223372036854775807L

    // ── 转换方法（由编译器内置支持）──

    /// 转换为 64 位长整数
    fun toLong(): Long

    /// 转换为 32 位浮点数
    fun toFloat(): Float

    /// 转换为 64 位双精度浮点数
    fun toDouble(): Double

    /// 转换为字符串表示
    override fun toString(): String

    /// 转换为布尔值（非零为 true）
    fun toBoolean(): Boolean

    /// 转换为字符（取最低 16 位）
    fun toChar(): Char

    /// 转换为字节（取最低 8 位）
    fun toByte(): Byte

    /// 转换为短整型（取最低 16 位）
    fun toShort(): Short

    // ── 比较方法 ──

    /// 比较两个整数，返回 -1/0/1
    fun compareTo(other: Int): Int

    /// 检查两个整数是否相等
    fun equals(other: Int): Boolean

    // ── 算术方法 ──

    /// 绝对值
    fun abs(): Int

    /// 最大值
    fun max(other: Int): Int

    /// 最小值
    fun min(other: Int): Int

    /// 取余
    fun mod(other: Int): Int

    // ── 位操作方法 ──

    /// 按位与
    fun and(other: Int): Int

    /// 按位或
    fun or(other: Int): Int

    /// 按位异或
    fun xor(other: Int): Int

    /// 左移
    fun shl(bits: Int): Int

    /// 右移（算术右移）
    fun shr(bits: Int): Int

    /// 无符号右移
    fun ushr(bits: Int): Int

    // ── 数学方法 ──

    /// 计算幂
    fun pow(exp: Int): Int

    /// 转换为字符串（指定基数）
    fun toRadixString(radix: Int): String

    /// 从字符串解析为整数
    fun parseInt(text: String): Int
}
```

##### `String.aura`

```aura
// aura://builtin/String.aura
// 语言内置类型，不可修改

/// 不可变字符串类型。
///
/// - 存储：VM 中为 `Rc<str>`（引用计数共享），AOT 中为 `{ptr, len}` 结构
/// - 字符串字面量在编译期驻留于常量池
/// - 支持字符串插值：`"hello ${name}"`
///
/// @see List, Char
/// @since 0.1
internal value class String {
    // ── 常量 ──
    static val EMPTY: String = ""

    // ── 属性 ──

    /// 字符串的字符数量
    val length: Int
        get()

    // ── 构造 ──

    /// 从字符数组构造
    fun String(chars: Array<Char>): String

    // ── 转换方法 ──

    /// 转换为整数
    fun toInt(): Int

    /// 转换为长整数
    fun toLong(): Long

    /// 转换为浮点数
    fun toFloat(): Float

    /// 转换为双精度浮点数
    fun toDouble(): Double

    /// 转换为布尔值（"true" → true, 其他 → false）
    fun toBoolean(): Boolean

    /// 转换为字符数组
    fun toCharArray(): Array<Char>

    // ── 比较方法 ──

    /// 比较两个字符串（字典序）
    fun compareTo(other: String): Int

    /// 检查两个字符串是否相等（内容相等）
    fun equals(other: String): Boolean

    /// 检查字符串是否为空
    fun isEmpty(): Boolean

    /// 检查字符串是否为空白（仅含空白字符或为空）
    fun isBlank(): Boolean

    // ── 查找方法 ──

    /// 检查是否包含子字符串
    fun contains(substring: String): Boolean

    /// 检查是否以指定前缀开头
    fun startsWith(prefix: String): Boolean

    /// 检查是否以指定后缀结尾
    fun endsWith(suffix: String): Boolean

    /// 查找子字符串的索引（从前往后）
    fun indexOf(substring: String): Int

    /// 查找子字符串的索引（从后往前）
    fun lastIndexOf(substring: String): Int

    // ── 转换方法 ──

    /// 转换为小写
    fun toLowerCase(): String

    /// 转换为大写
    fun toUpperCase(): String

    /// 去除首尾空白
    fun trim(): String

    /// 去除前导空白
    fun trimStart(): String

    /// 去除尾部空白
    fun trimEnd(): String

    // ── 截取方法 ──

    /// 截取子字符串 [from, to)
    fun substring(from: Int, to: Int): String

    /// 截取子字符串 [from, len)
    fun substring(from: Int, len: Int): String

    /// 截取分隔符之前的部分
    fun substringBefore(delimiter: String): String

    /// 截取分隔符之后的部分
    fun substringAfter(delimiter: String): String

    // ── 分割与合并 ──

    /// 按分隔符分割为列表
    fun split(separator: String): List<String>

    /// 按换行分割为列表
    fun splitLines(): List<String>

    /// 用分隔符合并列表
    fun join(list: List<String>, separator: String): String

    // ── 替换与格式化 ──

    /// 替换所有匹配的子字符串
    fun replace(target: String, replacement: String): String

    /// 用正则表达式替换
    fun replaceAll(regex: String, replacement: String): String

    /// 格式化字符串（{0} {1} 占位符）
    fun format(template: String, vararg args: Any): String

    /// 重复字符串 n 次
    fun repeat(times: Int): String

    /// 左填充到指定长度
    fun padStart(length: Int, char: Char): String

    /// 右填充到指定长度
    fun padEnd(length: Int, char: Char): String

    // ── 正则 ──

    /// 检查是否匹配正则表达式
    fun matches(regex: String): Boolean

    // ── 隐式 toString 支持 ──

    /// 任何类型转字符串（编译器自动插入）
    override fun toString(): String
}
```

#### 4.1.4 标准库 Phantom Source 示例

##### `aura/math/Math.aura`

```aura
// aura://stdlib/aura/math/Math.aura
// 标准库模块，不可修改
// 实现：Rust 原生函数（compiler/src/std/std_math.rs）

package aura.lang.std.Math

/// 数学函数与常量模块。
///
/// 使用前需 import：
/// ```aura
/// import aura.lang.std.Math.*
/// ```
///
/// 部分函数也在 prelude 中可用（免 import）：`abs`, `sqrt`, `pow`
///
/// @since 0.1
internal object Math {

    // ── 常量 ──

    /// 圆周率 π
    static val PI: Float = 3.14159265f

    /// 自然常数 e
    static val E: Float = 2.71828182f

    /// 32 位整数最大值
    static val INT_MAX: Int = 2147483647

    /// 32 位整数最小值
    static val INT_MIN: Int = -2147483648

    /// 浮点数最大值
    static val FLOAT_MAX: Float = 3.4028235e38f

    // ── 基础运算 ──

    /// 返回绝对值
    /// - 参数：`x` — 数字（Int 或 Float）
    /// - 返回：绝对值（与输入同类型）
    /// - 注意：`abs(Int.MIN_VALUE)` 溢出为 `Int.MIN_VALUE` 本身
    static fun abs(x: Int): Int
    static fun abs(x: Float): Float

    /// 返回最小值
    static fun min(a: Int, b: Int): Int
    static fun min(a: Float, b: Float): Float

    /// 返回最大值
    static fun max(a: Int, b: Int): Int
    static fun max(a: Float, b: Float): Float

    // ── 取整 ──

    /// 向上取整
    static fun ceil(x: Float): Float

    /// 向下取整
    static fun floor(x: Float): Float

    /// 四舍五入
    static fun round(x: Float): Float

    /// 截断小数部分
    static fun trunc(x: Float): Float

    // ── 幂与根 ──

    /// 平方根
    static fun sqrt(x: Float): Float

    /// 立方根
    static fun cbrt(x: Float): Float

    /// 幂运算 x^y
    static fun pow(base: Float, exp: Float): Float

    // ── 指数与对数 ──

    /// e 的 x 次方
    static fun exp(x: Float): Float

    /// 自然对数
    static fun log(x: Float): Float

    /// 以 2 为底的对数
    static fun log2(x: Float): Float

    /// 以 10 为底的对数
    static fun log10(x: Float): Float

    // ── 三角函数 ──

    /// 正弦（弧度）
    static fun sin(x: Float): Float

    /// 余弦（弧度）
    static fun cos(x: Float): Float

    /// 正切（弧度）
    static fun tan(x: Float): Float

    /// 反正弦（返回弧度）
    static fun asin(x: Float): Float

    /// 反余弦（返回弧度）
    static fun acos(x: Float): Float

    /// 反正切（返回弧度）
    static fun atan(x: Float): Float

    /// 反正切 atan2(y, x)（返回弧度）
    static fun atan2(y: Float, x: Float): Float

    // ── 辅助 ──

    /// 符号函数：-1, 0, 1
    static fun sign(x: Float): Float

    /// 限制在 [min, max] 范围内
    static fun clamp(x: Float, min: Float, max: Float): Float
}
```

##### `prelu.aura`

```aura
// aura://builtin/prelude.aura
// 免 import 的全局内置函数（17 个）

/// Prelude 函数集——免 import，始终可用。
///
/// 这些函数由编译器自动注册到符号表，无需 `import` 声明。
/// 用户不能重定义这些函数名（编译报错）。
///
/// @since 0.2
internal object Prelude {

    /// 打印一行文本（末尾自动追加换行符）
    fun println(vararg args: Any): Unit

    /// 打印文本（不追加换行符）
    fun print(vararg args: Any): Unit

    /// C 风格输出
    fun puts(text: String): Unit

    /// 绝对值
    fun abs(x: Any): Any

    /// 平方根
    fun sqrt(x: Float): Float

    /// 幂运算
    fun pow(base: Float, exp: Float): Float

    /// 转换为整数
    fun toInt(x: Any): Int

    /// 转换为浮点数
    fun toFloat(x: Any): Float

    /// 转换为字符串
    fun toStr(x: Any): String

    /// 转换为字符串（同 toStr）
    fun toString(x: Any): String

    /// 获取当前时钟（Unix 时间戳，秒）
    fun clock(): Float

    /// 获取字符串长度
    fun strlen(text: String): Int

    /// C 字符串转换
    fun CString(text: String): String

    /// C 字符串转换（别名）
    fun CStr(text: String): String

    /// 指针判空
    fun ptrIsNull(ptr: Any): Boolean

    /// 指针转整数
    fun ptrToInt(ptr: Any): Int

    /// 整数转指针
    fun intToPtr(value: Int): Any

    /// 创建回调函数
    fun makeCallback(fn: (Any) -> Any): Any

    /// 创建列表
    fun listOf(vararg items: Any): List<Any>
}
```

### 4.2 Source Metadata 格式

#### 4.2.1 SourceIndex 数据结构

SourceIndex 是编译期的核心元数据结构，描述所有符号到源码位置的映射：

```rust
/// 源码索引——编译期生成，LSP 读取
/// 存储在 .auc 的 source_index 段中（可选段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceIndex {
    /// 版本（用于向前兼容）
    pub version: u16,

    /// 类型定义映射：类型名 → 源码位置
    /// 例：{"Int" → {uri: "aura://builtin/Int.aura", line: 12, col: 0}}
    pub type_defs: HashMap<String, SourceLocation>,

    /// 函数定义映射：函数全名 → 源码位置
    /// 例：{"aura.lang.std.Math.sin" → {uri: "aura://stdlib/aura/math/Math.aura", line: 85, col: 4}}
    pub function_defs: HashMap<String, SourceLocation>,

    /// 模块定义映射：模块路径 → 源码文件
    /// 例：{"aura.lang.std.Math" → {uri: "aura://stdlib/aura/math/Math.aura", file_path: "math/Math.aura"}}
    pub module_defs: HashMap<String, SourceLocation>,

    /// 常量定义映射：常量全名 → 源码位置
    pub constant_defs: HashMap<String, SourceLocation>,

    /// 变量定义映射（字段、属性）
    pub variable_defs: HashMap<String, SourceLocation>,

    /// 枚举定义映射
    pub enum_defs: HashMap<String, SourceLocation>,

    /// 枚举变体映射
    pub enum_variant_defs: HashMap<String, SourceLocation>,

    /// 接口定义映射
    pub interface_defs: HashMap<String, SourceLocation>,

    /// phantom source 归档（可选）
    /// 若包含，则 .auc 自带全部 phantom source 文本
    /// 若不含，则 LSP 从 .auz 的 SOURCE/ 段或独立 source 包获取
    pub source_archive: Option<SourceArchive>,
}

/// 源码位置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// 虚拟 URI（aura:// 协议）
    pub uri: String,
    /// 起始行（1-based）
    pub line: u32,
    /// 起始列（0-based）
    pub col: u32,
    /// 结束行
    pub end_line: u32,
    /// 结束列
    pub end_col: u32,
}

/// 源码归档（可选内嵌）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceArchive {
    /// 文件路径 → 内容
    pub files: HashMap<String, String>,
    /// SHA-256 校验和
    pub checksum: [u8; 32],
}
```

#### 4.2.2 索引粒度

| 粒度级别 | 包含内容 | .auc 增长 | 适用场景 |
|---------|---------|----------|---------|
| **Level 0：无索引** | 不含 source_index 段 | 0 | 旧版本 .auc，仅执行 |
| **Level 1：位置索引** | 仅 type_defs + function_defs | ~20KB | 最小 LSP 支持 |
| **Level 2：完整索引** | 全部 defs + 无 archive | ~40KB | 推荐，LSP 从 .auz 获取源码 |
| **Level 3：完整索引 + 归档** | 全部 defs + source_archive | ~300-800KB | 自包含 .auc，无需外部 source |

#### 4.2.3 与 .auc 格式的关系

```
当前 .auc v5 格式：
  header (64 bytes)
  consts 段
  natives 段
  functions 段
  closures 段
  vtables 段
  [aot 段]

新增 v6 格式（向后兼容 v5）：
  header (64 bytes, 新增 HEADER_HAS_SOURCE_INDEX + HEADER_HAS_CLASS_DEFS)
  consts 段         ← 不变
  natives 段        ← 不变
  functions 段      ← 不变
  closures 段       ← 不变
  vtables 段        ← 不变
  [class_defs 段]   ← Any基类方案新增
  [source_index 段] ← 本方案新增（可选）
  [aot 段]          ← 不变

source_index 段格式：
  u16    version          // 索引版本
  u16    type_def_count
  for each type_def:
    u16    name_offset    // string_pool 偏移
    u16    name_len
    u16    uri_offset
    u16    uri_len
    u32    line
    u32    col
    u32    end_line
    u32    end_col
  u16    function_def_count
  for each function_def:
    ...（同上）
  u16    module_def_count
  ...（同上）
  u16    constant_def_count
  ...（同上）
  u16    variable_def_count
  ...（同上）
  u16    enum_def_count
  ...（同上）
  u16    enum_variant_def_count
  ...（同上）
  u16    interface_def_count
  ...（同上）
  u8     has_source_archive    // 0/1
  if has_source_archive:
    u16    file_count
    for each file:
      u16    path_offset
      u16    path_len
      u32    content_offset
      u32    content_len
```

**向后兼容策略**：
- 读取 `.auc` 时检查 `HEADER_HAS_SOURCE_INDEX` 标志
- 若标志为 0（旧版本），跳过 source_index 段
- VM/JIT/AOT **永远不检查此标志**（不读取此段）

### 4.3 .auz 制品扩展

#### 4.3.1 .auz 结构扩展

```
current .auz:
  aura/
    manifest.toml       ← PackageManifest
    meta/
      signature.sig     ← 签名（可选）
      types.sig         ← 类型签名（可选）
    lib/
      module.auc        ← 字节码
    resources/          ← 资源文件（可选）

新增 .auz:
  aura/
    manifest.toml       ← 不变
    meta/
      signature.sig     ← 不变
      types.sig         ← 不变
      source-index.aum  ← 新增：二进制 SourceIndex
    lib/
      module.auc        ← 不变（可含 source_index 段）
    source/             ← 新增：phantom source 归档
      builtin/
        Int.aura
        Float.aura
        String.aura
        ...
      stdlib/
        aura/math/Math.aura
        aura/string/String.aura
        ...
    resources/          ← 不变
```

#### 4.3.2 Manifest 扩展

```toml
# manifest.toml 新增字段

[package]
name = "my-lib"
version = "1.0.0"
# ... 现有字段

# 新增：源码分发配置
[source]
# 是否包含 phantom source（默认 true）
include-source = true

# phantom source 来源
# "embedded" = 内嵌在 .auz 的 source/ 段
# "external" = 独立 .source.auz 包（按需下载）
source-mode = "embedded"

# phantom source 版本（与编译器版本匹配）
source-version = "0.5"
```

### 4.4 LSP 虚拟文件协议

#### 4.4.1 URI Scheme 设计

```
aura://builtin/Int.aura            ← 基础类型
aura://builtin/Int.aura#L12        ← 特定行
aura://stdlib/aura/math/Math.aura   ← stdlib 模块
aura://stdlib/aura/math/Math.aura#L85
aura://prelude/prelu.aura          ← prelude 函数
```

**设计决策**：
- 使用 `aura://` 自定义 URI scheme（非 `file://`）
- 路径与 phantom source 目录结构一致
- 带 `#L` 锚点支持行级跳转

#### 4.4.2 LSP 扩展方法

##### `textDocument/definition`（增强）

```json
// 请求
{
  "method": "textDocument/definition",
  "params": {
    "textDocument": { "uri": "file:///project/main.aura" },
    "position": { "line": 5, "character": 10 }
  }
}

// 响应（当前用户代码）
{
  "result": {
    "uri": "file:///project/main.aura",
    "range": {
      "start": { "line": 20, "character": 0 },
      "end": { "line": 35, "character": 1 }
    }
  }
}

// 响应（stdlib/基础类型——本方案新增）
{
  "result": {
    "uri": "aura://stdlib/aura/math/Math.aura",
    "range": {
      "start": { "line": 84, "character": 4 },
      "end": { "line": 85, "character": 1 }
    }
  }
}
```

**实现逻辑**：
```rust
fn handle_definition(&self, params: &serde_json::Value) -> serde_json::Value {
    // 1. 先在用户文档符号表中查找（现有逻辑）
    if let Some(loc) = lookup_in_user_symbols(...) {
        return loc;
    }

    // 2. 在 SourceIndex 中查找（新增逻辑）
    if let Some(loc) = lookup_in_source_index(...) {
        return loc;  // 返回 aura:// URI
    }

    // 3. 在基本类型定义中查找（新增逻辑）
    if let Some(loc) = lookup_in_builtin_types(...) {
        return loc;  // 返回 aura:// URI
    }

    None
}
```

##### `textDocument/hover`（增强）

```json
{
  "result": {
    "contents": {
      "kind": "markdown",
      "value": "```aura\nstatic fun sin(x: Float): Float\n```\n\n正弦函数（弧度）\n\n**定义于**: aura://stdlib/aura/math/Math.aura:85\n\n**源文件**: 可点击跳转"
    },
    "range": { ... }
  }
}
```

##### `textDocument/completion`（增强）

```json
{
  "result": [
    {
      "label": "sin",
      "kind": 3,  // Function
      "detail": "static fun sin(x: Float): Float",
      "documentation": {
        "kind": "markdown",
        "value": "正弦函数（弧度）"
      },
      "sortText": "0002"  // stdlib 补全优先级低于用户代码
    }
  ]
}
```

##### 新增：`textDocument/didOpen`（虚拟文件）

```json
// 当用户点击 aura:// URI 时，IDE 通过 LSP 请求虚拟文件内容
{
  "method": "textDocument/didOpen",
  "params": {
    "textDocument": {
      "uri": "aura://stdlib/aura/math/Math.aura",
      "languageId": "aura",
      "version": 1,
      "text": ""  // 空——由 LSP 填充
    }
  }
}
```

**LSP 响应**：
```json
{
  "method": "textDocument/didOpen",
  "result": {
    "text": "// aura://stdlib/aura/math/Math.aura\n// 标准库模块，不可修改\n\npackage aura.lang.std.Math\n\n...",
    "readOnly": true,
    "languageId": "aura"
  }
}
```

##### 新增：`workspace/symbol`（全局符号）

```json
{
  "method": "workspace/symbol",
  "params": { "query": "sin" }
}

// 响应
{
  "result": [
    {
      "name": "sin",
      "kind": 3,  // Function
      "location": {
        "uri": "aura://stdlib/aura/math/Math.aura",
        "range": {
          "start": { "line": 84, "character": 4 },
          "end": { "line": 85, "character": 1 }
        }
      }
    }
  ]
}
```

#### 4.4.3 LSP 内部架构

```
LSP Server
├── DocumentManager（现有：管理用户文档）
├── SourceIndexProvider（新增：管理 phantom source 元数据）
│   ├── source_indices: HashMap<String, SourceIndex>  // 模块名 → 索引
│   ├── phantom_sources: HashMap<String, String>      // URI → 源码文本
│   └── builtin_types: HashMap<String, SourceLocation> // 基础类型索引
│
├── IncrementalEngine（现有：增量编译）
│   └── 新增：编译时生成 SourceIndex
│
└── LspHandler（增强）
    ├── handle_definition: 先用户符号 → 再 SourceIndex
    ├── handle_hover: 查 SourceIndex 增强信息
    ├── handle_completion: 含 stdlib 补全
    ├── handle_didOpen (virtual): 服务 phantom source
    └── handle_workspaceSymbol: 全局符号搜索
```

#### 4.4.4 SourceIndex 加载策略

| 场景 | 加载方式 | 内存开销 |
|------|---------|---------|
| LSP 启动 | 加载当前工作区的 SourceIndex（从 .auc 或 .auz） | ~50KB/项目 |
| 依赖模块 | 加载依赖的 .auz 中的 source/ 段 | ~200KB/依赖 |
| 基础类型 | 内嵌在 LSP 二进制中（compile-time include_str!） | ~50KB |
| phantom source 归档 | 按需加载（首次打开 aura:// URI 时） | ~1-5MB（全量） |

### 4.5 IDE 扩展集成

#### 4.5.1 VS Code 扩展架构

```
vscode-extension/
├── package.json                 ← 新增 aura:// 协议注册
├── src/
│   ├── extension.ts             ← 主入口
│   ├── lspClient.ts             ← LSP 客户端
│   ├── uriHandler.ts            ← 新增：aura:// URI 处理
│   ├── sourceResolver.ts        ← 新增：phantom source 解析
│   └── readOnlyEditor.ts        ← 新增：只读编辑器模式
```

#### 4.5.2 URI Handler 实现

```typescript
// uriHandler.ts
export function registerUriHandler(context: vscode.ExtensionContext) {
  // 注册 aura:// 协议处理器
  vscode.workspace.registerUriHandler({
    async handleUri(uri: vscode.Uri) {
      if (uri.scheme === 'aura') {
        // 1. 向 LSP 请求虚拟文件内容
        const content = await lspClient.requestVirtualFile(uri.toString());

        // 2. 打开只读编辑器
        await openReadOnlyEditor(uri, content);
      }
    }
  });
}

async function openReadOnlyEditor(uri: vscode.Uri, content: string) {
  // 使用 custom read-only editor（非 untitled 文件）
  // 编辑器标记为只读，禁止保存
  const doc = await vscode.workspace.openTextDocument({
    language: 'aura',
    content: content,
    uri: uri  // 保留 aura:// URI
  });
  await vscode.window.showTextDocument(doc, {
    preview: false,
    preserveFocus: false
  });
}
```

#### 4.5.3 只读编辑器体验

| 特性 | 普通 .aura 文件 | phantom source (aura://) |
|------|----------------|------------------------|
| 语法高亮 | ✅ | ✅ |
| 代码补全 | ✅ | ✅（含 stdlib） |
| 跳转定义 | ✅ | ✅（含跨文件） |
| 悬停提示 | ✅ | ✅ |
| 编辑 | ✅ 可编辑 | ❌ 只读（顶部显示 "只读" 标签） |
| 保存 | ✅ | ❌ 不允许保存 |
| 文件树 | ✅ 在 Explorer 中显示 | ❌ 不在 Explorer 中显示（虚拟文件） |
| 搜索 | ✅ 可全局搜索 | ✅ 可全局搜索（通过 LSP） |

### 4.6 文档生成器集成

#### 4.6.1 从 SourceIndex 渲染 Markdown

当前 `docgen.rs` 使用 `StdDoc` Rust 结构体（与实现分离、与源码分离）。本方案后：

```
当前流程：
  StdDoc (Rust struct) → render_markdown() → docs/api/*.md

新流程：
  SourceIndex → phantom_source_text → render_markdown() → docs/api/*.md
                ↓
          docgen 从 phantom source 提取文档注释
          生成与 phantom source 一致的 Markdown
```

**好处**：
- 单一真相源：phantom source 是唯一真相，文档从它生成
- 消除重复维护：不需要同时维护 StdDoc 和 phantom source
- 文档与源码一致：docgen 输出与 IDE 看到的 phantom source 一致

#### 4.6.2 文档生成流程

```
aura docgen
  │
  ├── 1. 从 .auc 加载 SourceIndex
  ├── 2. 从 phantom source 提取文档注释（/// 注释）
  ├── 3. 按模块分组
  ├── 4. 渲染为 Markdown
  └── 5. 输出到 docs/api/
```

### 4.7 性能保证分析

#### 4.7.1 运行时性能

| 路径 | 影响 | 原因 |
|------|------|------|
| VM 解释器 | **零影响** | 不读取 source_index 段 |
| JIT 编译器 | **零影响** | 不读取 source_index 段 |
| AOT 编译器 | **零影响** | 不读取 source_index 段 |
| 字节码执行 | **零影响** | source_index 段被跳过 |
| NativeRegistry | **零影响** | 不依赖 source_index |
| 内存使用 | **零影响** | source_index 段不加载到 VM 堆 |

**保证机制**：
- source_index 段在 `.auc` header 中由 `HEADER_HAS_SOURCE_INDEX` 标志控制
- VM 加载 `.auc` 时**仅读取 functions / vtables / consts / closures 段**
- source_index 段的存在对 VM/JIT/AOT 完全不可见
- 编译后的二进制中不含任何 phantom source 文本

#### 4.7.2 编译期性能

| 阶段 | 影响 | 增量 |
|------|------|------|
| 词法分析 | 零影响 | phantom source 不参与编译 |
| 语法分析 | 零影响 | phantom source 不参与编译 |
| 语义分析 | 零影响 | 类型系统不变 |
| HIR 生成 | 零影响 | 不改变 AST→HIR 降级 |
| 字节码生成 | 极小影响 | 新增 source_index 段序列化 |
| 文档生成 | 增加 | 从 SourceIndex 生成（替代 StdDoc） |

**source_index 生成开销**：
- 类型定义索引：~0.5ms（19 个基本类型 + N 个用户类型）
- 函数定义索引：~2ms（338 个 stdlib 函数 + N 个用户函数）
- 总增量：~5ms/编译（可忽略）

#### 4.7.3 编译产物大小

| 产物 | 当前 | 新增 | 增量 |
|------|------|------|------|
| `.auc`（无 source_index） | ~50KB | ~50KB | 0 |
| `.auc`（含 Level 1 索引） | - | ~70KB | +20KB |
| `.auc`（含 Level 2 索引） | - | ~90KB | +40KB |
| `.auc`（含 Level 3 归档） | - | ~800KB | +750KB |
| `.auz`（无 source/） | ~100KB | ~100KB | 0 |
| `.auz`（含 source/） | - | ~400KB | +300KB |

#### 4.7.4 LSP 性能

| 操作 | 当前 | 新增 | 影响 |
|------|------|------|------|
| 启动 | ~50ms | ~80ms（+加载 SourceIndex） | +30ms（可接受） |
| 补全（含 stdlib） | ~2ms | ~3ms | +1ms |
| 跳转定义（用户代码） | ~1ms | ~1ms | 0 |
| 跳转定义（stdlib） | ❌ 无响应 | ~2ms | 新功能 |
| 悬停（用户代码） | ~1ms | ~1ms | 0 |
| 悬停（stdlib） | ❌ 无响应 | ~2ms | 新功能 |
| 虚拟文件打开 | ❌ 不支持 | ~5ms | 新功能 |

### 4.8 与其他方案的兼容性

#### 4.8.1 与 Any 基类方案

```
Any基类引入方案分析.md 规划：
  - .auc v6 新增 class_defs 段
  - Any 基类、type_id、vtable_idx

本方案：
  - .auc v6 新增 source_index 段
  - phantom source 中声明 Any 基类

兼容性：✅ 完全兼容
  - 两个新增段独立，互不干扰
  - phantom source 的 Any.aura 与 class_defs 段的 Any 定义一致
  - phantom source 是"源代码视图"，class_defs 是"运行时视图"
```

#### 4.8.2 与 Std-Prelude 改造

```
Std-Prelude-改造方案.md 规划：
  - prelu 函数免 import（17 个）
  - 命名空间函数需 import（320+ 个）

本方案：
  - phantom source 中 prelu.aura 声明 17 个函数
  - phantom source 中各模块声明命名空间函数
  - LSP 补全时区分 prelu（免 import）和命名空间（需 import）

兼容性：✅ 完全兼容
  - phantom source 遵循 prelu/import 分层
  - LSP 补全显示 import 状态
```

#### 4.8.3 与库导出与包格式方案

```
库导出与包格式设计方案.md 规划：
  - .auz 制品格式
  - .sig 类型签名
  - manifest.toml

本方案：
  - .auz 新增 source/ 段
  - manifest.toml 新增 [source] 配置
  - .auc 新增 source_index 段

兼容性：✅ 完全兼容
  - source/ 段是 .auz 的可选段
  - source_index 段是 .auc 的可选段
  - 旧版本忽略新段，新版本按需读取
```

---

## 五、实施计划

### 5.1 阶段划分

```
┌─────────────────────────────────────────────────────────────────────┐
│ Phase 1: Phantom Source Tree 基础（1 周）                             │
│                                                                       │
│ • 创建 core/ 目录结构                                       │
│ • 编写 19 个基础类型的 .aura 文件（Int/Float/String/...）              │
│ • 编写 prelu.aura（17 个免 import 函数）                              │
│ • 编写 19 个 stdlib 模块的 .aura 文件                                 │
│ • 定义 SourceIndex 数据结构（Rust）                                   │
│ • 实现 SourceIndex 序列化/反序列化                                    │
│                                                                       │
│ 交付物：                                                              │
│   core/builtin/*.aura    （19 个基础类型）                    │
│   core/stdlib/aura/*/*.aura  （19 个模块）                   │
│   compiler/src/std/source_index.rs （SourceIndex 结构体）              │
│                                                                       │
│ 验收：                                                                │
│   - phantom source 文件可被 IDE 语法高亮                              │
│   - SourceIndex 可序列化/反序列化                                     │
│   - 文档注释完整（每个函数有 /// 注释）                                │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Phase 2: 编译期 SourceIndex 生成（0.5 周）                            │
│                                                                       │
│ • 在 docgen.rs 中集成 phantom source 解析                             │
│ • 从 phantom source 提取符号 → 生成 SourceIndex                       │
│ • 将 SourceIndex 写入 .auc 的 source_index 段                        │
│ • 修改 serialize.rs 支持新段（HEADER_HAS_SOURCE_INDEX）               │
│                                                                       │
│ 交付物：                                                              │
│   compiler/src/codegen/serialize.rs    （新增 source_index 段）        │
│   compiler/src/docgen.rs               （从 phantom source 生成索引）   │
│                                                                       │
│ 验收：                                                                │
│   - .auc 文件含 source_index 段                                      │
│   - 旧版本 VM 加载新 .auc 正常（忽略 source_index 段）                 │
│   - 编译时间增量 < 5ms                                                │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Phase 3: LSP 虚拟文件服务（1 周）                                      │
│                                                                       │
│ • 实现 SourceIndexProvider（加载/缓存 SourceIndex）                   │
│ • 增强 handle_definition（先用户 → 再 SourceIndex）                   │
│ • 增强 handle_hover（含 stdlib 类型信息）                              │
│ • 增强 handle_completion（含 stdlib 补全）                            │
│ • 实现虚拟文件服务（textDocument/didOpen for aura://）                 │
│ • 实现 workspace/symbol（全局符号搜索）                                │
│                                                                       │
│ 交付物：                                                              │
│   compiler/src/lsp.rs                 （增强 LSP 处理器）               │
│   compiler/src/lsp/source_index.rs    （SourceIndex 加载/缓存）        │
│                                                                       │
│ 验收：                                                                │
│   - IDE 中对 Int 按 F12 跳转到 aura://builtin/Int.aura               │
│   - IDE 中对 sin 按 F12 跳转到 aura://stdlib/aura/math/Math.aura     │
│   - 悬停 stdlib 函数显示完整签名 + 文档                                │
│   - 补全显示 stdlib 函数（标注 import 状态）                           │
│   - 打开 aura:// URI 显示只读 phantom source                          │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Phase 4: 制品集成 + 文档生成 + 测试（0.5 周）                          │
│                                                                       │
│ • 实现 .auz 的 source/ 段打包                                         │
│ • 实现 .auz 的 source-index.aum 生成                                 │
│ • 修改 docgen 从 SourceIndex 渲染 Markdown                            │
│ • 实现 VS Code 扩展的 uriHandler                                      │
│ • 回归测试：现有测试全部通过                                          │
│ • 新增测试：LSP phantom source 跳转/悬停/补全                         │
│                                                                       │
│ 交付物：                                                              │
│   compiler/src/package.rs          （.auz source/ 段）                 │
│   cli/src/main.rs                  （aura docgen 命令）                │
│   vscode-extension/src/uriHandler.ts  （URI 处理）                    │
│   compiler/tests/lsp_phantom_source.rs （LSP 测试）                   │
│                                                                       │
│ 验收：                                                                │
│   - .auz 含 source/ 段和 source-index.aum                             │
│   - 下游项目可跳转到依赖库的 phantom source                            │
│   - docgen 输出与 phantom source 一致                                 │
│   - VS Code 中 aura:// URI 正常打开                                   │
│   - 所有现有测试通过（零回归）                                        │
└─────────────────────────────────────────────────────────────────────┘
```

### 5.2 影响文件清单

| 文件 | 修改类型 | 预估改动量 | Phase |
|------|----------|-----------|-------|
| `core/builtin/*.aura` | 新增 | ~2000 行 | P1 |
| `core/stdlib/aura/*/*.aura` | 新增 | ~4000 行 | P1 |
| `compiler/src/std/source_index.rs` | 新增 | ~200 行 | P1 |
| `compiler/src/codegen/serialize.rs` | 修改 | +60 行 | P2 |
| `compiler/src/docgen.rs` | 修改 | +80 行 | P2, P4 |
| `compiler/src/lsp.rs` | 修改 | +200 行 | P3 |
| `compiler/src/lsp/source_index.rs` | 新增 | ~150 行 | P3 |
| `compiler/src/package.rs` | 修改 | +40 行 | P4 |
| `cli/src/main.rs` | 修改 | +30 行 | P4 |
| `vscode-extension/src/uriHandler.ts` | 新增 | ~80 行 | P4 |
| `compiler/tests/lsp_phantom_source.rs` | 新增 | ~200 行 | P4 |

**总计**：约 5340 行新增/修改代码（其中 phantom source 文本 ~6000 行不计入）。

### 5.3 向后兼容性

| 场景 | 兼容性 |
|------|--------|
| 旧版本 `.auc` 加载新版本 VM | ✅ 兼容（无 source_index 段，正常执行） |
| 新版本 `.auc` 加载旧版本 VM | ✅ 兼容（旧 VM 跳过 source_index 段） |
| 新版本 `.auz` 加载旧版本 LSP | ✅ 兼容（旧 LSP 忽略 source/ 段） |
| 新版本 `.auz` 加载新版本 LSP | ✅ 增强（LSP 可浏览 phantom source） |
| 用户代码不含任何 stdlib import | ✅ 兼容（phantom source 不影响执行） |
| 用户重定义 stdlib 函数名 | ⚠️ prelu 函数报错（已有机制），命名空间函数需 import 后才冲突 |

### 5.4 风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| phantom source 与实现不一致 | 中 | 高 | CI 检查：对比 phantom source 签名 vs docgen StdDoc |
| SourceIndex 段导致旧 VM 崩溃 | 低 | 高 | header 标志位 + 跳过逻辑；充分测试 |
| phantom source 体积过大 | 低 | 中 | Level 0/1/2/3 分级；.auz 中可选段 |
| LSP 性能下降 | 低 | 中 | SourceIndex 惰性加载；缓存 |
| 用户误编辑 phantom source 并保存 | 低 | 低 | 只读模式 + 编辑器标签提示 |

### 5.5 不做什么

- ❌ 不实现 phantom source 的编译（仅 IDE 导航用）
- ❌ 不实现 phantom source 的热重载（修改 phantom source 不影响编译）
- ❌ 不实现 stdlib 源码级别的断点调试（调试器仍断点在用户代码）
- ❌ 不实现 phantom source 的语法检查（编译器不解析 phantom source）
- ❌ 不改变现有 `StdDoc` 结构体（Phase 4 后逐步迁移）

---

## 六、附录

### 附录 A：与其他语言的 Phantom Source 对比

| 特性 | Java | Kotlin | Swift | Rust | Aura（本方案） |
|------|------|--------|-------|------|---------------|
| 基础类型有源码？ | ✅ JDK sources | ✅ kotlin stdlib | ✅ Swift stdlib | ✅ core/src | ✅ core/ |
| 源码随制品分发？ | ✅ `-sources.jar` | ✅ 内嵌 .kt | ✅ 含接口 | ✅ 含源码 | ✅ `.auz` 含 source/ |
| IDE 可跳转？ | ✅ JDK sources 附加 | ✅ 内建导航 | ✅ SourceKit | ✅ rust-analyzer | ✅ LSP VFS |
| 源码可修改？ | ❌ 只读 | ❌ 只读 | ❌ 只读 | ❌ 只读 | ❌ 只读 |
| 源码影响执行？ | ❌ | ❌ | ❌ | ✅（部分 crate） | ❌ |
| 源码格式 | `.class`（字节码） | `.kt`（源码） | `.swiftinterface` | `.rs`（源码） | `.aura`（源码） |

### 附录 B：Phantom Source 文件清单

#### 基础类型（19 个）

| 文件 | 对应 Ty | 对应 Value | 行数 |
|------|---------|-----------|------|
| `Any.aura` | Ty::Any | Ref(usize) | ~100 |
| `Nothing.aura` | Ty::Nothing | - | ~30 |
| `Unit.aura` | Ty::Unit | - | ~30 |
| `Int.aura` | Ty::Int | Value::Int(i64) | ~150 |
| `Long.aura` | Ty::Long | Value::Int(i64) | ~80 |
| `Short.aura` | Ty::Short | Value::Int(i64) | ~60 |
| `Byte.aura` | Ty::Byte | Value::Int(i64) | ~60 |
| `Float.aura` | Ty::Float | Value::Float(f64) | ~120 |
| `Double.aura` | Ty::Double | Value::Float(f64) | ~80 |
| `Boolean.aura` | Ty::Boolean | Value::Bool(bool) | ~80 |
| `Char.aura` | Ty::Char | Value::Str(Rc<str>) | ~80 |
| `String.aura` | Ty::String | Value::Str(Rc<str>) | ~250 |
| `List.aura` | Ty::List | Value::List | ~150 |
| `Map.aura` | Ty::Map | Value::Map | ~150 |
| `Array.aura` | Ty::Array | - | ~80 |
| `Function.aura` | Ty::Function | - | ~60 |
| `Type.aura` | - | - | ~80 |
| `prelu.aura` | - | - | ~80 |

#### 标准库（19 个模块）

| 模块 | 文件 | 函数数 | 行数 |
|------|------|-------|------|
| `aura.lang.std.Math` | `Math.aura` | 30 | ~180 |
| `aura.lang.std.String` | `String.aura` | 40 | ~200 |
| `aura.lang.std.IO` | `IO.aura` | 11 | ~100 |
| `aura.lang.std.Collections` | `Collections.aura` | 30 | ~180 |
| `aura.lang.std.FileSystem` | `FileSystem.aura` | 22 | ~150 |
| `aura.lang.std.Network` | `Network.aura` | 11 | ~100 |
| `aura.lang.std.Json` | `Json.aura` | 11 | ~100 |
| `aura.lang.std.Time` | `Time.aura` | 10 | ~100 |
| `aura.lang.std.Test` | `Test.aura` | 20 | ~150 |
| `aura.lang.std.Builtin` | `Builtin.aura` | 16 | ~120 |
| `aura.lang.std.Env` | `Env.aura` | 14 | ~100 |
| `aura.lang.std.Process` | `Process.aura` | 10 | ~100 |
| `aura.lang.std.Random` | `Random.aura` | 11 | ~100 |
| `aura.lang.std.Encoding` | `Encoding.aura` | 8 | ~80 |
| `aura.lang.std.Ascii` | `Ascii.aura` | 13 | ~100 |
| `aura.lang.std.Console` | `Console.aura` | 25 | ~150 |
| `aura.lang.std.Path` | `Path.aura` | 13 | ~100 |
| `aura.lang.std.Assert` | `Assert.aura` | 8 | ~80 |
| `aura.lang.std.Iter` | `Iter.aura` | 30 | ~200 |

### 附录 C：SourceIndex 示例

```json
{
  "version": 1,
  "type_defs": {
    "Int": { "uri": "aura://builtin/Int.aura", "line": 12, "col": 0, "end_line": 150, "end_col": 1 },
    "Float": { "uri": "aura://builtin/Float.aura", "line": 12, "col": 0, "end_line": 120, "end_col": 1 },
    "String": { "uri": "aura://builtin/String.aura", "line": 12, "col": 0, "end_line": 250, "end_col": 1 },
    "Any": { "uri": "aura://builtin/Any.aura", "line": 8, "col": 0, "end_line": 50, "end_col": 1 },
    "Animal": { "uri": "file:///project/main.aura", "line": 5, "col": 0, "end_line": 12, "end_col": 1 }
  },
  "function_defs": {
    "println": { "uri": "aura://builtin/prelu.aura", "line": 14, "col": 4, "end_line": 15, "end_col": 1 },
    "aura.lang.std.Math.sin": { "uri": "aura://stdlib/aura/math/Math.aura", "line": 84, "col": 4, "end_line": 85, "end_col": 1 },
    "aura.lang.std.Math.cos": { "uri": "aura://stdlib/aura/math/Math.aura", "line": 88, "col": 4, "end_line": 89, "end_col": 1 },
    "aura.lang.std.String.contains": { "uri": "aura://stdlib/aura/string/String.aura", "line": 45, "col": 4, "end_line": 46, "end_col": 1 }
  },
  "module_defs": {
    "aura.lang.std.Math": { "uri": "aura://stdlib/aura/math/Math.aura", "line": 1, "col": 0, "end_line": 1, "end_col": 0 },
    "aura.lang.std.String": { "uri": "aura://stdlib/aura/string/String.aura", "line": 1, "col": 0, "end_line": 1, "end_col": 0 }
  },
  "constant_defs": {
    "aura.lang.std.Math.PI": { "uri": "aura://stdlib/aura/math/Math.aura", "line": 16, "col": 4, "end_line": 16, "end_col": 35 }
  }
}
```

### 附录 D：LSP 请求/响应示例

#### 用户代码中跳转到 `sin`

```json
// 用户代码
// line 5: fun main() { val x = sin(1.0) }
//                          ↑ cursor here

// 请求
{
  "method": "textDocument/definition",
  "params": {
    "textDocument": { "uri": "file:///project/main.aura" },
    "position": { "line": 5, "character": 28 }
  }
}

// 响应
{
  "result": {
    "uri": "aura://stdlib/aura/math/Math.aura",
    "range": {
      "start": { "line": 84, "character": 4 },
      "end": { "line": 85, "character": 1 }
    }
  }
}
```

#### 用户代码中跳转到 `Int`

```json
// 请求
{
  "method": "textDocument/definition",
  "params": {
    "textDocument": { "uri": "file:///project/main.aura" },
    "position": { "line": 3, "character": 10 }
    // line 3: val x: Int = 42
  }
}

// 响应
{
  "result": {
    "uri": "aura://builtin/Int.aura",
    "range": {
      "start": { "line": 12, "character": 0 },
      "end": { "line": 150, "character": 1 }
    }
  }
}
```

#### 虚拟文件打开

```json
// 请求
{
  "method": "textDocument/didOpen",
  "params": {
    "textDocument": {
      "uri": "aura://stdlib/aura/math/Math.aura",
      "languageId": "aura",
      "version": 1,
      "text": ""
    }
  }
}

// 响应
{
  "result": {
    "text": "// aura://stdlib/aura/math/Math.aura\n// 标准库模块，不可修改\n\npackage aura.lang.std.Math\n\n/// 数学函数与常量模块。\n///\n/// @since 0.1\ninternal object Math {\n\n    /// 圆周率 π\n    static val PI: Float = 3.14159265f\n\n    /// 自然常数 e\n    static val E: Float = 2.71828182f\n\n    // ... 其他函数\n}\n",
    "readOnly": true,
    "languageId": "aura"
  }
}
```

---

## 七、总结

本方案通过 **Phantom Source Tree + Source Metadata** 机制，让 Aura 像 Java/Kotlin/Swift 一样支持 stdlib 和基础类型的源码可见性，核心特点：

1. **源码可见**：IDE 中 F12 跳转到 phantom source，查看完整 API 签名和文档
2. **零性能代价**：源码元数据仅存于编译期，VM/JIT/AOT 完全不感知
3. **向后兼容**：新增段为可选，旧版本忽略，新旧版本互操作
4. **单一真相源**：phantom source 是 API 表面的唯一真相，docgen 从中生成文档
5. **与现有方案兼容**：与 Any 基类、Std-Prelude、包格式三个方案完全兼容

**关键设计决策**：
- Phantom source 是**只读的**——用户不能修改，但可查看
- Phantom source **不参与编译**——编译器不解析它们
- Phantom source **不参与执行**——运行时不加载它们
- Phantom source **内嵌于制品**——`.auz` 中的 `source/` 段可分发
- SourceIndex 是**编译期产物**——嵌入 `.auc` 的可选段

---

*设计文档版本：v1.0*
*最后更新：2026-09-09*


