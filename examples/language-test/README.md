# Aura 语言特性全量测试 Demo

> 分阶段、可独立验证的语言特性覆盖清单。每个阶段文件独立可编译、可运行、可单独 `aura check`。

---

## 目录结构

```
examples/language-test/
├── README.md              ← 本文件（规划方案 + 验证指南）
├── 01-lexer.aura          ← Phase 1: 词法基础（字面量 / 运算符 / 插值 / 注释）
├── 02-types-variables.aura ← Phase 2: 类型与变量（待开发）
├── 03-functions.aura       ← Phase 3: 函数（待开发）
├── 04-control-flow.aura    ← Phase 4: 控制流（待开发）
├── 05-classes.aura         ← Phase 5: 类与对象（待开发）
├── 06-null-safety.aura     ← Phase 6: 空安全（待开发）
├── 07-error-handling.aura  ← Phase 7: 错误处理（待开发）
├── 08-concurrency.aura     ← Phase 8: 并发（待开发）
├── 09-ffi.aura             ← Phase 9: FFI（待开发）
├── 10-memory.aura          ← Phase 10: 内存管理（待开发）
├── 11-imports.aura         ← Phase 11: 导入（待开发）
├── 12-annotations.aura     ← Phase 12: 注解与 Comptime（待开发）
├── 13-stdlib.aura          ← Phase 13: 标准库快照（待开发）
├── 14-string-interp.aura   ← Phase 14: 字符串插值（待开发）
├── 15-advanced.aura        ← Phase 15: 高级特性（待开发）
├── 16-script-mode.aura     ← Phase 16: 脚本模式（待开发）
└── run-all.sh              ← 一键测试全部阶段（待开发）
```

---

## 阶段划分与特性覆盖

### Phase 1 — 词法基础（✅ 已开发）

| 覆盖项 | 说明 |
|--------|------|
| **字面量** | Int（十进制 / 十六进制 `0xFF` / 二进制 `0b1100` / 下划线 `1_000`）、Long（`100L`）、Float（`3.14` / `3.14f`）、Double（`3.14d`）、Char（`'A'`）、Bool（`true` / `false`） |
| **字符串** | 普通 `"hello"`、转义 `\n\t`、插值 `$var` / `${expr}`、原始字符串 `"""..."""` |
| **运算符** | 算术 `+ - * / %`、复合赋值 `+= -= *= /= %=`、自增自减 `++ --`、比较 `== != < > <= >=`、逻辑 `&& \|\| !`、位运算 `& \| ^ << >> >>>`、空安全 `!!` |
| **分隔符** | `( ) { } [ ] , ; : . :: @` |
| **注释** | 单行 `//`、多行 `/* */`、文档 `///` |
| **关键字** | 全部 60+ 关键字（通过代码引用覆盖） |

**验证命令**：
```bash
aura check examples/language-test/01-lexer.aura              # 语法/语义检查
aura run examples/language-test/01-lexer.aura                 # VM 运行时验证
aura run examples/language-test/01-lexer.aura --jit           # JIT 模式验证
aura build examples/language-test/01-lexer.aura --aot --output /tmp/01-lexer && /tmp/01-lexer  # AOT 验证
aura tokens examples/language-test/01-lexer.aura              # 词法分析输出
```

---

### Phase 2 — 类型与变量（待开发）

| 覆盖项 |
|--------|
| `val` / `var` / `lateinit var` / `val by lazy` |
| 类型推断 vs 显式类型 |
| 可空类型 `Int?` + `typealias` |
| 类型转换 `as` / `is` |

---

### Phase 3 — 函数（待开发）

| 覆盖项 |
|--------|
| 表达式体 / 块体 / 默认参数 / 命名参数 / `vararg` |
| 泛型（单参 / 多参 / 带约束 `<T: Comparable<T>>`） |
| Lambda `{ x -> x*2 }` / 函数类型 `(Int)->Int` |
| 修饰符：`suspend` / `async` / `inline` / `comptime` / `override` |
| 可见性：`public` / `private` / `protected` |

---

### Phase 4 — 控制流（待开发）

| 覆盖项 |
|--------|
| `if-else`（作为表达式） |
| `when`（字面量 / `in range` / `is` 智能转换 / `else` / 守卫 `&&`） |
| `for` / `while` / `do-while` |
| 标签循环 + `break@label` / `continue@label` |

---

### Phase 5 — 类与对象（待开发）

| 覆盖项 |
|--------|
| `struct` / `data struct` / `sealed struct` |
| `value class` / `value data class` / `sealed value class` |
| `class`（继承 + `override` + `init` + `this` + `super`） |
| `interface`（含默认实现） |
| `enum`（单元 + 带数据变体） |
| `actor` |

---

### Phase 6 — 空安全（待开发）

| 覆盖项 |
|--------|
| `?.` 安全调用 / `?:` elvis / `!!` 强制解包 |
| 可空 vs 非空互转 |

---

### Phase 7 — 错误处理（待开发）

| 覆盖项 |
|--------|
| `try { } catch (e: Type) { } finally { }` |
| `throw` 表达式 |
| `Result<T,E>` + `case` pattern 匹配 |

---

### Phase 8 — 并发（待开发）

| 覆盖项 |
|--------|
| `suspend fun` + `await` |
| `async { }` 块 |
| Actor：`spawnActor` / `send` / `ask` / `supervise` / `actorAlive` |
| Channel：`newChannel` / `channelSend` / `channelRecv` / `channelTryRecv` |
| `select` 多路复用 |
| 协程 `spawn` + `await` 组合 |

---

### Phase 9 — FFI（待开发）

