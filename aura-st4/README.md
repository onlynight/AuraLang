# Aura Sublime Text 4 Plugin

Sublime Text 4 插件源码 — 语法高亮 · 自动缩进 · 代码格式化 · 智能括号 · 代码片段 · 构建集成。

## 项目结构

```
aura-st4/
├── Aura.sublime-syntax           # 语法高亮文法 (YAML)
├── aura_auto_indent.py           # 自动缩进引擎
├── aura_formatter.py             # 代码格式化引擎
├── aura_bracket_matcher.py       # 智能括号匹配
├── Aura.sublime-settings         # 默认配置
├── Aura.sublime-commands         # 命令面板
├── Aura.sublime-keymap           # 快捷键绑定
├── Aura.sublime-build            # 构建系统
├── Snippets/                     # 19 个代码片段
│   ├── Function.sublime-snippet
│   ├── FunctionExpr.sublime-snippet
│   ├── Struct.sublime-snippet
│   ├── DataStruct.sublime-snippet
│   ├── Enum.sublime-snippet
│   ├── Interface.sublime-snippet
│   ├── Class.sublime-snippet
│   ├── Actor.sublime-snippet
│   ├── IfElse.sublime-snippet
│   ├── When.sublime-snippet
│   ├── ForLoop.sublime-snippet
│   ├── WhileLoop.sublime-snippet
│   ├── TryCatch.sublime-snippet
│   ├── SuspendFn.sublime-snippet
│   ├── GenericFn.sublime-snippet
│   ├── Ffi.sublime-snippet
│   ├── Import.sublime-snippet
│   ├── Main.sublime-snippet
│   └── Result.sublime-snippet
├── deploy.ps1                    # 部署脚本
└── README.md
```

## 部署

### 方式 1：运行部署脚本

```powershell
cd aura-st4
.\deploy.ps1
```

### 方式 2：手动拷贝

将本目录所有文件拷贝到：

```
%APPDATA%\Sublime Text\Packages\AuraLanguage\
```

## 使用

打开 `.aura` 文件后，语法高亮自动生效。

| 功能 | 快捷键 | 命令面板 |
|------|--------|---------|
| 格式化文档 | `Ctrl+Shift+F` | `Aura: Format Document` |
| 自动缩进 | `Ctrl+Shift+I` | `Aura: Auto Indent` |
| 运行 | `Ctrl+B` | `Aura: Run (Aura)` |
| 符号大纲 | `Ctrl+Shift+O` | `Aura: Symbol Outline` |
| 检查 | — | `Aura: Check (Aura)` |

## 自定义配置

在 `%APPDATA%\Sublime Text\Packages\User\` 下创建 `Aura.sublime-settings`：

```json
{
    "auto_format_on_save": true,
    "format_engine": "local",
    "indent_size": 4
}
```

## 版本

基于 Aura v0.1.x · 目标 ST4 Build 4185+
