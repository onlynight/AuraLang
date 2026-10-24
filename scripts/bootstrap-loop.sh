#!/usr/bin/env bash
# -------------------------------------------------------------
# P4.1 Bootstrap closed loop (VM-PA-00 v3.2, D6)
#
#   Stage-1  Rust seed compiles the Aura compiler source (Main.aura)
#            into the compiler artifact  build/bootstrap/stage1.auc
#   Stage-2  The Aura VM (pure-Aura VmRunner, AOT-compiled to
#            build/bin/auc_vm_runner.exe) EXECUTES stage1.auc; the
#            compiler running on the Aura VM compiles Main.aura again
#            (AOT backend) -> build/bootstrap/stage2.exe
#   Stage-3  stage2.exe (the self-hosted compiler) compiles Main.aura
#            once more -> build/bootstrap/stage3.exe
#   Check    All three substrates (Aura VM, stage2.exe, stage3.exe)
#            compile the same sample in jit mode; outputs must be
#            byte-identical.
#
# Substrate note: the meta-circular route (Aura VM interpreted by the
# Rust VM) cannot host the 1.4 MB compiler .auc in reasonable time
# (interpreter-under-interpreter). The AOT-compiled Aura VM runner is
# the same pure-Aura VM implementation at native speed; the Rust VM
# is not involved in Stages 2-3.
#
# Usage: bash scripts/bootstrap-loop.sh
# -------------------------------------------------------------
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MAIN_SRC="aura/compiler/aura/lang/compiler/Main.aura"
SAMPLE="tests/helloworld.aura"
BOOT="build/bootstrap"
AUC_CHANNEL="build/vm/current.auc"
RUNNER="build/bin/auc_vm_runner.exe"

# resolve the seed compiler (same order as diff-test.ps1)
SEED=""
for c in "seed/target/release/aura.exe" "seed/target/debug/aura.exe" "build/bin/aura.exe" "aura/seed/aura.exe"; do
    if [ -f "$c" ]; then SEED="$c"; break; fi
done
if [ -z "$SEED" ]; then
    echo "[bootstrap] ERROR: no seed compiler found (cd seed && cargo build -p compiler --features llvm --release)"
    exit 1
fi

say() { echo "[bootstrap] $*"; }
fail() { echo "[bootstrap] FAIL: $*"; exit 1; }

mkdir -p "$BOOT" "build/vm"

# ── Stage-1: seed compiles the compiler ──────────────────────────
say "Stage-1: seed compiles Main.aura -> $BOOT/stage1.auc"
"$SEED" build "$MAIN_SRC" --output "$BOOT/stage1.auc" > "$BOOT/stage1.log" 2>&1
[ -f "$BOOT/stage1.auc" ] || fail "Stage-1: stage1.auc not produced (see $BOOT/stage1.log)"
say "Stage-1 OK: $(ls -la "$BOOT/stage1.auc" | awk '{print $5}') bytes"

# ── Stage-2: Aura VM executes stage1.auc, compiler recompiles itself ──
[ -f "$RUNNER" ] || fail "Aura VM runner missing: $RUNNER (build: $SEED build examples/compiler/auc_vm_runner.aura --output $RUNNER --aot)"
cp "$BOOT/stage1.auc" "$AUC_CHANNEL"
say "Stage-2: Aura VM executes stage1.auc; compiler AOT-compiles Main.aura -> stage2.exe"
"./$RUNNER" "$MAIN_SRC" --output "$BOOT/stage2" > "$BOOT/stage2.log" 2>&1
ST2=$?
if [ $ST2 -ne 0 ]; then
    tail -20 "$BOOT/stage2.log" | grep -v "semantic warning" || true
    fail "Stage-2: compiler-on-AuraVM exited $ST2 (see $BOOT/stage2.log)"
fi
[ -f "$BOOT/stage2.exe" ] || fail "Stage-2: stage2.exe not produced (see $BOOT/stage2.log)"
say "Stage-2 OK: stage2.exe $(ls -la "$BOOT/stage2.exe" | awk '{print $5}') bytes"

# ── Stage-3: the self-hosted compiler compiles itself once more ──
say "Stage-3: stage2.exe compiles Main.aura -> stage3.exe"
"./$BOOT/stage2.exe" "$MAIN_SRC" --output "$BOOT/stage3" > "$BOOT/stage3.log" 2>&1
ST3=$?
if [ $ST3 -ne 0 ]; then
    tail -20 "$BOOT/stage3.log" | grep -v "semantic warning" || true
    fail "Stage-3: stage2.exe exited $ST3 (see $BOOT/stage3.log)"
fi
[ -f "$BOOT/stage3.exe" ] || fail "Stage-3: stage3.exe not produced (see $BOOT/stage3.log)"
say "Stage-3 OK: stage3.exe $(ls -la "$BOOT/stage3.exe" | awk '{print $5}') bytes"

# ── Differential check: three substrates, one compile task ──────
say "Differential: jit-compile $SAMPLE on Aura VM / stage2.exe / stage3.exe"
"./$RUNNER" "$SAMPLE" -b jit 2>/dev/null > "$BOOT/diff_vm.txt"
"./$BOOT/stage2.exe" "$SAMPLE" -b jit 2>/dev/null > "$BOOT/diff_s2.txt"
"./$BOOT/stage3.exe" "$SAMPLE" -b jit 2>/dev/null > "$BOOT/diff_s3.txt"

ok=1
if ! cmp -s "$BOOT/diff_vm.txt" "$BOOT/diff_s2.txt"; then ok=0; say "MISMATCH: Aura VM vs stage2.exe (diff_vm.txt / diff_s2.txt)"; fi
if ! cmp -s "$BOOT/diff_vm.txt" "$BOOT/diff_s3.txt"; then ok=0; say "MISMATCH: Aura VM vs stage3.exe (diff_vm.txt / diff_s3.txt)"; fi
[ "$ok" -eq 1 ] || fail "differential mismatch across bootstrap generations"
say "Differential OK: $(wc -l < "$BOOT/diff_vm.txt") lines, byte-identical across 3 substrates"

echo "=== [bootstrap] Stage-1/2/3 PASSED (self-host loop closed, D6) ==="
