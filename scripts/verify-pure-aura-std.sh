#!/bin/bash
# -------------------------------------------------------------
# 条件4 校验：std 加载走 Aura 源码、不依赖 Rust 预编译的 build/*.auc
#
# 两种模式：
#   * Windows(bash) 且存在 powershell → 委派给 verify-pure-aura-std.ps1 做**实证**校验
#     （隔离全部 build/*.auc 后编译 + 自举 + 还原）
#   * 其它宿主（如 CI 的 ubuntu）→ 做**静态断言**：
#       1. Aura 侧 AOT 链路源码（aot/*.aura）不出现 `.auc` 字面量
#       2. std 自动包含清单（essentialStdModules）全部指向 `.aura` 源码
#       3. Aura 版 CLI 的 AOT 链路不引用 AucLoader / .auc 读取
#
# Usage:
#   scripts/verify-pure-aura-std.sh
# -------------------------------------------------------------
set -e

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

step() { echo ""; echo "[$1]"; }
fail=0

static_checks() {
    step "Static check 1: Aura AOT backend must not mention .auc"
    # 注释里出现 .auc 是允许的会误报，故只在「非注释行」上判定。
    hits1=$(grep -rn --include='*.aura' '\.auc' aura/compiler/aura/lang/compiler/aot/ 2>/dev/null \
            | grep -v '^\s*//' | grep -v ':[0-9]*:\s*//' || true)
    if [ -n "$hits1" ]; then
        echo "  FAIL: .auc referenced in AOT backend:"
        echo "$hits1" | sed 's/^/    /'
        fail=1
    else
        echo "  OK: no .auc usage in aura/compiler/**/aot/*.aura"
    fi

    step "Static check 2: essential std modules must point at .aura sources"
    modfile="aura/compiler/aura/lang/compiler/aot/ModuleLink.aura"
    if [ ! -f "$modfile" ]; then
        echo "  FAIL: $modfile not found"
        fail=1
    else
        # 只统计**字符串字面量内**的模块路径条目（避免注释干扰）：
        # 形如 "aura/core/aura/lang/.../X.aura\n"
        total=$(grep -c '"aura/core/aura/lang' "$modfile" || true)
        as_aura=$(grep -c '"aura/core/aura/lang[^"]*\.aura' "$modfile" || true)
        if [ "$total" -gt 0 ] && [ "$total" -eq "$as_aura" ]; then
            echo "  OK: $as_aura/$total std module entries are .aura sources"
        else
            echo "  FAIL: std module entries $as_aura/$total point at .aura"
            fail=1
        fi
    fi

    step "Static check 3: Aura CLI AOT path must not load .auc"
    hits3=$(grep -rn --include='*.aura' -E 'AucLoader|\.auc"' aura/toolchain/cli/ 2>/dev/null \
            | grep -v 'Disassemble' || true)
    if [ -n "$hits3" ]; then
        echo "  FAIL: .auc loading found in Aura CLI:"
        echo "$hits3" | sed 's/^/    /'
        fail=1
    else
        echo "  OK: no .auc loading in aura/toolchain/cli"
    fi
}

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        if command -v powershell.exe >/dev/null 2>&1; then
            echo "Windows host detected -> delegating to verify-pure-aura-std.ps1 (empirical)"
            exec powershell.exe -NoProfile -ExecutionPolicy Bypass \
                -File "$(cygpath -w "$ROOT_DIR/scripts/verify-pure-aura-std.ps1")" "$@"
        fi
        echo "Windows host without powershell -> running static checks only"
        static_checks
        ;;
    *)
        echo "Non-Windows host (CI) -> running static checks only"
        echo "  (full empirical verification requires Windows + LLVM; see verify-pure-aura-std.ps1)"
        static_checks
        ;;
esac

echo ""
if [ "$fail" -eq 0 ]; then
    echo "=============================================================="
    echo "  PASS: std comes from Aura sources; no .auc dependency"
    echo "=============================================================="
else
    echo "=============================================================="
    echo "  FAIL: pure-Aura std loading verification failed"
    echo "=============================================================="
fi
exit "$fail"
