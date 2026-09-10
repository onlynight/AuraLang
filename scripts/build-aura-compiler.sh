#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# 构建 Aura 编译器（纯 Aura 化迁移 · Phase 0）
#
# 用 Rust 编译器编译「Aura 编写的 Aura 编译器」源码：
#   aura/compiler/aura/lang/compiler/Main.aura  →  build/aura-compiler.(auc|exe)
#
# 用法：
#   scripts/build-aura-compiler.sh             # 默认产出字节码 .auc
#   scripts/build-aura-compiler.sh --aot       # 产出原生可执行文件（需 LLVM）
#   scripts/build-aura-compiler.sh --help
#
# 约束：Rust 编译器（compiler/）零修改，本脚本只读取它、不改动源码。
# ─────────────────────────────────────────────────────────────
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

ENTRY="aura/compiler/aura/lang/compiler/Main.aura"
OUT_DIR="build"

AOT=0
for arg in "$@"; do
    case "$arg" in
        --aot) AOT=1 ;;
        -h|--help)
            echo "用法: scripts/build-aura-compiler.sh [--aot]"
            echo "  --aot   使用 LLVM 后端产出原生可执行文件（默认产出 .auc 字节码）"
            exit 0
            ;;
        *)
            echo "未知参数: $arg" >&2
            exit 1
            ;;
    esac
done

if [ ! -f "$ENTRY" ]; then
    echo "[build-aura-compiler] 错误: 找不到编译器入口 $ENTRY" >&2
    exit 1
fi

# ── 定位 Rust 编译器可执行文件 ──────────────────────────────
find_aura() {
    for candidate in \
        "target/release/aura.exe" "target/release/aura" \
        "target/debug/aura.exe" "target/debug/aura"; do
        if [ -x "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

AURA="$(find_aura || true)"
if [ -z "$AURA" ]; then
    echo "[build-aura-compiler] 未找到 aura 可执行文件，正在用 cargo 构建 Rust 编译器..."
    cargo build --release --manifest-path compiler/Cargo.toml
    AURA="$(find_aura || true)"
fi

if [ -z "$AURA" ]; then
    echo "[build-aura-compiler] 错误: 构建后仍未找到 aura 可执行文件" >&2
    exit 1
fi

echo "[build-aura-compiler] Rust 编译器: $AURA"
echo "[build-aura-compiler] 入口源码:   $ENTRY"

mkdir -p "$OUT_DIR"

if [ "$AOT" = "1" ]; then
    OUT="$OUT_DIR/aura-compiler"
    echo "[build-aura-compiler] 模式: AOT（LLVM） → $OUT"
    "$AURA" build "$ENTRY" --aot --output "$OUT"
else
    OUT="$OUT_DIR/aura-compiler.auc"
    echo "[build-aura-compiler] 模式: 字节码 → $OUT"
    "$AURA" build "$ENTRY" --output "$OUT"
fi

echo "[build-aura-compiler] ✓ 构建完成: $OUT"
