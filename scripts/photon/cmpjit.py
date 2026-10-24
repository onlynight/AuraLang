#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""JIT vs VM 输出保真度比对（字节级）。

用法: python build/cmpjit.py <src.aura> <auc>

步骤：
  1. 种子 VM：`aura.exe run <src>`，捕获原始字节
  2. JIT 宿主：槽交换（备份 build/vm/current.auc → 覆盖 → 跑 → 恢复），捕获原始字节
  3. 两侧都剥掉宿主调试行（[LAF]/[A78]/[62]/[VMLOADOK]/[VMDONE]/[VEHI]）后逐行比对
"""
import os
import shutil
import subprocess
import sys

ROOT = r"D:\Code\AuraLang"
HOST = os.path.join(ROOT, "build", "hat-vmhost", "current.exe")
SEED = os.path.join(ROOT, "rust", "target", "release", "aura.exe")
SLOT = os.path.join(ROOT, "build", "vm", "current.auc")

DEBUG_PREFIX = (b"[LAF]", b"[A78]", b"[62]", b"[VMLOADOK]", b"[VMDONE]", b"[VEHI]", b"VEHI")
# VM（种子）侧的编译/装载横幅不是被测程序输出
VM_BANNER = (b"[ok]", b"[vm]", b"[warn]", b"semantic warning")


def strip_debug(raw: bytes) -> list:
    out = []
    for line in raw.replace(b"\r\n", b"\n").split(b"\n"):
        s = line.lstrip()
        if not s:
            continue
        if any(s.startswith(p) for p in DEBUG_PREFIX):
            continue
        out.append(line.rstrip())
    return out


def main():
    if len(sys.argv) < 3:
        print("usage: cmpjit.py <src.aura> <auc>")
        return 2
    src, auc = sys.argv[1], sys.argv[2]

    vm = subprocess.run([SEED, "run", src], cwd=ROOT,
                        capture_output=True, timeout=180)

    bak = SLOT + ".cmpbak"
    if os.path.exists(SLOT):
        shutil.copy2(SLOT, bak)
    shutil.copy2(auc, SLOT)
    try:
        jit = subprocess.run([HOST], cwd=ROOT, capture_output=True, timeout=180)
    finally:
        if os.path.exists(bak):
            shutil.copy2(bak, SLOT)
            os.remove(bak)

    vml = strip_debug(vm.stdout)
    vml = [l for l in vml if not any(l.lstrip().startswith(p) for p in VM_BANNER)]
    jtl = strip_debug(jit.stdout)
    print("VM  exit=%d  lines=%d" % (vm.returncode, len(vml)))
    print("JIT exit=%d  lines=%d" % (jit.returncode, len(jtl)))
    n = min(len(vml), len(jtl))
    bad = 0
    for i in range(n):
        if vml[i] != jtl[i]:
            bad += 1
            if bad <= 8:
                print("DIFF line %d:" % (i + 1))
                print("   VM : %r" % vml[i][:120])
                print("   JIT: %r" % jtl[i][:120])
    if len(vml) != len(jtl):
        print("LEN-DIFF: VM %d vs JIT %d" % (len(vml), len(jtl)))
        for i in range(n, min(len(vml), n + 4)):
            print("   VM-extra: %r" % vml[i][:120])
        for i in range(n, min(len(jtl), n + 4)):
            print("   JIT-extra: %r" % jtl[i][:120])
    print("RESULT: %s  (%d/%d lines equal)" % ("SAME" if bad == 0 and len(vml) == len(jtl) else "DIFF", n - bad, n))
    return 0


if __name__ == "__main__":
    sys.exit(main())
