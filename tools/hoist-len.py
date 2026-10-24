#!/usr/bin/env python3
"""把 `<expr>.length` 从 `while` 循环条件里提到循环外（Aura 专项重构工具）。

为什么需要：Aura 的 String 是 NUL 结尾 C 串，`.length` 在 AOT 下就是 `strlen()`。
写成 `while (i < s.length)` 时每轮都要重扫整条串 —— 对长度 n 的串是 O(n^2)。
自举实测 `fnvFile`（逐字符哈希源文件）因此烧掉 55s，`unionCsvContains`
（联合查找名字表）把 ssaBuild 抬到 100s+。

用法：
    python tools/hoist-len.py <files...>            # 只看会改什么（dry run）
    python tools/hoist-len.py --apply <files...>    # 真改
    python tools/hoist-len.py --apply $(find aura/compiler aura/photon -name '*.aura')

安全规则（任一不满足就**跳过整个循环**，绝不半改）：
  * 只认 `x` / `this.x` 这类简单标识符链（不含调用/下标）；
  * 循环体内不得声明同名局部量；
  * 循环体内不得对该表达式赋值（含 `x = x + ...`）；
  * 生成的局部量名不得与文件里已有标识符冲撞（冲突时加数字后缀）；
  * 外层已为同一表达式 hoist 过时，内层直接复用外层名，不重复声明（避免同名遮蔽）。

注意：`while (i <= s.length)` 这类也算命中（正则只匹配 `.length` 侧）。
回滚：改动前可 `git checkout -- <file>`，或用 `build/lenbak/before.tar` 快照。
"""

import re, sys, io, os

# Hoist `<expr>.length` out of `while` conditions (Aura `.length` == strlen,
# so a length in the loop condition re-scans the whole string every iteration).
#
# Safety rules (any violation => skip that loop entirely):
#   * only simple identifier chains (x, this.x) -- no calls / indexing
#   * expr must be declared before the loop  (no local `val x` inside the body)
#   * expr must NOT be reassigned anywhere in the loop body
#   * generated local name must not already exist in the body
#   * the loop body must be brace-balanced

IDENT = r"(?:this\.)?[A-Za-z_][A-Za-z0-9_]*"
COND_RE = re.compile(r"(" + IDENT + r")\.length\b")


def match_brace(lines, li, ci):
    """From (line, col) of an opening '{', return (end_line, end_col)."""
    depth = 0
    l = li
    c = ci
    while l < len(lines):
        line = lines[l]
        while c < len(line):
            ch = line[c]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return (l, c)
            c += 1
        l += 1
        c = 0
    return None


def process(path, apply=False):
    with io.open(path, encoding="utf-8") as f:
        text = f.read()
    lines = text.split("\n")
    used = set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", text))
    changed = []
    ls = list(lines)  # mutable copy, we edit bottom-up per file pass
    # collect candidates first (top-down indices), apply bottom-up to keep indices valid
    inserts = []   # (line_idx, [vals])
    replaces = []  # (line_idx, (expr, name))
    # active enclosing-loop scopes: list of (end_line, {expr: name}); outer first
    scopes = []

    for li, line in enumerate(ls):
        while scopes and scopes[-1][0] < li:
            scopes.pop()
        m = re.match(r"^(\s*)while\s*\((.*)\)\s*\{\s*$", line)
        if not m:
            continue
        indent, cond = m.group(1), m.group(2)
        exprs = []
        for mm in COND_RE.finditer(cond):
            e = mm.group(1)
            if e not in exprs:
                exprs.append(e)
        if not exprs:
            continue

        # body extent: find the '{' at end of this line
        open_col = line.rindex("{")
        end = match_brace(ls, li, open_col)
        if end is None:
            continue
        el, ec = end

        body_lines = []
        if el == li:
            body_lines.append(ls[li][open_col + 1:ec])
        else:
            body_lines.append(ls[li][open_col + 1:])
            for k in range(li + 1, el):
                body_lines.append(ls[k])
            body_lines.append(ls[el][:ec])
        body = "\n".join(body_lines)

        ok = True
        plan = []
        outer = {}
        for _, d in scopes:
            outer.update(d)
        for e in exprs:
            if e in outer:
                plan.append((e, outer[e], False))
                continue
            base = e.split(".")[-1]
            stem = base + "Len"
            name = stem
            n = 2
            while name in used:
                name = stem + str(n)
                n += 1
            # declared inside body (would shadow / be undefined before loop)?
            if re.search(r"\b(val|var)\s+" + re.escape(base) + r"\b", body):
                ok = False
                break
            # reassigned inside body?
            if re.search(r"(?<![=!<>])\b" + re.escape(e) + r"\s*=(?!=)", body):
                ok = False
                break
            if re.search(r"\bfor\s*\(\s*" + re.escape(base) + r"\s+in\b", body):
                ok = False
                break
            # name collision?
            if re.search(r"\b" + name + r"\b", body) or re.search(r"\b" + name + r"\b", cond):
                ok = False
                break
            used.add(name)
            # whole-file: expr reassigned before this loop in the same function? too hard;
            # rely on "not in body" + no local declaration.
            plan.append((e, name, True))
        if not ok or not plan:
            continue

        fresh = {e: nm for (e, nm, isnew) in plan if isnew}
        changed.append((li + 1, exprs, [p[1] for p in plan]))
        if apply:
            for e, name, isnew in plan:
                if isnew:
                    inserts.append((li, indent + "val " + name + ": Int = " + e + ".length"))
            # innermost hoists must win on lines they own: apply enclosing scopes first,
            # so emit outer->inner order per line (plan order already outer->inner here)
            for k in range(li, el + 1):
                for e, name, isnew in plan:
                    replaces.append((k, e, name))
            if fresh:
                scopes.append((el, fresh))
    if apply and changed:
        # do replacements line by line
        for li, line in enumerate(ls):
            newline = line
            for (rj, e, nm) in replaces:
                if rj != li:
                    continue
                # replace only `<expr>.length` occurrences
                newline = re.sub(re.escape(e) + r"\.length\b", nm, newline)
            ls[li] = newline
        # apply inserts bottom-up
        by_line = {}
        for li, txt in inserts:
            by_line.setdefault(li, []).append(txt)
        for li in sorted(by_line.keys(), reverse=True):
            for txt in reversed(by_line[li]):
                ls.insert(li, txt)
        out = "\n".join(ls)
        with io.open(path, "w", encoding="utf-8") as f:
            f.write(out)
    return changed


if __name__ == "__main__":
    apply = "--apply" in sys.argv
    files = [a for a in sys.argv[1:] if not a.startswith("--")]
    total = 0
    for p in files:
        ch = process(p, apply)
        if ch:
            print("== " + os.path.basename(p) + (" (APPLIED)" if apply else " (dry)"))
            for li, exprs, names in ch:
                print("   line %-6d %s" % (li, ", ".join(exprs)))
            total += len(ch)
    print("loops touched: %d" % total)

