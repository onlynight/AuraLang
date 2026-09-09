#!/usr/bin/env python3
"""
Migrate decl.rs to the new aura.lang.std.* namespace scheme.

Changes:
- Rename all aura.<module>.<fn> -> aura.lang.std.<ClassName>.<fn>
- Split aura.concurrent.* into Coroutine/Actor/Channel under aura.lang.std
- Add prelu functions with full aura.lang.std.<fn> names (in addition to short names)
- Keep the short prelude names unchanged
"""

from pathlib import Path

SRC = Path(r"D:\Code\AuraLang\compiler\src\std\decl.rs")

# ---- Prefix renames (simple, one-to-one) ----
PREFIX_MAP = [
    ("aura.ascii.",     "aura.lang.std.Ascii."),
    ("aura.assert.",    "aura.lang.std.Assert."),
    ("aura.builtin.",   "aura.lang.std.Builtin."),
    ("aura.collections.", "aura.lang.std.Collections."),
    ("aura.console.",   "aura.lang.std.Console."),
    ("aura.encoding.",  "aura.lang.std.Encoding."),
    ("aura.env.",       "aura.lang.std.Env."),
    ("aura.fs.",        "aura.lang.std.FileSystem."),
    ("aura.io.",        "aura.lang.std.IO."),
    ("aura.iter.",      "aura.lang.std.Iter."),
    ("aura.json.",      "aura.lang.std.Json."),
    ("aura.math.",      "aura.lang.std.Math."),
    ("aura.net.",       "aura.lang.std.Network."),
    ("aura.path.",      "aura.lang.std.Path."),
    ("aura.process.",   "aura.lang.std.Process."),
    ("aura.random.",    "aura.lang.std.Random."),
    ("aura.string.",    "aura.lang.std.String."),
    ("aura.test.",      "aura.lang.std.Test."),
    ("aura.time.",      "aura.lang.std.Time."),
]

# ---- Explicit concurrent function mapping (must be applied before prefix renames
# since aura.concurrent isn't in PREFIX_MAP; we do it separately) ----
CONCURRENT_MAP = {
    # Coroutine
    "aura.concurrent.spawn":              "aura.lang.std.Coroutine.spawn",
    "aura.concurrent.ask":                "aura.lang.std.Coroutine.ask",
    # Actor
    "aura.concurrent.send":               "aura.lang.std.Actor.send",
    "aura.concurrent.reply":              "aura.lang.std.Actor.reply",
    "aura.concurrent.spawnActor":         "aura.lang.std.Actor.spawnActor",
    "aura.concurrent.supervise":          "aura.lang.std.Actor.supervise",
    "aura.concurrent.actorAlive":         "aura.lang.std.Actor.actorAlive",
    "aura.concurrent.spawnActorProcess":  "aura.lang.std.Actor.spawnActorProcess",
    "aura.concurrent.sendProcessActor":   "aura.lang.std.Actor.sendProcessActor",
    "aura.concurrent.recvProcessActor":   "aura.lang.std.Actor.recvProcessActor",
    "aura.concurrent.processActorAlive":  "aura.lang.std.Actor.processActorAlive",
    "aura.concurrent.killProcessActor":   "aura.lang.std.Actor.killProcessActor",
    # Channel
    "aura.concurrent.newChannel":         "aura.lang.std.Channel.newChannel",
    "aura.concurrent.channelSend":        "aura.lang.std.Channel.channelSend",
    "aura.concurrent.channelRecv":        "aura.lang.std.Channel.channelRecv",
    "aura.concurrent.channelTryRecv":     "aura.lang.std.Channel.channelTryRecv",
    "aura.concurrent.select":             "aura.lang.std.Channel.select",
    "aura.concurrent.selectTimeout":      "aura.lang.std.Channel.selectTimeout",
    "aura.concurrent.newTcpChannel":      "aura.lang.std.Channel.newTcpChannel",
    "aura.concurrent.tcpChannelSend":     "aura.lang.std.Channel.tcpChannelSend",
}

