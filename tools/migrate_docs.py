# -*- coding: utf-8 -*-
"""Batch rename in Markdown docs. Only affects string literals, not code syntax."""

from pathlib import Path
import re

ROOT = Path(r"D:\Code\AuraLang")

# Ordered prefix renames (longest first to avoid partial matches).
# We use word boundaries where possible; otherwise we use a specific regex.
PREFIX_MAP = [
    (r"\baura\.ascii\.",     r"aura.lang.std.Ascii."),
    (r"\baura\.assert\.",    r"aura.lang.std.Assert."),
    (r"\baura\.builtin\.",   r"aura.lang.std.Builtin."),
    (r"\baura\.collections\.", r"aura.lang.std.Collections."),
    (r"\baura\.console\.",   r"aura.lang.std.Console."),
    (r"\baura\.encoding\.",  r"aura.lang.std.Encoding."),
    (r"\baura\.env\.",       r"aura.lang.std.Env."),
    (r"\baura\.fs\.",        r"aura.lang.std.FileSystem."),
    (r"\baura\.io\.",        r"aura.lang.std.IO."),
    (r"\baura\.iter\.",      r"aura.lang.std.Iter."),
    (r"\baura\.json\.",      r"aura.lang.std.Json."),
    (r"\baura\.math\.",      r"aura.lang.std.Math."),
    (r"\baura\.net\.",       r"aura.lang.std.Network."),
    (r"\baura\.path\.",      r"aura.lang.std.Path."),
    (r"\baura\.process\.",   r"aura.lang.std.Process."),
    (r"\baura\.random\.",    r"aura.lang.std.Random."),
    (r"\baura\.string\.",    r"aura.lang.std.String."),
    (r"\baura\.test\.",      r"aura.lang.std.Test."),
    (r"\baura\.time\.",      r"aura.lang.std.Time."),
]

# Concurrent split (explicit)
CONCURRENT_MAP = [
    (r"\baura\.concurrent\.spawn(?!\w)",                r"aura.lang.std.Coroutine.spawn"),
    (r"\baura\.concurrent\.ask(?!\w)",                  r"aura.lang.std.Coroutine.ask"),
    (r"\baura\.concurrent\.send(?!\w)",                 r"aura.lang.std.Actor.send"),
    (r"\baura\.concurrent\.reply(?!\w)",                r"aura.lang.std.Actor.reply"),
    (r"\baura\.concurrent\.spawnActor(?!\w)",           r"aura.lang.std.Actor.spawnActor"),
    (r"\baura\.concurrent\.supervise(?!\w)",            r"aura.lang.std.Actor.supervise"),
    (r"\baura\.concurrent\.actorAlive(?!\w)",           r"aura.lang.std.Actor.actorAlive"),
    (r"\baura\.concurrent\.spawnActorProcess(?!\w)",    r"aura.lang.std.Actor.spawnActorProcess"),
    (r"\baura\.concurrent\.sendProcessActor(?!\w)",     r"aura.lang.std.Actor.sendProcessActor"),
    (r"\baura\.concurrent\.recvProcessActor(?!\w)",     r"aura.lang.std.Actor.recvProcessActor"),
    (r"\baura\.concurrent\.processActorAlive(?!\w)",    r"aura.lang.std.Actor.processActorAlive"),
    (r"\baura\.concurrent\.killProcessActor(?!\w)",     r"aura.lang.std.Actor.killProcessActor"),
    (r"\baura\.concurrent\.newChannel(?!\w)",           r"aura.lang.std.Channel.newChannel"),
    (r"\baura\.concurrent\.channelSend(?!\w)",          r"aura.lang.std.Channel.channelSend"),
    (r"\baura\.concurrent\.channelRecv(?!\w)",          r"aura.lang.std.Channel.channelRecv"),
    (r"\baura\.concurrent\.channelTryRecv(?!\w)",       r"aura.lang.std.Channel.channelTryRecv"),
    (r"\baura\.concurrent\.select(?!\w)",               r"aura.lang.std.Channel.select"),
    (r"\baura\.concurrent\.selectTimeout(?!\w)",        r"aura.lang.std.Channel.selectTimeout"),
    (r"\baura\.concurrent\.newTcpChannel(?!\w)",        r"aura.lang.std.Channel.newTcpChannel"),
    (r"\baura\.concurrent\.tcpChannelSend(?!\w)",       r"aura.lang.std.Channel.tcpChannelSend"),
]

