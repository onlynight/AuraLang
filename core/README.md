# Phantom Source Tree（虚拟源码树）

> Phase 1 交付物 — 标准库与基础类型源码可见性设计方案
>
> 设计文档：[docs/stdlib与基础类型源码可见性设计方案.md](../docs/stdlib与基础类型源码可见性设计方案.md)

## 概述

Phantom Source Tree 是一组**只读的 `.aura` 源码文件**，描述基础类型和 stdlib 的 API 表面。
它们**永不参与编译、永不参与执行**，仅用于 IDE 导航（LSP 跳转定义、悬停、补全）。

自 v0.5 起，std 命名空间统一为 `aura.lang.std.<ClassName>`：

- `import aura.lang.std.Math`     → `Math.sin(3.0)`
- `import aura.lang.std.Math.*`   → `sin(3.0)`（短名）
- `import aura.lang.std.*`        → 只注册类名作模块别名

并发相关 API 拆分为三个模块（原 `aura.concurrent` 已废弃）：

| 旧命名                | 新命名                                |
|----------------------|-------------------------------------|
| `aura.concurrent.spawn`         | `aura.lang.std.Coroutine.spawn`    |
| `aura.concurrent.ask`           | `aura.lang.std.Coroutine.ask`      |
| `aura.concurrent.send`          | `aura.lang.std.Actor.send`         |
| `aura.concurrent.spawnActor`    | `aura.lang.std.Actor.spawnActor`   |
| `aura.concurrent.newChannel`    | `aura.lang.std.Channel.newChannel` |
| `aura.concurrent.channelSend`   | `aura.lang.std.Channel.channelSend` |
| `aura.concurrent.select`        | `aura.lang.std.Channel.select`     |

## 目录结构

```
phantom-source/
└── aura/
    └── lang/                      ← 语言包
        ├── Any.aura               ← 运行时基类/顶级类型
        ├── Nothing.aura           ← 底部类型
        ├── Unit.aura              ← 无返回值类型
        ├── Int.aura               ← 32 位整数
        ├── Long.aura              ← 64 位长整数
        ├── Short.aura             ← 16 位短整型
        ├── Byte.aura              ← 8 位字节
        ├── Float.aura             ← 32 位浮点数
        ├── Double.aura            ← 64 位双精度浮点数
        ├── Boolean.aura           ← 布尔
        ├── Char.aura              ← 字符
        ├── String.aura            ← 字符串（含所有字符串方法）
        ├── List.aura              ← List<T> 集合
        ├── Map.aura               ← Map<K, V> 映射
        ├── Array.aura             ← Array<T> 数组
        ├── Function.aura          ← 函数类型
        ├── Type.aura              ← 反射类型
        ├── Box.aura               ← 内存分配器
        ├── Weak.aura              ← 弱引用
        ├── prelu.aura             ← 34 个免 import 函数（含测试断言）
        │
        └── std/                   ← 标准库（22 个模块）
            ├── Math.aura          ← 数学函数与常量
            ├── IO.aura            ← 标准输入输出
            ├── String.aura        ← 字符串工具（与 lang/String.aura 方法分离）
            ├── Collections.aura   ← 集合辅助
            ├── FileSystem.aura    ← 文件系统
            ├── Network.aura       ← 网络 Socket
            ├── Json.aura          ← JSON 解析与序列化
            ├── Time.aura          ← 时间/日期
            ├── Test.aura          ← 测试断言
            ├── Builtin.aura       ← 编译期内 builtins
            ├── Env.aura           ← 环境变量
            ├── Process.aura       ← 进程管理
            ├── Random.aura        ← 随机数
            ├── Encoding.aura      ← 编码/解码
            ├── Ascii.aura         ← ASCII 字符工具
            ├── Console.aura       ← 终端控制
            ├── Path.aura          ← 路径操作
            ├── Assert.aura        ← 通用断言
            ├── Iter.aura          ← 迭代器/函数式工具
            │
            ├── Coroutine.aura     ← 协程（spawn, ask）
            ├── Actor.aura         ← Actor 模型（send, spawnActor, supervise, DeathStrategy, ProcessActor）
            └── Channel.aura       ← 消息通道（newChannel, channelSend, select, TCP）
```

