#!/usr/bin/env python3
"""Fix spawnActor misassignment and add full prelu-name aliases."""

from pathlib import Path

NATIVE = Path(r"D:\Code\AuraLang\compiler\src\vm\native.rs")

# Fix spawnActor mapping
CORTEX_FIX = [
    ('"aura.lang.std.Coroutine.spawnActor"', '"aura.lang.std.Actor.spawnActor"'),
    ('"aura.lang.std.Coroutine.spawnActorProcess"', '"aura.lang.std.Actor.spawnActorProcess"'),
]

# Aliases to add (short_name, full_name, native_fn_path)
ALIAS_PAIRS = [
    ("println",       "aura.lang.std.println",       "native_println"),
    ("print",         "aura.lang.std.print",         "native_print"),
    ("puts",          "aura.lang.std.puts",          "native_puts"),
    ("abs",           "aura.lang.std.abs",           "native_abs"),
    ("sqrt",          "aura.lang.std.sqrt",          "native_sqrt"),
    ("pow",           "aura.lang.std.pow",           "native_pow"),
    ("toInt",         "aura.lang.std.toInt",         "native_to_int"),
    ("toFloat",       "aura.lang.std.toFloat",       "native_to_float"),
    ("toStr",         "aura.lang.std.toStr",         "native_to_str"),
    ("toString",      "aura.lang.std.toString",      "native_to_str"),  # same impl
    ("clock",         "aura.lang.std.clock",         "native_clock"),
    ("strlen",        "aura.lang.std.strlen",        "native_strlen"),
    ("CString",       "aura.lang.std.CString",       "native_cstring"),
    ("CStr",          "aura.lang.std.CStr",          "native_cstr"),
    ("ptrIsNull",     "aura.lang.std.ptrIsNull",     "native_ptr_is_null"),
    ("ptrToInt",      "aura.lang.std.ptrToInt",      "native_ptr_to_int"),
    ("intToPtr",      "aura.lang.std.intToPtr",      "native_int_to_ptr"),
    ("makeCallback",  "aura.lang.std.makeCallback",  "native_make_callback"),
    ("aura_isOfType", "aura.lang.std.aura_isOfType", "native_is_of_type"),
    ("equals",        "aura.lang.std.equals",        "native_equals"),
    ("hashCode",      "aura.lang.std.hashCode",      "native_hash_code"),
    ("typeOf",        "aura.lang.std.typeOf",        "native_type_of"),
    ("aura_cast",     "aura.lang.std.aura_cast",     "native_cast"),
    ("aura_cast_safety", "aura.lang.std.aura_cast_safety", "native_cast_safety"),
]

def main():
    text = NATIVE.read_text(encoding="utf-8")

    # 1) Fix spawnActor misassignments
    for old, new in CORTEX_FIX:
        text = text.replace(old, new)

    # 2) Insert alias block after both "register prelude" sections
    # Anchor: the last prelu registration in each block, followed by a marker comment.
    # There are 2 prelude blocks. We insert aliases right after each.
    #
    # Block 1 (native.rs::new) — after `r.register("aura_isOfType", native_is_of_type);`
    #     and before the `// Phase 4` comment (Phase 4 comment is INSIDE the prelude block).
    #     Actually there are two such markers. We'll target by unique surrounding context.
    #
    # We'll add aliases just before each "// Fix 9:" marker in new() and before "// P10:" in
    # with_modules() that's after the prelude block.

    # Build the alias block text
    alias_lines = []
    alias_lines.append("")
    alias_lines.append("        // Full-name aliases for prelude (aura.lang.std.<fn>)")
    for short, full, fn in ALIAS_PAIRS:
        alias_lines.append(f'        r.register("{full}", {fn});')
    alias_lines.append("")
    alias_block = "\n".join(alias_lines)

    # Insert into NativeRegistry::new — before the "// Fix 9:" comment
    anchor1 = '''        // Fix 9: 榛樿养浠呭姞杞?prelu锛屼笉鍔犺浇鍏ㄩ儴 std 妯″潡
        // 濡傞渶鍔犺浇 std 妯″潡锛屼娇鐢?NativeRegistry::with_modules()'''
    if anchor1 in text:
        text = text.replace(anchor1, alias_block + anchor1)
    else:
        print("WARN: anchor1 not found in new()")

    # Insert into NativeRegistry::with_modules — before "// P10: 并发运行时"
    # The marker "// P10:" appears twice. We want the one AFTER the prelude section in with_modules.
    # Approach: find "// P10:" occurrences, only replace the second one (in with_modules).
    idx1 = text.find("// P10:")
    if idx1 >= 0:
        idx2 = text.find("// P10:", idx1 + 1)
        if idx2 >= 0:
            # Insert before idx2
            text = text[:idx2] + alias_block + text[idx2:]
        else:
            print("WARN: only one P10 marker found; skipped alias in with_modules")
    else:
        print("WARN: no P10 marker found")

    NATIVE.write_text(text, encoding="utf-8")

    # Verify
    count_full = text.count("aura.lang.std.")
    count_coroutine_spawnactor = text.count("aura.lang.std.Coroutine.spawnActor")
    count_actor_spawnactor = text.count("aura.lang.std.Actor.spawnActor")
    count_alias_prelude = text.count("aura.lang.std.println")
    print(f"total 'aura.lang.std.' occurrences: {count_full}")
    print(f"Coroutine.spawnActor (should be 0): {count_coroutine_spawnactor}")
    print(f"Actor.spawnActor (should be 2):     {count_actor_spawnactor}")
    print(f"aura.lang.std.println alias:        {count_alias_prelude}")


if __name__ == "__main__":
    main()
