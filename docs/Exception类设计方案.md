# AuraLang 异常类设计方案

## 一、当前现状分析

通过对 `compiler/src`、`aura/core`、`docs`、`examples` 全量扫描，当前异常子系统 **仅有一层语法外壳，无实体支撑**：

| 层级 | 现状 | 问题 |
|------|------|------|
| **词法/语法** | `throw` 为硬关键字；`try/catch/finally` 可解析 ✅ | — |
| **类型检查** | `checker.rs:2838` 检查 thrown 值为 `String` 或 `Ty::Named("Exception")` | **`Exception` 从未被定义为 class**——`Ty::Named("Exception")` 是幽灵类型，任何类型都通过 `can_assign_to` |
| **HIR 降级** | `throw x` → `__throw(x)` 原生调用；`try/catch` → `PushHandler/PopHandler` | 异常值原样传递，无包装 |
| **VM 解释器** | `interp.rs:1078` `raise()` 展开 handler 栈，截断帧栈，写入 catch 槽 | 无类型过滤——`catch (e: Exception)` 捕获**一切**（包括 `throw 42`）；无异常对象 |
| **AOT 后端** | `emit.rs:3456` `longjmp` 跳回最近 `setjmp`；`runtime.rs:617` `Runtime(msg)` 返回裸字符串指针 | `AotCallContext.exception` 仅为 `i32` 码；无异常值/类型/层次 |
| **JIT** | 无异常支持 | 无法从 JIT 代码抛出 |
| **标准库** | **无任何 Exception 类定义**——`Nothing.aura` 调用 `Exception("...")`、`ArrayList.aura` 调用 `IndexOutOfBounds("...")`、`Assert.aura`/`Test.aura` 直接 `throw "msg"` | 全部构造调用指向未定义类；异常为裸字符串，无 `.message`、无 `.cause`、无栈追踪 |

**核心问题**：语言有 `throw`/`try`/`catch` 语法，但 **运行时无任何异常对象**——抛出的值是裸字符串或裸值，捕获时无法区分类别，无法查询 `message`/`cause`/`stackTrace`，标准库中引用了 `Exception`、`IndexOutOfBounds`、`EmptyList` 等均未定义的类。

---

## 二、设计方案

### 2.1 异常类层次结构

采用 Java/Python 混合的简洁设计，**以 Aura 源码定义，纯逻辑嵌入标准库**：

```
Throwable                    // 不可构造基类（类似 Java）
├── Error                    // 系统级错误（用户通常不捕获）
│   ├── OutOfMemoryError
│   └── StackOverflowError
└── Exception                // 程序级错误（用户应捕获）
    ├── RuntimeException     // 非受检异常（Unchecked）
    │   ├── IllegalArgumentException
    │   ├── IllegalStateException
    │   ├── NullPointerException
    │   ├── IndexOutOfBoundsException
    │   │   ├── ArrayIndexOutOfBoundsException
    │   │   └── EmptyListException
    │   ├── UnsupportedOperationException
    │   ├── ArithmeticException
    │   └── ClassCastException
    ├── IOException
    ├── FileNotFoundException
    ├── TimeoutException
    └── AssertionError       // 测试断言失败
```

### 2.2 类定义（核心）

```aura
package aura.lang

/// 所有异常的基类——所有可抛出的异常必须是 Throwable 的子类。
open class Throwable(val message: String, val cause: Throwable?) {

    /// 栈追踪（帧索引列表），VM 在 raise 时自动填充
    val stackTrace: List<Int>

    /// 返回可读的异常描述
    override fun toString(): String {
        var result: String = this.class.toString() + ": " + message
        if (cause != null) {
            result = result + " caused by: " + cause.toString()
        }
        return result
    }
}

/// 系统级错误——程序无法恢复，通常不捕获
open class Error(val message: String) : Throwable(message, null) {}
open class OutOfMemoryError(val message: String) : Error(message) {}
open class StackOverflowError(val message: String) : Error(message) {}

/// 程序级异常的基类——用户应捕获
open class Exception(val message: String) : Throwable(message, null) {}

/// 非受检异常基类——编译期不强制捕获
open class RuntimeException(val message: String) : Exception(message) {}

open class IllegalArgumentException(val message: String) : RuntimeException(message) {}
open class IllegalStateException(val message: String) : RuntimeException(message) {}
open class NullPointerException(val message: String) : RuntimeException(message) {}
open class IndexOutOfBoundsException(val message: String) : RuntimeException(message) {}
open class ArrayIndexOutOfBoundsException(val message: String) : IndexOutOfBoundsException(message) {}
open class EmptyListException(val message: String) : IndexOutOfBoundsException(message) {}
open class UnsupportedOperationException(val message: String) : RuntimeException(message) {}
open class ArithmeticException(val message: String) : RuntimeException(message) {}
open class ClassCastException(val message: String) : RuntimeException(message) {}

open class IOException(val message: String) : Exception(message) {}
open class FileNotFoundException(val message: String) : IOException(message) {
    val path: String
}
open class TimeoutException(val message: String) : IOException(message) {}

/// 测试断言失败
open class AssertionError(val message: String) : Exception(message) {}
```

### 2.3 类型系统改动

**`checker.rs`**：

```rust
// 当前:
if !vt.is_string() && !vt.can_assign_to(&Ty::Named("Exception".into())) {

// 改为：
if !vt.is_string() && !vt.is_subtype_of("Exception") {
    self.report(*span, format!(
        "cannot throw value of type '{}': not a Throwable subtype", vt.name()
    ));
}
```

新增 `is_subtype_of` 方法（查 `symbols.type_hierarchy`）以替代当前的 `can_assign_to` 裸字符串匹配。

