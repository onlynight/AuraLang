# Aura 脚本模式技术方案

> **目标**：让 Aura 像 Python/Node 一样，无需 `main` 函数即可直接执行脚本代码。

---

## 一、问题分析

### 1.1 当前限制

```rust
// emit.rs:49
let entry = fn_index.get("main").copied().unwrap_or(0);

// vm/mod.rs:526-527
if entry >= self.module.funcs.len() {
    return Err(VmError::NoEntry);
}
```

| 场景 | 当前行为 | 期望行为 |
|------|----------|----------|
| 有 `main` 函数 | ✅ 正常执行 | ✅ 正常执行（兼容） |
| 无 `main` 有顶层语句 | ❌ 报错 `no entry function` | ✅ 自动包装执行 |
| 无 `main` 无顶层语句 | ❌ 报错 | ⚠️ 警告提示 |
| 无 `main` 有函数无语句 | ⚠️ 执行第一个函数 | ⚠️ 执行第一个函数（兼容） |

### 1.2 影响示例

| 文件 | 状态 | 原因 |
|------|------|------|
| `std_demo.aura` | ❌ 失败 | 只有顶层语句，无函数 |
| `std_demo2.aura` | ❌ 失败 | 只有顶层语句，无函数 |
| `showcase.aura` | ❌ 失败 | 只有声明，无 main |
| `demo.aura` | ❌ 失败 | 有函数无 main，entry 指向错误 |

---

## 二、设计目标

| 目标 | 说明 | 优先级 |
|------|------|--------|
| **脚本模式** | 无 `main` 时，顶层语句自动包装为隐式 `main` | P0 |
| **兼容模式** | 有 `main` 时，行为不变（向后兼容） | P0 |
| **终端执行** | `aura run file.aura` 直接执行，类似 Python/Node | P0 |
| **REPL 模式** | `aura repl` 交互式执行 | P1 |
| **模块系统** | `import` 顶层语句支持 | P2 |
| **shebang** | `#!/usr/bin/env aura` 直接执行 | P2 |

---

## 三、核心架构

### 3.1 处理流程

```
┌─────────────────────────────────────────────────────────────────┐
│                        源码解析层                                 │
├─────────────────────────────────────────────────────────────────┤
│  源码 → Lexer → Parser → AST                                     │
└──────────────────────────────────┬──────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                      脚本模式检测                                 │
├─────────────────────────────────────────────────────────────────┤
│  检查 AST:                                                        │
│  ├─ 有 fun main → 跳过（兼容模式）                               │
│  ├─ 有顶层语句 → 合成隐式 main                                   │
│  └─ 无顶层语句 → 报错/警告                                        │
└──────────────────────────────────┬──────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                      HIR 构建层                                   │
├─────────────────────────────────────────────────────────────────┤
│  AST → HIR                                                       │
│  ├─ 函数声明 → HirFunction                                       │
│  ├─ 顶层语句 → top_level_statements（新增）                      │
│  └─ synthesize_main_if_missing()（新增）                         │
└──────────────────────────────────┬──────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                      字节码发射层                                 │
├─────────────────────────────────────────────────────────────────┤
│  HIR → MIR → Bytecode                                            │
│  ├─ entry = fn_index["main"]（兼容模式）                          │
│  ├─ entry = 0（脚本模式，合成的 main 在第一个位置）               │
│  └─ entry = 0（无 main 有函数，兼容行为）                        │
└──────────────────────────────────┬──────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────┐
│                        VM 执行层                                  │
├─────────────────────────────────────────────────────────────────┤
│  加载 BytecodeModule → 执行 entry 函数                            │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 数据流对比

**兼容模式（有 main）：**
```
fun main() { println("hi") }
    ↓
AST: [Decl::Fun(main)]
    ↓
HIR: functions=[main]
    ↓
emit: entry = fn_index["main"] = 0
    ↓
VM: 执行 main
```

**脚本模式（无 main）：**
```
println("hi")
val x = 42
println(x)
    ↓
AST: [Stmt::Expr(println), Stmt::Val(x), Stmt::Expr(println)]
    ↓
HIR: top_level_statements=[...], functions=[]
    ↓
synthesize_main_if_missing: functions=[main(body=[...])]
    ↓
emit: entry = fn_index["main"] = 0
    ↓
VM: 执行合成的 main
```

---

## 四、详细设计

### 4.1 HIR 扩展（Phase 1）

#### 4.1.1 新增顶层语句字段

```rust
// compiler/src/codegen/hir.rs

pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub top_level_statements: Option<HirBlock>,  // 新增：顶层语句块
    pub imports: Vec<String>,
    pub constants: Vec<(String, ConstValue)>,
    // ... 其他字段
}
```

#### 4.1.2 顶层语句收集

在 `build_hir` 中收集顶层语句：

```rust
fn build_hir(program: &Program) -> HirProgram {
    let mut top_level = HirBlock::default();
    
    for stmt in &program.body {
        match stmt {
            // 顶层表达式语句（如 println("hi")）
            ast::Stmt::Expr(expr, span) => {
                let hir_expr = lower_expr_to_hir(expr);
                top_level.stmts.push(HirStmt::Expr(hir_expr));
            }
            // 顶层变量声明（如 val x = 42）
            ast::Stmt::Val(decl) => {
                let hir_decl = lower_val_decl(decl);
                top_level.stmts.push(HirStmt::Val(hir_decl));
            }
            // 顶层函数/结构体声明 → 继续处理为函数
            ast::Stmt::Decl(decl) => {
                // 函数声明加入 functions
                // 结构体声明加入 types
                // ...
            }
            // 其他语句类型
            _ => {}
        }
    }
    
    HirProgram {
        functions,
        top_level_statements: if top_level.stmts.is_empty() {
            None
        } else {
            Some(top_level)
        },
        // ...
    }
}
```

### 4.2 隐式 Main 合成（Phase 2）

#### 4.2.1 合成函数

```rust
/// 若程序没有 main 函数，合成一个
/// 将顶层语句包装为隐式 main 函数体
/// 
/// 返回：true 表示合成了隐式 main，false 表示已有 main 或无顶层语句
pub fn synthesize_main_if_missing(hir: &mut HirProgram) -> bool {
    // 1. 检查是否已有 main 函数
    if hir.functions.iter().any(|f| f.name == "main") {
        return false; // 已有 main，不处理（兼容模式）
    }
    
    // 2. 获取顶层语句
    let body = hir.top_level_statements.take();
    
    match body {
        Some(block) if !block.stmts.is_empty() => {
            // 3. 合成 main 函数
            let main_func = HirFunction {
                name: "main".into(),
                params: vec![],
                return_type: Some(HirType::Int),
                body: Some(block),
                type_params: vec![],
                type_args: vec![],
                is_generic: false,
                visibility: ast::Visibility::Public,
                modifiers: vec![],
                doc: None,
                span: Span::dummy(),
            };
            
            // 4. 插入到 functions 开头（确保 entry=0 指向 main）
            hir.functions.insert(0, main_func);
            true
        }
        _ => {
            // 无顶层语句，无需合成
            false
        }
    }
}
```

#### 4.2.2 合成示例

**输入：**
```aura
println("Hello, World!")
val x = 42
println("x = " + x.toString())
```

**AST：**
```
Program {
    body: [
        Stmt::Expr(CallExpr(println, ["Hello, World!"])),
        Stmt::Val(ValDecl(x, Int, CallExpr(ToString, [42]))),
        Stmt::Expr(CallExpr(println, [AddExpr(...)])),
    ]
}
```

**HIR（合成前）：**
```
HirProgram {
    functions: [],
    top_level_statements: Some(HirBlock {
        stmts: [
            HirStmt::Expr(CallExpr(...)),
            HirStmt::Val(VarDecl(x, ...)),
            HirStmt::Expr(CallExpr(...)),
        ]
    }),
    // ...
}
```

**HIR（合成后）：**
```
HirProgram {
    functions: [
        HirFunction {
            name: "main",
            params: [],
            return_type: Some(Int),
            body: Some(HirBlock {
                stmts: [
                    HirStmt::Expr(CallExpr(...)),
                    HirStmt::Val(VarDecl(x, ...)),
                    HirStmt::Expr(CallExpr(...)),
                ]
            }),
            // ...
        }
    ],
    top_level_statements: None,
    // ...
}
```

### 4.3 Entry 逻辑修改（Phase 3）

#### 4.3.1 emit.rs 修改

```rust
// 修改前
let entry = fn_index.get("main").copied().unwrap_or(0);