# Bare namespace references (no trailing dot): aura.concurrent, aura.math, aura.io, ...
# Only rewrite when the string appears as a bare token (surrounded by non-word chars or EOF).
BARE_MAP = [
    (r"\baura\.ascii\b(?!\.)",     r"aura.lang.std.Ascii"),
    (r"\baura\.assert\b(?!\.)",    r"aura.lang.std.Assert"),
    (r"\baura\.builtin\b(?!\.)",   r"aura.lang.std.Builtin"),
    (r"\baura\.collections\b(?!\.)", r"aura.lang.std.Collections"),
    (r"\baura\.console\b(?!\.)",   r"aura.lang.std.Console"),
    (r"\baura\.encoding\b(?!\.)",  r"aura.lang.std.Encoding"),
    (r"\baura\.env\b(?!\.)",       r"aura.lang.std.Env"),
    (r"\baura\.fs\b(?!\.)",        r"aura.lang.std.FileSystem"),
    (r"\baura\.io\b(?!\.)",        r"aura.lang.std.IO"),
    (r"\baura\.iter\b(?!\.)",      r"aura.lang.std.Iter"),
    (r"\baura\.json\b(?!\.)",      r"aura.lang.std.Json"),
    (r"\baura\.math\b(?!\.)",      r"aura.lang.std.Math"),
    (r"\baura\.net\b(?!\.)",       r"aura.lang.std.Network"),
    (r"\baura\.path\b(?!\.)",      r"aura.lang.std.Path"),
    (r"\baura\.process\b(?!\.)",   r"aura.lang.std.Process"),
    (r"\baura\.random\b(?!\.)",    r"aura.lang.std.Random"),
    (r"\baura\.string\b(?!\.)",    r"aura.lang.std.String"),
    (r"\baura\.test\b(?!\.)",      r"aura.lang.std.Test"),
    (r"\baura\.time\b(?!\.)",      r"aura.lang.std.Time"),
    (r"\baura\.concurrent\b(?!\.)", r"aura.lang.std.{Coroutine,Actor,Channel}"),
]


def in_scope(rel: Path) -> bool:
    """Only process markdown files under README, docs/, book/, skills/, ide-extension/."""
    parts = rel.parts
    if rel.suffix.lower() not in (".md", ".markdown"):
        return False
    if parts[0] in ("README.md", "README.zh-CN.md", "mdbook.toml"):
        return True
    if len(parts) >= 2 and parts[0] in ("docs", "book", "skills", "ide-extension", "examples"):
        return True
    return False


def migrate(text: str) -> tuple[str, int]:
    changes = 0
    for pat, repl in CONCURRENT_MAP:
        text, n = re.subn(pat, repl, text)
        changes += n
    for pat, repl in PREFIX_MAP:
        text, n = re.subn(pat, repl, text)
        changes += n
    for pat, repl in BARE_MAP:
        text, n = re.subn(pat, repl, text)
        changes += n
    return text, changes


def main():
    total = 0
    modified = []
    for p in ROOT.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(ROOT)
        if not in_scope(rel):
            continue
        try:
            orig = p.read_text(encoding="utf-8", errors="strict")
        except UnicodeDecodeError:
            continue
        new, n = migrate(orig)
        if new != orig:
            p.write_text(new, encoding="utf-8")
            print(f"{rel}: {n} replacements")
            total += n
            modified.append(rel)
    print(f"\nModified {len(modified)} files, {total} total replacements")


if __name__ == "__main__":
    main()
