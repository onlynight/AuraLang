# Hat Sublime Text 4 Plugin

Sublime Text 4 插件源码 — HAT v3.0 SSA IR 语法高亮 · 代码片段 · 构建集成。

> `Hat.sublime-syntax` 与 VS Code 扩展的 `syntaxes/hat.tmLanguage.json` **逐 scope 对齐**
> （sigil + 令牌架构，scope 名完全相同），改动请同步；一致性由仓库根的
> `python build/grammar-check/check-st4-sync.py` 校验。

## 项目结构

```
hat-st4/
├── Hat.sublime-syntax            # 语法高亮文法 (YAML)
├── Hat.sublime-settings          # 默认配置
├── Hat.sublime-commands          # 命令面板
├── Hat.sublime-keymap            # 快捷键绑定
├── Hat.sublime-build             # 构建系统
├── Snippets/                     # 15 个代码片段
│   ├── Module.sublime-snippet
│   ├── Function.sublime-snippet
│   ├── Extern.sublime-snippet
│   ├── BasicBlock.sublime-snippet
│   ├── ConstInt.sublime-snippet
│   ├── ConstFloat.sublime-snippet
│   ├── ConstStr.sublime-snippet
│   ├── ConstBool.sublime-snippet
│   ├── Phi.sublime-snippet
│   ├── Add.sublime-snippet
│   ├── Call.sublime-snippet
│   ├── Alloc.sublime-snippet
│   ├── Load.sublime-snippet
│   ├── Store.sublime-snippet
│   ├── Branch.sublime-snippet
│   └── Return.sublime-snippet
├── deploy.ps1                    # 部署脚本
└── README.md
```

## 部署

### 方式 1：运行部署脚本

```powershell
cd hat-st4
.\deploy.ps1
```

### 方式 2：手动拷贝

将本目录所有文件拷贝到：

```
%APPDATA%\Sublime Text\Packages\HatLanguage\
```

## 使用

打开 `.hat` 文件后，语法高亮自动生效。

| 功能 | 快捷键 | 命令面板 |
|------|--------|---------|
| 格式化文档 | `Ctrl+Shift+F` | `Hat: Format Document` |
| 自动缩进 | `Ctrl+Shift+I` | `Hat: Auto Indent` |
| 运行 | `Ctrl+B` | `Hat: Run (Hat)` |
| 符号大纲 | — | `Hat: Symbol Outline` |
| 检查 | — | `Hat: Check (Hat)` |

## 代码片段

输入以下片段名并按 Tab 展开：

| 片段名 | 说明 |
|--------|------|
| `module` | 模块头 |
| `fn` | 函数声明 |
| `extern` | 外部函数 |
| `bb` | 基本块 |
| `i32c` | 整数常量 |
| `f64c` | 浮点常量 |
| `strc` | 字符串常量 |
| `boolc` | 布尔常量 |
| `phi` | Phi 节点 |
| `add` | 加法指令 |
| `call` | 函数调用 |
| `alloc` | 分配 |
| `load` | 加载 |
| `store` | 存储 |
| `br` | 分支 |
| `ret` | 返回 |

## 自定义配置

在 `%APPDATA%\Sublime Text\Packages\User\` 下创建 `Hat.sublime-settings`：

```json
{
    "auto_format_on_save": true,
    "indent_size": 4
}
```

## 版本

基于 HAT v3.0 SSA IR · 目标 ST4 Build 4185+
