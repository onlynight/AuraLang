# Demo 1: C FFI — Aura 调用 Aura AOT 的 C ABI 产物

## 概述

本 demo 展示 Aura 作为上层调用方，通过 `extern "C"` 调用 Aura AOT 编译的 C ABI 产物。

## 架构

```
demo_cffi/src/main.aura ──extern "C"──→ utils.dll (Aura AOT + --cabi --shared)
   │                              │
   └── JIT ─── libloading ────────┘
```

## 构建命令

```bash
cd examples/ext_ffi_demo

# 1. 生成 C 头文件（供文档参考）
aura export-header libs/utils/src/lib.aura --out demo_cffi/utils.h

# 2. 构建共享库（AOT + C ABI）
loom build --member utils --aot --shared --cabi

# 3. 构建调用方
loom build --member demo-cffi

# 4. 运行
loom run --member demo-cffi
```

## 源码说明

### 调用方（src/main.aura）

通过 `extern "C"` 块声明 4 个 FFI 函数，JIT 解释器在调用时通过 `libloading`（dlsym/dllexport）
解析符号地址。未链接 dylib 时返回占位值（不崩溃），链接后 0 开销直接调用。

### C 头文件（utils.h）

由 `aura export-header` 生成，展示 Aura AOT + `--cabi` 导出的 C ABI 接口。
导出符号前缀为 `aura_c_`，便于与标准 C 函数区分。

## 验证标准

| 函数 | 输入 | 期望输出 |
|------|------|----------|
| `add(3, 4)` | (3, 4) | 7 |
| `multiply(3, 4)` | (3, 4) | 12 |
| `factorial(5)` | 5 | 120 |
| `power(2, 10)` | (2, 10) | 1024 |

## 编译器改动

❌ 无 — 复用已完成的 `--cabi` 基础设施。

## 外部依赖

- `clang` / `llc`（已有）— LLVM 后端
- `libloading` crate — 运行时动态库加载
