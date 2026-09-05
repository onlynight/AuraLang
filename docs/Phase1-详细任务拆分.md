# Phase 1 详细任务拆分（按开发阶段）

> **前置文档**：`docs/游戏领域发展路线规划.md` §5  
> **目标**：让 Lambda/Enum/await/ask 在游戏场景中真正可用  
> **代码核实日期**：2026-09  
> **总预估**：~73 人天，6 个阶段  

---

## 目录

- [阶段总览](#阶段总览)
- [阶段 1：基础设施修复](#阶段-1基础设施修复)
- [阶段 2：闭包核心](#阶段-2闭包核心)
- [阶段 3：类型系统扩展](#阶段-3类型系统扩展)
- [阶段 4：并发语义修正](#阶段-4并发语义修正)
- [阶段 5：AOT 后端补全](#阶段-5aot-后端补全)
- [阶段 6：集成与演示](#阶段-6集成与演示)
- [测试策略总览](#测试策略总览)
- [风险清单](#风险清单)
- [完成定义（DoD）](#完成定义dod)
- [附录 A：关键文件修改清单](#附录-a关键文件修改清单)
- [附录 B：版本规划](#附录-b版本规划)

---

## 阶段总览

```
阶段 1 ──→ 阶段 2 ──→ 阶段 3 ──→ 阶段 4 ──→ 阶段 5 ──→ 阶段 6
  │           │           │           │           │           │
基础设施     闭包核心     类型扩展     并发语义     AOT 补全    集成演示
修复         (Lambda)    (Enum/Fn)   (ask/select) (AOT 闭包)  (Raylib/Demo)
 ~12d        ~21d        ~10d        ~9d         ~8d         ~13d
```

| 阶段 | 名称 | 核心任务 | 预估 | 依赖 | 交付物 |
|------|------|---------|------|------|--------|
| **1** | 基础设施修复 ✅ | await 语义、emit_call 验证、C ABI 蹦床、静态链接 | ~12d | 无 | 编译期语义正确、FFI 可靠 |
| **2** | 闭包核心 ✅ | Lambda MIR→捕获→结构→字节码→VM | ~21d | 阶段 1 | 闭包在 VM/JIT 可用 |
| **3** | 类型系统扩展 ✅ | Enum MIR/VM、Function 类型 MIR/VM | ~10d | 阶段 2 | 枚举+函数指针在 VM 可用 |
| **4** | 并发语义修正 ✅ | ask 真阻塞、select 事件驱动 | ~9d | 阶段 1 | Actor 通信语义正确 |
| **5** | AOT 后端补全 ✅ | AOT-LLVM 闭包、AOT-C 闭包 | ~8d | 阶段 2,3 | 闭包在 AOT 可用 |
| **6** | 集成与演示 🔄 | Raylib 绑定、测试套件、Demo | ~13d | 阶段 1-5 | game_2d_demo 端到端运行 |

> **2026-09 进度更新**：
> - 阶段 1-3 已完成，89 个测试全部通过
> - 阶段 4：ask 真阻塞语义已实现（PendingRequest + response_queue），select 事件驱动已实现（selectTimeout + 通道 ID 返回）
> - 阶段 5：AOT-LLVM 闭包捕获支持已实现（自由变量收集 + 捕获参数），AOT-C 闭包支持已实现（静态函数 + 函数指针）
> - 阶段 6：game_2d_demo.aura 已创建，Lambda 调用已修复（CallClosure + Return 指令）
> - 预存缺陷：结构体构造器、顶层 val 声明、字符串插值等仍需后续修复

### 阶段依赖图

```
阶段 1（独立）──┬──→ 阶段 2（Lambda）──┬──→ 阶段 3（Enum/Fn）──┐
                 │                       │                       │
                 ├──→ 阶段 4（并发）      ├──→ 阶段 5（AOT）      ├──→ 阶段 6（集成）
                 │                       │                       │
                 │    阶段 1 也可并行到    │    阶段 3,5 可并行     │
                 │    阶段 2,4            │                        │
                 └────────────────────────────────────────────────┘
```

---

## 阶段 1：基础设施修复 ✅ 已完成

> **完成日期**：2026-09  
> **实际耗时**：~2 人天（4 个子任务中 3 个修复已预存在，1 个 await 语义修正新增实现）  
> **测试验证**：143 个测试通过，0 失败  

### 1.1 阶段目标

修复 4 个独立的阻塞性缺陷，为后续阶段奠定基础。这些任务**互不依赖**，可并行执行。

### 1.2 阶段范围

| 任务 | 内容 | 预估 |
|------|------|------|
| 1.9 | await 编译期语义修正 | 2d |
| 1.12 | AOT-LLVM emit_call 返回值验证 | 2d |
| 1.14 | C ABI 蹦床可变参数 | 3d |
| 1.15 | 静态链接类型安全调用 | 5d |

### 1.3 阶段依赖

- **前置**：无（可立即开始）
- **后续**：阶段 2、4 依赖本阶段完成

### 1.4 任务 1.9：await 编译期语义修正

#### 1.4.1 问题描述

当前 `sema/checker.rs:962`：
```rust
Expr::Await { expr, .. } => self.check_expr(expr),
```

无论当前函数是否为 `suspend`/`async`，`await` 都静默通过。非 suspend 函数中的 `await` 在 MIR/AOT 层被降级为"直接返回内部值"（`mir.rs:592-595`、`aot/emit.rs:894-897`），产生**静默语义错误**。

#### 1.4.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.9.1 | 在 checker 中追踪当前函数的 suspend 状态 | `sema/checker.rs` | 0.5d |
| 1.9.2 | 非 suspend 函数中的 `await` 报错 | `sema/checker.rs` | 0.5d |
| 1.9.3 | 嵌套 suspend 调用检查 | `sema/checker.rs` | 0.5d |
| 1.9.4 | 测试 | `tests/sema_tests.rs` | 0.5d |

#### 1.4.3 技术设计

```rust
// sema/checker.rs — 新增字段
pub struct Checker {
    // ... 现有字段
    pub(crate) is_in_suspend_fn: bool,  // 新增
}

impl Checker {
    fn check_function(&mut self, f: &FnDecl) {
        let was_suspend = self.is_in_suspend_fn;
        self.is_in_suspend_fn = f.modifiers.iter()
            .any(|m| matches!(m, FnModifier::Suspend | FnModifier::Async));
        // ... 检查函数体
        self.is_in_suspend_fn = was_suspend;
    }
}
```

```rust
// 非 suspend 函数中的 await 报错
Expr::Await { expr, span, .. } => {
    let et = self.check_expr(expr);
    if !self.is_in_suspend_fn {
        self.errors.push(CompileError::new(
            "await can only be used in suspend/async functions",
            *span,
        ));
    }
    et
}
```

#### 1.4.4 测试用例

```rust
#[test]
fn test_await_in_non_suspend_function_is_error() {
    let src = "fun main() { await foo() } suspend fun foo(): Int = 1";
    let errors = check_source(src);
    assert!(errors.iter().any(|e| e.message.contains("await")));
}

#[test]
fn test_await_in_suspend_function_ok() {
    let src = "suspend fun main() { await foo() } suspend fun foo(): Int = 1";
    let errors = check_source(src);
    assert!(errors.is_empty());
}
```

#### 1.4.5 验收标准

- ✅ `await` 在非 suspend 函数中报编译错误
- ✅ `await` 在 suspend/async 函数中正常通过
- ✅ 调用 suspend 函数时调用者必须是 suspend

---

### 1.5 任务 1.12：AOT-LLVM emit_call 验证

#### 1.5.1 问题描述

`docs/遗留问题与风险分析报告.md` #14/#32 声称 `emit_call` 返回值硬编码 `i32`。开发规划文档说"2026-09 已修复"。需要**验证修复是否真实**。

#### 1.5.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.12.1 | 检查 `emit_call` 当前实现 | `aot/emit.rs:1039` | 0.5d |
| 1.12.2 | 编写测试用例（void/Int/Float/Ptr 返回值） | `tests/aot_return_type_tests.rs` | 1d |
| 1.12.3 | 运行测试验证 | — | 0.5d |

#### 1.5.3 测试用例

```rust
#[test]
fn test_aot_void_return() {
    // extern "c" { fun foo(): Unit }
    // 验证不生成 `tmp = call void`
}

#[test]
fn test_aot_ptr_return() {
    // extern "c" { fun malloc(size: Int): Pointer<Byte> }
    // 验证生成 `ptr` 返回值
}

#[test]
fn test_aot_float_return() {
    // extern "c" { fun sqrt(x: Float): Float }
    // 验证生成 `f32` 返回值
}
```

#### 1.5.4 验收标准

- ✅ 所有返回类型（void/Int/Float/Ptr/String）正确生成
- ✅ 无 `i32` vs `ptr` 类型不匹配错误

---

### 1.6 任务 1.14：C ABI 蹦床可变参数

#### 1.6.1 问题描述

`ffi.rs:111-117` 蹦床硬编码 4 参数，超过的截断。

#### 1.6.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.14.1 | 设计可变参数蹦床 | — | 0.5d |
| 1.14.2 | 实现 `aura_callback_trampoline_varargs` | `vm/ffi.rs` | 1d |
| 1.14.3 | 更新回调注册 | `vm/ffi.rs` | 0.5d |
| 1.14.4 | 测试 | `tests/ffi_varargs_tests.rs` | 1d |

#### 1.6.3 技术设计

```c
// C ABI 蹦床：可变参数
extern "C" {
    void aura_callback_trampoline_varargs(void* context, ...);
}
```

通过 `context` 参数传递回调 ID，查全局注册表获取参数类型签名，按签名转换参数后派发回 Aura VM。

#### 1.6.4 验收标准

- ✅ 5+ 参数回调正确传递
- ✅ 向后兼容 4 参数蹦床

---

### 1.7 任务 1.15：静态链接类型安全调用

#### 1.7.1 问题描述

`ffi.rs:148` 所有参数强制转 `i64`，f64/指针/结构体参数依赖平台 ABI 巧合。

#### 1.7.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.15.1 | 新增 `CFuncInfo` 结构（类型签名） | `vm/ffi.rs` | 1d |
| 1.15.2 | 基于类型的参数转换 | `vm/ffi.rs` | 2d |
| 1.15.3 | 更新 FFI 调用路径 | `vm/interp.rs` | 1d |
| 1.15.4 | 测试 | `tests/ffi_type_safe_tests.rs` | 1d |

#### 1.7.3 技术设计

```rust
// vm/ffi.rs — 新增
pub struct CFuncInfo {
    pub name: String,
    pub params: Vec<CParamType>,
    pub return_type: CType,
}

pub enum CType {
    I32, I64, F32, F64, Bool, Void, Pointer,
}
```

基于 `CFuncInfo` 的类型签名，在调用前将 Aura `Value` 转换为正确的 C ABI 表示（f64 → 浮点寄存器、指针 → 指针寄存器、结构体 → 内存传递）。

#### 1.7.4 验收标准

- ✅ f64 参数正确传递
- ✅ 指针参数正确传递
- ✅ 结构体参数（小结构体）正确传递

---

### 1.8 阶段 1 交付物

| 交付物 | 说明 |
|--------|------|
| 编译期 await 语义正确 | 非 suspend 函数中的 await 报错 |
| AOT-LLVM emit_call 验证 | 所有返回类型正确生成 |
| C ABI 蹦床可变参数 | 5+ 参数回调支持 |
| 静态链接类型安全 | f64/指针/结构体参数正确 |
| 测试套件 | `tests/sema_tests.rs`、`tests/aot_return_type_tests.rs`、`tests/ffi_varargs_tests.rs`、`tests/ffi_type_safe_tests.rs` |

### 1.9 阶段 1 验收标准

- ✅ `cargo test --workspace` 全部通过
- ✅ await 语义测试通过
- ✅ emit_call 验证测试通过
- ✅ C ABI 蹦床测试通过
- ✅ 静态链接类型安全测试通过

### 1.10 阶段 1 风险

| 风险 | 概率 | 缓解 |
|------|------|------|
| await 误报 | 低 | 充分测试 |
| emit_call 未真正修复 | 中 | 先验证，不通过则修复 |
| C ABI 蹦床 ABI 不匹配 | 中 | 跨平台测试 |
| 静态链接类型转换错误 | 中 | 类型签名测试 |

---

## 阶段 2：闭包核心

### 2.1 阶段目标

实现完整的 Lambda/闭包链路，让闭包在 VM/JIT 模式下可用。这是**游戏回调的核心**，也是本阶段最重要的工作。

### 2.2 阶段范围

| 任务 | 内容 | 预估 | 依赖 |
|------|------|------|------|
| 1.1 | MIR 层 Lambda 降级 | 5d | 阶段 1 |
| 1.2 | Lambda 捕获分析 | 5d | 1.1 |
| 1.3 | Lambda 捕获结构体生成 | 3d | 1.2 |
| 1.4 | 字节码 MakeClosure opcode | 3d | 1.3 |
| 1.5 | VM ClosureObj 运行时 | 5d | 1.4 |

### 2.3 阶段依赖

- **前置**：阶段 1 完成（或可并行，但建议阶段 1 先完成）
- **后续**：阶段 3（Function 类型）、阶段 5（AOT 闭包）依赖本阶段

### 2.4 任务 1.1：MIR 层 Lambda 降级

#### 2.4.1 问题描述

当前 `mir.rs:597-600`：
```rust
// Fix 4: Lambda — MIR 层暂不支持，返回空注册器
HirExpr::Lambda { .. } => {
    self.alloc_reg()
}
```

Lambda 在 MIR 层被**完全丢弃**，仅分配一个空寄存器。

#### 2.4.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.1.1 | `MirInstr` 新增 `MakeClosure` 变体 | `codegen/mir.rs` | 0.5d |
| 1.1.2 | `MirFunction` 新增 `closures` 字段 | `codegen/mir.rs` | 0.5d |
| 1.1.3 | `MirBuilder` 新增 `lower_lambda` 方法 | `codegen/mir.rs` | 2d |
| 1.1.4 | Lambda 参数声明为局部变量 | `codegen/mir.rs` | 0.5d |
| 1.1.5 | Lambda body 降级为子 MIR 函数 | `codegen/mir.rs` | 1d |
| 1.1.6 | 测试 | `tests/mir_lambda_tests.rs` | 0.5d |

#### 2.4.3 技术设计

```rust
// codegen/mir.rs — MirInstr 新增
pub enum MirInstr {
    // ... 现有
    /// 创建闭包：dst = closure(func_name, captures...)
    MakeClosure {
        dst: Reg,
        func: String,
        captures: Vec<Reg>,
    },
}

// codegen/mir.rs — MirFunction 新增
pub struct MirFunction {
    // ... 现有
    pub closures: Vec<MirClosure>,
}

pub struct MirClosure {
    pub name: String,
    pub params: Vec<String>,
    pub reg_count: usize,
    pub captures: Vec<Reg>,
    pub body: MirBlock,
}
```

```rust
// lower_lambda 方法
impl MirBuilder {
    fn lower_lambda(&mut self, lambda: &HirLambda, ctx: &mut LowerCtx) -> Reg {
        let closure_name = format!("__lambda_{}", ctx.next_closure_id);
        ctx.next_closure_id += 1;
        
        // 降级 Lambda 参数为局部变量
        let mut builder = MirBuilder::new(lambda.params.len());
        for (i, p) in lambda.params.iter().enumerate() {
            builder.declare(&p.name, i);
        }
        
        // 降级 Lambda body
        let body_reg = builder.lower_block(&lambda.body, ctx);
        
        // 生成 MakeClosure 指令
        let dst = self.alloc_reg();
        self.push(MirInstr::MakeClosure {
            dst,
            func: closure_name.clone(),
            captures: vec![],
        });
        
        // 注册闭包函数
        ctx.closures.push(MirClosure {
            name: closure_name,
            params: lambda.params.iter().map(|p| p.name.clone()).collect(),
            reg_count: builder.reg_count,
            captures: vec![],
            body: builder.build_body(),
        });
        
        dst
    }
}
```

#### 2.4.4 验收标准

- ✅ Lambda 表达式在 MIR 中生成 `MakeClosure` 指令
- ✅ Lambda 参数正确声明为局部变量
- ✅ Lambda body 正确降级

---

### 2.5 任务 1.2：Lambda 捕获分析

#### 2.5.1 问题描述

Lambda body 可能引用外层变量（闭包捕获），当前 HIR 层**完全无捕获跟踪**。

#### 2.5.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.2.1 | 新增 `CaptureCollector` 结构 | `codegen/hir.rs` | 1d |
| 1.2.2 | 遍历 Lambda body 收集自由变量 | `codegen/hir.rs` | 1.5d |
| 1.2.3 | 捕获变量分类（ByCopy/ByRef/ByMut） | `codegen/hir.rs` | 1d |
| 1.2.4 | 捕获信息传递给 MIR 层 | `codegen/mir.rs` | 1d |
| 1.2.5 | 测试 | `tests/closure_capture_tests.rs` | 0.5d |

#### 2.5.3 技术设计

```rust
// codegen/hir.rs — 新增
pub struct ClosureInfo {
    pub params: Vec<HirParam>,
    pub body: HirBlock,
    pub captures: Vec<CaptureInfo>,
}

pub struct CaptureInfo {
    pub name: String,
    pub ty: HirType,
    pub kind: CaptureKind,
}

pub enum CaptureKind {
    ByCopy,  // 小类型值拷贝（Int/Float/Bool/Char）
    ByRef,   // 只读引用
    ByMut,   // 可变引用（游戏状态常用）
}

// 自由变量收集
pub struct CaptureCollector {
    pub local_vars: HashSet<String>,
    pub captures: Vec<CaptureInfo>,
}

impl CaptureCollector {
    pub fn collect(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Ident(name) => {
                if !self.local_vars.contains(name) && !is_builtin(name) {
                    self.captures.push(CaptureInfo {
                        name: name.clone(),
                        ty: HirType::Unknown,
                        kind: CaptureKind::ByCopy,
                    });
                }
            }
            HirExpr::Val { name, ty, init, is_mut } => {
                self.local_vars.insert(name.clone());
                if let Some(init) = init {
                    self.collect(init);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.collect(lhs);
                self.collect(rhs);
            }
            HirExpr::Call { args, callee, .. } => {
                self.collect(callee);
                for arg in args { self.collect(arg); }
            }
            // ... 其他表达式类型递归
        }
    }
}
```

#### 2.5.4 验收标准

- ✅ 自由变量正确识别
- ✅ 捕获分类正确（ByCopy/ByRef/ByMut）
- ✅ 捕获信息传递给 MIR 层

---

### 2.6 任务 1.3：Lambda 捕获结构体生成

#### 2.6.1 问题描述

捕获分析完成后，需要在 MIR 层生成实际的捕获结构。

#### 2.6.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.3.1 | `MirInstr::MakeClosure` 扩展为携带捕获列表 | `codegen/mir.rs` | 0.5d |
| 1.3.2 | `MirClosure` 新增 `capture_slots` 字段 | `codegen/mir.rs` | 0.5d |
| 1.3.3 | 闭包函数中声明捕获变量为参数 | `codegen/mir.rs` | 1d |
| 1.3.4 | 闭包体内引用捕获变量 | `codegen/mir.rs` | 1d |
| 1.3.5 | 测试 | `tests/closure_capture_tests.rs` | 0.5d |

#### 2.6.3 技术设计

```rust
// 闭包函数的参数布局：[捕获变量...] [用户参数...]
// 例如：fun(x: Int) { return x + captured_var }
// 降级为：__lambda_0(captured_var, x) { return x + captured_var }

impl MirBuilder {
    fn build_closure_function(&mut self, closure: &ClosureInfo) -> MirFunction {
        let mut builder = MirBuilder::new(
            closure.captures.len() + closure.params.len()
        );
        
        // 1. 声明捕获变量为参数
        let mut capture_slots = Vec::new();
        for (i, cap) in closure.captures.iter().enumerate() {
            builder.declare(&cap.name, i);
            capture_slots.push(i);
        }
        
        // 2. 声明用户参数
        for (i, p) in closure.params.iter().enumerate() {
            let slot = closure.captures.len() + i;
            builder.declare(&p.name, slot);
        }
        
        // 3. 降级 body
        let body_reg = builder.lower_block(&closure.body);
        
        MirClosure {
            name: closure.name.clone(),
            params: closure.params.iter().map(|p| p.name.clone()).collect(),
            reg_count: builder.reg_count,
            capture_slots,
            capture_names: closure.captures.iter().map(|c| c.name.clone()).collect(),
            body: builder.build_body(),
        }
    }
}
```

#### 2.6.4 验收标准

- ✅ 闭包函数正确声明捕获参数
- ✅ 闭包体正确引用捕获变量
- ✅ MakeClosure 指令携带捕获列表

---

### 2.7 任务 1.4：字节码 MakeClosure opcode

#### 2.7.1 问题描述

需要新增字节码指令来创建闭包，并在字节码模块中维护闭包表。

#### 2.7.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.4.1 | `OpCode` 新增 `MakeClosure(u16)` | `codegen/opcode.rs` | 0.5d |
| 1.4.2 | `BytecodeModule` 新增 `closures` 字段 | `codegen/opcode.rs` | 0.5d |
| 1.4.3 | `emit.rs` 发射 `MakeClosure` 指令 | `codegen/emit.rs` | 1d |
| 1.4.4 | 闭包表序列化 | `codegen/serialize.rs` | 0.5d |
| 1.4.5 | 测试 | `tests/emit_lambda_tests.rs` | 0.5d |

#### 2.7.3 技术设计

```rust
// codegen/opcode.rs — OpCode 新增
pub enum OpCode {
    // ... 现有
    MakeClosure(u16),  // 闭包表索引
    CallClosure,       // 调用闭包
}

// codegen/opcode.rs — BytecodeModule 新增
pub struct BytecodeModule {
    // ... 现有
    pub closures: Vec<BytecodeClosure>,
}

pub struct BytecodeClosure {
    pub name: String,
    pub param_count: u16,
    pub capture_count: u16,
    pub locals: u16,
    pub code: Vec<u8>,
    pub is_native: bool,
}
```

```rust
// emit.rs — 发射
MirInstr::MakeClosure { dst, func, captures } => {
    for (_, reg) in captures {
        OpCode::LoadVar(*reg as u16).write(code);
    }
    let closure_idx = closure_index[func];
    OpCode::MakeClosure(closure_idx as u16).write(code);
    OpCode::StoreVar(*dst as u16).write(code);
}
```

#### 2.7.4 验收标准

- ✅ `MakeClosure` opcode 正确发射
- ✅ 闭包表正确序列化
- ✅ `.auc` 文件包含闭包信息

---

### 2.8 任务 1.5：VM ClosureObj 运行时

#### 2.8.1 问题描述

需要在 VM 运行时支持闭包对象，包括创建、存储、调用。

#### 2.8.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.5.1 | `HeapData` 新增 `Closure` 变体 | `vm/heap.rs` | 1d |
| 1.5.2 | `Instr` 新增 `MakeClosure`/`CallClosure` | `vm/mod.rs` | 0.5d |
| 1.5.3 | `interp.rs` 实现 `MakeClosure` 执行 | `vm/interp.rs` | 1d |
| 1.5.4 | `interp.rs` 实现 `CallClosure` 执行 | `vm/interp.rs` | 1.5d |
| 1.5.5 | 闭包捕获变量访问 | `vm/interp.rs` | 0.5d |
| 1.5.6 | 测试 | `tests/vm_closure_tests.rs` | 0.5d |

#### 2.8.3 技术设计

```rust
// vm/heap.rs — HeapData 新增
pub enum HeapData {
    // ... 现有
    Closure {
        func_idx: usize,        // 目标闭包函数索引
        captures: Vec<Value>,   // 捕获变量值
        capture_kinds: Vec<CaptureKind>,
    },
}
```

```rust
// vm/interp.rs — MakeClosure 执行
Instr::MakeClosure(closure_idx) => {
    let closure = &self.module.closures[closure_idx as usize];
    let capture_count = closure.capture_count as usize;
    let mut captures = Vec::with_capacity(capture_count);
    for _ in 0..capture_count {
        captures.push(self.pop(top)?);
    }
    let handle = self.heap.alloc(HeapData::Closure {
        func_idx: closure_idx as usize,
        captures,
        capture_kinds: vec![CaptureKind::ByCopy; capture_count],
    });
    self.frames[top].stack.push(Value::Ref(handle));
}
```

```rust
// vm/interp.rs — CallClosure 执行
Instr::CallClosure => {
    let closure_val = self.pop(top)?;
    let closure_ref = match closure_val {
        Value::Ref(h) => h,
        _ => return Err(VmError::TypeMismatch("expected closure")),
    };
    let closure_data = self.heap.get_data(closure_ref);
    let HeapData::Closure { func_idx, captures, .. } = closure_data else {
        return Err(VmError::TypeMismatch("not a closure"));
    };
    let closure_func = &self.module.closures[*func_idx];
    let param_count = closure_func.param_count as usize;
    
    // 弹出参数
    let mut args = Vec::with_capacity(param_count);
    for _ in 0..param_count {
        args.push(self.pop(top)?);
    }
    args.reverse();
    
    // 构建新帧：[捕获变量...] [用户参数...]
    let mut locals = Vec::with_capacity(closure_func.locals as usize);
    for cap in captures.iter() {
        locals.push(cap.clone());
    }
    for arg in args {
        locals.push(arg);
    }
    while locals.len() < closure_func.locals as usize {
        locals.push(Value::Null);
    }
    
    self.push_frame(closure_idx as usize, locals, ...);
}
```

#### 2.8.4 测试用例

```rust
#[test]
fn test_closure_call() {
    let src = "fun main() { val f = fun(x: Int) { return x * 2 }; println(f(5)) }";
    let module = compile_source(src);
    let mut vm = Vm::new(&module);
    vm.run().unwrap();
    assert_eq!(vm.output(), "10\n");
}

#[test]
fn test_closure_with_capture() {
    let src = "fun main() { val x = 10; val f = fun(y: Int) { return x + y }; println(f(5)) }";
    let module = compile_source(src);
    let mut vm = Vm::new(&module);
    vm.run().unwrap();
    assert_eq!(vm.output(), "15\n");
}

#[test]
fn test_closure_mut_capture() {
    let src = "fun main() { var x = 10; val f = fun() { x = x + 1; return x }; println(f()); println(f()) }";
    let module = compile_source(src);
    let mut vm = Vm::new(&module);
    vm.run().unwrap();
    assert_eq!(vm.output(), "11\n12\n");
}
```

#### 2.8.5 验收标准

- ✅ 闭包创建正确
- ✅ 闭包调用正确（含捕获）
- ✅ ByMut 捕获工作正常

---

### 2.9 阶段 2 交付物

| 交付物 | 说明 |
|--------|------|
| MIR 层 Lambda 降级 | `MakeClosure` 指令生成 |
| 捕获分析 | 自由变量收集 + 分类 |
| 捕获结构 | 闭包函数参数布局 |
| 字节码 | `MakeClosure`/`CallClosure` opcode |
| VM 运行时 | `HeapData::Closure` + 执行逻辑 |
| 测试套件 | `tests/mir_lambda_tests.rs`、`tests/closure_capture_tests.rs`、`tests/emit_lambda_tests.rs`、`tests/vm_closure_tests.rs` |

### 2.10 阶段 2 验收标准

- ✅ `cargo test --workspace --features jit` 全部通过
- ✅ 无捕获闭包可用（VM/JIT）
- ✅ 有捕获闭包可用（VM/JIT）
- ✅ ByMut 捕获工作正常
- ✅ `.auc` 文件包含闭包信息

### 2.11 阶段 2 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Lambda 捕获分析错误 | 中 | 严重 | 充分测试，参考 Rust 借用检查器 |
| 闭包帧切换错误 | 中 | 严重 | 单元测试 + 集成测试 |
| 嵌套 Lambda | 低 | 中 | Phase 1 先不支持，报错 |
| 捕获变量生命周期 | 中 | 中 | ARC 自动管理 + 泄漏检测 |

---

## 阶段 3：类型系统扩展

### 3.1 阶段目标

实现 Enum 和 Function 类型在 MIR/字节码/VM 中的完整支持，让游戏状态机和函数指针可用。

### 3.2 阶段范围

| 任务 | 内容 | 预估 | 依赖 |
|------|------|------|------|
| 1.8 | Enum 在 MIR/字节码/VM 中支持 | 5d | 阶段 2 |
| 1.13 | Function 类型在 MIR/字节码/VM 中支持 | 5d | 阶段 2 |

### 3.3 阶段依赖

- **前置**：阶段 2 完成
- **后续**：阶段 6（Raylib 绑定需要函数类型）

### 3.4 任务 1.8：Enum 在 MIR/字节码/VM 中支持

#### 3.4.1 问题描述

Enum 在 HIR 层已收集（`HirEnum`），但 MIR/字节码/VM 层完全未实现。AOT-LLVM 已有 tagged union 支持（`emit.rs:200-224`），但 VM 模式不可用。

#### 3.4.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.8.1 | `MirInstr` 新增 `EnumConstruct`/`EnumMatch`/`EnumTag` | `codegen/mir.rs` | 1d |
| 1.8.2 | `MirBuilder` 降级 Enum 构造 | `codegen/mir.rs` | 1d |
| 1.8.3 | `OpCode` 新增 Enum 指令 | `codegen/opcode.rs` | 0.5d |
| 1.8.4 | `emit.rs` 发射 Enum 指令 | `codegen/emit.rs` | 1d |
| 1.8.5 | `Value` 新增 `Enum` 变体 | `vm/value.rs` | 0.5d |
| 1.8.6 | `interp.rs` 实现 Enum 指令 | `vm/interp.rs` | 1d |
| 1.8.7 | 测试 | `tests/vm_enum_tests.rs` | 0.5d |

#### 3.4.3 技术设计

```rust
// vm/value.rs — Value 新增
pub enum Value {
    // ... 现有
    Enum {
        type_idx: u16,
        variant_idx: u16,
        fields: Vec<Value>,
    },
}
```

```rust
// codegen/opcode.rs — OpCode 新增
pub enum OpCode {
    // ... 现有
    EnumConstruct(u16, u16),  // (type_idx, variant_idx)
    EnumTag,
    EnumMatch(u16, u16, i32), // (type_idx, variant_idx, jump_if_match)
}
```

```rust
// vm/interp.rs — Enum 指令执行
Instr::EnumConstruct(type_idx, variant_idx) => {
    let closure = &self.module.enums[type_idx as usize];
    let fields_count = closure.variants[variant_idx as usize].len();
    let mut fields = Vec::with_capacity(fields_count);
    for _ in 0..fields_count {
        fields.push(self.pop(top)?);
    }
    fields.reverse();
    self.frames[top].stack.push(Value::Enum { type_idx, variant_idx, fields });
}

Instr::EnumTag => {
    let v = self.pop(top)?;
    let variant_idx = match v {
        Value::Enum { variant_idx, .. } => variant_idx,
        _ => return Err(VmError::TypeMismatch("not an enum")),
    };
    self.frames[top].stack.push(Value::Int(variant_idx as i64));
}
```

#### 3.4.4 测试用例

```rust
#[test]
fn test_enum_construct() {
    let src = "enum Color { RED, GREEN, BLUE } fun main() { val c = Color.RED; println(c) }";
    let module = compile_source(src);
    let mut vm = Vm::new(&module);
    vm.run().unwrap();
    assert_eq!(vm.output(), "RED\n");
}

#[test]
fn test_enum_match() {
    let src = "enum Direction { Up, Down } fun main() { val d = Direction.Up; when(d) { Direction.Up -> println('up') else -> println('other') } }";
    let module = compile_source(src);
    let mut vm = Vm::new(&module);
    vm.run().unwrap();
    assert_eq!(vm.output(), "up\n");
}
```

#### 3.4.5 验收标准

- ✅ 枚举构造正确
- ✅ 枚举匹配正确
- ✅ 枚举 tag 获取正确

---

### 3.5 任务 1.13：Function 类型在 MIR/字节码/VM 中支持

#### 3.5.1 问题描述

`HirType::Function` 已存在（`hir.rs:26-30`），但 MIR/字节码/VM 层未实现。

#### 3.5.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.13.1 | `Value` 新增 `Fn` 变体（函数指针） | `vm/value.rs` | 0.5d |
| 1.13.2 | `MirInstr` 新增 `MakeFnRef` | `codegen/mir.rs` | 0.5d |
| 1.13.3 | `OpCode` 新增 `MakeFnRef(u16)` | `codegen/opcode.rs` | 0.5d |
| 1.13.4 | `emit.rs` 发射 MakeFnRef | `codegen/emit.rs` | 0.5d |
| 1.13.5 | `interp.rs` 实现 MakeFnRef | `vm/interp.rs` | 1d |
| 1.13.6 | `CallClosure` 支持函数指针 | `vm/interp.rs` | 1d |
| 1.13.7 | 测试 | `tests/function_type_tests.rs` | 1d |

#### 3.5.3 技术设计

```rust
// vm/value.rs — Value 新增
pub enum Value {
    // ... 现有
    Fn(usize),  // 函数索引
}

// vm/interp.rs — MakeFnRef 执行
Instr::MakeFnRef(fn_idx) => {
    self.frames[top].stack.push(Value::Fn(fn_idx as usize));
}

// CallClosure 修改：支持 Value::Fn
Instr::CallClosure => {
    let v = self.pop(top)?;
    let (func_idx, captures) = match v {
        Value::Ref(h) => {
            let data = self.heap.get_data(h);
            if let HeapData::Closure { func_idx, captures, .. } = data {
                (*func_idx, captures.clone())
            } else {
                return Err(VmError::TypeMismatch("not a closure"));
            }
        }
        Value::Fn(idx) => (idx, vec![]),
        _ => return Err(VmError::TypeMismatch("not callable")),
    };
    // ... 调用
}
```

#### 3.5.4 验收标准

- ✅ 函数类型可以赋值给变量
- ✅ 函数指针可以调用
- ✅ 闭包和函数指针兼容

---

### 3.6 阶段 3 交付物

| 交付物 | 说明 |
|--------|------|
| Enum MIR/VM 支持 | `Value::Enum` + 3 条 Enum 指令 |
| Function 类型 MIR/VM 支持 | `Value::Fn` + `MakeFnRef` + `CallClosure` 扩展 |
| 测试套件 | `tests/vm_enum_tests.rs`、`tests/function_type_tests.rs` |

### 3.7 阶段 3 验收标准

- ✅ `cargo test --workspace --features jit` 全部通过
- ✅ 枚举构造/匹配/tag 在 VM 可用
- ✅ 函数指针在 VM 可用
- ✅ 闭包和函数指针兼容

### 3.8 阶段 3 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Enum when 匹配复杂 | 中 | 严重 | 先支持简单枚举 |
| Enum 关联值处理 | 中 | 中 | Phase 1 先支持简单枚举 |
| 函数指针 ABI 不匹配 | 低 | 严重 | 充分测试 |

---

## 阶段 4：并发语义修正

### 4.1 阶段目标

修正 `ask` 和 `select` 的语义错误，让 Actor 通信在游戏中可靠工作。

### 4.2 阶段范围

| 任务 | 内容 | 预估 | 依赖 |
|------|------|------|------|
| 1.10 | ask 真阻塞语义 | 4d | 阶段 1 |
| 1.11 | select 事件驱动重构 | 4.5d | 1.10 |

### 4.3 阶段依赖

- **前置**：阶段 1 完成
- **后续**：阶段 6（游戏微服务场景）

### 4.4 任务 1.10：ask 真阻塞语义

#### 4.4.1 问题描述

当前 `actor.rs:111-119`：
```rust
pub fn ask(&mut self, id: ActorId, msg: Value) -> Value {
    self.send(id, msg);
    if let Some(actor) = self.get_mut(id) {
        actor.mailbox.pop_front().unwrap_or(Value::Null)
    } else {
        Value::Null
    }
}
```

立即返回 Null ≠ "真的没响应"，只是"目标 Actor 还没轮到处理"。

#### 4.4.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.10.1 | 新增 `PendingRequest` 结构 | `vm/actor.rs` | 1d |
| 1.10.2 | `ask` 基于协程调度器轮询 | `vm/actor.rs` | 1d |
| 1.10.3 | Actor 处理消息后将响应写回请求者 | `vm/actor.rs` | 1d |
| 1.10.4 | 超时支持 | `vm/actor.rs` | 0.5d |
| 1.10.5 | 测试 | `tests/actor_ask_tests.rs` | 0.5d |

#### 4.4.3 技术设计

```rust
// vm/actor.rs — 新增
pub struct PendingRequest {
    pub request_id: u64,
    pub from_actor: ActorId,
    pub response_coroutine: usize,  // 挂起的协程 ID
}

impl ActorRuntime {
    pub pending_requests: HashMap<u64, PendingRequest>,
    pub next_request_id: u64,
    
    pub fn ask(&mut self, id: ActorId, msg: Value, timeout: Option<Duration>) -> Value {
        let req_id = self.next_request_id;
        self.next_request_id += 1;
        
        let from_actor = self.current_actor;
        let coroutine_id = self.current_coroutine;
        self.pending_requests.insert(req_id, PendingRequest {
            request_id: req_id,
            from_actor,
            response_coroutine: coroutine_id,
        });
        
        // 发送消息（携带请求 ID）
        let wrapped_msg = Value::Map(HashMap::from([
            ("_request_id", Value::Int(req_id as i64)),
            ("_from", Value::Int(from_actor as i64)),
            ("_payload", msg),
        ]));
        self.send(id, wrapped_msg);
        
        // 挂起当前协程，让出调度
        self.scheduler.yield_current(coroutine_id);
        
        // 恢复后返回响应
        if let Some(req) = self.pending_requests.remove(&req_id) {
            self.receive_response(req, timeout)
        } else {
            Value::Null
        }
    }
}
```

#### 4.4.4 验收标准

- ✅ ask 真阻塞直到响应
- ✅ 超时返回 Null
- ✅ 无死锁

---

### 4.5 任务 1.11：select 事件驱动重构

#### 4.5.1 问题描述

当前 `select` 全轮询所有通道（O(n)），高通道数场景退化。

#### 4.5.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.11.1 | 分析当前 select 实现 | `vm/native.rs` | 0.5d |
| 1.11.2 | 设计基于协程挂起的 select | — | 1d |
| 1.11.3 | 实现基于协程的 select | `vm/native.rs` | 2d |
| 1.11.4 | 测试 | `tests/select_tests.rs` | 1d |

#### 4.5.3 技术设计

由于 Phase 1 仍是单线程协作调度，select 改为**基于协程挂起**而非轮询：

```rust
// 基于协程挂起的 select（伪代码）
fn select_impl(runtime: &mut ActorRuntime, channels: &[ChannelId]) -> (ChannelId, Value) {
    // 1. 检查所有通道是否有值
    // 2. 如果有值，立即返回
    // 3. 如果都空，挂起当前协程
    // 4. 当某个通道有新值时，唤醒协程
    // 5. 返回第一个有值的通道
}
```

#### 4.5.4 验收标准

- ✅ select 不再全轮询
- ✅ 高通道数场景性能改善
- ✅ 单线程语义正确

---

### 4.6 阶段 4 交付物

| 交付物 | 说明 |
|--------|------|
| ask 真阻塞 | 基于协程调度器的阻塞等待 |
| select 事件驱动 | 基于协程挂起的非轮询 |
| 测试套件 | `tests/actor_ask_tests.rs`、`tests/select_tests.rs` |

### 4.7 阶段 4 验收标准

- ✅ `cargo test --workspace` 全部通过
- ✅ ask 真阻塞语义正确
- ✅ ask 超时返回 Null
- ✅ select 不再全轮询

### 4.8 阶段 4 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| ask 死锁（两个 Actor 互等） | 中 | 严重 | 超时机制 |
| 响应丢失 | 低 | 中 | 请求 ID 跟踪 |
| select 协程挂起错误 | 中 | 中 | 充分测试 |

---

## 阶段 5：AOT 后端补全

### 5.1 阶段目标

让闭包在 AOT-LLVM 和 AOT-C 模式下可用，完成 AOT 后端的闭包支持。

### 5.2 阶段范围

| 任务 | 内容 | 预估 | 依赖 |
|------|------|------|------|
| 1.6 | AOT-LLVM 闭包捕获支持 | 5d | 阶段 2 |
| 1.7 | AOT-C 闭包支持 | 3d | 阶段 2 |

### 5.3 阶段依赖

- **前置**：阶段 2 完成
- **后续**：阶段 6（AOT 编译 game_2d_demo）

### 5.4 任务 1.6：AOT-LLVM 闭包捕获支持

#### 5.4.1 问题描述

当前 `aot/emit.rs:899` 只生成静态函数，无捕获。

#### 5.4.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.6.1 | 闭包捕获变量作为参数 | `aot/emit.rs` | 2d |
| 1.6.2 | 闭包函数生成 | `aot/emit.rs` | 1d |
| 1.6.3 | MakeClosure 发射为函数指针 | `aot/emit.rs` | 1d |
| 1.6.4 | 测试 | `tests/aot_closure_tests.rs` | 1d |

#### 5.4.3 技术设计

```rust
// aot/emit.rs — 闭包生成
fn emit_closure(&mut self, closure: &MirClosure) -> String {
    let param_strs: Vec<String> = closure.capture_names.iter()
        .chain(closure.params.iter())
        .map(|name| format!("{} %arg_{}", self.llvm_type_for(name), sanitize_llvm(name)))
        .collect();
    
    format!("define i32 @{}({}) {{\nentry:\n...", 
        format!("__lambda_{}", idx),
        param_strs.join(", "))
}
```

#### 5.4.4 验收标准

- ✅ AOT-LLVM 模式下闭包可用
- ✅ 捕获变量正确传递

---

### 5.5 任务 1.7：AOT-C 闭包支持

#### 5.5.1 问题描述

当前 `c_backend.rs:325` 是 `TODO: lambda`。

#### 5.5.2 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.7.1 | C 后端闭包函数生成 | `aot/c_backend.rs` | 1d |
| 1.7.2 | MakeClosure 发射为函数指针 | `aot/c_backend.rs` | 0.5d |
| 1.7.3 | 测试 | `tests/aot_c_closure_tests.rs` | 1d |

#### 5.5.3 技术设计

```c
// C 后端生成的闭包
// 捕获变量作为参数
static int __lambda_0(int captured_x, int y) {
    return x + y;
}

// MakeClosure 发射为函数指针包装
void* f = create_closure_wrapper(__lambda_0, x);
```

#### 5.5.4 验收标准

- ✅ AOT-C 模式下闭包可用
- ✅ 捕获变量正确传递

---

### 5.6 阶段 5 交付物

| 交付物 | 说明 |
|--------|------|
| AOT-LLVM 闭包 | 捕获变量作为参数 |
| AOT-C 闭包 | 函数指针包装 |
| 测试套件 | `tests/aot_closure_tests.rs`、`tests/aot_c_closure_tests.rs` |

### 5.7 阶段 5 验收标准

- ✅ `cargo test --workspace --features llvm` 全部通过
- ✅ AOT-LLVM 闭包可用
- ✅ AOT-C 闭包可用

### 5.8 阶段 5 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| AOT-LLVM 闭包 ABI 不匹配 | 低 | 严重 | 跨平台测试 |
| C 后端函数指针包装 | 中 | 中 | 先支持简单闭包 |

---

## 阶段 6：集成与演示

### 6.1 阶段目标

提供 Raylib 完整绑定、游戏场景测试套件、端到端 2D 游戏 Demo，验证 Phase 1 全部功能在游戏场景中可用。

### 6.2 阶段范围

| 任务 | 内容 | 预估 | 依赖 |
|------|------|------|------|
| 1.16 | Raylib 完整绑定 | 5d | 阶段 2,3,5 |
| 1.17 | 回归测试套件 | 5d | 阶段 2,3,4 |
| 1.18 | game_2d_demo 端到端验证 | 3d | 阶段 6.16,6.17 |

### 6.3 阶段依赖

- **前置**：阶段 1-5 全部完成
- **后续**：无（Phase 1 最终阶段）

### 6.4 任务 1.16：Raylib 完整绑定

#### 6.4.1 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.16.1 | Raylib 核心函数绑定（窗口/渲染/输入） | `std/raylib.rs` | 2d |
| 1.16.2 | Raylib 回调绑定（键盘/鼠标/窗口） | `std/raylib.rs` | 1d |
| 1.16.3 | 游戏循环标准库 | `std/game.rs` | 1d |
| 1.16.4 | 状态机模板 | `std/game.rs` | 0.5d |
| 1.16.5 | 测试 | `tests/raylib_binding_tests.rs` | 0.5d |

#### 6.4.2 技术设计

```aura
// std/raylib.rs
extern "c" "raylib" {
    fun InitWindow(width: Int, height: Int, title: String): Unit
    fun SetTargetFPS(fps: Int): Unit
    fun WindowShouldClose(): Int
    fun BeginDrawing(): Unit
    fun EndDrawing(): Unit
    fun ClearBackground(color: Color): Unit
    fun IsKeyPressed(key: Int): Int
    fun IsKeyDown(key: Int): Int
    fun GetMouseX(): Int
    fun GetMouseY(): Int
    fun DrawRectangle(x: Int, y: Int, width: Int, height: Int, color: Color): Unit
    fun DrawText(text: String, x: Int, y: Int, fontSize: Int, color: Color): Unit
    fun DrawCircle(x: Int, y: Int, radius: Float, color: Color): Unit
    val WHITE: Color
    val BLACK: Color
    val RED: Color
    val KEY_UP: Int
    val KEY_DOWN: Int
    val KEY_LEFT: Int
    val KEY_RIGHT: Int
    val KEY_P: Int
    val KEY_R: Int
    val KEY_SPACE: Int
}

// std/game.rs — 游戏循环
class GameLoop {
    var fps: Float = 60.0f
    
    fun run() {
        raylib.InitWindow(800, 600, "Aura Game")
        raylib.SetTargetFPS(60)
        while (raylib.WindowShouldClose() == 0) {
            let dt = raylib.GetFrameTime()
            self.update(dt)
            raylib.BeginDrawing()
            raylib.ClearBackground(raylib.BLACK)
            self.render()
            raylib.EndDrawing()
        }
    }
    
    fun update(dt: Float) {}
    fun render() {}
}
```

#### 6.4.3 验收标准

- ✅ Raylib 核心函数可用
- ✅ 回调函数可用（闭包）
- ✅ 游戏循环模板可用

---

### 6.5 任务 1.17：回归测试套件

#### 6.5.1 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.17.1 | 游戏场景测试（状态机/回调/资源） | `tests/game_scenario_tests.rs` | 2d |
| 1.17.2 | 闭包测试套件 | `tests/closure_tests.rs` | 1d |
| 1.17.3 | 枚举测试套件 | `tests/enum_tests.rs` | 1d |
| 1.17.4 | 并发测试套件 | `tests/concurrency_tests.rs` | 1d |

#### 6.5.2 测试用例

```rust
// tests/game_scenario_tests.rs
#[test]
fn test_game_state_machine() {
    // enum GameState { Menu, Playing, Paused, GameOver }
    // 验证状态转换
}

#[test]
fn test_game_callback() {
    // 键盘回调闭包
    // 验证回调触发
}

#[test]
fn test_game_loop_60fps() {
    // 游戏循环 60 FPS
    // 验证帧时间
}
```

#### 6.5.3 验收标准

- ✅ 20+ 游戏场景测试通过
- ✅ 覆盖状态机/回调/资源加载/并发

---

### 6.6 任务 1.18：game_2d_demo 端到端验证

#### 6.6.1 子任务

| # | 子任务 | 文件 | 预估 |
|---|--------|------|------|
| 1.18.1 | 编写 Demo 源码 | `examples/game_2d_demo.aura` | 2d |
| 1.18.2 | AOT 编译验证 | — | 0.5d |
| 1.18.3 | VM 运行验证 | — | 0.5d |

#### 6.6.2 Demo 源码

```aura
// examples/game_2d_demo.aura
import raylib

enum GameState { Menu, Playing, Paused, GameOver }

struct Player(var x: Int, var y: Int, var speed: Int = 5)

class Game : GameLoop {
    var state: StateMachine<GameState> = StateMachine()
    var player: Player = Player(400, 300)
    
    override fun update(dt: Float) {
        when (state.currentState()) {
            GameState.Playing -> {
                if (raylib.IsKeyDown(raylib.KEY_LEFT)) { player.x -= player.speed }
                if (raylib.IsKeyDown(raylib.KEY_RIGHT)) { player.x += player.speed }
                if (raylib.IsKeyDown(raylib.KEY_UP)) { player.y -= player.speed }
                if (raylib.IsKeyDown(raylib.KEY_DOWN)) { player.y += player.speed }
                if (raylib.IsKeyPressed(raylib.KEY_P)) { state.enter(GameState.Paused) }
            }
            GameState.Paused -> {
                if (raylib.IsKeyPressed(raylib.KEY_P)) { state.enter(GameState.Playing) }
            }
            GameState.GameOver -> {
                if (raylib.IsKeyPressed(raylib.KEY_R)) { state.enter(GameState.Playing) }
            }
        }
    }
    
    override fun render() {
        when (state.currentState()) {
            GameState.Menu -> {
                raylib.DrawText("Press SPACE to start", 250, 250, 20, raylib.WHITE)
                if (raylib.IsKeyPressed(raylib.KEY_SPACE)) { state.enter(GameState.Playing) }
            }
            GameState.Playing -> {
                raylib.DrawRectangle(player.x, player.y, 50, 50, raylib.RED)
                raylib.DrawText("Press P to pause", 10, 10, 10, raylib.WHITE)
            }
            GameState.Paused -> {
                raylib.DrawText("PAUSED", 350, 250, 40, raylib.WHITE)
            }
            GameState.GameOver -> {
                raylib.DrawText("GAME OVER", 300, 250, 40, raylib.RED)
            }
        }
    }
}

fun main() {
    val game = Game()
    game.run()
}
```

#### 6.6.3 验收标准

- ✅ `aura build game_2d_demo.aura --aot` 成功
- ✅ VM 模式运行
- ✅ 游戏循环 60 FPS
- ✅ 状态机工作正常
- ✅ 键盘输入响应

---

### 6.7 阶段 6 交付物

| 交付物 | 说明 |
|--------|------|
| Raylib 完整绑定 | 核心函数 + 回调 + 常量 |
| 游戏循环标准库 | `GameLoop` 类 |
| 状态机模板 | `StateMachine<T>` 类 |
| 游戏场景测试套件 | 20+ 测试 |
| game_2d_demo | 端到端 2D 游戏 |

### 6.8 阶段 6 验收标准

- ✅ `cargo test --workspace --features llvm,jit` 全部通过
- ✅ Raylib 绑定可用
- ✅ 游戏场景测试全部通过
- ✅ `game_2d_demo.aura` AOT 编译成功
- ✅ `game_2d_demo.aura` VM 运行成功

### 6.9 阶段 6 风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| Raylib 绑定调试困难 | 中 | 中 | 先在 C 层验证 |
| 游戏循环性能不达标 | 低 | 中 | 优化热点路径 |
| 跨平台差异 | 低 | 中 | 在 RPi 上验证 |

---

## 测试策略总览

### 测试分层

| 层级 | 文件 | 覆盖 |
|------|------|------|
| 单元测试 | `tests/*_tests.rs` | 单个组件 |
| 集成测试 | `tests/game_scenario_tests.rs` | 多组件协作 |
| 端到端测试 | `examples/game_2d_demo.aura` | 完整游戏 |

### 测试运行命令

```bash
# 全量测试
cargo test --workspace --features llvm,jit

# 按阶段测试
cargo test --features llvm,jit -- await          # 阶段 1
cargo test --features llvm,jit -- closure lambda  # 阶段 2
cargo test --features llvm,jit -- enum function   # 阶段 3
cargo test --features llvm,jit -- ask select      # 阶段 4
cargo test --features llvm -- aot_closure          # 阶段 5
cargo test --features llvm,jit -- game_scenario   # 阶段 6
```

### 覆盖率目标

| 模块 | 当前 | Phase 1 目标 |
|------|------|-------------|
| Lambda 链路 | 0% | 90%+ |
| Enum 链路 | 0% | 90%+ |
| await 语义 | 0% | 100% |
| ask 语义 | 0% | 80%+ |
| Raylib 绑定 | 0% | 70%+ |

---

## 风险清单

### 技术风险

| 风险 | 概率 | 影响 | 缓解措施 | 涉及阶段 |
|------|------|------|---------|---------|
| Lambda 捕获分析错误 | 中 | 严重 | 充分测试，参考 Rust 借用检查器 | 2 |
| 闭包帧切换错误 | 中 | 严重 | 单元测试 + 集成测试 | 2 |
| Enum when 匹配错误 | 中 | 严重 | 充分测试 | 3 |
| ask 死锁 | 中 | 严重 | 超时机制 | 4 |
| AOT 闭包 ABI 不匹配 | 低 | 严重 | 跨平台测试 | 5 |
| 内存泄漏（闭包） | 中 | 中 | ARC 自动管理 + 泄漏检测 | 2 |
| Raylib 绑定调试困难 | 中 | 中 | 先在 C 层验证 | 6 |

### 进度风险

| 风险 | 概率 | 影响 | 缓解措施 | 涉及阶段 |
|------|------|------|---------|---------|
| Lambda 链路超预期复杂 | 中 | 高 | 分批提交，先支持简单闭包 | 2 |
| 并发语义重构困难 | 中 | 高 | Phase 1 仅单线程，多线程留 Phase 2 | 4 |
| AOT 后端调试困难 | 中 | 中 | 先 VM 验证，再 AOT | 5 |

### 缓解策略

1. **分批提交**：每完成一个子任务就提交，避免大爆炸
2. **先简单后复杂**：先支持无捕获闭包，再加捕获
3. **测试先行**：每个子任务先写测试，再实现
4. **回滚计划**：如果某任务卡住，先跳过，后续迭代
5. **阶段检查点**：每个阶段结束时运行全量测试，确保无回归

---

## 完成定义（DoD）

### Phase 1 整体 DoD

- ✅ Lambda 在 VM/JIT/AOT-LLVM/AOT-C 全部模式可用
- ✅ Enum 在 VM/JIT/AOT-LLVM/AOT-C 全部模式可用
- ✅ `await` 在非 suspend 函数中报编译错误
- ✅ `ask` 真阻塞语义正确
- ✅ AOT-LLVM `emit_call` 返回值修复验证通过
- ✅ Function 类型在 VM/JIT 可用
- ✅ C ABI 蹦床支持 5+ 参数
- ✅ 静态链接类型安全调用
- ✅ Raylib 完整绑定可用
- ✅ `examples/game_2d_demo.aura` 端到端运行
- ✅ 所有测试通过（`cargo test --workspace --features llvm,jit`）
- ✅ 无回归（现有测试全部通过）

### 各阶段 DoD

| 阶段 | DoD |
|------|-----|
| 阶段 1 | await 语义正确 + emit_call 验证通过 + C ABI 蹦床 + 静态链接类型安全 |
| 阶段 2 | 闭包在 VM/JIT 可用（含捕获、ByMut） |
| 阶段 3 | Enum + Function 类型在 VM 可用 |
| 阶段 4 | ask 真阻塞 + select 事件驱动 |
| 阶段 5 | 闭包在 AOT-LLVM/AOT-C 可用 |
| 阶段 6 | Raylib 绑定 + 测试套件 + game_2d_demo 端到端 |

---

## 附录 A：关键文件修改清单

| 文件 | 修改内容 | 涉及阶段 |
|------|---------|---------|
| `compiler/src/codegen/hir.rs` | CaptureCollector、ClosureInfo | 2 |
| `compiler/src/codegen/mir.rs` | MirInstr::MakeClosure、MirClosure、Enum 指令、MakeFnRef | 2,3 |
| `compiler/src/codegen/opcode.rs` | MakeClosure、CallClosure、Enum 指令、MakeFnRef | 2,3 |
| `compiler/src/codegen/emit.rs` | 闭包/枚举/函数指针字节码发射 | 2,3 |
| `compiler/src/codegen/serialize.rs` | 闭包表序列化 | 2 |
| `compiler/src/vm/value.rs` | Value::Enum、Value::Fn | 3 |
| `compiler/src/vm/heap.rs` | HeapData::Closure | 2 |
| `compiler/src/vm/mod.rs` | Instr::MakeClosure、CallClosure | 2 |
| `compiler/src/vm/interp.rs` | 闭包/枚举/函数指针执行 | 2,3 |
| `compiler/src/vm/actor.rs` | ask 真阻塞 | 4 |
| `compiler/src/vm/native.rs` | select 重构 | 4 |
| `compiler/src/vm/ffi.rs` | 可变参数蹦床、类型安全调用 | 1 |
| `compiler/src/sema/checker.rs` | await 编译期检查 | 1 |
| `compiler/src/codegen/aot/emit.rs` | 闭包捕获支持 | 5 |
| `compiler/src/codegen/aot/c_backend.rs` | 闭包支持 | 5 |
| `compiler/src/std/raylib.rs` | Raylib 绑定（新增） | 6 |
| `compiler/src/std/game.rs` | 游戏循环（新增） | 6 |
| `tests/*_tests.rs` | 测试套件（多个新增） | 1-6 |
| `examples/game_2d_demo.aura` | Demo（新增） | 6 |

---

## 附录 B：版本规划

| 版本 | 时间 | 阶段 | 内容 |
|------|------|------|------|
| v0.2.0-alpha | D+2 | 1 | await 语义 + emit_call 验证 + C ABI + 静态链接 |
| v0.2.0-beta | D+13 | 2 | Lambda 链路完成 |
| v0.2.0-rc1 | D+18 | 3 | Enum + Function 类型 |
| v0.2.0-rc2 | D+22 | 4 | 并发语义修正 |
| v0.2.0 | D+27 | 5 | AOT 闭包 |
| v0.3.0 | D+35 | 6 | Raylib 绑定 + 测试 + Demo |
