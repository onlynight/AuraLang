#!/usr/bin/env bash
# 内存回收判定矩阵 —— 外部 RSS（peak_wset）与 VM 内部 arena 水位/值栈斜率并排输出。
#
# 为何存在：单看 peak_wset 无法回答「运行中是否回收了」——它只是峰值快照。
# 真正的回收判定靠宿主打印的 [MEMPROBE] 行（Vm.run 每 2^20 条指令采样一次
# bump arena 水位与值栈容量，结束时汇总）：
#
#   slope ≈ 8                    ⇒  ✓ 回收正常（arena 只按探针自身 alloc(8) 增长）
#   slope >> 8（如千/万级）      ⇒  ✗ 线性泄漏（bump 指针单调，无回收）
#   capGrow=1 / capLast=64       ⇒  ✓ 值栈零倍增（栈账目平衡）
#   capGrow 持续上涨 / capLast 巨大  ⇒  ✗ 栈顶单调上升
#   exit_ok=false                ⇒  ✗ 先看 VM-ERROR= 行：Instruction limit
#                                   是指令上限（2×10⁹）非内存问题
#
# 判读方法论详见 docs/photon/vm-memory-comparison.md §十.4。
#
# 用法: bash tools/photon-prof/mem-reclaim-matrix.sh
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
PY="C:/Users/zhang/.workbuddy/binaries/python/envs/default/Scripts/python.exe"
EXE="build/hat-vmhost/current.exe"

# 场景表：标签|.auc 路径|取峰轮数（loop1100m 慢，1 轮）
SCENARIOS=(
  "smallstep-1.5M|build/prof2/smallstep.auc|3"
  "intloop-1M|build/prof2/intloop.auc|3"
  "longstep-1M|build/prof2/longstep.auc|3"
  "loop1m|build/bench/bench_loop1m.auc|3"
  "loop1100k|build/bench/bench_loop1100k.auc|3"
  "loop2m|build/prof2/loop2m.auc|3"
  "loop1100m|build/prof2/loop1100m.auc|1"
  "fib28|build/bench/bench_fib28.auc|3"
)

printf '%-14s %-5s %-8s %-9s %-6s %-8s %-7s %-7s %-42s %s\n' \
  "场景" "exit" "wset" "private" "wall" "slope" "cap末" "栈倍增" "[MEMPROBE] hi/last/avg 相对偏移" "回收判定"

for s in "${SCENARIOS[@]}"; do
  label="${s%%|*}"; rest="${s#*|}"
  src="${rest%%|*}"; runs="${rest##*|}"
  if [ ! -f "$src" ]; then
    printf '%-14s %s\n' "$label" "SKIP（缺 $src）"
    continue
  fi
  cp "$src" build/vm/current.auc
  all=""
  for _ in $(seq 1 "$runs"); do
    out=$(timeout 400 "$PY" tools/photon-prof/memprof.py --runs 1 --interval 20 \
      --label "$label" --quiet -- "$EXE" 2>&1)
    all="$all$out"$'\n'
  done
  json=$(printf '%s' "$all" | grep -F '{"label"' | tail -1)
  # §10.111 家族：宿主对布局/ASLR 敏感，偶发装载后早崩（实测 ~1/4 概率，
  # 重跑即好）。无 JSON 时整体重试一轮。
  if [ -z "$json" ]; then
    all=""
    for _ in $(seq 1 "$runs"); do
      out=$(timeout 400 "$PY" tools/photon-prof/memprof.py --runs 1 --interval 20 \
        --label "$label" --quiet -- "$EXE" 2>&1)
      all="$all$out"$'\n'
    done
    json=$(printf '%s' "$all" | grep -F '{"label"' | tail -1)
  fi
  probe=$(printf '%s' "$all" | grep '\[MEMPROBE\]' | head -1)
  if [ -z "$json" ]; then
    printf '%-14s %s\n' "$label" "FAIL（重试后仍无 JSON，连续早期崩溃）"
    continue
  fi
  read -r wset priv wall exit_ok <<< "$("$PY" -c "
import json,sys
d=json.loads(sys.argv[1])
print(d['peak_wset_mb'], d['peak_private_mb'], d['wall_ms'], d['exit_ok'])
" "$json")"

  # 解析 MEMPROBE 字段
  g() { printf '%s' "$probe" | tr ' ' '\n' | grep -F "$1=" | head -1 | cut -d= -f2; }
  slope=$(g slope); capLast=$(g capLast); capGrow=$(g capGrow)
  hiRel=$(g hiRel); lastRel=$(g lastRel); avgRel=$(g avgRel)

  # 判定
  verdict="✓ 回收正常"
  if [ "$exit_ok" != "True" ] && [ "$exit_ok" != "true" ]; then
    if printf '%s' "$all" | grep -q "Instruction limit"; then
      verdict="❌ exit≠0（指令上限 2×10⁹，非内存）"
    else
      verdict="❌ exit≠0（$(printf '%s' "$all" | grep -oE 'VM-ERROR=[^"]*' | head -1)）"
    fi
  fi
  if [ -n "$capGrow" ] && [ "$capGrow" -ge 3 ] 2>/dev/null; then
    verdict="⚠ 栈倍增 ×$capGrow（栈顶单调上升）"
  fi
  if [ -n "$slope" ] && [ "$slope" -gt 64 ] 2>/dev/null; then
    verdict="✗ arena 线性泄漏（slope=$slope B/采样点）"
  fi

  printf '%-14s %-5s %-8s %-9s %-6s %-8s %-7s %-7s %-42s %s\n' \
    "$label" "$exit_ok" "${wset}MB" "${priv}MB" "$wall" \
    "${slope:-?}" "${capLast:-?}" "${capGrow:-?}" \
    "${hiRel:+h$hiRel }${lastRel:+l$lastRel }${avgRel:+a$avgRel}" "$verdict"
done
echo
echo "口径：wset=峰值工作集（探针常驻表使各场景 +2MB）；slope=B/采样点（2^20 指令），"
echo "      8=探针自身开销即零泄漏；capGrow=值栈倍增次数（1=仅初始分配，健康）。"
