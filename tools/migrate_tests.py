# -*- coding: utf-8 -*-
"""Third-pass: fix test files and remaining src/comments with old aura.concurrent/aura.X syntax.

This is a targeted string substitution to bring test files and remaining references
to the new namespace scheme.
"""

from pathlib import Path

ROOT = Path(r"D:\Code\AuraLang")

# Ordered string substitutions (most specific first)
REPLACEMENTS = [
    # Concurrent split (functional tests first, then doc examples)
    ("import aura.concurrent.* as",    "import aura.lang.std.Coroutine.* as"),  # keep wildcard as Coroutine.*
    ("import aura.concurrent.*",        "import aura.lang.std.Coroutine.*"),
    ("import aura.concurrent as",       "import aura.lang.std.Coroutine as"),
    ("import aura.concurrent",          "import aura.lang.std.Coroutine"),
    ("\"aura.concurrent\"",             "\"aura.lang.std.Coroutine\""),
    ("'aura.concurrent'",              "'aura.lang.std.Coroutine'"),
    # Old namespace bare references (word-bounded)
    ("aura.io",      "aura.lang.std.IO"),
    ("aura.math",    "aura.lang.std.Math"),
    ("aura.string",  "aura.lang.std.String"),
    ("aura.net",     "aura.lang.std.Network"),
    ("aura.fs",      "aura.lang.std.FileSystem"),
    ("aura.time",    "aura.lang.std.Time"),
    ("aura.json",    "aura.lang.std.Json"),
    ("aura.encoding", "aura.lang.std.Encoding"),
    ("aura.ascii",   "aura.lang.std.Ascii"),
    ("aura.collections", "aura.lang.std.Collections"),
    ("aura.console", "aura.lang.std.Console"),
    ("aura.env",     "aura.lang.std.Env"),
    ("aura.path",    "aura.lang.std.Path"),
    ("aura.process", "aura.lang.std.Process"),
    ("aura.random",  "aura.lang.std.Random"),
    ("aura.test",    "aura.lang.std.Test"),
    ("aura.iter",    "aura.lang.std.Iter"),
    ("aura.assert",  "aura.lang.std.Assert"),
    ("aura.builtin", "aura.lang.std.Builtin"),
]

# Files we want to touch
TARGETS = [
    "compiler/tests/import_syntax_tests.rs",
    "compiler/tests/p1c_volume_tests.rs",
    "compiler/tests/p9_std_tests.rs",
    "compiler/tests/p10_concurrency_tests.rs",
    "compiler/tests/std_selective_load_tests.rs",
    "compiler/tests/docgen_supplement_tests.rs",
    "compiler/tests/p1_prelude_tests.rs",
    "compiler/tests/p3_feature_tests.rs",
    "compiler/tests/p1_prelude_tests.rs",
]


def migrate(text: str) -> tuple[str, int]:
    changes = 0
    for old, new in REPLACEMENTS:
        count = text.count(old)
        if count > 0:
            text = text.replace(old, new)
            changes += count
    return text, changes


def main():
    total = 0
    for rel in TARGETS:
        p = ROOT / rel
        if not p.exists():
            print(f"MISSING: {rel}")
            continue
        orig = p.read_text(encoding="utf-8", errors="replace")
        new, n = migrate(orig)
        if new != orig:
            p.write_text(new, encoding="utf-8")
            print(f"{rel}: {n} replacements")
            total += n
        else:
            print(f"{rel}: no changes")
    print(f"\nTotal replacements: {total}")


if __name__ == "__main__":
    main()
