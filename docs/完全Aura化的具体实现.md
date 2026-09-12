# 完全 Aura 化的具体实现

> **版本**: 1.0  
> **日期**: 2026-07-05  
> **状态**: 阶段 1 开发中  
> **目标**: 消除核心类型和标准库对 Rust native 的依赖，实现完整的 Aura 自举

---

## 一、当前状态分析

### 1.1 核心问题

**当前状态**: 大量方法仅有签名声明，无 Aura 实现，依赖 Rust native 兜底  
**目标状态**: 所有核心类型和标准库函数在 Aura 中完整实现

### 1.2 问题分类

#### 1.2.1 核心类型文件（aura/core/aura/lang/）

| 文件 | 问题描述 | 虚方法数量 |
|------|---------|-----------|
| Any.aura | `toString()`, `equals()`, `hashCode()`, `typeOf()` 无实现 | 4 |
| Int.aura | `toLong()`, `toFloat()`, `abs()`, `compareTo()` 等无实现 | ~20 |
| Float.aura | `toInt()`, `toDouble()`, `ceil()`, `floor()` 等无实现 | ~15 |
| Boolean.aura | `not()`, `and()`, `or()` 等无实现 | ~8 |
| String.aura | `length`, `substring()`, `indexOf()` 等无实现 | ~25 |
| List.aura | `add()`, `get()`, `size`, `remove()` 等无实现 | ~20 |
| Map.aura | `put()`, `get()`, `remove()`, `containsKey()` 等无实现 | ~15 |
| 其他 | Byte/Char/Long/Short/Double/Array/Function/Type/Unit/Nothing | ~50 |

**问题根源**: 这些是"接口定义"而非"实现"，依赖 `compiler/src/std/std_*.rs` 中的 Rust native 函数兜底。

#### 1.2.2 标准库文件（aura/core/aura/lang/std/）

| 文件 | 问题描述 |
|------|---------|
| Math.aura | `ceil()`, `floor()`, `round()`, `sqrt()`, `sin()`, `cos()` 等无实现 |
| Builtin.aura | `typeof()`, `isNull()`, `isZero()`, `isPositive()` 等无实现 |
| Encoding.aura | 部分实现，但 URL 编码/解码依赖未验证 |
| Path.aura | 部分实现，`normalize()` 逻辑可能不完整 |

#### 1.2.3 编译器基础设施（aura/compiler/aura/lang/compiler/）

| 文件 | 问题描述 |
|------|---------|
| Vm.aura | `interpret()` 返回 null，仅为占位实现 |
| Gc.aura | `malloc()`, `free()`, `mark()`, `sweep()` 均为占位 |
| 其他 | 均为 Phase 4 占位代码，非完整实现 |

---

## 二、修改方案

### 2.1 阶段划分

```
阶段 1: 核心类型 Aura 实现（当前）
    ├── Any.aura          → 实现 toString/equals/hashCode（基于 Value tag）
    ├── Int.aura          → 实现算术/位运算/转换（纯逻辑）
    ├── Float.aura        → 实现算术/取整/转换（纯逻辑）
    ├── Boolean.aura      → 实现逻辑运算/转换
    ├── String.aura       → 实现字符串操作
    ├── List.aura         → 实现集合操作
    └── Map.aura          → 实现映射操作

阶段 2: 标准库函数 Aura 实现（后续）
    ├── Math.aura         → 实现 ceil/floor/round/sqrt（需要 libm FFI）
    ├── Builtin.aura      → 实现 typeof/isNull/isZero（需要 VM 支持）
    └── String.aura       → 补充缺失的方法

阶段 3: 编译器基础设施完善（Phase 4）
    ├── Vm.aura           → 实现完整字节码解释器
    ├── Gc.aura           → 实现标记-清除 GC
    └── memory/           → 实现内存管理
```

### 2.2 依赖关系图

```
阶段 1: 核心类型 Aura 实现
    ├── Any.aura (基础)
    ├── Int.aura, Float.aura, Boolean.aura (基本类型)
    └── String.aura, List.aura, Map.aura (集合类型)
            │
            ▼
阶段 2: 标准库函数 Aura 实现
    ├── Math.aura (依赖 Float)
    ├── Builtin.aura (依赖 VM)
    └── String.aura (依赖 String)
            │
            ▼
阶段 3: 编译器基础设施
    ├── Vm.aura (依赖所有类型)
    └── Gc.aura (依赖内存管理)
```

---

