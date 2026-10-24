#!/usr/bin/env python3
"""hat-langtest.py — Photon HAT 独立链路跑 tests/language-test。

判定口径（用户 2026-10-16 指定）：
    用 build/hat-native/PhotonHatCompile.exe 把 .aura 编译成 exe，
    运行 exe，**结果通过** = PASS。

对每个用例：
  1. 驱动编译：PhotonHatCompile.exe（读 AURA_HAT_AURA 环境变量）
     stdout 只含 ===COFF-MAIN=== / ===COFF-RUNTIME=== / ===LINK=== / ===RESULT=== 协议行。
  2. 从 ===LINK=== 行取 lld-link 完整命令行，原样执行 → exe。
  3. 运行 exe，超时杀掉进程树。
  4. 附加参照：`aura run <src>` 的 VM 输出（不作判据，仅打印差异帮助定位）。

PASS 条件：exe 生成 + 运行退出码 0 + 未超时。
输出 JSON 报告，供后续 triage。

用法:
  python3 scripts/photon/hat-langtest.py [--only 01-lexer 04] [--out build/lt-report]
  python3 scripts/photon/hat-langtest.py --list
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time

PY = r"D:\Espressif\tools\python\python.exe"  # 本机 python3 shim 是 Windows Store 存根
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DRIVER = os.path.join(ROOT, "build", "hat-native", "PhotonHatCompile.exe")
AURA = os.path.join(ROOT, "rust", "target", "release", "aura.exe")
SRC_DIR = os.path.join(ROOT, "tests", "language-test")

# AOT 判定口径排除清单：这些用例由设计不纳入 photon AOT 通过率统计。
# 用户 2026-10-16 明确指定：16-script-mode「不需要测试 aot」。
# 该用例验证的是「顶层可执行语句无需 main」的合成 main 路径
# （见文件头：Rust 的 synthesize_main_if_missing / Aura 的 synthesizeMainInKids），
# 走 VM 或 seed Rust AOT 都能通过；photon HAT 链路对该形态尚未验证。
DEFAULT_SKIP = ["16-script-mode"]

# ── 机器保护（2026-10-16 用户两次因编译进程内存峰值卡死）──────────
# 每个驱动编译的前端峰值 ~7GB（stdlib 闭包）。三条硬性防线：
#   1. 驱动内存护栏强制 8192MB（覆盖环境变量，任何路径都关不掉）；
#   2. 超时/异常时 taskkill /T 整树收割（防 lld-link 孤儿）；
#   3. 单飞锁 + 物理内存水位门（<6GB 空闲则等待；已有驱动实例在跑则拒绝）。
MEM_LIMIT_MB = "8192"
MIN_FREE_PHYS_GB = 6.0
DRIVER_IMAGE = "PhotonHatCompile.exe"

HEX_RE = re.compile(r"^[0-9a-fA-F]{40,}$")


def find_exe(name, cwd):
    for p in (os.path.join(cwd, name), name):
        if os.path.exists(p):
            return os.path.abspath(p)
    return name


def avail_phys_gb():
    """GlobalMemoryStatusEx：物理内存可用量（GB）。"""
    import ctypes

    class MEMORYSTATUSEX(ctypes.Structure):
        _fields_ = [
            ("dwLength", ctypes.c_ulong), ("dwMemoryLoad", ctypes.c_ulong),
            ("ullTotalPhys", ctypes.c_ulonglong), ("ullAvailPhys", ctypes.c_ulonglong),
            ("ullTotalPageFile", ctypes.c_ulonglong), ("ullAvailPageFile", ctypes.c_ulonglong),
            ("ullTotalVirtual", ctypes.c_ulonglong), ("ullAvailVirtual", ctypes.c_ulonglong),
            ("ullAvailExtendedVirtual", ctypes.c_ulonglong),
        ]
    st = MEMORYSTATUSEX()
    st.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
    if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(st)):
        return st.ullAvailPhys / (1024 ** 3)
    return 999.0  # 查询失败不阻塞


def driver_instances_running():
    """正在运行的驱动镜像数（tasklist 查询；失败视为 0）。"""
    try:
        raw = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq %s" % DRIVER_IMAGE, "/FO", "CSV", "/NH"],
            capture_output=True, timeout=15, creationflags=0x08000000,
        ).stdout
        out = raw.decode("utf-8", "replace")
        return sum(1 for line in out.splitlines() if DRIVER_IMAGE.lower() in line.lower())
    except Exception:
        return 0


def wait_for_headroom(max_wait_secs=300):
    """等待物理内存水位 + 单飞（无并发驱动实例）。返回 None 或拒绝原因。"""
    deadline = time.time() + max_wait_secs
    while time.time() < deadline:
        n = driver_instances_running()
        if n > 0:
            print("    [guard] 检测到 %d 个运行中的 %s，等待…" % (n, DRIVER_IMAGE), flush=True)
        elif avail_phys_gb() < MIN_FREE_PHYS_GB:
            print("    [guard] 物理内存不足 %.0fGB（剩 %.1fGB），等待…" % (
                MIN_FREE_PHYS_GB, avail_phys_gb()), flush=True)
        else:
            return None
        time.sleep(5)
    return "headroom-wait-timeout"


def kill_tree(pid):
    """taskkill /T /F 整树收割（subprocess.run 的 timeout 只杀直接子进程）。"""
    try:
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(pid)],
                       capture_output=True, timeout=15, creationflags=0x08000000)
    except Exception:
        pass


def run(exe, args, cwd, timeout, env=None):
    """返回 (exit_code, stdout, stderr, timed_out)。超时整树收割。"""
    e = {k: v for k, v in os.environ.items() if k not in ("AURA_HAT_AURA", "AURA_HAT_SRC", "AURA_HAT_OUT", "AURA_HAT_MODULE")}
    if env:
        e.update(env)
    if os.path.basename(exe).lower() == DRIVER_IMAGE.lower():
        # 防线 1：护栏强制覆盖，ambient env 关不掉
        e["AURA_MEM_LIMIT_MB"] = MEM_LIMIT_MB
    p = subprocess.Popen(
        [exe] + args, cwd=cwd, env=e,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        creationflags=0x08000000,  # CREATE_NO_WINDOW
    )
    try:
        out, err = p.communicate(timeout=timeout)
        return p.returncode, out.decode("utf-8", "replace"), err.decode("utf-8", "replace"), False
    except subprocess.TimeoutExpired:
        kill_tree(p.pid)
        try:
            out, err = p.communicate(timeout=10)
        except Exception:
            out, err = b"", b""
        outs = out if isinstance(out, str) else out.decode("utf-8", "replace")
        errs = err if isinstance(err, str) else err.decode("utf-8", "replace")
        return -999, outs, errs, True
    except FileNotFoundError:
        return -1, "", "spawn-fail: %s" % exe, False


def parse_driver(text):
    """解析驱动协议行。hex 与标记同在一行或跨行均可。"""
    st = {"main": None, "rt": None, "link": None, "result": None, "err": None}
    mode = None
    for raw in text.splitlines():
        t = raw.strip()
        if not t:
            continue
        if t.startswith("===COFF-MAIN==="):
            mode = "main"
            rest = t[len("===COFF-MAIN==="):].strip()
            if HEX_RE.match(rest):
                st["main"] = rest
            continue
        if t.startswith("===COFF-RUNTIME==="):
            mode = "rt"
            rest = t[len("===COFF-RUNTIME==="):].strip()
            if HEX_RE.match(rest):
                st["rt"] = rest
            continue
        if t.startswith("===LINK==="):
            st["link"] = t[len("===LINK==="):].strip()
            mode = "link"
            continue
        if t.startswith("===RESULT==="):
            st["result"] = t[len("===RESULT==="):].strip()
            mode = "result"
            continue
        if t.startswith("===ERR==="):
            st["err"] = t[len("===ERR==="):].strip()
            continue
        if HEX_RE.match(t):
            if mode == "main" and st["main"] is None:
                st["main"] = t
            elif mode == "rt" and st["rt"] is None:
                st["rt"] = t
    return st


def compile_one(src, stem, outdir, timeout):
    """驱动编译 + lld-link。返回 (exe_path, reasons, info)。"""
    reasons = []
    info = {}
    # 防线 2/3：水位 + 单飞，等待而非失败（最多 5 分钟）
    why = wait_for_headroom()
    if why:
        reasons.append(why)
        return None, reasons, info

    # work 目录按驱动二进制版本命名：驱动重build后缓存 COFF 自动隔离
    #（rmtree 在 Windows 上会因文件占用静默失败，不能依赖删除失效旧缓存
    # —— 2026-10-16 实踩：旧 COFF 被回放、新修复看似无效）。旧目录顺手清。
    import glob as _glob
    drv_key = "%d_%d" % (int(os.path.getmtime(DRIVER)), os.path.getsize(DRIVER))
    out = os.path.join(outdir, "work_" + drv_key)
    for old in _glob.glob(os.path.join(outdir, "work_*")):
        if old != out:
            shutil.rmtree(old, ignore_errors=True)
    os.makedirs(out, exist_ok=True)

    t0 = time.time()
    code, dout, derr, tmo = run(
        DRIVER, [], ROOT, timeout,
        env={"AURA_HAT_AURA": src, "AURA_HAT_SRC": os.path.join(out, stem + ".hat"),
             "AURA_HAT_OUT": out, "AURA_HAT_MODULE": stem, "AURA_BUILD_CACHE": "1"},
    )
    info["front_secs"] = round(time.time() - t0, 1)
    if tmo:
        reasons.append("front-timeout")
    st = parse_driver(dout)
    info["result"] = st["result"]
    info["err"] = st["err"]

    exe = os.path.join(out, stem + ".exe")
    link_cmd = st["link"] or ""
    # 驱动自己已经执行过 lld-link（===LINK=== 行）。但它的 outDir 受
    # AURA_HAT_OUT 影响，产物已落在 <work>/，故这里只需核对产物并在失败时补跑。
    if not os.path.exists(exe) and os.path.exists(os.path.join(out, stem + ".obj")):
        import shlex
        toks = shlex.split(link_cmd)
        if toks:
            toks[0] = find_exe(toks[0], ROOT)
            rc, lo, le, lt = run(toks[0], toks[1:], ROOT, 120)
            info["rel_link_exit"] = rc
            info["rel_link_err"] = (lo + le).strip().splitlines()[-6:]
        else:
            info["rel_link_exit"] = -1
            info["rel_link_err"] = ["no-link-cmd"]
    info["link_cmd"] = link_cmd
    # 补链只在 exe 缺失时做：驱动自己已链接（或缓存命中直接复用）。
    # 无条件重跑 lld-link 反而会把好 exe 覆盖坏（缓存回放路径 shlex 吃
    # 反斜杠路径 → link-fail，2026-10-16 实踩）。
    if not os.path.exists(exe):
        if not os.path.exists(os.path.join(out, stem + ".obj")):
            reasons.append("no-obj")
        elif not link_cmd:
            reasons.append("no-link-line")
        else:
            import shlex
            # posix=False：Windows 路径的反斜杠不是转义符（posix 模式会把
            # D:\Code\AuraLang 吃成 D:CodeAuraLang）。
            toks = shlex.split(link_cmd, posix=False)
            toks = [t.strip('"') for t in toks]
            if not toks:
                reasons.append("link-cmd-empty")
            else:
                lld = find_exe(toks[0], ROOT)
                args = toks[1:]
                t0 = time.time()
                lcode, lout, lerr, ltmo = run(lld, args, ROOT, 120)
                info["link_secs"] = round(time.time() - t0, 1)
                info["link_exit"] = lcode
                if ltmo:
                    reasons.append("link-timeout")
                elif lcode != 0 or not os.path.exists(exe):
                    reasons.append("link-fail")
                    info["link_err"] = (lout + lerr).strip().splitlines()[-8:]

    if os.path.exists(exe):
        return exe, reasons, info
    return None, reasons, info


def vm_ref(src, timeout=90):
    code, out, err, tmo = run(AURA, ["run", src], ROOT, timeout)
    lines = [l for l in out.replace("\r", "").splitlines()
             if not re.match(r"^\[ok\].*compiled", l) and not l.startswith("[vm] backend=")]
    return code, "\n".join(lines).strip(), tmo


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", nargs="*", default=[])
    ap.add_argument("--skip", nargs="*", default=[])
    ap.add_argument("--timeout", type=int, default=120)
    ap.add_argument("--outdir", default=os.path.join(ROOT, "build", "lt-report"))
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--no-vm", action="store_true", help="跳过 VM 参照比对")
    ap.add_argument("--rerun", action="store_true", help="忽略缓存结果全部重跑")
    ap.add_argument("--shard", default="", help="N:P 分片（并行时用）")
    args = ap.parse_args()

    files = sorted(f for f in os.listdir(SRC_DIR) if f.endswith(".aura"))
    if args.list:
        for f in files:
            print(f)
        return 0

    if args.only:
        files = [f for f in files if any(k in f for k in args.only)]
    if args.skip:
        files = [f for f in files if not any(k in f for k in args.skip)]
    for k in DEFAULT_SKIP:
        files = [f for f in files if k not in f]

    if args.shard:
        n, p = (int(x) for x in args.shard.split(":"))
        files = [f for i, f in enumerate(files) if i % n == p]

    os.makedirs(args.outdir, exist_ok=True)

    # 单飞锁：同一时刻只允许一个 runner 在跑（两个 7GB 编译并行 = 卡死）
    import msvcrt
    lockf = open(os.path.join(args.outdir, ".compile.lock"), "a+")
    try:
        msvcrt.locking(lockf.fileno(), msvcrt.LK_NBLCK, 1)
    except OSError:
        print("拒绝启动：另一个 hat-langtest 实例持有锁（%s）" % lockf.name, flush=True)
        return 2

    report_path = os.path.join(args.outdir, "report.json")
    if not args.shard:
        report = {"results": []}
        # 断点续跑（--rerun 忽略缓存结果全部重跑）
        if os.path.exists(report_path) and not args.rerun:
            try:
                report = json.load(open(report_path, encoding="utf-8"))
            except Exception:
                report = {"results": []}
        done = {r["file"] for r in report["results"] if r.get("pass") or r.get("fail")}
    else:
        report = {"results": []}

    if args.skip or DEFAULT_SKIP:
        excluded = sorted(set(args.skip + DEFAULT_SKIP))
        print("=== Photon HAT language-test: %d files ===" % len(files), flush=True)
        print("  excluded (AOT 判定口径外): %s" % ", ".join(excluded), flush=True)
    print("driver: %s" % DRIVER, flush=True)
    npass = nfail = 0
    for f in files:
        if not args.shard and f in done:
            print("  SKIP(cached) %s" % f, flush=True)
            continue
        src = os.path.join(SRC_DIR, f)
        stem = os.path.splitext(f)[0]
        outdir = os.path.join(args.outdir, stem)
        t0 = time.time()
        exe, reasons, info = compile_one(src, stem, outdir, args.timeout)

        rcode = rerr = None
        rexout = ""
        if exe:
            rcode, rexout, rerr, rtmo = run(exe, [], os.path.dirname(exe), args.timeout)
            if rtmo:
                reasons.append("run-timeout")
                rcode = -999

        vmcode = vmout = None
        if not args.no_vm:
            vmcode, vmout, vm_tmo = vm_ref(src)

        ok = (exe is not None) and (rcode == 0) and not any(x.startswith("run") for x in reasons)
        rec = {"file": f, "pass": bool(ok), "fail": not bool(ok),
               "reasons": reasons, "info": info,
               "exit": rcode, "exe_out": rexout,
               "vm_exit": vmcode, "vm_out": vmout}
        report["results"].append(rec)
        if not args.shard:
            json.dump(report, open(report_path, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
        else:
            sp = report_path + ".s%s" % args.shard
            json.dump(report, open(sp, "w", encoding="utf-8"), ensure_ascii=False, indent=1)

        tag = "PASS" if ok else "FAIL"
        if ok:
            npass += 1
        else:
            nfail += 1
        print("  %-5s %-32s exit=%-5s %-9s %s" % (tag, f, rcode, info.get("front_secs"), ",".join(reasons) or "-"), flush=True)
        if not ok and reasons:
            d = (info.get("err") or "")[:200]
            if d:
                print("        ERR: %s" % d, flush=True)
            if rexout and vmout is not None and rexout != vmout:
                print("        VM : %s" % (vmout[:150].replace("\n", " | ")), flush=True)
                print("        EXE: %s" % (rexout[:150].replace("\n", " | ")), flush=True)
    print("TOTAL: PASS=%d FAIL=%d (%d)" % (npass, nfail, len(files)), flush=True)
    return 1 if nfail else 0


if __name__ == "__main__":
    sys.exit(main())
