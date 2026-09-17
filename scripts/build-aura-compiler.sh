#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# 构建 Aura 编译器（纯 Aura 化迁移）
#
# 产物集中放在 build/bin/：
#   aura.exe            ← Rust 最小 bootstrap 编译器
#   aura-compiler.auc   ← 「Aura 编写的 Aura 编译器」字节码（默认）
#   aura-compiler.exe   ← 同上，AOT 原生可执行文件（--aot，需 LLVM）
#
# 用法：
#   scripts/build-aura-compiler.sh             # 默认产出字节码 .auc
#   scripts/build-aura-compiler.sh --aot       # 产出原生可执行文件（需 LLVM）
#   scripts/build-aura-compiler.sh --no-bootstrap  # 跳过 bootstrap 拷贝
#   scripts/build-aura-compiler.sh --help
#
# 约束：Rust 编译器（compiler/）零修改，本脚本只读取它、不改动源码。
# ─────────────────────────────────────────────────────────────
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

ENTRY="aura/compiler/aura/lang/compiler/Main.aura"
OUT_DIR="build/bin"

AOT=0
NO_BOOTSTRAP=0
for arg in "$@"; do
    case "$arg" in
        --aot) AOT=1 ;;
        --no-bootstrap) NO_BOOTSTRAP=1 ;;
        -h|--help)
            echo "Usage: scripts/build-aura-compiler.sh [--aot] [--no-bootstrap]"
            echo "  --aot            Produce native executable via LLVM backend (default: .auc bytecode)"
            echo "  --no-bootstrap   Do not copy bootstrap aura binary to build/bin"
            echo ""
            echo "Outputs (build/bin): aura.exe, aura-compiler.(auc|exe)"
            exit 0
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            exit 1
            ;;
    esac
done

if [ ! -f "$ENTRY" ]; then
    echo "[build-aura-compiler] ERROR: compiler entry not found: $ENTRY" >&2
    exit 1
fi

# ── 定位 Rust bootstrap 可执行文件 ──────────────────────────
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

# AOT 需要 LLVM 后端：bootstrap 必须以 `--features llvm` 构建。
build_bootstrap() {
    echo "[build-aura-compiler] Building Rust bootstrap: cargo build --release -p cli --features llvm"
    cargo build --release -p cli --features llvm
}

if [ "$AOT" = "1" ]; then
    build_bootstrap
    AURA="$(find_aura || true)"
else
    AURA="$(find_aura || true)"
    if [ -z "$AURA" ]; then
        echo "[build-aura-compiler] aura executable not found, building Rust compiler via cargo..."
        cargo build --release --manifest-path compiler/Cargo.toml
        AURA="$(find_aura || true)"
    fi
fi

if [ -z "$AURA" ]; then
    echo "[build-aura-compiler] ERROR: aura executable still not found after build" >&2
    exit 1
fi

echo "[build-aura-compiler] Rust bootstrap: $AURA"
echo "[build-aura-compiler] entry source:      $ENTRY"
echo "[build-aura-compiler] output dir:      $OUT_DIR"

mkdir -p "$OUT_DIR"

# 1) 最小 bootstrap 编译器 → build/bin/aura(.exe)
if [ "$NO_BOOTSTRAP" = "0" ]; then
    case "$AURA" in
        *.exe) BOOTSTRAP_OUT="$OUT_DIR/aura.exe" ;;
        *)     BOOTSTRAP_OUT="$OUT_DIR/aura" ;;
    esac
    cp -f "$AURA" "$BOOTSTRAP_OUT"
    echo "[build-aura-compiler] bootstrap → $BOOTSTRAP_OUT"
fi

# 2) Aura 编写的编译器 → build/bin/aura-compiler.(auc|exe)
if [ "$AOT" = "1" ]; then
    OUT="$OUT_DIR/aura-compiler"
    [ -x "$OUT_DIR/aura.exe" ] && OUT="$OUT_DIR/aura-compiler.exe"
    echo "[build-aura-compiler] mode: AOT (LLVM) → $OUT"
    "$AURA" build "$ENTRY" --aot --output "$OUT"
else
    OUT="$OUT_DIR/aura-compiler.auc"
    echo "[build-aura-compiler] mode: bytecode → $OUT"
    "$AURA" build "$ENTRY" --output "$OUT"
fi

echo "[build-aura-compiler] ✓ Build complete: $OUT"
