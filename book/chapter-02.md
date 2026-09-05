# 第二章：示例项目

> 完整的项目示例 — 游戏、工具、服务

---

## 2.1 游戏示例：射线库绘图

### 项目结构

```
raylib-game/
├── aura.toml
├── main.aura
├── game/
│   ├── player.aura
│   ├── level.aura
│   └── renderer.aura
└── assets/
```

### aura.toml

```json
{
    "name": "raylib-game",
    "version": "1.0.0",
    "description": "Aura + Raylib 游戏示例",
    "authors": ["Aura Team"],
    "license": "MIT",
    "entry": "main.aura",
    "dependencies": [
        {
            "name": "aura-raylib",
            "version": { "GreaterThanEqual": { "major": 5, "minor": 0, "patch": 0, "prerelease": [], "build": [] } },
            "source": { "Git": "https://github.com/aura-lang/aura-raylib.git" }
        }
    ]
}
```

### main.aura

```aura
import aura-raylib as raylib

// 游戏状态
var running: Boolean = true
var score: Int = 0

struct Player {
    var x: Float = 400f
    var y: Float = 300f
    var speed: Float = 200f
}

fun init() {
    raylib.InitWindow(800, 600, "Aura Game")
    println("游戏开始!")
}

fun update(dt: Float) {
    // 键盘输入
    if (raylib.IsKeyDown(KeyboardKey.A)) {
        player.x -= player.speed * dt
    }
    if (raylib.IsKeyDown(KeyboardKey.D)) {
        player.x += player.speed * dt
    }
}

fun render() {
    raylib.BeginDrawing()
    raylib.ClearBackground(raylib.RAYWHITE)
    
    // 绘制玩家
    raylib.DrawCircle(player.x.toInt(), player.y.toInt(), 20, raylib.RED)
    
    // 绘制分数
    raylib.DrawText("Score: $score", 10, 10, 20, raylib.BLACK)
    
    raylib.EndDrawing()
}

fun main() {
    init()
    
    while (running) {
        val dt = raylib.GetFrameTime()
        
        if (raylib.WindowShouldClose()) {
            running = false
        }
        
        update(dt)
        render()
    }
    
    raylib.CloseWindow()
    println("游戏结束!")
}
```

## 2.2 工具示例：文件处理器

### file-processor.aura

```aura
import std.fs as fs
import std.io as io
import std.string as str
import std.collections as collections

struct FileStats {
    val name: String
    val size: Int
    val lines: Int
    val words: Int
}

fun analyzeFile(path: String): FileStats {
    val content = fs.readText(path)
    val lines = content.split("\n").size
    val words = content.split(" ").size
    
    return FileStats(
        name = path,
        size = content.length,
        lines = lines,
        words = words
    )
}

fun report(stats: FileStats) {
    println("=== 文件统计 ===")
    println("文件: ${stats.name}")
    println("大小: ${stats.size} 字节")
    println("行数: ${stats.lines}")
    println("词数: ${stats.words}")
}

fun main() {
    val args = io.args
    if (args.isEmpty()) {
        println("用法: aura run file-processor.aura <file>")
        return
    }
    
    val path = args[0]
    if (!fs.exists(path)) {
        println("文件不存在: $path")
        return
    }
    
    val stats = analyzeFile(path)
    report(stats)
}
```

## 2.3 服务示例：Actor 消息处理

### actor-service.aura

```aura
import std.concurrent as concurrent
import std.io as io
import std.collections as collections

// 消息类型
enum Message {
    TEXT(String),
    COMMAND(String),
    SHUTDOWN
}

// Actor 定义
actor Server {
    private var messages: List<Message> = emptyList()
    private var running: Boolean = true
    
    fun start() {
        println("服务器启动")
    }
    
    fun handle(msg: Message) {
        when (msg) {
            is Message.TEXT -> {
                println("收到文本: ${msg.value}")
            }
            is Message.COMMAND -> {
                println("收到命令: ${msg.value}")
            }
            is Message.SHUTDOWN -> {
                println("收到关闭命令")
                running = false
            }
        }
    }
    
    fun isRunning(): Boolean = running
}

fun main() {
    // 创建 Actor
    val server = concurrent.spawnActor(Server::class)
    
    // 发送消息
    concurrent.send(server, Message.TEXT("Hello"))
    concurrent.send(server, Message.COMMAND("status"))
    concurrent.send(server, Message.TEXT("World"))
    
    // 等待处理
    Thread.sleep(100)
    
    // 关闭
    concurrent.send(server, Message.SHUTDOWN)
    println("服务器已关闭")
}
```

## 2.4 并发示例：协程与 Channel

### channel-demo.aura

```aura
import std.concurrent as concurrent
import std.io as io
import std.time as time

fun producer(channel: Channel<Int>) {
    for (i in 0..10) {
        concurrent.channelSend(channel, i)
        time.sleep(10)
    }
    concurrent.channelClose(channel)
}

fun consumer(channel: Channel<Int>) {
    var count = 0
    while (true) {
        val value = concurrent.channelRecv(channel)
        if (value == null) break
        count += value
        println("收到: $value, 累计: $count")
    }
    println("总计: $count")
}

fun main() {
    // 创建无界通道
    val channel = concurrent.newChannel<Int>(0)
    
    // 启动生产者
    concurrent.spawn(producer(channel))
    
    // 消费
    consumer(channel)
}
```

## 2.5 FFI 示例：调用 C 函数

### ffi-demo.aura

```aura
// 声明 C 库函数
extern "c" "libc" {
    fun strlen(s: CString): Int
    fun sqrt(x: Float): Float
    fun clock(): Long
}

// 使用 C 函数
fun main() {
    // 计算平方根
    val result = sqrt(144f)
    println("sqrt(144) = $result")
    
    // 获取时间
    val t = clock()
    println("clock() = $t")
}
```

## 2.6 标准库示例：JSON 处理

### json-demo.aura

```aura
import std.json as json
import std.io as io

fun main() {
    // 解析 JSON
    val data = json.parse(r"""
    {
        "name": "Alice",
        "age": 30,
        "skills": ["Kotlin", "Rust", "Aura"],
        "address": {
            "city": "Beijing",
            "country": "China"
        }
    }
    """)
    
    // 访问字段
    val name = json.get(data, "name")
    val age = json.get(data, "age")
    val skills = json.get(data, "skills")
    
    println("Name: $name")
    println("Age: $age")
    println("Skills: $skills")
    
    // 修改 JSON
    json.set(data, "age", 31)
    
    // 序列化
    val output = json.stringify(data)
    println("Modified: $output")
}
```

## 2.7 包管理示例

### 创建新包

```bash
# 创建新包
aura new my-math-lib

# 安装依赖
aura install

# 查看依赖树
aura deps

# 更新依赖
aura update

# 发布包
aura publish
```

### aura.toml

```json
{
    "name": "my-math-lib",
    "version": "1.0.0",
    "description": "数学工具库",
    "authors": ["Developer"],
    "license": "MIT",
    "repository": "https://github.com/user/my-math-lib",
    "entry": "main.aura",
    "dependencies": [
        {
            "name": "aura-math",
            "version": { "GreaterThanEqual": { "major": 1, "minor": 0, "patch": 0, "prerelease": [], "build": [] } },
            "source": { "Git": "https://github.com/aura-lang/aura-math.git" }
        }
    ],
    "dev_dependencies": []
}
```