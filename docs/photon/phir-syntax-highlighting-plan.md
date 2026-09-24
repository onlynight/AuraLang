# PHIR 语法高亮方案 — 详细设计文档

> **目标**：为 Photon IR (`.phir`) 文本格式设计 TextMate 语法高亮方案，参照现有 Aura VSCode 插件的 `aura.tmLanguage.json` 架构。
>
> **产出**：
> - `tools/ide-extension/vscode-extension/syntaxes/phir.tmLanguage.json` — TextMate 语法文件
> - `tools/ide-extension/vscode-extension/snippets/phir.json` — 代码片段
> - 更新 `package.json` — 注册新语言 + 颜色自定义
> - 更新 `language-configuration.json` — PHIR 语言配置
>
> **当前阶段**：分析 + 方案设计，不做实际修改。

---

## 1. 现有 Aura 高亮方案分析

### 1.1 架构总览

| 组件 | 文件 | 说明 |
|------|------|------|
| **语法定义** | `syntaxes/aura.tmLanguage.json` | TextMate 语法规则，scopeName = `source.aura` |
| **语言注册** | `package.json` → `contributes.languages` | 注册 `aura` 语言 ID、文件扩展名、图标 |
| **语法注册** | `package.json` → `contributes.grammars` | 绑定 `source.aura` ↔ `aura.tmLanguage.json` |
| **颜色定制** | `package.json` → `configurationDefaults` | 通过 `editor.tokenColorCustomizations.textMateRules` 按 scope 着色 |
| **编辑行为** | `language-configuration.json` | 注释、括号、缩进、折叠、续行 |
| **代码片段** | `snippets/aura.json` | `fun` / `if` / `while` 等模板 |

### 1.2 tmLanguage.json 结构模式

```
顶层 patterns → #code (主容器)
                ↓
         #comments       — 注释优先匹配
         #function-declaration — 函数声明 (begin/end)
         #keywords        — 关键字分类
         #builtin-types   — 内置类型
         #string          — 字符串
         #operators       — 运算符
         ... (40+ 规则)
                ↓
    repository 中所有规则通过 #include 引用，可复用
```

**关键设计模式**：

| 模式 | 示例 | 用途 |
|------|------|------|
| **`begin`/`end` + `beginCaptures`/`endCaptures`** | `function-declaration` | 复杂结构体（函数体、块、括号） |
| **`match` + `captures`** | `keywords`, `operators` | 单一 token 高亮 |
| **嵌套 `include`** | `#code` → `#parameter-declaration` | 上下文复用 |
| **命名约定** | 所有 scope 以 `.aura` 结尾 | 区分语言，避免主题冲突 |
| **颜色定制** | `package.json` → `textMateRules` | 按 scope 覆盖默认主题颜色 |

### 1.3 Scope 命名规范（Aura）

```
source.aura                          — 文件根 scope
├── meta.*                           — 元结构
│   ├── meta.package.aura
│   ├── meta.import.aura
│   ├── meta.template.expression.aura
│   └── meta.*.aura
├── entity.name.*                    — 实体名称
│   ├── entity.name.type.aura
│   ├── entity.name.type.class.aura
│   ├── entity.name.function.declaration.aura
│   ├── entity.name.function.call.aura
│   ├── entity.name.package.aura
│   ├── entity.name.label.aura
│   ├── entity.name.type.annotation.aura
│   └── entity.name.enum.variant.aura
├── storage.*                        — 存储/类型标记
│   ├── storage.type.aura
│   ├── storage.type.package.aura
│   ├── storage.type.import.aura
│   ├── storage.type.function.aura
│   ├── storage.type.builtin.aura
│   ├── storage.type.variable.aura
│   ├── storage.type.variable.readonly.aura
│   ├── storage.modifier.other.aura
│   └── storage.type.function.arrow.aura
├── keyword.*                        — 关键字
│   ├── keyword.control.aura
│   ├── keyword.soft.aura
│   ├── keyword.hard.aura
│   ├── keyword.operator.*           — 各类运算符
│   └── keyword.other.documentation.javadoc.aura
├── variable.*                       — 变量
│   ├── variable.parameter.aura
│   ├── variable.other.readwrite.aura
│   ├── variable.other.object.aura
│   ├── variable.field.aura
│   ├── variable.other.property.aura
│   ├── variable.other.constant.aura
│   ├── variable.language.this.aura
│   └── variable.language.wildcard.aura
├── constant.*                       — 常量
│   ├── constant.numeric.*           — decimal / hex / binary
│   ├── constant.language.boolean.aura
│   ├── constant.language.null.aura
│   └── constant.character.escape.aura
├── string.*                         — 字符串
│   ├── string.quoted.double.aura
│   └── string.quoted.single.aura
├── comment.*                        — 注释
│   ├── comment.line.double-slash.aura
│   ├── comment.block.aura
│   ├── comment.block.javadoc.aura
│   └── comment.line.documentation.aura
├── punctuation.*                    — 标点
│   ├── punctuation.separator.*      — delimiter / period / type-annotation
│   ├── punctuation.terminator.statement.aura
│   ├── punctuation.section.block.*  — begin / end
│   ├── punctuation.section.parameters.*
│   ├── punctuation.definition.annotation.*
│   ├── punctuation.definition.template-expression.*
│   └── punctuation.accessor.*       — 属性访问
├── support.function.std.aura        — 标准库函数
└── variable.string-escape.aura      — 字符串插值
```

### 1.4 颜色定制方案（Aura）

在 `package.json` 的 `configurationDefaults["[aura]"].editor.tokenColorCustomizations.textMateRules` 中定义：

| Scope | 颜色 | 用途 |
|-------|------|------|
| `support.function.std.aura` | `#4EC9B0` (青绿) | 标准库函数调用 |
| `entity.name.type.annotation.aura` | `#BC83FF` (紫) | 注解 |
| `storage.type.extern.aura` | `#C586C0` (紫粉) | extern 标记 |
| `storage.type.package.aura` | `#569CD6` (蓝) | package 关键字 |
| `entity.name.type.enum.variant.aura` | `#4FC1FF` (亮蓝) | 枚举值 |
| `variable.other.constant.aura` | `#AAA0FA` (紫蓝) | 常量名 |
| `variable.other.readwrite.aura, variable.other.object.aura, variable.field.aura, variable.other.property.aura` | `#9CDCFE` (蓝) | 变量/属性 |
| `variable.parameter.aura` | `#CE9178` (橙) | 参数名 |
| `keyword.operator.elvis.aura, keyword.operator.null-assert.aura, keyword.operator.null-coalesce.aura` | `#D7BA7D` (黄) | 特殊运算符 |

---

## 2. PHIR 语法元素分析

### 2.1 语法元素全景图

根据 `photon-ir-format-spec.md`，PHIR 包含以下语法元素：

```
PHIR 语法元素
├── 元数据
│   ├── 模块头: # module <name> target <triple> flags <hex>
│   ├── 源文件: # source <path>
│   └── 方言声明: # dialect <name>
├── 全局声明
│   ├── 全局常量: @str.hello = "hello world"
│   └── 全局变量: @counter = i32 0
├── 函数声明
│   ├── 原生函数: native fun write(fd: i32, buf: ptr, len: i64) -> i32
│   └── 函数定义: fun strlen(s: ptr) -> i64 { ... }
├── 函数前言 (Preamble)
│   ├── 栈槽:     ss0 = f64 8
│   ├── 函数引用: fn0 = @strlen(ptr) -> i64
│   ├── 签名:     sig0 = (i32, i32) -> i32
│   └── 全局值:   gv0 = @str.hello / gv0 = vmctx
├── 基本块
│   ├── 块头:     bb entry(v0: ptr, v1: i64):
│   ├── 块体:     (statements + terminator)
│   └── 结束:     (由下一 bb 或 })
├── 语句 (Statement)
│   ├── 赋值:     v2 = const i64 42
│   ├── 栈存储:   store i32 v3, ss0
│   ├── 栈加载:   v4 = load.i32 ss1
│   ├── 定义:     let x: i32 = 5 / var y = 3
│   └── 调试:     debug x => v5
├── 终止符 (Terminator)
│   ├── 返回:     return v1
│   ├── 无条件跳转: br block1
│   ├── 条件跳转: br if v2, block1, block2
│   ├── 调用:     v3 = call fn0(v1, v2)
│   └── 不可达:   unreachable
├── 缩进语法
│   ├── if/else:  if a > b { return a } else { return b }
│   └── while:    while m != 0 { m = m / 10 }
├── 指令属性
│   ├── 操作数属性: { commutative } / { associative }
│   ├── 内存属性:   { readonly } / { volatile }
│   ├── 调用属性:   { nounwind } / { tail } / { inline }
│   └── 原子属性:   { atomic { seqcst } } / { seqcst }
├── 方言指令
│   ├── 内存:    load.f32 / store.f64 / load.i8 / load.ptr
│   ├── 浮点:    fpromote.f64 / fdemote.f32 / fmin / fmax / frnd
│   ├── SIMD:    vadd / vload.f32x4 / vstore.f32x4 / vsplat
│   ├── 原子:    atom.load / atom.store / atom.cas / fence
│   ├── 对象:    new / retain / release / getfield / setfield
│   └── 调试:    debug / printf / verify
├── 值系统
│   ├── SSA 值:   v0, v1, v2, ...
│   ├── 栈槽:     ss0, ss1, ...
│   ├── 局部引用: %v1
│   ├── 全局引用: @str.hello
│   └── 自我引用: this
├── 操作数
│   ├── 常量:     const i32 42 / const f64 0.0
│   ├── 复制:     copy v1
│   └── 移动:     move v1
├── 类型系统
│   ├── 基础类型: void, bool, i1-i128, f32, f64
│   ├── 指针类型: ptr, ptr<T>, ref<T>, ref mut<T>
│   ├── 复合类型: string, struct<T>, list<T>, map<K,V>
│   ├── 函数类型: (T) -> R
│   ├── SIMD:     vector<N, T>
│   └── 所有权:   owned<T>, borrowed<T>, guaranteed<T>
├── 所有权修饰符
│   ├── owned, borrowed, guaranteed, copy
├── 调用约定
│   ├── system_v, fast, cold, windows_fastcall
│   └── probestack, apple_aarch64, winch
├── 函数属性
│   ├── noinline, cold, noreturn, nosideeffects
│   └── noalias, nobuiltin, always_inline
├── 异常处理
│   ├── [return: ok, unwind: catch]
├── 调试作用域
│   └── scope 1 { debug x => v1 }
└── 调试断点
    └── debug v1
```