# ---- Add full names for prelu functions ----
# PRELUDE_NAMES (short) -> aura.lang.std.<short>  (all of them)
PRELUDE_NAMES = [
    "println", "print", "puts", "abs", "sqrt", "pow",
    "toInt", "toFloat", "toStr", "toString",
    "clock", "strlen",
    "CString", "CStr",
    "ptrIsNull", "ptrToInt", "intToPtr",
    "makeCallback",
    "listOf",
    "typeof", "isNull", "isNotNull",
    "isZero", "isPositive", "isNegative",
    "toBool",
    "sizeOf", "hash", "compare", "clone", "identity",
    "assertTrue", "assertFalse",
    "assertEq", "assertNotEq",
    "assertNotNull", "assertNull",
    "assertContains", "assertNotContains",
    "assertGt", "assertGte",
    "assertLt", "assertLte",
    "assertApprox", "assertArrayEq", "assertMapEq",
    "pass", "fail",
    "equals", "hashCode", "typeOf",
    "aura_isOfType", "aura_cast", "aura_cast_safety",
]


def migrate(text: str) -> str:
    # 1) Concurrent functions (must come first so they don't collide with generic prefix rules)
    for old, new in CONCURRENT_MAP.items():
        text = text.replace(old, new)

    # 2) Generic module prefixes
    for old, new in PREFIX_MAP:
        text = text.replace(old, new)

    # 3) Inject aura.lang.std.<prelu> full-name block right after the short-name block
    #    in build_all_names. We locate the closing of the "顶层内置" block:
    #        "makeCallback",
    #    ] {
    #        s.insert(n);
    #    }
    #    and append a new block after it.
    anchor = '''    for n in [
        "println",
        "print",
        "puts",
        "abs",
        "sqrt",
        "pow",
        "toInt",
        "toFloat",
        "toStr",
        "toString",
        "clock",
        "strlen",
        "CString",
        "CStr",
        "ptrIsNull",
        "ptrToInt",
        "intToPtr",
        "makeCallback",
    ] {
        s.insert(n);
    }'''
    assert anchor in text, "short prelu anchor not found!"
    injection = anchor + '''

    // ── 顶层内置的全名别名（aura.lang.std.<fn>，用于 import 展开和点分全名调用）──
    for n in [
        "aura.lang.std.println",
        "aura.lang.std.print",
        "aura.lang.std.puts",
        "aura.lang.std.abs",
        "aura.lang.std.sqrt",
        "aura.lang.std.pow",
        "aura.lang.std.toInt",
        "aura.lang.std.toFloat",
        "aura.lang.std.toStr",
        "aura.lang.std.toString",
        "aura.lang.std.clock",
        "aura.lang.std.strlen",
        "aura.lang.std.CString",
        "aura.lang.std.CStr",
        "aura.lang.std.ptrIsNull",
        "aura.lang.std.ptrToInt",
        "aura.lang.std.intToPtr",
        "aura.lang.std.makeCallback",
        "aura.lang.std.listOf",
        "aura.lang.std.typeof",
        "aura.lang.std.isNull",
        "aura.lang.std.isNotNull",
        "aura.lang.std.isZero",
        "aura.lang.std.isPositive",
        "aura.lang.std.isNegative",
        "aura.lang.std.toBool",
        "aura.lang.std.sizeOf",
        "aura.lang.std.hash",
        "aura.lang.std.compare",
        "aura.lang.std.clone",
        "aura.lang.std.identity",
        "aura.lang.std.assertTrue",
        "aura.lang.std.assertFalse",
        "aura.lang.std.assertEq",
        "aura.lang.std.assertNotEq",
        "aura.lang.std.assertNotNull",
        "aura.lang.std.assertNull",
        "aura.lang.std.assertContains",
        "aura.lang.std.assertNotContains",
        "aura.lang.std.assertGt",
        "aura.lang.std.assertGte",
        "aura.lang.std.assertLt",
        "aura.lang.std.assertLte",
        "aura.lang.std.assertApprox",
        "aura.lang.std.assertArrayEq",
        "aura.lang.std.assertMapEq",
        "aura.lang.std.pass",
        "aura.lang.std.fail",
        "aura.lang.std.equals",
        "aura.lang.std.hashCode",
        "aura.lang.std.typeOf",
        "aura.lang.std.aura_isOfType",
        "aura.lang.std.aura_cast",
        "aura.lang.std.aura_cast_safety",
    ] {
        s.insert(n);
    }'''
    text = text.replace(anchor, injection)

    # 4) Add aura.lang.std.<prelu> names to PRELUDE_NAMES constant (so is_prelude works)
    prelu_anchor = '''    "aura_isOfType",
    "aura_cast",
    "aura_cast_safety",
];'''
    prelu_injection = '''    "aura_isOfType",
    "aura_cast",
    "aura_cast_safety",
    // ── 全名别名（免 import 也可用）──
    "aura.lang.std.println",
    "aura.lang.std.print",
    "aura.lang.std.puts",
    "aura.lang.std.abs",
    "aura.lang.std.sqrt",
    "aura.lang.std.pow",
    "aura.lang.std.toInt",
    "aura.lang.std.toFloat",
    "aura.lang.std.toStr",
    "aura.lang.std.toString",
    "aura.lang.std.clock",
    "aura.lang.std.strlen",
    "aura.lang.std.CString",
    "aura.lang.std.CStr",
    "aura.lang.std.ptrIsNull",
    "aura.lang.std.ptrToInt",
    "aura.lang.std.intToPtr",
    "aura.lang.std.makeCallback",
    "aura.lang.std.listOf",
    "aura.lang.std.typeof",
    "aura.lang.std.isNull",
    "aura.lang.std.isNotNull",
    "aura.lang.std.isZero",
    "aura.lang.std.isPositive",
    "aura.lang.std.isNegative",
    "aura.lang.std.toBool",
    "aura.lang.std.sizeOf",
    "aura.lang.std.hash",
    "aura.lang.std.compare",
    "aura.lang.std.clone",
    "aura.lang.std.identity",
    "aura.lang.std.assertTrue",
    "aura.lang.std.assertFalse",
    "aura.lang.std.assertEq",
    "aura.lang.std.assertNotEq",
    "aura.lang.std.assertNotNull",
    "aura.lang.std.assertNull",
    "aura.lang.std.assertContains",
    "aura.lang.std.assertNotContains",
    "aura.lang.std.assertGt",
    "aura.lang.std.assertGte",
    "aura.lang.std.assertLt",
    "aura.lang.std.assertLte",
    "aura.lang.std.assertApprox",
    "aura.lang.std.assertArrayEq",
    "aura.lang.std.assertMapEq",
    "aura.lang.std.pass",
    "aura.lang.std.fail",
    "aura.lang.std.equals",
    "aura.lang.std.hashCode",
    "aura.lang.std.typeOf",
    "aura.lang.std.aura_isOfType",
    "aura.lang.std.aura_cast",
    "aura.lang.std.aura_cast_safety",
];'''
    assert prelu_anchor in text, "PRELUDE_NAMES tail anchor not found!"
    text = text.replace(prelu_anchor, prelu_injection)

    # 5) Update the test that checks "aura.math.sin" and "aura.concurrent.spawn"
    text = text.replace(
        'assert!(is_builtin("aura.lang.std.Math.sin"));',
        'assert!(is_builtin("aura.lang.std.Math.sin"));',
    )  # already renamed
    text = text.replace(
        'assert!(is_builtin("aura.lang.std.Coroutine.spawn"));',
        'assert!(is_builtin("aura.lang.std.Coroutine.spawn"));',
    )  # already renamed

    # 6) Update the module-prefix test list
    old_test = '''        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Ascii.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Assert.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Builtin.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Collections.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Console.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Encoding.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Env.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.FileSystem.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.IO.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Iter.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Json.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Math.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Network.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Path.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Process.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Random.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.String.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Test.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Time.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Coroutine.")));'''
    new_test = '''        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Ascii.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Assert.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Builtin.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Collections.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Console.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Encoding.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Env.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.FileSystem.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.IO.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Iter.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Json.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Math.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Network.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Path.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Process.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Random.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.String.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Test.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Time.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Coroutine.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Actor.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Channel.")));'''
    if old_test in text:
        text = text.replace(old_test, new_test)

    # 7) Also add explicit concurrent test lines (they were already renamed, but add Channel & Actor)
    text = text.replace(
        'assert!(is_builtin("aura.lang.std.Coroutine.spawn"));',
        'assert!(is_builtin("aura.lang.std.Coroutine.spawn"));\n        assert!(is_builtin("aura.lang.std.Actor.send"));\n        assert!(is_builtin("aura.lang.std.Channel.newChannel"));',
    )

    return text


def main():
    src = SRC.read_text(encoding="utf-8")
    new = migrate(src)
    SRC.write_text(new, encoding="utf-8")

    # Stats
    old_aura_math = src.count("aura.math.")
    new_aura_lang_std = new.count("aura.lang.std.")
    new_aura_concurrent = new.count("aura.concurrent.")
    print(f"Original aura.math.* count:    {old_aura_math}")
    print(f"New aura.lang.std.* count:     {new_aura_lang_std}")
    print(f"Residual aura.concurrent.*:    {new_aura_concurrent}")
    print(f"File size: {len(src)} -> {len(new)} bytes")


if __name__ == "__main__":
    main()