// 修改后
let entry = if let Some(idx) = fn_index.get("main") {
    *idx  // 兼容模式：有 main 函数
} else if !functions.is_empty() {
    0  // 兼容行为：无 main 但有函数，执行第一个
} else {
    // 无任何函数，报错
    return Err(CodeGenError::NoEntry(
        "no entry function found (no main, no top-level statements)".into()
    ));
};
```

#### 4.3.2 mod.rs 集成

```rust
pub fn compile(program: &Program, opts: &CodeGenOptions) -> Result<BytecodeModule, String> {
    // 1. 构建 HIR
    let mut hir = build_hir(program);
    
    // 2. 脚本模式：合成隐式 main（新增）
    let _synthesized = synthesize_main_if_missing(&mut hir);
    
    // 3. 后续流程不变
    let mut mir_funcs = lower_program(&hir)?;
    // ...
    
    Ok(emit_module(&hir, &mir_funcs))
}
```

### 4.4 VM 错误处理优化（Phase 3）

#### 4.4.1 错误信息改进

```rust
// vm/mod.rs
pub enum VmError {
    // 修改前
    NoEntry,
    
    // 修改后：更详细的错误信息
    NoEntry(String),  // 携带原因
}

// 在 run() 中
if entry >= self.module.funcs.len() {
    return Err(VmError::NoEntry(format!(
        "no entry function (module has {} functions, entry={}, max={})",
        self.module.funcs.len(),
        entry,
        self.module.funcs.len().saturating_sub(1)
    )));
}
```

---

## 五、扩展功能设计

### 5.1 REPL 模式（Phase 4）

#### 5.1.1 CLI 新增子命令

```bash
aura repl              # 进入交互式 REPL
aura repl file.aura    # 加载文件后进入 REPL
```

#### 5.1.2 REPL 实现架构

```rust
// cli/src/repl.rs

pub struct Repl {
    vm: Vm,
    module: BytecodeModule,
    history: Vec<String>,
}

impl Repl {
    pub fn new() -> Self {
        // 初始化空模块
        let module = BytecodeModule::empty();
        let vm = Vm::new(module);
        Self { vm, module, history: vec![] }
    }
    
    pub fn run_loop(&mut self) -> io::Result<()> {
        println!("Aura REPL v0.1.0");
        println!("Type :help for help, :quit to exit");
        
        loop {
            print!(">>> ");
            io::stdout().flush()?;
            
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            let line = line.trim().to_string();
            
            if line.is_empty() {
                continue;
            }
            
            if line.starts_with(':') {
                self.handle_command(&line)?;
            } else {
                self.eval_line(&line)?;
            }
        }
    }
    
