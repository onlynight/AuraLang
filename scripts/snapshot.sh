#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# 源码快照对比机制（纯 Aura 化迁移 · Phase 0）
#
# 对 tests/snapshots/cases/ 下的用例抓取 Rust 编译器的词法/语法输出，
# 基线存放于 tests/snapshots/baseline/。
#
# Phase 0：基线由 Rust 编译器（参考实现）生成，check 验证机制本身稳定。
# Phase 1+：用 --compiler <Aura 编译器路径> 将 Aura 实现与同一基线对比。
#
# 用法：
#   scripts/snapshot.sh --update                 # 重新生成基线
#   scripts/snapshot.sh                          # 与基线对比
#   scripts/snapshot.sh --compiler ./build/aura-compiler
# ─────────────────────────────────────────────────────────────
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

CASES_DIR="tests/snapshots/cases"
BASE_DIR="tests/snapshots/baseline"

UPDATE=0
COMPILER=""

while [ $# -gt 0 ]; do
    case "$1" in
        --update) UPDATE=1 ;;
        --compiler)
            shift
            COMPILER="${1:-}"
            ;;
        -h|--help)
            echo "用法: scripts/snapshot.sh [--update] [--compiler <path>]"
            exit 0
            ;;
        *)
            echo "[snapshot] 未知参数: $1" >&2
            exit 1
            ;;
    esac
    shift
done

find_aura() {
    for candidate in \
        "target/release/aura.exe" "target/release/aura" \
        "target/debug/aura.exe" "target/debug/aura"; do
        if [ -x "$candidate" ]; then echo "$candidate"; return 0; fi
    done
    return 1
}

if [ -n "$COMPILER" ]; then
    AURA="$COMPILER"
else
    AURA="$(find_aura || true)"
fi

if [ -z "$AURA" ] || [ ! -x "$AURA" ]; then
    echo "[snapshot] 错误: 未找到编译器可执行文件（可用 --compiler 指定）" >&2
    exit 1
fi

normalize() {
    # CRLF → LF，并去掉结尾的空行（与 PowerShell 版行为一致）
    awk '{ sub(/\r$/, ""); lines[n++] = $0 }
         END {
             while (n > 0 && lines[n - 1] == "") n--
             for (i = 0; i < n; i++) print lines[i]
         }'
}

if [ ! -d "$CASES_DIR" ]; then
    echo "[snapshot] 错误: 用例目录不存在: $CASES_DIR" >&2
    exit 1
fi

if [ "$UPDATE" = "1" ]; then
    mkdir -p "$BASE_DIR"
fi

echo "[snapshot] compiler: $AURA"
if [ "$UPDATE" = "1" ]; then echo "[snapshot] mode:     UPDATE"; else echo "[snapshot] mode:     CHECK"; fi
echo ""

failures=0
checked=0

for case_file in "$CASES_DIR"/*.aura; do
    [ -e "$case_file" ] || continue
    name="$(basename "$case_file" .aura)"

    for kind in tokens ast; do
        rel="$BASE_DIR/$name.$kind.txt"
        if ! actual="$("$AURA" "$kind" "$case_file" 2>/dev/null | normalize)"; then
            echo "[snapshot] FAIL  $rel ($kind, 编译器出错)"
            failures=$((failures + 1))
            continue
        fi

        if [ "$UPDATE" = "1" ]; then
            printf '%s\n' "$actual" > "$rel"
            echo "[snapshot] WROTE $rel"
            continue
        fi

        if [ ! -f "$rel" ]; then
            echo "[snapshot] MISS  $rel (运行 --update)"
            failures=$((failures + 1))
            continue
        fi

        checked=$((checked + 1))
        expected="$(cat "$rel" | normalize)"
        if [ "$expected" = "$actual" ]; then
            echo "[snapshot] ok    $rel"
        else
            echo "[snapshot] DIFF  $rel"
            diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") | head -n 10 || true
            failures=$((failures + 1))
        fi
    done
done

echo ""
if [ "$UPDATE" = "1" ]; then
    echo "[snapshot] ✓ 基线已更新"
    exit 0
fi

if [ "$failures" -eq 0 ]; then
    echo "[snapshot] ✓ OK: $checked 个快照一致"
    exit 0
fi

echo "[snapshot] ✗ 失败: $failures 个快照不一致或缺失" >&2
exit 1