**`ty.rs`**：新增 `Ty::Named("Exception")` 的 `is_exception_like()` 判定——检查类层次继承链是否到达 `Throwable`。

**`check_try`**：
```rust
// 当前 catch 变量类型为 Ty::Named(type_name)，但无过滤。
// 改为：记录 catch 的 Throwable 类型 → 在 MIR 层生成类型检查指令：
//   catch (e: RuntimeException) → 展开到处理器时先 isInstance 检查，不匹配则继续向上查找。
```

### 2.4 VM `raise` 增强

```rust
// vm/interp.rs
fn raise(&mut self, value: Value) -> Result<(), VmError> {
    while let Some(h) = self.handlers.pop() {
        // 新增：类型过滤
        if let Some(filter_type) = h.catch_type {
            if !is_instance_of(&value, filter_type) {
                continue; // 类型不匹配，继续向上查找
            }
        }
        // ... 原有的帧截断、槽位写入、IP 跳转逻辑
    }
    Err(VmError::Runtime(format!("uncaught exception: {}", value)))
}
```

`Handler` 增加 `catch_type: Option<Ty>` 字段；MIR 的 `PushHandler` 指令增加类型参数。

### 2.5 AOT 后端

替换 `longjmp` 方案为 **结构化异常 + 虚方法表过滤**：

```
throw Runtime("msg")
  → 创建 Exception 对象 (heap alloc, vtable, message, stackTrace)
  → 沿 handler 栈查找 isInstance 匹配的 catch
  → 写入 catch 槽 + 跳转
```

`AotCallContext.exception` 从 `i32` 改为 `i8*`（异常对象指针）。

### 2.6 标准库文件组织

```
aura/core/aura/lang/
├── Throwable.aura           // 基类
├── Exception.aura           // 程序异常基类
├── RuntimeException.aura    // 非受检异常基类
├── errors/
│   ├── IllegalArgument.aura
│   ├── IllegalState.aura
│   ├── NullPointer.aura
│   ├── IndexOutOfBounds.aura
│   ├── EmptyList.aura
│   ├── UnsupportedOp.aura
│   ├── Arithmetic.aura
│   └── ClassCast.aura
├── io/
│   ├── IOException.aura
│   ├── FileNotFound.aura
│   └── Timeout.aura
└── AssertionError.aura
```

嵌入 `embedded_stdlib.rs` 注册。

### 2.7 迁移路径

| 阶段 | 内容 | 影响 |
|------|------|------|
| **P1** | 定义异常类层次（纯 Aura 源码 + embedded_stdlib）| 新文件，零改动现有代码 |
| **P2** | 类型检查收紧：`throw` 只接受 Throwable 子类 | `checker.rs` ~10 行 |
| **P3** | VM 异常过滤：Handler 增加 catch_type + isInstance 检查 | `interp.rs` ~30 行 |
| **P4** | MIR/AOT：`PushHandler` 指令携带类型信息 | `mir.rs` + `emit.rs` ~40 行 |
| **P5** | 标准库迁移：`ArrayList`/`Assert`/`Test` 改用异常对象 | 各文件 `throw IndexOutOfBounds("...")` → `throw IndexOutOfBoundsException("...")` |
| **P6** | `stackTrace` 自动填充（VM raise 时写入帧索引列表）| `interp.rs` ~15 行 |

---

## 三、与 Java/Kotlin/Python 对比

| 维度 | Java | Python | 本方案 |
|------|------|--------|------|
| 类型检查 | 编译期检查受检异常 | 无 | **可选类型过滤**（VM 展开时 isInstance） |
| 异常链 | `getCause()` | `__context__` | `cause: Throwable?` |
| 非受检异常 | `RuntimeException` | 所有异常 | `RuntimeException` 子类 |
| 栈追踪 | `fillInStackTrace()` | `traceback` 模块 | `stackTrace: List<Int>`（帧索引） |
| 多 catch | ✅ | ✅ | ✅（Handler 栈 + 类型过滤） |
| `as` 强制转换异常 | `ClassCastException` | `TypeError` | `ClassCastException`（AOT 已有） |

---

## 四、关键设计决策

1. **异常对象是 class 而非 struct**——异常需要引用语义（传递身份），且 `.message`、`.cause`、`.stackTrace` 字段需要可变性（`stackTrace` 在抛出时填充）。

2. **catch 类型过滤是运行时的，不是编译期的**——编译期只检查 `throw` 值是 Throwable 子类；catch 的类型匹配在 VM 展开时通过虚方法表 `isInstance` 判定。这与 Java 一致，避免引入复杂的类型推导。

3. **`throw` 仍接受 String 字面量**——为兼容现有 `Assert.aura`、`Test.aura`、`07-error-handling.aura` 等使用 `throw "msg"` 的代码，在 HIR 降级时自动包装为 `Exception("msg")`。

4. **AOT 改用结构化方案替代 `longjmp`**——`longjmp` 绕过 GC 和 Rust 安全边界，且有平台依赖。改为 handler 栈 + 虚方法过滤，与 VM 一致。

---

## 五、实施原则

1. **Aura 自举编译器注意不要引用 Rust 使用 native 实现**——所有异常类定义在 `aura/core/aura/lang/` 下，通过嵌入 `.auc` 被 VM 加载执行，不依赖任何 Rust native 函数。
2. **Rust 编译器要支持**——VM 的 `raise`、`isInstance` 检查、MIR/AOT 发射等基础设施改动在 Rust 侧完成。
3. **纯逻辑优先**——异常类的构造、继承、`toString`、`getMessage` 等方法全部用 Aura 实现，只有底层原语（`Memory.alloc`、虚方法表派发）使用 native。
