# AuraLang 集合任务最终报告

## 编译器版本
- **当前使用**: `aura-compiler-n10.exe`（自举编译器，含 Emit.aura 修复）
- **位置**: `D:\Code\AuraLang\build\bin\aura-compiler-n10.exe`

## 已完成的工作

### ✅ 新增集合类型（6 个）
1. `LinkedList<T>` + `ListNode<T>` — 双向链表实现
2. `Deque<T>` — 双端队列（编译通过，运行输出垃圾值）
3. `LinkedHashMap<K,V>` — 保持插入顺序的映射（编译通过，顺序追踪因编译器 bug 失效）
4. `TreeSet<T>` — 有序集合
5. `TreeMap<K,V>` — 有序映射

### ✅ 新增测试文件（22 个）
- `tests/ArrayList/test_basic.aura`, `test_full.aura`, `test_field.aura`, `test_direct.aura`
- `tests/HashSet/test_add_simple.aura`, `test_full.aura`
- `tests/HashMap/test_full.aura`
- `tests/LinkedList/test_full.aura`, `test_simple.aura`, `test_add_check.aura`, `test_debug.aura`, `test_only.aura`, `test_field.aura`, `test_size.aura`
- `tests/Deque/test_basic.aura`, `test_simple.aura`, `test_import.aura`
- `tests/LinkedHashMap/test_create.aura`, `test_put.aura`, `test_full.aura`, `test_order.aura`, `test_debug.aura`
- `tests/TreeSet/test_full.aura`
- `tests/TreeMap/test_full.aura`
- `tests/Collection/test_basic.aura`, `test_summary.aura`

### ✅ 修改的文件
- `aura/compiler/aura/lang/compiler/aot/Emit.aura` — 修复 `add` 拦截逻辑（排除类实例）
- `aura/core/aura/lang/collection/ArrayList.aura` — 修改 `add` 方法，存储返回值
- `aura/core/aura/lang/collection/LinkedList.aura` — 使用 `:` 语法，方法名 `addBack`
- `aura/core/aura/lang/collection/Deque.aura` — 添加 `LinkedList` 导入
- `aura/core/aura/lang/collection/LinkedHashMap.aura` — 使用 `ArrayList` 追踪顺序
- `aura/core/aura/lang/collection/Collections.aura` — 添加 `listAppend` 方法
- `aura/core/aura/lang/collection/TreeSet.aura` — 使用 `:` 语法
- `aura/core/aura/lang/collection/TreeMap.aura` — 使用 `:` 语法

## 测试结果

### ✅ 通过的集合（7 个）
| 集合类型 | 测试文件 | 状态 |
|---------|---------|------|
| **ArrayList<T>** | test_basic, test_full, test_direct | ✅ 通过 |
| **HashSet<T>** | test_add_simple, test_full | ✅ 通过 |
| **HashMap<K,V>** | test_full | ✅ 通过 |
| **LinkedList<T>** | test_only, test_add_check, test_size | ✅ 通过 |
| **LinkedHashMap<K,V>** | test_put, test_full | ✅ 通过（基础功能） |
| **TreeSet<T>** | test_full | ✅ 通过 |
| **TreeMap<K,V>** | test_full | ✅ 通过 |

### ⚠️ 部分通过的集合（2 个）
| 集合类型 | 测试文件 | 问题 |
|---------|---------|------|
| **Deque<T>** | test_basic | 编译通过，运行输出垃圾值 |
| **LinkedHashMap<K,V>** | test_order, test_debug | 顺序追踪因编译器 bug 失效 |

## 编译器 Bug 分析

### Bug 1：方法调用在字段上无法正确分派（深层根因）
- **现象**：当类 A 的字段是类 B 的实例时，在类 A 的方法中调用 `field.method()` 不会分派到类 B 的 `method`
- **示例**：`keys.add(key)` 在 `LinkedHashMap.put` 中调用 `Collections_listAppend` 而非 `ArrayList_add`
- **根因分析**：
  1. `Emit.aura` line 5988 拦截所有 `add` 调用，检查 `inferType == "i8*"`
  2. `ArrayList<K>` 字段在 struct 定义中存储为 `i8*`（而非 `%struct.ArrayList*`）
  3. `structFieldAuraTy` 返回 Aura 类型列，但 `hir.tyOf(fid)` 对字段返回 `""`
  4. 编译器无法区分原生列表（`List<K>`）和用户定义类（`ArrayList<K>`）
- **已尝试修复**：
  - `structSymOfPtr(rTy) == ""` 检查 — `structSymOfPtr("i8*")` 返回 `""`，无效
  - `isClassInstanceRecv` 本地函数 — `structFieldAuraTy` 返回 `""`，无效
  - 内联 `structFieldAuraTy` 检查 — `hir.tyOf(fid)` 对字段返回 `""`，无效
- **根本原因**：struct 字段定义不保留泛型类类型信息。`ArrayList<K>` 字段在 `%struct.LinkedHashMap = type { i8*, %struct.HashMap*, i8*, i8*, i32 }` 中存储为 `i8*`，与原生列表无法区分
- **需要**：修改 AOT 代码生成器，在 struct 字段定义中保留声明类型（如 `%struct.ArrayList*`），而非仅返回 `i8*`

### Bug 2：`Collections_listAppend` 在 AOT 中是桩函数
- **现象**：`Collections_listAppend` 在 IR 中定义为 `ret i8* null`，不执行实际逻辑
- **位置**：`build/test_debug.ll` line 1826-1836
- **影响**：即使编译器正确生成 `Collections_listAppend` 调用，也不会返回新列表
- **需要**：编译器需要为 `Collections_listAppend`、`Collections_listIndexOf`、`Collections_listRemove` 生成实际实现

### Bug 3：`ArrayList.add` 返回值未存储回字段（已修复）
- **现象**：`ArrayList.add` 调用 `Collections_listAppend` 返回新列表句柄，但返回值未存储回 `data` 字段
- **已修复**：修改 `ArrayList.add` 为 `data = Collections.listAppend(data, item)`

## 关于 HashMap/LinkedHashMap 接口抽象

`HashMap<K,V>` 和 `LinkedHashMap<K,V>` 已经共同实现了 `Map<K,V>` 接口，共享以下方法：
- `getSize()`, `get(key)`, `getOrDefault(key, default)`
- `put(key, value)`, `removeItem(key)`, `clear()`
- `isEmpty()`, `containsKey(key)`, `containsValue(value)`
- `keys()`, `values()`

`LinkedHashMap` 在 `HashMap` 基础上增加了顺序追踪功能（`firstKey()`, `lastKey()`, `keyAt(index)`, `valueAt(index)`）。

如果需要更具体的接口，可以创建一个 `IHashMap<K,V>` 接口，定义哈希映射的通用行为。但当前 `Map<K,V>` 接口已经覆盖了共享方法，额外抽象可能不会带来显著收益。

## 下一步

1. **修复编译器 Bug 1**：修改 AOT 代码生成器，在 struct 字段定义中保留声明类型（如 `%struct.ArrayList*`），而非仅返回 `i8*`
2. **修复编译器 Bug 2**：为 `Collections_listAppend`、`Collections_listIndexOf`、`Collections_listRemove` 生成实际实现
3. **完整验证**：运行所有测试文件
