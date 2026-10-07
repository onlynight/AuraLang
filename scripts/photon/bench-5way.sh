#!/usr/bin/env bash
# -------------------------------------------------------------
# 五模式基准对比：同一份 Aura 源码，五条执行链
#
#   ① LLVM IR AOT    : seed `aura build --aot` → .ll → llc → clang → 原生 exe
#   ② Seed/Rust VM   : seed `aura run`（Rust 字节码解释器）
#   ③ Aura VM(seed)  : build/bin/auc_vm_runner.exe 加载 build/vm/current.auc
#                       —— VM 自身由 **seed 的 LLVM-AOT** 编译
#   ④ Aura VM(Photon): build/hat-vmhost/current.exe 加载同一份 .auc
#                       —— VM 自身由 **Photon 后端** 编译（本会话优化的产物）
#   ⑤ Photon AOT     : PhotonHatCompile → COFF → lld-link → 原生 exe
#                       —— 自研 X86 后端直出机器码（与 ① 同类，可直比）
#
# ③④ 跑**完全相同的字节码**，差异只来自「编译 VM 用的后端」。
# ①⑤ 都是原生 AOT，差异纯来自「两个后端生成的代码质量」。
#
# ⚠ ⑤ 会覆盖 build/hat-vmhost/current.{aura,exe}（驱动的输入通道是固定路径、
#   且驱动自身会落盘链接产物）——本脚本在 ⑤ 前后自动备份/恢复宿主。
#
# 用法: scripts/photon/bench-5way.sh [轮数]
# -------------------------------------------------------------
set -u
cd "$(dirname "$0")/../.." || exit 1
RUNS="${1:-5}"
SEED="rust/target/release/aura.exe"
RUNNER="build/bin/auc_vm_runner.exe"
HOST="build/hat-vmhost/current.exe"
DRV="build/hat-native/PhotonHatCompile.exe"
LLD="D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin/lld-link.exe"
AUC="build/vm/current.auc"
WORK="build/bench4"
BAK="$WORK/photon-aot-bak"
BASE="build/tmpwork/host_baseline.exe"   # 会话起点 P409 宿主（仅作对比）
mkdir -p "$WORK" "$BAK"

