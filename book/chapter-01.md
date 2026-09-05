# 第一章：语言教程

> 从入门到精通 — 10 个练习掌握 Aura 语言

---

## 1.1 变量与类型

Aura 使用 Kotlin 风格的类型系统：

```aura
// 不可变变量
val MAX_PLAYERS = 100
val PI: Float = 3.14159f

// 可变变量
var score: Int = 0
var name = "Aura"  // 类型推断

// 基本类型
val int: Int = 100
val long: Long = 100L
val float: Float = 3.14f
val double: Double = 3.14
val bool: Boolean = true
val char: Char = 'A'
val string: String = "Hello, $name!"
```

### 练习 1.1

```aura
fun exercise1() {
    val pi = 3.14159f
    val radius: Float = 5.0f
    val area = pi * radius * radius
    println("面积: $area")
}
```

## 1.2 函数

```aura
// 标准函数
fun add(a: Int, b: Int): Int {
    return a + b
}

// 单表达式函数
fun double(x: Int): Int = x * 2

// 无返回值
fun log(message: String) {
    println(message)
}

// 默认参数
fun createWindow(
    title: String = "Aura App",
    width: Int = 800,
    height: Int = 600
): Window {
    // ...
}
```

### 练习 1.2

```aura
fun exercise2() {
    val sum = add(10, 20)
    val d = double(sum)
    println("sum=$sum, double=$d")
}
```

## 1.3 控制流

### if 表达式

```aura
val max = if (a > b) a else b
```

### when 表达式

```aura
val result = when (x) {
    0 -> "zero"
    1 -> "one"
    2 -> "two"
    else -> "other"
}

// 范围匹配
when (score) {
    in 90..100 -> grade = 'A'
    in 80..89 -> grade = 'B'
    in 70..79 -> grade = 'C'
    else -> grade = 'F'
}
```

### 循环

```aura
// for 循环
for (i in 0..10) {
    println(i)
}

// while 循环
while (running) {
    update()
    render()
}
```

### 练习 1.3

```aura
fun exercise3() {
    val score = 85
    val grade = when (score) {
        in 90..100 -> "A"
        in 80..89 -> "B"
        in 70..79 -> "C"
        else -> "F"
    }
    println("Score: $score, Grade: $grade")
}
```

## 1.4 数据类与结构体

```aura
struct Player(
    val id: Int,
    var name: String,
    var x: Float,
    var y: Float,
    var health: Int = 100
)

val player = Player(1, "Alice", 0f, 0f)
val copy = player.copy(name = "Bob", health = 50)
```

### 练习 1.4

```aura
fun exercise4() {
    val p = Player(1, "Alice", 10f, 20f)
    println("Player: ${p.name} at (${p.x}, ${p.y})")
    val updated = p.copy(health = 80)
    println("Updated health: ${updated.health}")
}
```

## 1.5 枚举

```aura
enum Color {
    RED,
    GREEN,
    BLUE,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

val c = Color.CUSTOM(255, 128, 0)
```

### 练习 1.5

```aura
fun exercise5() {
    val colors = listOf(Color.RED, Color.GREEN, Color.BLUE)
    for (color in colors) {
        println("Color: $color")
    }
}
```

## 1.6 接口与实现

```aura
interface Renderable {
    fun render()
    fun bounds(): Rect
    fun zOrder(): Int = 0  // 默认实现
}

class Sprite : Renderable {
    var texture: Texture
    var x: Float
    var y: Float
    
    override fun render() {
        DrawTexture(texture, x, y, Color.WHITE)
    }
    
    override fun bounds(): Rect {
        return Rect(x, y, width, height)
    }
}
```

## 1.7 错误处理

### Result 类型（推荐）

```aura
fun loadTexture(path: String): Result<Texture, Error> {
    val tex = raylib.LoadTexture(path.toCStr())
    return if (tex.isNull()) {
        Result.Error(Error("Failed to load: $path"))
    } else {
        Result.Success(tex)
    }
}
```

### 异常处理

```aura
fun divide(a: Int, b: Int): Int {
    if (b == 0) throw DivideByZeroException()
    return a / b
}

try {
    val result = divide(10, 0)
} catch (e: DivideByZeroException) {
    println("Cannot divide by zero!")
}
```

## 1.8 并发与异步

### 协程

```aura
suspend fun fetchData(url: String): Result<Data, Error> {
    val response = await httpGet(url)
    val data = parseJson(response)
    return Result.Success(data)
}
```

### Actor 模型

```aura
actor WindowManager {
    private var windows: List<Window> = emptyList()
    
    fun onCreate(config: WindowConfig): Window {
        val win = Window(config)
        windows = windows + win
        return win
    }
}
```

## 1.9 FFI 与系统集成

```aura
// 声明外部函数
extern "c" "raylib" {
    fun DrawCircle(x: Int, y: Int, radius: Float, color: Color)
    fun GetFrameTime(): Float
    val WHITE: Color
    val BLACK: Color
}

// 使用
fun render() {
    DrawCircle(400, 300, 50f, Color.RED)
}
```

## 1.10 泛型与集合

```aura
fun <T> identity(value: T): T = value

fun <T : Comparable<T>> max(a: T, b: T): T {
    return if (a.compare(b) == Ordering.GREATER) a else b
}

// 集合操作
val list: List<Int> = listOf(1, 2, 3, 4, 5)
val result = list
    .filter { it > 2 }
    .map { it * it }
    .take(3)
```

### 练习 1.10

```aura
fun exercise10() {
    val list = listOf(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
    val result = list
        .filter { it % 2 == 0 }
        .map { it * it }
        .take(3)
    println("Result: $result")
}
```

---

## 总结

| 概念 | 语法 |
|------|------|
| 变量 | `val` / `var` |
| 函数 | `fun name(params): Type { }` |
| 条件 | `if (cond) { } else { }` |
| 匹配 | `when (x) { ... }` |
| 循环 | `for (x in iter) { }` / `while (cond) { }` |
| 数据类 | `struct Name(val x: Type, ...)` |
| 枚举 | `enum Name { A, B, C }` |
| 接口 | `interface Name { fun method() }` |
| 错误 | `Result<T, E>` / `try { } catch { }` |
| 并发 | `suspend fun` / `actor` |
| FFI | `extern "c" "lib" { ... }` |
| 泛型 | `fun <T> f(x: T): T` |