### 2.2 与 Aura 语法的关键差异

| 维度 | Aura | PHIR |
|------|------|------|
| **注释** | `//` 和 `/* */` | `#` |
| **作用域** | 花括号 `{}` | 缩进 + 花括号混用 |
| **类型系统** | 类/结构体/枚举 | SSA + 栈槽 + BB 参数 |
| **控制流** | 结构化 if/while/for | 显式 BB + br/br if |
| **值引用** | 变量名 | `v0` (SSA) / `ss0` (栈槽) / `%v0` / `@global` |
| **函数调用** | `fn0(args)` | `call fn0(args) -> type` |
| **指令** | 表达式语句 | 单一赋值 + 终止符 |
| **属性** | `@` 注解 | `{ }` 属性块 |
| **方言** | 无 | 前缀指令 (`load.f32`) |
| **所有权** | `val`/`var` | `owned`/`borrowed`/`guaranteed` |
| **特殊符号** | `this`, `super` | `this`, `vmctx` |

---

## 3. PHIR 高亮方案设计

### 3.1 文件结构

```
tools/ide-extension/vscode-extension/
├── syntaxes/
│   ├── aura.tmLanguage.json        # (已有) Aura 语法
│   └── phir.tmLanguage.json        # (新增) PHIR 语法
├── snippets/
│   ├── aura.json                   # (已有) Aura 片段
│   └── phir.json                   # (新增) PHIR 片段
├── language-configuration.json     # (更新) 添加 PHIR 编辑行为
├── package.json                    # (更新) 注册 phir 语言
└── ...
```

### 3.2 Scope 命名规范

```
source.phir                          — 文件根 scope
├── meta.*                           — 元结构
│   ├── meta.module-header.phir
│   ├── meta.preamble.phir
│   ├── meta.basic-block.phir
│   ├── meta.attribute.phir          — { ... } 属性块
│   ├── meta.exception-landing.phir  — [return: , unwind: ]
│   └── meta.dialect.phir            — 方言前缀
├── entity.name.*                    — 实体名称
│   ├── entity.name.module.phir      — 模块名
│   ├── entity.name.function.phir    — 函数名
│   ├── entity.name.function.call.phir — 调用目标
│   ├── entity.name.global.phir      — 全局变量名 (@x)
│   ├── entity.name.block.phir       — BB 名称 (entry, loop)
│   ├── entity.name.stack-slot.phir  — 栈槽名 (ss0)
│   ├── entity.name.type.phir        — 类型名
│   ├── entity.name.type.builtin.phir — 内置类型
│   ├── entity.name.type.generic.phir — 泛型类型
│   ├── entity.name.parameter.phir   — 参数名
│   ├── entity.name.local.phir       — SSA 局部变量 (v0)
│   ├── entity.name.operand.const.phir  — 常量值
│   ├── entity.name.operand.copy.phir   — copy 操作数
│   ├── entity.name.operand.move.phir   — move 操作数
│   ├── entity.name.dialect.phir   — 方言名
│   ├── entity.name.attr.phir      — 属性名
│   ├── entity.name.calling-convention.phir — 调用约定
│   └── entity.name.ownership.phir — 所有权修饰符
├── storage.*                        — 存储/类型标记
│   ├── storage.type.global.phir
│   ├── storage.type.native.phir
│   ├── storage.type.function.phir
│   ├── storage.type.keyword.phir
│   ├── storage.modifier.ownership.phir
│   └── storage.type.builtin.phir
├── keyword.*                        — 关键字
│   ├── keyword.control.phir         — if, while, return, br, unreachable
│   ├── keyword.terminator.phir      — return, br, call, unreachable
│   ├── keyword.statement.phir       — store, load, let, var, debug
│   ├── keyword.operator.*           — 各类运算符
│   └── keyword.dialect.phir         — load, store, fence 等方言前缀
├── variable.*                       — 变量
│   ├── variable.parameter.phir      — 参数名
│   ├── variable.local.phir          — SSA 值 (v0, v1)
│   ├── variable.stack-slot.phir     — 栈槽 (ss0)
│   ├── variable.global.phir         — 全局引用 (@x)
│   ├── variable.language.this.phir  — this
│   ├── variable.language.vmctx.phir — vmctx
│   ├── variable.debug-var.phir      — 调试变量 (debug x => v5)
│   └── variable.scope-id.phir       — 作用域 ID
├── constant.*                       — 常量
│   ├── constant.numeric.phir        — 数值字面量
│   ├── constant.numeric.hex.phir    — 十六进制
│   ├── constant.language.boolean.phir
│   ├── constant.language.null.phir
│   ├── constant.language.nan.phir   — NaN
│   └── constant.language.true.phir
├── string.*                         — 字符串
│   └── string.quoted.double.phir
├── comment.*                        — 注释
│   └── comment.line.number-sign.phir
├── punctuation.*                    — 标点
│   ├── punctuation.separator.*      — delimiter / period / assignment / arrow
│   ├── punctuation.terminator.statement.phir — 语句结束符 (;)
│   ├── punctuation.section.block.*  — { }
│   ├── punctuation.section.parameters.*
│   ├── punctuation.section.attribute.* — { } 属性
│   ├── punctuation.section.exception.* — [ ] 异常
│   ├── punctuation.section.scope.*  — 作用域
│   └── punctuation.accessor.*       — .field / [index]
├── support.*                        — 支持元素
│   ├── support.function.call.phir   — 函数调用
│   └── support.instruction.phir     — 指令操作码
└── variable.string-escape.phir      — 字符串插值（PHIR 中不适用）
```

### 3.3 语法规则详细设计

#### 3.3.1 顶层结构

```json
{
    "$schema": "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
    "name": "Photon IR",
    "scopeName": "source.phir",
    "fileTypes": ["phir"],
    "patterns": [
        { "include": "#comments" },
        { "include": "#module-header" },
        { "include": "#globals" },
        { "include": "#native-function-declaration" },
        { "include": "#function-declaration" },
        { "include": "#dialect-declaration" },
        { "include": "#code" }
    ]
}
```

#### 3.3.2 注释

PHIR 只有 `#` 行注释：

```json
"comments": {
    "patterns": [
        {
            "include": "#comment-line"
        }
    ]
},
"comment-line": {
    "begin": "#",
    "end": "$",
    "name": "comment.line.number-sign.phir"
}
```

#### 3.3.3 模块头

```phir
# module hello target x86_64-pc-windows-msvc flags 0x01
# source hello.aura
# dialect <name>
```

```json
"module-header": {
    "begin": "#\\s*(module|source|dialect)\\b",
    "end": "$",
    "name": "meta.module-header.phir",
    "captures": {
        "1": { "name": "storage.type.keyword.phir" }
    }
}
```

> 注意：模块头本身也是注释形式，所以需要先匹配注释规则，再在注释内部识别关键字。

#### 3.3.4 全局声明

```phir
@str.hello = "hello world"
@counter = i32 0
```

