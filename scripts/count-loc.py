#!/usr/bin/env python3
"""count-loc.py —— 统计 AuraLang 核心代码行数。

用法:
    python scripts/count-loc.py                 # 统计默认核心目录（aura/* 与 rust/{compiler,cli,loom}）
    python scripts/count-loc.py <dir> [...]     # 统计指定目录 / 文件
    python scripts/count-loc.py --json          # 输出 JSON
    python scripts/count-loc.py --all-ext       # 按扩展名分组（不合并为语言名）
    python scripts/count-loc.py --by-root       # 只输出按目录明细

统计口径:
    total   文件总行数
    code    代码行 = 总行数 - 空行 - 纯注释行
    blank   空行（仅含空白的行也算空行）
    comment 纯注释行（行首注释；块注释按起始/结束行判定；行尾注释不计入注释）
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple

# ---------------------------------------------------------------- 配置

# 默认统计的核心目录（相对仓库根，脚本位于 <root>/scripts/）
DEFAULT_ROOTS: Tuple[str, ...] = (
    "aura/compiler",
    "aura/core",
    "aura/runtime",
    "aura/seed",
    "aura/toolchain",
    "rust/compiler",
    "rust/cli",
    "rust/loom",
)

# 目录前缀 -> 分组名
GROUP_SPEC: Tuple[Tuple[str, str], ...] = (
    ("aura/", "aura"),
    ("rust/", "rust"),
)

# 扩展名 -> 语言名
LANG_MAP: Dict[str, str] = {
    ".aura": "Aura",
    ".rs": "Rust",
    ".ll": "LLVM IR",
    ".c": "C",
    ".h": "C",
    ".cpp": "C++",
    ".hpp": "C++",
    ".lua": "Lua",
    ".toml": "TOML",
    ".md": "Markdown",
    ".ps1": "PowerShell",
    ".sh": "Shell",
    ".snap": "Snapshot",
    ".txt": "Text",
}

# 扩展名 -> 注释语法 (行注释 token, 是否支持 /* */ 块注释, 是否支持 ';' 行注释)
COMMENT_STYLE: Dict[str, Tuple[Optional[str], bool]] = {
    ".aura": ("//", True),
    ".rs": ("//", True),
    ".c": ("//", True),
    ".h": ("//", True),
    ".cpp": ("//", True),
    ".hpp": ("//", True),
    ".ll": (";", False),
    ".lua": ("--", False),
    ".toml": ("#", False),
    ".ps1": ("#", False),
    ".sh": ("#", False),
}

# 直接跳过的二进制/非文本扩展名
SKIP_EXT: Tuple[str, ...] = (
    ".exe", ".dll", ".so", ".dylib", ".a", ".lib", ".o", ".obj", ".pdb",
    ".bin", ".png", ".jpg", ".jpeg", ".gif", ".ico", ".pdf", ".zip", ".gz",
    ".lock", ".bak",
)

# 跳过的目录名（任意层级）
SKIP_DIRS = {
    ".git", ".github", ".codebuddy", "target", "build", "dist", "out",
    "node_modules", "vendor", "__pycache__", ".cache",
}


# ---------------------------------------------------------------- 数据结构


@dataclass
class Stat:
    files: int = 0
    total: int = 0
    blank: int = 0
    comment: int = 0

    @property
    def code(self) -> int:
        return max(self.total - self.blank - self.comment, 0)

    def add(self, other: "Stat") -> None:
        self.files += other.files
        self.total += other.total
        self.blank += other.blank
        self.comment += other.comment


@dataclass
class Report:
    """rel_root -> lang -> Stat"""
    tree: Dict[str, Dict[str, Stat]] = field(
        default_factory=lambda: defaultdict(lambda: defaultdict(Stat))
    )


# ---------------------------------------------------------------- 统计逻辑


def lang_of(path: str, all_ext: bool = False) -> str:
    ext = os.path.splitext(path)[1].lower()
    if all_ext:
        return ext or "(no ext)"
    return LANG_MAP.get(ext, ext.lstrip(".").upper() or "(no ext)")


def count_text(lines: List[str], ext: str) -> Stat:
    st = Stat(files=1, total=len(lines))
    line_tok, has_block = COMMENT_STYLE.get(ext, (None, False))
    in_block = False

    for raw in lines:
        s = raw.strip()
        if not s:
            st.blank += 1
            continue

        if in_block:
            st.comment += 1
            if "*/" in s:
                in_block = False
            continue

        if line_tok and s.startswith(line_tok):
            st.comment += 1
            continue

        if has_block and s.startswith("/*"):
            st.comment += 1
            if "*/" not in s:
                in_block = True
            continue

    return st


def count_file(path: str, all_ext: bool = False) -> Stat:
    ext = os.path.splitext(path)[1].lower()
    try:
        with open(path, "r", encoding="utf-8") as fh:
            text = fh.read()
    except UnicodeDecodeError:
        with open(path, "r", encoding="latin-1") as fh:
            text = fh.read()
    except OSError:
        return Stat()
    return count_text(text.splitlines(), ext)


def scan(paths: List[str], rel_of, report: Report, all_ext: bool,
         skipped: List[str]) -> None:
    """遍历目录或文件，累加进 report。"""
    for path in paths:
        if os.path.isfile(path):
            rel = rel_of(path)
            ext = os.path.splitext(path)[1].lower()
            if ext in SKIP_EXT:
                continue
            st = count_file(path, all_ext)
            if st.files:
                report.tree[rel][lang_of(path, all_ext)].add(st)
            continue

        for dirpath, dirnames, filenames in os.walk(path):
            dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
            for name in sorted(filenames):
                ext = os.path.splitext(name)[1].lower()
                if ext in SKIP_EXT:
                    continue
                full = os.path.join(dirpath, name)
                try:
                    st = count_file(full, all_ext)
                except Exception as exc:  # pragma: no cover - 防御性
                    skipped.append(f"{full}: {exc}")
                    continue
                if st.files:
                    report.tree[rel_of(path)][lang_of(full, all_ext)].add(st)


def group_of(rel_root: str) -> str:
    norm = rel_root.replace(os.sep, "/")
    for prefix, group in GROUP_SPEC:
        if norm.startswith(prefix):
            return group
    return "other"


# ---------------------------------------------------------------- 输出


def pct(part: int, whole: int) -> str:
    return f"{part / whole * 100:.1f}%" if whole else "-"


def print_table(headers: List[str], rows: List[List[str]]) -> None:
    widths = [len(h) for h in headers]
    for row in rows:
        for i, cell in enumerate(row):
            widths[i] = max(widths[i], len(cell))
    sep = "  "
    print(sep.join(h.ljust(widths[i]) for i, h in enumerate(headers)))
    print(sep.join("-" * w for w in widths))
    for row in rows:
        print(sep.join(cell.ljust(widths[i]) for i, cell in enumerate(row)))


def summarize(stats: Dict[str, Stat]) -> Stat:
    total = Stat()
    for st in stats.values():
        total.add(st)
    return total


def group_stats(report: Report) -> Dict[str, Dict[str, Stat]]:
    groups: Dict[str, Dict[str, Stat]] = defaultdict(lambda: defaultdict(Stat))
    for root, stats in report.tree.items():
        g = groups[group_of(root)]
        for lang, st in stats.items():
            g[lang].add(st)
    return groups


def render(report: Report, by_root_only: bool) -> None:
    if not by_root_only:
        rows: List[List[str]] = []
        for root in report.tree:
            for lang, st in sorted(report.tree[root].items(),
                                   key=lambda kv: (-kv[1].total, kv[0])):
                rows.append([
                    root, lang, str(st.files), str(st.total),
                    str(st.code), str(st.blank), str(st.comment),
                ])
        if rows:
            print("== 按目录 x 语言 ==")
            print_table(
                ["目录", "语言", "文件", "总行", "代码行", "空行", "注释行"],
                rows,
            )
            print()

    rows = []
    for root, stats in report.tree.items():
        st = summarize(stats)
        rows.append([
            root, group_of(root), str(st.files), str(st.total),
            str(st.code), str(st.blank), str(st.comment),
        ])
    print("== 按目录 ==")
    print_table(["目录", "分组", "文件", "总行", "代码行", "空行", "注释行"], rows)
    print()

    groups = group_stats(report)
    grand = Stat()
    rows = []
    for gname in sorted(groups):
        gstats = groups[gname]
        st = summarize(gstats)
        grand.add(st)
        langs = ", ".join(
            f"{lang} {s.total}" for lang, s in
            sorted(gstats.items(), key=lambda kv: -kv[1].total)
        )
        rows.append([
            gname, str(st.files), str(st.total), str(st.code),
            str(st.blank), str(st.comment), langs,
        ])
    rows.append([
        "TOTAL", str(grand.files), str(grand.total), str(grand.code),
        str(grand.blank), str(grand.comment), "",
    ])
    print("== 按分组汇总 ==")
    print_table(
        ["分组", "文件", "总行", "代码行", "空行", "注释行", "语言构成（总行）"],
        rows,
    )

    core_rows = []
    for gname in sorted(groups):
        for lang, st in sorted(groups[gname].items(), key=lambda kv: -kv[1].total):
            if lang in ("Aura", "Rust"):
                core_rows.append([
                    gname, lang, str(st.files), str(st.total),
                    str(st.code), pct(st.code, grand.code),
                ])
    if core_rows:
        print()
        print("== 核心语言代码行 ==")
        print_table(["分组", "语言", "文件", "总行", "代码行", "占全部代码行"], core_rows)


def to_json(report: Report) -> str:
    out: Dict[str, object] = {"roots": {}, "groups": {}}
    roots: Dict[str, Dict[str, dict]] = {}
    for root, stats in report.tree.items():
        roots[root] = {
            lang: {
                "files": st.files, "total": st.total,
                "code": st.code, "blank": st.blank, "comment": st.comment,
            }
            for lang, st in sorted(stats.items())
        }
    groups: Dict[str, dict] = {}
    for gname, gstats in sorted(group_stats(report).items()):
        st = summarize(gstats)
        groups[gname] = {
            "files": st.files, "total": st.total,
            "code": st.code, "blank": st.blank, "comment": st.comment,
            "langs": {
                lang: {"files": s.files, "total": s.total, "code": s.code}
                for lang, s in sorted(gstats.items(), key=lambda kv: -kv[1].total)
            },
        }
    out["roots"] = roots
    out["groups"] = groups
    return json.dumps(out, ensure_ascii=False, indent=2)


# ---------------------------------------------------------------- main


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(
        description="统计 AuraLang 核心代码行数（默认 aura/* 与 rust/{compiler,cli,loom}）"
    )
    parser.add_argument("paths", nargs="*", help="要统计的目录或文件（默认使用内置核心目录）")
    parser.add_argument("--root", default=None, help="仓库根目录（默认脚本上一级目录）")
    parser.add_argument("--json", action="store_true", help="输出 JSON")
    parser.add_argument("--by-root", action="store_true", help="只输出按目录明细")
    parser.add_argument("--all-ext", action="store_true", help="按扩展名分组，不合并为语言名")
    args = parser.parse_args(argv)

    repo_root = os.path.abspath(
        args.root or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    )

    if args.paths:
        targets = [os.path.abspath(p) for p in args.paths]

        def rel_of(p: str) -> str:
            try:
                r = os.path.relpath(p, repo_root)
            except ValueError:
                return p.replace(os.sep, "/")
            return p.replace(os.sep, "/") if r.startswith("..") else r.replace(os.sep, "/")
    else:
        targets = [os.path.join(repo_root, r) for r in DEFAULT_ROOTS]

        def rel_of(p: str) -> str:
            return os.path.relpath(p, repo_root).replace(os.sep, "/")

    report = Report()
    skipped: List[str] = []
    missing: List[str] = []
    for target in targets:
        if not os.path.exists(target):
            missing.append(target)
            continue
        scan([target], rel_of, report, args.all_ext, skipped)

    for m in missing:
        print(f"[warn] 路径不存在，已跳过: {m}", file=sys.stderr)

    if not report.tree:
        print("没有可统计的文件", file=sys.stderr)
        return 1

    if args.json:
        print(to_json(report))
    else:
        render(report, by_root_only=args.by_root)

    if skipped:
        print(f"[warn] {len(skipped)} 个文件读取失败", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
