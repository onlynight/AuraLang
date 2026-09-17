#!/bin/bash
# Phase C.4: Aura 编译器自举验证脚本 (Linux/macOS)
# 验证流程：Rust AOT → aura-compiler-native.exe → 自举编译 → 行为一致性验证
set -e

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
AURA_BIN="$ROOT_DIR/target/release/aura"
MAIN_AURA="$ROOT_DIR/aura/compiler/aura/lang/compiler/Main.aura"
NATIVE_OUT="$ROOT_DIR/build/bin/aura-compiler-native"
NATIVE2_OUT="$ROOT_DIR/build/bin/aura-compiler-native2"
TEST_DIR="$ROOT_DIR/tests"
BUILD_DIR="$ROOT_DIR/build/test"
OUT1="$BUILD_DIR/output1.txt"
OUT2="$BUILD_DIR/output2.txt"

cd "$ROOT_DIR"
mkdir -p "$ROOT_DIR/build/bin" "$BUILD_DIR"

# ── 阶段 0: 环境检查 ──
echo "=== Phase 0: Environment check ==="
if [ ! -x "$AURA_BIN" ]; then
    echo "  Building aura..."
    cargo build --release -p cli --features llvm
fi
if [ ! -f "$MAIN_AURA" ]; then
    echo "✗ Main.aura not found: $MAIN_AURA"; exit 1
fi

# ── 阶段 1: 编译原生载体（Rust AOT 后端） ──
echo "=== Phase 1: Compile native carrier (Rust AOT) ==="
$AURA_BIN build "$MAIN_AURA" --aot -o "$NATIVE_OUT"
echo "  ✓ $NATIVE_OUT generated"

# ── 阶段 2: 用原生载体重新编译自身（Aura 侧 AOT 后端） ──
echo "=== Phase 2: Self-bootstrap (Aura AOT) ==="
$NATIVE_OUT "$MAIN_AURA" -o "$NATIVE2_OUT"
echo "  ✓ $NATIVE2_OUT generated"

# ── 阶段 3: 行为一致性验证 ──
echo "=== Phase 3: Behavioral consistency ==="
TEST_FILE="$TEST_DIR/pure_aura/hir_b1_when_tests.aura"
if [ ! -f "$TEST_FILE" ]; then
    TEST_FILE="$TEST_DIR/string_methods_test.aura"
fi

$NATIVE_OUT "$TEST_FILE" -o "$BUILD_DIR/native1"
$BUILD_DIR/native1 > "$OUT1"

$NATIVE2_OUT "$TEST_FILE" -o "$BUILD_DIR/native2"
$BUILD_DIR/native2 > "$OUT2"

if diff -q "$OUT1" "$OUT2" > /dev/null 2>&1; then
    echo "  ✓ Behavior consistent"
else
    echo "  ✗ Behavior inconsistent:"
    diff "$OUT1" "$OUT2" || true
    exit 1
fi

# ── 阶段 4: 性能对比 ──
echo "=== Phase 4: Performance comparison ==="
T1=$({ /usr/bin/time -f "%e" $NATIVE_OUT "$MAIN_AURA" -o /dev/null 2>&1; } 2>&1)
T1_VAL=$(echo "$T1" | awk '{print $1}')
T2=$({ /usr/bin/time -f "%e" $NATIVE2_OUT "$MAIN_AURA" -o /dev/null 2>&1; } 2>&1)
T2_VAL=$(echo "$T2" | awk '{print $1}')
DIFF=$(echo "$T1_VAL $T2_VAL" | awk '{if($1>0){d=($1-$2)/$1*100; if(d<0)d=-d; printf "%.2f",d}else{print "0.00"}}')

echo "  native1: ${T1_VAL}s"
echo "  native2: ${T2_VAL}s"
echo "  diff:    ${DIFF}%"

# ── 总结 ──
echo ""
echo "═══════════════════════════════════════════════════════"
echo "  Self-bootstrap verification complete!"
echo "  Behavior consistency: ✓"
echo "  Performance diff: ${DIFF}%"
echo "═══════════════════════════════════════════════════════"