```json
"globals": {
    "begin": "(@)([\\w.]+)(?=\\s*=)",
    "beginCaptures": {
        "1": { "name": "punctuation.accessor.global.phir" },
        "2": { "name": "entity.name.global.phir" }
    },
    "end": "$",
    "patterns": [
        { "include": "#string" },
        { "include": "#type-annotation" },
        { "include": "#numeric-literal" },
        { "include": "#boolean-literal" }
    ]
}
```

#### 3.3.5 原生函数声明

```phir
native fun write(fd: i32, buf: ptr, len: i64) -> i32
native fun malloc(size: i64) -> ptr
```

```json
"native-function-declaration": {
    "begin": "\\b(native)\\s+(fun)\\s+(\\b\\w+\\b)(\\s*\\()",
    "beginCaptures": {
        "1": { "name": "storage.type.native.phir" },
        "2": { "name": "storage.type.function.phir" },
        "3": { "name": "entity.name.function.phir" },
        "4": { "name": "punctuation.section.parameters.begin.phir" }
    },
    "end": "(\\))",
    "endCaptures": {
        "1": { "name": "punctuation.section.parameters.end.phir" }
    },
    "patterns": [
        { "include": "#parameter" },
        { "include": "#type-annotation" },
        { "include": "#return-arrow" }
    ]
}
```

#### 3.3.6 函数定义

```phir
fun strlen(s: ptr) -> i64 {
    ss0 = i64 8
    ss1 = ptr 8

    bb entry(v0: ptr):
        v1 = const i64 0
        ...
        return v9
}
```

**关键挑战**：PHIR 函数体包含 preamble（栈槽、函数引用等）和基本块。需要用 `begin`/`end` 捕获花括号范围，内部按缩进区分。

```json
"function-declaration": {
    "begin": "\\b(fun)\\s+(\\b\\w+\\b)(\\s*\\()",
    "beginCaptures": {
        "1": { "name": "storage.type.function.phir" },
        "2": { "name": "entity.name.function.phir" },
        "3": { "name": "punctuation.section.parameters.begin.phir" }
    },
    "end": "(^\\s*\\})",
    "endCaptures": {
        "1": { "name": "punctuation.section.block.end.phir" }
    },
    "patterns": [
        { "include": "#parameter" },
        { "include": "#return-arrow" },
        { "include": "#function-attribute" },
        { "include": "#calling-convention" },
        { "include": "#preamble-stack-slot" },
        { "include": "#preamble-fn-ref" },
        { "include": "#preamble-sig" },
        { "include": "#preamble-global-value" },
        { "include": "#basic-block" },
        { "include": "#indent-scope" },
        { "include": "#comments" },
        { "include": "#code" }
    ]
}
```

#### 3.3.7 函数前言

##### 栈槽声明

```phir
ss0 = f64 8
ss1 = ptr 8
```

```json
"preamble-stack-slot": {
    "match": "\\b(ss\\d+)\\s*=\\s*([\\w?]+(?:<[^>]+>)?)\\s+(\\d+)",
    "captures": {
        "1": { "name": "variable.stack-slot.phir" },
        "2": { "name": "entity.name.type.builtin.phir" },
        "3": { "name": "constant.numeric.phir" }
    }
}
```

##### 函数引用

```phir
fn0 = @strlen(ptr) -> i64
```

```json
"preamble-fn-ref": {
    "begin": "\\b(fn\\d+)\\s*=(?!=)",
    "beginCaptures": {
        "1": { "name": "entity.name.parameter.phir" }
    },
    "end": "$",
    "patterns": [
        { "include": "#global-reference" },
        { "include": "#parameter-list" },
        { "include": "#return-arrow" },
        { "include": "#type-annotation" }
    ]
}
```

##### 签名声明

```phir
sig0 = (i32, i32) -> i32
```

```json
"preamble-sig": {
    "begin": "\\b(sig\\d+)\\s*=",
    "beginCaptures": {
        "1": { "name": "entity.name.parameter.phir" }
    },
    "end": "$",
    "patterns": [
        { "include": "#parameter-list" },
        { "include": "#return-arrow" },
        { "include": "#type-annotation" }
    ]
}
```

##### 全局值

```phir
gv0 = @str.hello
gv0 = vmctx
gv3 = load.i32 gv0[8]
```

```json
"preamble-global-value": {
    "begin": "\\b(gv\\d+)\\s*=",
    "beginCaptures": {
        "1": { "name": "entity.name.parameter.phir" }
    },
    "end": "$",
    "patterns": [
        { "include": "#global-reference" },
        { "include": "#vmctx" },
        { "include": "#dialect-instruction" }
    ]
}
```

#### 3.3.8 基本块

```phir
bb entry(v0: ptr, v1: i64):
    v2 = const f64 0.0
    store f64 v2, ss0
    br if v1, block1, block2

bb block1:
    ...

bb block2(v4: i64):
    ...
```

```json
"basic-block": {
    "begin": "\\b(bb)\\s+(\\b\\w+\\b)(?:(\\s*\\([^)]*\\))?):",
    "beginCaptures": {
        "1": { "name": "storage.type.keyword.phir" },
        "2": { "name": "entity.name.block.phir" },
        "3": { "name": "meta.block-params.phir" }
    },
    "end": "(?=\\bb\\b|\\}|$)",
    "patterns": [
        { "include": "#block-params" },
        { "include": "#statement-assignment" },
        { "include": "#statement-store" },
        { "include": "#statement-load" },
        { "include": "#statement-debug" },
        { "include": "#statement-def" },
        { "include": "#terminator" },
        { "include": "#exception-landing" },
        { "include": "#comments" },
        { "include": "#code" }
    ]
}
```

> **注意**：基本块的 `end` 是 lookahead 模式 — 遇到下一个 `bb` 或函数结束 `}` 时结束。这要求块内不能有 `{` 嵌套块（PHIR 的 BB 块体不使用 `{}`）。

#### 3.3.9 块参数（BB 参数）

```phir
bb entry(v0: ptr, v1: i64):
```

```json
"block-params": {
    "begin": "\\(",
    "end": "\\)",
    "patterns": [
        { "include": "#parameter" }
    ]
}
```

#### 3.3.10 参数

```phir
param       = ident (':' type)?
```

```json
"parameter": {
    "match": "(\\b\\w+\\b)(\\s*:\\s*([\\w?]+(?:<[^>]+>)?(?:\\s*->\\s*[\\w?]+)?))?(,)?",
    "captures": {
        "1": { "name": "variable.parameter.phir" },
        "3": { "name": "entity.name.type.phir" },
        "4": { "name": "punctuation.separator.delimiter.phir" }
    }
}
```

#### 3.3.11 返回箭头

```json
"return-arrow": {
    "match": "->",
    "name": "keyword.operator.assignment.phir"
}
```

#### 3.3.12 语句

##### 赋值语句

```phir
v2 = const i64 42
v5 = add v4, 4
v12 = icmp ult v11, v1
v14 = fcvt_from_uint.f64 v1
```

```json
"statement-assignment": {
    "begin": "(v\\d+)\\s*(=)(?!=)",
    "beginCaptures": {
        "1": { "name": "variable.local.phir" }
    },
    "end": ";|$",
    "patterns": [
        { "include": "#operand" },
        { "include": "#instruction-opcode" },
        { "include": "#dialect-instruction" },
        { "include": "#attribute-block" },
        { "include": "#numeric-literal" },
        { "include": "#boolean-literal" },
        { "include": "#local-reference" },
        { "include": "#global-reference" },
        { "include": "#stack-slot-reference" },
        { "include": "#type-annotation" },
        { "include": "#operators" },
        { "include": "#string" }
    ]
}
```

##### 栈存储

```phir
store i32 v3, ss0
store f64 v10, ss0
```

```json
"statement-store": {
    "begin": "\\b(store)\\b",
    "beginCaptures": {
        "1": { "name": "keyword.statement.phir" }
    },
    "end": ";|$",
    "patterns": [
        { "include": "#attribute-block" },
        { "include": "#type-annotation" },
        { "include": "#local-reference" },
        { "include": "#stack-slot-reference" }
    ]
}
```

##### 栈加载

```phir
v4 = load.i32 ss1
v2 = load.ptr ss1
v13 = load.f64 ss0
```

`load` 属于方言指令，统一在 `#dialect-instruction` 中处理。

##### 调试语句

```phir
debug x => v5
debug v1
printf v1
verify v1
```

```json
"statement-debug": {
    "begin": "\\b(debug|printf|verify)\\b",
    "beginCaptures": {
        "1": { "name": "keyword.statement.phir" }
    },
    "end": ";|$",
    "patterns": [
        { "include": "#debug-variable" },
        { "include": "#local-reference" }
    ]
}
```

#### 3.3.13 终止符

