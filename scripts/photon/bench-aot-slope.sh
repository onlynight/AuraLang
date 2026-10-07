#!/usr/bin/env bash
# -------------------------------------------------------------
# AOT 后端斜率对比：Photon 自研 X86 后端 vs LLVM -O2
#
# 背景：fib28/loop1M 这类小基准的纯计算量（几 ms）远小于进程启动开销
# （实测空程序 AOT 也要 0.65~0.74s），**区分不了 AOT 后端**。本脚本用
# 两个大规模档位做斜率，消掉启动开销：
#
#     (T_N2 - T_N1) / (N2 - N1)  =  单次迭代纯计算成本
#
# 默认 N1=600000000（6e8）、N2=1000000000（1e9）。
# ⚠ N 的上限受 `i * 2` 的 Int32 约束：i*2 必须 < 2147483647 ⇒ N ≤ 1.07e9。
#   （实测 N=1.2e9 时两端**溢出语义分歧**：LLVM 版 i32 回绕、Photon 版
#    得数学精确值——语义不一致的用例不能用于性能对比。）
#
# 用法: scripts/photon/bench-aot-slope.sh [N1] [N2] [轮数]
# ⚠ 会临时覆盖 build/hat-vmhost/current.{aura,exe}（Photon 驱动输入通道
#   是固定路径）——脚本内自动备份/恢复宿主。
# -------------------------------------------------------------
set -u
cd "$(dirname "$0")/../.." || exit 1
N1="${1:-600000000}"
N2="${2:-1000000000}"
RUNS="${3:-7}"
SEED="rust/target/release/aura.exe"
DRV="build/hat-native/PhotonHatCompile.exe"
LLD="D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin/lld-link.exe"
SC="build/bench4/slope"
BAK="build/bench4/photon-aot-bak"
mkdir -p "$SC" "$BAK"

timeit() {
  local exe="$1" n="$2" best=999999999 t0 t1 ms
  for ((i=0;i<n;i++)); do
    t0=$(date +%s%N); "$exe" >/dev/null 2>&1; t1=$(date +%s%N)
    ms=$(( (t1-t0)/1000000 )); [ "$ms" -lt "$best" ] && best=$ms
  done
  echo "$best"
}

# 宿主保护：判据=首行特征（宿主入口源仅 1398B 的薄入口，不能用大小阈值）
head -1 build/hat-vmhost/current.aura | grep -q 'auc_vm_runner' \
  || { echo "FATAL: current.aura 首行不是宿主入口（可能上次中断未恢复）"; exit 1; }
[ -f "$BAK/current.aura" ] || cp build/hat-vmhost/current.aura "$BAK/current.aura"
[ -f "$BAK/current.exe" ]  || cp build/hat-vmhost/current.exe  "$BAK/current.exe"

gen_src() {  # $1=N  $2=输出路径
  python3 - "$1" "$2" << 'PY'
import sys
n, path = int(sys.argv[1]), sys.argv[2]
src = (
    "// slope bench N=%d\n"
    "fun main() {\n"
    "    var i: Int = 0\n"
    "    var acc: Long = 0\n"
    "    while (i < %d) {\n"
    "        acc = acc + (i * 2 - 1) as Long\n"
    "        i = i + 1\n"
    "    }\n"
    '    println("loop acc = " + toStr(acc))\n'
    "}\n" % (n, n)
)
open(path, 'w', encoding='utf-8', newline='\n').write(src)
PY
}

# 期望值 Σ(2i-1) = N²-2N
expect() { python3 -c "print($1*$1 - 2*$1)"; }

compile_both() {  # $1=tag  $2=N
  # ⚠ 不可写成 `local tag="$1" ... src="$SC/loop$tag.aura"`——
  # 同一 local 语句内引用刚声明的变量时，bash 尚未使其生效（set -u 直接报
  # unbound variable）。必须拆成多条。
  local tag="$1"
  local n="$2"
  local src="$SC/loop$tag.aura"
  gen_src "$n" "$src"
  "$SEED" build --aot "$src" --output "$SC/llvm_$tag.exe" >/dev/null 2>&1
  cp "$src" build/hat-vmhost/current.aura
  timeout 900 "$DRV" > "$SC/photon_$tag.log" 2>&1
  grep -q '===RESULT===success' "$SC/photon_$tag.log" \
    || { echo "  [$tag] Photon 编译失败"; cp "$BAK/current.aura" build/hat-vmhost/current.aura; return 1; }
  python3 - "$SC/photon_$tag.log" "$SC" "$tag" << 'PY'
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
open(outdir + '/photon_%s.obj' % tag, 'wb').write(bytes.fromhex(got[M_MAIN]))
open(outdir + '/rt_%s.obj' % tag, 'wb').write(bytes.fromhex(got[M_RT]))
PY
  MSYS2_ARG_CONV_EXCL="*" "$LLD" "$SC/photon_$tag.obj" "$SC/rt_$tag.obj" \
    "/OUT:$SC/photon_$tag.exe" /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 \
    /NODEFAULTLIB KERNEL32.lib > "$SC/link_$tag.log" 2>&1
  cp "$BAK/current.aura" build/hat-vmhost/current.aura
  cp "$BAK/current.exe"  build/hat-vmhost/current.exe
  [ -f "$SC/photon_$tag.exe" ]
}

echo "=== 生成并编译（N1=$N1, N2=$N2）==="
compile_both n1 "$N1" || exit 1
compile_both n2 "$N2" || exit 1

echo "=== 输出校验（两端须一致且 = N²-2N）==="
for t in n1 n2; do
  n=$([ "$t" = n1 ] && echo "$N1" || echo "$N2")
  exp=$(expect "$n")
  l=$("$SC/llvm_$t.exe"); p=$("$SC/photon_$t.exe")
  printf "  %s: llvm=%-24s photon=%-24s 期望=%s %s\n" \
    "$t" "${l##*= }" "${p##*= }" "$exp" \
    "$([ "${l##*= }" = "$exp" ] && [ "${p##*= }" = "$exp" ] && echo ✓ || echo ✗)"
done

echo "=== 计时（$RUNS 轮取 min）==="
L1=$(timeit "$SC/llvm_n1.exe" "$RUNS");   P1=$(timeit "$SC/photon_n1.exe" "$RUNS")
L2=$(timeit "$SC/llvm_n2.exe" "$RUNS");   P2=$(timeit "$SC/photon_n2.exe" "$RUNS")
printf "  llvm  : N1=%sms  N2=%sms\n" "$L1" "$L2"
printf "  photon: N1=%sms  N2=%sms\n" "$P1" "$P2"

python3 - "$N1" "$N2" "$L1" "$L2" "$P1" "$P2" << 'PY'
import sys
n1, n2, l1, l2, p1, p2 = map(int, sys.argv[1:7])
dl, dp = l2 - l1, p2 - p1
it = n2 - n1
# ms → ns 需 ×1e6（此前漏乘，%.3f 下恒显示 0.000）
lns, pns = dl * 1e6 / it, dp * 1e6 / it
print()
print("=== 斜率（消掉启动开销）===")
print("  LLVM   : %dms / %d 次迭代 = %.3f ns/迭代" % (dl, it, lns))
print("  Photon : %dms / %d 次迭代 = %.3f ns/迭代" % (dp, it, pns))
if dl > 0:
    print("  ⇒ Photon 后端 = LLVM-O2 的 1/%.2f 速（慢 %.2f×）" % (dp / dl, dp / dl))
PY
echo
echo "DONE（宿主机已恢复）"
