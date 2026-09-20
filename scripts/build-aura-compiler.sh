#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# 构建 Aura 编译器（自举模式）
#
# 产物集中放在：
#   build/bin/             ← Bootstrap 编译器 (从 seed 或 Rust 构建)
#   build/auc/compiler/    ← 「Aura 编写的 Aura 编译器」字节码 (.auc)
#                            或 AOT 原生可执行文件 (.exe)（--aot，需 LLVM）
#
# 用法：
#   scripts/build-aura-compiler.sh              # 默认产出字节码 .auc
#   scripts/build-aura-compiler.sh --aot        # 产出原生可执行文件（需 LLVM）
#   scripts/build-aura-compiler.sh --no-bootstrap  # 跳过 bootstrap 拷贝
#   scripts/build-aura-compiler.sh --rebuild-seed  # 从 Rust 源码重建 seed
#   scripts/build-aura-compiler.sh --help
#
# Bootstrap 解析顺序：
#   1. aura/seed/aura(.exe)          ← 预编译种子（优先，无需 Rust 工具链）
#   2. target/release/aura(.exe)     ← 本地 Rust 构建
#   3. cargo build                   ← 从 Rust 源码重建
# ─────────────────────────────────────────────────────────────
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

ENTRY="aura/compiler/aura/lang/compiler/Main.aura"
BIN_DIR="build/bin"
AUC_DIR="build/auc/compiler"

AOT=0
NO_BOOTSTRAP=0
REBUILD_SEED=0
for arg in "$@"; do
    case "$arg" in
        --aot) AOT=1 ;;
        --no-bootstrap) NO_BOOTSTRAP=1 ;;
        --rebuild-seed) REBUILD_SEED=1 ;;
        -h|--help)
            echo "Usage: scripts/build-aura-compiler.sh [--aot] [--no-bootstrap] [--rebuild-seed]"
            echo "  --aot            Produce native executable via LLVM backend (default: .auc bytecode)"
            echo "  --no-bootstrap   Do not copy bootstrap aura binary to build/bin"
            echo "  --rebuild-seed   Rebuild aura/seed/aura from Rust source"
            echo ""
            echo "Bootstrap resolution order:"
            echo "  1. aura/seed/aura(.exe)          (pre-built seed, preferred)"
            echo "  2. target/release/aura(.exe)     (local Rust build)"
            echo "  3. cargo build                   (if neither exists)"
            echo ""
            echo "Outputs:"
            echo "  build/bin/aura(.exe)                     Bootstrap compiler"
            echo "  build/auc/compiler/aura-compiler.(auc|exe)   Aura-written compiler"
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

# ── 定位 Bootstrap 可执行文件 ────────────────────────────────
# 优先使用预编译的种子文件（无需 Rust 工具链）
find_aura() {
    # 1. 预编译种子文件（优先）
    if [ -x "aura/seed/aura" ]; then
        echo "aura/seed/aura"
        return 0
    fi
    if [ -x "aura/seed/aura.exe" ]; then
        echo "aura/seed/aura.exe"
        return 0
    fi
    # 2. 本地 Rust 构建
    for candidate in \
        "target/release/aura" "target/release/aura.exe" \
        "target/debug/aura" "target/debug/aura.exe"; do
        if [ -x "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

# 从 Rust 源码重建种子文件
rebuild_seed() {
    echo "[build-aura-compiler] Rebuilding seed from Rust source..."
    if [ ! -f "compiler/Cargo.toml" ]; then
        echo "[build-aura-compiler] ERROR: compiler/Cargo.toml not found, cannot rebuild seed" >&2
        exit 1
    fi
    cargo build --release -p cli --features llvm
    mkdir -p "aura/seed"
    if [ -f "target/release/aura" ]; then
        cp -f "target/release/aura" "aura/seed/aura"
    elif [ -f "target/release/aura.exe" ]; then
        cp -f "target/release/aura.exe" "aura/seed/aura.exe"
    fi
    echo "[build-aura-compiler] seed rebuilt -> aura/seed/aura"
}

# 重建种子文件（可选）
if [ "$REBUILD_SEED" = "1" ]; then
    rebuild_seed
    exit 0
fi

AURA="$(find_aura || true)"

if [ "$AOT" = "1" ]; then
    # AOT 需要 LLVM 后端：确保种子文件支持，否则重建
    if [ "$AURA" = "aura/seed/aura" ] || [ "$AURA" = "aura/seed/aura.exe" ]; then
        echo "[build-aura-compiler] AOT mode: seed may lack LLVM support, rebuilding..."
        if [ ! -f "compiler/Cargo.toml" ]; then
            echo "[build-aura-compiler] ERROR: compiler/Cargo.toml not found, cannot rebuild for AOT" >&2
            exit 1
        fi
        cargo build --release -p cli --features llvm
        if [ -f "target/release/aura" ]; then
            AURA="target/release/aura"
        elif [ -f "target/release/aura.exe" ]; then
            AURA="target/release/aura.exe"
        fi
    fi
else
    if [ -z "$AURA" ]; then
        echo "[build-aura-compiler] no bootstrap found, attempting cargo build..."
        if [ ! -f "compiler/Cargo.toml" ]; then
            echo "[build-aura-compiler] ERROR: compiler/Cargo.toml not found" >&2
            exit 1
        fi
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
echo "[build-aura-compiler] bin dir:           $BIN_DIR"
echo "[build-aura-compiler] auc dir:           $AUC_DIR"

mkdir -p "$BIN_DIR" "$AUC_DIR"

# 1) 最小 bootstrap 编译器 → build/bin/aura(.exe)
if [ "$NO_BOOTSTRAP" = "0" ]; then
    case "$AURA" in
        *.exe) BOOTSTRAP_OUT="$BIN_DIR/aura.exe" ;;
        *)     BOOTSTRAP_OUT="$BIN_DIR/aura" ;;
    esac
    cp -f "$AURA" "$BOOTSTRAP_OUT"
    echo "[build-aura-compiler] bootstrap → $BOOTSTRAP_OUT"
fi

# 2) Aura 编写的编译器 → build/auc/compiler/aura-compiler.(auc|exe)
if [ "$AOT" = "1" ]; then
    OUT="$AUC_DIR/aura-compiler"
    [ -x "$BIN_DIR/aura.exe" ] && OUT="$AUC_DIR/aura-compiler.exe"
    echo "[build-aura-compiler] mode: AOT (LLVM) → $OUT"
    "$AURA" build "$ENTRY" --aot --output "$OUT"
else
    OUT="$AUC_DIR/aura-compiler.auc"
    echo "[build-aura-compiler] mode: bytecode → $OUT"
    "$AURA" build "$ENTRY" --output "$OUT"
fi

echo "[build-aura-compiler] ✓ Build complete: $OUT"
