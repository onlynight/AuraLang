#!/usr/bin/env bash
# -------------------------------------------------------------
# Bootstrap isolation guard (VM-PA-00 v3.1, P0.1 / ADR-004)
# Enforces decision D1: bootstrap/ is kept but never referenced.
# See scripts/check-bootstrap-isolation.ps1 for the constraint list.
# -------------------------------------------------------------
set -u
cd "$(dirname "$0")/.." || exit 1

violations=0

is_comment() {
    # $1 = line; comment if it starts with optional spaces then // or #
    echo "$1" | grep -Eq '^[[:space:]]*(//|#)'
}

# --- Constraint 1: aura/compiler/**/*.aura, non-comment refs forbidden ---
while IFS= read -r -d '' f; do
    n=0
    while IFS= read -r line; do
        n=$((n + 1))
        case "$line" in
            *bootstrap*)
                if ! is_comment "$line"; then
                    echo "VIOLATION [constraint 1] $f:$n: $line"
                    violations=$((violations + 1))
                fi
                ;;
        esac
    done < "$f"
done < <(find aura/compiler -type f -name '*.aura' -print0)

# --- Constraint 2: seed/compiler/src/vm/, zero references ---
while IFS= read -r -d '' f; do
    n=0
    while IFS= read -r line; do
        n=$((n + 1))
        case "$line" in
            *bootstrap*)
                echo "VIOLATION [constraint 2] $f:$n: $line"
                violations=$((violations + 1))
                ;;
        esac
    done < "$f"
done < <(find seed/compiler/src/vm -type f -name '*.rs' -print0)

# --- Constraint 3: rest of seed/compiler/src, comments + lib.rs mount only ---
while IFS= read -r -d '' f; do
    case "$f" in
        *bootstrap/*) continue ;;
    esac
    n=0
    while IFS= read -r line; do
        n=$((n + 1))
        case "$line" in
            *bootstrap*)
                is_comment "$line" && continue
                case "$f" in
                    */lib.rs)
                        [ "$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')" = "pub mod bootstrap;" ] && continue
                        ;;
                esac
                echo "VIOLATION [constraint 3] $f:$n: $line"
                violations=$((violations + 1))
                ;;
        esac
    done < "$f"
done < <(find seed/compiler/src -type f -name '*.rs' -print0)

# --- Constraint 4: isolation marker in bootstrap/mod.rs ---
mod_rs=seed/compiler/src/bootstrap/mod.rs
if [ -f "$mod_rs" ]; then
    if ! head -20 "$mod_rs" | grep -Eq 'ISOLATED.*DO NOT REFERENCE|保留但不引用'; then
        echo "VIOLATION [constraint 4] $mod_rs: isolation marker missing (see ADR-004)"
        violations=$((violations + 1))
    fi
else
    echo "VIOLATION [constraint 4] $mod_rs not found"
    violations=$((violations + 1))
fi

if [ "$violations" -gt 0 ]; then
    echo "=== bootstrap isolation guard: FAIL ($violations violation(s)) ==="
    exit 1
fi
echo "=== bootstrap isolation guard: PASS ==="
exit 0
