# Demo 2: Rust FFI — 同一 Aura AOT DLL 可被 Rust 直接调用

## 概述

本 demo 展示同一 Aura AOT DLL（`--cabi --shared` 产物）可被 Rust 直接通过 `extern "C"` 调用，
无需额外编译步骤。与 Demo 1 使用完全相同的运行时路径（`libloading` + `dlsym`），
区别在于消费角度不同。

## 架构

```
demo_rustffi/src/main.aura ──extern "C"──→ utils.dll (Aura AOT + --cabi --shared)
   │                              │
   └── JIT ─── libloading ────────┘  (与 Demo 1 完全相同)
```

```
Rust (utils.rs) ──extern "C"──→ utils.dll (同一 DLL)
   │                              │
   └── rustc/link ────────────────┘
```

## 构建命令

```bash
cd examples/ext_ffi_demo

# 与 Demo 1 完全相同
loom build --member utils --aot --shared --cabi
loom build --member demo-rustffi
loom run --member demo-rustffi
```

## Rust 绑定验证（可选）

```bash
# 编译 Rust 绑定文件（展示 Rust 兼容性）
rustc demo_rustffi/utils.rs -L target/build/libs/utils/ -l utils
# 运行
./demo_rustffi/utils
```

## 源码说明

### 调用方（src/main.aura）

与 Demo 1 完全相同的 `extern "C"` 块 + `libloading` 调用路径。

### Rust 绑定（utils.rs）

使用 `#[link(name = "utils", kind = "dylib")]` 链接同一 DLL，通过 `extern "C"`
直接消费 `aura_c_*` 导出符号。无需任何额外编译步骤或中间层。

## 验证标准

| 函数 | 输入 | 期望输出 |
|------|------|----------|
| `add(3, 4)` | (3, 4) | 7 |
| `multiply(3, 4)` | (3, 4) | 12 |
| `factorial(5)` | 5 | 120 |
| `power(2, 10)` | (2, 10) | 1024 |

Rust 绑定验证同样期望上述输出。

## 编译器改动

❌ 无 — 复用已完成的 `--cabi` 基础设施。

## 外部依赖

- `clang` / `llc`（已有）— LLVM 后端
- `cargo`（已有）— Rust 编译（仅验证用）
