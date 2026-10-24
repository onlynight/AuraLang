#!/usr/bin/env python3
"""解析 .auc（seed serialize.rs v3+ 格式），按函数 dump 操作码直方图 / 反汇编。

用法:
  python3 auc-disasm.py <path.auc> [--hist] [--disasm FUNC]
"""
import struct, sys, collections

OPNAMES = {
0:"LoadConst",1:"LoadVar",2:"StoreVar",3:"Add",4:"Sub",5:"Mul",6:"Div",7:"Rem",8:"Neg",9:"Not",
10:"And",11:"Or",12:"BitAnd",13:"BitOr",14:"BitXor",15:"Shl",16:"Shr",17:"Eq",18:"Ne",19:"Lt",
20:"Gt",21:"Le",22:"Ge",23:"Jump",24:"JumpIfTrue",25:"JumpIfFalse",26:"Call",27:"CallNative",28:"Return",
29:"ReturnUnit",30:"NewObject",31:"NewArray",32:"GetField",33:"SetField",38:"GetIndex",39:"SetIndex",
34:"IncRef",35:"DecRef",36:"CallC",40:"CallMethod",41:"CallCtor",80:"InstanceOf",81:"CheckCast",
42:"NewList",43:"NewMap",44:"ListPush",45:"ListPop",46:"ListLen",47:"MapSet",48:"MapGet",49:"MapLen",
50:"Yield",51:"NewCoroutine",52:"ResumeCoroutine",53:"DropRef",54:"Retain",55:"Release",56:"WeakRef",
57:"WeakGet",58:"BoxAlloc",59:"DeferBegin",60:"DeferEnd",37:"Halt",61:"CString",62:"ReadCStr",
63:"PtrIsNull",64:"PtrToInt",65:"IntToPtr",66:"MakeCallback",72:"MakeClosure",73:"CallClosure",
74:"EnumConstruct",75:"EnumTag",76:"MakeFnRef",70:"CallExport",71:"CallExternal",77:"CallAot",
82:"PushHandler",83:"PopHandler",84:"ThreadSpawn",85:"ThreadJoin",86:"ThreadSleep",87:"ThreadId",
88:"ThreadParallelism",89:"MutexNew",90:"MutexLock",91:"MutexUnlock",92:"MutexTryLock",93:"AtomicNew",
94:"AtomicLoad",95:"AtomicStore",96:"AtomicAdd",97:"AtomicCas",98:"RwLockNew",99:"RwLockReadLock",
100:"RwLockWriteLock",101:"RwLockReadUnlock",102:"RwLockWriteUnlock",103:"ChannelNew",104:"ChannelSend",
105:"ChannelRecv",106:"CondvarNew",107:"CondvarWait",108:"CondvarSignal",109:"CondvarBroadcast",
112:"ListIsEmpty",113:"ListContains",114:"ListIndexOf",115:"ListRemoveAt",110:"LoadGlobal",111:"StoreGlobal",
78:"CallNativeArgs",
}
OPSIZE = {0:2,1:2,2:2,30:2,32:2,33:2,72:2,74:2,76:2,77:2,110:2,111:2,23:4,24:4,25:4,82:8,
26:2,27:2,36:2,78:4,40:2,41:2,51:2,80:2,81:2,66:2,70:2,71:4,84:2}

class R:
    def __init__(s, d): s.d, s.o = d, 0
    def u8(s):
        v = s.d[s.o]; s.o += 1; return v
    def u16(s):
        v = struct.unpack_from("<H", s.d, s.o)[0]; s.o += 2; return v
    def u32(s):
        v = struct.unpack_from("<I", s.d, s.o)[0]; s.o += 4; return v
    def i64(s):
        v = struct.unpack_from("<q", s.d, s.o)[0]; s.o += 8; return v
    def f64(s):
        v = struct.unpack_from("<d", s.d, s.o)[0]; s.o += 8; return v
    def st(s):  # write_str: u16 len + bytes
        n = s.u16(); b = s.d[s.o:s.o+n]; s.o += n; return b.decode("utf-8", "replace")
    def st32(s):  # write_const Str: u32 len + bytes
        n = s.u32(); b = s.d[s.o:s.o+n]; s.o += n; return b.decode("utf-8", "replace")

def main():
    path = sys.argv[1]
    mode = sys.argv[2] if len(sys.argv) > 2 else "--hist"
    want = sys.argv[3] if len(sys.argv) > 3 else None
    d = open(path, "rb").read()
    r = R(d)
    assert d[:4] == b"AURA", d[:4]
    r.o = 4  # 跳过 magic
    version = r.u16(); flags = r.u32()
    name = r.st(); mver = r.st(); uuid = d[r.o:r.o+16]; r.o += 16
    print(f"version={version} flags={flags:#x} module={name!r} {mver!r}")
    nconst = r.u32(); consts = []
    for _ in range(nconst):
        t = r.u8()
        if t == 0: consts.append(("Int", r.i64()))
        elif t == 1: consts.append(("Float", r.f64()))
        elif t == 2: consts.append(("Str", r.st32()))
        elif t == 3: consts.append(("Bool", r.u8()))
        else: consts.append(("Null", None))
    print(f"consts={nconst}")
    nnat = r.u16()
    for _ in range(nnat):
        r.st(); r.u16(); r.u8()
        if r.u8() == 1: r.st()
        npt = r.u16()
        for _ in range(npt): r.u8()
        r.u8()
    print(f"natives={nnat}")
    nfn = r.u16()
    print(f"functions={nfn}")
    for fi in range(nfn):
        fname = r.st(); pc = r.u16(); loc = r.u16()
        is_native = r.u8(); aot_mode = r.u8()
        aot_idx = r.u32()  # v4+: u32（对齐 seed from_bytes）
        clen = r.u32(); code = d[r.o:r.o+clen]; r.o += clen
        mark = " <== main" if fname == "main" else ""
        print(f"  [{fi}] {fname} params={pc} locals={loc} native={is_native} aot_mode={aot_mode} code={clen}B{mark}")
        if (mode == "--disasm" and want == fname) or (mode == "--disasm" and want is None and not is_native):
            i = 0
            while i < len(code):
                op = code[i]; sz = OPSIZE.get(op, 0)
                arg = int.from_bytes(code[i+1:i+1+sz], "little") if sz else None
                nm = OPNAMES.get(op, f"op{op}")
                print(f"    {i:5d}: {nm} {arg if arg is not None else ''}")
                i += 1 + sz
        elif mode == "--hist":
            h = collections.Counter(); i = 0
            while i < len(code):
                op = code[i]; sz = OPSIZE.get(op, 0)
                h[OPNAMES.get(op, f"op{op}")] += 1
                i += 1 + sz
            for k, c in h.most_common():
                print(f"      {k:16s} x{c}")
    # 其余段（entry/exports/...）对本任务无用，不解析
    print(f"stopped at offset {r.o} / {len(d)}")

main()
