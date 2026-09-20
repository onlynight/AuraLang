# Aura Sublime Text 4 插件设计文档

> 目标：为 Aura 语言在 Sublime Text 4 (Build 4185+) 提供完整的代码查看与编辑能力。
>
> **覆盖范围**：语法高亮 · 自动缩进 · 代码格式化 · 智能补全 · 跳转定义 · 悬停提示 · 诊断显示 · 括号匹配 · 代码片段 · 构建集成
>
> 设计日期：2026-07-26 · 基于 Aura v0.1.x

---

## 目录

1. [总览与设计目标](#1-总览与设计目标)
2. [技术选型与架构](#2-技术选型与架构)
3. [项目结构](#3-项目结构)
4. [语法高亮（Syntax Highlighting）](#4-语法高亮syntax-highlighting)
5. [自动缩进（Auto-Indentation）](#5-自动缩进auto-indentation)
6. [代码格式化（Code Formatting）](#6-代码格式化code-formatting)
7. [智能补全（Intelligent Completion）](#7-智能补全intelligent-completion)
8. [代码导航（Go to Definition）](#8-代码导航go-to-definition)
9. [诊断与错误显示（Diagnostics）](#9-诊断与错误显示diagnostics)
10. [悬停与类型信息（Hover / Type Info）](#10-悬停与类型信息hover--type-info)
11. [代码片段（Snippets）](#11-代码片段snippets)
12. [括号匹配与智能配对](#12-括号匹配与智能配对)
13. [设置与配置（Settings）](#13-设置与配置settings)
14. [快捷键（Keybindings）](#14-快捷键keybindings)
15. [构建系统集成（Build System）](#15-构建系统集成build-system)
16. [性能与缓存策略](#16-性能与缓存策略)
17. [测试策略](#17-测试策略)
18. [实施路线图](#18-实施路线图)

---

## 1. 总览与设计目标

### 1.1 目标

为 Aura 语言在 Sublime Text 4 中提供**完整、流畅、低延迟**的编辑体验，达到或超越 VS Code 扩展的功能覆盖度。

### 1.2 功能矩阵

| 功能 | 优先级 | 方案 | 依赖 |
|------|--------|------|------|
| 语法高亮 | P0 | `.sublime-syntax` 文法 | — |
| 自动缩进 | P0 | 自定义 `AutoIndent` 插件 | 语法高亮 |
| 代码格式化 | P0 | 调用 `aura fmt` / LSP | CLI/LSP |
| 智能补全 | P1 | 自定义 `CompletionProvider` | LSP / 本地索引 |
| 跳转定义 | P1 | LSP `textDocument/definition` | aura-lsp |
| 诊断显示 | P1 | LSP `textDocument/diagnostic` | aura-lsp |
| 悬停提示 | P1 | LSP `textDocument/hover` | aura-lsp |
| 代码片段 | P1 | `.sublime-snippet` 文件 | — |
| 括号匹配 | P0 | 语法内嵌 `block_ends` | — |
| 构建集成 | P2 | `.sublime-build` 文件 | CLI |
| 项目符号大纲 | P2 | LSP 文档符号 | aura-lsp |
| 重命名重构 | P2 | LSP `textDocument/rename` | aura-lsp |

### 1.3 非目标

- **不实现**独立的编译/运行（依赖 CLI 二进制）
- **不实现**独立的类型推断（依赖 LSP 或 aura-lsp）
- **不修改** Sublime Text 内核（仅通过插件 API 扩展）

---

## 2. 技术选型与架构

### 2.1 Sublime Text 4 插件能力矩阵

ST4 提供了比 ST3 更强的 Python 3 插件 API，关键能力包括：

```
┌─────────────────────────────────────────────────────────────────┐
│                    Sublime Text 4 插件架构                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────────┐  │
│  │ .sublime-    │  │ .sublime-    │  │ .sublime-snippet      │  │
│  │ syntax       │  │ settings     │  │ (XML 代码片段)         │  │
│  │ (YAML 文法)  │  │ (JSON 配置)  │  │                       │  │
│  └──────┬───────┘  └──────┬───────┘  └───────────────────────┘  │
│         │                 │                                       │
│  ┌──────▼─────────────────▼────────────────────────────────────┐  │
│  │                    Sublime Text 核心                          │  │
│  │  ┌────────────┐ ┌────────────┐ ┌────────────┐ ┌──────────┐ │  │
│  │  │ 语法引擎   │ │ 补全引擎   │ │ 缩进引擎   │ │ 构建引擎 │ │  │
│  │  └────────────┘ └────────────┘ └────────────┘ └──────────┘ │  │
│  └────────────────────────┬─────────────────────────────────────┘ │
│                           │                                        │
│  ┌────────────────────────▼─────────────────────────────────────┐ │
│  │              Python 插件层 (aura_st4_plugin/)                 │ │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │ │
│  │  │ auto_indent  │ │ formatter    │ │ lsp_client           │ │ │
│  │  │ _provider.py │ │ .py          │ │ .py                  │ │ │
│  │  └──────────────┘ └──────────────┘ └──────────────────────┘ │ │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │ │
│  │  │ completion   │ │ diagnostics  │ │ navigation           │ │ │
│  │  │ _provider.py │ │ .py          │ │ .py                  │ │ │
│  │  └──────────────┘ └──────────────┘ └──────────────────────┘ │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

### 2.2 关键技术决策

| 决策 | 选项 | 选择 | 理由 |
|------|------|------|------|
| 文法格式 | TextMate JSON / YAML `.sublime-syntax` | **YAML** | ST4 原生格式，支持 `block_ends`、`meta`、`escape`、`embed` 等高级特性 |
| 补全来源 | 本地索引 / LSP / 混合 | **混合** | 简单补全本地，语义补全走 LSP |
| 格式化引擎 | 本地 Python / LSP / CLI | **CLI 优先** | 复用 `aura fmt` 保证一致性 |
| 缩进策略 | 语法内嵌 / Python 自定义 | **Python 自定义** | Aura 有 `when`/`if` 等多行表达式，需要上下文感知 |
| 诊断缓存 | 实时 / 防抖 | **防抖 200ms** | 避免输入过程中频繁请求 |

### 2.3 LSP 通信架构

```
┌──────────────────┐         ┌──────────────────┐
│  Sublime Text    │         │  aura-lsp 进程    │
│  (Python 插件)   │         │  (JSON-RPC)       │
│                  │         │                   │
│  lsp_client.py ──┼──stdio──┼──► aura-lsp       │
│                  │  │      │  ┌─────────────┐  │
│  completion ◄────┼──┘      │  │ completion  │  │
│  definition ◄────┼─────────┼──► definition  │  │
│  hover ◄─────────┼─────────┼──► hover       │  │
│  diagnostic ◄────┼─────────┼──► diagnostic  │  │
│  formatting ◄────┼─────────┼──► formatting  │  │
└──────────────────┘         └─────────────────┘
```

---

## 3. 项目结构

```
aura-st4/                                    # 插件根目录
├── Plugins/                                 # ST4 插件目录
│   └── AuraLanguage/                        # 插件包
│       ├── aura_syntax.sublime-syntax       # 语法高亮文法（YAML）
│       ├── aura_auto_indent.py              # 自动缩进引擎
│       ├── aura_formatter.py                # 代码格式化命令
│       ├── aura_completion.py               # 智能补全提供器
│       ├── aura_lsp_client.py               # LSP JSON-RPC 客户端
│       ├── aura_navigation.py               # 跳转定义 / 悬停
│       ├── aura_diagnostics.py              # 诊断显示
│       ├── aura_symbol_outline.py           # 符号大纲
│       ├── aura_bracket_matcher.py          # 智能括号匹配
│       ├── aura_project.py                  # 项目管理 (aura.toml)
│       ├── aura_utils.py                    # 共享工具
│       ├── aura_settings.py                 # 配置管理
│       ├── Aura.sublime-snippet/            # 代码片段目录
│       │   ├── Function.sublime-snippet
│       │   ├── Struct.sublime-snippet
│       │   ├── Enum.sublime-snippet
│       │   ├── Class.sublime-snippet
│       │   ├── Interface.sublime-snippet
│       │   ├── Actor.sublime-snippet
│       │   ├── When.sublime-snippet
│       │   ├── ForLoop.sublime-snippet
│       │   ├── WhileLoop.sublime-snippet
│       │   ├── IfElse.sublime-snippet
│       │   ├── TryCatch.sublime-snippet
│       │   ├── SuspendFn.sublime-snippet
│       │   ├── Ffi.sublime-snippet
│       │   ├── Import.sublime-snippet
│       │   ├── Main.sublime-snippet
│       │   ├── DataStruct.sublime-snippet
│       │   ├── Result.sublime-snippet
│       │   └── GenericFn.sublime-snippet
│       ├── Aura.sublime-settings            # 默认设置
│       ├── Aura.sublime-build               # 构建系统
│       ├── Aura.sublime-keymap              # 快捷键绑定
│       ├── Aura.sublime-project             # 项目模板
│       ├── Aura.sublime-commands            # 自定义命令
│       ├── Messages/
│       │   └── install.txt                  # 安装说明
│       ├── tests/                           # 测试
│       │   ├── test_syntax.py
│       │   ├── test_indent.py
│       │   ├── test_formatter.py
│       │   └── fixtures/
│       │       ├── simple.aura
│       │       ├── complex.aura
│       │       ├── strings.aura
│       │       └── generics.aura
│       └── README.md
├── syntaxes/                                # 备用：TextMate JSON（兼容模式）
│   └── Aura.sublime-syntax.json             # 可选 JSON 备用文法
├── .gitignore
├── LICENSE
└── Package Control.sublime-package          # 打包产物（gitignore）
```

---

## 4. 语法高亮（Syntax Highlighting）

### 4.1 文法格式

采用 Sublime Text 4 原生 `.sublime-syntax` YAML 格式。文法结构遵循 ST4 最佳实践：

```yaml
# 语法声明
%YAML 1.2
---
name: Aura
file_extensions: [aura]
scope: source.aura
version: 2

# 语法根节点
context:
  source:
    - include: imports
    - include: code

# ... context 定义 ...
```

### 4.2 作用域命名约定

与 VS Code tmLanguage 保持一致的 `.aura` 后缀：

```
source.aura                          # 根作用域
├── storage.type.import.aura         # import 关键字
├── entity.name.package.aura         # 包名
├── storage.type.class.aura          # class 关键字
├── storage.type.struct.aura         # struct 关键字
├── storage.type.enum.aura           # enum 关键字
├── storage.type.interface.aura      # interface 关键字
├── storage.type.actor.aura          # actor 关键字
├── storage.type.object.aura         # object 关键字
├── storage.type.function.aura       # fun 关键字
├── storage.type.variable.aura       # var 关键字
├── storage.type.variable.readonly.aura  # val 关键字
├── storage.modifier.other.aura      # 修饰符 (public/private/open...)
├── entity.name.type.class.aura      # 类名
├── entity.name.type.struct.aura     # 结构体名
├── entity.name.type.enum.aura       # 枚举名
├── entity.name.type.interface.aura  # 接口名
├── entity.name.type.superclass.aura # 父类/接口名
├── entity.name.function.declaration.aura  # 函数声明名
├── entity.name.function.call.aura   # 函数调用名
├── entity.name.enum.variant.aura    # 枚举变体名
├── entity.name.label.aura           # 标签名 (loop@)
├── entity.name.type.annotation.aura # 注解名
├── entity.name.type.alias.aura      # typealias 名
├── entity.name.type.aura            # 类型引用
├── variable.parameter.aura          # 参数名
├── variable.other.readwrite.aura    # 变量名
├── variable.other.constant.aura     # 常量名
├── variable.other.property.aura     # 属性访问
├── variable.field.aura              # 成员变量
├── variable.language.this.aura      # this/super
├── variable.string-escape.aura      # 字符串插值 $var
├── variable.language.wildcard.aura  # * (通配符)
├── support.function.std.aura        # 标准库函数
├── keyword.control.aura             # 控制流 (if/while/for...)
├── keyword.hard.aura                # 硬关键字 (as/is/in/to/it)
├── keyword.soft.aura                # 软关键字 (catch/finally/else...)
├── keyword.operator.comparison.aura # 比较运算符
├── keyword.operator.arithmetic.aura # 算术运算符
├── keyword.operator.assignment.aura # 赋值运算符
├── keyword.operator.logical.aura    # 逻辑运算符
├── keyword.operator.bitwise.aura    # 位运算符
├── keyword.operator.elvis.aura      # Elvis ?:
├── keyword.operator.null-assert.aura# 非空断言 !!
├── keyword.operator.null-coalesce.aura# 空合并 ??
├── keyword.operator.range.aura      # 范围 .. ..<
├── keyword.operator.method-reference.aura # 方法引用 ::
├── keyword.other.documentation.javadoc.aura # 文档注释标记
├── string.quoted.double.aura        # 双引号字符串
├── string.quoted.single.aura        # 单引号字符
├── comment.line.double-slash.aura   # 行注释
├── comment.line.documentation.aura  # 文档行注释
├── comment.block.aura               # 块注释
├── comment.block.javadoc.aura       # 文档块注释
├── constant.numeric.decimal.aura    # 十进制字面量
├── constant.numeric.hex.aura        # 十六进制字面量
├── constant.numeric.binary.aura     # 二进制字面量
├── constant.language.boolean.aura   # true/false
├── constant.language.null.aura      # null
├── constant.character.escape.aura   # 转义字符
├── punctuation.section.block.begin.aura # {
├── punctuation.section.block.end.aura   # }
├── punctuation.section.parameters.begin.aura  # (
├── punctuation.section.parameters.end.aura    # )
├── punctuation.section.type.begin.aura # <
├── punctuation.section.type.end.aura   # >
├── punctuation.accessor.aura          # . 成员访问
├── punctuation.accessor.optional.aura # ?. 安全访问
├── punctuation.separator.period.aura  # . 分隔符
├── punctuation.separator.delimiter.aura # ,
├── punctuation.terminator.statement.aura # ;
├── meta.import.aura                   # 导入语句
├── meta.template.expression.aura      # 模板表达式 ${...}
├── storage.type.function.arrow.aura   # ->
└── storage.type.extern.aura           # extern 关键字
```

### 4.3 完整语法设计

完整语法文法包含以下 context（以实际 `.sublime-syntax` YAML 格式编写）：

#### 4.3.1 根节点与导入

```yaml
context:
  source:
    - include: imports
    - include: code

  imports:
    - match: '\b(import)\b'
      name: storage.type.import.aura
      set: imports_rest
    - match: '(?=\w)'
      push: code

  imports_rest:
    - match: '([A-Za-z_]\w*)(\.)([A-Za-z_]\w*)'
      captures:
        1: entity.name.package.aura
        2: punctuation.separator.period.aura
        3: entity.name.package.aura
    - match: '([A-Za-z_]\w*)'
      name: entity.name.package.aura
    - match: '(\.)'
      name: punctuation.separator.period.aura
    - match: '(\*)'
      name: variable.language.wildcard.aura
    - match: '(as)\s+(\w+)'
      captures:
        1: keyword.hard.aura
        2: variable.other.alias.aura
    - match: ';'
      name: punctuation.terminator.statement.aura
      pop: true
    - match: '$'
      pop: true
    - match: '\b(import)\b'
      captures: {1: storage.type.import.aura}
      set: imports_rest
    - match: '(?=\w)'
      push: code
```

#### 4.3.2 代码块

```yaml
  code:
    - include: comments
    - include: for_loop_variable
    - include: annotation
    - include: extern_declaration
    - include: data_struct_declaration
    - include: struct_declaration
    - include: struct_declaration_nobody
    - include: class_declaration
    - include: enum_declaration
    - include: interface_declaration
    - include: actor_declaration
    - include: object_declaration
    - include: type_alias
    - include: function_declaration
    - include: constant_declaration
    - include: variable_declaration
    - include: keywords
    - include: builtin_types
    - include: std_function_call
    - include: constant_name
    - include: enum_access
    - include: variable_reference
    - include: object_reference
    - include: function_call
    - include: type_annotation
    - include: property_reference
    - include: method_reference
    - include: string
    - include: string_empty
    - include: string_multiline
    - include: character
    - include: lambda_arrow
    - include: operators
    - include: self_reference
    - include: label
    - include: decimal_literal
    - include: hex_literal
    - include: binary_literal
    - include: boolean_literal
    - include: null_literal
    - include: enum_variant
    - match: ','
      name: punctuation.separator.delimiter.aura
    - match: ';'
      name: punctuation.terminator.statement.aura
    - match: '\.'
      name: punctuation.separator.period.aura
```

#### 4.3.3 注释

```yaml
  comments:
    - include: doc_comment_line
    - include: comment_line
    - include: javadoc
    - include: comment_block

  comment_line:
    - match: '//'
      scope: comment.line.double-slash.aura
      set: comment_line_content

  comment_line_content:
    - match: '$'
      pop: true
    # 行内高亮 @param 等标记

  doc_comment_line:
    - match: '///'
      scope: comment.line.documentation.aura
      set: doc_comment_content

  doc_comment_content:
    - match: '$'
      pop: true
    - match: '@(param|return|throws|see|author|version|since|deprecated|example)\b'
      name: keyword.other.documentation.javadoc.aura
    - match: '(@param)\s+(\S+)'
      captures:
        1: keyword.other.documentation.javadoc.aura
        2: variable.parameter.aura

  javadoc:
    - match: '/\*\*'
      scope: comment.block.javadoc.aura
      set: javadoc_content
    - meta_scope: comment.block.javadoc.aura

  javadoc_content:
    - match: '\*/'
      scope: comment.block.javadoc.aura
      pop: true
    - match: '@(author|deprecated|return|see|serial|since|version|param|example|throws)\b'
      name: keyword.other.documentation.javadoc.aura
    - match: '(@param)\s+(\S+)'
      captures:
        1: keyword.other.documentation.javadoc.aura
        2: variable.parameter.aura
    - match: '(@(exception|throws))\s+(\S+)'
      captures:
        1: keyword.other.documentation.javadoc.aura
        2: entity.name.type.class.aura

  comment_block:
    - match: '/\*(?!\*)'
      scope: comment.block.aura
      set: comment_block_content

  comment_block_content:
    - match: '\*/'
      scope: comment.block.aura
      pop: true
```

#### 4.3.4 关键字

```yaml
  keywords:
    - include: prefix_modifiers
    - include: postfix_modifiers
    - include: soft_keywords
    - include: hard_keywords
    - include: control_keywords
    - include: map_keywords

  prefix_modifiers:
    - match: '\b(abstract|final|enum|open|annotation|sealed|data|override|lateinit|private|protected|public|internal|inner|noinline|crossinline|vararg|reified|tailrec|operator|infix|inline|external|const|suspend|comptime|value|defer|extern|lazy|box|weak|async)\b'
      name: storage.modifier.other.aura

  postfix_modifiers:
    - match: '\b(where|by|get|set)\b'
      name: storage.modifier.other.aura

  soft_keywords:
    - match: '\b(catch|finally|field|else|then|unit)\b'
      name: keyword.soft.aura

  hard_keywords:
    - match: '\b(as|is|in|to|it)\b'
      name: keyword.hard.aura

  control_keywords:
    - match: '\b(if|while|do|when|try|throw|break|continue|return|for|select|await)\b'
      name: keyword.control.aura

  map_keywords:
    - match: '\b(to)\b'
      name: keyword.map.aura
```

#### 4.3.5 声明

```yaml
  annotation:
    - match: '(?<!\w)@([\w.]+)(?=[^:\(]|$)'
      captures:
        1: entity.name.type.annotation.aura
    - match: '(?<!\w)@([\w.]+)\s*\('
      captures:
        1: entity.name.type.annotation.aura
        2: punctuation.definition.annotation.begin.aura
      push: annotation_args

  annotation_args:
    - match: '\)'
      name: punctuation.definition.annotation.end.aura
      pop: true
    - include: code

  extern_declaration:
    - match: '\b(extern)\b\s*("[^"]*")\s*("[^"]*")?\s*\{'
      captures:
        1: storage.type.extern.aura
        2: string.quoted.double.aura
        3: string.quoted.double.aura
        4: punctuation.section.block.begin.aura
      push: extern_body

  extern_body:
    - match: '^\s*\}'
      captures:
        1: punctuation.section.block.end.aura
      pop: true
    - include: code
```

**结构体声明（带构造函数）**：

```yaml
  struct_declaration:
    - match: '(?:(data|sealed)\s+)?(struct)\s+(\b\w+\b)'
      captures:
        1: storage.modifier.other.aura
        2: storage.type.struct.aura
        3: entity.name.type.struct.aura
      push: struct_body_block

  struct_body_block:
    - match: '<[^>]+>'
      push: type_parameter_list
    - match: '\('
      push: constructor_params
    - match: '\{'
      scope: punctuation.section.block.begin.aura
      push: struct_body
    - match: '$'
      pop: true

  struct_body:
    - match: '^\s*\}'
      scope: punctuation.section.block.end.aura
      pop: true
    - include: member_variable
    - include: code
```

**类声明**：

```yaml
  class_declaration:
    - match: '(?:(sealed|data)\s+)?(class)\s+(\b\w+\b)'
      captures:
        1: storage.modifier.other.aura
        2: storage.type.class.aura
        3: entity.name.type.class.aura
      push: class_body_block

  class_body_block:
    - match: '<[^>]+>'
      push: type_parameter_list
    - match: ':\s*(\b\w+\b)(\s*<[^>]+>)?'
      captures:
        1: entity.name.type.superclass.aura
        2: entity.name.type.aura
      push: class_body_block
    - match: '\('
      push: class_construction_args
    - match: '\{'
      scope: punctuation.section.block.begin.aura
      push: class_body
    - match: '$'
      pop: true

  class_body:
    - match: '^\s*\}'
      scope: punctuation.section.block.end.aura
      pop: true
    - include: member_variable
    - include: code
```

**枚举声明**：

```yaml
  enum_declaration:
    - match: '(?:(sealed|data)\s+)?(enum)\s+(\b\w+\b)'
      captures:
        1: storage.modifier.other.aura
        2: storage.type.enum.aura
        3: entity.name.type.enum.aura
```

**接口声明**：

```yaml
  interface_declaration:
    - match: '(?:(sealed|data)\s+)?(interface)\s+(\b\w+\b)'
      captures:
        1: storage.modifier.other.aura
        2: storage.type.interface.aura
        3: entity.name.type.interface.aura
      push: interface_body
```

**Actor 声明**：

```yaml
  actor_declaration:
    - match: '(?:(sealed|data)\s+)?(actor)\s+(\b\w+\b)'
      captures:
        1: storage.modifier.other.aura
        2: storage.type.actor.aura
        3: entity.name.type.actor.aura
      push: actor_body
```

**Type alias**：

```yaml
  type_alias:
    - match: '\b(typealias)\s+(\b\w+\b)(\s*<[^>]+>)?'
      captures:
        1: storage.type.alias.aura
        2: entity.name.type.alias.aura
        3: entity.name.type.aura
```

#### 4.3.6 函数声明

```yaml
  function_declaration:
    - match: '\b(fun)\b\s*(<[^>]+>)?\s*([\w`]+|`[^`]+`)(<[^>]+>)?\s*\('
      captures:
        1: storage.type.function.aura
        2: entity.name.type.aura
        3: entity.name.function.declaration.aura
        4: entity.name.type.aura
      push: function_parameters

  function_parameters:
    - match: '\)'
      pop: true
      set: function_rest
    - include: parameter_declaration
    - include: code

  function_rest:
    - match: '->'
      name: storage.type.function.arrow.aura
      set: function_rest_ret
    - match: ':'
      name: punctuation.separator.type-annotation.aura
      set: function_rest_ret
    - match: '='
      name: keyword.operator.assignment.aura
      set: function_rest_expr
    - match: '\{'
      scope: punctuation.section.block.begin.aura
      set: function_body
    - match: '$'
      pop: true

  function_body:
    - match: '^\s*\}'
      scope: punctuation.section.block.end.aura
      pop: true
    - include: code
```

#### 4.3.7 参数声明

```yaml
  parameter_declaration:
    - match: '(\b\w+\b)\s*(=:)\s*([\w?]+(?:<[^>]+>)?)(\?)?(,)?'
      captures:
        1: variable.parameter.aura
        2: keyword.operator.assignment.type.aura
        3: entity.name.type.aura
        4: keyword.operator.optional.aura
        5: punctuation.separator.delimiter.aura
```

#### 4.3.8 内置类型

```yaml
  builtin_types:
    - match: '\b(Int|Long|Short|Byte|Float|Double|Boolean|Char|String|Any|Nothing|Unit|List|Map|Set|Array|MutableList|MutableMap|MutableSet|Result|Optional|Throwable|Exception|Iterable|Iterator|Sequence|Pair|Triple|Comparable|Number|Annotation)\b'
      name: storage.type.builtin.aura
```

#### 4.3.9 标准库函数调用

```yaml
  std_function_call:
    - match: '(aura\.\w+\.\w+)\s*\('
      captures:
        1: support.function.std.aura
      push: function_call_body
```

#### 4.3.10 字符串

```yaml
  string:
    - match: '"(?!"")'
      scope: string.quoted.double.aura
      set: string_content

  string_content:
    - match: '"(?!"")'
      scope: string.quoted.double.aura
      pop: true
    - match: '\\.'
      name: constant.character.escape.aura
    - include: string_escape_simple
    - include: string_escape_bracketed

  string_escape_simple:
    - match: '(?<!\\)\$\w+\b'
      name: variable.string-escape.aura

  string_escape_bracketed:
    - match: '(?<!\\)(\$\{)'
      captures:
        1: punctuation.definition.template-expression.begin.aura
      scope: meta.template.expression.aura
      push: template_expression

  template_expression:
    - match: '(\})'
      captures:
        1: punctuation.definition.template-expression.end.aura
      pop: true
    - include: code

  string_empty:
    - match: '(?<!")""(?!"")'
      name: string.quoted.double.aura

  string_multiline:
    - match: '"""'
      scope: string.quoted.double.aura
      set: string_multiline_content

  string_multiline_content:
    - match: '"""'
      scope: string.quoted.double.aura
      pop: true
```

#### 4.3.11 字符

```yaml
  character:
    - match: "'"
      scope: string.quoted.single.aura
      set: character_content

  character_content:
    - match: "'"
      scope: string.quoted.single.aura
      pop: true
    - match: '\\.'
      name: constant.character.escape.aura
```

#### 4.3.12 数值字面量

```yaml
  decimal_literal:
    - match: '\b\d[\d_]*(\.[\d_]+)?((e|E)\d+)?(u|U)?(L|F|f|D|d)?\b'
      name: constant.numeric.decimal.aura

  hex_literal:
    - match: '0(x|X)[A-Fa-f0-9][A-Fa-f0-9_]*(u|U)?(L|F|f|D|d)?'
      name: constant.numeric.hex.aura

  binary_literal:
    - match: '0(b|B)[01][01_]*(u|U)?(L|F|f|D|d)?'
      name: constant.numeric.binary.aura
```

#### 4.3.13 布尔与空值

```yaml
  boolean_literal:
    - match: '\b(true|false)\b'
      name: constant.language.boolean.aura

  null_literal:
    - match: '\bnull\b'
      name: constant.language.null.aura
```

#### 4.3.14 运算符

```yaml
  operators:
    - include: comparison_operators
    - include: null_operators
    - include: assignment_operators
    - include: arithmetic_operators
    - include: logical_operators
    - include: negation_operator
    - include: bitwise_operators
    - include: increment_decrement
    - include: range_operators
    - include: annotation_at
    - include: method_reference
```

#### 4.3.15 Lambda 箭头

```yaml
  lambda_arrow:
    - match: '->'
      name: storage.type.function.arrow.aura
```

#### 4.3.16 变量与引用

```yaml
  constant_name:
    - match: '\b([A-Z][A-Z0-9_]*)(?=\s*[:=])'
      captures:
        1: variable.other.constant.aura

  variable_reference:
    - match: '\b(\w+)(?=\s*[:=])'
      captures:
        1: variable.other.readwrite.aura

  object_reference:
    - match: '\b([a-zA-Z]\w*)(?=\.)'
      captures:
        1: variable.other.object.aura

  property_reference:
    - match: '(?:(\?\.)|(\.))(\w+)\b'
      captures:
        1: punctuation.accessor.optional.aura
        2: punctuation.separator.period.aura
        3: variable.other.property.aura

  method_reference:
    - match: '(\??::)(\b\w+\b|`[^`]+`)'
      captures:
        1: keyword.operator.method-reference.aura
        2: entity.name.function.reference.aura

  self_reference:
    - match: '\b(this|super)(@\w+)?\b'
      name: variable.language.this.aura

  label:
    - match: '\b(\w+)@(?!\w)'
      captures:
        1: entity.name.label.aura
```

#### 4.3.17 枚举变体

```yaml
  enum_variant:
    - match: '\b([A-Z]\w*)\b'
      name: entity.name.enum.variant.aura

  enum_access:
    - match: '\b([A-Z]\w*)(\.)([A-Z]\w*)(?!\s*\()'
      captures:
        1: entity.name.type.aura
        2: punctuation.separator.period.aura
        3: entity.name.enum.variant.aura
```

#### 4.3.18 函数调用

```yaml
  function_call:
    - match: '((?:(?:\.|\?\.)?)?)(?:(?:\w+\.)+\w+|`[^`]+`|(\w+))(?:<[^>]+>)?\s*\('
      captures:
        1: punctuation.accessor.aura
        2: entity.name.function.call.aura
      push: function_call_body

  function_call_body:
    - match: '\)'
      pop: true
    - include: named_argument
    - include: string
    - include: comments
    - include: code

  named_argument:
    - match: '\b(\w+)(?=\s*=[^=])'
      captures:
        1: variable.parameter.aura
```

#### 4.3.19 For 循环变量

```yaml
  for_loop_variable:
    - match: '\b(for)\s*\(\s*(\w+)(?::\s*[^)]*?)?\s+(in)\b'
      captures:
        1: keyword.control.aura
        2: variable.parameter.aura
        3: keyword.hard.aura
```

#### 4.3.20 成员变量

```yaml
  member_variable:
    - match: '\b(\w+)(?=\s*[:=])'
      captures:
        1: variable.field.aura
```

#### 4.3.21 结构体参数属性

```yaml
  struct_param_property:
    - match: '\b(val)\s+(\w+)'
      captures:
        1: storage.type.variable.readonly.aura
        2: variable.parameter.aura
    - match: '\b(var)\s+(\w+)'
      captures:
        1: storage.type.variable.aura
        2: variable.parameter.aura
```

### 4.4 括号匹配

在语法文件中声明 `block_ends`，让 ST4 自动匹配：

```yaml
# 在相关 context 中声明
  function_body:
    - match: '^\s*\}'
      scope: punctuation.section.block.end.aura
      block_ends: punctuation.section.block.begin.aura
      pop: true
    - include: code
```

### 4.5 主题兼容

ST4 的所有主题使用相同的 scope 名称，因此文法天然兼容所有主题（Default、Adaptive、Monokai、Solarized 等）。无需额外适配。

---

## 5. 自动缩进（Auto-Indentation）

### 5.1 设计原则

Aura 自动缩进基于**上下文感知**策略，而非简单的括号计数。核心规则：

| 触发场景 | 缩进增量 | 说明 |
|----------|---------|------|
| `{` 或 `(` 行尾 | +4 | 进入块 / 参数列表 |
| `}` 或 `)` 行首 | -4 | 退出块 / 参数列表 |
| `when` 表达式 | +4 | 分支体 |
| `if`/`else`/`try`/`catch`/`finally` | +4 | 条件块 |
| `for`/`while`/`do` | +4 | 循环体 |
| `fun`/`class`/`struct` 等声明 | +4 | 声明体 |
| 枚举体 | +4 | 枚举变体 |
| `actor`/`object`/`interface` 体 | +4 | 声明体 |
| `else ->`/`else {` | +4 | when 分支 |
| `is`/`in` 分支 | +4 | when 守卫 |
| 字符串续行 | 0 | 多行字符串 |
| 注释续行 | 0 | 保持原缩进 |

### 5.2 实现架构

```python
# aura_auto_indent.py

import sublime
import re

class AuraAutoIndent:
    """Aura 语言自动缩进引擎"""
    
    BLOCK_OPEN_PATTERNS = [
        re.compile(r'^\s*(\{)$'),                    # 块开始
        re.compile(r'^\s*(\{)\s*$'),                 # 仅括号
        re.compile(r'^\s*(\()$'),                    # 参数列表开始
        re.compile(r'\{'),                           # 行内 {
    ]
    
    BLOCK_CLOSE_PATTERNS = [
        re.compile(r'^\s*(\})'),                    # 块结束
        re.compile(r'^\s*(\))'),                    # 参数列表结束
    ]
    
    # 需要增加缩进的关键字
    KEYWORD_INDENT = re.compile(
        r'\b(if|else|when|try|catch|finally|for|while|do|fun|class|struct|enum|interface|actor|object)\b'
    )
    
    # 不需要额外缩进的场景
    NO_INDENT_PREFIXES = [
        '///', '//', '/*', '*', '*/',
    ]
    
    INDENT_SIZE = 4  # 4 空格
    
    @staticmethod
    def calculate_indent(view, pos):
        """计算 pos 位置应应用的缩进"""
        # 1. 获取当前行内容
        # 2. 向上追溯上下文，计算基础缩进
        # 3. 检测行尾符号，计算增量
        pass
    
    @staticmethod  
    def on_enter_pressed(view, region):
        """处理回车键按下的缩进"""
        pass
```

### 5.3 自动缩进算法

```
算法 AuraAutoIndent.calculate_indent():

1. 取当前行行首至行尾的文本 (current_line)
2. 如果 current_line 为空或仅包含空白:
   a. 向上寻找第一个非空行 (prev_line)
   b. 提取 prev_line 的基础缩进 (base_indent)
   c. 分析 prev_line 的末尾字符:
      - 如果以 '{' 或 '(' 结尾: 返回 base_indent + INDENT_SIZE
      - 如果以 ':' 结尾: 返回 base_indent + INDENT_SIZE (函数返回类型/注解)
      - 如果以 '=' 结尾: 返回 base_indent + INDENT_SIZE (赋值)
      - 如果以 ',' 结尾: 返回 base_indent (续行)
      - 如果行包含 'else ->' 或 'else {': 返回 base_indent + INDENT_SIZE
      - 如果行包含 'when (...)' : 返回 base_indent + INDENT_SIZE
      - 否则: 返回 base_indent
3. 如果 current_line 以 '}' 或 ')' 开头:
   a. 返回 base_indent - INDENT_SIZE
4. 否则: 返回 base_indent
```

### 5.4 上下文感知

关键：追踪**字符串内**和**注释内**的上下文，避免在字符串/注释中插入缩进。

```python
def _is_in_string_or_comment(view, pos):
    """检查 pos 是否在字符串或注释中"""
    sel = view.sel()
    # 获取当前行的语法高亮
    scope = view.scope_name(pos)
    if 'comment' in scope or 'string' in scope:
        return True
    return False
```

### 5.5 多行字符串特殊处理

```python
def handle_multiline_string(view, pos):
    """多行字符串 ("""...""") 内不自动调整缩进"""
    scope = view.scope_name(pos)
    if 'string.quoted.double' in scope:
        return sublime.NOT_FULL  # 不处理
```

---

## 6. 代码格式化（Code Formatting）

### 6.1 设计策略

| 层级 | 方案 | 延迟 | 适用场景 |
|------|------|------|---------|
| 本地快速格式化 | Python 实现 | <50ms | 简单重排、保存时格式化 |
| LSP 格式化 | aura-lsp `textDocument/formatting` | 50-200ms | 完整格式化 |
| CLI 格式化 | `aura fmt` | 100-500ms | 大文件/批量格式化 |

**策略**：优先使用 LSP 格式化；LSP 不可用时回退到本地快速格式化；Ctrl+Shift+P → "Aura: Format" 可选择 CLI。

### 6.2 本地格式化引擎

```python
# aura_formatter.py

class AuraFormatter:
    """本地代码格式化引擎（与 LSP 格式化器保持一致的规则）"""
    
    INDENT_SIZE = 4
    MAX_LINE_LENGTH = 120
    
    @staticmethod
    def format_source(source: str) -> str:
        """格式化源代码"""
        lines = source.split('\n')
        result = []
        indent_level = 0
        
        for line in lines:
            trimmed = line.strip()
            
            # 空行
            if not trimmed:
                result.append('')
                continue
            
            # 关闭括号: 减少缩进
            if trimmed.startswith('}') or trimmed.startswith(')'):
                indent_level = max(0, indent_level - 1)
            
            # 计算缩进
            indent = ' ' * (indent_level * AuraFormatter.INDENT_SIZE)
            formatted_line = f"{indent}{trimmed}"
            
            # 开启括号: 增加缩进
            if trimmed.endswith('{') or trimmed.endswith('('):
                indent_level += 1
            
            result.append(formatted_line)
        
        return '\n'.join(result)
    
    @staticmethod
    def format_document(view):
        """格式化当前文档"""
        content = view.substr(sublime.Region(0, view.size()))
        formatted = AuraFormatter.format_source(content)
        
        if formatted != content:
            view.replace(sublime.Region(0, view.size()), formatted)
```

### 6.3 LSP 格式化集成

```python
async def format_via_lsp(view):
    """通过 LSP 格式化文档"""
    client = get_lsp_client()
    if not client:
        return False
    
    edits = await client.format_document(view.file_name())
    if edits:
        apply_edits(view, edits)
        return True
    return False
```

### 6.4 CLI 格式化

```python
async def format_via_cli(view):
    """通过 aura fmt CLI 格式化文档"""
    filepath = view.file_name()
    if not filepath:
        # 保存到临时文件
        tmp = save_to_tmp(view)
        filepath = tmp
    
    proc = await asyncio.create_subprocess_exec(
        'aura', 'fmt', filepath,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()
    
    if proc.returncode == 0:
        # 读取格式化后的文件
        with open(filepath, 'r') as f:
            formatted = f.read()
        view.replace(sublime.Region(0, view.size()), formatted)
```

### 6.5 格式化规则详表

| 规则 | 示例 |
|------|------|
| 4 空格缩进 | `    val x = 1` |
| 块 `{}` 后换行 | `if (cond) {` |
| 函数声明后换行 | `fun add(a: Int, b: Int): Int {` |
| 行尾空格移除 | `val x = 1 ` → `val x = 1` |
| 连续空行合并 | 最多保留 1 个空行 |
| 枚举体缩进 | `    RED,` |
| 类体缩进 | `    val radius: Int = 10` |
| 操作符两侧空格 | `a + b` |
| 逗号后空格 | `fun f(a: Int, b: Int)` |
| 冒号后空格 | `val x: Int = 1` |
| 函数调用空格 | `foo(a, b)` |
| 类型参数空格 | `List<Int>` |

---

## 7. 智能补全（Intelligent Completion）

### 7.1 补全来源

| 来源 | 触发方式 | 内容 |
|------|---------|------|
| 关键字 | 输入匹配 | 所有保留字 |
| 内置类型 | 输入匹配 | Int/Long/String/... |
| 当前文件符号 | 输入匹配 | 当前文档的类/函数/变量 |
| 标准库 | 输入匹配 | aura.lang.std.Math.* / aura.lang.std.IO.* 等 |
| LSP 语义补全 | 输入匹配 | aura-lsp completionProvider |
| 代码片段 | Tab 插入 | `fun` → 函数模板等 |
| 导入提示 | 输入 | 自动添加 import 语句 |

### 7.2 CompletionProvider 实现

```python
# aura_completion.py

import sublime
from abc import abstractmethod


class AuraCompletionProvider(sublime.CompletionProvider):
    """Aura 语言补全提供器"""
    
    # 静态补全表
    KEYWORDS = [
        ('if', 'keyword.control.aura', 'if 条件语句'),
        ('else', 'keyword.soft.aura', 'else 分支'),
        ('when', 'keyword.control.aura', 'when 表达式'),
        ('for', 'keyword.control.aura', 'for 循环'),
        ('while', 'keyword.control.aura', 'while 循环'),
        ('try', 'keyword.control.aura', 'try 异常处理'),
        ('catch', 'keyword.soft.aura', 'catch 捕获'),
        ('fun', 'storage.type.function.aura', '函数声明'),
        ('val', 'storage.type.variable.readonly.aura', '不可变变量'),
        ('var', 'storage.type.variable.aura', '可变变量'),
        ('class', 'storage.type.class.aura', '类声明'),
        ('struct', 'storage.type.struct.aura', '结构体声明'),
        ('enum', 'storage.type.enum.aura', '枚举声明'),
        ('interface', 'storage.type.interface.aura', '接口声明'),
        ('actor', 'storage.type.actor.aura', 'Actor 声明'),
        ('import', 'storage.type.import.aura', '导入模块'),
        ('return', 'keyword.control.aura', '返回'),
        ('break', 'keyword.control.aura', '跳出'),
        ('continue', 'keyword.control.aura', '继续'),
        ('throw', 'keyword.control.aura', '抛出异常'),
        ('is', 'keyword.hard.aura', '类型检查'),
        ('in', 'keyword.hard.aura', '成员检查'),
        ('as', 'keyword.hard.aura', '类型转换'),
        ('true', 'constant.language.boolean.aura', '布尔真'),
        ('false', 'constant.language.boolean.aura', '布尔假'),
        ('null', 'constant.language.null.aura', '空值'),
        ('it', 'keyword.hard.aura', '隐式迭代变量'),
    ]
    
    BUILTIN_TYPES = [
        ('Int', 'storage.type.builtin.aura', '32位整数'),
        ('Long', 'storage.type.builtin.aura', '64位整数'),
        ('Short', 'storage.type.builtin.aura', '16位整数'),
        ('Byte', 'storage.type.builtin.aura', '8位整数'),
        ('Float', 'storage.type.builtin.aura', '单精度浮点'),
        ('Double', 'storage.type.builtin.aura', '双精度浮点'),
        ('Boolean', 'storage.type.builtin.aura', '布尔值'),
        ('Char', 'storage.type.builtin.aura', '字符'),
        ('String', 'storage.type.builtin.aura', '字符串'),
        ('Any', 'storage.type.builtin.aura', '任意类型'),
        ('Nothing', 'storage.type.builtin.aura', '空类型'),
        ('Unit', 'storage.type.builtin.aura', '无返回'),
        ('List', 'storage.type.builtin.aura', '列表'),
        ('Map', 'storage.type.builtin.aura', '映射'),
        ('Set', 'storage.type.builtin.aura', '集合'),
        ('Array', 'storage.type.builtin.aura', '数组'),
        ('Result', 'storage.type.builtin.aura', '结果类型'),
        ('Exception', 'storage.type.builtin.aura', '异常'),
        ('Pair', 'storage.type.builtin.aura', '二元组'),
    ]
    
    STD_MODULES = [
        ('aura.lang.std.Math', 'support.function.std.aura', '数学函数'),
        ('aura.lang.std.IO', 'support.function.std.aura', 'I/O 函数'),
        ('aura.lang.std.Collections', 'support.function.std.aura', '集合操作'),
        ('aura.lang.std.{Coroutine,Actor,Channel}', 'support.function.std.aura', '并发 API'),
        ('aura.lang.std.Json', 'support.function.std.aura', 'JSON 解析'),
        ('aura.lang.std.String', 'support.function.std.aura', '字符串操作'),
        ('aura.lang.std.FileSystem', 'support.function.std.aura', '文件系统'),
        ('aura.lang.std.Env', 'support.function.std.aura', '环境变量'),
        ('aura.lang.std.Time', 'support.function.std.aura', '时间与日期'),
        ('aura.lang.std.Path', 'support.function.std.aura', '路径操作'),
        ('aura.lang.std.Network', 'support.function.std.aura', '网络'),
        ('aura.lang.std.Random', 'support.function.std.aura', '随机数'),
        ('aura.lang.std.Encoding', 'support.function.std.aura', '编码'),
        ('aura.lang.std.Console', 'support.function.std.aura', '终端控制'),
        ('aura.lang.std.Assert', 'support.function.std.aura', '断言'),
        ('aura.lang.std.Test', 'support.function.std.aura', '测试框架'),
        ('aura.lang.std.Ascii', 'support.function.std.aura', 'ASCII 操作'),
        ('aura.lang.std.Iter', 'support.function.std.aura', '迭代器'),
    ]
    
    def __init__(self, view):
        self.view = view
    
    def is_applicable(self, prefix):
        return True
    
    def get_completions(self, prefix, done, is_async):
        completions = []
        
        # 1. 关键字
        for name, scope, detail in self.KEYWORDS:
            if name.startswith(prefix):
                completions.append(sublime.CompletionItem(
                    trigger=name,
                    annotation=detail,
                    kind=sublime.KIND_KEYWORD
                ))
        
        # 2. 内置类型
        for name, scope, detail in self.BUILTIN_TYPES:
            if name.startswith(prefix):
                completions.append(sublime.CompletionItem(
                    trigger=name,
                    annotation=detail,
                    kind=sublime.KIND_TYPE
                ))
        
        # 3. 标准库模块
        for name, scope, detail in self.STD_MODULES:
            if name.startswith(prefix):
                completions.append(sublime.CompletionItem(
                    trigger=name,
                    annotation=detail,
                    kind=sublime.KIND_SNIPPET
                ))
        
        # 4. 当前文件符号（异步从索引获取）
        if not is_async:
            done(completions)
        else:
            # 异步获取文件符号和 LSP 补全
            async def fetch_async():
                file_completions = await self._get_file_symbols()
                completions.extend(file_completions)
                
                # LSP 补全
                lsp_completions = await self._get_lsp_completions(prefix)
                completions.extend(lsp_completions)
                
                done(completions)
            
            sublime.set_async(lambda: fetch_async())
```

### 7.3 触发字符配置

在 `Aura.sublime-settings` 中配置：

```json
{
    "auto_complete_triggers": [
        { "characters": [".", "(", ":", " "] }
    ],
    "auto_complete_selector": "source.aura, string.aura",
    "match_brackets": true,
    "match_brackets_content": true,
    "match_brackets_on_demand": false
}
```

### 7.4 LSP 语义补全

```python
async def _get_lsp_completions(self, prefix):
    """从 LSP 获取语义补全"""
    client = get_lsp_client()
    if not client:
        return []
    
    items = await client.completion()
    if not items:
        return []
    
    completions = []
    for item in items:
        label = item.get('label', '')
        if label.startswith(prefix):
            kind = self._map_lsp_kind(item.get('kind', 0))
            detail = item.get('detail', '')
            completions.append(sublime.CompletionItem(
                trigger=label,
                annotation=detail,
                kind=kind
            ))
    
    return completions
```

---

## 8. 代码导航（Go to Definition）

### 8.1 实现策略

| 方式 | 触发 | 延迟 | 准确率 |
|------|------|------|--------|
| LSP definition | Ctrl+Click / F12 | 50-200ms | 高 |
| 本地符号索引 | F12 (fallback) | <10ms | 中 |

### 8.2 LSP 跳转定义

```python
# aura_navigation.py

async def goto_definition(view, point):
    """跳转到定义位置"""
    client = get_lsp_client()
    if not client:
        # 回退到本地符号索引
        return local_goto_definition(view, point)
    
    pos = sublime.pos2linecol(view.rowcol(point))
    location = await client.goto_definition(pos['line'], pos['column'])
    
    if location:
        uri = location.get('uri', '')
        range_info = location.get('range', {})
        target_line = range_info.get('start', {}).get('line', 0)
        target_col = range_info.get('start', {}).get('character', 0)
        
        # 打开目标文件并定位
        target_view = open_file(uri)
        if target_view:
            rowcol = (target_line, target_col)
            point = sublime.linecol2pos(rowcol, target_view)
            target_view.sel().clear()
            target_view.sel().add(sublime.Region(point))
            target_view.show_at_center(point)
```

### 8.3 本地符号索引（fallback）

```python
def build_local_symbol_index(view):
    """扫描文件构建本地符号索引"""
    content = view.substr(sublime.Region(0, view.size()))
    symbols = {}
    
    # 函数定义
    for match in re.finditer(r'\bfun\s+(\w+)\s*\(', content):
        line = content[:match.start()].count('\n')
        symbols[match.group(1)] = {
            'kind': 'function',
            'line': line,
            'col': match.start() - content.rfind('\n', 0, match.start()),
        }
    
    # 类定义
    for match in re.finditer(r'\bclass\s+(\w+)', content):
        line = content[:match.start()].count('\n')
        symbols[match.group(1)] = {
            'kind': 'class',
            'line': line,
            'col': match.start() - content.rfind('\n', 0, match.start()),
        }
    
    # 结构体定义
    for match in re.finditer(r'\bstruct\s+(\w+)', content):
        line = content[:match.start()].count('\n')
        symbols[match.group(1)] = {
            'kind': 'struct',
            'line': line,
            'col': match.start() - content.rfind('\n', 0, match.start()),
        }
    
    # 变量定义
    for match in re.finditer(r'\bval\s+(\w+)', content):
        line = content[:match.start()].count('\n')
        if match.group(1) not in symbols:
            symbols[match.group(1)] = {
                'kind': 'val',
                'line': line,
                'col': match.start() - content.rfind('\n', 0, match.start()),
            }
    
    return symbols
```

---

## 9. 诊断与错误显示（Diagnostics）

### 9.1 架构

```
┌────────────────────┐    ┌──────────────────┐    ┌──────────────────┐
│  aura-lsp          │    │  LSP Client       │    │  Sublime View    │
│  textDocument/     │◄──►│  aura_diagnostics │◄──►│  区域高亮        │
│  diagnostic        │    │  .py              │    │  错误列表        │
└────────────────────┘    └──────────────────┘    └──────────────────┘
```

### 9.2 诊断显示

```python
# aura_diagnostics.py

class AuraDiagnostics:
    """诊断显示管理"""
    
    REGION_KEY = 'aura_diagnostics'
    FLAG_OVERLAY = 0x00000100  # 覆盖显示
    
    def show_diagnostics(self, view, diagnostics):
        """在视图上显示诊断"""
        # 清除旧的诊断区域
        view.erase_regions(self.REGION_KEY)
        
        regions = []
        # 诊断图标
        for diag in diagnostics:
            range = diag.get('range', {})
            start = sublime.LineCol(
                range.get('start', {}).get('line', 0),
                range.get('start', {}).get('character', 0)
            )
            end = sublime.LineCol(
                range.get('end', {}).get('line', 0),
                range.get('end', {}).get('character', 0)
            )
            
            start_pos = sublime.linecol2pos(start, view)
            end_pos = sublime.linecol2pos(end, view)
            region = sublime.Region(start_pos, end_pos)
            
            severity = diag.get('severity', 3)
            if severity == 1:  # Error
                region.append(
                    sublime.Region(region.a, region.a + 1),
                    'aura_diag_error'
                )
            elif severity == 2:  # Warning
                region.append(
                    sublime.Region(region.a, region.a + 1),
                    'aura_diag_warning'
                )
            
            regions.append(region)
        
        view.add_regions(self.REGION_KEY, regions)
    
    def show_diagnostics_panel(self, view, diagnostics):
        """在侧边面板显示诊断列表"""
        pass
```

### 9.3 诊断防抖

```python
class DebouncedDiagnostics:
    """诊断请求防抖"""
    
    DEBOUNCE_MS = 200
    
    def __init__(self, view):
        self.view = view
        self.pending_request = None
    
    def request(self):
        """发起防抖请求"""
        if self.pending_request:
            self.pending_request.cancel()
        
        self.pending_request = sublime.set_timeout_async(
            self._do_request, self.DEBOUNCE_MS
        )
    
    async def _do_request(self):
        """实际请求"""
        client = get_lsp_client()
        if not client:
            return
        
        diagnostics = await client.get_diagnostics()
        self.view.run_command(
            'aura_show_diagnostics', {'diagnostics': diagnostics}
        )
```

---

## 10. 悬停与类型信息（Hover / Type Info）

### 10.1 悬停实现

```python
# aura_navigation.py

async def show_hover(view, point):
    """显示悬停信息"""
    client = get_lsp_client()
    if not client:
        return
    
    pos = sublime.pos2linecol(view.rowcol(point))
    result = await client.hover(pos['line'], pos['column'])
    
    if result and result.get('contents'):
        markdown = result['contents'].get('value', '')
        
        # 使用 ST4 的 show_popup
        view.show_popup(
            self._format_hover_popup(markdown),
            location=point,
            max_width=600,
            max_height=400,
        )
    
    @staticmethod
    def _format_hover_popup(markdown):
        """将 LSP Markdown 转换为 ST4 popup HTML"""
        # 简化的 Markdown → HTML 转换
        html = ''
        for line in markdown.split('\n'):
            if line.startswith('**') and line.endswith('**'):
                html += f'<b>{line[2:-2]}</b>'
            elif line.startswith('```'):
                continue
            else:
                html += line
        return html
```

### 10.2 类型信息快速查看

```python
def quick_type_info(view, point):
    """快速查看光标位置的类型信息"""
    content = view.substr(view.word(point))
    scope = view.scope_name(point)
    
    # 从语法作用域推断类型
    if 'storage.type.builtin' in scope:
        return content + ' (built-in type)'
    if 'entity.name.function' in scope:
        return f'Function: {content}'
    if 'variable' in scope:
        return f'Variable: {content}'
    
    # 通过 LSP 获取详细类型
    # ...
```

---

## 11. 代码片段（Snippets）

### 11.1 片段列表

基于 VS Code 扩展的 19 个片段，转换为 ST4 `.sublime-snippet` XML 格式：

| 文件 | 前缀 | 模板 |
|------|------|------|
| Function.sublime-snippet | fn | `fun ${1:name}(${2:params}): ${3:ReturnType} { $0 }` |
| FunctionExpr.sublime-snippet | fn= | `fun ${1:name}(${2:params}): ${3:ReturnType} = ${4:expression}` |
| Struct.sublime-snippet | struct | `struct ${1:Name}( val ${2:field1}: ${3:Type}, var ${4:field2}: ${5:Type} = ${6:defaultValue} )` |
| DataStruct.sublime-snippet | data | `struct ${1:Name}( val ${2:field1}: ${3:Type1}, val ${4:field2}: ${5:Type2} )` |
| Enum.sublime-snippet | enum | `enum ${1:Name} { ${2:VALUE1}, ${3:VALUE2}, ${4:CUSTOM}(val ${5:name}: ${6:Type}) }` |
| Interface.sublime-snippet | interface | `interface ${1:Name} { fun ${2:method}(${3:params}): ${4:ReturnType}; fun ${5:method2}(${6:params}): ${7:ReturnType} = ${8:defaultImpl} }` |
| Class.sublime-snippet | class | `class ${1:Name} : ${2:Interface} { override fun ${3:method}(${4:params}): ${5:ReturnType} { $0 } }` |
| Actor.sublime-snippet | actor | `actor ${1:Name} { private var ${2:state}: ${3:Type} = ${4:default}; fun start() { $0 } }` |
| IfElse.sublime-snippet | if | `if (${1:condition}) { $0 } else { ${2:else} }` |
| When.sublime-snippet | when | `when (${1:value}) { ${2:case1} -> { $0 } else -> { ${3:else} } }` |
| ForLoop.sublime-snippet | for | `for (${1:item} in ${2:iterator}) { $0 }` |
| WhileLoop.sublime-snippet | while | `while (${1:condition}) { $0 }` |
| TryCatch.sublime-snippet | try | `try { $0 } catch (e: ${1:Exception}) { ${2:handle} }` |
| SuspendFn.sublime-snippet | suspend | `suspend fun ${1:name}(${2:params}): ${3:ReturnType} { $0 }` |
| GenericFn.sublime-snippet | gen | `fun <${1:T}> ${2:name}(${3:arg}: ${1:T}): ${1:T} { return ${3:arg} }` |
| Ffi.sublime-snippet | ffi | `extern "c" "${1:library}" { fun ${2:functionName}(${3:params}): ${4:ReturnType}; val ${5:CONSTANT}: ${6:Type} }` |
| Import.sublime-snippet | import | `import ${1:module} as ${2:alias}` |
| Main.sublime-snippet | main | `fun main() { println("Hello, Aura!") }` |
| Result.sublime-snippet | result | `fun ${1:name}(): Result<${2:Type}, ${3:Error}> { if (${4:success}) { return Result.Success(${5:value}) } else { return Result.Error(${6:error}) } }` |

### 11.2 片段 XML 格式示例

```xml
<!-- Function.sublime-snippet -->
<?xml version="1.0" encoding="UTF-8"?>
<snippet>
    <content><![CDATA[fun ${1:name}(${2:params}): ${3:ReturnType} {
    $0
}]]></content>
    <tabTrigger>fn</tabTrigger>
    <scope>source.aura</scope>
    <description>创建新函数</description>
</snippet>
```

### 11.3 自动导入片段

```xml
<!-- Import.sublime-snippet -->
<?xml version="1.0" encoding="UTF-8"?>
<snippet>
    <content><![CDATA[import ${1:module} as ${2:alias}]]></content>
    <tabTrigger>import</tabTrigger>
    <scope>source.aura</scope>
    <description>导入模块</description>
</snippet>
```

---

## 12. 括号匹配与智能配对

### 12.1 语法级括号匹配

在 `.sublime-syntax` 中，通过 `block_ends` 自动声明括号匹配对：

```yaml
# 块匹配
  block:
    - match: '\{'
      scope: punctuation.section.block.begin.aura
      push: block_content
    
  block_content:
    - match: '\}'
      scope: punctuation.section.block.end.aura
      block_ends: punctuation.section.block.begin.aura
      pop: true
    - include: code
```

### 12.2 智能配对增强

```python
# aura_bracket_matcher.py

class SmartBracketMatcher:
    """增强括号匹配：自动补全成对的括号"""
    
    PAIRS = {
        '(': ')',
        '{': '}',
        '[': ']',
        '"': '"',
    }
    
    # 不自动配对的上下文
    NO_PAIR_SCOPES = [
        'comment',
        'string',
    ]
    
    @staticmethod
    def should_auto_pair(view, pos):
        """判断是否应自动配对"""
        scope = view.scope_name(pos)
        for no_scope in SmartBracketMatcher.NO_PAIR_SCOPES:
            if no_scope in scope:
                return False
        return True
```

### 12.3 智能跳过

按下右括号时，如果前面已经有右括号，自动跳过而非插入：

```python
def on_text_inserted(view, text, regions):
    """插入文本时的智能处理"""
    if text in [')', ']', '}']:
        pos = regions[0].b
        if pos < view.size():
            next_char = view.substr(sublime.Region(pos, pos + 1))
            if next_char == text:
                # 跳过已有括号
                view.sel().add(sublime.Region(pos + 1))
```

---

## 13. 设置与配置（Settings）

### 13.1 默认设置

```json
{
    // ── 格式化 ──
    "auto_format_on_save": false,
    "format_engine": "lsp",           // "lsp" | "local" | "cli"
    "indent_size": 4,
    "max_line_length": 120,
    "trailing_comma": true,
    "remove_trailing_whitespace": true,
    "ensure_newline_at_eof": true,
    
    // ── LSP ──
    "lsp_enabled": true,
    "lsp_binary": "aura-lsp",         // 可配置路径
    "lsp_debounce_ms": 200,
    
    // ── 自动缩进 ──
    "auto_indent_enabled": true,
    "auto_indent_use_spaces": true,
    
    // ── 补全 ──
    "auto_complete_trigger_chars": [".", "(", ":", " "],
    "show_documentation": true,
    "enable_snippet_completion": true,
    
    // ── 导航 ──
    "definition_command": "ctrl+click",
    "hover_delay_ms": 300,
    
    // ── 诊断 ──
    "show_diagnostics": true,
    "diagnostic_region_flags": 0x00000100,
    
    // ── 括号 ──
    "match_brackets": true,
    "match_brackets_content": true,
    
    // ── 大纲 ──
    "show_symbol_outline": true,
    
    // ── 构建 ──
    "build_system": "Aura.sublime-build",
    "build_command": "aura run"
}
```

### 13.2 配置加载

```python
# aura_settings.py

class AuraSettings:
    """配置管理"""
    
    _instance = None
    
    @classmethod
    def get_instance(cls):
        if cls._instance is None:
            cls._instance = cls()
        return cls._instance
    
    def __init__(self):
        # 加载默认设置
        defaults = sublime.load_settings('Aura.sublime-settings')
        # 加载用户设置覆盖
        user_settings = sublime.load_settings('User/Aura.sublime-settings')
        # 合并（用户覆盖默认）
        self.settings = {**defaults, **user_settings}
    
    def get(self, key, default=None):
        return self.settings.get(key, default)
```

---

## 14. 快捷键（Keybindings）

### 14.1 默认快捷键

```json
[
    // 格式化文档
    {
        "keys": ["ctrl+shift+f"],
        "command": "aura_format_document",
        "args": { "engine": "auto" }
    },
    // 本地快速格式化
    {
        "keys": ["ctrl+shift+f"],
        "command": "aura_format_document",
        "args": { "engine": "local" },
        "context": [
            { "key": "setting.aura_format_engine", "operator": "equal", "value": "local" }
        ]
    },
    // 代码片段触发
    {
        "keys": ["tab"],
        "command": "next_completion",
        "args": { "index": 1 }
    },
    // 跳转到定义
    {
        "keys": ["f12"],
        "command": "aura_goto_definition"
    },
    // 跳转到定义（按住 Ctrl 点击）
    {
        "keys": ["ctrl+click"],
        "command": "aura_goto_definition"
    },
    // 悬停
    {
        "keys": ["ctrl+space"],
        "command": "aura_hover"
    },
    // 显示大纲
    {
        "keys": ["ctrl+shift+o"],
        "command": "show_symbol_list",
        "args": { "symbols_style": "all", "regex": "\\b(fun|class|struct|enum|interface|actor|object|typealias|val|var)\\s+\\w+" }
    },
    // 构建运行
    {
        "keys": ["ctrl+b"],
        "command": "build",
        "args": { "shell_cmd": "aura run", "file_regex": "^(.+\\.aura):(\\d+)(:\\d+)?:\\s(.*)$" }
    },
    // 编译检查
    {
        "keys": ["ctrl+shift+b"],
        "command": "build",
        "args": { "shell_cmd": "aura check" }
    },
    // 诊断刷新
    {
        "keys": ["f5"],
        "command": "aura_refresh_diagnostics"
    },
    // 格式化（仅 CLI）
    {
        "keys": ["alt+shift+f"],
        "command": "aura_format_document",
        "args": { "engine": "cli" }
    },
    // 跳转到上一个/下一个诊断
    {
        "keys": ["ctrl+shift+up"],
        "command": "aura_next_diagnostic",
        "args": { "direction": "prev" }
    },
    {
        "keys": ["ctrl+shift+down"],
        "command": "aura_next_diagnostic",
        "args": { "direction": "next" }
    },
    // 类型信息
    {
        "keys": ["ctrl+shift+i"],
        "command": "aura_type_info"
    }
]
```

---

## 15. 构建系统集成（Build System）

### 15.1 构建系统定义

```json
// Aura.sublime-build
{
    "cmd": [
        "aura",
        "run",
        "$file"
    ],
    "file_regex": "^(.+\\.aura):(\\d+)(:\\d+)?:\\s(.*)$",
    "shell_cmd": "aura run \"$file\"",
    "encoding": "UTF-8",
    "line_regex": "^(.+\\.aura):(\\d+)(:\\d+)?:\\s(.*)$",
    "windows": {
        "cmd": ["aura", "run", "$file"]
    },
    "linux": {
        "cmd": ["aura", "run", "$file"]
    },
    "mac": {
        "cmd": ["aura", "run", "$file"]
    }
}
```

### 15.2 扩展构建系统

```json
// Aura Check.sublime-build
{
    "cmd": ["aura", "check", "$file"],
    "file_regex": "^(.+\\.aura):(\\d+)(:\\d+)?:\\s(.*)$",
    "shell_cmd": "aura check \"$file\""
}

// Aura Build AOT.sublime-build
{
    "cmd": ["aura", "build", "--aot", "$file"],
    "file_regex": "^(.+\\.aura):(\\d+)(:\\d+)?:\\s(.*)$",
    "shell_cmd": "aura build --aot \"$file\""
}

// Aura Test.sublime-build
{
    "cmd": ["loom", "test", "$file"],
    "file_regex": "^(.+\\.aura):(\\d+)(:\\d+)?:\\s(.*)$",
    "shell_cmd": "loom test \"$file\""
}
```

---

## 16. 性能与缓存策略

### 16.1 性能预算

| 操作 | 目标延迟 | 策略 |
|------|---------|------|
| 高亮（小文件 <1000 行） | <10ms | 语法引擎原生 |
| 高亮（大文件 >5000 行） | <50ms | 语法引擎原生 + 渐进 |
| 自动缩进 | <1ms | 同步计算 |
| 格式化（本地） | <50ms | 同步计算 |
| 格式化（LSP） | <200ms | 异步 + 防抖 |
| 补全（本地） | <10ms | 缓存 |
| 补全（LSP） | <200ms | 异步 |
| 诊断刷新 | <200ms | 防抖 200ms |
| 悬停 | <300ms | 延迟 300ms |
| 跳转定义 | <200ms | 异步 |

### 16.2 LSP 进程管理

```python
# aura_lsp_client.py

class LspClient:
    """LSP JSON-RPC 客户端"""
    
    def __init__(self):
        self.process = None
        self.next_id = 1
        self.pending = {}  # id → future
    
    async def start(self):
        """启动 LSP 进程"""
        self.process = await asyncio.create_subprocess_exec(
            self.settings.get('lsp_binary', 'aura-lsp'),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
    
    async def request(self, method, params):
        """发送 LSP 请求"""
        self.next_id += 1
        msg = {
            'jsonrpc': '2.0',
            'id': self.next_id - 1,
            'method': method,
            'params': params,
        }
        
        line = json.dumps(msg) + '\n'
        self.process.stdin.write(line.encode('utf-8'))
        await self.process.stdin.drain()
        
        # 等待响应
        future = self.pending[self.next_id - 1]
        return await future
    
    async def notify(self, method, params):
        """发送 LSP 通知"""
        msg = {
            'jsonrpc': '2.0',
            'method': method,
            'params': params,
        }
        
        line = json.dumps(msg) + '\n'
        self.process.stdin.write(line.encode('utf-8'))
        await self.process.stdin.drain()
    
    async def _read_loop(self):
        """读取 LSP 响应"""
        while True:
            # 读取 Content-Length
            header = await self.process.stdout.readline()
            length = int(header.decode().split(':')[1].strip())
            
            # 读取空行
            await self.process.stdout.readline()
            
            # 读取消息体
            body = await self.process.stdout.readexactly(length)
            msg = json.loads(body.decode('utf-8'))
            
            if 'id' in msg:
                # 响应消息
                future = self.pending.pop(msg['id'], None)
                if future:
                    if 'error' in msg:
                        future.set_exception(msg['error'])
                    else:
                        future.set_result(msg.get('result'))
            
            elif 'method' in msg:
                # 服务器推送（诊断等）
                await self._handle_server_push(msg)
```

### 16.3 防抖机制

```python
class Debouncer:
    """通用防抖器"""
    
    def __init__(self, delay_ms):
        self.delay_ms = delay_ms
        self.timeout = None
    
    async def call(self, func, *args, **kwargs):
        """带防抖的异步调用"""
        if self.timeout:
            self.timeout.cancel()
        
        self.timeout = asyncio.create_task(self._debounced_call(func, *args, **kwargs))
        return await self.timeout
    
    async def _debounced_call(self, func, *args, **kwargs):
        await asyncio.sleep(self.delay_ms / 1000.0)
        self.timeout = None
        return await func(*args, **kwargs)
```

### 16.4 进程健康检查

```python
class LspProcessMonitor:
    """LSP 进程监控"""
    
    CHECK_INTERVAL = 30.0
    
    @staticmethod
    async def monitor():
        while True:
            await asyncio.sleep(LspProcessMonitor.CHECK_INTERVAL)
            client = get_lsp_client()
            if client and client.process is None:
                # 进程已退出，尝试重启
                await client.start()
                log.error("LSP process restarted")
```

---

## 17. 测试策略

### 17.1 测试框架

使用 `unittest` + ST4 的 `sublime_plugin` 测试框架（ST4 内置）：

```python
# tests/test_syntax.py
import unittest
import sublime
import sublime_plugin

class AuraSyntaxTest(unittest.TestCase):
    """语法高亮测试"""
    
    def setUp(self):
        # 打开测试文件
        self.view = sublime.load_settings('Aura.sublime-settings')
        sublime.settings('Aura.sublime-settings').add_on_change('aura_test', self._reload)
    
    def _reload(self, key):
        pass
    
    def test_keyword_highlight(self):
        """测试关键字高亮"""
        view = sublime.active_window().new_file()
        view.set_syntax_file('Packages/AuraLanguage/aura_syntax.sublime-syntax')
        view.set_contents("fun main() {\n    val x = 1\n}\n")
        
        # 检查 fun 的作用域
        scope = view.scope_name(0)
        self.assertIn('storage.type.function', scope)
    
    def test_string_highlight(self):
        """测试字符串高亮"""
        view = sublime.active_window().new_file()
        view.set_syntax_file('Packages/AuraLanguage/aura_syntax.sublime-syntax')
        view.set_contents('val s = "hello"\n')
        
        # 检查字符串的作用域
        # ...
    
    def test_comment_highlight(self):
        """测试注释高亮"""
        view = sublime.active_window().new_file()
        view.set_syntax_file('Packages/AuraLanguage/aura_syntax.sublime-syntax')
        view.set_contents('// comment\n')
        
        scope = view.scope_name(0)
        self.assertIn('comment.line.double-slash', scope)
    
    def test_annotation_highlight(self):
        """测试注解高亮"""
        view = sublime.active_window().new_file()
        view.set_syntax_file('Packages/AuraLanguage/aura_syntax.sublime-syntax')
        view.set_contents('@Deprecated fun f() {}\n')
        
        scope = view.scope_name(0)
        self.assertIn('entity.name.type.annotation', scope)


class AuraIndentTest(unittest.TestCase):
    """自动缩进测试"""
    
    def test_basic_block_indent(self):
        """基本块缩进"""
        result = calculate_indent("fun main() {\n", 2)
        self.assertEqual(result, 4)
    
    def test_nested_block_indent(self):
        """嵌套块缩进"""
        result = calculate_indent("fun main() {\n    if (x) {\n", 2)
        self.assertEqual(result, 8)
    
    def test_close_brace_dedent(self):
        """关闭括号退格"""
        result = calculate_indent("fun main() {\n    \n}\n", 1)
        self.assertEqual(result, 0)
```

### 17.2 语法测试覆盖

| 测试项 | 覆盖内容 |
|--------|---------|
| 关键字 | 所有保留字的高亮 |
| 类型 | 内置类型名 |
| 声明 | class/struct/enum/interface/actor/object |
| 函数 | 函数声明 + 参数 + 返回类型 |
| 变量 | val/var/常量/成员变量 |
| 字符串 | 单行/多行/插值/转义 |
| 注释 | 行注释/块注释/文档注释 |
| 运算符 | 所有运算符类型 |
| 数值 | 十进制/十六进制/二进制 |
| 注解 | 简单注解/带参注解 |
| FFI | extern 声明 |
| 泛型 | 类型参数 + 约束 |
| Lambda | 箭头函数 |
| 标签 | 循环标签 |

### 17.3 集成测试

```python
class AuraFormatterTest(unittest.TestCase):
    """格式化测试"""
    
    def test_indent_basic(self):
        """基本缩进"""
        input = "fun main() {\nval x = 1\n}"
        output = "fun main() {\n    val x = 1\n}"
        self.assertEqual(format_source(input), output)
    
    def test_nested_blocks(self):
        """嵌套块"""
        input = "fun main() {\nif (x) {\nval y = 1\n}\n}"
        output = "fun main() {\n    if (x) {\n        val y = 1\n    }\n}"
        self.assertEqual(format_source(input), output)
    
    def test_enum_body(self):
        """枚举体"""
        input = "enum Color {\nRED\nGREEN\n}"
        output = "enum Color {\n    RED\n    GREEN\n}"
        self.assertEqual(format_source(input), output)
```

---

## 18. 实施路线图

### Phase 1：基础功能（核心体验）

```
├── Week 1-2: 语法高亮
│   ├── .sublime-syntax 文法编写
│   ├── 关键字/类型/运算符/字面量
│   ├── 声明（class/struct/enum/interface/actor）
│   ├── 函数声明/参数
│   ├── 字符串/字符/注释
│   └── 作用域命名 + 主题兼容验证
│
├── Week 2-3: 自动缩进
│   ├── 基础括号计数缩进
│   ├── 关键字感知缩进
│   ├── 上下文检测（字符串/注释）
│   └── 多行字符串处理
│
├── Week 3-4: 代码格式化
│   ├── 本地格式化引擎
│   ├── LSP 格式化集成
│   ├── CLI 格式化回退
│   └── 格式化命令 + 快捷键
│
└── Week 4: 基础测试
    ├── 语法测试
    ├── 缩进测试
    └── 格式化测试
```

### Phase 2：智能编辑（LSP 集成）

```
├── Week 5-6: LSP 客户端
│   ├── JSON-RPC 通信层
│   ├── 进程生命周期管理
│   ├── 文档同步（didOpen/didChange/didClose）
│   └── 防抖 + 异步处理
│
├── Week 6-7: 智能补全
│   ├── CompletionProvider 实现
│   ├── 关键字/类型/标准库补全
│   ├── 文件符号索引
│   └── LSP 语义补全
│
├── Week 7-8: 导航与诊断
│   ├── 跳转定义（LSP + 本地 fallback）
│   ├── 诊断显示（区域高亮）
│   ├── 诊断防抖
│   └── 悬停提示
│
└── Week 8: 代码片段 + 快捷键
    ├── 19 个 .sublime-snippet
    ├── 快捷键配置
    └── 构建系统配置
```

### Phase 3：增强体验

```
├── Week 9: 大纲视图
│   ├── 符号提取
│   └── Ctrl+Shift+O 大纲面板
│
├── Week 10: 智能括号
│   ├── 自动配对
│   ├── 智能跳过
│   └── 匹配高亮
│
├── Week 11: 项目管理
│   ├── aura.toml 识别
│   ├── 项目符号视图
│   └── 依赖高亮
│
├── Week 12: 重命名重构
│   ├── LSP rename
│   └── 命令面板集成
│
└── Week 13: 性能优化
    ├── 大文件渐进高亮
    ├── LSP 连接池
    ├── 缓存优化
    └── 基准测试
```

### Phase 4：发布与生态

```
├── Week 14: Package Control 发布
│   ├── Package Control.sublime-package 打包
│   ├── GitHub Pages 安装
│   └── README + 安装文档
│
├── Week 15: 社区贡献
│   ├── CONTRIBUTING.md
│   ├── 代码审查流程
│   └── Issue 模板
│
└── Week 16: 稳定化
    ├── Bug 修复
    ├── 边缘测试
    └── 1.0 发布
```

---

## 附录 A：与 VS Code 扩展对比

| 能力 | VS Code 扩展 | ST4 插件 | 说明 |
|------|-------------|---------|------|
| 语法高亮 | tmLanguage JSON | .sublime-syntax YAML | 功能等价 |
| 作用域命名 | `source.aura` | `source.aura` | 完全一致 |
| 补全 | LSP completionProvider | CompletionProvider + LSP | ST4 混合方案 |
| 跳转定义 | LSP definition | LSP + 本地 fallback | ST4 增强 |
| 悬停 | LSP hover | LSP + scope 推断 | ST4 增强 |
| 格式化 | LSP documentFormattingProvider | 本地 + LSP + CLI | ST4 三层方案 |
| 诊断 | LSP diagnostic | 区域高亮 + 面板 | ST4 增强 |
| 片段 | aura.lang.std.Json | .sublime-snippet XML | 格式不同，内容一致 |
| 构建 | task.json | .sublime-build | 格式不同 |
| 键映射 | keybindings.json | .sublime-keymap | 格式不同 |

## 附录 B：关键 API 参考

| API | 用途 | 调用频率 |
|-----|------|---------|
| `view.scope_name(pos)` | 获取位置作用域 | 高（缩进/补全） |
| `view.substr(region)` | 读取文本 | 高 |
| `view.replace(region, text)` | 替换文本 | 低 |
| `view.add_regions(key, regions)` | 添加区域高亮 | 低 |
| `view.erase_regions(key)` | 清除区域 | 低 |
| `view.show_popup(content, ...)` | 显示弹窗 | 低（悬停） |
| `view.sel()` | 光标选择 | 高 |
| `view.rowcol(point)` | 坐标转换 | 高 |
| `sublime.set_timeout_async(fn, ms)` | 异步调度 | 高 |
| `sublime.load_settings(path)` | 加载配置 | 低 |
| `sublime.active_window()` | 获取窗口 | 高 |
| `sublime_plugin.Plugin` | 插件基类 | — |
| `sublime.CompletionProvider` | 补全基类 | — |
| `asyncio.create_subprocess_exec()` | 启动子进程 | 低 |
| `asyncio.create_task()` | 创建任务 | 中 |

## 附录 C：错误处理策略

```python
# 通用错误处理包装器
def safe_lsp_call(coro_func, *args, default=None):
    """安全的 LSP 调用：异常时返回默认值"""
    try:
        return asyncio.run(coro_func(*args))
    except Exception as e:
        log.warning("LSP call failed: %s", e)
        return default

# LSP 连接失败回退
def get_lsp_client():
    """获取 LSP 客户端（可能为 None）"""
    client = _lsp_client
    if client and client.process and client.process.returncode is not None:
        # 进程已退出
        log.warning("LSP process died, reconnecting...")
        asyncio.run(client.start())
    return client if client and client.process else None
```

## 附录 D：版本兼容

| 特性 | 最低 ST4 Build | 本文档目标 |
|------|--------------|-----------|
| Python 3 插件 API | 4000 | 4185 |
| `sublime.CompletionProvider` | 4000 | 4185 |
| `view.show_popup` | 3200 | 4185 |
| `asyncio` 支持 | 4000 | 4185 |
| `sublime.Region` 操作 | 3200 | 4185 |
| `block_ends` (语法) | 4000 | 4185 |

---

*本文档基于 Aura v0.1.x 编写，随语言规范演进将同步更新。*
