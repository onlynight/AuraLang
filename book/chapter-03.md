# 第三章：Lua 迁移指南

> 从 Lua 到 Aura — 语法对照与迁移策略

---

## 3.1 为什么要迁移？

| 维度 | Lua | Aura |
|------|-----|------|
| 语法 | 简单但原始 | 现代 Kotlin 风格 |
| 类型系统 | 动态 | 静态 + 类型推断 |
| 内存管理 | GC | ARC（无暂停） |
| 性能 | 解释执行 | AOT + JIT |
| 并发 | 弱支持 | 协程 + Actor |
| FFI | 有限 | 零开销 C ABI |
| 空安全 | 运行时错误 | 编译期检查 |

## 3.2 语法对照表

### 变量声明

```lua
-- Lua
local x = 10
local function foo() end
```

```aura
// Aura
val x = 10
fun foo() {}
```

### 函数定义

```lua
-- Lua
function foo(a, b)
    return a + b
end
```

```aura
// Aura
fun foo(a: Int, b: Int): Int {
    return a + b
}
```

### 条件语句

```lua
-- Lua
if x > 0 then
    print("positive")
elseif x == 0 then
    print("zero")
else
    print("negative")
end
```

```aura
// Aura
if (x > 0) {
    println("positive")
} else if (x == 0) {
    println("zero")
} else {
    println("negative")
}
```

### 循环

```lua
-- Lua
for i = 0, 10 do
    print(i)
end

while running do
    update()
end
```

```aura
// Aura
for (i in 0..10) {
    println(i)
}

while (running) {
    update()
}
```

### 字符串插值

```lua
-- Lua
local name = "Aura"
print("Hello, " .. name .. "!")
print(string.format("Hello, %s!", name))
```

```aura
// Aura
val name = "Aura"
println("Hello, $name!")
println("Hello, ${name}!")
```

### 表/字典

```lua
-- Lua
local t = { "a", "b", "c" }
local d = { name = "Aura", version = 1 }
t[4] = "d"
d.name = "new"
```

```aura
// Aura
val list = listOf("a", "b", "c")
val map = mapOf("name" to "Aura", "version" to 1)
// 可变集合
val mutableList = mutableListOf("a", "b", "c")
mutableList.add("d")
```

### 闭包

```lua
-- Lua
local function makeCounter()
    local count = 0
    return function()
        count = count + 1
        return count
    end
end
```

```aura
// Aura
fun makeCounter(): () -> Int {
    var count = 0
    return fun(): Int {
        count++
        return count
    }
}
```

### 错误处理

```lua
-- Lua
local ok, err = pcall(function()
    error("something went wrong")
end)
if not ok then
    print("Error: " .. err)
end
```

```aura
// Aura
try {
    throw RuntimeException("something went wrong")
} catch (e: RuntimeException) {
    println("Error: ${e.message}")
}

// 或使用 Result
fun safeOp(): Result<Int, String> {
    return Result.Error("something went wrong")
}
```

## 3.3 类型系统迁移

### 从动态到静态

```lua
-- Lua：动态类型，运行时检查
local x = 10
x = "hello"  -- 运行时不报错，但逻辑错误
```

```aura
// Aura：静态类型，编译期检查
val x = 10        // x 的类型固定为 Int
x = "hello"       // 编译错误！
```

### 空安全

```lua
-- Lua：nil 检查
local x = getSomething()
if x ~= nil then
    print(x)
end
```

```aura
// Aura：编译期空安全
var x: String? = getSomething()
if (x != null) {
    println(x)  // 智能转换，x 自动窄化为 String
}

// 或安全调用
val result = x?.length
// 或 Elvis
val safe = x ?: "default"
// 或断言非空
val forced = x!!  // 谨慎使用
```

### 数据类

```lua
-- Lua：用表模拟
local Player = {}
Player.__index = Player

function Player.new(id, name, health)
    return setmetatable({
        id = id,
        name = name,
        health = health
    }, Player)
end

function Player:takeDamage(dmg)
    self.health = self.health - dmg
end
```

```aura
// Aura：结构体
struct Player(
    val id: Int,
    var name: String,
    var health: Int = 100
)

// 自动生成 toString, equals, hashCode, copy
val p = Player(1, "Alice")
val p2 = p.copy(health = 80)
```

## 3.4 迁移策略

### 阶段 1：评估（1-2 天）

1. 分析现有 Lua 代码库
2. 识别类型不安全的代码
3. 评估空指针风险
4. 确定并发使用场景

### 阶段 2：基础迁移（1 周）

1. 将纯计算逻辑迁移到 Aura
2. 使用类型推断减少注解
3. 逐步添加类型注解
4. 使用 Result 替代 pcall

### 阶段 3：高级特性（1-2 周）

1. 使用协程替代回调地狱
2. 使用 Actor 处理并发
3. 使用 FFI 替代 C 互操作
4. 使用 ARC 管理内存

### 阶段 4：优化（持续）

1. 使用 AOT 编译关键路径
2. 使用 JIT 优化热点代码
3. 使用 ARC 优化减少保留/释放
4. 使用逃逸分析优化栈分配

## 3.5 常见陷阱

### 陷阱 1：全局变量

```lua
-- Lua：全局变量随处可见
x = 10  -- 创建了全局变量 x
```

```aura
// Aura：必须显式声明
val x = 10  // 局部变量
fun f() {
    val y = 20  // 局部变量
}
```

### 陷阱 2：动态类型

```lua
-- Lua：类型在运行时确定
local x = 1
x = "hello"  -- 运行时类型变化
```

```aura
// Aura：类型在编译时确定
val x = 1        // Int
x = "hello"      // 编译错误！
var y: Any = 1   // 需要 Any 类型
```

### 陷阱 3：表作为对象

```lua
-- Lua：表可以作为对象、数组、哈希表
local t = {}
t[1] = "a"       -- 数组
t.name = "b"     -- 对象
t["key"] = "c"   -- 哈希表
```

```aura
// Aura：不同类型需要不同结构
val list = mutableListOf<String>()  // 数组
val map = mutableMapOf<String, String>()  // 哈希表
struct Obj { val name: String }     // 对象
```

## 3.6 迁移工具

### aura-migrate 工具（计划中）

```bash
# 扫描 Lua 文件
aura migrate --scan .

# 生成迁移报告
aura migrate --report lua-to-aura-report.md

# 自动转换基础语法
aura migrate --convert input.lua --output output.aura
```

## 3.7 性能对比

| 场景 | Lua 5.4 | Aura (VM) | Aura (AOT) |
|------|---------|-----------|-----------|
| 数学计算 | 1x | 2x | 13-44x |
| 字符串处理 | 1x | 1.5x | 5-10x |
| 文件 I/O | 1x | 1x | 1x |
| 网络 I/O | 1x | 1x | 1x |
| 内存分配 | GC 暂停 | ARC 无暂停 | ARC 无暂停 |

> 详见 [性能基准报告](./chapter-04.md)