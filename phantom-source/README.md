# Phantom Source Tree（虚拟源码树）

> Phase 1 交付物 — 标准库与基础类型源码可见性设计方案
>
> 设计文档：[docs/stdlib与基础类型源码可见性设计方案.md](../docs/stdlib与基础类型源码可见性设计方案.md)

## 概述

Phantom Source Tree 是一组**只读的 `.aura` 源码文件**，描述基础类型和 stdlib 的 API 表面。
它们**永不参与编译、永不参与执行**，仅用于 IDE 导航（LSP 跳转定义、悬停、补全）。

## 目录结构

```
phantom-source/
├── builtin/                     ← 18 个基础类型
│   ├── Any.aura                 ← 运行时基类/顶级类型
│   ├── Nothing.aura             ← 底部类型
│   ├── Unit.aura                ← 无返回值类型
│   ├── Int.aura                 ← 32 位整数
│   ├── Long.aura                ← 64 位长整数
│   ├── Short.aura               ← 16 位短整型
│   ├── Byte.aura                ← 8 位字节
│   ├── Float.aura               ← 32 位浮点数
│   ├── Double.aura              ← 64 位双精度浮点数
│   ├── Boolean.aura             ← 布尔
│   ├── Char.aura                ← 字符
│   ├── String.aura              ← 字符串
│   ├── List.aura                ← List<T> 集合
│   ├── Map.aura                 ← Map<K, V> 映射
│   ├── Array.aura               ← Array<T> 数组
│   ├── Function.aura            ← 函数类型
│   ├── Type.aura                ← 反射类型
│   └── prelu.aura               ← 17 个免 import 函数
│
└── stdlib/                      ← 19 个标准库模块
    └── aura/
        ├── math/Math.aura
        ├── string/String.aura
        ├── io/IO.aura
        ├── collections/Collections.aura
        ├── fs/FileSystem.aura
        ├── net/Network.aura
        ├── json/Json.aura
        ├── time/Time.aura
        ├── test/Test.aura
        ├── builtin/Builtin.aura
        ├── env/Env.aura
        ├── process/Process.aura
        ├── random/Random.aura
        ├── encoding/Encoding.aura
        ├── ascii/Ascii.aura
        ├── console/Console.aura
        ├── path/Path.aura
        ├── assert/Assert.aura
        └── iter/Iter.aura
```

## URI Scheme

每个 phantom source 文件对应一个 `aura://` 虚拟 URI：

| URI | 对应文件 |
|-----|---------|
| `aura://builtin/Int.aura` | `builtin/Int.aura` |
| `aura://stdlib/aura/math/Math.aura` | `stdlib/aura/math/Math.aura` |
| `aura://prelude/prelu.aura` | `builtin/prelu.aura` |

## 与编译器的关系

- **编译期**：docgen 阶段从 phantom source 提取符号，生成 `SourceIndex`
- **LSP 阶段**：读取 `SourceIndex`，通过 `aura://` URI 服务虚拟文件
- **执行期**：VM/JIT/AOT **完全不读取** phantom source 或 SourceIndex
- **运行时代价**：**零**——元数据仅在编译期生成，对执行路径完全不可见

## 与实现的对应

| phantom source 模块 | 实际实现（Rust/C） |
|--------------------|-------------------|
| `builtin/Int.aura` | `compiler/src/sema/ty.rs` (Ty::Int) + `compiler/src/vm/value.rs` (Value::Int) |
| `stdlib/aura/math/Math.aura` | `compiler/src/std/std_math.rs` |
| `stdlib/aura/string/String.aura` | `compiler/src/std/std_string.rs` |
| `stdlib/aura/io/IO.aura` | `compiler/src/std/std_io.rs` |
| ... | ... |

## 只读约束

- Phantom source 是**只读的**——用户不能修改
- 编译器**不解析** phantom source
- 修改 phantom source **不影响编译结果**
- IDE 打开 phantom source 时标记为只读