    /// 求值单行代码
    fn eval_line(&mut self, line: &str) -> io::Result<()> {
        // 1. 包装为临时函数
        let source = format!("fun __repl__(): Any {{\n    {}\n}}", line);
        
        // 2. 编译
        match compile_source(&source) {
            Ok(module) => {
                // 3. 执行
                let mut vm = Vm::new(module);
                let result = vm.run();
                
                match result {
                    Ok(value) => {
                        if value != Value::Null {
                            println!("{}", value);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Syntax error: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// 处理 :command
    fn handle_command(&mut self, cmd: &str) -> io::Result<()> {
        match cmd {
            ":quit" | ":q" => {
                println!("Bye!");
                process::exit(0);
            }
            ":help" | ":h" => {
                println!("Commands:");
                println!("  :help  :h   Show this help");
                println!("  :quit  :q   Exit REPL");
                println!("  :clear       Clear screen");
                println!("  :history     Show history");
                println!("  :load <file> Load file");
                println!("  :reset       Reset VM state");
            }
            ":clear" => {
                print!("\x1b[2J\x1b[H");
            }
            ":history" => {
                for (i, h) in self.history.iter().enumerate() {
                    println!("{:>3}: {}", i + 1, h);
                }
            }
            ":load" if cmd.len() > 6 => {
                let path = &cmd[6..].trim();
                self.load_file(path)?;
            }
            ":reset" => {
                self.reset();
                println!("VM state reset");
            }
            _ => {
                eprintln!("Unknown command: {}", cmd);
                eprintln!("Type :help for available commands");
            }
        }
        Ok(())
    }
    
    fn load_file(&mut self, path: &str) -> io::Result<()> {
        let source = fs::read_to_string(path)?;
        self.eval_source(&source)?;
        println!("Loaded: {}", path);
        Ok(())
    }
    
    fn reset(&mut self) {
        // 重置 VM 状态
        self.history.clear();
    }
}
```

#### 5.1.3 REPL 特性

| 特性 | 说明 | 实现状态 |
|------|------|----------|
| **表达式求值** | 输入表达式立即返回结果 | ✅ 基础实现 |
| **变量持久化** | 变量在多次输入间保持 | ⚠️ 需扩展 VM |
| **历史记录** | 上下键切换历史 | ⚠️ 需 readline |
| **多行输入** | 支持函数定义等多行 | ⚠️ 需扩展解析器 |
| **Tab 补全** | 自动补全标识符 | 🔜 后续实现 |
| **`%` 变量** | `_` 存储上次结果（类似 Ruby） | 🔜 后续实现 |

### 5.2 模块系统（Phase 5）

#### 5.2.1 当前 import 语法

```aura
import aura.io.*           // 通配引入
import aura.math as m      // 别名引入
import aura.math.sin       // 精确引入
import aura.math           // 模块引用
```

#### 5.2.2 扩展：顶层语句 import

```aura
// 文件 a.aura
fun add(a: Int, b: Int): Int = a + b
val pi = 3.14

// 文件 b.aura
import "./a.aura"          // 导入本地文件
import "aura.math.*"       // 导入内置模块

println(add(1, 2))        // 调用导入的函数
println(pi)               // 使用导入的常量
```

#### 5.2.3 模块解析流程

```
import "./a.aura"
    ↓
1. 解析导入路径
2. 编译 a.aura → a.auc
3. 加载 a.auc 到 VM
4. 将 a.auc 的函数/变量合并到当前模块
    ↓
符号表合并
├─ functions: [add, ...]
├─ variables: [pi, ...]
└─ types: [...]
```

### 5.3 shebang 支持（Phase 5）

#### 5.3.1 语法

```aura
#!/usr/bin/env aura
// 或
#!/usr/bin/aura
```

#### 5.3.2 实现

```rust
// lexer.rs - 跳过 shebang 行
fn skip_shebang(source: &str) -> &str {
    if source.starts_with("#!") {
        if let Some(newline) = source.find('\n') {
            return &source[newline + 1..];
        }
        return "";
    }
    source
}
```

#### 5.3.3 执行权限

```bash
chmod +x script.aura
./script.aura          # 直接执行
aura run script.aura   # 通过 aura 执行
```

### 5.4 内置命令（Phase 6）

#### 5.4.1 CLI 新增命令

```bash
aura eval "code"       # 直接执行代码字符串
aura eval -f file.aura  # 执行文件
aura -e "code"         # 简写：直接执行
```

#### 5.4.2 示例

```bash
# 执行单行代码
aura eval "println(1 + 1)"
# 输出: 2

# 执行多行代码
aura eval "
val x = 42
println(x)
"
# 输出: 42

# 管道输入
echo "println(42)" | aura eval
# 输出: 42
```

---

## 六、影响范围分析

### 6.1 代码改动

| 文件 | 改动类型 | 行数估算 | 风险 |
|------|----------|----------|------|
| `codegen/hir.rs` | 新增字段 + 函数 | ~100 行 | 低 |
| `codegen/emit.rs` | 修改 entry 逻辑 | ~10 行 | 低 |
| `codegen/mod.rs` | 集成合成调用 | ~5 行 | 低 |
| `vm/mod.rs` | 错误信息优化 | ~5 行 | 低 |
| `cli/src/repl.rs` | 新增 REPL | ~200 行 | 中 |
| `cli/src/main.rs` | 新增子命令 | ~20 行 | 低 |
| `lexer.rs` | shebang 支持 | ~10 行 | 低 |

**总计：~350 行新增代码**

### 6.2 向后兼容性

| 场景 | 当前行为 | 新行为 | 兼容？ |
|------|----------|--------|--------|
| 有 `main` 函数 | 执行 main | 执行 main | ✅ 完全兼容 |
| 无 `main` 有函数 | 执行第一个函数 | 执行第一个函数 | ✅ 完全兼容 |
| 无 `main` 有顶层语句 | 报错 | 自动执行 | ⚠️ 行为变更（修复） |
| 无 `main` 无顶层语句 | 报错 | 报错（改进信息） | ✅ 完全兼容 |

### 6.3 测试覆盖

| 测试类型 | 用例数 | 说明 |
|----------|--------|------|
| 单元测试 | 10 | HIR 合成、entry 逻辑 |
| 集成测试 | 15 | 脚本执行、REPL |
| 回归测试 | 20 | 现有示例文件 |
| **总计** | **45** | |

---

## 七、实施路线图

### Phase 1：核心功能（3 天）

| 任务 | 工期 | 依赖 |
|------|------|------|
| 扩展 HIR 支持顶层语句 | 1 天 | 无 |
| 实现 `synthesize_main_if_missing` | 0.5 天 | Phase 1.1 |
| 修改 emit entry 逻辑 | 0.5 天 | Phase 1.1 |
| 集成到 compile 流程 | 0.5 天 | Phase 1.2, 1.3 |
| 测试现有示例文件 | 0.5 天 | Phase 1.4 |

### Phase 2：REPL 模式（2 天）

| 任务 | 工期 | 依赖 |
|------|------|------|
| 实现 Repl 核心结构 | 0.5 天 | Phase 1 |
| 实现表达式求值 | 0.5 天 | Phase 2.1 |
| 实现命令系统 | 0.5 天 | Phase 2.1 |
| CLI 集成 | 0.5 天 | Phase 2.3 |

### Phase 3：扩展功能（3 天）

| 任务 | 工期 | 依赖 |
|------|------|------|
| 模块系统（import 文件） | 1 天 | Phase 1 |
| shebang 支持 | 0.5 天 | 无 |
| `aura eval` 命令 | 0.5 天 | Phase 1 |
| 文档更新 | 1 天 | Phase 1-3 |

### Phase 4：优化（2 天）

| 任务 | 工期 | 依赖 |
|------|------|------|
| REPL 变量持久化 | 1 天 | Phase 2 |
| Tab 补全 | 1 天 | Phase 2 |

### 总工期：10 天

---

## 八、验收标准

### 8.1 功能验收

| 场景 | 命令 | 期望输出 |
|------|------|----------|
| 脚本模式 | `aura run demo.aura` | 输出 `Score: 45` |
| 兼容模式 | `aura run calc.aura` | 输出计算结果 |
| REPL 模式 | `aura repl` | 显示 `>>> ` 提示符 |
| REPL 求值 | `>>> 1 + 1` | 输出 `2` |
| eval 命令 | `aura eval "1+1"` | 输出 `2` |
| shebang | `./script.aura` | 直接执行 |

### 8.2 测试验收

```bash
# 单元测试
cargo test --workspace

# 集成测试
cargo test --features "llvm,std-all"

# 示例文件
for f in examples/*.aura; do
    aura run "$f" || echo "FAILED: $f"
done
```

### 8.3 性能验收

| 指标 | 目标 | 说明 |
|------|------|------|
| 脚本启动时间 | < 50ms | 编译 + VM 初始化 |
| REPL 响应时间 | < 10ms | 单行求值 |
| 内存占用 | < 10MB | 基础运行 |

---

## 九、风险与应对

| 风险 | 影响 | 概率 | 应对措施 |
|------|------|------|----------|
| HIR 扩展破坏现有代码 | 编译失败 | 低 | 充分的回归测试 |
| 顶层语句语义不清 | 类型推断错误 | 中 | 限制顶层语句类型 |
| REPL 变量持久化复杂 | 实现困难 | 中 | 分阶段实现，先求值后持久 |
| 模块系统循环依赖 | 运行时错误 | 低 | 检测并报错 |

---

## 十、参考

- [Python 交互式解释器](https://docs.python.org/3/tutorial/interpreter.html)
- [Node.js REPL](https://nodejs.org/api/repl.html)
- [Lua Interactive Shell](https://www.lua.org/manual/5.4/manual.html#6.1)
- [Rust Scripting](https://github.com/leynos/rust-script)

---

## 附录：示例文件修复清单

| 文件 | 当前状态 | 修复方式 |
|------|----------|----------|
| `std_demo.aura` | ❌ 无 main | 脚本模式自动执行 |
| `std_demo2.aura` | ❌ 无 main | 脚本模式自动执行 |
| `showcase.aura` | ❌ 无 main | 脚本模式自动执行 |
| `demo.aura` | ❌ 无 main | 脚本模式自动执行 |
| `demo_errors.aura` | ❌ 故意错误 | 保持错误（测试用） |
| `raylib_demo.aura` | ❌ 需要库 | 需要 Raylib 安装 |

---

**文档版本：v1.0**
**创建日期：2026-06-24**
**作者：SenseNova**
