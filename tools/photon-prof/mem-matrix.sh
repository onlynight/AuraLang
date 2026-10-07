#!/usr/bin/env bash
# 内存对比矩阵：Rust VM vs Photon Aura VM
# 每个场景 3 轮，取峰值
set -u
cd "$(dirname "$0")/../.." || exit 1
PY="C:/Users/zhang/.workbuddy/binaries/python/envs/default/Scripts/python.exe"
RUNS=3
OUT="build/prof2"
mkdir -p "$OUT"

M() {
  local label="$1"; shift
  "$PY" tools/photon-prof/memprof.py --runs "$RUNS" --label "$label" -- "$@"
}

echo "=== micro1（最小 guest）==="
M RustVM-micro1 rust/target/release/aura.exe run build/bench/micro1.aura
cp build/bench/micro1.auc build/vm/current.auc
M PhotonVM-micro1 build/hat-vmhost/current.exe

echo "=== fib28（调用密集）==="
M RustVM-fib28 rust/target/release/aura.exe run build/bench/bench_fib28.aura
cp build/bench/bench_fib28.auc build/vm/current.auc
M PhotonVM-fib28 build/hat-vmhost/current.exe

echo "=== loop1m（算术密集）==="
M RustVM-loop1m rust/target/release/aura.exe run build/bench/bench_loop1m.aura
cp build/bench/bench_loop1m.auc build/vm/current.auc
M PhotonVM-loop1m build/hat-vmhost/current.exe

echo "=== RustVM 纯编译（同源、不执行，用于拆分「编译 vs 执行」内存）==="
M RustVM-build-fib28 rust/target/release/aura.exe build build/bench/bench_fib28.aura --output "$OUT/fib28_build.auc"
M RustVM-build-loop1m rust/target/release/aura.exe build build/bench/bench_loop1m.aura --output "$OUT/loop1m_build.auc"

echo "DONE"
