# Aura Language — VS Code 扩展

> Aura 语言 — NovaOS 的下一代系统级脚本语言

[![VS Marketplace](https://img.shields.io/badge/VS%20Marketplace-Aura-blue)](https://marketplace.visualstudio.com/items?itemName=aura-lang.aura-language)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

## 功能特性

| 功能 | 状态 | 说明 |
|------|------|------|
| 语法高亮 | ✅ | Kotlin 风格 TextMate 语法 — 完整声明高亮（struct/class/enum/interface/actor/object）、修饰符、泛型参数、函数参数、字符串插值、数字字面量、运算符、自引用、标签 |
| 代码补全 | ✅ | 基于 AST 符号表的智能补全 |
| 跳转定义 | ✅ | 点击跳转到函数/结构体/枚举定义 |
| 悬停提示 | ✅ | 显示类型信息、可见性、文档 |
| 诊断推送 | ✅ | 实时错误/警告/信息推送 |
| 代码格式化 | ✅ | `aura fmt` 格式化 |
| 保存检查 | ✅ | 保存时自动检查 |
| 代码折叠 | ✅ | 按函数体、块折叠 |
| 代码片段 | ✅ | 20+ 常用模板 |
| 参数提示 | ✅ | 函数调用参数提示 |
| 重构 | 🔮 | 重命名、提取方法（待实现） |
| AI 助手 | 🔮 | Copilot 风格补全（待实现） |

## 安装

### 从源码构建

```bash
cd vscode-extension
npm install
npm run compile
code --install-extension aura-language-0.1.9.vsix
```

> **LSP 功能开箱即用**：扩展将 `aura-lsp` 二进制**直接打包**在 `bin/` 下（`bin/aura-lsp.exe` 用于 Windows），无需系统 PATH、无需手动编译 Aura。安装后打开 `.aura` 文件即自动启动 LSP。
>
> 解析顺序（高优先级在前）：
> 1. `aura.serverPath` 显式配置的绝对路径（本地开发自编译版本用）
> 2. **扩展内置的 `bin/aura-lsp[.exe]`（默认）**
> 3. 当前工作区内的构建产物（`target/debug`、`target/release`、`bin`）
> 4. 系统 PATH 中的 `aura-lsp` 命令
>
> 扩展直接启动独立的 `aura-lsp` 二进制，不占用 `aura.exe`，`cargo build` debug profile 可正常覆盖它。

### 从 VS Code 市场安装

> 搜索 "Aura Language" 并点击安装（待发布）

## 配置

### 基本配置

```jsonc
{
    // Aura LSP 服务器路径。默认使用扩展内置的 bin/aura-lsp（开箱即用），
    // 仅在本地开发 Aura 本身时才需要覆盖为绝对路径。
    "aura.serverPath": "aura-lsp",

    // 传递给服务器的额外参数
    "aura.serverArgs": [],

    // 保存时自动检查
    "aura.checkOnSave": true,

    // 保存时自动格式化
    "aura.formatOnSave": false,

    // 启用诊断推送
    "aura.diagnosticsEnabled": true
}
```

### 自定义服务器路径

默认无需配置——扩展使用内置的 `bin/aura-lsp`。如需覆盖（例如本地开发 Aura 时使用自编译的 debug 版本），可设置为绝对路径：

```jsonc
{
    "aura.serverPath": "C:/Aura/AuraLang/target/debug/aura-lsp.exe"
}
```

或 Linux/macOS:

```jsonc
{
    "aura.serverPath": "/home/user/aura/target/release/aura-lsp"
}
```

> **注意**：
> - `aura-lsp` 是独立二进制，**不需要** `lsp` 子命令。
> - 如果你从旧版本升级过来，请把 `aura.serverPath` 从 `"aura"` 改为 `"aura-lsp"`（或直接删除该项），并删除 `serverArgs: ["lsp"]`。否则扩展会尝试启动 `aura-lsp.exe lsp`（失败）。
> - 保留旧配置 `"aura.serverPath": "aura"` 会占用 `aura.exe`，导致 `cargo build` debug profile 失败。
> - 如果 `aura.serverPath` 未显式设置或仍为默认值 `"aura-lsp"`，扩展会**优先使用内置二进制**，避免误启动 PATH 中的同名程序。

## 快捷键

| 命令 | 默认快捷键 | 说明 |
|------|-----------|------|
| 格式化文档 | Shift+Alt+F | 格式化当前文件 |
| 显示诊断 | Ctrl+Shift+D | 显示所有诊断信息 |
| 重启 LSP | Ctrl+Shift+R | 重启 LSP 服务器 |
| 显示版本 | Ctrl+Alt+V | 显示服务器版本 |

## 语法高亮配色（内置默认色）

扩展为 `aura` 文件内置了一组默认 token 颜色（仅作用于 Aura 语法，不干扰其它语言与主题）。Aura 语法使用常见的作用域命名（`variable.other.constant`、`variable.other.readwrite`、`support.function` 等），因此开箱即用时，大多数主题（含 VS Code 默认深色主题）都会给出合适的颜色：

| 元素 | 作用域 | 默认观感（VS Code 深色主题） |
|------|--------|---------|
| 常量（`val GAME_WIDTH: Int` 等全大写） | `variable.other.constant.aura` | 紫色/亮蓝（主题相关） |
| 变量 / 属性（`var x`、赋值） | `variable.other.readwrite.aura`、`variable.field.aura` 等 | 亮蓝 |
| 参数 / 具名参数 / 结构体构造属性 | `variable.parameter.aura` | 橙 |
| std 库函数（`aura.lang.std.Math.abs(...)` 等） | `support.function.std.aura` | 青绿（与普通函数金黄区分） |
| 普通函数调用 | `entity.name.function.call.aura` | 跟随主题（默认金黄） |

如需自定义配色，可在 `settings.json` 中覆盖：

```jsonc
"[aura]": {
    "editor.tokenColorCustomizations": {
        "textMateRules": [
            { "scope": "support.function.std.aura", "settings": { "foreground": "#FF8800" } }
        ]
    }
}
```

## 语法高亮示例

```aura
// 导入
import aura.concurrent.*
import aura.std.fs as fs

// 文档注释
/**
 * 玩家结构体（data struct + 字段默认值 + 可空字段）
 * @param id 玩家 ID
 * @return 无
 */
data struct Player(
    val id: Int,
    var name: String = "unknown",
    var health: Int = 100,
    var tag: String? = null
)

// 类 + 泛型 + 继承 + 接口实现
sealed class Shape<T : Number> {
    fun area(): Float = 0.0f
}
class Circle : Shape {
    override fun area(): Float = 0.0f
}
class Dog : Animal(), Pet {
    override fun name(): String = "dog"
}

// 接口
interface Drawable {
    fun draw(): Unit
}

// 枚举（单元变体 + 带数据变体）
enum Color {
    RED,
    GREEN,
    CUSTOM(val r: Int, val g: Int, val b: Int)
}

// Actor
actor Scheduler {
    private var tick: Int = 0
    fun step() { tick += 1 }
}

// 类型别名
typealias Vec2 = Point

// 函数声明（泛型 + 修饰符）
suspend fun fetch(): Int = 0
inline fun max(a: Int, b: Int): Int = a
comptime fun constValue(): Int = 1

// 外部函数声明（FFI）
extern "c" fun puts(msg: String): Int

// 函数调用（泛型实参）
val nums: List<Int> = listOf(10, 20, 30)
val result = when (score) {
    0 -> "zero"
    in 1..50 -> "low"
    in 51..100 -> "high"
    else -> "extreme"
}

// 字符串插值
val message = "Score: ${score * 2}"
val path = "C:\\tmp\\file.aura"

// 多行原始字符串（无插值）
val raw = """
    No interpolation: $var
    Backslash: \n
"""

// 空安全操作符
val safe: Int = n ?: 0
val len: Int? = p.tag?.length
val forced: Int = n!!

// 方法引用
val fn = obj::method

// Lambda
val transform = (x) -> x + 1

// 标签循环
outer@ for (a in 0..3) {
    for (b in 0..3) {
        if (a == b) break@outer
    }
}

// 数字字面量
val hex = 0xFF
val binary = 0b1100
val float = 3.14f
val long = 1_000_000L
```

## 代码片段

| 前缀 | 描述 |
|------|------|
| `fn` | 函数 |
| `fn=` | 单表达式函数 |
| `struct` | 结构体 |
| `enum` | 枚举 |
| `interface` | 接口 |
| `class` | 类 |
| `suspend` | 协程函数 |
| `actor` | Actor |
| `if` | if-else |
| `when` | when 表达式 |
| `for` | for 循环 |
| `while` | while 循环 |
| `try` | try-catch |
| `gen` | 泛型函数 |
| `result` | Result 错误处理 |
| `ffi` | FFI 声明 |
| `dep` | 依赖声明 |
| `main` | main 函数 |
| `data` | 数据类 |
| `import` | 导入模块 |

## LSP 协议

Aura LSP 服务器实现以下 LSP 方法：

| 方法 | 说明 |
|------|------|
| `initialize` | 初始化握手 |
| `textDocument/didOpen` | 文档打开 |
| `textDocument/didChange` | 文档变更（增量） |
| `textDocument/didClose` | 文档关闭 |
| `textDocument/completion` | 代码补全 |
| `textDocument/definition` | 跳转定义 |
| `textDocument/hover` | 悬停提示 |
| `textDocument/diagnostic` | 诊断推送 |
| `textDocument/formatting` | 文档格式化 |

## 开发

```bash
# 安装依赖
npm install

# 编译 TypeScript
npm run compile

# 开发模式
npm run watch

# 打包 VSIX
vsce package
```

## 架构

```
vscode-extension/
├── package.json              # 扩展清单
├── tsconfig.json             # TypeScript 配置
├── language-configuration.json  # 语言配置（括号、注释等）
├── src/
│   ├── extension.ts          # 扩展入口
│   ├── client.ts             # LSP 客户端（解析内置 bin/aura-lsp）
│   └── diagnostics.ts        # 诊断管理
├── bin/
│   └── aura-lsp.exe          # 内置 LSP 二进制（Windows；随 VSIX 打包，开箱即用）
├── syntaxes/
│   └── aura.tmLanguage.json  # TextMate 语法（Kotlin VSCode 插件风格）
├── snippets/
│   └── aura.lang.std.Json             # 代码片段
└── resources/
    ├── aura-icon-dark.png    # 深色主题文件图标 (256×256)
    ├── aura-icon-light.png   # 浅色主题文件图标 (256×256)
    ├── aura-icon.png         # 默认图标 (48×48, 深色变体)
    └── aura-icon.svg         # 源矢量图标 (光环渐变设计)
```

> **打包说明**：`bin/aura-lsp.exe` 通过 `.vscodeignore` 的 `!bin/**` 规则被打入 VSIX。
> 如需在其它平台使用，请从 Aura 仓库交叉编译出对应平台的 `aura-lsp`，放入 `bin/` 后重打包。

## 许可证

MIT License