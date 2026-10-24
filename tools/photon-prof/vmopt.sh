#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Photon Aura VM 优化闭环：重建宿主 + 五门回归 + 计时
#
# 用法：
#   ./scripts/photon/vmopt.sh build     重建宿主（驱动 -> obj -> lld-link）
#   ./scripts/photon/vmopt.sh regress   跑五门回归（micro1/2/3 + fib28 + loop1M）
#   ./scripts/photon/vmopt.sh bench     计时（5轮中位）
#   ./scripts/photon/vmopt.sh all       build + regress + bench
#   ./scripts/photon/vmopt.sh regress <exe>   用指定宿主跑回归（历史产物对照）
#
# 关键路径约定（宿主源码写死，勿改）：
#   宿主源   build/hat-vmhost/current.aura
#   宿主产物 build/hat-vmhost/current.exe
#   客体 .auc build/vm/current.auc      ← 换基准只改这个文件，无需重编宿主
#
# ⚠ 驱动选择（2026-10-07 定案）：必须用 `PhotonHatCompile_old.exe`（14:47 构建）。
# 用 `aura build --aot` 重新构建的驱动（17:14/17:51）会让宿主**静默错值**
# （micro2 → `m2=0` 而非 `7`，五门全坏），且构建完全确定（同源码两次 md5 相同）
# ——是该驱动二进制自身的回归，不是源码问题。恢复方式：
#   cp build/hat-native/PhotonHatCompile_old.exe build/hat-native/PhotonHatCompile.exe
# 症状：main.obj 493280→502556 字节、宿主 .text 0x64145→0x65685。
# ─────────────────────────────────────────────────────────────
set -uo pipefail
cd "$(dirname "$0")/../.."
ROOT=$(pwd)

HOST_SRC="build/hat-vmhost/current.aura"
HOST_EXE="build/hat-vmhost/current.exe"
HOST_OBJ="build/hat-vmhost/current.obj"
RT_OBJ="build/hat-vmhost/aura_runtime.obj"
AUC="build/vm/current.auc"
DRV="build/hat-native/PhotonHatCompile.exe"
LLD="D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin/lld-link.exe"
LOG="build/tmpwork"

mkdir -p "$LOG" build/vm

# guest 输出 = 装载期最后一个探针标记 `[winC2] post-loadPredecoded` **之后**的所有行。
# 该标记之前的内容全是Vm.aura 装载期的诊断探针（[DC]/[T32]/[FLAT*]/[OC]/[CL2]/…），
# 它们本身也会污染 stdout（println 的实参被先一步打印成裸值），不能当 guest 输出。
# 标记名一旦变化，用 `grep -n` 重新定位——宁可空跑也不要误判。
guest_out() {
  awk '/^\[winC2\] post-loadPredecoded$/{seen=1; next} seen' | grep -vE '^\s*$'
}

do_build() {
  echo "=== [1/3] 驱动编译宿主 ==="
  if [ ! -f "$DRV" ]; then echo "FATAL: 驱动缺失 $DRV"; return 1; fi
  timeout 900 "$DRV" > "$LOG/build.log" 2>&1
  local rc=$?
  if [ $rc -ne 0 ]; then echo "FATAL: 驱动 rc=$rc"; tail -20 "$LOG/build.log"; return 1; fi
  if ! grep -q "success" "$LOG/build.log"; then
    echo "FATAL: 驱动未报success"; tail -20 "$LOG/build.log"; return 1
  fi

  # 驱动把 COFF 以**单行 hex 文本**打到 stdout，紧跟 ===COFF-MAIN=== /
  # ===COFF-RUNTIME=== 标记；marker 之后还混有 [rt]/RDATA-CHK 等诊断行与
  # ===LINK===/===RESULT=== 尾标，须只取「紧跟 marker 的第一行」（与
  # build-photon-hat.ps1 的 Write-HexFile 同款口径）。驱动自身不落盘 obj。
  echo "=== [2/3] 提取 COFF hex -> obj ==="
  python3 - "$LOG/build.log" "$HOST_OBJ" "$RT_OBJ" << 'PYEOF'
import sys
log, main_obj, rt_obj = sys.argv[1], sys.argv[2], sys.argv[3]
M_MAIN, M_RT = '===COFF-MAIN===', '===COFF-RUNTIME==='
state, got = {}, {}
for line in open(log, encoding='utf-8', errors='replace'):
    t = line.rstrip('\r\n').strip()
    if t == M_MAIN or t == M_RT:
        state[t] = 'pending'
        continue
    for marker in (M_MAIN, M_RT):
        if state.get(marker) == 'pending':
            if t and len(t) % 2 == 0 and all(c in '0123456789abcdefABCDEF' for c in t):
                got[marker] = t
            state[marker] = 'done'
            break

def dump(h, path):
    if h is None:
        raise SystemExit('COFF hex missing for ' + path)
    data = bytes.fromhex(h)
    open(path, 'wb').write(data)
    return len(data)

print('  main.obj=%d bytes  runtime.obj=%d bytes'
      % (dump(got.get(M_MAIN), main_obj), dump(got.get(M_RT), rt_obj)))
PYEOF
  if [ $? -ne 0 ]; then echo "FATAL: hex 提取失败"; return 1; fi

  echo "=== [3/3] lld-link ==="
  # Git Bash 会把 /NODEFAULTLIB 等参数当成路径改写，需禁 MSYS 路径转换
  MSYS2_ARG_CONV_EXCL="*" timeout 600 "$LLD" \
    "$HOST_OBJ" "$RT_OBJ" \
    "/OUT:$HOST_EXE" "/SUBSYSTEM:CONSOLE" "/ENTRY:main" \
    "/MACHINE:X64" "/NODEFAULTLIB" "KERNEL32.lib" \
    > "$LOG/link.log" 2>&1
  rc=$?
  if [ $rc -ne 0 ]; then echo "FATAL: 链接 rc=$rc"; tail -20 "$LOG/link.log"; return 1; fi
  echo "  exe: $(stat -c%s "$HOST_EXE") bytes"
  echo "BUILD OK"
}