```phir
return v16
return
br loop
br block1
br if v12, block2(v11), block3
v3 = call @strlen(ptr @str.hello) -> i64
unreachable
```

```json
"terminator": {
    "patterns": [
        { "include": "#terminator-return" },
        { "include": "#terminator-br" },
        { "include": "#terminator-call" },
        { "include": "#terminator-unreachable" }
    ]
},
"terminator-return": {
    "begin": "\\b(return)\\b",
    "beginCaptures": {
        "1": { "name": "keyword.terminator.phir" }
    },
    "end": ";|$",
    "patterns": [
        { "include": "#local-reference" },
        { "include": "#numeric-literal" }
    ]
},
"terminator-br": {
    "begin": "\\b(br)\\s+if\\b",
    "beginCaptures": {
        "1": { "name": "keyword.terminator.phir" }
    },
    "end": ";|$",
    "patterns": [
        { "include": "#local-reference" },
        { "include": "#block-target" },
        { "include": "#block-args" }
    ]
},
"terminator-call": {
    "begin": "(?=\\w+\\s*=?\\s*call\\b|\\bcall\\b)",
    "end": ";|$",
    "patterns": [
        { "include": "#terminator-call-body" }
    ]
}
```

##### 异常落地

```phir
v1 = call @divide(i32 100, i32 v0) -> i32
      [return: ok, unwind: catch]
```

```json
"exception-landing": {
    "begin": "\\[\\s*(return|unwind)\\s*:",
    "end": "\\]",
    "patterns": [
        { "include": "#block-target" },
        { "include": "#keyword-exception" }
    ]
}
```

#### 3.3.14 缩进语法

```phir
fun max(a: i32, b: i32) -> i32 {
    if a > b {
        return a
    } else {
        return b
    }
}
```

```json
"indent-scope": {
    "patterns": [
        { "include": "#indent-if" },
        { "include": "#indent-else" },
        { "include": "#indent-while" }
    ]
},
"indent-if": {
    "begin": "\\b(if)\\b",
    "beginCaptures": {
        "1": { "name": "keyword.control.phir" }
    },
    "end": "(?=\\bb\\b|\\}|$)",
    "patterns": [
        { "include": "#code" }
    ]
},
"indent-else": {
    "match": "\\b(else)\\b",
    "name": "keyword.control.phir"
},
"indent-while": {
    "begin": "\\b(while)\\b",
    "beginCaptures": {
        "1": { "name": "keyword.control.phir" }
    },
    "end": "(?=\\bb\\b|\\}|$)",
    "patterns": [
        { "include": "#code" }
    ]
}
```

#### 3.3.15 指令操作码

```phir
add, sub, mul, div, rem, icmp, fcmp, phi,
ret, br, call, invoke, vcall, new,
retain, release, getfield, setfield, gep,
syscall, fence
```

```json
"instruction-opcode": {
    "match": "\\b(add|sub|mul|div|rem|icmp|fcmp|phi|ret|br|call|invoke|vcall|new|retain|release|getfield|setfield|gep|syscall|fence|const|let|var)\\b",
    "name": "support.instruction.phir"
}
```

#### 3.3.16 方言指令

方言指令格式：`dialect.opcode` 或 `dialect.opcode.type`

```phir
load.f32 v1
store.f64 v1, ss0
fpromote.f64 v2
fdemote.f32 v2
vadd i32x4 v2, v3
vload.f32x4 v2
atom.load { seqcst } v2
```

```json
"dialect-instruction": {
    "match": "\\b((?:load|store|fpromote|fdemote|fmin|fmax|frnd|vadd|vsub|vmul|vdiv|vload|vstore|vsplat|atom\\.load|atom\\.store|atom\\.cas|atom\\.cmpxchg)\\.\\w+)?\\b",
    "name": "keyword.dialect.phir"
}
```

> **注意**：这里需要更精细的模式来区分方言前缀和操作码。实际实现应使用 `begin`/`end` 模式：

```json
"dialect-instruction": {
    "begin": "\\b((?:load|store|fpromote|fdemote|fmin|fmax|frnd|vadd|vsub|vmul|vdiv|vload|vstore|vsplat|atom\\.load|atom\\.store|atom\\.cas)\\.)(\\w+)?",
    "beginCaptures": {
        "1": { "name": "keyword.dialect.phir" },
        "2": { "name": "entity.name.type.builtin.phir" }
    },
    "end": "$"
}
```

#### 3.3.17 操作数

```phir
const i32 42
const f64 0.0
copy v1
move v1
```

```json
"operand": {
    "begin": "\\b(const|copy|move)\\b",
    "beginCaptures": {
        "1": { "name": "storage.modifier.operand.phir" }
    },
    "end": ";|$",
    "patterns": [
        { "include": "#type-annotation" },
        { "include": "#numeric-literal" },
        { "include": "#boolean-literal" },
        { "include": "#local-reference" },
        { "include": "#global-reference" }
    ]
}
```

#### 3.3.18 属性块

```phir
{ commutative }
{ readonly }
{ volatile }
{ nounwind }
{ noreturn }
{ norecurse }
{ tail }
{ inline }
{ speculatable }
{ associative }
{ atomic { seqcst } }
{ seqcst }
{ release }
{ acquire }
```

```json
"attribute-block": {
    "begin": "\\{\\s*",
    "end": "\\}",
    "name": "meta.attribute.phir",
    "patterns": [
        { "include": "#attribute-name" },
        { "include": "#attribute-block" }
    ]
},
"attribute-name": {
    "match": "\\b(commutative|associative|readonly|volatile|nounwind|noreturn|norecurse|tail|inline|speculatable|atomic|seqcst|release|acquire|unordered|relaxed|monotonic)\\b",
    "name": "entity.name.attr.phir"
}
```

#### 3.3.19 函数属性

```phir
fun fast(noinline) -> void { ... }
fun cold() -> void { ... }
fun noreturn() -> void { ... }
```

```json
"function-attribute": {
    "match": "\\b(noinline|cold|noreturn|nosideeffects|noalias|nobuiltin|always_inline)\\b",
    "name": "entity.name.attr.phir"
}
```

#### 3.3.20 调用约定

```phir
fun add(a: i32, b: i32) -> i32 system_v
fun fast_func(a: i32, b: i32) -> i32 fast
```

```json
"calling-convention": {
    "match": "\\b(system_v|fast|cold|windows_fastcall|probestack|apple_aarch64|winch)\\b",
    "name": "entity.name.calling-convention.phir"
}
```

#### 3.3.21 所有权修饰符

```phir
owned(ptr)
borrowed(ptr)
guaranteed(ptr)
copy(ptr)
```

```json
"ownership-modifier": {
    "match": "\\b(owned|borrowed|guaranteed|copy)\\b",
    "name": "storage.modifier.ownership.phir"
}
```

#### 3.3.22 类型系统

```phir
void, bool, i1, i8, i16, i32, i64, i128, f32, f64,
ptr, ptr<T>, ref<T>, ref mut<T>,
string, struct<T>, list<T>, map<K,V>,
vector<N, T>, owned<T>, borrowed<T>, guaranteed<T>,
(T) -> R
```

```json
"type-annotation": {
    "match": "(?<![:?])(:)(?!\\s*[:?])\\s*([\\w?]+(?:<[^>]+>)?(?:\\s*->\\s*[\\w?]+(?:<[^>]+>)?)?)",
    "captures": {
        "1": { "name": "punctuation.separator.type-annotation.phir" },
        "2": { "name": "entity.name.type.phir" }
    }
}
```

> **注意**：PHIR 的类型标注语法与 Aura 类似，但需要额外处理泛型、函数类型、所有权类型。

```json
"builtin-types": {
    "match": "\\b(void|bool|i1|i8|i16|i32|i64|i128|f32|f64|ptr|string|vector)\\b",
    "name": "storage.type.builtin.phir"
}
```

#### 3.3.23 值引用

```phir
v0, v1, v2         — SSA 局部值
ss0, ss1           — 栈槽
%v1                 — 局部引用
@str.hello          — 全局引用
this                — 自我引用
vmctx               — VM 上下文
```

```json
"local-reference": {
    "match": "\\b(v\\d+)\\b",
    "name": "variable.local.phir"
},
"stack-slot-reference": {
    "match": "\\b(ss\\d+)\\b",
    "name": "variable.stack-slot.phir"
},
"global-reference": {
    "match": "@([\\w.]+)",
    "captures": {
        "1": { "name": "entity.name.global.phir" }
    }
},
"local-copy-reference": {
    "match": "%([\\w]+)",
    "captures": {
        "1": { "name": "variable.local.phir" }
    }
},
"vmctx": {
    "match": "\\bvmctx\\b",
    "name": "variable.language.vmctx.phir"
},
"self-reference": {
    "match": "\\b(this)\\b",
    "name": "variable.language.this.phir"
}
```

