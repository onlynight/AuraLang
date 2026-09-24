#!/usr/bin/env bash
# -------------------------------------------------------------
# Build the Aura core standard library (Aura → .auc bytecode)
#
# Compiles all .aura files under aura/core/ into .auc bytecode,
# preserving the directory structure so each class lands in its
# own .auc file (its own package):
#
#   aura/core/aura/lang/String.aura
#       → build/aura_core_auc/aura/lang/String.auc
#   aura/core/aura/lang/std/Math.aura
#       → build/aura_core_auc/aura/lang/std/Math.auc
#   aura/core/aura/lang/concurrent/Mutex.aura
#       → build/aura_core_auc/aura/lang/concurrent/Mutex.auc
#
# Output: build/aura_core_auc/  (directory mirrors aura/core/)
#
# Usage:
#   scripts/build-aura-core.sh
#   scripts/build-aura-core.sh --help
# -------------------------------------------------------------
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

CORE_DIR="aura/core"
OUT_DIR="build/aura_core_auc"

# ---- locate bootstrap compiler -------------------------------------------
find_aura() {
    for candidate in \
        "rust/target/release/aura" "rust/target/release/aura.exe" \
        "rust/target/debug/aura"   "rust/target/debug/aura.exe" \
        "build/bin/aura"           "build/bin/aura.exe" \
        "aura/seed/aura"           "aura/seed/aura.exe"; do
        if [ -x "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

if [ "$#" -gt 0 ] && [ "$1" = "--help" ]; then
    echo "Usage: scripts/build-aura-core.sh [--help]"
    echo ""
    echo "Pre-compile the Aura core standard library (aura/core/**/*.aura) into .auc"
    echo "bytecode files under build/aura_core_auc/, mirroring the source directory"
    echo "structure so each class gets its own .auc file (package)."
    echo ""
    echo "Bootstrap compiler resolution:"
    echo "  1. rust/target/{release,debug}/aura"
    echo "  2. build/bin/aura"
    echo "  3. aura/seed/aura"
    echo ""
    echo "Outputs:"
    echo "  build/aura_core_auc/aura/lang/**/*.auc   (mirrors aura/core/)"
    exit 0
fi

AURA="$(find_aura || true)"

if [ -z "$AURA" ]; then
    echo "[build-aura-core] ERROR: no bootstrap compiler found" >&2
    echo "  Build it with:  cd rust && cargo build -p cli --features llvm --release" >&2
    exit 1
fi

if [ ! -d "$CORE_DIR" ]; then
    echo "[build-aura-core] ERROR: source directory not found: $CORE_DIR" >&2
    exit 1
fi

echo "[build-aura-core] compiler: $AURA"
echo "[build-aura-core] source:   $CORE_DIR"
echo "[build-aura-core] output:   $OUT_DIR"

# ---- run stdlib-compile ---------------------------------------------------
"$AURA" stdlib-compile "$CORE_DIR" --output "$OUT_DIR"

# ---- artifact summary -----------------------------------------------------
AUC_COUNT=$(find "$OUT_DIR" -name "*.auc" -type f 2>/dev/null | wc -l)
AUC_SIZE=$(find "$OUT_DIR" -name "*.auc" -type f -exec stat -c%s {} + 2>/dev/null | awk '{s+=$1} END {printf "%.2f", s/1048576}')

if [ "$AUC_COUNT" -eq 0 ]; then
    echo "[build-aura-core] WARNING: no .auc files produced" >&2
    exit 1
fi

echo "[build-aura-core] OK: $AUC_COUNT .auc files, ${AUC_SIZE} MB total"
echo "[build-aura-core] output: $OUT_DIR"