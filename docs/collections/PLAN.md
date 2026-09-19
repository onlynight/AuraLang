# Aura Lang 集合类型接口实现与测试 — 技术方案

> 目录：`aura/core/aura/lang/collection/`
>
> 目标：使常用集合类型**正确实现对应接口**并通过测试，覆盖 List/Set/Map 三大类别的多种实现变体。

---

## 一、现状诊断

### 1.1 接口定义（5 个，无需修改）

| 接口 | 继承 | 方法数 | 状态 |
|------|------|--------|------|
| `Collection<T>` | — | 8 | ✅ 已正确 |
| `List<T>` | `Collection<T>` | +8 | ✅ 已正确 |
| `Array<T>` | `Collection<T>` | +2 | ✅ 仅接口占位（无实现类） |
| `Set<T>` | `Collection<T>` | +4 | ✅ 已正确 |
| `Map<K,V>` | — | 12 | ✅ 已正确 |

### 1.2 实现类问题

| 类 | 声明 | check 错误数 | 主要问题 |
|---|------|-------------|----------|
| `ArrayList<T>` | `: List<T>` | 30+ | 缺 `count/getAt/isEmpty`；`mutableListOf` 类型擦除失败；`override` 标记错乱；抛非 Throwable |
| `HashSet<T>` | （无继承） | 31 | 无 import；用 `init()`；未实现 `Set` |
| `HashMap<K,V>` | `: Map<K,V>` | 0 | ✅ 已正确（测试 111/114，3 个失败来自外部 String 方法未链接） |

### 1.3 关键编译器约束（从已有代码注释中提取）

1. 构造函数必须用 `constructor()`，不能用 `init()`
2. 位运算必须用符号形式 `^`/`>>`/`&`，不能用单词形式 `xor`/`shr`/`and`
3. 列表追加必须用 `list.add(item)`，不能用 `list + item`（后者被当作字符串拼接）
4. 抛异常必须用 `throw "msg"` 字符串形式（自动包成 Exception），不能抛自定义非 Throwable 类
5. `List<T>` 接口类型不能索引（`list[i]`），必须用具体 `ArrayList<T>` 类型
6. `mutableListOf<T>()` 返回 `List<Any>`，赋给 `List<T>` 会类型擦除失败，改用直接构造 `ArrayList<T>()`
7. 泛型形参必须用单个大写字母命名（`class ArrayList<T>`）
8. 单文件可含多个类（已验证，如 `ListNode<T>` + `LinkedList<T>`）

---

## 二、修改策略

### 阶段 1：修复现有实现（2 个文件）

#### 1.1 `ArrayList.aura`
- 加 `import aura.lang.collection.Collection`
- `data` 字段类型改为 `ArrayList<T>`（避免 `List<T>` 不能索引）
- 构造函数用 `ArrayList<T>()` 直接构造
- 去掉 `getSize`/`get` 的错误 `override`，补齐 8 个真实 `override`（filter/map/distinct/every/any/take/skip/lastIndexOf）
- 补全 Collection 必需方法：`count()`、`getAt()`、`isEmpty()`
- 异常改用 `throw "msg"` 字符串形式
- 保留原实现风格（while 循环、私有辅助、minOf）

#### 1.2 `HashSet.aura`（重写）
- imports：`Set`、`ArrayList`、`Collection`
- 声明改为 `class HashSet<T> : Set<T>`
- `init()` → `constructor()`
- 补全 Set 接口：add/remove/clear/toList（已存在但需 `override`）
- 补全 Collection 继承方法：count/getAt/isEmpty/first/last/toString/contains/indexOf/getSize
- 保留 union/intersection/subtract 集合运算
- 修 `HashSetUtils.mutableSetOf` 语义

### 阶段 2：新增集合实现（5 个文件）

