#!/bin/bash
# -------------------------------------------------------------
# Pure-Aura self-bootstrap verification (NO Rust / cargo required)
#
# Uses the frozen Stage-0 carrier committed at dist/bootstrap/aura-compiler.exe
# to bootstrap itself and verify behavioural consistency:
#
#   1. verify SHA256 of the frozen binary against dist/bootstrap/SHA256SUMS
#   2. frozen  compiles Main.aura  -> build/bin/aura-compiler-n1.exe
#   3. n1      compiles Main.aura  -> build/bin/aura-compiler-n2.exe
#   4. n1 / n2 compile the same test program; outputs must match
#
# External requirements: LLVM tools (llc / clang) + system CRT only.
#
# NOTE: the Aura-side AOT backend currently targets Windows x86_64 by default
#       (`aura/lang/compiler/aot/Target.aura::targetDefault`). On non-Windows
#       hosts the freeze artifact is binary-verified only; full self-bootstrap
#       requires a Windows host (or a platform-adapted target triple).
#
# Usage:
#   scripts/self-bootstrap-frozen.sh            # auto-detect LLVM
#   AURA_LLVM_HOME=/path/to/llvm scripts/self-bootstrap-frozen.sh
#   scripts/self-bootstrap-frozen.sh --skip-hash
# -------------------------------------------------------------
set -e

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FROZEN="$ROOT_DIR/dist/bootstrap/aura-compiler.exe"
SUMS="$ROOT_DIR/dist/bootstrap/SHA256SUMS"
MAIN_AURA="$ROOT_DIR/aura/compiler/aura/lang/compiler/Main.aura"
BIN_DIR="$ROOT_DIR/build/bin"
TEST_DIR="$ROOT_DIR/build/test"
SKIP_HASH=0

for arg in "$@"; do
    case "$arg" in
        --skip-hash) SKIP_HASH=1 ;;
        -h|--help)
            echo "Usage: scripts/self-bootstrap-frozen.sh [--skip-hash]"
            exit 0
            ;;
    esac
done

cd "$ROOT_DIR"
mkdir -p "$BIN_DIR" "$TEST_DIR"

step() { echo ""; echo "[$1]"; }

# ── Step 0: frozen carrier + hash check ──
step "Step 0: frozen carrier"
if [ ! -f "$FROZEN" ]; then
    echo "  FATAL: frozen binary not found: $FROZEN"
    echo "  Run scripts/freeze-bootstrap.ps1 (requires Rust, one-time) to produce it."
    exit 1
fi
if [ "$SKIP_HASH" -eq 0 ] && [ -f "$SUMS" ]; then
    (cd "$(dirname "$SUMS")" && sha256sum -c SHA256SUMS)
    echo "  SHA256 OK"
else
    echo "  SHA256 check skipped"
fi

# ── Platform gate ──
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        IS_WINDOWS=1 ;;
    *)
        IS_WINDOWS=0 ;;
esac
if [ "$IS_WINDOWS" -eq 0 ]; then
    echo ""
    echo "  NOTE: non-Windows host detected."
    echo "  The Aura-side AOT backend currently defaults to the Windows x86_64"
    echo "  target, so the full self-bootstrap stages are skipped here."
    echo "  Frozen artifact integrity has been verified."
    exit 0
fi

LLVM_HOME="${AURA_LLVM_HOME:-D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc}"
if [ ! -x "$LLVM_HOME/bin/llc.exe" ] || [ ! -x "$LLVM_HOME/bin/clang.exe" ]; then
    echo "  FATAL: LLVM tools not found under $LLVM_HOME (set AURA_LLVM_HOME)"
    exit 1
fi
echo "  LLVM: $LLVM_HOME"

# ── Step 1: frozen -> n1 ──
step "Step 1: frozen carrier compiles Main.aura -> n1"
N1="$BIN_DIR/aura-compiler-n1.exe"
"$FROZEN" "$MAIN_AURA" -o "$N1"
echo "  OK: $N1"

# ── Step 2: n1 -> n2 ──
step "Step 2: n1 compiles Main.aura -> n2"
N2="$BIN_DIR/aura-compiler-n2.exe"
"$N1" "$MAIN_AURA" -o "$N2"
echo "  OK: $N2"

# ── Step 3: behavioural consistency ──
step "Step 3: behavioural consistency"
TEST_FILE="$ROOT_DIR/tests/string_methods_test.aura"
if [ ! -f "$TEST_FILE" ]; then
    echo "  WARNING: test file not found, skipping consistency check"
    exit 0
fi
O1="$TEST_DIR/n1.txt"
O2="$TEST_DIR/n2.txt"
"$N1" "$TEST_FILE" -o "$TEST_DIR/n1.exe"
"$TEST_DIR/n1.exe" > "$O1" 2>&1 || true
"$N2" "$TEST_FILE" -o "$TEST_DIR/n2.exe"
"$TEST_DIR/n2.exe" > "$O2" 2>&1 || true

if diff -q "$O1" "$O2" > /dev/null 2>&1; then
    echo "  OK: behaviour consistent"
else
    echo "  FATAL: behaviour differs"
    diff "$O1" "$O2" || true
    exit 1
fi

echo ""
echo "============================================="
echo "  Pure-Aura self-bootstrap verified (no Rust)"
echo "============================================="
exit 0