## 三、具体修改清单

### 3.1 阶段 1：核心类型文件修改

| 文件 | 需要实现的方法 | 预估工作量 | 状态 |
|------|--------------|-----------|------|
| Any.aura | toString/equals/hashCode | 小 | ⏳ 待实现 |
| Int.aura | 20+ 方法 | 中 | ⏳ 待实现 |
| Float.aura | 15+ 方法 | 中 | ⏳ 待实现 |
| Boolean.aura | 8+ 方法 | 小 | ⏳ 待实现 |
| String.aura | 25+ 方法 | 大 | ⏳ 待实现 |
| List.aura | 20+ 方法 | 中 | ⏳ 待实现 |
| Map.aura | 15+ 方法 | 中 | ⏳ 待实现 |

### 3.2 阶段 2：标准库文件修改

| 文件 | 需要实现的方法 | 预估工作量 | 状态 |
|------|--------------|-----------|------|
| Math.aura | ceil/floor/round/sqrt/pow | 中 | ⏳ 待实现 |
| Builtin.aura | typeof/isNull/isZero/isPositive | 小 | ⏳ 待实现 |
| String.aura | startsWith/endsWith/replace | 小 | ⏳ 待实现 |

### 3.3 阶段 3：编译器基础设施

| 文件 | 需要实现的功能 | 预估工作量 | 状态 |
|------|--------------|-----------|------|
| Vm.aura | 完整字节码解释器 | 大 | ⏳ 占位 |
| Gc.aura | 标记-清除 GC | 大 | ⏳ 占位 |
| memory/ | 内存管理（ARC + 内存池） | 大 | ⏳ 占位 |

---

## 四、风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| String 操作性能 | 高 | 使用 StringBuilder 优化 |
| libm 函数精度 | 中 | 保留 Rust native 兜底 |
| VM 操作依赖 | 低 | 标记为 Layer 0-A 不可上移 |
| 编译器自举复杂度 | 高 | Phase 4 目标，当前为占位 |

---

## 五、目录结构（v3.1）

```
aura/
├── core/aura/lang/
│   ├── Any.aura              # 根类型
│   ├── Int.aura              # 整数类型
│   ├── Float.aura            # 浮点类型
│   ├── Boolean.aura          # 布尔类型
│   ├── String.aura           # 字符串类型
│   ├── List.aura             # 列表类型
│   ├── Map.aura              # 映射类型
│   ├── Byte.aura             # 字节类型
│   ├── Char.aura             # 字符类型
│   ├── Long.aura             # 长整数类型
│   ├── Short.aura            # 短整数类型
│   ├── Double.aura           # 双精度浮点
│   ├── Array.aura            # 数组类型
│   ├── Function.aura         # 函数类型
│   ├── Type.aura             # 类型元信息
│   ├── Unit.aura             # 单元类型
│   ├── Nothing.aura          # 空类型
│   ├── prelu.aura            # 预lude
│   └── std/                  # 标准库模块
│       ├── Math.aura
│       ├── String.aura
│       ├── Time.aura
│       ├── Collections.aura
│       ├── Builtin.aura
│       ├── Encoding.aura
│       ├── Path.aura
│       └── ... (23 个模块)
├── compiler/aura/lang/compiler/
│   ├── vm/                   # VM 上层逻辑
│   ├── gc/                   # 垃圾收集器
│   ├── memory/               # 内存管理
│   └── runtime/              # 运行时支持
└── build/                    # 预编译 .auc 文件

compiler/                     # Rust 编译器
└── src/
    ├── vm/                   # VM 核心
    ├── codegen/              # 代码生成
    └── std/                  # Rust native 实现
```

---

## 六、下一步行动

### 6.1 阶段 1 执行计划

1. **立即开始**: Any.aura, Boolean.aura（工作量小，基础依赖）
2. **第二批**: Int.aura, Float.aura（基本类型，纯逻辑）
3. **第三批**: String.aura, List.aura, Map.aura（集合类型，工作量较大）

### 6.2 验证方法

```bash
# 编译单个文件
aura build aura/core/aura/lang/Any.aura

# 编译所有标准库
aura stdlib-compile aura/core/aura/lang/std --output build

# 验证嵌入式标准库
aura run examples/language-test/test_stdlib_aura.aura
```

---

## 七、变更记录

| 日期 | 版本 | 变更内容 |
|------|------|---------|
| 2026-07-05 | 1.0 | 初始创建，阶段 1 开发中 |