| 覆盖项 |
|--------|
| `extern "c" { }` 匿名块 |
| `extern "c" "lib" { }` 具名库 |
| `extern "rust" { }` |
| `Handle` / `Pointer<T>` / `ptrIsNull` / `ptrToInt` / `intToPtr` |
| `makeCallback` 回调 |

---

### Phase 10 — 内存管理（待开发）

| 覆盖项 |
|--------|
| `defer { }` 延迟清理 |
| ARC 引用计数（create + release 路径） |
| `Box<T>` 装箱 |

---

### Phase 11 — 导入（待开发）

| 覆盖项 |
|--------|
| 通配 `import module.*` |
| 模块 `import module` |
| 精确 `import module.fn` |
| 别名 `import module.fn as alias` / `import module as alias` |

---

### Phase 12 — 注解与 Comptime（待开发）

| 覆盖项 |
|--------|
| `@Annotation` / `@Deprecated` |
| `comptime fun`（编译时执行） |

---

### Phase 13 — 标准库快照（待开发）

| 覆盖项 |
|--------|
| `aura.math` / `io` / `string` / `collections` / `json` / `time` |
| `env` / `random` / `encoding` / `ascii` / `path` / `iter` / `net` / `fs` |

---

### Phase 14 — 字符串插值（待开发）

| 覆盖项 |
|--------|
| `$var` 简单插值 / `${expr}` 表达式插值 / raw string |

---

### Phase 15 — 高级特性（待开发）

| 覆盖项 |
|--------|
| 解构声明 `val (a, b) = pair` |
| 迭代器 `list.filter { }.map { }.take(n)` |
| `this` / `super` 引用 |

---

### Phase 16 — 脚本模式（待开发）

| 覆盖项 |
|--------|
| 顶层语句（无 `main` 也能执行） |

---

## 验证指南

### 单阶段验证

```bash
# 1. 语法/语义检查（零错误）
aura check examples/language-test/NN-phase.aura

# 2. 词法分析（Phase 1 专用）
aura tokens examples/language-test/NN-phase.aura

# 3. AST 输出
aura ast examples/language-test/NN-phase.aura

# 4. 字节码编译 + VM 执行
aura run examples/language-test/NN-phase.aura

# 5. JIT 模式
aura run examples/language-test/NN-phase.aura --jit

# 6. AOT 编译为原生可执行文件
aura build examples/language-test/NN-phase.aura --aot --output /tmp/nn-phase
# 运行 AOT 产物
/tmp/nn-phase

# 7. AOT 仅生成 LLVM IR
aura build examples/language-test/NN-phase.aura --aot --emit-llvm

# 8. 格式化检查
aura fmt examples/language-test/NN-phase.aura --check

# 9. ARC 泄漏检查
aura leak-check examples/language-test/NN-phase.aura
```

### 全部阶段验证

```bash
# 一键检查全部阶段（语法/语义）
for f in examples/language-test/*.aura; do
  echo "── Checking $f ──"
  aura check "$f" || exit 1
done

# 一键运行全部阶段（VM）
for f in examples/language-test/*.aura; do
  echo "── Running VM $f ──"
  aura run "$f" || exit 1
done

# 一键 AOT 编译 + 运行全部阶段
for f in examples/language-test/*.aura; do
  base=$(basename "$f" .aura)
  echo "── AOT compiling $f ──"
  aura build "$f" --aot --output "/tmp/${base}-aot" || exit 1
  echo "── Running AOT /tmp/${base}-aot ──"
  "/tmp/${base}-aot" || exit 1
done
```

### 编译器单元测试

```bash
# 全量单元测试
cargo test --workspace

# 仅示例集成测试
cargo test -p compiler --test examples_test

# 仅词法单元测试
cargo test -p compiler lexer

# 仅语法单元测试
cargo test -p compiler parser
```

---

## 设计原则

| 原则 | 说明 |
|------|------|
| **独立可验证** | 每个阶段文件包含完整的 `main()` 入口，可单独 `aura check` / `aura run` |
| **运行时验证** | 通过 `println` 输出实际值，而非仅编译通过 |
| **注释标注** | 每行标注对应阶段（P1–P16）便于追溯 |
| **边界覆盖** | 覆盖空安全边界（`null`）、范围边界（`0..10`） |
| **无外部依赖** | 不依赖外部 dylib 或网络，离线可运行 |
| **跨平台** | 不依赖特定 OS 的路径或命令 |

---

## 与现有示例的关系

| 现有文件 | 关系 |
|----------|------|
| `compiler/showcase.aura` | 本系列的超集，多了运行时验证和边界测试 |
| `basics/demo.aura` | Phase 1 覆盖其全部内容并扩展 |
| `stdlib/std_demo.aura` | Phase 13 是其精简快照 |
| `concurrency/concurrent_integration.aura` | Phase 8 覆盖其全部 7 个子场景 |
| `ffi/p8_c_ffi_demo.aura` | Phase 9 覆盖其全部 3 个场景 |
| `compiler/memory_test.aura` | Phase 10 覆盖其全部特性 |

---

## 风险与注意事项

| 风险 | 应对 |
|------|------|
| 部分语法仅"解析通过"但 sema 报错 | 先 `aura check`，若报错则降级为注释引用或移除 |
| 并发特性的 VM 执行时序不确定 | 使用 `select` 或确定性的 channel 收发顺序 |
| FFI 未链接 dylib 时返回占位值 | 按 `p8_c_ffi_demo.aura` 的做法，用 `ptrIsNull` 判空 |
| `case` pattern 可能未完全实现 | 若不支持，降级为 `is` + `when` |
| 文件过长导致可读性下降 | 每分区用 `// ═══ 标题 ═══` 分隔，分区独立函数 |

---

## 更新日志

| 日期 | 阶段 | 说明 |
|------|------|------|
| 2026-09-07 | Phase 1 | 词法基础 — 创建并验证通过 |
