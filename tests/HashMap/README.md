# HashMap 功能测试套件

对 `aura.core.aura.lang.collection.HashMap` 实现的完整功能测试，
确保在各种规模（尤其是大数量）下功能正确。

## 测试文件

| 文件 | 说明 | 覆盖内容 |
|------|------|----------|
| `hashmap_basic.aura` | 基础功能测试 | 空表、put/get、覆盖写、getOrDefault、contains、remove、删除后重插、clear、toString、keys/values、null 值、Int/Boolean/String 键、Map 接口多态 |
| `hashmap_large_scale.aura` | 大数量规模测试 | 1000/5000/10000 条插入全量检索、大批量删除重插、密集删除后扩容压实、键值一致性校验、大批量 null 值 |
| `hashmap_resize.aura` | 扩容重建压力测试 | 逐步扩容 (16→512)、墓碑压实、算术级数键哈希混合、多次 clear+重填周期、扩容后大量删除、多轮压缩、负数键 |
| `hashmap_hash_distribution.aura` | 哈希分布验证 | 步长 1/2/16/1000 键、containsValue 一致性、keys/values 一一对应、混合键类型、同值不同键、删除后 containsValue 检测 |
| `hashmap_edge_cases.aura` | 边界与异常场景 | null 值反复覆盖、删除已删除键、空表操作序列、同键连续操作、值覆盖写与删除后重插组合、HashMapUtils 工厂、clear 后大量重填、getOrDefault null 处理、Map 接口多态完整验证 |

## 运行方式

使用自举编译器运行：

```bash
# 运行单个测试
aura run tests/HashMap/hashmap_basic.aura

# 运行全部测试
for f in tests/HashMap/*.aura; do
    echo "=== $f ==="
    aura run "$f"
    echo ""
done
```

或使用 PowerShell：

```powershell
$compiler = "D:\Code\AuraLang\build\bin\aura.exe"
$tests = Get-ChildItem -Path "D:\Code\AuraLang\tests\HashMap" -Filter "*.aura"
foreach ($test in $tests) {
    Write-Host "=== $($test.Name) ==="
    & $compiler run $test.FullName
}
```

## 测试框架

所有测试使用内置的 `Checker` 类进行断言计数：

- `check(name, condition)` — 记录单次断言结果
- `summary()` — 输出通过/失败计数，全部通过时输出 "ALL TESTS PASSED"

## 覆盖的功能点

### 基本操作
- `put` / `get` / `getOrDefault` / `remove` / `clear`
- `containsKey` / `containsValue` / `isEmpty`
- `size` / `getSize` / `keys` / `values` / `toString`

### 规模测试
- **1000 条** Int 键全量插入与检索
- **5000 条** Int 键插入 + 删除一半 + 重插
- **10000 条** Int 键插入 + 随机删除
- **1000 条** String 键插入与检索
- **5 个键各 200 次** 覆盖写
- **2000 条** 交替删除/重插 500 轮
- **500 条** null 值混合 + 删除

### 扩容重建
- 逐步扩容 (16→32→64→128→256→512)
- 墓碑压实与链长有界
- 多次 clear + 重填周期 (5 轮 × 200 条)
- 扩容后立即大量删除
- 多轮压缩 (10 轮 × 30 条)

### 哈希分布
- 步长 1/2/16/1000 的等差键
- hashKey 混合效果验证
- containsValue 在大量键下的一致性
- keys()/values() 一一对应关系

### 边界场景
- null 值反复覆盖
- 删除已删除键
- 空表完整操作序列
- 同键连续操作 (put→get→put→remove→put→remove)
- 值覆盖写与删除后重插组合
- HashMapUtils 工厂函数
- clear 后大量重填
- getOrDefault 对 null 值的处理
- Map 接口多态完整验证

## 实现参考

测试基于 `aura/core/aura/lang/collection/HashMap.aura` 的实现：

- **哈希分桶**：`hashKey(key) & (capacity - 1)`，容量恒为 2 的幂
- **哈希混合**：`hashKey` 在 `hashCode()` 之上做雪崩混合
- **只追加的桶链**：`put`/`remove` 追加记录，查找自链尾向前扫描
- **负载阈值扩容**：`_records * 10 >= capacity * 7` 时翻倍重建
- **墓碑压实**：扩容重建时丢弃失效记录与被遮蔽的旧记录
