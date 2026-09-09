#!/usr/bin/env python3
"""
Apply the same namespace renames to all Rust files under compiler/src/std/,
compiler/src/vm/native.rs, compiler/src/codegen/hir.rs, compiler/src/docgen.rs,
compiler/src/std/source_index.rs.

This is the mechanical string-rename phase. Semantic changes (checker.rs
import expansion, FFI symbol mangling) are handled separately.
"""

from pathlib import Path

ROOT = Path(r"D:\Code\AuraLang")

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

CONCURRENT_MAP = {
    "aura.concurrent.spawn":              "aura.lang.std.Coroutine.spawn",
    "aura.concurrent.ask":                "aura.lang.std.Coroutine.ask",
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
    "aura.concurrent.newChannel":         "aura.lang.std.Channel.newChannel",
    "aura.concurrent.channelSend":        "aura.lang.std.Channel.channelSend",
    "aura.concurrent.channelRecv":        "aura.lang.std.Channel.channelRecv",
    "aura.concurrent.channelTryRecv":     "aura.lang.std.Channel.channelTryRecv",
    "aura.concurrent.select":             "aura.lang.std.Channel.select",
    "aura.concurrent.selectTimeout":      "aura.lang.std.Channel.selectTimeout",
    "aura.concurrent.newTcpChannel":      "aura.lang.std.Channel.newTcpChannel",
    "aura.concurrent.tcpChannelSend":     "aura.lang.std.Channel.tcpChannelSend",
}

# Files to process (relative to ROOT)
TARGETS = [
    "compiler/src/vm/native.rs",
    "compiler/src/std/std_ascii.rs",
    "compiler/src/std/std_assert.rs",
    "compiler/src/std/std_builtin.rs",
    "compiler/src/std/std_collections.rs",
    "compiler/src/std/std_console.rs",
    "compiler/src/std/std_encoding.rs",
    "compiler/src/std/std_env.rs",
    "compiler/src/std/std_fs.rs",
    "compiler/src/std/std_io.rs",
    "compiler/src/std/std_iter.rs",
    "compiler/src/std/std_json.rs",
    "compiler/src/std/std_math.rs",
    "compiler/src/std/std_net.rs",
    "compiler/src/std/std_path.rs",
    "compiler/src/std/std_process.rs",
    "compiler/src/std/std_random.rs",
    "compiler/src/std/std_string.rs",
    "compiler/src/std/std_test.rs",
    "compiler/src/std/std_time.rs",
    "compiler/src/std/mod.rs",
    "compiler/src/std/source_index.rs",
    "compiler/src/codegen/hir.rs",
    "compiler/src/docgen.rs",
]


def migrate(text: str) -> tuple[str, int]:
    changes = 0
    # Concurrent first (longer strings win)
    for old, new in CONCURRENT_MAP.items():
        while old in text:
            text = text.replace(old, new)
            changes += 1
    for old, new in PREFIX_MAP:
        while old in text:
            text = text.replace(old, new)
            changes += 1
    return text, changes


def main():
    total = 0
    for rel in TARGETS:
        p = ROOT / rel
        if not p.exists():
            print(f"SKIP (missing): {rel}")
            continue
        orig = p.read_text(encoding="utf-8")
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
