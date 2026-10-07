# P414 寄存器着色重启：已落地部分 + 边界探测报告

> 2026-10-07 续 [P0–P2 报告](photon-vm-p0-p2-report.md)。
> 目标：攻 P1b（寄存器分配）——报告判定的「唯一高杠杆」。
> 结果：**小幅落地（fib28 1.78% / loop1M 3.48%），并把失效边界钉死到一条指令**。

---

## 一、找到了真正被禁用的东西

`RegisterAllocator.allocate()`（`aura/photon/aura/lang/compiler/photon/RegisterAllocator.aura`）
里整段寄存器着色被 **P270 注释掉**，原文：

> ⚠️ P270 **全部值走栈槽**（禁用寄存器着色）——每值一个 `slotForNode` 专属槽，
> 读写家一致，彻底消除：寄存器冲突的布局敏感性、跨块值被其他值覆写、
> phi 合并值的 RA 区间不可靠。**性能：全部经内存读写（比寄存器慢 ~2×），
> 但正确性优先。** 后续优化：只对跨块值溢出、短寿命值恢复着色（需正确 liveness）。

干扰图构建（`buildIntervalInterference`）、着色（`assignColor`）、溢出（`spill`）、
乐观重着色（`tryOptimisticRecolor`）、回边分析（`buildBackEdges`）、
反向定点 liveness（`computeLiveAtEnd`）——**整套机制完整存在，只是没被调用**。

这就是 P412 分析测出的「31~48% 指令是栈往返、IPC≈2、串行 store-forwarding
依赖链」的**直接来源**。P270 自己留了重启路线，本轮按此路线做。

---

## 二、两个必须绕过的坑（实测定位，均已修复）

### ① RA 溢出区与 `slotForNode` 额外槽区在帧里**重叠**（新发现的硬冲突）

```
getSpillOffset(nodeId) → 144 + slotNum * 8        （用作 -offset）
slotForNode(nodeId)    → -(144 + 8 + extraSlotCount * 8)
```

两者**同处 144 基线以下、逐 8 字节向下分配**。直接启用带 `spill()` 的
`assignColor` ⇒ **宿主立刻段错误**。P270 下 `spillSlots` 恒为 0，冲突从未暴露。

**处置**：新增 `tryColorLocal` —— 只在**有空闲颜色**时着色，**绝不调用 `spill()`**。
无颜色即不着色，退回 `slotForNode`，行为与 P270 完全一致。

### ② P413 peephole 的**过期快照**（附带修复的 P413 潜在 bug）

`storeDstReg` 在 `dstRaw == srcReg`（两侧同名寄存器）路径**既不发射也不失效快照**，
上一次落槽的快照残留；此时四重计数未变，紧随的 `loadDstReg` 会误判「紧邻」
而删掉一条必需的 load。P270 下几乎所有目的都是栈槽，该路径几乎不触发；
着色启用后寄存器操作数变多，概率显著上升。

**处置**：`storeDstReg` 入口统一 `lastStLen = -1`，仅在真正落槽后重建快照。

---

## 三、保守判据与落地收益

`isColorableLocal(nodeId)` 三个必要条件（缺一即放弃着色）：
1. 有活跃区间信息（`first/last >= 0`）；
2. 区间跨度 ≤ `COLOR_MAX_SPAN`；
3. 区间内不含基本块边界（不存在块入口 `b` 满足 `first < b <= last`）
   —— 一次性排除 P270 记载的「跨块值被覆写」与「phi 合并区间不可靠」。

**`COLOR_MAX_SPAN = 0`（节点只出现在单条指令中）五门全绿。**

| 基准 | P413 | P414 | 提升 |
|---|---|---|---|
| fib28 | 4332 ms | **4255 ms** | **1.78%** |
| loop1M | 7750 ms | **7480 ms** | **3.48%** |

（7 轮 / 3 轮交替 A/B 取 min。宿主 exe 67534336 → 67520000 字节。）

---

## 四、失效边界：**放宽到 3 条指令即段错误**

```
COLOR_MAX_SPAN = 0   → 五门全绿
COLOR_MAX_SPAN = 3   → 段错误
```

**失败形态很明确**：loader 解出的指令数塌缩
（`[OC] n=34`，正确值 `n=241`）⇒ 说明**两个同时活着的值被分到同一寄存器**。

即：**着色在「值跨 ≥2 条指令存活」时不可靠**；单指令区间（定义与使用在同一条
指令内）绝对安全。

---

## 五、已排除的方向（本轮实测，供后续省时）

| 怀疑方向 | 结论 | 依据 |
|---|---|---|
| **干扰判定低估**（区间偏短） | ❌ **已排除** | 把干扰判定改为「**整块区间**」——同一基本块内任意两节点都互判干扰，块内不可能漏判——**仍然段错误**。测试代码：`buildExpandedSpans()` 把每节点区间扩到首末块边界，`buildIntervalInterference` 改用扩展区间；结果同 span=3 |
| `emitShlRR`/`emitShrRR` 用 r12 作暂存 | ❌ 不构成冲突 | 成对 `push r12` … `pop r12`，且中途 `mov r12,rcx` 后**末尾有 `mov rcx,r12` 恢复**，r12 净效果守恒 |
| `Const` 节点被指令选择预分配寄存器 | ❌ 无此问题 | `InstructionSelection.emitConst` 明确注释「寄存器由分配器决定」 |
| 前导 callee-saved 的 push/pop 受着色影响 | ❌ 不受 | 所有函数 prologue 恒为 `push=8`（7 池寄存器 + rbp），是**固定集合**，不随用法变化 |
| `emitCall` 未处理已着色目的 | ❌ 已处理 | `emitCall` 经 `regOfNode(node)` 后按 `dst.startsWith("stack:")` 分支，寄存器目的走 `emitMovRR(dst, "rax")` |

**结论：问题不在 RA 的干扰判定，而在「跨指令保持寄存器」时发射器侧的某条路径。**

---

## 六、下一步的高价值怀疑点

按可能性排序：

1. **`touchInstr` 的区间计算只深入操作数 `args` 一层**
   （`RegisterAllocator.touchInstr`：遍历 `instr.nodeAt(k)` 后只取 `cn.args` 的直接
   成员，不再递归）。若某节点的使用只通过更深层的链可见，其区间偏短 ——
   **但这已被第五节第 1 项排除**（整块区间仍失败），故本条降级为「次要」。
2. **发射器某处把池内寄存器当临时用**：已查 `X86Emitter` / `X86Encoder` 中
   池名（rbx/rsi/rdi/r12-r15）的字符串出现——只有 `isCalleeSaved` 判定与
   shl/shr 的 r12。建议再查 `PhotonRuntime.aura` 与运行时 obj 侧。
3. **`InstructionSelection` / `Lowering` 阶段对 `DagNode.reg` 的既有约定**：
   若某类节点依赖「`reg == ""` 即走栈槽」的隐含契约，着色会打破它。
   建议：在 `applyColors` 后加断言，dump 所有「被着色但随后仍被 `slotForNode`
   读回」的节点（即读写两个家的节点）。
4. **`emitSetcc` / `emitCmpImm` 的 `instr.cond` 与 `setcc` 结果的寄存器契约**。

**可复现基线**（下次直接用）：
```
# 把 RegisterAllocator.aura 的 COLOR_MAX_SPAN 改为 3，然后
./scripts/photon/vmopt.sh full
# 预期：段错误；[OC] n=34（正确 241）
```
