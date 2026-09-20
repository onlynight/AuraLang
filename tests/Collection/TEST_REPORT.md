# AuraLang 集合测试报告

## 编译器版本
- **编译器**: `aura-compiler-n1.exe`（自举编译器）
- **位置**: `D:\Code\AuraLang\build\bin\aura-compiler-n1.exe`

## 测试结果

### ✅ 通过的集合

| 集合类型 | 测试文件 | 状态 |
|---------|---------|------|
| **ArrayList<T>** | `test_basic.aura`, `test_full.aura` | ✅ 通过 |
| **HashSet<T>** | `test_add_simple.aura`, `test_full.aura` | ✅ 通过 |
| **HashMap<K,V>** | `test_full.aura` | ✅ 通过 |
| **LinkedList<T>** | `test_full.aura` | ⚠️ 部分通过 |

### ❌ 失败的集合

| 集合类型 | 测试文件 | 问题 |
|---------|---------|------|
| **Deque<T>** | `test_full.aura` | 类型推断错误（ptr vs i16） |
| **LinkedHashMap** | `test_put.aura` | HashMap.put 调用崩溃 |
| **TreeSet<T>** | 未测试 | 依赖 ArrayList，可能有相同问题 |
| **TreeMap<K,V>** | 未测试 | 依赖 ArrayList，可能有相同问题 |

## 已知问题

### 1. LinkedList.add 被降级
- `LinkedList<T> implements List<T>`
- 编译器将 `add` 方法降级为 `Collections_listAppend`
- 导致 `add` 方法不增加 `_size`

### 2. Deque 类型推断错误
- `back()`/`front()` 返回 `T` 类型
- 编译器将 `T` 推断为 `i16`，但实际是 `i8*`
- IR 错误：`sext i16 %var.2193 to i64`

### 3. LinkedHashMap.put 崩溃
- `LinkedHashMap.put` 调用 `HashMap.put`
- `HashMap.put` 返回 `V?`，`LinkedHashMap.put` 期望 `V`
- 类型不匹配导致崩溃

## 文件列表

### 集合实现
- `aura/core/aura/lang/collection/ArrayList.aura`
- `aura/core/aura/lang/collection/HashSet.aura`
- `aura/core/aura/lang/collection/HashMap.aura`
- `aura/core/aura/lang/collection/LinkedList.aura` (新增)
- `aura/core/aura/lang/collection/ListNode.aura` (新增，链表节点)
- `aura/core/aura/lang/collection/Deque.aura` (新增)
- `aura/core/aura/lang/collection/LinkedHashMap.aura` (新增)
- `aura/core/aura/lang/collection/TreeSet.aura` (新增)
- `aura/core/aura/lang/collection/TreeMap.aura` (新增)

### 接口定义
- `aura/core/aura/lang/collection/Collection.aura`
- `aura/core/aura/lang/collection/List.aura`
- `aura/core/aura/lang/collection/Set.aura`
- `aura/core/aura/lang/collection/Map.aura`

### 测试文件
- `tests/ArrayList/test_basic.aura`
- `tests/ArrayList/test_full.aura`
- `tests/HashSet/test_add_simple.aura`
- `tests/HashSet/test_full.aura`
- `tests/HashMap/test_full.aura`
- `tests/LinkedList/test_full.aura`
- `tests/LinkedList/test_simple.aura`
- `tests/Deque/test_full.aura`
- `tests/LinkedHashMap/test_create.aura`
- `tests/LinkedHashMap/test_put.aura`
- `tests/LinkedHashMap/test_full.aura`
- `tests/Collection/test_basic.aura`
- `tests/Collection/test_summary.aura`

## 下一步

1. **修复编译器**: 修改 `Emit.aura` 排除 `LinkedList`/`Deque` 的 `add` 降级
2. **修复 LinkedHashMap**: 处理 `HashMap.put` 的 `V?` 返回类型
3. **测试 TreeSet/TreeMap**: 验证新集合类型
4. **完整验证**: 运行所有测试文件
