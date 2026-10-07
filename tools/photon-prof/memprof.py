#!/usr/bin/env python3
"""进程内存采样器 — 启动目标命令并高频采样其内存，退出后输出峰值统计。

用法:
    python3 memprof.py [--runs N] [--interval MS] -- <command> [args...]

输出（每次运行一行 JSON + 汇总）：
    label / exit / wall_ms / peak_wset / peak_private / peak_vms / last_wset

口径说明（Windows / psutil）：
    peak_wset    峰值工作集（物理驻留峰值，含共享页）—— 「任务管理器」口径
    peak_private 峰值私有提交（PagefileUsage，不含共享）—— 「私有字节」口径
    peak_vms     峰值虚拟地址空间（含 mmap/预留，不代表实际占用）

建立于 2026-10-07（Rust VM vs Photon Aura VM 内存对比）。
"""
import argparse
import json
import subprocess
import sys
import time

import psutil


def sample_once(cmd, interval_s=0.005, quiet=True):
    devnull = subprocess.DEVNULL
    p = subprocess.Popen(
        cmd,
        stdout=devnull if quiet else None,
        stderr=devnull if quiet else None,
    )
    try:
        proc = psutil.Process(p.pid)
    except psutil.NoSuchProcess:
        p.wait()
        return None
    t0 = time.perf_counter()
    peak_wset = peak_private = peak_vms = 0
    last_wset = last_private = last_vms = 0
    samples = 0
    while p.poll() is None:
        try:
            mi = proc.memory_info()
        except psutil.NoSuchProcess:
            break
        last_wset = mi.rss
        last_private = getattr(mi, "private", 0) or 0
        last_vms = mi.vms
        if last_wset > peak_wset:
            peak_wset = last_wset
        if last_private > peak_private:
            peak_private = last_private
        if last_vms > peak_vms:
            peak_vms = last_vms
        samples += 1
        time.sleep(interval_s)
    p.wait()
    wall_ms = int((time.perf_counter() - t0) * 1000)
    return {
        "exit": p.returncode,
        "wall_ms": wall_ms,
        "peak_wset": peak_wset,
        "peak_private": peak_private,
        "peak_vms": peak_vms,
        "last_wset": last_wset,
        "last_private": last_private,
        "last_vms": last_vms,
        "samples": samples,
    }


def mb(x):
    return round(x / (1024 * 1024), 1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--interval", type=float, default=5.0, help="采样间隔 ms")
    ap.add_argument("--label", default="")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    cmd = args.cmd
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        print("用法: memprof.py [--runs N] -- <command...>", file=sys.stderr)
        return 2

    runs = []
    for _ in range(args.runs):
        r = sample_once(cmd, interval_s=args.interval / 1000.0)
        if r:
            runs.append(r)
    if not runs:
        print("采样失败（进程未启动）", file=sys.stderr)
        return 1

    def best(key):
        return max(r[key] for r in runs)

    # 取各口径的峰值（多次运行取最大峰值，内存测量比计时稳定，取峰即上界）
    summary = {
        "label": args.label or " ".join(cmd),
        "cmd": cmd,
        "runs": len(runs),
        "exit_ok": all(r["exit"] == 0 for r in runs),
        "wall_ms": min(r["wall_ms"] for r in runs),
        "peak_wset_mb": mb(best("peak_wset")),
        "peak_private_mb": mb(best("peak_private")),
        "peak_vms_mb": mb(best("peak_vms")),
        "last_wset_mb": mb(best("last_wset")),
        "last_private_mb": mb(best("last_private")),
        "last_vms_mb": mb(best("last_vms")),
    }
    print(json.dumps(summary, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
