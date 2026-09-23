# Photon IR Language (VSCode Extension)

Photon IR (.phir) 语言支持 — 语法高亮 + 代码片段。适用于 NovaOS Photon 编译器的中间表示文件。

## 特性

- **语法高亮**：完整的 TextMate 语法高亮，覆盖所有 PHIR 语法元素
- **代码片段**：30+ 常用 PHIR 代码模板（函数、基本块、栈槽、调用等）
- **智能编辑**：注释自动续行、括号匹配、代码折叠
- **颜色定制**：针对 PHIR 语义的专用配色方案

## 高亮覆盖

| 语法元素 | 示例 |
|---------|------|
| 模块头 | `# module hello target x86_64-pc-windows-msvc flags 0x01` |
| 全局声明 | `@str.hello = "hello world"` |
| 原生函数 | `native fun write(fd: i32, buf: ptr) -> i32` |
| 函数定义 | `fun strlen(s: ptr) -> i64 { ... }` |
| 栈槽 | `ss0 = f64 8` |
| 基本块 | `bb entry(v0: ptr):` |
| 赋值语句 | `v2 = const i32 42` |
| 栈操作 | `store i32 v3, ss0` / `v4 = load.i32 ss1` |
| 终止符 | `return v1` / `br if v2, b1, b2` |
| 函数调用 | `v3 = call @fn(ptr v0) -> i32` |
| 异常处理 | `[return: ok, unwind: catch]` |
| 方言指令 | `load.f32` / `atom.load` / `vadd` |
| 属性块 | `{ readonly }` / `{ atomic { seqcst } }` |
| 所有权 | `owned(ptr)` / `borrowed(ptr)` |
| 调用约定 | `system_v` / `fast` / `cold` |
| 调试信息 | `scope 1 { debug x => v5 }` |

## 使用

### 安装

1. 将 `phir-extension` 目录复制到 VSCode extensions 目录，或使用 `vsce package` 打包后安装 `.vsix` 文件
2. 打开 `.phir` 文件即可自动应用语法高亮

### 颜色方案

默认使用 VSCode Dark+ 主题兼容的配色：

| 元素 | 颜色 |
|------|------|
| 关键字 (return/br/native) | 紫粉 `#C586C0` |
| 函数名 | 黄色 `#DCDCAA` |
| 全局变量 (@x) | 青绿 `#4EC9B0` |
| 基本块名 | 亮蓝 `#4FC1FF` |
| 栈槽 (ss0) | 黄色 `#D7BA7D` |
| SSA 值 (v0) | 浅蓝 `#9CDCFE` |
| 参数名 | 橙色 `#CE9178` |
| 方言前缀 | 青绿 `#4EC9B0` |
| 属性/调用约定 | 紫色 `#BC83FF` |
| 注释 | 绿色 `#6A9955` |

### 代码片段

输入以下前缀触发代码片段：

| 前缀 | 说明 |
|------|------|
| `fun` | 函数定义 |
| `funss` | 带栈槽的函数 |
| `bb` | 基本块 |
| `nfun` | 原生函数声明 |
| `ss` | 栈槽声明 |
| `gv` | 全局值引用 |
| `fnref` | 函数引用 |
| `sig` | 签名声明 |
| `const` | 常量定义 |
| `op` | 二元运算 |
| `icmp` | 整数比较 |
| `br` | 无条件跳转 |
| `brif` | 条件跳转 |
| `call` | 函数调用 |
| `invoke` | 异常调用 |
| `ret` | 返回 |
| `ld` | 栈加载 |
| `st` | 栈存储 |
| `attr` | 属性块 |
| `own` | 所有权修饰 |
| `dbg` | 调试映射 |
| `scope` | 调试作用域 |
| `main` | main 函数模板 |
| `funloop` | 循环函数模板 |

## 文件结构

```
phir-extension/
├── syntaxes/
│   └── phir.tmLanguage.json    # TextMate 语法定义
├── snippets/
│   └── phir.json               # 代码片段
├── resources/
│   ├── phir-icon.png           # 图标（普通）
│   ├── phir-icon-light.png     # 图标（浅色主题）
│   └── phir-icon-dark.png      # 图标（深色主题）
├── language-configuration.json # 语言编辑配置
├── package.json                # 扩展清单
├── README.md
└── .vscodeignore
```

## 开发

```bash
# 安装依赖
npm install

# 打包扩展
npx vsce package

# 安装本地扩展
code --install-extension phir-language-0.1.0.vsix
```

## 参考

- [Photon IR Format Specification](../../docs/photon/photon-ir-format-spec.md)
- [PHIR 高亮方案](../../docs/photon/phir-syntax-highlighting-plan.md)
- [Aura VSCode Extension](../vscode-extension/)

## 许可证

Apache-2.0
