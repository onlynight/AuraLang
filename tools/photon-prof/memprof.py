#!/usr/bin/env python3
"""进程内存采样器 — 启动目标命令并高频采样其内存，退出后输出峰值 + 回收判定。

用法:
    python3 memprof.py [--runs N] [--interval MS] [--quiet] -- <command> [args...]

输出（每次运行一行 JSON）：
    label / cmd / runs / exit_ok / wall_ms
    peak_wset_mb / peak_private_mb / peak_vms_mb
    last_wset_mb / last_private_mb / last_vms_mb

口径说明（Windows / psutil）：
    peak_wset    峰值工作集（物理驻留峰值，含共享页）—— 「任务管理器」口径
    peak_private 峰值私有提交（PagefileUsage，不含共享）—— 「私有字节」口径
    peak_vms     峰值虚拟地址空间（含 mmap/预留，不代表实际占用）

⚠ peak_wset 只是峰值快照，**回答不了「运行中是否回收了」**——真正的回收判定
  靠 VM 内部的 [MEMPROBE] 行（arena 水位斜率 + 值栈倍增计数，用 --quiet 抓取）。
  另：exit≠0 先查 VM-ERROR= 行——Instruction limit 是 2×10⁹ 指令守卫，与内存
  无关（loop1100m 曾被误判为「泄漏在 wset 上不可见」，实为指令上限）。

  判读 [MEMPROBE] 的方法见 docs/photon/vm-memory-comparison.md §十.4。

建立于 2026-10-07（Rust VM vs Photon Aura VM 内存对比）；
2026-10-08 增加 --quiet（用于抓取宿主 stdout 里的 [MEMPROBE] 行）。
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
        stdout=devnull if quiet else sys.stdout,
        stderr=devnull if quiet else sys.stderr,
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
    ap.add_argument(
        "--quiet",
        action="store_true",
        help="转发宿主 stdout/stderr（用于抓取 [MEMPROBE] 行）；默认静默宿主输出",
    )
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    args = ap.parse_args()
    cmd = args.cmd
    if cmd and cmd[0] == "--":
        cmd = cmd[1:]
    if not cmd:
        print("用法: memprof.py [--runs N] [--interval MS] [--quiet] -- <command...>", file=sys.stderr)
        return 2

    runs = []
    for _ in range(args.runs):
        # --quiet: 转发宿主 stdout/stderr（用于抓 [MEMPROBE] 行），不打 JSON；
        # 默认: 静默宿主输出，只打 JSON。
        r = sample_once(cmd, interval_s=args.interval / 1000.0, quiet=not args.quiet)
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
    # JSON 恒为最后一行（--quiet 模式下宿主输出在前，脚本用 tail -1 取本行）
    print(json.dumps(summary, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
