# Aura 代码验证工具规格

## 概述

创建一个 DSH 工具 `aura-check`，让 agent 可以验证生成的 Aura 代码。
支持三种验证模式，按可用工具自动降级。

---

## 模式 A：完整编译验证（需 aura CLI）

### 前置条件
- `aura` CLI 可执行文件在 PATH 中或指定路径

### 工具命令

```bash
# 语法检查
aura check <file.aura>
# 输出: exit code 0 = 通过, 非0 = 有错误
# stdout: 诊断信息（行号、列号、错误消息）

# 格式化检查
aura fmt --check <file.aura>
# 输出: exit code 0 = 格式正确, 非0 = 需要格式化

# 编译
aura build <file.aura>
# 输出: exit code 0 = 编译成功, 非0 = 编译失败
```

### DSH 工具接口

```typescript
// 伪代码
async function auraCheck(filePath: string): Promise<CheckResult> {
    const result = await exec(`aura check "${filePath}"`);
    return {
        ok: result.exitCode === 0,
        errors: parseDiagnostics(result.stdout),
        warnings: parseWarnings(result.stdout)
    };
}
```

---

## 模式 B：轻量语法验证（无 aura CLI）

### 验证规则（基于 TextMate 语法提炼）

#### B.1 括号匹配
- 所有 `{` 必须有配对的 `}`
- 所有 `(` 必须有配对的 `)`
- 所有 `[` 必须有配对的 `]`
- 字符串内的括号不计入

#### B.2 声明完整性
- `fun` 声明必须包含 `fun name(params): ReturnType` 或 `fun name(params) { ... }`
- `struct` 声明必须包含 `struct Name(val/var field: Type)` 或 `struct Name { ... }`
- `enum` 声明必须包含 `enum Name { ... }`
- `interface` 声明必须包含 `interface Name { ... }`
- `actor` 声明必须包含 `actor Name { ... }`
- `class` 声明必须包含 `class Name : Super { ... }`

#### B.3 关键字位置
- `val` / `var` 后必须跟变量名和 `:` 或 `=`
- `return` 后可跟表达式
- `if` / `while` / `for` / `when` / `try` 后必须有 `{`
- `suspend` / `inline` / `comptime` 必须在 `fun` 之前

#### B.4 类型标注
- 变量声明 `val x: Type` 必须包含冒号
- 函数返回类型 `fun f(): Type` 必须包含冒号
- 泛型类型参数必须在 `<...>` 中

#### B.5 字符串
- 双引号字符串必须闭合
- 三引号多行字符串必须闭合
- 插值 `${...}` 中的括号必须匹配

#### B.6 常见错误检测
| 检测项 | 规则 | 错误消息 |
|--------|------|---------|
| `data class` | 检测到 `data class` | "Aura 用 `data struct`，不是 `data class`" |
| `fun` 缺返回类型 | `fun name()` 无 `:` 且非块体 | "函数声明缺少返回类型" |
| `extern` 无引号 | `extern fun` 无 `"c"` | "FFI 声明需要语言标记：`extern \"c\"`" |
| `Result` 单参数 | `Result<T>` 单类型参数 | "Aura 的 Result 需要两个类型参数：`Result<T, E>`" |
| 缺分号 | 语句末尾无 `;`（非块体） | "建议语句末尾加分号" |
| `await` 作函数 | `await(x)` | "`await` 是关键字，不是函数：`await x`" |

### 实现方式

```
输入: .aura 文件内容（字符串）
处理: 正则匹配 + 括号栈 + 状态机
输出: 诊断列表（行号、列号、严重程度、消息、建议）
```

---

## 模式 C：格式化验证

### 检查项
- 缩进是否为 4 空格
- 函数/声明间是否有空行
- 运算符前后是否有空格
- 逗号后是否有空格
- 块体花括号是否与上一行同行（Kotlin 风格）

---

## 工具输出格式

```json
{
  "file": "main.aura",
  "ok": false,
  "errors": [
    {
      "line": 5,
      "column": 10,
      "severity": "error",
      "message": "Expected ':' in function return type",
      "hint": "fun add(a: Int, b: Int): Int"
    }
  ],
  "warnings": [
    {
      "line": 3,
      "column": 1,
      "severity": "warning",
      "message": "Consider adding documentation comment"
    }
  ],
  "suggestions": [
    {
      "from": "data class",
      "to": "data struct",
      "message": "Aura uses `data struct`, not Kotlin's `data class`"
    }
  ]
}
```

---

## DSH 工具实现建议

### 方案 1：Node.js 脚本（推荐，无需外部依赖）
```
tools/aura-check.mjs  → 纯 JS 实现模式 B 验证
```

### 方案 2：调用 aura CLI（如果有编译器）
```
pwsh: aura check file.aura → 解析输出
```

### 方案 3：调用 LSP（如果有 LSP 服务器）
```
LSP 协议 → textDocument/diagnostic → 解析诊断
```

### 方案 4：集成到 VS Code 扩展（最完整）
```
复用 VS Code 扩展的 LSP 客户端
```

---

## 错误级别

| 级别 | 含义 | 行为 |
|------|------|------|
| `error` | 语法错误 | 必须修正 |
| `warning` | 可能有问题 | 建议修正 |
| `info` | 改进建议 | 可选修正 |
| `hint` | 修复提示 | 自动替换建议 |
