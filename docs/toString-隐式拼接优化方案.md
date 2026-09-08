# toString 隐式拼接优化方案

> 参考 Java (JLS §15.18.1) / Kotlin (`String.plus(other: Any?)`)，允许 `String + T` 隐式调用 `toString()`，免去手写 `.toString()`。

---

## 现状分析

| 层 | 位置 | 对 `String + Int` 的支持 |
|---|---|---|
| **Sema** | `checker.rs:1854-1876` | ✅ 已允许。`lt==Ty::String \|\| rt==Ty::String` → 返回 `Ty::String` |
| **HIR 降级** | `hir.rs:2673-2677` | ❌ 只生成 `Binary{Add, lhs, rhs}`，**不插入 `toString` 转换** |
| **VM 解释器** | `interp.rs:1199-1203` | ✅ `format!("{}{}", a, b)` 已支持混合拼接 |
| **AOT LLVM** | `aot/emit.rs:1477-1487` | ⚠️ 检测到一个侧是字符串后调用 `extract_string_parts`，对非字符串侧走 fallback 把 i32 当 i8* 调 `aura_string_length` —— **LLVM IR 非法** |
| **AOT C 后端** | `c_backend.rs:428-450` | ❌ `Add` 一律输出 `+`，不支持字符串拼接 |
| **JIT / 优化器** | `jit.rs` / `jit_opt.rs` / `opt.rs` | 未涉及，需跟进 |

**结论**：Sema 和 VM 已就绪，阻碍集中在 HIR→AOT 的降级路径。现有示例代码全部手写 `.toString()` 不是因为 Sema 拒绝，而是因为 AOT 路径目前走不通。

## Java / Kotlin 参考

- **Java**：`+` 是编译期运算符，遇到字符串侧即触发 `String.valueOf(Object)` 或 `StringBuilder`，无需手写。JLS §15.18.1 规定：当且仅当操作数之一为 `String` 时才执行拼接，且另一侧走隐式转换。
- **Kotlin**：`+` 是函数式扩展（`fun String.plus(other: Any?): String = "$this$other"`），同样是 Sema 识别后直接降级为模板插值。
- **共同点**：转换在**编译器层**完成（HIR/IR 插入），而非在运行时类型分派。

## 技术方案（分层）

### 第 1 层：HIR 降级（核心改动）

**位置**：`compiler/src/codegen/hir.rs` 中 `desugar_expr` 的 `Expr::Binary` 分支（~2673 行）。

在生成 `HirExpr::Binary` 前，检测 `op == BinOp::Add` 且左右类型**非双数值**。若一侧为 `Ty::String` 而另一侧非 `String`，把非字符串侧包装为 `HirExpr::Call { callee: "aura.builtin.toString", args: [原值] }`。

```rust
// 伪代码
HirExpr::Binary {
    op: HirBinOp::from_ast(*op),
    lhs: Box::new(desugar_expr(lhs)),
    rhs: Box::new(desugar_expr(rhs)),
}
// ↓ 改为
let lhs_h = desugar_expr(lhs);
let rhs_h = desugar_expr(rhs);
let (lhs_h, rhs_h) = insert_implicit_tostring(*op, &lhs_h, &rhs_h, lhs_ty, rhs_ty);
HirExpr::Binary { op: ..., lhs: Box::new(lhs_h), rhs: Box::new(rhs_h) }
```

`insert_implicit_tostring` 规则：
- `Add` + 一侧 String + 另一侧非 String → 包装 `toString`
- `Add` + 一侧 Any + 另一侧 String → 包装 `toString`（与 Sema 现有 `Ty::Any` 语义对齐）
- 其他情况不动

**收益**：所有后端（LLVM / C / 未来其他）只需处理纯字符串或纯数值两种输入，混合拼接统一在 HIR 层消除。

### 第 2 层：AOT LLVM 后端（防御性）

**位置**：`compiler/src/codegen/aot/emit.rs:1477-1487`。

若 HIR 层已做隐式转换，理论上这里不会再遇到混合类型。但为防御性考虑，将 `extract_string_parts` 的 fallback 改为：对非字符串侧调用 `aura_string_from_int` / `aura_string_from_float` / `aura_string_from_bool` 等新 runtime 函数。

**新增 runtime 函数**（`runtime.rs` + C FFI `aura_std_cffi.c`）：

| 名称 | 签名 |
|---|---|
| `aura_string_from_int` | `i8* (i32)` |
| `aura_string_from_float` | `i8* (double)` |
| `aura_string_from_bool` | `i8* (i1)` |
| `aura_string_from_ptr` | `i8* (i64)` |
| `aura_string_from_value` | `i8* (Value)` — 兜底 |

### 第 3 层：AOT C 后端

**位置**：`compiler/src/codegen/aot/c_backend.rs:428-450`。

`Add` 需区分：若左右类型均为字符串 → 调 `aura_string_concat`；若一侧字符串 → 先调对应 `aura_string_from_*` 再 concat；均为数值 → 输出 `+`。需要把 `emit_c_binop` 升级为接收类型信息（或独立处理 Add 分支）。