#### 2.1 `LinkedList.aura` — 双链表 `LinkedList<T> : List<T>`
```
class ListNode<T> {           // 内部节点
    var value: T
    var prev: ListNode<T>?
    var next: ListNode<T>?
    constructor(v: T) { ... }
}
class LinkedList<T> : List<T> {
    private var head: ListNode<T>?
    private var tail: ListNode<T>?
    private var _size: Int
    // 实现 List + Collection 全部方法
    // 额外：addFirst/addLast/removeFirst/removeLast/insert/addAll/reversed
}
```
- 复杂度：add(O(1))、remove(O(1))、get(O(n))
- 时间优于 ArrayList 的插入/删除

#### 2.2 `LinkedHashMap.aura` — 插入顺序 Map `LinkedHashMap<K,V> : Map<K,V>`
```
class LinkedHashMap<K, V> : Map<K, V> {
    private var buckets: HashMap<K, V>       // 底层哈希存储
    private var order: ArrayList<K>          // 插入顺序键链
    // 实现 Map 全部方法
    // 额外：iterator-order 保持插入顺序
}
```
- 复杂度：get/put/remove O(1)，keys/values 保持插入顺序

#### 2.3 `TreeSet.aura` — 排序集合 `TreeSet<T> : Set<T>`
```
class TreeSet<T> : Set<T> {
    private var data: ArrayList<T>           // 排序存储
    private var comparator: (T, T) -> Int    // 比较回调：<0/0/>0
    // 实现 Set + Collection 全部方法
    // 额外：lower/upper/floor/ceiling/subSet/headSet/tailSet
}
```
- 复杂度：add/remove O(n)（顺序数组），contains O(log n)（二分）
- 有序性保证

#### 2.4 `TreeMap.aura` — 排序 Map `TreeMap<K,V> : Map<K,V>`
```
class TreeMap<K, V> : Map<K, V> {
    private var keys: ArrayList<K>           // 排序键
    private var values: ArrayList<V>         // 对齐值
    private var comparator: (K, K) -> Int
    // 实现 Map 全部方法
    // 额外：firstKey/lastKey/ceilingEntry/floorEntry/subMap
}
```

#### 2.5 `Deque.aura` — 双端队列（独立类）
```
class Deque<T> {
    private var data: ArrayList<T>
    private var head: Int    // 头部游标
    // pushFront/pushBack/popFront/popBack/front/back/size/isEmpty/offer/poll
    // 兼容 List 接口的方法（可选）
}
```
- 用作 Queue/Stack/PriorityQueue 的底座

### 阶段 3：测试（新增 14 个文件）

参照 `tests/HashMap/hashmap_basic.aura` 的 `Checker` 模式。

```
tests/ArrayList/
├── arraylist_basic.aura          # 基础 CRUD + 接口方法
├── arraylist_hops.aura           # filter/map/distinct/every/any/take/skip
├── arraylist_edge_cases.aura     # 越界、空表、1000+ 规模
└── arraylist_interfaces.aura     # List/Collection 多态调用

tests/HashSet/
├── hashset_basic.aura            # 基础 CRUD + 接口方法
├── hashset_ops.aura              # union/intersection/subtract
└── hashset_edge_cases.aura       # 重复、null、混合类型

tests/LinkedList/
├── linkedlist_basic.aura         # 双链表基本操作
├── linkedlist_insert_remove.aura # 头/尾/中间插入删除
└── linkedlist_interfaces.aura    # List 多态调用

tests/LinkedHashMap/
├── linkedhashmap_basic.aura      # 基本操作 + 顺序验证
└── linkedhashmap_interfaces.aura # Map 多态调用

tests/TreeSet/
├── treeset_basic.aura            # 有序性 + CRUD
└── treeset_ops.aura              # subSet/headSet/tailSet

tests/TreeMap/
├── treemap_basic.aura            # 有序 Map + CRUD
└── treemap_ops.aura              # subMap/firstKey/lastKey

tests/Deque/
└── deque_basic.aura              # push/pop 头尾
```

### 阶段 4：验证
- 新测试全部 `aura run` → "ALL TESTS PASSED"
- `aura check` 对所有 collection 文件 → 无 sema 错误
- 回归 `tests/HashMap/*.aura` 不破坏

