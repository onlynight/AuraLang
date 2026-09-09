# -*- coding: utf-8 -*-
"""Second-pass rename across all remaining Rust sources.

Fixes the path-matching bug (Windows uses backslashes).
"""
from pathlib import Path

ROOT = Path(r"D:\Code\AuraLang")
EXCLUDE_NAMES = {"decl.rs.bak"}

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


def in_scope(rel: Path) -> bool:
    """Return True if the file is under compiler/src, compiler/tests, or compiler/benches."""
    parts = rel.parts
    return len(parts) >= 2 and parts[0] == "compiler" and parts[1] in ("src", "tests", "benches", "examples")


def migrate(text: str) -> tuple[str, int]:
    changes = 0
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
    modified = []
    for p in ROOT.rglob("*.rs"):
        rel = p.relative_to(ROOT)
        if p.name in EXCLUDE_NAMES:
            continue
        if not in_scope(rel):
            continue
        orig = p.read_text(encoding="utf-8", errors="replace")
        new, n = migrate(orig)
        if new != orig:
            p.write_text(new, encoding="utf-8")
            print(f"{rel}: {n} replacements")
            total += n
            modified.append(rel)
    print(f"\nModified {len(modified)} files, {total} total replacements")


if __name__ == "__main__":
    main()
