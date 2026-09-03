# Aura 编程语言

> 为 NovaOS 从零构建的系统级脚本语言 —— Rust 实现、Kotlin 风格语法、AOT + JIT 混合编译、零开销 FFI、ARC 内存管理。

详细设计见 [技术方案.md](技术方案.md)，阶段规划与进度见 [开发规划与实现进度.md](开发规划与实现进度.md)。

---

## 当前状态

编译器前端（词法 → 语法 → 语义）已完成并可运行，后端（字节码 / VM / AOT）尚未开始。

| 阶段 | 内容 | 状态 |
|------|------|------|
| P0 | 基础设施（workspace、SourceMap、诊断、测试、CI、文档） | ✅ |
| P1 | 词法分析器 | ✅ |
| P2 | 语法分析器 + AST | ✅ |
| P3 | 语义分析（类型推断、空安全、诊断） | ✅ 基础版 |
| P4+ | 字节码编译 / VM / AOT / 内存管理 / FFI / 标准库 … | ⬜ 未开始 |

---

## 仓库结构

```text
aura-compiler/            编译器库（前端：lexer / parser / sema）
  src/lexer.rs            词法分析器（手写，支持插值、原始字符串、文档注释）
  src/token.rs            Token 定义
  src/ast.rs              AST 节点定义
  src/parser.rs           递归下降 + Pratt 优先级解析器
  src/sema/               语义分析（ty.rs 类型 / symbol.rs 符号表 / checker.rs 检查器）
  src/source_map.rs       源码映射（字节偏移 ↔ 行列、源码片段渲染）
  src/errors.rs           诊断错误类型
  src/span.rs             源码位置
  tests/sema_tests.rs     语义集成测试
  tests/snapshots.rs      insta 快照测试（Token 流 / AST / 诊断）
  tests/perf_lexer.rs     词法性能基准
aura-cli/                 命令行工具（tokens / parse / check）
examples/                 示例 Aura 源码
```

## 构建与测试

```bash
cargo build --workspace
cargo test --workspace

# 词法吞吐率基准（release 下断言 > 10 MB/s）
cargo test --release --test perf_lexer -- --nocapture

# 快照测试：更新快照
INSTA_UPDATE=always cargo test
```

CI 在 `.github/workflows/ci.yml`，包含 `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`（debug + release）与覆盖率采集。

## 命令行用法

```bash
cargo run -p aura-cli -- check examples/demo.aura      # 词法 + 语法 + 语义检查
cargo run -p aura-cli -- tokens examples/demo.aura     # 打印 Token 流
cargo run -p aura-cli -- parse examples/demo.aura      # 打印 AST
```

诊断输出带源码片段：

```text
semantic error: type mismatch: cannot initialize 'Int' with 'String'
 --> examples/demo_errors.aura:8:6
  |
8 | val bad: Int = "not a number"
  |     ^^^
```

## 已支持的语言特性（前端）

- 变量：`val` / `var` / `lateinit var` / `val x by lazy { ... }` / 解构 `val (a, b) = pair`
- 函数：默认参数、命名参数、可变参数、表达式体、泛型与约束 `fun <T : Comparable<T>>`
- 类型：`Int` `Long` `Float` `Double` `Boolean` `Char` `String` `Any` `Unit` `Nothing`、可空 `T?`、数组 `T[]`、函数类型、泛型 `List<T>`、类型别名 `typealias`
- 声明：`struct` / `data struct` / `sealed struct` / `class` / `data class` / `sealed class` / `interface` / `enum` / `actor` / `extern "c"` / `import`
- 控制流：`if` 表达式、`when`（分支 / 范围 `in 90..100` / 类型匹配 `is T` / 守卫 `&&`）、`for`、`while`、`do-while`、`break` / `continue`
- 空安全：`?.`、`?:`（Elvis）、`!!`
- 字符串：插值 `$name` / `${expr}`、转义序列、原始多行字符串 `"""..."""`
- 文档注释：`///` 与 `/** */`（收集到 AST 的 `doc` 字段）
- 注解：`@Deprecated` 等；修饰符：`suspend` / `inline` / `override` / `comptime`

## 示例

```aura
/// 玩家结构体
struct Player(val id: Int, var name: String, var health: Int = 100)

fun greet(name: String): String {
    return "hello $name"
}

fun grade(score: Int): String {
    return when (score) {
        in 90..100 -> "A"
        in 80..89  -> "B"
        else       -> "F"
    }
}

val raw = """
原始字符串不做转义，也不做 $ 插值
"""
```

更完整的“当前已支持”语法与语义覆盖见 [`examples/showcase.aura`](examples/showcase.aura)；其编译零错误由 [`aura-compiler/tests/examples_test.rs`](aura-compiler/tests/examples_test.rs) 端到端校验。

## 路线图

见 [开发规划与实现进度.md](开发规划与实现进度.md)：P4 字节码编译器 → P5 Aura VM + JIT → P6 AOT（LLVM/inkwell）→ … → P13 工具链与 IDE。

## 许可证

MIT
