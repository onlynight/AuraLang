#!/usr/bin/env bash
# -------------------------------------------------------------
# Dual-VM isolation guard (VM-PA-00 v3.2, P4.5 / ADR-005)
# Enforces decision D6: Rust VM and Aura VM are two independent
# implementations that must never reference each other.
#   - aura/compiler/**/*.aura must not reference Rust VM symbols
#   - seed/compiler/src/vm/*.rs must not reference Aura VM symbols
#   - every Rust VM file must carry the freeze-baseline marker (P4.3)
# Sole allowed intersection: the .auc binary format itself.
# See scripts/check-dual-impl-isolation.ps1 for the same constraints.
# -------------------------------------------------------------
set -u
cd "$(dirname "$0")/.." || exit 1

violations=0

is_comment() {
    # $1 = line; comment if it starts with optional spaces then // or #
    echo "$1" | grep -Eq '^[[:space:]]*(//|#)'
}

# --- Constraint 1: aura/compiler/**/*.aura, no Rust VM symbols (non-comment) ---
RUST_VM_TOKENS='compiler::vm|interp::|Value::Int|jit_compile_cranelift|VmOptions|seed/compiler/src/vm'
while IFS= read -r -d '' f; do
    n=0
    while IFS= read -r line; do
        n=$((n + 1))
        case "$line" in
            *compiler::vm*|*interp::*|*Value::Int*|*jit_compile_cranelift*|*VmOptions*|*seed/compiler/src/vm*)
                if ! is_comment "$line"; then
                    echo "VIOLATION [constraint 1: aura->rust-vm] $f:$n: $line"
                    violations=$((violations + 1))
                fi
                ;;
        esac
    done < "$f"
done < <(find aura/compiler -type f -name '*.aura' -print0)

# --- Constraint 2: seed/compiler/src/vm/*.rs, no Aura VM symbols (non-comment) ---
while IFS= read -r -d '' f; do
    n=0
    while IFS= read -r line; do
        n=$((n + 1))
        case "$line" in
            *AucLoader*|*VmRunner*|*Vm.aura*|*aura/lang/compiler/vm*|*aura/compiler/*)
                if ! is_comment "$line"; then
                    echo "VIOLATION [constraint 2: rust-vm->aura] $f:$n: $line"
                    violations=$((violations + 1))
                fi
                ;;
        esac
    done < "$f"
done < <(find seed/compiler/src/vm -type f -name '*.rs' -print0)

# --- Constraint 3: freeze-baseline marker present in every Rust VM file (P4.3) ---
while IFS= read -r -d '' f; do
    if ! grep -q 'FROZEN BASELINE' "$f"; then
        echo "VIOLATION [constraint 3: freeze marker] $f: missing D6 freeze-baseline marker"
        violations=$((violations + 1))
    fi
done < <(find seed/compiler/src/vm -type f -name '*.rs' -print0)

# --- Constraint 4: coroutine.rs carries the D5 legacy marker ---
coroutine_rs=seed/compiler/src/vm/coroutine.rs
if [ -f "$coroutine_rs" ]; then
    if ! grep -q 'LEGACY per D5' "$coroutine_rs"; then
        echo "VIOLATION [constraint 4] $coroutine_rs: D5 legacy marker missing"
        violations=$((violations + 1))
    fi
else
    echo "VIOLATION [constraint 4] $coroutine_rs not found"
    violations=$((violations + 1))
fi

if [ "$violations" -gt 0 ]; then
    echo "=== dual-impl isolation guard: FAIL ($violations violation(s)) ==="
    exit 1
fi
echo "=== dual-impl isolation guard: PASS ==="
exit 0