# 用法：run_case <名称> <期望正则> <auc源>
# seed CLI 的 `--vm=aura` 产物路径**写死**为 build/vm/current.auc
# （宿主与 CLI 共用同一固定路径约定，见 examples/compiler/auc_vm_runner.aura
#  注释），因此生成后必须从current.auc 搬到按名缓存的位置。
gen_auc() {
  local src="$1" out="${1%.aura}.auc"
  [ -f "$out" ] && return 0
  rm -f "$AUC"
  ./rust/target/release/aura.exe run "$src" --vm=aura > /dev/null 2>&1
  [ -f "$AUC" ] || return 1
  mv "$AUC" "$out"
}

run_case() {
  local name="$1" expect="$2" src="$3"
  local exe="${4:-$HOST_EXE}"
  if [ ! -f "$src" ]; then echo "  [$name] SKIP（缺 $src）"; return 0; fi
  if ! gen_auc "$src"; then echo "  [$name] 生成 .auc 失败"; return 1; fi
  cp "${src%.aura}.auc" "$AUC"
  local out
  out=$(timeout 300 "$exe" 2>/dev/null | guest_out)
  if echo "$out" | grep -qE "$expect"; then
    echo "  [$name] PASS  <<< $(echo "$out" | tr '\n' '|')"
    return 0
  fi
  echo "  [$name] FAIL  期望匹配 /$expect/"
  echo "$out" | sed 's/^/         got: /'
  return 1
}

do_regress() {
  local exe="${1:-$HOST_EXE}"
  echo "=== 五门回归（宿主 $exe） ==="
  local fail=0
  run_case micro1 'm1=10'            build/bench/micro1.aura            "$exe" || fail=1
  run_case micro2 'm2=7'             build/bench/micro2.aura            "$exe" || fail=1
  run_case micro3 'm3=[0-9]+'        build/bench/micro3.aura            "$exe" || fail=1
  run_case fib28  'fib\(28\) *= *317811' build/bench/bench_fib28.aura    "$exe" || fail=1
  run_case loop1m 'loop acc = 999998000000' build/bench/bench_loop1m.aura "$exe" || fail=1
  if [ $fail -ne 0 ]; then echo "REGRESS: 有门失败"; else echo "REGRESS: 全绿"; fi
  return $fail
}

do_bench() {
  echo "=== fib28 计时（5 轮中位） ==="
  if [ ! -f build/bench/bench_fib28.aura ]; then echo "SKIP"; return 0; fi
  gen_auc build/bench/bench_fib28.aura || { echo "SKIP"; return 0; }
  cp build/bench/bench_fib28.auc "$AUC"
  local times=()
  for i in 1 2 3 4 5; do
    local t0 t1
    t0=$(date +%s%N)
    timeout 300 "$HOST_EXE" > /dev/null 2>&1
    t1=$(date +%s%N)
    times+=( $(( (t1 - t0) / 1000000 )) )
  done
  echo "  times(ms)=[${times[*]}]"
  local sorted=($(printf '%s\n' "${times[@]}" | sort -n))
  echo "  median=${sorted[2]} ms"
}

case "${1:-all}" in
  build)   do_build ;;
  regress) do_regress "${2:-}" ;;
  bench)   do_bench ;;
  all)     do_build && do_regress; do_bench ;;
  regressonly) do_regress "${2:-}" ;;
  *)       echo "用法: $0 {build|regress|bench|all}"; exit 2 ;;
esac
