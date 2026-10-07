#!/usr/bin/env bash
# -------------------------------------------------------------
# 四模式基准对比：同一份 Aura 源码，四条执行链
#
#   ① LLVM IR AOT   : seed `aura build --aot` → .ll → llc → clang → 原生 exe
#   ② Seed/Rust VM  : seed `aura run`（Rust 字节码解释器）
#   ③ Aura VM(seed) : build/bin/auc_vm_runner.exe 加载 build/vm/current.auc
#                     —— VM 自身由 **seed 的 LLVM-AOT** 编译
#   ④ Aura VM(Photon): build/hat-vmhost/current.exe 加载同一份 .auc
#                     —— VM 自身由 **Photon 后端** 编译（即本会话优化的产物）
#
# ③④ 跑**完全相同的字节码**，差异只来自「编译 VM 用的后端」+ 本会话的
# 发射器优化（P412/P413/P414/P414e）。这使④的收益可被干净地分离出来。
#
# 用法: scripts/photon/bench-4way.sh [轮数]
# -------------------------------------------------------------
set -u
cd "$(dirname "$0")/../.." || exit 1
RUNS="${1:-5}"
SEED="rust/target/release/aura.exe"
RUNNER="build/bin/auc_vm_runner.exe"
HOST="build/hat-vmhost/current.exe"
AUC="build/vm/current.auc"
WORK="build/bench4"
BASE="build/tmpwork/host_baseline.exe"   # 会话起点 P409 宿主（仅作对比）
mkdir -p "$WORK"

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

printf '%-14s %10s %10s %10s %10s %10s %10s\n' "基准" "LLVM-AOT" "Rust VM" "AuraVM(seed)" "AuraVM(Photon)" "④vs③" "④vs会话起点"
printf '%s\n' "--------------------------------------------------------------------------------------------"

for B in bench_fib28 bench_loop1m; do
  SRC="build/bench/$B.aura"
  OUT="$WORK/$B"
  mkdir -p "$OUT"

  # ① LLVM IR AOT：编译 + 运行
  if [ ! -f "$OUT/llvm.exe" ]; then
    "$SEED" build --aot "$SRC" --output "$OUT/llvm.exe" >/dev/null 2>&1
  fi
  T1=$(timeit "$OUT/llvm.exe" "" "$RUNS")

  # ② Seed/Rust VM（解释器；含固定启动开销）
  T2=$(timeit "$SEED" "run $SRC" "$RUNS")

  # ③④ 共用同一份 .auc
  "$SEED" build "$SRC" --output "$AUC" >/dev/null 2>&1
  T3=$(timeit "$RUNNER" "" "$RUNS")
  T4=$(timeit "$HOST" "" "$RUNS")
  T0=""
  if [ -f "$BASE" ]; then T0=$(timeit "$BASE" "" "$RUNS"); fi

  SP=$(python3 -c "print(f'{100*(1-$T4/$T3):.1f}%' if $T3>0 else 'n/a')")
  SP0=$(python3 -c "print(f'{100*(1-$T4/$T0):.1f}%' if '$T0' not in ('','0') and $T0>0 else 'n/a')")
  printf '%-14s %8sms %8sms %8sms %8sms %10s %12s\n' "$B" "$T1" "$T2" "$T3" "$T4" "$SP" "$SP0"
done

echo
echo "说明："
echo "  · ①LLVM-AOT = 种子 Rust 编译器的 LLVM 后端产出的原生码。"
echo "  · ②Rust VM  = 种子的 Rust 字节码解释器（含 ~0.6s 固定启动）。"
echo "  · ③④ 跑**完全相同的 .auc 字节码**，装载逻辑同源"
echo "    （examples/compiler/auc_vm_runner.aura）；差异纯粹来自"
echo "    「编译 VM 自身所用的后端」——③ 用种子 LLVM-AOT，④ 用 Photon 后端。"
echo "  · ⚠️ ③ 的二进制是 2026-10-05 构建的，其源码快照**早于**本会话优化；"
echo "    ④ 含本会话全部优化。故 ④vs③ 差距中混入了「源码版本差」，"
echo "    对 ④ 是**保守**估计（用旧源码的 ③ 当基准，已不利于 ④）。"
echo "  · ④vs会话起点 = 本会话 P409→当前 的净收益（同一 Photon 宿主）。"
echo "  · 轮数 = $RUNS，取 min；启动开销未扣除（micro1 实测两者均 ~0.65s）。"