# 基准源归档恢复：build/ 在 .gitignore 中，基准源归档于
# tools/photon-prof/bench-src/（缺失时才复制，不覆盖本地改动）。
mkdir -p build/bench
for _f in tools/photon-prof/bench-src/*.aura; do
  _b="build/bench/$(basename "$_f")"
  [ -f "$_b" ] || cp "$_f" "$_b"
done

# 计时：返回 min（CPU 密集任务用 min 最稳健）
timeit() {
  local exe="$1" args="$2" n="$3"
  local best=999999999 t0 t1 ms
  for ((i=0;i<n;i++)); do
    t0=$(date +%s%N)
    if [ -n "$args" ]; then "$exe" $args >/dev/null 2>&1; else "$exe" >/dev/null 2>&1; fi
    t1=$(date +%s%N)
    ms=$(( (t1-t0)/1000000 ))
    [ "$ms" -lt "$best" ] && best=$ms
  done
  echo "$best"
}

# ── 宿主保护：备份（幂等）与恢复 ──
# ⚠ 判据用「首行特征」而非文件大小：宿主入口源是 1398B 的薄入口
# （真逻辑在其 import 的 aura/compiler/... 里），bench 源仅 200~600B，
# 用大小阈值会误判。
host_backup() {
  if ! head -1 build/hat-vmhost/current.aura 2>/dev/null | grep -q 'auc_vm_runner'; then
    echo "FATAL: current.aura 首行不是宿主入口（可能上次中断未恢复）" >&2
    return 1
  fi
  [ -f "$BAK/current.aura" ] || cp build/hat-vmhost/current.aura "$BAK/current.aura"
  [ -f "$BAK/current.exe" ]  || cp build/hat-vmhost/current.exe  "$BAK/current.exe"
  return 0
}
host_restore() {
  [ -f "$BAK/current.aura" ] && cp "$BAK/current.aura" build/hat-vmhost/current.aura
  [ -f "$BAK/current.exe" ]  && cp "$BAK/current.exe"  build/hat-vmhost/current.exe
}

# ── ⑤ Photon AOT：入口源 → 驱动 → COFF hex → obj → lld-link ──
# 用法: photon_aot <源> <tag> <输出目录>
photon_aot() {
  local src="$1" tag="$2" out="$3"
  mkdir -p "$out"
  host_backup || return 1
  cp "$src" build/hat-vmhost/current.aura
  timeout 900 "$DRV" > "$out/$tag.log" 2>&1
  if ! grep -q '===RESULT===success' "$out/$tag.log"; then
    host_restore; echo "  [$tag] Photon AOT 编译失败" >&2; return 1
  fi
  python3 - "$out/$tag.log" "$out" "$tag" << 'PY'
import sys
log, outdir, tag = sys.argv[1], sys.argv[2], sys.argv[3]
M_MAIN, M_RT = '===COFF-MAIN===', '===COFF-RUNTIME==='
state, got = {}, {}
for line in open(log, encoding='utf-8', errors='replace'):
    t = line.rstrip('\r\n').strip()
    if t in (M_MAIN, M_RT):
        state[t] = 'pending'; continue
    for m in (M_MAIN, M_RT):
        if state.get(m) == 'pending':
            if t and len(t) % 2 == 0 and all(c in '0123456789abcdefABCDEF' for c in t):
                got[m] = t
            state[m] = 'done'; break
for m, name in ((M_MAIN, 'main_%s.obj' % tag), (M_RT, 'rt_%s.obj' % tag)):
    if m not in got:
        raise SystemExit('COFF hex missing: ' + m)
    open(outdir + '/' + name, 'wb').write(bytes.fromhex(got[m]))
PY
  MSYS2_ARG_CONV_EXCL="*" "$LLD" "$out/main_$tag.obj" "$out/rt_$tag.obj" \
    "/OUT:$out/$tag.exe" /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 \
    /NODEFAULTLIB KERNEL32.lib > "$out/link_$tag.log" 2>&1
  host_restore
  [ -f "$out/$tag.exe" ] || { echo "  [$tag] 链接失败" >&2; return 1; }
  return 0
}

printf '%-14s %10s %10s %12s %14s %11s\n' "基准" "LLVM-AOT" "Rust VM" "AuraVM(seed)" "AuraVM(Photon)" "PhotonAOT"
printf '%s\n' "----------------------------------------------------------------------------------"

for B in bench_fib28 bench_loop1m; do
  SRC="build/bench/$B.aura"
  OUT="$WORK/$B"
  mkdir -p "$OUT"

  # ① LLVM IR AOT
  [ -f "$OUT/llvm.exe" ] || "$SEED" build --aot "$SRC" --output "$OUT/llvm.exe" >/dev/null 2>&1
  T1=$(timeit "$OUT/llvm.exe" "" "$RUNS")

  # ② Seed/Rust VM
  T2=$(timeit "$SEED" "run $SRC" "$RUNS")

  # ③④ 共用同一份 .auc
  "$SEED" build "$SRC" --output "$AUC" >/dev/null 2>&1
  T3=$(timeit "$RUNNER" "" "$RUNS")
  T4=$(timeit "$HOST" "" "$RUNS")

  # ⑤ Photon AOT（会临时覆盖宿主，函数内自恢复）
  T5="n/a"
  if photon_aot "$SRC" "$B" "$OUT"; then T5=$(timeit "$OUT/$B.exe" "" "$RUNS"); fi

  T0=""
  [ -f "$BASE" ] && T0=$(timeit "$BASE" "" "$RUNS")

  printf '%-14s %8sms %8sms %10sms %12sms %9sms\n' "$B" "$T1" "$T2" "$T3" "$T4" "$T5"
  python3 - "$B" "$T1" "$T3" "$T4" "$T5" "$T0" << 'PY'
import sys
args = sys.argv[1:7]
b = args[0]
def f(s):
    try:
        return float(s)
    except Exception:
        return None
t1, t3, t4, t5, t0 = map(f, args[1:6])
def pct(a, c):
    return ('%+.1f%%' % (100 * (1 - a / c))) if (a and c) else 'n/a'
print('  %s: ④vs③=%s  ④vs起点=%s  ⑤vs①=%s'
      % (b, pct(t4, t3), pct(t4, t0), pct(t5, t1)))
PY
done

echo
echo "派生指标说明："
echo "  · ④vs③   = 同一份字节码下，Photon 构建的 VM 相对 seed 构建的 VM（负数=更慢）"
echo "  · ④vs起点 = 本会话 P409→当前 在 VM 上的净收益（同一 Photon 宿主）"
echo "  · ⑤vs①  = 两个 AOT 后端的原生代码差距（小基准上被 ~0.65s 启动开销淹没）"

echo
echo "说明："
echo "  · ①⑤ 都是**原生 AOT**（无 VM/装载）：⑤vs① 即「Photon 后端 vs LLVM 后端」"
echo "    的代码质量差（⑤ 含 67MB runtime obj，① 仅 369KB）。"
echo "  · ③④ 跑**完全相同的 .auc 字节码**（装载逻辑同源）；差异纯来自编译 VM"
echo "    所用的后端——③ 种子 LLVM-AOT，④ Photon 后端（含本会话优化）。"
echo "  · ⚠ 所有模式都含 ~0.65s 进程启动开销（实测：空程序 AOT 也要 0.65~0.74s），"
echo "    fib28/loop1M 的纯计算量远小于该量级 ⇒ **小基准区分不了 AOT 后端**。"
echo "    要看后端真实差距请跑 scripts/photon/bench-aot-slope.sh（600M vs 1000M"
echo "    迭代的斜率法，实测 Photon 后端 ≈ LLVM-O2 的 1/2.9 速）。"
echo "  · 轮数 = $RUNS，取 min。"
