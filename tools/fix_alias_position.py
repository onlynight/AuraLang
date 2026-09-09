#!/usr/bin/env python3
"""Move alias block from with_modules() to new() properly."""

from pathlib import Path

NATIVE = Path(r"D:\Code\AuraLang\compiler\src\vm\native.rs")

text = NATIVE.read_text(encoding="utf-8")

# Find the alias block (currently misplaced in with_modules)
# It starts with a blank line then "// Full-name aliases for prelude" comment.
alias_marker = "\n        // Full-name aliases for prelude (aura.lang.std.<fn>)"
idx = text.find(alias_marker)
if idx < 0:
    print("ERR: alias block not found")
    exit(1)

# Find the end of the alias block — it ends right before the "// P10:" marker that follows it.
end_idx = text.find("\n        // P10:", idx)
if end_idx < 0:
    end_idx = text.find("\n// P10:", idx)
if end_idx < 0:
    print("ERR: cannot find end of alias block")
    exit(1)

# Extract the block (from alias_marker to end_idx, but keep the leading "\n        ")
alias_block = text[idx:end_idx]
print(f"Alias block length: {len(alias_block)} chars")

# Remove the block from its current position
text_no_block = text[:idx] + text[end_idx:]

# Find the correct insertion point: in NativeRegistry::new, before "// Fix 9:"
# "// Fix 9:" appears exactly once (in new()).
fix9_marker = "\n        // Fix 9:"
fix9_idx = text_no_block.find(fix9_marker)
if fix9_idx < 0:
    print("ERR: // Fix 9: marker not found")
    exit(1)

# Insert alias block BEFORE "// Fix 9:"
# The alias block currently starts with "\n        // Full-name..." — that's fine, it's already indented properly.
new_text = text_no_block[:fix9_idx] + alias_block + text_no_block[fix9_idx:]

NATIVE.write_text(new_text, encoding="utf-8")

# Verify
count_println_alias = new_text.count("aura.lang.std.println")
count_fix9 = new_text.count("// Fix 9:")
count_p10 = new_text.count("// P10:")
print(f"println alias count: {count_println_alias} (should be 2: new + with_modules)")
print(f"// Fix 9: count: {count_fix9} (should be 1)")
print(f"// P10: count: {count_p10} (should be 2)")
