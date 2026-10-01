# ADR-003：VM 对象模型与对象指令契约

- **状态**：已接受（2026-10-01）
- **关联**：VM-PA-00 v3.1 §1.7 缺陷 1 / P0.5；ADR-001

## 背景（缺陷）

`Vm.aura` 的对象字段落在**全局变量表**（`globals["field:"+name]`），跨实例共享：
两个 `Dog` 实例先后 `SET_FIELD name` 后，读任何一个都得到最后写入的值。此外
`callMethod`/`callCtor` 是桩（忽略接收者与参数）、`newObject` 类型标签恒 0。

## 决策

### 1. 字段存储：按实例隔离

字段存入**按对象指针键控的实例字段表**：
`objectFields: HashMap<String, Any>`，键为 `"<objPtr>|<fieldName>"`；
`objectClass: HashMap<String, String>` 记录 `"<objPtr>" -> 类名`。

**与 VM-PA-00 §2.2 原文（objPtr+16 起按字段槽写入堆内存）的偏差及理由**：
原始方案要求把字段值直接 `Memory.write64` 进对象堆布局，但本 VM 的值模型是
AOT 运行时的装箱 `Any`（Int 内联、String/对象为句柄），**没有把任意 `Any`
安全地 round-trip 成裸 i64 的原语**（`write64` 对字符串句柄会截断——这正是
原实现退到全局表的根因，见 `boxAlloc` 既有缺陷记录）。在装箱值模型被
P1.1 对齐 Rust `Value` 10 变体重做之前，裸内存字段槽无法不破坏字符串/嵌套对象。
因此 P0.5 采用实例键控哈希实现**语义等价**的按实例隔离；堆内联布局作为
P1.1/P1.5 的目标形态延续推进。验收标准不变：多实例字段必须隔离。

### 2. 对象头

`NEW_OBJECT` 分配 16 字节头并注册类名：
- `offset 0`（i64）：类标识（预留 vtable/类表扩展位）；
- `offset 8`（i32）：引用计数 = 1（与既有 ARC 指令的布局约定一致）。

### 3. 对象指令字节码契约（文本模式与 `.auc` 翻译模式共用）

| 指令 | 操作数 | 栈序 | 语义 |
|------|--------|------|------|
| `NEW_OBJECT cls` | 类名 | → ptr | 分配 + 注册 `objectClass[ptr]=cls` |
| `SET_FIELD name` | 字段名 | ptr, value → | `objectFields["ptr|name"] = value` |
| `GET_FIELD name` | 字段名 | ptr → value | 取实例字段；未设置时压 `0`（兼容旧值）；内建成员（`length`/`size`/`count`/`isEmpty`/`first`/`last`）优先 |
| `CALL_METHOD name [argc]` | 方法名；argc 可选 | receiver, args… → ret | 见下 |
| `CALL_CTOR name [argc]` | 构造器名；argc 可选 | receiver, args… → | 见下 |

### 4. 方法分派约定

- 方法函数在函数表中以 `"类名.方法名"` 注册（如 `Dog.speak`），**第一个形参
  固定为接收者 self**（`argc` 操作数=除 self 外的实参个数；无 argc 时由
  `funcParamCount("类名.方法名")` 推导）。
- 解析顺序：接收者类名（`objectClass`）+ 方法名 → `"cls.name"` 命中函数表则
  调用；否则退让旧路径 `callOrBuiltin(name, 0)`（兼容遗留字节码）。
- 构造器同理：`CALL_CTOR` 调用 `"name"` 函数，self 为首参。

### 5. 生命周期

`decRef` 归零释放堆块时，实例字段表条目的清理由 P1.5（ARC 扩展 + 泄漏检测）
统一承接；P0 阶段不做扫描回收（避免 O(全字段) 清理打断验收路径，缺陷已登记）。

## 验收

`class Dog { var name }` 等价的文本字节码：两个实例分别赋值 `"Rex"`/`"Fido"`，
分别读取得到各自值（回归测试 `tests/vm_object_model_tests.aura`）。