#### 3.3.24 调试变量

```phir
debug x => v5
debug y => v2
```

```json
"debug-variable": {
    "match": "(\\b\\w+\\b)(\\s*=>\\s*)(v\\d+)",
    "captures": {
        "1": { "name": "variable.debug-var.phir" },
        "2": { "name": "keyword.operator.assignment.phir" },
        "3": { "name": "variable.local.phir" }
    }
}
```

#### 3.3.25 作用域

```phir
scope 1 {
    debug x => v1;
    debug y => v2;
}
```

```json
"scope-block": {
    "begin": "\\b(scope)\\s+(\\d+)(\\s*\\{)",
    "beginCaptures": {
        "1": { "name": "keyword.statement.phir" },
        "2": { "name": "variable.scope-id.phir" },
        "3": { "name": "punctuation.section.scope.begin.phir" }
    },
    "end": "(\\})",
    "endCaptures": {
        "1": { "name": "punctuation.section.scope.end.phir" }
    },
    "patterns": [
        { "include": "#statement-debug" },
        { "include": "#comments" }
    ]
}
```

#### 3.3.26 字符串

```phir
@str.hello = "hello world"
```

```json
"string": {
    "begin": "\"",
    "end": "\"",
    "name": "string.quoted.double.phir",
    "patterns": [
        {
            "match": "\\\\.",
            "name": "constant.character.escape.phir"
        }
    ]
}
```

#### 3.3.27 数值字面量

```phir
const i32 42
const f64 0.0
0x01
-1
NaN
```

```json
"numeric-literal": {
    "patterns": [
        { "include": "#hex-literal" },
        { "include": "#decimal-literal" }
    ]
},
"hex-literal": {
    "match": "0(x|X)[A-Fa-f0-9][A-Fa-f0-9_]*",
    "name": "constant.numeric.hex.phir"
},
"decimal-literal": {
    "match": "\\b\\d[\\d_]*(\\.[\\d_]+)?((e|E)\\d+)?(u|U)?(L|F|f|D|d)?\\b",
    "name": "constant.numeric.phir"
},
"boolean-literal": {
    "match": "\\b(true|false)\\b",
    "name": "constant.language.boolean.phir"
},
"nan-literal": {
    "match": "\\bNaN\\b",
    "name": "constant.language.nan.phir"
}
```

#### 3.3.28 运算符

```phir
+  -  *  /  %  &  |  ^  <<  >>  >>>
==  !=  <  >  <=  >=  &&  ||  is  as
!  -  +  *  &  ~  ~=  &mut  &
=>  ->
```

```json
"operators": {
    "patterns": [
        { "include": "#comparison-operators" },
        { "include": "#assignment-operators" },
        { "include": "#arithmetic-operators" },
        { "include": "#logical-operators" },
        { "include": "#bitwise-operators" },
        { "include": "#other-operators" }
    ]
},
"comparison-operators": {
    "match": "(==|!=|<=|>=|<|>)",
    "name": "keyword.operator.comparison.phir"
},
"assignment-operators": {
    "match": "(=)",
    "name": "keyword.operator.assignment.phir"
},
"arithmetic-operators": {
    "match": "([+*/%-])",
    "name": "keyword.operator.arithmetic.phir"
},
"logical-operators": {
    "match": "(&&|\\|\\|)",
    "name": "keyword.operator.logical.phir"
},
"bitwise-operators": {
    "match": "(&|\\||\\^|~|<<|>>>|>>)",
    "name": "keyword.operator.bitwise.phir"
},
"other-operators": {
    "patterns": [
        {
            "match": "=>",
            "name": "keyword.operator.assignment.phir"
        },
        {
            "match": "->",
            "name": "keyword.operator.assignment.phir"
        },
        {
            "match": "([=!])",
            "name": "keyword.operator.negation.phir"
        },
        {
            "match": "\\b(is|as)\\b",
            "name": "keyword.operator.comparison.phir"
        }
    ]
}
```

#### 3.3.29 Place（左值）表达式

```phir
v1              # 局部变量
ss0             # 栈槽
v1.field        # 字段访问
v1[0]           # 下标访问
*v2             # 解引用
this            # 自我
```

```json
"place": {
    "patterns": [
        { "include": "#field-access" },
        { "include": "#index-access" },
        { "include": "#deref" },
        { "include": "#local-reference" },
        { "include": "#stack-slot-reference" },
        { "include": "#self-reference" }
    ]
},
"field-access": {
    "match": "(\\b\\w+\\b)(\\.)((?:\\w+))",
    "captures": {
        "1": { "name": "variable.local.phir" },
        "2": { "name": "punctuation.separator.period.phir" },
        "3": { "name": "variable.other.property.phir" }
    }
},
"index-access": {
    "match": "(\\b\\w+\\b)(\\[)([^\\]]+)(\\])",
    "captures": {
        "1": { "name": "variable.local.phir" },
        "2": { "name": "punctuation.section.index.begin.phir" },
        "3": { "name": "constant.numeric.phir" },
        "4": { "name": "punctuation.section.index.end.phir" }
    }
},
"deref": {
    "match": "(\\*)(v\\d+)",
    "captures": {
        "1": { "name": "keyword.operator.bitwise.phir" },
        "2": { "name": "variable.local.phir" }
    }
}
```

#### 3.3.30 #code 主容器

```json
"code": {
    "patterns": [
        { "include": "#comments" },
        { "include": "#module-header" },
        { "include": "#globals" },
        { "include": "#native-function-declaration" },
        { "include": "#function-declaration" },
        { "include": "#dialect-declaration" },
        { "include": "#statement-assignment" },
        { "include": "#statement-store" },
        { "include": "#statement-load" },
        { "include": "#statement-debug" },
        { "include": "#statement-def" },
        { "include": "#terminator" },
        { "include": "#scope-block" },
        { "include": "#indent-scope" },
        { "include": "#basic-block" },
        { "include": "#preamble-stack-slot" },
        { "include": "#preamble-fn-ref" },
        { "include": "#preamble-sig" },
        { "include": "#preamble-global-value" },
        { "include": "#attribute-block" },
        { "include": "#function-attribute" },
        { "include": "#calling-convention" },
        { "include": "#ownership-modifier" },
        { "include": "#instruction-opcode" },
        { "include": "#dialect-instruction" },
        { "include": "#operand" },
        { "include": "#local-reference" },
        { "include": "#stack-slot-reference" },
        { "include": "#global-reference" },
        { "include": "#local-copy-reference" },
        { "include": "#self-reference" },
        { "include": "#vmctx" },
        { "include": "#builtin-types" },
        { "include": "#type-annotation" },
        { "include": "#field-access" },
        { "include": "#index-access" },
        { "include": "#deref" },
        { "include": "#operators" },
        { "include": "#string" },
        { "include": "#hex-literal" },
        { "include": "#decimal-literal" },
        { "include": "#boolean-literal" },
        { "include": "#nan-literal" },
        { "include": "#return-arrow" },
        { "include": "#debug-variable" },
        { "include": "#exception-landing" },
        { "match": ",", "name": "punctuation.separator.delimiter.phir" },
        { "match": ";", "name": "punctuation.terminator.statement.phir" },
        { "match": "\\.\\b", "name": "punctuation.separator.period.phir" }
    ]
}
```

### 3.4 规则匹配顺序说明

tmLanguage 的规则按 `patterns` 数组顺序匹配，先匹配的规则优先。PHIR 的规则顺序需要仔细考虑：

```
1.  #comments           — 注释最优先（# 号不会被误匹配）
2.  #module-header      — 模块头特殊处理
3.  #globals            — @global 全局声明
4.  #native-function-declaration — native fun 声明
5.  #function-declaration        — fun 定义（begin/end 捕获整个函数体）
6.  #basic-block        — bb 基本块（begin/end 捕获块体）
7.  #scope-block        — scope 调试作用域
8.  #indent-scope       — if/else/while 缩进语法
9.  #preamble-*         — 函数前言元素
10. #statement-*        — 语句
11. #terminator         — 终止符
12. #attribute-block    — { } 属性块
13. #function-attribute — 函数属性关键字
14. #calling-convention — 调用约定关键字
15. #ownership-modifier — 所有权修饰符
16. #instruction-opcode — 指令操作码
17. #dialect-instruction — 方言指令
18. #operand            — 操作数
19. #local-reference    — v0, v1...
20. #stack-slot-reference — ss0, ss1...
21. #global-reference   — @global
22. #local-copy-reference — %v0
23. #self-reference     — this
24. #vmctx              — vmctx
25. #builtin-types      — 内置类型
26. #type-annotation    — : Type 标注
27. #field-access       — .field
28. #index-access       — [index]
29. #deref              — *v0
30. #operators          — 运算符
31. #string             — 字符串
32. #numeric-literal    — 数值
33. #boolean-literal    — true/false
34. #nan-literal        — NaN
35. #return-arrow       — ->
36. #debug-variable     — debug x => v0
37. #exception-landing  — [return:, unwind:]
38. 标点符号            — , ; .
```

