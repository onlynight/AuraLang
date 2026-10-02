#!/usr/bin/env bash
# 构建并运行 tests/concurrent 下全部用例（AOT 路径）。
# 每个用例独立输出目录，避免中间 .ll/.obj 相互覆盖造成误判。
set -u
cd "$(dirname "$0")/.."
echo "[run-concurrent-aot] PWD=$(pwd)"

AURA="./seed/target/release/aura.exe"
OUTROOT="build/aot-conc"
mkdir -p "$OUTROOT"

pass=0; fail=0; failed_list=""
for t in atomic_ops barrier_semaphore future_chain integration memory_test \
         mutex_shared promise_test rwlock_condvar thread_basics; do
  out="$OUTROOT/$t"
  rm -rf "$out"; mkdir -p "$out"
  printf '=== %-20s ' "$t"
  if ! timeout 180 "$AURA" build "tests/concurrent/$t.aura" --aot --output "$out/$t.exe" > "$out/build.log" 2>&1; then
    echo "BUILD FAIL"
    grep -iE "error|undefined|redefinition" "$out/build.log" | head -3
    fail=$((fail+1)); failed_list="$failed_list $t(build)"; continue
  fi
  timeout 60 "$out/$t.exe" > "$out/run.log" 2>&1
  rc=$?
  if [ $rc -ne 0 ]; then
    echo "RUN FAIL exit=$rc"
    tail -4 "$out/run.log"
    fail=$((fail+1)); failed_list="$failed_list $t(run:$rc)"; continue
  fi
  summary=$(grep -E "passed|FAIL" "$out/run.log" | tail -1)
  echo "OK  $summary"
  pass=$((pass+1))
done

echo "----------------------------------------------"
echo "AOT concurrent: $pass/9 passed; failed:$failed_list"
