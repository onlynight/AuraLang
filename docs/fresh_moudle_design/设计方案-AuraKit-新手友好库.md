# Aura 新手友好库（aura.kit）设计方案

> **目标**：像 Python 一样，让新人"几行代码就能写一个网页、一个 CLI 工具、一个数据处理脚本"。
>
> **核心思路**：在 Aura 现有的静态类型 + 协程 + Actor 体系上，补一层"新手友好"的高层库 —— `aura.kit`。
> 语言层面不降级（不取消类型检查、不引入全局可变状态），而是在库这一层把"样板代码"降到 5 行以内。
>
> **中文文档** — 本文件

---

## 目录

1. [背景：Aura 当前的新手门槛](#一背景aura-当前的新手门槛)
2. [Python 设计哲学分析](#二python-设计哲学分析)
3. [设计目标与度量](#三设计目标与度量)
4. [`aura.kit` 整体设计](#四aurakit-整体设计)
5. [模块详细设计](#五模块详细设计)
   - [5.1 aura.kit.web — 迷你 Web 框架](#51-aurakittestweb--迷你-web-框架)
   - [5.2 aura.kit.cli — CLI 工具](#52-aurakittcli--cli-工具)
   - [5.3 aura.kit.net — HTTP 客户端](#53-aurakittestnet--http-客户端)
   - [5.4 aura.kit.data — 数据处理](#54-aurakitdata--数据处理)
   - [5.5 aura.kit.file — 文件操作](#55-aurakittestfile--文件操作)
   - [5.6 aura.kit.text — 文本处理](#56-aurakittesttext--文本处理)
   - [5.7 aura.kit.log — 日志](#57-aurakittestlog--日志)
   - [5.8 aura.kit.task — 任务调度](#58-aurakittesttask--任务调度)
   - [5.9 aura.kit.test — 测试](#59-aurakittesttest--测试)
   - [5.10 aura.kit.ui — 终端 UI](#510-aurakittestui--终端-ui)
   - [5.11 aura.kit.config — 配置加载](#511-aurakittestconfig--配置加载)
6. [语言层面配套](#六语言层面配套)
7. [Python vs Aura kit 代码量对比](#七python-vs-aura-kit-代码量对比)
8. [实现路线图](#八实现路线图)
9. [风险与取舍](#九风险与取舍)
10. [附录：5 行代码速查表](#十附录5-行代码速查表)

---

## 一、背景：Aura 当前的新手门槛

Aura 语言本身是"Kotlin 风格 + Rust 实现"的，对系统级编程友好，但**对新人不友好**。问题不在于"能不能做"，而在于"要做完第一件小事，需要写多少无关代码"。

### 1.1 现状对照：写一个 "Hello World" 网页

**Python + Flask**（5 行有效代码）：

```python
from flask import Flask
app = Flask(__name__)

@app.route("/")
def home(): return "Hello"

app.run(port=8000)
```

**Aura 现状**（基于 `examples/app/server.aura` 风格，~40 行）：

```aura
import aura.std.net
import aura.std.io as io
import aura.std.fs as fs

data struct Config(val port: Int = 8080, val host: String = "127.0.0.1")

fun main() {
    val cfg = Config()
    println("Server starting on ${cfg.host}:${cfg.port}")
    val server = HttpServer(cfg.host, cfg.port)
    server.route("/") { println("Hello") }
    server.start()
    // ... 需要补 await / shutdown / 错误处理
}
```

差距：**有效信息量差不多，但样板量差 8 倍**。

### 1.2 五大痛点

| 痛点 | 现状 | 影响 |
|------|------|------|
| **`fun main()` 样板** | 必须写一个 `main` 函数 | 每个脚本多 3 行 |
| **import 冗长** | `import aura.std.fs as fs` | 一个文件常需 3-5 行 import |
| **类型注解负担** | `val cfg: Config = Config()` | 简单脚本仍要写类型 |
| **`data struct` 构造繁琐** | `Player(1, "Alice", 50)` | 缺少命名参数/默认值 |
| **没有"开箱即用"的高层库** | 只有 19 个 std 模块，都是原子能力 | 没有"5 行写网页"这种东西 |

### 1.3 关键区分：不是要重写语言，而是要补"高层库"

Python 的成功不是因为它没有类型系统，而是因为它有：
- 丰富的标准库（batteries included）
- 繁荣的第三方库（batteries on tap）
- 优雅的高层 API（requests / Flask / Click / pandas）

**Aura 现在只有"底层库"，没有"高层库"**。`aura.kit` 要补的就是这一层。

---

## 二、Python 设计哲学分析

Python 的官方哲学是 **PEP 20 · The Zen of Python**（20 条），但真正让它"新手友好"的是 6 条支柱。

### 2.1 PEP 20 的 20 条（摘要）

```
Beautiful is better than ugly.          美丽胜于丑陋。
Explicit is better than implicit.       显式优于隐式。
Simple is better than complex.          简单优于复杂。
Complex is better than complicated.     复杂优于混乱。
Readability counts.                     可读性重要。
Special cases aren't special enough to break the rules.
Errors should never pass silently.      错误不应静默通过。
In the face of ambiguity, refuse the temptation to guess.
There should be one -- and preferably only one -- obvious way to do it.
                                          ↓ 这是核心
Now is better than never.               现在比"以后再说"好。
```

对 Aura 最有启发的 3 条：
- **Simple is better than complex** → 高层 API 应该简单
- **There should be one obvious way** → 避免"同一件事 3 种写法"
- **Errors should never pass silently** → 库要主动报错，不吞异常

### 2.2 6 大新手友好支柱

#### 支柱 1：Batteries Included（标准库丰富）

Python 的标准库有 200+ 模块：`http.server`、`argparse`、`json`、`csv`、`pathlib`、`logging`、`unittest`……
**一个语言装完就能干活**，不用装第三方。

→ **Aura 应对**：`aura.kit` 尽量只用 `aura.std.*` 实现，零外部依赖。

#### 支柱 2：Batteries on Tap（生态繁荣）

即使标准库不够，`pip install requests flask click` 3 秒就能装上。

→ **Aura 应对**：`aura.kit` 是标准高层库；未来通过 `.auz` 包管理扩展生态。

#### 支柱 3：Optional Typing（类型可选）

Python 可以完全不用类型注解；也可以逐步添加（PEP 484）。
**新手先写能跑，再补类型**。

→ **Aura 现状**：类型是必须的。
→ **Aura 应对**：`aura.kit` 的 API 设计尽量让类型可以从**默认值**或**参数名**推断，减少写类型的场景。

#### 支柱 4：Scripting-Friendly（脚本友好）

Python 没有 `main()` 概念，顶层语句直接执行；`python script.py` 就能跑。

→ **Aura 现状**：需要 `fun main()`。
→ **Aura 应对**：[脚本模式](./语言-script-mode-技术方案.md) 正在做；`aura.kit` 库的"5 行版本"应假设脚本模式可用。

#### 支柱 5：One Obvious Way（一种明显的方式）

Python 不会让你写 `if x: pass else: pass`、`if x is not None or x is not False` 这种花活。

→ **Aura 应对**：`aura.kit` 每个 API 只暴露一种写法，不搞"重载地狱"。

#### 支柱 6：Fluent API（流畅的 API 设计）

Python 的"几行代码"靠两个语言特性实现：

```python
# 链式调用
data = requests.get(url).json()
# 列表推导
total = sum(x*x for x in range(10))
```

→ **Aura 应对**：Aura 已有字符串插值、`when`、lambda；`aura.kit` 大量用**链式调用**和**高阶函数**来压缩代码。

### 2.3 关键机制对照

| Python 机制 | Aura 对应 | aura.kit 用法 |
|------------|----------|--------------|
| f-string | `$var` / `${expr}` | 直接用 |
| 列表推导 | `listOf(...).map { ... }` | `aura.kit.data` 复用 |
| `@decorator` | 无（Aura 没有装饰器语法） | 用高阶函数 + `suspend fun` |
| `with open(...) as f:` | 无（Aura 没有 `try-with-resources`） | `aura.kit.file` 提供 `read`/`write` 一站式 |
| `lambda x: x+1` | `{ x: Int -> x+1 }` | 直接用 |
| 默认参数 | `fun f(x: Int = 1)` | 直接用 |
| 命名参数 | `f(x = 1, y = 2)` | 直接用 |
| `None` | `null` / `?` | 库 API 优先返回非空类型 |
| `asyncio.run()` | 无（Aura 的 `suspend` 是原生的） | 库 API 全部 `suspend`，不用额外 `run()` |
| `if __name__ == "__main__"` | 无（脚本模式） | 直接顶层语句 |

---

## 三、设计目标与度量

### 3.1 三大目标

| 目标 | 度量 |
|------|------|
| **5 行写网页** | 一个 HTTP 路由的服务器，从 `import` 到 `run` 共 5 行 |
| **5 行写 CLI 工具** | 一个带参数和选项的命令行工具，共 5 行 |
| **5 行写数据处理脚本** | 读 CSV → 过滤 → 排序 → 输出，共 5 行 |

### 3.2 四条设计原则

1. **零样板**：不写 `main()`、不写 import 之外的任何"框架代码"。
2. **零依赖**：`aura.kit` 只依赖 `aura.std.*`，不引入第三方。
3. **零学习曲线**：API 命名贴近 Python 习惯（`get`/`post`/`read`/`write`/`filter`/`sort`），让 Python 用户"看到就会"。
4. **零静默错误**：API 返回类型要么是"成功值"，要么是 `Result<T, E>`；绝不返回 `null` 当"失败"。

### 3.3 不做什么（明确边界）

| 不做 | 理由 |
|------|------|
| ❌ 重写 Web 框架（如"完整 Flask"） | 那是另一个项目；`aura.kit.web` 只做"5 行写一个演示服务器"的 DSL |
| ❌ 替代标准库 | `aura.std` 仍是底层；`aura.kit` 是"高层便利层" |
| ❌ 引入全局可变状态 | 保持静态类型 + 不可变优先 |
| ❌ 装饰器语法 | Aura 没有装饰器；用高阶函数实现同等效果 |
| ❌ 隐式类型转换 | 保持类型安全；不做 `int → str` 的隐式转换 |

---

## 四、`aura.kit` 整体设计

### 4.1 库名与定位

- **库名**：`aura.kit`（"工具包"）
- **定位**：Aura 的"高层便利层"，对标 Python 的 `flask` + `click` + `requests` + `pandas-lite`
- **分发**：作为 `.auz` 包发布，`loom` 安装
- **依赖**：只依赖 `aura.std.*`，零外部依赖

### 4.2 目录结构

```
aura-kit/
├── aura.toml                       # 包清单
├── kit/
│   ├── Web.aura                    # 迷你 Web 框架
│   ├── Cli.aura                    # CLI 工具
│   ├── Net.aura                    # HTTP 客户端
│   ├── Data.aura                   # 数据处理 (CSV/JSON/YAML + DataFrame-like)
│   ├── File.aura                   # 文件操作
│   ├── Text.aura                   # 文本处理
│   ├── Log.aura                    # 日志
│   ├── Task.aura                   # 任务调度
│   ├── Test.aura                   # 测试框架
│   ├── Ui.aura                     # 终端 UI（表格、颜色、进度条）
│   └── Config.aura                 # 配置加载（YAML/TOML/JSON）
└── examples/                       # 每个模块一个 5 行示例
```

### 4.3 包清单（`aura.toml`）

```toml
name = "aura-kit"
version = "0.1.0"
description = "Beginner-friendly high-level library for Aura (like Python's Flask + requests + pandas)"
entry = "kit/Main.aura"

[dependencies]
# aura.std.* 已内置，无需声明
```

### 4.4 统一 API 风格

所有 `aura.kit` 模块遵循同一套 API 约定：

| 约定 | 说明 | 示例 |
|------|------|------|
| **全部 `suspend`** | I/O 相关 API 一律 `suspend`，让 Aura 的协程体系原生接入 | `suspend fun get(url: String): Response` |
| **默认参数** | 所有非必填参数给默认值 | `fun run(host: String = "0.0.0.0", port: Int = 8000)` |
| **命名参数** | 构造器/高阶函数支持命名参数 | `Web().get("/x", handler)` / `Web(port = 9000)` |
| **链式调用** | 返回 `this` 或新对象 | `df.filter{}.sort{}.take(5)` |
| **不返回 null** | 失败用 `Result<T, E>` 或抛异常 | `fun read(path: String): Result<String, IOException>` |
| **不吞异常** | 错误必须能定位到源 | 所有异常带 `cause` 链 |
| **顶层函数** | 每个模块导出顶层便利函数，避免 `WebApp.get(...)` 这种长路径 | `import aura.kit.web` 后可直接 `get("/x") {}` |

---

## 五、模块详细设计

### 5.1 `aura.kit.web` — 迷你 Web 框架

#### 目标

对标 Flask 的"5 行 Hello World"。让新人不学路由、不学请求/响应、不学生命周期，就能跑起来一个 HTTP 服务。

#### 5.1.1 最小示例（5 行）

```aura
import aura.kit.web

val app = Web()
app.get("/") { "Hello!" }
app.run(port = 8000)
```

**对比 Python Flask**：

```python
from flask import Flask
app = Flask(__name__)
@app.route("/")
def home(): return "Hello"
app.run(port=8000)
```

行数几乎相同，Aura 版本少 1 行（不需要 `def` 函数名）。

#### 5.1.2 进阶示例（10 行：带参数路由 + JSON + 错误处理）

```aura
import aura.kit.web

val app = Web()
app.get("/api/user/:id") { req ->
    val id = req.param<Int>("id")
    json(userId = id, name = "Alice", age = 25)
}
app.post("/api/user") { req ->
    val body = req.bodyJson<UserCreate>()
    json(created = true, id = body.name.hashCode())
}
app.run(host = "0.0.0.0", port = 8000, debug = true)
```

#### 5.1.3 核心 API

```aura
package aura.kit.web

// ── 应用入口 ──────────────────────────────────────────────
class Web(
    val host: String = "0.0.0.0",
    val port: Int = 8000,
    val debug: Boolean = false
) {
    fun get(path: String, handler: Handler): Web
    fun post(path: String, handler: Handler): Web
    fun put(path: String, handler: Handler): Web
    fun delete(path: String, handler: Handler): Web
    fun route(path: String, methods: List<String>, handler: Handler): Web
    fun static(prefix: String, dir: String): Web        // 静态文件
    fun error(status: Int, handler: ErrorHandler): Web  // 错误处理
    fun use(middleware: Middleware): Web                // 中间件
    fun run(): Unit                                     // 阻塞启动
}

// ── 类型别名 ──────────────────────────────────────────────
typealias Handler = (Request) -> Response
typealias ErrorHandler = (Request, Exception) -> Response
typealias Middleware = (Request, next: Handler) -> Response

// ── 请求对象 ──────────────────────────────────────────────
data struct Request(
    val method: String,
    val path: String,
    val headers: Map<String, String>,
    val queryParams: Map<String, String>,
    val pathParams: Map<String, String>,
    val bodyBytes: ByteArray
) {
    fun param<T>(name: String): T?                  // 路径参数 :id
    fun query<T>(name: String): T?                  // 查询参数 ?x=1
    fun header(name: String): String?
    fun bodyText(): String
    fun bodyJson<T>(): T                            // 自动反序列化
}

// ── 响应类型 ──────────────────────────────────────────────
// 三种返回形式都支持：
//   1. 直接返回 String  → 自动包装为 text/plain
//   2. 返回 data struct → 自动序列化为 JSON
//   3. 返回 Response    → 完全控制
data struct Response(
    val status: Int = 200,
    val body: Any = "",
    val headers: Map<String, String> = mapOf(),
    val contentType: String = "text/plain; charset=utf-8"
)

// ── 顶层便利函数 ──────────────────────────────────────────
fun json(obj: Any): Response                          // 快捷 JSON 响应
fun html(content: String): Response
fun redirect(location: String, status: Int = 302): Response
fun status(code: Int, body: Any = ""): Response
```

#### 5.1.4 路由匹配算法

- 路径参数语法：`:id`（对标 Flask 的 `<int:id>`，但用统一占位符，类型由 `req.param<Int>("id")` 推断）
- 通配符：`*`（如 `/static/*`）
- 匹配顺序：先精确、再参数、最后通配符
- 冲突检测：启动时报错（"路由 /x 已被注册"）

#### 5.1.5 实现要点

- 底层用 `aura.std.net` 的 TCP socket（已有 `listen`/`bind`/`accept`/`send`/`recv` 原生调用）
- 用 `actor` 管理连接池（每个连接一个 actor）
- 用 `channel` 做请求分发
- JSON 序列化/反序列化用 `aura.std.json`
- HTTP 解析器：自己写一个最小 HTTP/1.1 解析器（~200 行），不依赖外部库
- 模板渲染：暂不支持（后续 v0.2 加 `app.render("home.html", vars)`）

#### 5.1.6 已知限制（v0.1）

- ❌ 无模板引擎（v0.2 计划）
- ❌ 无 WebSocket（v0.3 计划）
- ❌ 无 CORS 中间件（用 `app.use(cors())` 手动加，v0.1 提供 `cors()` 助手）
- ❌ 无自动 HTTPS（需自己配置）

---

### 5.2 `aura.kit.cli` — CLI 工具

#### 目标

对标 Click / Typer。让新人不学 `System.argv`、不写 `try/catch`、不写 usage 文本，就能写一个带参数和选项的命令行工具。

#### 5.2.1 最小示例（5 行）

```aura
import aura.kit.cli

fun main(name: String = "World", count: Int = 3) {
    for (i in 0 until count) println("Hello $name!")
}
```

**对比 Python Click**：

```python
import click
@click.command()
@click.option("-n", default=3, help="Count")
@click.argument("name")
def cli(n, name):
    for _ in range(n): print(f"Hello {name}!")
cli()
```

Aura 版本**少 5 行**（不需要 decorator、不需要 `def cli`、不需要 `cli()`）。

#### 5.2.2 进阶示例（10 行：带选项 + 帮助文本 + 错误处理）

```aura
import aura.kit.cli
import aura.kit.file

fun main(
    input: String = "input.txt",       // 位置参数（无 flag）
    output: String = "output.txt",     // 位置参数
    verbose: Boolean = false,          // -v / --verbose
    force: Boolean = false,            // -f / --force
    help: Boolean = false              // 自动处理
) {
    if (help) {
        println("Usage: $appName [input] [output] [-v] [-f]")
        return
    }
    val content = read(input)
    if (verbose) println("Read ${content.length} bytes")
    write(output, content.uppercase(), force)
    println("Done.")
}
```

#### 5.2.3 核心 API

```aura
package aura.kit.cli

// ── 顶层便利函数 ──────────────────────────────────────────
// 让 main() 自动支持 CLI 解析；参数名直接对应命令行
fun run(main: () -> Unit)                        // 阻塞直到解析完成
fun run(name: String, desc: String, main: () -> Unit)

// ── 参数与选项类型 ──────────────────────────────────────
// main() 的每个参数就是一个"选项"或"位置参数"
// 通过 Aura 的"默认值"判断是选项（有默认值）还是位置参数（无默认值）

// ── 参数元数据 ──────────────────────────────────────────
data struct Option(
    val name: String,           // 变量名（如 "input"）
    val flags: List<String>,    // 命令行 flag（如 ["-i", "--input"]）
    val default: Any?,
    val help: String,
    val required: Boolean,
    val isFlag: Boolean         // true = -v 这种布尔开关
)

// ── 便利函数 ──────────────────────────────────────────────
fun arg(name: String): Any                                  // 位置参数
fun option(short: String, long: String, default: Any? = null, help: String = ""): Any
fun choice(options: List<String>, default: String = ""): Any

// ── 应用元信息 ──────────────────────────────────────────
val appName: String      // 自动从文件名推断
val version: String = "0.1.0"
fun banner(): Unit       // 打印 banner + 帮助文本
```

#### 5.2.4 关键设计：函数参数即选项

Aura 的 `fun main(input: String = "input.txt", verbose: Boolean = false)` 已经具备 Python Click 的全部语义：
- **默认值** → 选项的默认值
- **Boolean 类型** → `-v` 这种开关
- **无默认值** → 位置参数（required）
- **变量名** → `--name` 的长形式

所以 `aura.kit.cli` 的实现只需要：
1. 反射读取 `main` 的函数签名（Aura 编译期信息 + Aura runtime 反射）
2. 解析 `System.argv`
3. 构造一个"调用上下文"，把解析结果传进去
4. 调用 `main(...)`

**这是 Python Click 的"装饰器"在 Aura 里的等价物** —— 不是装饰器，而是"约定 + 反射"。

#### 5.2.5 实现要点

- 反射：用 Aura 的编译期 `TypeInfo` 拿到 `main` 的参数名、类型、默认值
- 解析器：手写一个"短 flag / 长 flag / 位置参数"解析器（~150 行）
- 自动 help：`-h` / `--help` 永远处理，打印所有参数和默认值
- 错误：参数缺失、类型不匹配，统一抛 `CliException`，打印到 stderr

#### 5.2.6 已知限制（v0.1）

- ❌ 无子命令（v0.2 计划：`app.command("serve")` / `app.command("build")`）
- ❌ 无交互式提示（v0.2 计划：`prompt("Enter name:")`）
- ❌ 无确认对话框（v0.3 计划：`confirm("Delete?")`）

---

### 5.3 `aura.kit.net` — HTTP 客户端

#### 目标

对标 `requests`。让新人写 `get(url).json()` 就行，不学 socket、不学 HTTP 协议、不学异步。

#### 5.3.1 最小示例（3 行）

```aura
import aura.kit.net

val r = get("https://api.github.com")
println(r.json()["name"])
```

**对比 Python**：

```python
import requests
r = requests.get("https://api.github.com")
print(r.json()["name"])
```

**完全一致**（Python 也是 3 行）。

#### 5.3.2 进阶示例（8 行：POST + headers + timeout + 错误处理）

```aura
import aura.kit.net

suspend fun createUser(name: String): Result<Int, HttpError> {
    val r = post("https://api.example.com/users",
        body = json(name = name),
        headers = mapOf("Authorization" to "Bearer $token"),
        timeout = 10
    )
    if (r.status == 200) return Result.Success(r.json()["id"] as Int)
    return Result.Error(HttpError(r.status, r.text))
}
```

#### 5.3.3 核心 API

```aura
package aura.kit.net

// ── 顶层便利函数 ──────────────────────────────────────────
suspend fun get(url: String, headers: Map<String, String> = mapOf(), timeout: Int = 30): Response
suspend fun post(url: String, body: Any? = null, headers: Map<String, String> = mapOf(), timeout: Int = 30): Response
suspend fun put(url: String, body: Any? = null, headers: Map<String, String> = mapOf(), timeout: Int = 30): Response
suspend fun delete(url: String, headers: Map<String, String> = mapOf(), timeout: Int = 30): Response
suspend fun head(url: String, headers: Map<String, String> = mapOf(), timeout: Int = 30): Response

// ── 响应对象 ──────────────────────────────────────────────
data struct Response(
    val status: Int,
    val text: String,
    val headers: Map<String, String>
) {
    fun json(): Map<String, Any>           // 自动反序列化
    fun json<T>(): T                       // 反序列化为指定类型
    fun content(): ByteArray
    val ok: Boolean                        // status in 200..299
    fun raiseForStatus(): Response         // 非 2xx 抛异常
}

// ── 异常类型 ──────────────────────────────────────────────
class HttpError(val status: Int, val body: String) : Exception("HTTP $status: $body")
class TimeoutError(val url: String) : Exception("Timeout: $url")
class ConnectionError(val url: String) : Exception("Connection failed: $url")

// ── 会话（用于 cookie / 复用连接） ──────────────────────
class Session {
    fun get(url: String, ...): Response
    fun post(url: String, ...): Response
    fun close(): Unit
}
suspend fun session(): Session    // 创建会话
```

#### 5.3.4 关键设计：`suspend` 是原生的

Python 需要 `asyncio.run()` 才能跑异步；Aura 的 `suspend` 是语言原生的，所以 `aura.kit.net` 的 API 全部 `suspend`，**调用者无感**。

顶层 `main` 如果不调用 `suspend` 函数，可以直接写：

```aura
import aura.kit.net

val r = get("https://api.github.com")  // 内部 suspend，但 main 是顶层，自动运行
```

（依赖脚本模式 + 自动 coroutine runner，详见第六章"语言层面配套"。）

#### 5.3.5 实现要点

- 底层用 `aura.std.net` 的 socket API（已有 TCP 原生调用）
- TLS 用 `aura.std.tls`（如果还没实现，v0.1 暂只支持 HTTP，HTTPS 走"系统调用"）
- DNS 解析：用 `getaddrinfo` 原生调用
- 连接池：`actor` 管理复用连接
- 超时：用 `withTimeout` 协程辅助函数
- 错误处理：所有失败都抛具体异常，不返回 null

#### 5.3.6 已知限制（v0.1）

- ❌ 无 HTTP/2（v0.3 计划）
- ❌ 无 WebSocket（v0.3 计划）
- ❌ 无自动重试（v0.2 计划：`retry = 3` 参数）
- ❌ 无代理（v0.2 计划）

---

### 5.4 `aura.kit.data` — 数据处理

#### 目标

对标 `pandas-lite`。让新人不学 `List<Int>`、不写 for 循环，就能读 CSV → 过滤 → 排序 → 输出。

#### 5.4.1 最小示例（5 行）

```aura
import aura.kit.data

val df = readCsv("data.csv")
val top = df.filter { it["score"] > 90 }
             .sort { it["age"] }
             .take(5)
println(top.toTable())
```

**对比 Python pandas**：

```python
import pandas as pd
df = pd.read_csv("data.csv")
top = df[df["score"] > 90].sort_values("age").head(5)
print(top.to_string())
```

Aura 版本**完全对标**，甚至更短（Python 需要 `df["score"] > 90` 这种布尔索引，Aura 用 `filter { ... }` 更直观）。

#### 5.4.2 进阶示例（10 行：多表 + 聚合 + 导出）

```aura
import aura.kit.data

val orders = readCsv("orders.csv")
val users  = readCsv("users.csv")

val enriched = orders.join(users, on = "userId", how = "left")
val summary  = enriched
    .groupBy("region")
    .agg { group ->
        data struct Row(
            total = group.sum("amount"),
            count = group.size(),
            avg   = group.avg("amount")
        )
    }
summary.toCsv("summary.csv")
```

#### 5.4.3 核心 API

```aura
package aura.kit.data

// ── 行类型 ──────────────────────────────────────────────
typealias Row = Map<String, Any?>    // 简单起见用 Map

// ── DataFrame 类型 ──────────────────────────────────────
class DataFrame(
    val rows: List<Row>,
    val columns: List<String>
) {
    fun filter(pred: (Row) -> Boolean): DataFrame
    fun sort(key: (Row) -> Comparable<*>): DataFrame
    fun sortDesc(key: (Row) -> Comparable<*>): DataFrame
    fun take(n: Int): DataFrame
    fun drop(n: Int): DataFrame
    fun select(cols: List<String>): DataFrame
    fun drop(cols: List<String>): DataFrame
    fun map(transform: (Row) -> Row): DataFrame
    fun groupBy(key: String): Group
    fun head(n: Int = 5): DataFrame
    fun tail(n: Int = 5): DataFrame
    fun size(): Int
    fun empty(): Boolean

    // 聚合
    fun sum(col: String): Double
    fun avg(col: String): Double
    fun min(col: String): Double
    fun max(col: String): Double
    fun count(col: String): Int

    // 输入输出
    fun toCsv(path: String): Unit
    fun toTable(): String        // 渲染为终端表格
    fun toJson(): String
    fun toJson(path: String): Unit
}

class Group(
    val df: DataFrame,
    val key: String
) {
    fun size(): Int
    fun sum(col: String): Double
    fun avg(col: String): Double
    fun min(col: String): Double
    fun max(col: String): Double
    fun apply(transform: (DataFrame) -> Row): DataFrame
}

// ── 顶层便利函数 ──────────────────────────────────────────
fun readCsv(path: String): DataFrame
fun readCsv(text: String): DataFrame
fun readJson(path: String): DataFrame      // 行级 JSON（每行一个对象）
fun readJsonl(path: String): DataFrame     // JSON Lines
fun readYaml(path: String): DataFrame
fun writeCsv(path: String, rows: List<Row>): Unit
fun writeJson(path: String, df: DataFrame): Unit
fun writeYaml(path: String, df: DataFrame): Unit

fun join(a: DataFrame, b: DataFrame, on: String, how: String = "inner"): DataFrame
fun merge(a: DataFrame, b: DataFrame, leftOn: String, rightOn: String): DataFrame
```

#### 5.4.4 关键设计：链式调用 + 类型推断

Aura 的链式调用（`df.filter{}.sort{}.take(5)`）语法与 Python 完全一致。
但 Aura 有一个 Python 没有的优势：**类型安全**。
- `filter { it["score"] > 90 }` 中的 `it["score"]` 会被类型检查器推断为 `Any?`，`> 90` 会检查到"比较 Any 和 Int"的潜在问题，提示用户做类型转换。

#### 5.4.5 实现要点

- 底层数据结构：`List<Map<String, Any?>>`（行式存储）
- CSV 解析：用 `aura.std.io` 的 `readText` + 自己写一个 CSV 解析器（~100 行）
- JSON：用 `aura.std.json` 的 `parse`/`stringify`
- YAML：v0.1 暂不支持（v0.2 计划，用 `aura.std.yaml`）
- 聚合：`groupBy` 返回 `Group` 对象，`apply` 允许自定义
- 类型转换：`it["score"] as Int`（用户负责），库提供 `row["score"].toInt()` 便利

#### 5.4.6 已知限制（v0.1）

- ❌ 无类型推断（每列都是 `Any?`，用户手动转）
- ❌ 无窗口函数（v0.2 计划：`over("year").sum("amount")`）
- ❌ 无 SQL 接口（v0.3 计划：`df.query("SELECT * WHERE age > 30")`）
- ❌ 无绘图（v0.3 计划：`df.plot("age", "score", kind = "scatter")`）

---

### 5.5 `aura.kit.file` — 文件操作

#### 目标

对标 Python 的 `pathlib` + `open()`。让新人不写 `File` 对象、不学 `try/finally`、不处理编码。

#### 5.5.1 最小示例（2 行）

```aura
import aura.kit.file

val content = read("config.json")
write("out.txt", content.uppercase())
```

**对比 Python**：

```python
from pathlib import Path
content = Path("config.json").read_text()
Path("out.txt").write_text(content.upper())
```

Aura 版本**少 1 行**（Python 需要 `Path` 包装器）。

#### 5.5.2 核心 API

```aura
package aura.kit.file

// ── 顶层便利函数 ──────────────────────────────────────────
fun read(path: String): String                              // 读文本
fun read(path: String, encoding: String = "utf-8"): String
fun readBytes(path: String): ByteArray                      // 读字节
fun readLines(path: String): List<String>                   // 读多行
fun write(path: String, content: String): Unit              // 写文本
fun write(path: String, content: String, append: Boolean = false): Unit
fun writeBytes(path: String, data: ByteArray): Unit
fun append(path: String, content: String): Unit
fun exists(path: String): Boolean
fun isFile(path: String): Boolean
fun isDir(path: String): Boolean
fun remove(path: String): Unit                              // 删除文件
fun rmRf(path: String): Unit                                // 递归删除
fun copy(src: String, dst: String): Unit
fun move(src: String, dst: String): Unit
fun mkdir(path: String, parents: Boolean = false): Unit
fun list(path: String): List<String>                        // 列目录
fun walk(path: String): Iterable<String>                    // 递归遍历

// ── 路径操作 ──────────────────────────────────────────────
fun join(vararg parts: String): String                       // 路径拼接
fun base(path: String): String                                // 文件名
fun dir(path: String): String                                 // 目录
fun ext(path: String): String                                 // 扩展名
fun normalize(path: String): String                           // 规范化
fun home(): String                                           // 用户目录
fun temp(): String                                           // 临时目录
fun cwd(): String                                            // 当前工作目录

// ── 内容型便利 ──────────────────────────────────────────
fun readJson(path: String): Map<String, Any>
fun writeJson(path: String, obj: Any): Unit
fun readYaml(path: String): Map<String, Any>
fun writeYaml(path: String, obj: Any): Unit
fun readLines(path: String, minLen: Int = 0, maxLen: Int = Int.MAX_VALUE): List<String>
fun filterLines(path: String, pred: (String) -> Boolean): List<String>
```

#### 5.5.3 实现要点

- 全部基于 `aura.std.fs` + `aura.std.io`
- 错误处理：`read` 失败抛 `FileNotFoundException`，不返回 null
- 编码：默认 UTF-8，可显式指定
- 原子写：v0.1 不做（v0.2 计划：`writeAtomic(path, content)`）

---

### 5.6 `aura.kit.text` — 文本处理

#### 目标

对标 Python 的 `re` + `string` + `difflib`。提供正则、字符串操作、差异对比。

#### 5.6.1 最小示例（3 行）

```aura
import aura.kit.text

val email = "alice@example.com"
val local = match("^[^@]+", email)?.group(0) ?? email
println(local)
```

#### 5.6.2 核心 API

```aura
package aura.kit.text

// ── 正则 ─────────────────────────────────────────────────
fun match(pattern: String, text: String): Match?             // 第一个匹配
fun findAll(pattern: String, text: String): List<Match>
fun find(pattern: String, text: String): String?
fun replace(pattern: String, text: String, repl: String): String
fun split(pattern: String, text: String): List<String>
fun escape(pattern: String): String                           // 转义

data struct Match(
    val group0: String,
    val groups: List<String>,
    val start: Int,
    val end: Int
) {
    fun group(n: Int): String?
    fun text(): String        // = group0
    fun span(): Pair<Int, Int>
}

// ── 字符串操作 ──────────────────────────────────────────
fun camel(s: String): String                                  // "hello_world" → "helloWorld"
fun snake(s: String): String                                  // "helloWorld" → "hello_world"
fun kebab(s: String): String                                  // "helloWorld" → "hello-world"
fun truncate(s: String, max: Int, suffix: String = "..."): String
fun wrap(s: String, width: Int = 80): String
fun indent(s: String, spaces: Int = 2): String
fun slug(s: String): String                                   // URL slug
fun slugify(s: String): String                               // 同上，更激进
fun dedent(s: String): String
fun normalize(s: String): String                             // 去多余空白
fun strip(s: String, chars: String = " "): String

// ── 模板 ──────────────────────────────────────────────
fun render(template: String, vars: Map<String, String>): String  // 替换 ${var}

// ── 差异对比 ──────────────────────────────────────────
fun diff(a: String, b: String): List<DiffOp>
fun diffLines(a: String, b: String): List<DiffLine>
data struct DiffOp(val op: String, val text: String)          // op = "equal"|"insert"|"delete"
data struct DiffLine(val kind: String, val text: String)      // kind = "added"|"removed"|"context"

// ── 统计 ──────────────────────────────────────────────
fun wordCount(s: String): Int
fun charCount(s: String): Int
fun lineCount(s: String): Int
fun charFreq(s: String): Map<String, Int>
fun isPalindrome(s: String): Boolean
```

---

### 5.7 `aura.kit.log` — 日志

#### 目标

对标 Python `logging`。让新人不学 Logger、不学 Handler、不学 Formatter，就能打日志。

#### 5.7.1 最小示例（3 行）

```aura
import aura.kit.log

log.basicConfig("server", level = LOG_INFO)
log.info("Server started")
log.warn("Low memory")
```

**对比 Python**：

```python
import logging
logging.basicConfig(level=logging.INFO)
logging.info("Server started")
logging.warning("Low memory")
```

完全一致。

#### 5.7.2 核心 API

```aura
package aura.kit.log

// ── 级别常量 ──────────────────────────────────────────
val LOG_DEBUG: Int = 10
val LOG_INFO: Int = 20
val LOG_WARN: Int = 30
val LOG_ERROR: Int = 40
val LOG_FATAL: Int = 50

// ── 顶层 log 对象 ──────────────────────────────────────
object log {
    fun basicConfig(name: String, level: Int = LOG_INFO, file: String? = null): Unit
    fun debug(msg: String): Unit
    fun info(msg: String): Unit
    fun warn(msg: String): Unit
    fun error(msg: String): Unit
    fun fatal(msg: String): Unit
    fun getLogger(name: String): Logger
}

// ── Logger 类 ────────────────────────────────────────
class Logger(val name: String, val level: Int = LOG_INFO) {
    fun debug(msg: String): Unit
    fun info(msg: String): Unit
    fun warn(msg: String): Unit
    fun error(msg: String): Unit
    fun fatal(msg: String): Unit
    fun setLevel(level: Int): Unit
    fun addHandler(handler: LogHandler): Unit
}

// ── 格式 ──────────────────────────────────────────────
fun format(fmt: String): LogFormatter
fun addConsoleHandler(formatter: LogFormatter): Unit
fun addFileHandler(path: String, formatter: LogFormatter): Unit
fun addJsonHandler(): Unit           // JSON 格式日志（便于收集）

// ── 输出 ──────────────────────────────────────────────
fun log.debug(msg: String): Unit       // 也可作为顶层函数
```

#### 5.7.3 关键设计：`object log` 单例

Python 的 `logging` 模块本身就是"单例式"的（`logging.info(...)` 全局调用）。
Aura 用 `object log` 表达同样的语义，同时保留 `Logger` 类做实例化场景。

---

### 5.8 `aura.kit.task` — 任务调度

#### 目标

对标 `schedule` / `APScheduler`。让新人能写"每 5 分钟做 X"、"每天 9 点做 Y"。

#### 5.8.1 最小示例（5 行）

```aura
import aura.kit.task

every(5, MINUTES) { backup() }
everyDay("09:00") { sendReport() }
everyHour() { checkHealth() }
run()
```

**对比 Python**：

```python
import schedule, time
schedule.every(5).minutes.do(backup)
schedule.every_day().at("09:00").do(send_report)
schedule.every_hour().do(check_health)
while True:
    schedule.run_pending()
    time.sleep(1)
```

Aura 版本**少 4 行**（不需要 `while True` + `time.sleep`）。

#### 5.8.2 核心 API

```aura
package aura.kit.task

// ── 时间单位 ────────────────────────────────────────
val SECONDS: TimeUnit = TimeUnit.SECONDS
val MINUTES: TimeUnit = TimeUnit.MINUTES
val HOURS: TimeUnit = TimeUnit.HOURS
val DAYS: TimeUnit = TimeUnit.DAYS

// ── 顶层便利函数 ──────────────────────────────────────────
fun every(n: Int, unit: TimeUnit, task: () -> Unit): Task
fun everyHour(task: () -> Unit): Task
fun everyDay(hour: String, task: () -> Unit): Task
fun everyWeek(day: Int, hour: String, task: () -> Unit): Task
fun cron(expr: String, task: () -> Unit): Task

// ── 阻塞运行 ──────────────────────────────────────────
fun run(): Unit       // 阻塞直到中断（Ctrl+C）

// ── Task 对象 ──────────────────────────────────────
class Task(
    val schedule: Schedule,
    val task: () -> Unit
) {
    fun cancel(): Unit
    fun runOnce(): Unit          // 立即执行一次
    fun nextRun(): Long          // 下次执行时间戳
}

// ── 底层：调度循环（用 actor 实现） ──────────────────────
actor Scheduler {
    var tasks: List<Task> = listOf()
    fun add(task: Task): Unit
    fun remove(task: Task): Unit
    fun tick(): Unit             // 检查到期任务
}
```

#### 5.8.3 实现要点

- 调度循环用 `actor` 实现，避免多线程竞争
- 时间计算：用 `System.currentTimeMillis()`
- 中断：监听 Ctrl+C（用 `aura.std.signal` 或 `signal(SIGINT)` 原生调用）
- 持久化：v0.1 不支持（v0.2 计划：`schedule.save("tasks.json")`）

---

### 5.9 `aura.kit.test` — 测试框架

#### 目标

对标 `pytest`。让新人不学 `@Test`、不学 `assert` 类、不写 setUp/tearDown，就能写测试。

#### 5.9.1 最小示例（3 行）

```aura
import aura.kit.test

test("add") { assertEq(1 + 1, 2) }
test("fib") { assertEquals(fib(10), 55) }
```

**对比 Python pytest**：

```python
def test_add(): assert 1 + 1 == 2
def test_fib(): assert fib(10) == 55
```

**完全一致**。

#### 5.9.2 进阶示例（10 行：参数化 + fixture + 错误断言）

```aura
import aura.kit.test

fun fib(n: Int): Int = if (n < 2) n else fib(n-1) + fib(n-2)

test("fib") {
    for (n in 0..10) {
        assertEq(fib(n), fibRef(n), "fib($n)")
    }
}

parametrized("mul", listOf(1 to 2, 3 to 4, 0 to 100)) { (a, b) ->
    assertEq(a * b, 6)
}

withFixture("db") { db ->
    test("user exists") {
        assertEq(db.query("SELECT 1"), 1)
    }
}

testThrows("divide by zero") {
    divide(1, 0)
}
```

#### 5.9.3 核心 API

```aura
package aura.kit.test

// ── 顶层便利函数 ──────────────────────────────────────────
fun test(name: String, body: () -> Unit): TestCase
fun testOnly(name: String, body: () -> Unit): TestCase   // 只跑这个
fun testSkip(name: String, reason: String = "", body: () -> Unit): TestCase
fun testSlow(name: String, body: () -> Unit): TestCase

// ── 参数化 ──────────────────────────────────────────────
fun parametrized(name: String, cases: List<Any>, body: (Any) -> Unit): TestCase
fun parametrized(name: String, cases: Iterable<*> , body: (Any) -> Unit): TestCase

// ── Fixture ─────────────────────────────────────────────
fun withFixture(name: String, setup: () -> Any, teardown: (Any) -> Unit = {}, body: (Any) -> Unit): Unit
fun before(body: () -> Unit): Unit
fun after(body: () -> Unit): Unit

// ── 断言（全部抛 TestFailure） ──────────────────────────
fun assertTrue(cond: Boolean, msg: String = ""): Unit
fun assertFalse(cond: Boolean, msg: String = ""): Unit
fun assertEquals(actual: Any, expected: Any, msg: String = ""): Unit
fun assertNotEquals(actual: Any, expected: Any, msg: String = ""): Unit
fun assertNull(v: Any?, msg: String = ""): Unit
fun assertNotNull(v: Any?, msg: String = ""): Unit
fun assertThrows(type: Class<out Exception>, body: () -> Any, msg: String = ""): Exception
fun assertContains(container: Any, item: Any, msg: String = ""): Unit
fun assertNotContains(container: Any, item: Any, msg: String = ""): Unit
fun assertGt(a: Any, b: Any, msg: String = ""): Unit
fun assertLt(a: Any, b: Any, msg: String = ""): Unit
fun assertApprox(actual: Double, expected: Double, delta: Double = 0.0001, msg: String = ""): Unit

// ── 运行器 ──────────────────────────────────────────────
fun runAll(): TestResult      // 运行所有测试，返回结果
data struct TestResult(
    val total: Int,
    val passed: Int,
    val failed: Int,
    val skipped: Int,
    val duration: Long
) {
    fun report(): String       // 终端报告
}
```

#### 5.9.4 关键设计：自动发现 + 自动运行

- `test(...)` 注册的用例会进入全局注册表
- 运行器（`runAll()`）遍历注册表，逐个执行
- 报告格式：模仿 pytest 的"通过/失败/跳过"
- 颜色输出：用 `aura.kit.ui` 的颜色函数

---

### 5.10 `aura.kit.ui` — 终端 UI

#### 目标

对标 `rich`。让新人能打印彩色文字、表格、进度条、进度条、分隔线。

#### 5.10.1 最小示例（4 行）

```aura
import aura.kit.ui

println(green("OK") + ", " + red("FAIL"))
println(ProgressBar("Loading", 100).tick(50).render())
println(Table("A", "B").row("1", "2").row("3", "4").render())
```

#### 5.10.2 核心 API

```aura
package aura.kit.ui

// ── 颜色 ──────────────────────────────────────────────
fun red(s: String): String
fun green(s: String): String
fun yellow(s: String): String
fun blue(s: String): String
fun magenta(s: String): String
fun cyan(s: String): String
fun white(s: String): String
fun gray(s: String): String
fun bold(s: String): String
fun dim(s: String): String
fun italic(s: String): String
fun underline(s: String): String

// ── 表格 ──────────────────────────────────────────────
class Table(vararg columns: String) {
    fun row(vararg cells: String): Table
    fun row(cells: List<String>): Table
    fun render(): String
    fun align(center: Boolean = true): Table
}

// ── 进度条 ──────────────────────────────────────────
class ProgressBar(val label: String, val total: Int) {
    fun tick(delta: Int): ProgressBar
    fun tick(value: Int): ProgressBar     // 绝对值
    fun render(): String
    fun spin(): Unit                       // 自旋动画
}

// ── 分隔线 ──────────────────────────────────────────────
fun hr(char: String = "─", width: Int = 60): String
fun divider(char: String = "─"): String

// ── 面板 ──────────────────────────────────────────────
fun panel(title: String, body: String, width: Int = 60): String

// ── 加载动画 ──────────────────────────────────────────
fun spinner(label: String): Spinner
class Spinner(val label: String) {
    fun start(): Unit      // 开始自旋
    fun stop(): Unit       // 停止
}

// ── 终端信息 ──────────────────────────────────────────
fun width(): Int           // 终端宽度
fun height(): Int          // 终端高度
fun clear(): Unit          // 清屏
fun beep(): Unit           // 响铃
```

#### 5.10.3 关键设计：ANSI 转义序列

所有颜色/样式都基于 ANSI 转义序列（`\x1b[31m` 红色、`\x1b[0m` 重置）。
表格用等宽字符渲染（每列宽度自动对齐）。
进度条用 `█` 和 `░` 字符（Windows 10+ 支持）。

---

### 5.11 `aura.kit.config` — 配置加载

#### 目标

对标 Python 的 `yaml` + `dotenv`。让新人能加载 YAML/TOML/JSON/.env 配置。

#### 5.11.1 最小示例（3 行）

```aura
import aura.kit.config

val cfg = Config.load("config.yml")
val dbUrl = cfg.str("database.url", "sqlite:///default")
```

**对比 Python**：

```python
import yaml
cfg = yaml.safe_load(open("config.yml"))
db_url = cfg.get("database", {}).get("url", "sqlite:///default")
```

Aura 版本**少 1 行**（Python 需要嵌套 `.get()`）。

#### 5.11.2 核心 API

```aura
package aura.kit.config

// ── Config 类 ────────────────────────────────────────
class Config(val data: Map<String, Any>) {
    fun get(key: String): Any?
    fun str(key: String, default: String = ""): String
    fun int(key: String, default: Int = 0): Int
    fun double(key: String, default: Double = 0.0): Double
    fun bool(key: String, default: Boolean = false): Boolean
    fun list(key: String): List<Any>
    fun map(key: String): Map<String, Any>
    fun get(path: String): Config          // 点路径："database.url"
    fun has(key: String): Boolean
    fun keys(): List<String>
    fun mergedWith(other: Config): Config   // 深合并
}

// ── 顶层便利函数 ──────────────────────────────────────────
fun load(path: String): Config                    // 自动识别格式
fun loadYaml(path: String): Config
fun loadToml(path: String): Config
fun loadJson(path: String): Config
fun loadDotEnv(path: String): Map<String, String>
fun save(path: String, cfg: Config): Unit         // 保存为 YAML

// ── 环境变量 ──────────────────────────────────────────
fun env(name: String, default: String = ""): String
fun envInt(name: String, default: Int = 0): Int
fun envBool(name: String, default: Boolean = false): Boolean
fun setEnv(name: String, value: String): Unit

// ── 配置 schema 验证 ──────────────────────────────────
fun validate(cfg: Config, schema: Map<String, String>): Boolean  // schema = {"database.url": "string"}
```

#### 5.11.3 关键设计：点路径访问

`cfg.str("database.url", "...")` 自动解析为嵌套：
```
database:
  url: "..."
```

这是 Python `configparser` / `ruamel.yaml` 都缺的便利，Aura 通过 API 命名补足。

---

## 六、语言层面配套

`aura.kit` 是库层面的设计，但**要达到 Python 级别的"5 行"，需要语言层面同步支持**。以下是建议的语言级改进（已在路线图上的优先级）。

### 6.1 脚本模式（已在做）

参见 [语言-script-mode-技术方案.md](./语言-script-mode-技术方案.md)。
- **效果**：`aura run foo.aura` 直接执行，无需 `fun main()`
- **状态**：设计中

### 6.2 顶层 `suspend` 调用

`aura.kit.net` 的 API 全部 `suspend`，但顶层调用需要"自动 coroutine runner"。

**建议方案**：脚本模式检测到顶层语句调用 `suspend` 函数时，自动包装为：

```aura
// 用户写：
import aura.kit.net
val r = get("https://api.github.com")

// 编译器自动包：
fun main() {
    runBlocking {
        val r = get("https://api.github.com")
    }
}
```

**状态**：待设计（建议作为脚本模式的一部分）

### 6.3 简化 import

现状：`import aura.std.fs as fs` 仍然冗长。
建议：
- **短别名**：`import aura.std.*` 时自动短名（已有）
- **顶层 import**：`import aura.kit.web` 后，`Web`、`get`、`post` 等顶层符号直接可用
- **可选**：`import aura.kit.*` 时自动展开所有子模块（需要设计名字冲突解决策略）

### 6.4 命名参数 + 默认参数（已有）

Aura 已有 `fun f(x: Int = 1, y: String = "a")`，足够支撑 `aura.kit` 的"少参数"设计。

### 6.5 字符串插值（已有）

Aura 已有 `${expr}` 和 `$var`，等价于 Python f-string，足够用。

### 6.6 自动类型推断（部分有）

Aura 已有 `val x = 1` 推断为 Int。但 `val x = foo()` 需要显式返回类型。
建议：在 `aura.kit` 的 API 中，让返回类型尽量明确（`String`、`Int`、`DataFrame`），让推断能落地。

### 6.7 顶层常量（建议加）

`aura.kit.task` 用了 `SECONDS`/`MINUTES` 等常量。Aura 已有 `val` 顶层常量，足够。

---

## 七、Python vs Aura kit 代码量对比

下面是"5 行版本"的完整对照，所有示例都从 `import` 到 `run` 算起。

### 7.1 写一个 Hello World 网页

| 语言 | 代码 | 行数 |
|------|------|------|
| Python Flask | `from flask import Flask; app = Flask(__name__); @app.route("/"); def home(): return "Hello"; app.run(port=8000)` | 5 |
| **Aura kit** | `import aura.kit.web; val app = Web(); app.get("/") { "Hello" }; app.run(port = 8000)` | 4 |
| Aura（现状） | 需要写 `fun main()` + `import` + `HttpServer` + 路由注册 + `run()` | 8-15 |

### 7.2 写一个 CLI 工具

| 语言 | 代码 | 行数 |
|------|------|------|
| Python Click | `import click; @click.command(); @click.option("-n", default=3); @click.argument("name"); def cli(n, name): click.echo(f"Hello {name}! x{n}"); cli()` | 7 |
| **Aura kit** | `import aura.kit.cli; fun main(name: String = "World", count: Int = 3) { for (i in 0 until count) println("Hello $name!") }` | 3 |
| Aura（现状） | `fun main() { val args = System.argv; val name = args[0]; ... }` | 6-10 |

### 7.3 写一个 HTTP 客户端

| 语言 | 代码 | 行数 |
|------|------|------|
| Python | `import requests; r = requests.get("https://api.github.com"); print(r.json()["name"])` | 3 |
| **Aura kit** | `import aura.kit.net; val r = get("https://api.github.com"); println(r.json()["name"])` | 3 |
| Aura（现状） | 需要 socket + HTTP 解析 + JSON | 20+ |

### 7.4 写一个数据处理脚本

| 语言 | 代码 | 行数 |
|------|------|------|
| Python pandas | `import pandas as pd; df = pd.read_csv("data.csv"); top = df[df["score"] > 90].sort_values("age").head(5); print(top.to_string())` | 4 |
| **Aura kit** | `import aura.kit.data; val df = readCsv("data.csv"); val top = df.filter { it["score"] > 90 }.sort { it["age"] }.take(5); println(top.toTable())` | 4 |
| Aura（现状） | 需要 for 循环 + 手写排序 + 手写格式化 | 15+ |

### 7.5 写一个测试

| 语言 | 代码 | 行数 |
|------|------|------|
| Python pytest | `def test_add(): assert 1 + 1 == 2` | 1 |
| **Aura kit** | `import aura.kit.test; test("add") { assertEq(1 + 1, 2) }` | 2 |
| Aura（现状） | `fun main() { if (1+1 != 2) throw Exception("fail"); println("pass") }` | 3 |

### 7.6 写一个任务调度

| 语言 | 代码 | 行数 |
|------|------|------|
| Python schedule | `import schedule, time; schedule.every(5).minutes.do(backup); while True: schedule.run_pending(); time.sleep(1)` | 5 |
| **Aura kit** | `import aura.kit.task; every(5, MINUTES) { backup() }; run()` | 3 |
| Aura（现状） | 需要手写 `Thread` + `Timer` + 循环 | 15+ |

**结论**：`aura.kit` 在所有常见场景都能达到 Python 级别，且在 CLI、任务调度、网页上**比 Python 还少**。

---

## 八、实现路线图

### 8.1 Phase 1：基础工具库（v0.1，2-3 周）

优先级最高，覆盖"写脚本"的所有基础场景。

| 模块 | 工作量 | 依赖 | 验收 |
|------|--------|------|------|
| `aura.kit.file` | 1 天 | `aura.std.fs/io` | 读写文件 + 路径操作 + 目录遍历 |
| `aura.kit.text` | 1 天 | 标准库正则 | 正则 + 字符串操作 + diff |
| `aura.kit.log` | 0.5 天 | `aura.std.io` | 5 种级别 + 文件输出 + JSON 格式 |
| `aura.kit.net` | 3 天 | `aura.std.net` | GET/POST/PUT/DELETE + JSON + 超时 + 错误 |
| `aura.kit.test` | 1 天 | `aura.kit.ui` | test/parametrized/assert/fixture + 报告 |

**里程碑**：新人能用 `aura.kit.file` + `aura.kit.net` + `aura.kit.test` 写一个"下载网页内容并保存"的脚本。

### 8.2 Phase 2：高层框架（v0.2，2-3 周）

让"5 行写一个 X"真正落地。

| 模块 | 工作量 | 依赖 | 验收 |
|------|--------|------|------|
| `aura.kit.web` | 5 天 | `aura.kit.net` + HTTP 解析器 | 路由 + 参数 + JSON + 错误处理 + 静态文件 |
| `aura.kit.cli` | 3 天 | Aura 反射 API | 参数/选项/帮助/子命令（v0.2 加） |
| `aura.kit.data` | 4 天 | `aura.kit.file` + `aura.std.json` | CSV + JSON + filter/sort/take + groupBy |

**里程碑**：新人能用 `aura.kit.web` 写一个 5 行服务器；用 `aura.kit.cli` 写一个带参数的工具；用 `aura.kit.data` 处理 CSV。

### 8.3 Phase 3：完善体验（v0.3，2 周）

| 模块 | 工作量 | 依赖 | 验收 |
|------|--------|------|------|
| `aura.kit.ui` | 2 天 | ANSI 转义 | 颜色 + 表格 + 进度条 + spinner |
| `aura.kit.config` | 1 天 | `aura.std.fs` + YAML 解析器 | YAML/TOML/JSON/.env + 点路径 + schema 验证 |
| `aura.kit.task` | 2 天 | `actor` + `System.currentTimeMillis` | every/everyDay/cron + 中断 |

**里程碑**：一个"完整的小工具"（CLI + 配置文件 + 日志 + 数据 + UI + 调度）能完整跑通。

### 8.4 Phase 4：生态（v0.4，1-2 月）

- `loom install aura-kit` 支持
- 示例合集：`aura-kit/examples/` 每个模块一个可运行的 5 行示例
- 文档：`book/aura-kit/` 章节，每模块一节
- 测试套件：`tests/aura-kit/` 每个模块的 5+ 测试
- CI：`scripts/aura-kit-test.ps1` 一键跑所有示例

### 8.5 依赖关系图

```
aura.std.*  (标准库，底层)
    │
    ├── aura.kit.file       (Phase 1)
    ├── aura.kit.text       (Phase 1)
    ├── aura.kit.log        (Phase 1)
    ├── aura.kit.net        (Phase 1)
    ├── aura.kit.test       (Phase 1)
    │
    ├── aura.kit.web        (Phase 2，依赖 net)
    ├── aura.kit.cli        (Phase 2，依赖反射)
    └── aura.kit.data       (Phase 2，依赖 file + json)
        │
        ├── aura.kit.ui     (Phase 3)
        ├── aura.kit.config (Phase 3，依赖 file)
        └── aura.kit.task   (Phase 3，依赖 actor)
```

---

## 九、风险与取舍

### 9.1 风险

| 风险 | 等级 | 应对 |
|------|------|------|
| **`aura.kit.net` 的 TLS 实现复杂** | 🔴 高 | v0.1 暂只支持 HTTP；HTTPS 用"系统调用"（curl）或推迟到 v0.2 |
| **`aura.kit.cli` 的反射 API 不完善** | 🟡 中 | 先用"约定 + 手写解析器"，反射 API 成熟后再切 |
| **`aura.kit.web` 的 HTTP 解析器 bug** | 🟡 中 | 充分单元测试；用现成的 HTTP 测试用例（httpbin.org） |
| **`aura.kit.data` 类型不安全** | 🟡 中 | v0.1 用 `Any?`，v0.2 引入 `TypedRow` |
| **与标准库功能重叠** | 🟡 中 | 见 9.2 "边界" |
| **新手学习曲线** | 🟢 低 | API 命名贴近 Python 习惯 |
| **API 设计不合理** | 🟡 中 | 每个模块先发 v0.1 试用，收集反馈再迭代 |

### 9.2 与标准库的边界

**明确分工**：
- `aura.std.*` = 底层原子能力（socket、file、json、regex）
- `aura.kit.*` = 高层便利层（HTTP 客户端、Web 框架、CLI 解析器）

**不重叠原则**：
- `aura.kit.file` 不实现 `fs.exists`（用 `aura.std.fs.exists`）
- `aura.kit.net` 不实现 socket（用 `aura.std.net`）
- `aura.kit.data` 不实现 JSON 解析（用 `aura.std.json`）

**唯一例外**：`aura.kit.log` 与 `aura.std.io` 的 `println` 有重叠 —— `aura.kit.log` 是"结构化日志"，`println` 是"临时调试"，两者并存。

### 9.3 取舍

| 取舍 | 选择 | 理由 |
|------|------|------|
| **类型安全 vs 新手友好** | 类型安全优先 | Aura 的卖点；`aura.kit` 尽量让类型从默认值推断 |
| **零依赖 vs 功能完整** | 零依赖优先 | 不引入第三方；YAML 解析器可能自写（~200 行） |
| **单例 vs 实例化** | 单例 + 实例化都支持 | `object log` 单例 + `Logger` 实例类 |
| **同步 vs 异步** | 异步优先 | 用 `suspend`；同步版本通过 `runBlocking { ... }` 提供 |
| **链式调用 vs 函数式** | 都支持 | DataFrame 链式 + `data` 模块也有函数式 |
| **简洁 vs 完整** | 简洁优先 | "5 行版本"是 v0.1 目标，完整功能放后续 |

### 9.4 与 Kotlin / Java 的对比

Aura 的"5 行版本"在功能上**接近 Kotlin + Ktor**，但：
- Kotlin Ktor 需要 `JVM` + `Gradle`，部署重
- Aura kit 是"单文件脚本"，零配置
- Kotlin 的类型系统更复杂（`sealed interface`、`data class`），学习曲线更陡

**定位**：Aura kit = "Python 级别的简洁 + Kotlin 级别的类型安全"。

---

## 十、附录：5 行代码速查表

下面是 `aura.kit` 所有模块的"5 行版本"汇总，可直接复制到 Aura REPL 或脚本模式测试。

### A.1 Web 服务器

```aura
import aura.kit.web

val app = Web()
app.get("/") { "Hello!" }
app.get("/api/user/:id") { req -> json(userId = req.param<Int>("id")!!) }
app.run(port = 8000)
```

### A.2 CLI 工具

```aura
import aura.kit.cli

fun main(input: String = "input.txt", output: String = "out.txt", verbose: Boolean = false) {
    val content = read(input)
    if (verbose) println("Read ${content.length} bytes")
    write(output, content.uppercase())
}
```

### A.3 HTTP 客户端

```aura
import aura.kit.net

val r = get("https://api.github.com")
val data = r.json()
println(data["name"])
```

### A.4 数据处理

```aura
import aura.kit.data

val df = readCsv("data.csv")
val top = df.filter { it["score"] > 90 }.sort { it["age"] }.take(5)
println(top.toTable())
```

### A.5 文件操作

```aura
import aura.kit.file

val content = read("config.json")
write("out.txt", content.uppercase())
append("out.txt", "\nDone")
```

### A.6 测试

```aura
import aura.kit.test

test("add") { assertEq(1 + 1, 2) }
test("fib") { assertEq(fib(10), 55) }
runAll()
```

### A.7 任务调度

```aura
import aura.kit.task

every(5, MINUTES) { println("backup") }
everyDay("09:00") { println("report") }
run()
```

### A.8 日志

```aura
import aura.kit.log

log.basicConfig("server", level = LOG_INFO)
log.info("Server started")
log.warn("Low memory")
log.error("Failed")
```

### A.9 终端 UI

```aura
import aura.kit.ui

println(green("OK") + ", " + red("FAIL"))
println(ProgressBar("Loading", 100).tick(50).render())
println(Table("A", "B").row("1", "2").row("3", "4").render())
```

### A.10 配置

```aura
import aura.kit.config

val cfg = Config.load("config.yml")
val dbUrl = cfg.str("database.url", "sqlite:///default")
println("DB: $dbUrl")
```

### A.11 文本处理

```aura
import aura.kit.text

val email = "alice@example.com"
val local = match("^[^@]+", email)?.group(0) ?? email
println("$local (snake: ${snake(local)})")
```

---

## 总结

| 维度 | 现状 | aura.kit 目标 |
|------|------|--------------|
| 写网页 | 8-15 行 | 4-5 行 |
| 写 CLI | 6-10 行 | 3-5 行 |
| 写 HTTP 客户端 | 20+ 行 | 3 行 |
| 写数据处理 | 15+ 行 | 4 行 |
| 写测试 | 3 行 | 2 行 |
| 写调度 | 15+ 行 | 3 行 |

`aura.kit` 是"在 Aura 现有基础上，补一层 Python 级别的便利层"。
不重写语言、不引入全局可变、不破坏类型安全。
让新人"装完就能干活"，让老手"装完少写样板"。

---

**下一步**：
1. 在 `aura/core/` 下创建 `aura-kit/` 子目录，实现 Phase 1 模块
2. 补全脚本模式 + 顶层 `suspend` 支持（语言层面）
3. 写测试套件，用 CI 跑通所有 5 行示例
4. 收集社区反馈，迭代 API

---