> **关键注意事项**：
> - `#function-declaration` 放在 `#basic-block` 之前，因为 `fun` 开始时会用 `begin`/`end` 捕获整个函数体，内部的 BB 在函数体范围内匹配
> - `#basic-block` 在 `#statement-*` 之前，因为 BB 的 `begin` 先匹配
> - `#operators` 在 `#builtin-types` 之后，避免 `+`/`-` 被误判
> - `#dialect-instruction` 在 `#instruction-opcode` 之后，避免 `load` 先被匹配为普通操作码

### 3.5 颜色定制方案

#### 3.5.1 PHIR 专用颜色方案

```json
"configurationDefaults": {
    "[phir]": {
        "editor.tokenColorCustomizations": {
            "textMateRules": [
                {
                    "scope": "storage.type.native.phir",
                    "settings": { "foreground": "#C586C0" }
                },
                {
                    "scope": "storage.type.function.phir",
                    "settings": { "foreground": "#569CD6" }
                },
                {
                    "scope": "entity.name.function.phir",
                    "settings": { "foreground": "#DCDCAA" }
                },
                {
                    "scope": "entity.name.global.phir",
                    "settings": { "foreground": "#4EC9B0" }
                },
                {
                    "scope": "entity.name.block.phir",
                    "settings": { "foreground": "#4FC1FF" }
                },
                {
                    "scope": "variable.stack-slot.phir",
                    "settings": { "foreground": "#D7BA7D" }
                },
                {
                    "scope": "variable.local.phir",
                    "settings": { "foreground": "#9CDCFE" }
                },
                {
                    "scope": "variable.parameter.phir",
                    "settings": { "foreground": "#CE9178" }
                },
                {
                    "scope": "keyword.terminator.phir",
                    "settings": { "foreground": "#C586C0" }
                },
                {
                    "scope": "keyword.control.phir",
                    "settings": { "foreground": "#C586C0" }
                },
                {
                    "scope": "keyword.dialect.phir",
                    "settings": { "foreground": "#4EC9B0" }
                },
                {
                    "scope": "support.instruction.phir",
                    "settings": { "foreground": "#DCDCAA" }
                },
                {
                    "scope": "entity.name.attr.phir",
                    "settings": { "foreground": "#BC83FF" }
                },
                {
                    "scope": "entity.name.calling-convention.phir",
                    "settings": { "foreground": "#BC83FF" }
                },
                {
                    "scope": "storage.modifier.ownership.phir",
                    "settings": { "foreground": "#4EC9B0" }
                },
                {
                    "scope": "entity.name.operand.const.phir",
                    "settings": { "foreground": "#B5CEA8" }
                },
                {
                    "scope": "variable.language.this.phir, variable.language.vmctx.phir",
                    "settings": { "foreground": "#569CD6" }
                },
                {
                    "scope": "variable.debug-var.phir",
                    "settings": { "foreground": "#B5CEA8" }
                },
                {
                    "scope": "variable.scope-id.phir",
                    "settings": { "foreground": "#4EC9B0" }
                },
                {
                    "scope": "keyword.operator.assignment.phir",
                    "settings": { "foreground": "#D4D4D4" }
                },
                {
                    "scope": "comment.line.number-sign.phir",
                    "settings": { "foreground": "#6A9955" }
                },
                {
                    "scope": "string.quoted.double.phir",
                    "settings": { "foreground": "#CE9178" }
                },
                {
                    "scope": "constant.numeric.phir, constant.numeric.hex.phir",
                    "settings": { "foreground": "#B5CEA8" }
                },
                {
                    "scope": "constant.language.boolean.phir, constant.language.nan.phir",
                    "settings": { "foreground": "#569CD6" }
                }
            ]
        }
    }
}
```

#### 3.5.2 配色逻辑说明

| PHIR 元素 | 颜色 | 选择理由 |
|-----------|------|---------|
| `native` / `return` / `br` | `#C586C0` (紫粉) | 控制流关键字，同 Aura 的 `extern` |
| `fun` / `this` / `vmctx` | `#569CD6` (蓝) | 声明/引用关键字，同 Aura 的 `package` |
| `@global` | `#4EC9B0` (青绿) | 全局实体，同 Aura 的 `support.function.std` |
| `bb name` | `#4FC1FF` (亮蓝) | 控制流目标，同 Aura 的 `enum.variant` |
| `ss0` | `#D7BA7D` (黄) | 栈槽，特殊内存位置，同 Aura 的 `elvis/null-assert` |
| `v0` | `#9CDCFE` (浅蓝) | SSA 值，同 Aura 的变量 |
| 参数名 | `#CE9178` (橙) | 参数，同 Aura |
| 函数名 | `#DCDCAA` (黄) | 函数，标准 VSCode 函数色 |
| 方言前缀 | `#4EC9B0` (青绿) | 方言标识，全局实体色 |
| 指令操作码 | `#DCDCAA` (黄) | 操作码，同函数色 |
| 属性/调用约定 | `#BC83FF` (紫) | 元属性，同 Aura 的注解 |
| 所有权 | `#4EC9B0` (青绿) | 所有权修饰，全局实体色 |
| 注释 | `#6A9955` (绿) | 注释标准色 |
| 字符串 | `#CE9178` (橙) | 字符串标准色 |
| 数值 | `#B5CEA8` (浅绿) | 常量标准色 |
| 布尔/NaN | `#569CD6` (蓝) | 语言内建常量 |

### 3.6 language-configuration.json 更新

PHIR 的编辑行为配置：

```json
{
    "comments": {
        "lineComment": "#"
    },
    "brackets": [
        ["{", "}"],
        ["[", "]"],
        ["(", ")"],
        ["<", ">"]
    ],
    "autoClosingPairs": [
        { "open": "{", "close": "}" },
        { "open": "[", "close": "]" },
        { "open": "(", "close": ")" },
        { "open": "<", "close": ">", "notIn": ["comment"] },
        { "open": "\"", "close": "\"", "notIn": ["string"] },
        { "open": "{", "close": "}", "notIn": ["string"] }
    ],
    "surroundingPairs": [
        ["{", "}"],
        ["[", "]"],
        ["(", ")"],
        ["<", ">"],
        ["\"", "\""]
    ],
    "indentationRules": {
        "increaseIndentPattern": "^\\s*(\\{)$",
        "decreaseIndentPattern": "^\\s*\\}"
    },
    "folding": {
        "languageFolding": [
            {
                "start": "^\\s*fun\\s+\\w+.*\\{",
                "end": "^\\s*}",
                "startsAtFirstColumn": true,
                "nestable": true,
                "exclusive": false,
                "expansion": "none",
                "collapsed": true,
                "comment": "fold function bodies"
            },
            {
                "start": "^\\s*bb\\s+\\w+",
                "end": "^\\s*(bb\\s+\\w+|\\})",
                "startsAtFirstColumn": true,
                "nestable": true,
                "exclusive": true,
                "expansion": "none",
                "collapsed": false,
                "comment": "fold basic blocks"
            },
            {
                "start": "^\\s*scope\\s+\\d+\\s*\\{",
                "end": "^\\s*}",
                "startsAtFirstColumn": true,
                "nestable": true,
                "exclusive": false,
                "expansion": "none",
                "collapsed": true,
                "comment": "fold debug scopes"
            }
        ]
    },
    "wordPattern": "(-?\\d+(?:\\.\\d+)?|[a-zA-Z_$][\\w$]*)",
    "onEnterRules": [
        {
            "beforeText": "^\\s*#.*$",
            "action": { "indent": "indent", "removeTrailingWhitespace": true }
        }
    ]
}
```

### 3.7 PHIR 代码片段 (snippets/phir.json)

