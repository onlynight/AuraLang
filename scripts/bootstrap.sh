#!/bin/bash
# 完全 Aura 化自举验证脚本 (Linux/macOS)
# 验证流程：最小 aura.exe → 编译 vm.aura → 用 vm.exe 重新编译 → 验证行为一致性
set -e

# ── 配置 ──────────────────────────────────────────────────────────
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CDIR="$ROOT_DIR"
AURA_BIN="$ROOT_DIR/target/release/aura.exe"
AURA_BIN_DEBUG="$ROOT_DIR/target/debug/aura.exe"
VM_OUT="$ROOT_DIR/vm.exe"
VM2_OUT="$ROOT_DIR/vm2.exe"
GC_OUT="$ROOT_DIR/gc.exe"
GC2_OUT="$ROOT_DIR/gc2.exe"
MEM_OUT="$ROOT_DIR/memory.exe"
TEST_DIR="$ROOT_DIR/tests/self_bootstrap"
OUT1="$ROOT_DIR/output1.txt"
OUT2="$ROOT_DIR/output2.txt"

cd "$CDIR"

# ── 辅助函数 ──────────────────────────────────────────────────────
find_aura() {
    if [ -x "$AURA_BIN" ]; then
        echo "$AURA_BIN"
    elif [ -x "$AURA_BIN_DEBUG" ]; then
        echo "$AURA_BIN_DEBUG"
    else
        echo ""
    fi
}

check_file() {
    if [ ! -f "$1" ]; then
        echo "✗ 文件不存在: $1"
        echo "  请先完成 core/aura/lang/std/ 的编写"
        exit 1
    fi
}

# ── 阶段 0: 环境检查 ──────────────────────────────────────────────
echo "=== 阶段 0: 环境检查 ==="
check_file "compiler/Cargo.toml"

AURA=$(find_aura)
if [ -z "$AURA" ]; then
    echo "  使用 cargo build --release 编译 aura.exe..."
    AURA="$AURA_BIN"
fi

# ── 阶段 1: 编译最小 aura.exe（Rust） ─────────────────────────────
echo "=== 阶段 1: 编译最小 aura.exe ==="
echo "  运行: cargo build --release --manifest-path compiler/Cargo.toml"
cargo build --release --manifest-path compiler/Cargo.toml
AURA="$AURA_BIN"

# ── 阶段 2: 用最小 aura.exe 编译 core/aura/lang/std/ ───────────────
echo "=== 阶段 2: 用最小 aura.exe 编译 vm.aura / gc.aura / memory.aura ==="
check_file "core/aura/lang/std/vm/vm.aura"
check_file "core/aura/lang/std/gc/gc.aura"
check_file "core/aura/lang/std/memory/memory.aura"

echo "  编译 vm.aura → vm.exe..."
"$AURA" build "core/aura/lang/std/vm/vm.aura" --aot --output "$VM_OUT"

echo "  编译 gc.aura → gc.exe..."
"$AURA" build "core/aura/lang/std/gc/gc.aura" --aot --output "$GC_OUT"

echo "  编译 memory.aura → memory.exe..."
"$AURA" build "core/aura/lang/std/memory/memory.aura" --aot --output "$MEM_OUT"

# ── 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证） ────────────────
echo "=== 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证）==="
check_file "$VM_OUT"

echo "  用 vm.exe 编译 vm.aura → vm2.exe..."
"$VM_OUT" build "core/aura/lang/std/vm/vm.aura" --aot --output "$VM2_OUT"

echo "  用 vm2.exe 编译 gc.aura → gc2.exe..."
"$VM2_OUT" build "core/aura/lang/std/gc/gc.aura" --aot --output "$GC2_OUT"

# ── 阶段 4: 验证行为一致性 ────────────────────────────────────────
echo "=== 阶段 4: 验证行为一致性 ==="
check_file "$TEST_DIR/vm_test.aura"

echo "  vm.exe run vm_test.aura → output1.txt..."
"$VM_OUT" run "$TEST_DIR/vm_test.aura" > "$OUT1"