---

## 三、交付清单

| 文件 | 操作 | 类型 |
|------|------|------|
| `collection/{Collection,List,Array,Set,Map}.aura` | 不变 | 接口 |
| `collection/Collections.aura` | 不变 | 工具占位 |
| `collection/ArrayList.aura` | 修改 | List |
| `collection/HashSet.aura` | 重写 | Set |
| `collection/HashMap.aura` | 不变 | Map |
| **`collection/LinkedList.aura`** | **新增** | **List** |
| **`collection/LinkedHashMap.aura`** | **新增** | **Map** |
| **`collection/TreeSet.aura`** | **新增** | **Set** |
| **`collection/TreeMap.aura`** | **新增** | **Map** |
| **`collection/Deque.aura`** | **新增** | **Queue** |
| `tests/ArrayList/*.aura` | 新增 4 个 | 测试 |
| `tests/HashSet/*.aura` | 新增 3 个 | 测试 |
| **`tests/LinkedList/*.aura`** | **新增 3 个** | **测试** |
| **`tests/LinkedHashMap/*.aura`** | **新增 2 个** | **测试** |
| **`tests/TreeSet/*.aura`** | **新增 2 个** | **测试** |
| **`tests/TreeMap/*.aura`** | **新增 2 个** | **测试** |
| **`tests/Deque/*.aura`** | **新增 1 个** | **测试** |

**预计：2 修改 + 5 实现新增 + 14 测试新增，约 1500-2000 行代码。**

---

## 四、最终矩阵

| 类别 | 实现 | 接口 | 有序 | 重复 | 时间复杂度 |
|------|------|------|------|------|-----------|
| **List** | ArrayList | List | ✅ 索引 | ✅ | get O(1), add O(1), remove O(n) |
| **List** | LinkedList | List | ✅ 顺序 | ✅ | get O(n), add O(1), remove O(1) |
| **Set** | HashSet | Set | ❌ | ❌ | add/remove/contains O(1) |
| **Set** | TreeSet | Set | ✅ 排序 | ❌ | add/remove O(n), contains O(log n) |
| **Map** | HashMap | Map | ❌ | ❌ | get/put/remove O(1) |
| **Map** | LinkedHashMap | Map | ✅ 插入 | ❌ | get/put/remove O(1) |
| **Map** | TreeMap | Map | ✅ 排序 | ❌ | get/put/remove O(log n) |
| **Queue** | Deque | — | ✅ | ✅ | push/pop 头尾 O(1) |

---

## 五、风险与说明

1. **String 方法未链接**：已有 HashMap 测试 3 个失败来自此，与本次任务无关，不做修复。
2. **Array 具体实现**：暂不新增，避免与 prelu 中 `arrayOf` 递归冲突；`Array<T>` 接口保留占位。
3. **异常类型**：`IndexOutOfBoundsException`/`EmptyListException` 若非 Throwable，改用 `throw "msg"` 字符串形式。
4. **`mutableListOf` 泛型擦除**：`mutableListOf<T>()` 返回 `List<Any>`，赋给 `List<T>` 会失败，改用直接构造。
5. **泛型比较限制**：`T` 擦除为 `i8*`，TreeSet/TreeMap 必须通过 `comparator` 回调比较，不能直接 `a < b`。
6. **多类同文件**：Aura 支持单文件多类（已验证），`ListNode<T>` 与 `LinkedList<T>` 同文件。
7. **TreeSet/TreeMap 复杂度**：采用排序数组实现（而非平衡树），保证简单可靠；O(n) add 在测试规模下可接受。

---

## 六、执行顺序

```
阶段 1: 修 ArrayList → 修 HashSet → 跑 tests/ArrayList + tests/HashSet
阶段 2: 新增 LinkedList → LinkedHashMap → TreeSet → TreeMap → Deque
阶段 3: 补每个新类的测试
阶段 4: 全量验证 + 回归 HashMap
```