```json
{
    "// 模块头": {
        "prefix": "phirmod",
        "body": [
            "# module ${1:module_name} target ${2:x86_64-pc-windows-msvc} flags 0x${3:01}",
            "# source ${4:source.aura}",
            "",
            "$0"
        ],
        "description": "PHIR 模块头"
    },
    "// 全局常量": {
        "prefix": "gstr",
        "body": [
            "@str.${1:name} = \"${2:value}\""
        ],
        "description": "全局字符串常量"
    },
    "// 全局变量": {
        "prefix": "gvar",
        "body": [
            "@${1:name} = ${2:type} ${3:default}"
        ],
        "description": "全局变量"
    },
    "// 原生函数声明": {
        "prefix": "nfun",
        "body": [
            "native fun ${1:name}(${2:params}) -> ${3:return_type}"
        ],
        "description": "原生函数声明"
    },
    "// 函数定义": {
        "prefix": "fun",
        "body": [
            "fun ${1:name}(${2:params}) -> ${3:return_type} {",
            "    $0",
            "}"
        ],
        "description": "函数定义"
    },
    "// 函数 + 栈槽": {
        "prefix": "funss",
        "body": [
            "fun ${1:name}(${2:params}) -> ${3:return_type} {",
            "    ss0 = ${4:i32} ${5:4}",
            "    $0",
            "}"
        ],
        "description": "带栈槽的函数"
    },
    "// 基本块": {
        "prefix": "bb",
        "body": [
            "bb ${1:entry}(${2:params}):",
            "    $0",
            "    return ${3:result}"
        ],
        "description": "基本块"
    },
    "// 栈槽声明": {
        "prefix": "ss",
        "body": [
            "ss${1:0} = ${2:i32} ${3:4}"
        ],
        "description": "栈槽声明"
    },
    "// 函数引用": {
        "prefix": "fnref",
        "body": [
            "fn${1:0} = @${2:func_name}(${3:params}) -> ${4:return_type}"
        ],
        "description": "函数引用"
    },
    "// 签名声明": {
        "prefix": "sig",
        "body": [
            "sig${1:0} = (${2:params}) -> ${3:return_type}"
        ],
        "description": "签名声明"
    },
    "// 全局值": {
        "prefix": "gv",
        "body": [
            "gv${1:0} = @${2:global_name}"
        ],
        "description": "全局值引用"
    },
    "// 栈加载": {
        "prefix": "ld",
        "body": [
            "v${1:0} = load.${2:i32} ${3:ss0}"
        ],
        "description": "栈加载"
    },
    "// 栈存储": {
        "prefix": "st",
        "body": [
            "store ${1:i32} ${2:v0}, ${3:ss0}"
        ],
        "description": "栈存储"
    },
    "// 常量定义": {
        "prefix": "const",
        "body": [
            "v${1:0} = const ${2:i32} ${3:0}"
        ],
        "description": "常量定义"
    },
    "// 二元运算": {
        "prefix": "op",
        "body": [
            "v${1:0} = ${2:add} ${3:v0}, ${4:v1}"
        ],
        "description": "二元运算"
    },
    "// 条件比较": {
        "prefix": "icmp",
        "body": [
            "v${1:0} = icmp ${2:ult} ${3:i32} ${4:v0}, ${5:v1}"
        ],
        "description": "整数比较"
    },
    "// 无条件跳转": {
        "prefix": "br",
        "body": [
            "br ${1:block_name}"
        ],
        "description": "无条件跳转"
    },
    "// 条件跳转": {
        "prefix": "brif",
        "body": [
            "br if ${1:v0}, ${2:block_true}, ${3:block_false}"
        ],
        "description": "条件跳转"
    },
    "// 函数调用": {
        "prefix": "call",
        "body": [
            "v${1:0} = call @${2:func_name}(${3:params}) -> ${4:return_type}"
        ],
        "description": "函数调用"
    },
    "// 异常调用": {
        "prefix": "invoke",
        "body": [
            "v${1:0} = call @${2:func_name}(${3:params}) -> ${4:return_type}",
            "      [return: ${5:ok_block}, unwind: ${6:catch_block}]"
        ],
        "description": "异常调用 (invoke)"
    },
    "// 返回": {
        "prefix": "ret",
        "body": [
            "return ${1:v0}"
        ],
        "description": "返回"
    },
    "// 不可达": {
        "prefix": "unreach",
        "body": [
            "unreachable"
        ],
        "description": "不可达"
    },
    "// 属性块": {
        "prefix": "attr",
        "body": [
            "{ ${1:readonly} }"
        ],
        "description": "指令属性块"
    },
    "// 所有权": {
        "prefix": "own",
        "body": [
            "owned(${1:ptr})"
        ],
        "description": "所有权修饰"
    },
    "// 缩进 if": {
        "prefix": "if",
        "body": [
            "if ${1:condition} {",
            "    $0",
            "}"
        ],
        "description": "缩进语法 if"
    },
    "// 缩进 else": {
        "prefix": "else",
        "body": [
            "} else {",
            "    $0"
        ],
        "description": "缩进语法 else"
    },
    "// 缩进 while": {
        "prefix": "while",
        "body": [
            "while ${1:condition} {",
            "    $0",
            "}"
        ],
        "description": "缩进语法 while"
    },
    "// 调试作用域": {
        "prefix": "scope",
        "body": [
            "scope ${1:1} {",
            "    debug ${2:var_name} => ${3:v0};",
            "    $0",
            "}"
        ],
        "description": "调试作用域"
    },
    "// 调试映射": {
        "prefix": "dbg",
        "body": [
            "debug ${1:var_name} => ${2:v0};"
        ],
        "description": "调试变量映射"
    },
    "// main 函数": {
        "prefix": "main",
        "body": [
            "fun main() -> i32 {",
            "    bb entry:",
            "        v0 = const i32 0",
            "        return v0",
            "}"
        ],
        "description": "PHIR main 函数模板"
    },
    "// 函数 + BB 参数循环": {
        "prefix": "funloop",
        "body": [
            "fun ${1:name}(${2:count: i32}) -> ${3:i32} {",
            "    bb entry(v0: ${2:i32}):",
            "        v1 = icmp sgt ${2:i32} v0, ${4:0}",
            "        br if v1, ${5:loop}, ${6:done}",
            "",
            "    bb ${5:loop}:",
            "        $0",
            "        br ${6:done}",
            "",
            "    bb ${6:done}:",
            "        return v0"
        ],
        "description": "带循环的函数模板"
    }
}
```

### 3.8 package.json 更新

#### 3.8.1 新增语言注册

```json
{
    "contributes": {
        "languages": [
            {
                "id": "phir",
                "aliases": ["Photon IR", "PHIR", "phir"],
                "extensions": [".phir"],
                "configuration": "./language-configuration.json",
                "icon": {
                    "light": "./resources/phir-icon-light.png",
                    "dark": "./resources/phir-icon-dark.png"
                }
            }
        ],
        "grammars": [
            {
                "language": "phir",
                "scopeName": "source.phir",
                "path": "./syntaxes/phir.tmLanguage.json"
            }
        ],
        "snippets": [
            {
                "language": "phir",
                "path": "./snippets/phir.json"
            }
        ]
    }
}
```

#### 3.8.2 新增颜色定制

在 `configurationDefaults` 中添加 `"[phir]"` 块（见 §3.5.1）。

### 3.9 图标资源

需要创建：
- `resources/phir-icon.png`
- `resources/phir-icon-light.png`
- `resources/phir-icon-dark.png`

**建议**：使用与 Aura 图标风格一致的原子/光子主题图标（蓝色系）。

---

## 4. 实现注意事项与风险

### 4.1 技术风险

| 风险 | 描述 | 缓解措施 |
|------|------|---------|
| **缩进 vs 花括号冲突** | PHIR 混用缩进（BB、if/else）和花括号（函数体、scope） | 分两个规则集：函数体用 `begin`/`end` 捕获 `{}`，BB 体用 `begin`/`end` 捕获 `bb ... :` 到下一 `bb` |
| **`bb` 块结束检测** | BB 块的结束不是显式标记，而是下一个 `bb` 或 `}` | 使用 lookahead `(?=\\bb\\b|\\}|$)` 作为 `end` 模式 |
| **方言指令识别** | `load.f32` 中 `load` 可能被误匹配为普通关键字 | `#dialect-instruction` 规则应优先于 `#instruction-opcode` |
| **`const` 关键字双重语义** | `const` 既是 operand 修饰符也是指令操作码 | 通过上下文区分：`v0 = const i32 42` 中 `const` 属于 operand |
| **类型标注中的泛型** | `ptr<T>`、`map<K,V>` 的尖括号需要正确解析 | 使用 `(?:<[^>]+>)?` 模式，不嵌套泛型 |
| **异常落地跨行** | `[return: ok, unwind: catch]` 可能换行 | 使用 `begin`/`end` 匹配 `\[` 到 `\]` |

### 4.2 已知限制

| 限制 | 说明 | 影响 |
|------|------|------|
| **无嵌套泛型** | `map<K, map<K,V>>` 中内层 `>` 会误结束外层 | 可接受，PHIR 实际使用很少 |
| **BB 参数类型标注** | `bb entry(v0: ptr, v1: i64):` 中参数列表可能复杂 | 用 `#parameter` 复用规则处理 |
| **缩进语法中的嵌套块** | `if a > b { if c > d { ... } }` | 使用 `begin`/`end` 匹配 `{` 到 `}` |
| **字符串中的转义** | PHIR 字符串支持 `\n`, `\t`, `\"` | 在 `#string` 规则中添加 escape pattern |
| **数值后缀** | `u32`, `i64`, `f32` 等类型标注与数值后缀混淆 | 上下文区分：数值后跟 `)` 或 `,` 时是后缀 |

