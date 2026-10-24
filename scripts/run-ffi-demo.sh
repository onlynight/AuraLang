#!/usr/bin/env bash
# P3.4 — FFI Demo 端到端验证（C FFI + AOT 直调）。
#
# 复现 `examples/ext_ffi_demo` 的两条调用链，均在 **Aura VM** 下运行：
#
#   Demo 1 (demo_cffi)       extern "C" "utils"  → utils.dll (--cabi --shared, 导出 aura_c_*)
#   Demo 3 (demo_aot_direct) extern interface @aot → utils_aot.dll (--shared, 导出 aura_aot_*)
#
# 依赖：LLVM 工具链（AURA_LLVM_HOME 或仓库 Cargo.toml 配置）。
#
# 验收：两 demo 均得 add=7 / multiply=12 / factorial=120 / power=1024。
set -u
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
echo "[run-ffi-demo] PWD=$ROOT"

AURA="${AURA:-./seed/target/release/aura.exe}"
[ -x "$AURA" ] || AURA="./build/bin/aura.exe"
[ -x "$AURA" ] || { echo "aura binary not found"; exit 1; }

DEMO="$ROOT/examples/ext_ffi_demo"
LIB_SRC="$DEMO/libs/utils/src/lib.aura"
LIB_DIR="$DEMO/libs/utils/libs"
OUT="$ROOT/build/ffi-demo"
mkdir -p "$OUT"

strip_vm() { grep -vE "^\[vm\]|^\[ok\]|^semantic warning"; }

# fd 3 用于在「人读输出」之外回传每组的通过计数
exec 3>&1

check4() { # $1=log  $2=label
  local log="$1" label="$2" npass=0
  for want in "add(3, 4) = 7" "multiply(3, 4) = 12" "factorial(5) = 120" "power(2, 10) = 1024"; do
    if grep -qF "$want" "$log"; then echo "  OK   $label: $want"; npass=$((npass+1));
    else echo "  FAIL $label: $want"; fi
  done
  echo "$npass" >&3
}

# ── 1. C 头文件 ──
echo "--- 1. export-header ---"
"$AURA" export-header "$LIB_SRC" --out "$DEMO/demo_cffi/utils.h" || exit 1
grep -q "aura_c_add" "$DEMO/demo_cffi/utils.h" || { echo "header missing aura_c_add"; exit 1; }

# ── 2. C ABI 共享库（Demo 1） ──
echo "--- 2. build --aot --shared --cabi (C ABI) ---"
"$AURA" build "$LIB_SRC" --aot --shared --cabi --output "$OUT/utils.dll" || exit 1
# 必须是 PE(MZ) 而非 COFF 目标文件（COFF 头 0x6486）
if ! head -c 2 "$OUT/utils.dll" | od -An -tx1 | grep -qi "4d 5a"; then
  echo "utils.dll is not a valid PE image (copy picked the wrong artifact)"; exit 1
fi
cp "$OUT/utils.dll" "$LIB_DIR/utils.dll"

# ── 3. JitValue ABI 共享库（Demo 3） ──
echo "--- 3. build --aot --shared (JitValue ABI) ---"
"$AURA" build "$LIB_SRC" --aot --shared --output "$OUT/utils_aot.dll" || exit 1
if ! head -c 2 "$OUT/utils_aot.dll" | od -An -tx1 | grep -qi "4d 5a"; then
  echo "utils_aot.dll is not a valid PE image"; exit 1
fi
cp "$OUT/utils_aot.dll" "$LIB_DIR/utils_aot.dll"

total=0
# ── 4. Demo 1：C FFI under Aura VM ──
echo "--- 4. run demo_cffi (Aura VM, extern \"C\") ---"
LOG1="$OUT/run-cffi.log"
timeout 120 "$AURA" run "$DEMO/demo_cffi/src/main.aura" > "$LOG1" 2>&1
strip_vm < "$LOG1"
n1=$(check4 "$LOG1" "cffi" 2>/dev/null 3>&1 1>/dev/null)
total=$((total + n1))

# ── 5. Demo 3：AOT 直调 under Aura VM ──
echo "--- 5. run demo_aot_direct (Aura VM, extern interface @aot) ---"
LOG3="$OUT/run-aot.log"
timeout 120 "$AURA" run "$DEMO/demo_aot_direct/src/main.aura" > "$LOG3" 2>&1
strip_vm < "$LOG3"
n3=$(check4 "$LOG3" "aot " 2>/dev/null 3>&1 1>/dev/null)
total=$((total + n3))

echo "----------------------------------------------"
echo "FFI demo: $total/8 checks passed"
[ "$total" -eq 8 ] || exit 1
