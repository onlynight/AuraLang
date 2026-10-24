#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""JIT(宿主) vs VM(种子) 逐行保真度批量比对。

用法: python scripts/photon/fidelity.py [tag]
  对 tests/language-test/*.aura 逐个：
    1) aura.exe build → <work>/<stem>.auc
    2) aura.exe run   → VM 原始输出
    3) 宿主槽交换跑 .auc → JIT 原始输出
    4) 两侧剥掉调试行/横幅后逐行比对，统计 matched/total 与首个差异
"""
import os
import subprocess
import sys

ROOT = r"D:\Code\AuraLang"
HOST = os.path.join(ROOT, "build", "hat-vmhost", "current.exe")
SEED = os.path.join(ROOT, "rust", "target", "release", "aura.exe")
SLOT = os.path.join(ROOT, "build", "vm", "current.auc")
TESTS = os.path.join(ROOT, "tests", "language-test")

DEBUG_PREFIX = (b"[LAF]", b"[A78]", b"[62]", b"[VMLOADOK]", b"[VMDONE]", b"[VEHI]",
                b"VEHI", b"[CM]", b"[TS]", b"[RT]", b"[RTV]", b"[RT2]", b"[CF]")
VM_BANNER = (b"[ok]", b"[vm]", b"[warn]", b"semantic warning", b"aura:")


def strip_lines(raw: bytes, is_vm: bool):
    out = []
    for line in raw.replace(b"\r\n", b"\n").split(b"\n"):
        s = line.lstrip()
        if not s:
            continue
        if any(s.startswith(p) for p in DEBUG_PREFIX):
            continue
        if is_vm and any(s.startswith(p) for p in VM_BANNER):
            continue
        out.append(line.rstrip())
    return out


def run(exe, args, timeout=120):
    try:
        p = subprocess.run([exe] + args, cwd=ROOT, capture_output=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return -1, b"", b"TIMEOUT"


def main():
    tag = sys.argv[1] if len(sys.argv) > 1 else "fid"
    only = sys.argv[2] if len(sys.argv) > 2 else ""
    work = os.path.join(ROOT, "build", "fidelity", tag)
    os.makedirs(work, exist_ok=True)

    slot_bak = os.path.join(work, "_slot.bak")
    if os.path.exists(SLOT):
        shutil_copy(SLOT, slot_bak)

    names = sorted(n for n in os.listdir(TESTS) if n.endswith(".aura"))
    if only:
        keys = only.split(",")
        names = [n for n in names if any(k in n for k in keys)]
    total_m = 0
    total_n = 0
    rows = []
    for n in names:
        stem = n[:-5]
        src = os.path.join(TESTS, n)
        auc = os.path.join(work, stem + ".auc")
        rcB, _, _ = run(SEED, ["build", src, "--output", auc])
        rcV, vout, _ = run(SEED, ["run", src])
        if not os.path.exists(auc) or rcB != 0:
            rows.append((stem, 0, 0, "auc-build-fail", ""))
            continue
        shutil_copy(auc, SLOT)
        rcJ, jout, _ = run(HOST, [])
        shutil_copy(slot_bak, SLOT) if os.path.exists(slot_bak) else None
        vl = strip_lines(vout, True)
        jl = strip_lines(jout, False)
        n_lines = max(len(vl), len(jl))
        m = 0
        first = ""
        alld = []
        for i in range(n_lines):
            a = vl[i] if i < len(vl) else b"<missing>"
            b = jl[i] if i < len(jl) else b"<missing>"
            if a == b:
                m += 1
            else:
                d = "L%d VM=[%s] JIT=[%s]" % (
                    i, a.decode("utf-8", "replace")[:70], b.decode("utf-8", "replace")[:70])
                alld.append(d)
                if not first:
                    first = d
        if os.environ.get("FID_V") == "1" and len(alld) > 1:
            print("      " + "\n      ".join(alld[:25]))
        total_m += m
        total_n += n_lines
        status = "SAME" if (m == n_lines and len(vl) == len(jl)) else "DIFF"
        rows.append((stem, m, n_lines, status, first))
        print("[%-28s] %s %d/%d  %s" % (stem, status, m, n_lines, first[:100]))
    print("")
    print("=== 合计 %d/%d (%.1f%%) ===" % (
        total_m, total_n, 100.0 * total_m / total_n if total_n else 0.0))
    same = sum(1 for r in rows if r[3] == "SAME")
    print("=== 完全一致用例 %d/%d ===" % (same, len(rows)))
    if os.path.exists(slot_bak):
        shutil_copy(slot_bak, SLOT)


def shutil_copy(a, b):
    with open(a, "rb") as f:
        data = f.read()
    with open(b, "wb") as f:
        f.write(data)


if __name__ == "__main__":
    main()
