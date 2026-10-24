# R168：P5-① 的最小复现 —— `x as T` 内联当操作数/实参时被译坏

日期：2026-10-22　分支：dev　前置：R167（宿主可重建，但值模型仍错：`println("hello-jit")` 打 `0`）

## 一、结论先行

`.auc` 值模型崩掉的**直接原因**是一条可以在 49 KB 程序里 1 秒复现的产码缺陷：

```aura
val v: Any = "hello"
val t: String = v as String
println("ok=[" + t + "]")             → ok=[hello]        ✅
println("bad=[" + (v as String) + "]") → bad=[<ref#-1>]    ❌
```

**`as` 转换的结果先落局部是对的；把 `(x as T)` 直接当 `+` 的操作数（或调用实参）就坏。**
复现用例已入库：`tests/photon/debug/cast_inline_probe.aura`。

## 二、它是怎么被找到的（证据链）

### 1. 症状固定在「常量 → 局部 → println」这条链上

给 `Vm.run()` 开全指令轨迹（`xTraceCap`，仓库已有但从未启用的设施）后，`hello.auc` 的指令流是：

```
ip=0 op=7  a0=[0]          LOAD_CONST 0   ← 常量池第 0 条 = "hello-jit"
ip=1 op=11 a0=[1]          STORE_LOCAL 1
ip=2 op=10 a0=[1]          LOAD_LOCAL 1
ip=3 op=62 a0=[println]    CALL_NATIVE_ARGS println 1
```

`[K7]` 探针确认 op7 取到的常量是对的（`v=[hello-jit]`），而 `[CA]` 探针显示
`collectArgs` 弹出来的值是 `raw=1`（= 装箱整数 0）。⇒ 值在 **push / STORE / LOAD / pop**
这条路上丢的，与常量池、与 `println` 都无关。

### 2. 反汇编证明「写入点是正确的」

`VmStack_push`（`build/vmhost-h11/current.obj` @0x1B7DE）：

```asm
movq -0xa8(%rbp), %r10     ; v（第二个实参）
...
movq %r10, %rdx
movq %rdx, (%rcx)          ; *dest = v      ← 原样写出，没有转换
```

`as Long` 在**这里**被降成 no-op（`v + 0`），所以写入端没问题 ⇒ 可疑面收窄到
「实参 `v` 到达被调函数时是否还是原值」。

### 3. Heisenbug → 指向表达式形状

在 `VmStack.push` 里加一行 `println` 之后，同一份代码 `[PS] rawOf=140695287381696`（正确指针）；
去掉它就又变回 0。加/减一行无关代码改变结果 —— P5-① 的典型指纹。

于是把「Any 值打平进裸缓冲再读回」抽到一个**独立小类**里（`build/vmhost-src/stackprobe.aura`），
52 KB、1 秒构建，仍然失败（`push("hello")` → `pop()` 得到 `<ref#-1>`）⇒ 缺陷与 VM 无关。

### 4. 继续缩到两行

去掉类、去掉内存缓冲，只剩 cast：

```aura
val v1: Any = "hello"
println("[2] v1 direct=" + v1)          → hello          ✅  （Any 直接打印没问题）
val t1: String = v1 as String
println("[3] t1=" + t1)                 → hello          ✅  （cast 落局部没问题）
println("[7] inline=[" + (v1 as String) + "]") → <ref#-1>  ❌  （cast 内联当操作数坏）
```

⇒ **判据：`as` 表达式是否被当作「内联操作数/实参」**。

## 三、为什么它把整个 VM 值模型带崩

VM 的热路径全是这个形状：

| 位置 | 代码 | 形状 |
|---|---|---|
| `VmStack.push` | `Memory.write64(this.mc + sp0 * 8, v as Long)` | cast 当调用实参 |
| `VmLocals.putSlot` | `Memory.write64(this.mcSlots + k * 8, v as Long)` | 同上 |
| `VmLocals.getSlot` | `return Memory.read64(...) as Any` | cast 当返回值表达式 |

于是「字符串常量 → 栈 → 局部 → println」每一步都可能被清零，`.auc` 侧打出来的就是 `0`。

## 四、本轮试过但**没生效**的规避

按「先落局部」改写 `VmStack.push / pop` 与 `VmLocals.putSlot / getSlot`
（`val rawF: Long = v as Long` → `Memory.write64(addr, rawF)`；读侧同样两段式），
重建宿主后 `[CA] raw` **仍然是 1**。

⇒ 宿主上至少还有**第二个**丢失点（大概率在 `getConstByIdx` 的 `String → Any` 返回值，
或 `stack.push(Any)` 的实参传递），单点规避无效。这一版改动已回退，未入库。

## 五、下一步（T5 建议按此顺序）

1. **先修 `as` 内联这一个形状**。判据已经干净：`tests/photon/debug/cast_inline_probe.aura`
   第 7/8 行必须打 `[hello]` / `[lit]`。修好后立刻用 `stackprobe.aura`（52 KB）回归。
2. 修完再回 VM 宿主跑 `hello.auc`，看 `println("hello-jit")` 是否恢复；若仍为 `0`，
   用同样的「cast 内联」判据去扫 `getConstByIdx` / `stack.push` 这两处。
3. 只有值模型正常后，R166 定位的 8 例 `toStr(Float)` 除零、4 例 `Unknown function`
   才有真实验证载体。

## 六、相关资产（`build/` 被 gitignore，仅本地）

| 路径 | 用途 |
|---|---|
| `tests/photon/debug/cast_inline_probe.aura` | **最小复现**（已入库） |
| `build/vmhost-src/castprobe2.aura` | 同上，带装箱/拆箱分项 |
| `build/vmhost-src/stackprobe.aura` | VmStack 抽取版（52 KB，1 秒构建） |
| `build/_vmhost_build.ps1` | HAT 驱动 → COFF → lld-link，8 秒一轮 |
| `xTraceCap`（`Vm.run` 内） | 全指令轨迹 `[X] ip=.. op=.. top=.. sp=..`（仓库已有，默认 0） |