### 第 4 层：JIT / 优化器

- `jit.rs:553` / `jit_opt.rs:260`：`Instr::Add` 当前只走整数/浮点路径。需在 HIR 降级后确保 JIT 看到的是纯类型，无需改动。
- `opt.rs:183-192` 的常量折叠需新增 `(Add, Str, Str) → Str` 字面量拼接常量折叠。

### 第 5 层：Sema（可选收紧）

**位置**：`checker.rs:1854-1876`。

现有逻辑已允许混合拼接，无需必改。但建议**增加警告**（非错误）提示"已自动调用 toString"，便于迁移期用户感知，类似 Kotlin 的 `@Deprecated` 迁移期策略。

### 第 6 层：测试

1. **sema_tests** 新增：`Int + String`、`Float + String`、`Boolean + String`、`List + String`、`null + String`、`Any + String`、混合链式 `"a" + 1 + true + 2.0`。
2. **VM 集成测试**：跑 `examples/` 下替换 `.toString()` 为隐式拼接的版本，验证输出一致。
3. **AOT 集成测试**：编译同一段代码，验证 LLVM IR 合法 + 运行输出一致。
4. **回归测试**：保留 `01-lexer.aura` 等现有示例，确认手写 `.toString()` 仍工作。

### 第 7 层：文档

- `docs/01-aura-language-card.md` 运算符表新增 `+ (String + T) → String`。
- `docs/api/std_string.md` 补充隐式转换说明。
- `book/chapter-*.md` 教程更新示例，去掉手写 `.toString()`。
- `docs/技术方案.md` 记录设计决策与 Java/Kotlin 对齐说明。

## 关键风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| **`Float` 显示格式不一致** | `interp.rs:190` 现把 `3.0` 显示为 `"3.0"`，Java 也是 `"3.0"`，但 `0.0` → `"0.0"`，与 Kotlin 一致。但 `1e20` 显示 `"1e20"` vs Java `"1.0E20"` | 在 `Value::Display` 对齐 Java `Float.toString` / `Double.toString` 语义 |
| **`null + String` 语义** | Java: `"x" + null` → `"xnull"`；当前 `Value::Null` Display 为 `"null"` ✓ | 无需改动，但需明确文档 |
| **`List<T> + String`** | 现 `Value::List` Display 为 `"[1, 2, 3]"`，Java `List.toString()` 也是。但 `List<Int>.toString()` 与 Kotlin 一致 | 无需改动 |
| **`Char + String` vs `Int + Int`** | `Char` 在 AuraLang 是否为独立类型？若是，`'a' + 1` 应走拼接还是算术？ | 需明确语义：建议 `Char + String → String`，`Char + Int` 报错（避免 `1 + 2 + 'a'` 歧义） |
| **运算符重载冲突** | 若用户类定义了 `plus`，`MyClass + String` 该走重载还是隐式 toString？ | 优先级：重载 > 隐式 toString（保持 `operator_overload_return` 现有行为） |
| **可空类型** | `String? + Int`：Sema 当前报错"cannot apply operator on nullable"。Java 允许 | 需决策：建议保持现状（显式解包），或允许隐式 toString 但 `null` → `"null"` |
| **性能** | 隐式 `toString` 在循环内可能引入大量字符串分配 | AOT 后端可做**循环不变量外提**和**StringBuilder 优化**（后续阶段） |
| **`Any` 侧的虚分派** | `Any + String`：`Any` 可能是引用类型，toString 行为取决于运行时类型 | VM 已有 `Display` 兜底；AOT 需 `aura_string_from_value` 分派表 |
| **迁移成本** | 现有 222 处手写 `.toString()` 的示例代码 | 保留兼容（手写 `.toString()` 仍有效），新代码推荐省略。分阶段清理示例 |
| **C 后端覆盖不全** | C 后端目前不支持任何字符串操作 | 需评估：C 后端是否仍在使用？若已废弃可延后；若在用需补齐 |

## 可行性结论

**可行，且改动可控。**

- ✅ Sema 与 VM 已就绪，是**纯编译器后端**的改动。
- ✅ 改动集中在 HIR 降级（单点）+ AOT LLVM（已有骨架）+ AOT C（需新增字符串支持）。
- ✅ 手写 `.toString()` 仍兼容，无需迁移现有代码。
- ⚠️ 主要工作量在 C 后端（若仍在维护）和 runtime 新增 5 个字符串化函数。
- ⚠️ 需要决策 3 个语义点：`Char` 处理、可空类型是否放开、`Any` 虚分派策略。

**建议实施顺序**：
1. HIR 层 `insert_implicit_tostring`（单文件改动，~50 行）
2. AOT LLVM 后端 fallback 修复 + 新增 runtime 函数
3. 测试补齐（sema + VM + AOT 三层）
4. AOT C 后端补齐（若仍在使用）
5. 文档与示例更新
