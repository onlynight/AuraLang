# Demo 3: AOT 直调 — Aura 调用 Aura AOT 的 JitValue ABI 产物

## 概述

本 demo 展示 Aura 通过 `load_shared_library` / `call_func` 直接调用 Aura AOT 编译的
JitValue ABI 产物，无需手动映射到 C 类型，无需声明 `extern "C"` 块。

## 架构

```
demo_aot_direct/src/main.aura ──load_shared_library──→ utils.dll (Aura AOT + --shared)
   │                                                │
   └── call_func(module_id, 0, [3,4]) ──→ aura_aot_add!2!2!2!2
```

## 构建命令

```bash
cd examples/ext_ffi_demo

# 1. 构建共享库（AOT，不带 --cabi，导出 JitValue ABI）
loom build --member utils --aot --shared

# 2. 构建调用方
loom build --member demo-aot-direct

# 3. 运行
loom run --member demo-aot-direct
```

## 源码说明

### 调用方（src/main.aura）

1. `load_shared_library(path)` — 加载共享库，返回 `module_id`
2. `call_func(module_id, func_idx, [args])` — 通过函数索引调用，返回 JitValue 结果
3. `unload_shared_library(module_id)` — 清理资源

函数索引（func_idx）由运行时枚举 `aura_aot_*` 导出符号确定：
- func_idx=0: `add`
- func_idx=1: `multiply`
- func_idx=2: `factorial`
- func_idx=3: `power`

## 与 Demo 1/2 的关键差异

| 特性 | Demo 1/2 (C FFI) | Demo 3 (AOT 直调) |
|------|-------------------|---------------------|
| 编译参数 | `--cabi --shared` | `--shared` |
| 导出符号 | `aura_c_add` | `aura_aot_add!2!2!2!2` |
| 调用声明 | 需要 `extern "C" { ... }` | 不需要，运行时自动发现 |
| 类型系统 | 手动映射到 C 类型 | Aura 自动处理 |
| 函数索引 | 静态已知 | 运行时枚举 |
| 额外依赖 | 无 | 无 |

## 验证标准

| 调用 | 期望输出 |
|------|----------|
| `call_func(module_id, 0, [3, 4])` | 7 |
| `call_func(module_id, 1, [3, 4])` | 12 |
| `call_func(module_id, 2, [5])` | 120 |
| `call_func(module_id, 3, [2, 10])` | 1024 |

## 编译器改动

❌ 无 — 复用已完成的 `--shared` 基础设施。

## 外部依赖

- `clang` + `llc`（已有）— LLVM 后端
- `libloading` crate — 运行时动态库加载

## 参考

- `compiler/examples/aot_plugin_load.rs` — Rust 层验证逻辑（已存在）
- `compiler/src/vm/aot_runtime.rs` — `AotRuntime::load_shared_library` / `call_func`