echo "  vm2.exe run vm_test.aura → output2.txt..."
"$VM2_OUT" run "$TEST_DIR/vm_test.aura" > "$OUT2"

if diff -q "$OUT1" "$OUT2" > /dev/null 2>&1; then
    echo "  ✓ 行为一致: vm.exe 与 vm2.exe 输出相同"
    echo "  ✓ 自举验证通过"
else
    echo "  ✗ 行为不一致:"
    diff "$OUT1" "$OUT2" || true
    echo "  ✗ 自举验证失败"
    exit 1
fi

# ── 阶段 5: 验证性能 ──────────────────────────────────────────────
echo "=== 阶段 5: 验证性能 ==="
check_file "$TEST_DIR/performance_test.aura"

echo "  运行 performance_test.aura (vm.exe)..."
TIME1=$( { /usr/bin/time -f "%e" "$VM_OUT" run "$TEST_DIR/performance_test.aura" > /dev/null 2>&1; } 2>&1 )
TIME1_VAL=$(echo "$TIME1" | awk '{print $1}')

echo "  运行 performance_test.aura (vm2.exe)..."
TIME2=$( { /usr/bin/time -f "%e" "$VM2_OUT" run "$TEST_DIR/performance_test.aura" > /dev/null 2>&1; } 2>&1 )
TIME2_VAL=$(echo "$TIME2" | awk '{print $1}')

echo "  vm.exe 耗时:  ${TIME1_VAL}s"
echo "  vm2.exe 耗时: ${TIME2_VAL}s"

# 性能差异检查（5% 容差）
PERF_DIFF=$(echo "$TIME1_VAL $TIME2_VAL" | awk '{
    if ($1 > 0) {
        diff = ($1 - $2) / $1 * 100
        if (diff < 0) diff = -diff
        printf "%.2f", diff
    } else {
        print "0.00"
    }
}')

echo "  性能差异: ${PERF_DIFF}%"

if [ "$(echo "$PERF_DIFF < 5.0" | bc -l 2>/dev/null || echo "0")" = "1" ]; then
    echo "  ✓ 性能一致（差异 < 5%）"
else
    echo "  ⚠ 性能差异超过 5%，请检查"
    echo "  注意: 性能差异检查为参考指标，不影响验证结果"
fi

# ── 阶段 6: 替换（可选，默认不替换） ──────────────────────────────
echo "=== 阶段 6: 验证总结 ==="
echo "  vm.exe  →  $VM_OUT"
echo "  vm2.exe →  $VM2_OUT"
echo "  gc.exe  →  $GC_OUT"
echo "  gc2.exe →  $GC2_OUT"
echo "  memory.exe → $MEM_OUT"
echo ""

# 备份原始 aura.exe
if [ -f "$AURA_BIN" ]; then
    cp "$AURA_BIN" "${AURA_BIN}.bak"
    echo "  已备份原始 aura.exe → ${AURA_BIN}.bak"
fi

# 替换（仅当设置 AUTO_REPLACE=true 时执行）
if [ "${AUTO_REPLACE:-false}" = "true" ]; then
    echo "  AUTO_REPLACE=true，正在替换..."
    cp "$VM_OUT" "$AURA_BIN"
    cp "$GC_OUT" "$ROOT_DIR/gc.exe"
    cp "$MEM_OUT" "$ROOT_DIR/memory.exe"
    echo "  ✓ 替换完成"
else
    echo "  未设置 AUTO_REPLACE=true，跳过自动替换"
    echo "  如需替换，请手动执行:"
    echo "    cp vm.exe target/release/aura.exe"
    echo "    cp gc.exe target/release/gc.exe"
    echo "    cp memory.exe target/release/memory.exe"
fi

echo ""
echo "═══════════════════════════════════════════════════════"
echo "  自举验证完成！"
echo "  行为一致性: ✓"
echo "  性能一致性: ${PERF_DIFF}% 差异"
echo "═══════════════════════════════════════════════════════"
echo ""
echo "清理临时文件:"
rm -f "$OUT1" "$OUT2"
echo "  已删除 output1.txt, output2.txt"

echo ""
echo "验证完成时间: $(date)"
