#!/usr/bin/env bash
# map-sc-sites.sh — 全量站点表(all-sites) RVA → Aura 函数映射
#
# 输入: 驱动 obj(与产生 dump 的 PhotonHatCompile.exe 同一次构建) + 站点 dump 文件
#       (含 'rva=0x...' 的行,来自 oom-sc/exit-sc/reset-sc/big-concat 转储)
# 口径: exe .text 自 RVA 0x1000 起,obj 符号值 = .text 内偏移 ⇒ sym_off = rva-0x1000,
#       取最近下方符号(同 09 报告 A·补充八 函数级归因口径)。
# 输出: 原行 + " fn=<符号>+<偏移>"
#
# 用法: scripts/photon/map-sc-sites.sh build/hat-native/hat-native.obj <dump.txt>
set -euo pipefail

OBJ="$1"; DUMP="$2"
LLVM_BIN="${AURA_LLVM_BIN:-D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin}"

SYMS="$(mktemp)"
trap 'rm -f "$SYMS"' EXIT
"$LLVM_BIN/llvm-nm" --numeric-sort "$OBJ" \
  | awk '$2 != "U" && $2 != "w" && $2 != "v" { print strtonum("0x"$1), $3 }' \
  | sort -n -k1,1 > "$SYMS"

awk -v SYMS="$SYMS" '
  BEGIN { n = 0; while ((getline line < SYMS) > 0) { split(line, a, " "); off[n]=a[1]+0; nm[n]=a[2]; n++ } }
  function mapfn(rva,   lo, hi, mid, target, k) {
    target = rva - 0x1000
    if (n == 0) return "?<no-syms>"
    if (target < off[0]) return "?<below-first>"
    lo = 0; hi = n - 1
    while (lo < hi) { mid = int((lo + hi + 1) / 2); if (off[mid] <= target) lo = mid; else hi = mid - 1 }
    k = target - off[lo]
    return nm[lo] "+0x" sprintf("%x", k)
  }
  /rva=0x/ {
    for (i = 1; i <= NF; i++) {
      if ($i ~ /^rva=0x/) {
        printf "%s fn=%s\n", $0, mapfn(strtonum(substr($i, 5)))
        next
      }
    }
  }
  { print }
' "$DUMP"