## URI Scheme

每个 phantom source 文件对应一个 `aura://` 虚拟 URI：

| URI | 对应文件 |
|-----|---------|
| `aura:///aura/lang/Int.aura` | `aura/lang/Int.aura` |
| `aura:///aura/lang/prelu.aura` | `aura/lang/prelu.aura` |
| `aura:///aura/lang/std/Math.aura` | `aura/lang/std/Math.aura` |
| `aura:///aura/lang/std/IO.aura` | `aura/lang/std/IO.aura` |
| `aura:///aura/lang/std/Coroutine.aura` | `aura/lang/std/Coroutine.aura` |
| `aura:///aura/lang/std/Actor.aura` | `aura/lang/std/Actor.aura` |
| `aura:///aura/lang/std/Channel.aura` | `aura/lang/std/Channel.aura` |

## 与编译器的关系

- **编译期**：docgen 阶段从 phantom source 提取符号，生成 `SourceIndex`
- **LSP 阶段**：读取 `SourceIndex`，通过 `aura://` URI 服务虚拟文件
- **执行期**：VM/JIT/AOT **完全不读取** phantom source 或 SourceIndex
- **运行时代价**：**零**——元数据仅在编译期生成，对执行路径完全不可见

## 与实现的对应

| phantom source 模块 | 实际实现（Rust/C） |
|--------------------|-------------------|
| `aura/lang/Int.aura` | `compiler/src/sema/ty.rs` (Ty::Int) + `compiler/src/vm/value.rs` (Value::Int) |
| `aura/lang/String.aura` | `compiler/src/std/std_string.rs` |
| `aura/lang/prelu.aura` | `compiler/src/vm/native.rs` (prelude 注册) |
| `aura/lang/std/Math.aura` | `compiler/src/std/std_math.rs` |
| `aura/lang/std/IO.aura` | `compiler/src/std/std_io.rs` |
| `aura/lang/std/Coroutine.aura` | `compiler/src/vm/coroutine.rs` |
| `aura/lang/std/Actor.aura` | `compiler/src/vm/actor.rs` |
| `aura/lang/std/Channel.aura` | `compiler/src/vm/channel.rs` |
| ... | ... |

## Prelude 函数

免 import 的全局内置函数（34 个），同时以全名 `aura.lang.std.<fn>` 别名注册（可 import）：

| 类别 | 函数 |
|------|------|
| 输出 | `println`, `print`, `puts` |
| 数学 | `abs`, `sqrt`, `pow` |
| 转换 | `toInt`, `toFloat`, `toStr`, `toString` |
| 系统 | `clock`, `strlen`, `CString`, `CStr` |
| 指针 | `ptrIsNull`, `ptrToInt`, `intToPtr` |
| FFI | `makeCallback` |
| 集合 | `listOf` |
| 测试断言 | `assertTrue`, `assertFalse`, `assertEq`, `assertNotEq`, `assertNotNull`, `assertNull`, `assertContains`, `assertNotContains`, `assertGt`, `assertGte`, `assertLt`, `assertLte`, `assertApprox`, `assertArrayEq`, `assertMapEq`, `pass`, `fail` |

## 导入模式

```aura
// 精确：只引入 Math 类，用 Math.sin() 调用
import aura.lang.std.Math
val x = Math.sin(3.14)

// 通配短名：所有函数直接可用
import aura.lang.std.Math.*
val y = sin(3.14)

// 全 std：所有类作为模块别名（不做短名导入）
import aura.lang.std.*
val z = Math.sin(3.14)  // Math 作为别名
```

## 只读约束

- Phantom source 是**只读的**——用户不能修改
- 编译器**不解析** phantom source
- 修改 phantom source **不影响编译结果**
- IDE 打开 phantom source 时标记为只读
