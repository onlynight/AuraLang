#!/usr/bin/env python3
# P411-d：per-opcode 真实成本测量
#
# 方法学（规避本项目两大已知干扰）：
#   1. 布局噪声：宿主二进制固定，只有客体 .auc 变化 —— 客体是指令**数据**，
#      不是宿主代码，因此不存在宿主布局扰动。全部变体共用同一个宿主。
#   2. 启动固定开销：每个 guest 同时跑 N 与 2N 两档，取斜率
#      (T(2N)-T(N))/N —— 装载、初始化、进程启动全部相减抵消。
#
# 计时用**未插桩**宿主（build/tmpwork/host_baseline.exe），计数用插桩宿主。
import subprocess, sys, os, statistics, json

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
os.chdir(ROOT)

TIMING_HOST = os.path.join(ROOT, 'build/tmpwork/host_baseline.exe')
COUNT_HOST  = os.path.join(ROOT, 'build/hat-vmhost/current.exe')
SEED_CLI    = os.path.join(ROOT, 'rust/target/release/aura.exe')
AUC         = os.path.join(ROOT, 'build/vm/current.auc')
N1, N2      = 1_000_000, 2_000_000
REPS        = 5

def gen_auc(aura_path):
    if os.path.exists(AUC):
        os.remove(AUC)
    subprocess.run([SEED_CLI, 'run', aura_path, '--vm=aura'],
                   capture_output=True, timeout=180)
    if not os.path.exists(AUC):
        raise RuntimeError('auc 生成失败: ' + aura_path)
    return AUC

def time_guest(aura_path, reps=REPS):
    gen_auc(aura_path)
    runs = []
    import time
    for _ in range(reps):
        t0 = time.perf_counter()
        subprocess.run([TIMING_HOST], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=600)
        t1 = time.perf_counter()
        runs.append((t1 - t0) * 1000.0)
    return min(runs), runs

def count_guest(aura_path):
    gen_auc(aura_path)
    env = dict(os.environ, AURA_OP_PROF='1')
    p = subprocess.run([COUNT_HOST], capture_output=True, text=True,
                       env=env, timeout=900)
    counts, total = {}, 0
    for line in p.stdout.splitlines():
        if line.startswith('[OPPROF] '):
            parts = line.split()
            counts[int(parts[1])] = (parts[2], int(parts[3]))
        elif line.startswith('[OPPROF-TOTAL] '):
            total = int(line.split()[1])
    return counts, total

def main():
    guests = ['g0', 'g1', 'g2', 'g3']
    data = {}
    for g in guests:
        row = {}
        for sz, n in (('1M', N1), ('2M', N2)):
            path = f'build/prof/{g}_{sz}.aura'
            best, runs = time_guest(path)
            counts, total = count_guest(path)
            row[sz] = {'ms': best, 'runs': [round(r) for r in runs],
                       'instr': total, 'counts': {str(k): v[1] for k, v in counts.items()},
                       'names': {str(k): v[0] for k, v in counts.items()}}
            print(f'{g}_{sz}: {best:.0f}ms instr={total}', flush=True)
        data[g] = row

    print('\n=== 斜率（每 100 万次迭代）===')
    print(f'{"guest":6} {"Δms/1Miter":>12} {"Δinstr/1Miter":>14}')
    slopes = {}
    for g in guests:
        dm = data[g]['2M']['ms'] - data[g]['1M']['ms']
        di = data[g]['2M']['instr'] - data[g]['1M']['instr']
        slopes[g] = (dm, di)
        print(f'{g:6} {dm:12.1f} {di:14d}')

    print('\n=== 每迭代 opcode 增量 ===')
    opset = set()
    for g in guests:
        a = data[g]['1M']['counts']; b = data[g]['2M']['counts']
        for k in set(a) | set(b):
            opset.add(int(k))
    inc = {}
    for g in guests:
        a = data[g]['1M']['counts']; b = data[g]['2M']['counts']
        row = {}
        for op in sorted(opset):
            d = b.get(str(op), 0) - a.get(str(op), 0)
            if d:
                row[op] = d
        inc[g] = row
        names = data[g]['1M']['names']
        txt = ' '.join(f'{names.get(str(op),"?" )}={v}' for op, v in sorted(row.items(), key=lambda x: -x[1]))
        print(f'{g}: {txt}')

    json.dump({'slopes': {g: list(v) for g, v in slopes.items()},
               'inc': {g: {str(k): v for k, v in r.items()} for g, r in inc.items()},
               'raw': data}, open('build/prof/result.json', 'w'), indent=1)
    print('\n-> build/prof/result.json')

main()
