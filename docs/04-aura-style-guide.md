# Aura 代码风格指南

## 1. 命名约定

| 元素 | 约定 | 示例 |
|------|------|------|
| 常量 / 枚举值 | UPPER_CASE | `GAME_WIDTH`, `RED`, `MAX_PATH` |
| 类 / 接口 / 结构体 / Actor | PascalCase | `Player`, `Drawable`, `Server` |
| 函数 / 变量 / 参数 | camelCase | `loadConfig`, `playerName` |
| 泛型参数 | 单大写字母 | `T`, `U`, `K`, `V` |
| 包名 / 模块名 | 小写点分 | `aura.std.fs` |
| 文件 | kebab-case 或 camelCase | `hello-world.aura` |

## 2. 格式化

### 2.1 缩进
- 使用 4 空格缩进
- 不要用 tab
- 嵌套块正确缩进

### 2.2 空行
- 声明之间：1 个空行
- 逻辑块之间：1 个空行
- 函数之间：2 个空行
- 文件顶部：import 之后 1 个空行

### 2.3 空格
- 运算符前后有空格：`x + 1`（不是 `x+1`）
- 逗号后有空格：`f(a, b)`（不是 `f(a,b)`）
- 冒号后有空格：`val x: Int`（不是 `val x:Int`）
- 注解后有空格：`@param name`（不是 `@paramname`）

### 2.4 花括号
- 函数体：同行开启 `{`
```aura
fun add(a: Int, b: Int): Int {
    return a + b
}
```
- 控制流：同行开启 `{`
```aura
if (x > 0) {
    println("positive")
}
```
- 单表达式函数：用 `=` 代替花括号
```aura
fun add(a: Int, b: Int): Int = a + b
```

### 2.5 字符串
- 字符串插值用 `${}`
```aura
val msg = "Hello, ${name}!"
```
- 多行字符串用三引号（无插值）
```aura
val raw = """
    No interpolation: $var
"""
```

## 3. 导入

### 3.1 顺序
1. 标准库（`aura.std.*`）
2. 第三方库
3. 本地模块

### 3.2 别名
```aura
import aura.std.fs as fs          // 常用模块用别名
import aura.concurrent            // 无别名直接导入
```

### 3.3 通配符导入（谨慎使用）
```aura
import aura.std.*                 // ⚠️ 仅在确实需要大量使用时
```

## 4. 文档注释

### 4.1 函数文档
```aura
/**
 * 加载配置文件
 * @param path 配置文件路径
 * @return 配置对象，失败时返回错误
 */
fun loadConfig(path: String): Result<Config, Exception> {
    ...
}
```

### 4.2 结构体文档
```aura
/**
 * 玩家数据结构
 */
data struct Player(
    val id: Int,
    var name: String = "unknown"
)
```

## 5. 注释

### 5.1 单行注释
```aura
// 这是单行注释
```

### 5.2 块注释
```aura
/*
这是块注释
*/
```

### 5.3 文档注释标记
```
@param    — 参数说明
@return   — 返回值说明
@throws   — 异常说明
@see      — 参见
@author   — 作者
@version  — 版本
@since    — 自版本
@deprecated — 已废弃
@example  — 示例
```

## 6. 错误处理风格

### 6.1 推荐：Result 模式
```aura
fun load(): Result<Int, Exception> {
    if (ok) return Result.Success(42)
    else return Result.Error(Exception("failed"))
}
```

### 6.2 备选：try-catch
```aura
try {
    val v = parse(data)
} catch (e: Exception) {
    println("Error: ${e.message}")
}
```

## 7. 测试风格

### 7.1 函数式测试
```aura
fun testLoadConfig() {
    val result = loadConfig("test.json")
    assert(result.isSuccess) { "Config should load" }
}
```

## 8. 常见陷阱提醒

| 陷阱 | 正确 | 错误 |
|------|------|------|
| 数据结构体 | `data struct` | `data class` |
| 并发实体 | `actor` | `class` |
| 编译时函数 | `comptime fun` | `fun` |
| FFI | `extern "c"` | `extern` |
| Result 参数 | `Result<T, E>` | `Result<T>` |
| 可空类型 | `Int?` | `Optional<Int>` |
| 方法引用 | `obj::method` | `obj.method` |
| await | `await f()` | `await(f())` |
