# AOT 机器码嵌入方案 —— Phase 2/3 实现文档

## 概述

本方案实现了 AOT 编译的机器码与 VM/JIT 在同一进程内运行，由 VM 分发器直接调用，零 FFI 开销。

**核心思路**：AOT 机器码 = 预编译的 JIT 函数，复用现有 `JitValue`/`JitEntry` 调用约定，嵌入 `.auc` 文件。

## 设计文档

详见 `docs/AOT机器码嵌入方案-详细设计.md`

## Phase 2: 完整调用约定

### 2.1-2.5 全类型支持

`emit.rs` 的 `map_type_to_tag` 和 `emit_wrapper` 扩展支持以下类型：

| Aura 类型 | JitValue Tag | LLVM IR 处理 |
|-----------|-------------|-------------|
| Int/Long/Short/Byte/U8/Char | TAG_INT (0) | trunc/zext i64 |
| Float/Double | TAG_FLOAT (1) | bitcast i64↔double |
| Bool | TAG_BOOL (2) | trunc/zext i1 |
| Unit/Void | TAG_NULL (3) | payload = 0 |
| String | TAG_STR (4) | inttoptr/ptrtoint |
| Pointer\<T\> | TAG_PTR (5) | inttoptr/ptrtoint |
| Array\<T\> | TAG_ARRAY (6) | inttoptr/ptrtoint |
| List | TAG_LIST (7) | inttoptr/ptrtoint |
| Map | TAG_MAP (8) | inttoptr/ptrtoint |
| Closure | TAG_CLOSURE (9) | inttoptr/ptrtoint |
| Function | TAG_FUNC (10) | inttoptr/ptrtoint |
| CString | TAG_CSTRING (11) | inttoptr/ptrtoint |
| Nullable\<T\> | 递归映射 | 同内部类型 |

**参数解包**：引用类型通过 `inttoptr` 将 i64 payload 转为 LLVM 指针。
**返回值包装**：引用类型通过 `ptrtoint` 将 LLVM 指针转为 i64 payload。

### 2.6 异常处理

`AotCallContext.exception` 字段用于异常传播：
- VM 端 `AotCallContext::new()` 初始化为 0（正常）
- AOT 函数通过 ctx 指针写入非零值表示异常
- `AotRuntime::call_func` 检查 `ctx.exception != 0` 并返回错误

**ctx 布局**（`#[repr(C)]`）：
```
offset 0:  runtime    *mut ()  (8 bytes)
offset 8:  module_id  u32      (4 bytes)
offset 12: func_idx   u32      (4 bytes)
offset 16: call_depth u32      (4 bytes)
offset 20: exception  i32      (4 bytes)
```

### 2.7 混合模式

`aot_mode = 2` 表示字节码和 AOT 版本同时存在：
- VM 优先使用 AOT 分发表（`has_entry` 检查）
- AOT 未命中时回退到字节码解释
- 支持 `prefer_bytecode` / `prefer_aot` / `threshold_based` 策略

### 2.8-2.9 字符串池

`.auc` v4 新增 `SEG_STRING_POOL` 段（ID = 4）：
- 包含函数名、源文件名等字符串数据
- 格式：偏移表 + null 终止字符串数据
- `AuraFuncDesc.name_offset` / `name_len` 指向字符串池

### 2.10 集成测试

`aot_embed_full_tests.rs` 覆盖：
- Bool 返回函数
- Void 返回函数
- Float 边界值
- 多参数函数
- 嵌套调用（AOT 函数调用 AOT 函数）

## Phase 3: 安全与优化

### 3.1 Ed25519 签名

`serialize.rs` 新增：
- `Ed25519Keypair`：密钥对生成/导入/导出
- `sign_auc()`：对 `.auc` 文件签名（SHA-256 + Ed25519）
- `verify_auc_signature()`：验证签名
- `sign_auc_from_env()`：从环境变量加载密钥签名

**签名格式**（追加到 `.auc` 尾部）：
- 64 bytes: Ed25519 签名
- 32 bytes: 签名公钥

### 3.2 模块沙箱

`AotRuntime` 新增：
- `AOT_MAX_CALL_DEPTH = 1024`：调用深度上限
- `check_call_depth()`：深度检查

### 3.3 热重载

`AotRuntime::hot_reload_module()`：
- 卸载旧模块（`AotModule::unload()` → munmap/VirtualFree）
- 加载新版本模块
- 分配新模块 ID
- 无需重启进程

### 3.5 AOT 入口查找缓存

`AotRuntime.entry_cache`（`HashMap<usize, u32>`）：
- 缓存 `func_idx → module_id` 映射
- `cached_find_entry()`：O(1) 查找（命中时）
- `clear_entry_cache()`：模块卸载/重载后清除

### 3.6 诊断信息

`ModuleDiagnostics` 结构体：
- `module_id` / `name`：模块标识
- `is_loaded` / `code_base`：加载状态
- `func_count` / `dispatch_count`：函数/分发表统计
- `entry_offsets`：入口偏移列表

### 3.8 集成测试

`aot_embed_security_tests.rs` 覆盖：
- Ed25519 签名/验证/篡改检测
- 热重载模块
- 调用深度限制
- 入口缓存
- 模块诊断
- 签名保护下 VM 加载

## 文件改动清单

### 新增文件
- `compiler/tests/aot_embed_full_tests.rs` - Phase 2 全类型集成测试
- `compiler/tests/aot_embed_security_tests.rs` - Phase 3 安全/热重载测试
- `compiler/tests/aot_embed_demo.rs` - 端到端演示测试
- `docs/AOT机器码嵌入方案-Phase2-3实现.md` - 本文档

### 修改文件
- `compiler/src/codegen/aot/emit.rs` - 全类型支持 + 异常处理
- `compiler/src/vm/abi.rs` - JitValue 新增类型转换
- `compiler/src/codegen/aot_embed.rs` - 字符串池生成
- `compiler/src/vm/aot_runtime.rs` - 热重载/沙箱/缓存/诊断
- `compiler/src/codegen/serialize.rs` - Ed25519 签名
- `compiler/Cargo.toml` - 新增 ed25519-dalek 依赖
- `compiler/src/codegen/aot_embed.rs` (tests) - 测试更新
- `compiler/tests/aot_embed_tests.rs` - 测试更新（3 段表）

## 运行测试

```bash
# 全类型测试
cargo test --features llvm --test aot_embed_full_tests

# 安全测试
cargo test --features llvm --test aot_embed_security_tests

# 端到端演示
cargo test --features llvm --test aot_embed_demo -- --no-capture

# 全部 AOT 测试
cargo test --features llvm aot_embed

# 完整测试套件
cargo test --features llvm
```