### 4.3 性能考量

| 因素 | 评估 | 优化建议 |
|------|------|---------|
| **规则数量** | ~50-60 条规则 | 可接受，tmLanguage 通常 50-200 条 |
| **`begin`/`end` 模式** | 约 10 个 | 每个嵌套结构增加匹配开销，但 PHIR 嵌套深度低 |
| **长正则表达式** | `#instruction-opcode` 等包含多个选项 | 可接受，VSCode 使用 Oniguruma 引擎 |
| **文件大** | 典型 `.phir` 文件 < 1000 行 | 无性能问题 |

---

## 5. 实现路线

### Phase 1: 基础语法 (MVP)

- [ ] `phir.tmLanguage.json` — 顶层结构 + 注释 + 全局声明 + 函数定义 + 基本块 + 语句 + 终止符
- [ ] `snippets/phir.json` — 基础片段（模块头、函数、BB、常用指令）
- [ ] `package.json` — 语言注册 + 基础颜色

### Phase 2: 高级特性

- [ ] 指令属性 `{ }` 块 + 嵌套属性
- [ ] 方言指令（`load.f32`、`atom.load` 等）
- [ ] 所有权修饰符（`owned`、`borrowed` 等）
- [ ] 调用约定（`system_v`、`fast` 等）
- [ ] 函数属性（`noinline`、`cold` 等）
- [ ] 异常落地 `[return:, unwind:]`
- [ ] 缩进语法（`if`/`else`/`while`）

### Phase 3: 完善与优化

- [ ] `language-configuration.json` — PHIR 编辑行为
- [ ] `phir-icon.png` — 图标资源
- [ ] 更多代码片段（循环、所有权、属性等）
- [ ] 性能测试 + 正则优化
- [ ] 视觉测试（各种 PHIR 示例文件）

### Phase 4: 集成测试

- [ ] 与现有 Aura 语法共存验证
- [ ] 测试文件验证（`hello.phir`、`fib.phir`、`average.phir`）
- [ ] 颜色方案微调

---

## 6. 附录

### 6.1 PHIR 语法元素速查表

| 元素 | 示例 | 对应规则 | Scope |
|------|------|---------|-------|
| 模块头 | `# module hello target ...` | `#module-header` | `meta.module-header.phir` |
| 全局常量 | `@str.hello = "..."` | `#globals` | `entity.name.global.phir` |
| 全局变量 | `@counter = i32 0` | `#globals` | `entity.name.global.phir` |
| 原生函数 | `native fun write(...)` | `#native-function-declaration` | `storage.type.native.phir` |
| 函数定义 | `fun strlen(...) -> i64 {` | `#function-declaration` | `entity.name.function.phir` |
| 栈槽 | `ss0 = f64 8` | `#preamble-stack-slot` | `variable.stack-slot.phir` |
| 函数引用 | `fn0 = @strlen(...)` | `#preamble-fn-ref` | `entity.name.parameter.phir` |
| 签名 | `sig0 = (i32, i32) -> i32` | `#preamble-sig` | `entity.name.parameter.phir` |
| 全局值 | `gv0 = @str.hello` | `#preamble-global-value` | `entity.name.parameter.phir` |
| 基本块 | `bb entry(v0: ptr):` | `#basic-block` | `entity.name.block.phir` |
| 赋值 | `v2 = const i64 42` | `#statement-assignment` | `variable.local.phir` |
| 栈存储 | `store i32 v3, ss0` | `#statement-store` | `keyword.statement.phir` |
| 栈加载 | `v4 = load.i32 ss1` | `#dialect-instruction` | `keyword.dialect.phir` |
| 调试 | `debug x => v5` | `#statement-debug` | `variable.debug-var.phir` |
| 返回 | `return v1` | `#terminator-return` | `keyword.terminator.phir` |
| 无条件跳转 | `br block1` | `#terminator-br` | `keyword.terminator.phir` |
| 条件跳转 | `br if v2, b1, b2` | `#terminator-br` | `keyword.terminator.phir` |
| 调用 | `v3 = call fn0(v1, v2)` | `#terminator-call` | `keyword.terminator.phir` |
| 异常落地 | `[return: ok, unwind: catch]` | `#exception-landing` | `meta.exception-landing.phir` |
| 缩进 if | `if a > b { ... }` | `#indent-if` | `keyword.control.phir` |
| 缩进 else | `else { ... }` | `#indent-else` | `keyword.control.phir` |
| 缩进 while | `while m != 0 { ... }` | `#indent-while` | `keyword.control.phir` |
| 属性块 | `{ commutative }` | `#attribute-block` | `entity.name.attr.phir` |
| 函数属性 | `noinline` | `#function-attribute` | `entity.name.attr.phir` |
| 调用约定 | `system_v` | `#calling-convention` | `entity.name.calling-convention.phir` |
| 所有权 | `owned(ptr)` | `#ownership-modifier` | `storage.modifier.ownership.phir` |
| 指令操作码 | `add`, `sub`, `icmp` | `#instruction-opcode` | `support.instruction.phir` |
| 方言指令 | `load.f32` | `#dialect-instruction` | `keyword.dialect.phir` |
| 常量操作数 | `const i32 42` | `#operand` | `entity.name.operand.const.phir` |
| SSA 值 | `v0`, `v1` | `#local-reference` | `variable.local.phir` |
| 栈槽引用 | `ss0` | `#stack-slot-reference` | `variable.stack-slot.phir` |
| 局部引用 | `%v1` | `#local-copy-reference` | `variable.local.phir` |
| 全局引用 | `@str.hello` | `#global-reference` | `entity.name.global.phir` |
| 自我 | `this` | `#self-reference` | `variable.language.this.phir` |
| VM 上下文 | `vmctx` | `#vmctx` | `variable.language.vmctx.phir` |
| 内置类型 | `i32`, `f64`, `ptr` | `#builtin-types` | `storage.type.builtin.phir` |
| 类型标注 | `: i32` | `#type-annotation` | `entity.name.type.phir` |
| 字符串 | `"hello"` | `#string` | `string.quoted.double.phir` |
| 十六进制 | `0x01` | `#hex-literal` | `constant.numeric.hex.phir` |
| 十进制 | `42`, `0.0` | `#decimal-literal` | `constant.numeric.phir` |
| 布尔 | `true`, `false` | `#boolean-literal` | `constant.language.boolean.phir` |
| NaN | `NaN` | `#nan-literal` | `constant.language.nan.phir` |
| 运算符 | `+`, `==`, `->` | `#operators` | `keyword.operator.*.phir` |
| 字段访问 | `v1.field` | `#field-access` | `variable.other.property.phir` |
| 下标 | `v1[0]` | `#index-access` | `constant.numeric.phir` |
| 解引用 | `*v2` | `#deref` | `keyword.operator.bitwise.phir` |

### 6.2 与 Aura 高亮对比

| 特性 | Aura | PHIR |
|------|------|------|
| 注释风格 | `//` 和 `/* */` | `#` |
| 块结构 | 花括号 `{}` | 缩进 + 花括号混用 |
| 变量命名 | 自然变量名 | SSA `v0` + 栈槽 `ss0` |
| 函数调用 | `fn0(args)` | `call fn0(args) -> type` |
| 类型系统 | 类/结构体 | SSA + BB 参数 |
| 属性系统 | `@annotation` | `{ attribute }` |
| 方言 | 无 | 前缀指令 |
| 所有权 | `val`/`var` | `owned`/`borrowed` |
| 高亮规则数 | ~40 条 | ~50-60 条 |
| 颜色方案 | 独立 | 独立（可复用部分颜色） |
| 缩进规则 | 复杂（花括号 + 关键字） | 简单（花括号 + BB） |

---

## 7. 总结

本方案为 PHIR 文本格式设计了完整的 TextMate 语法高亮方案，包含：

1. **语法规则** (`phir.tmLanguage.json`)：~50-60 条规则，覆盖所有 PHIR 语法元素
2. **颜色定制** (`package.json`)：22 条 `textMateRules`，针对 PHIR 特定元素着色
3. **语言配置** (`language-configuration.json`)：注释、括号、缩进、折叠
4. **代码片段** (`snippets/phir.json`)：~30 个常用 PHIR 代码模板

**设计原则**：
- 遵循 Aura 插件的 tmLanguage.json 结构模式（`repository` + `include`）
- Scope 命名统一使用 `.phir` 后缀
- 颜色方案与 Aura 保持视觉一致（VSCode Dark+ 兼容）
- 分阶段实现，Phase 1 即可使用

**下一步**：确认方案后，开始实现 `phir.tmLanguage.json` 的 Phase 1 版本。
