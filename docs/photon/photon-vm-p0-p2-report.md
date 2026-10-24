# P0–P2 实施报告（2026-10-07）

> 承接 [`photon-vm-cost-analysis-p412.md`](photon-vm-cost-analysis-p412.md) 的方案。
> 本轮：**P0 修复 + P1a 落地 + P1c 判定 + P2 阻塞定案**。
> 对象 `D:/Code/AuraLang`，终点 commit `6bb46b3`。

---

## 一、总结果

| 基准 | 本轮起点(P412) | 本轮终点(P413) | 提升 |
|---|---|---|---|
| fib28 | 4739 ms | **4339 ms** | **8.4%** |
| loop1M | 7601 ms | **6960 ms** | **8.4%** |

（交替 A/B 5 轮 / 3 轮取 min；区间紧密：P412 4739-4794、P413 4339-4388）

**本会话累计**（P409 → P413）：

| 基准 | 会话起点 | 终点 | 累计 |
|---|---|---|---|
| fib28 | 5270 ms | **4339 ms** | **17.7%** |
| loop1M | 8011 ms | **6960 ms** | **13.1%** |

**相对 Seed VM（fib28 930ms）：5.6× → 4.67×。**

五门全绿：micro1=10 / micro2=7 / micro3=49995000 / fib28=317811 /
loop1M=999998000000。

---

## 二、P0（硬阻塞）——已根治：`aura build --aot` 非确定构建

### 症状

连续两次构建同一份 `PhotonHatCompile.aura`，md5 不同；用它编译的宿主
**五门 0~2/5**（micro2 输出 `m2=0`）。这是发射器侧一切优化的硬阻塞——
在非确定驱动 + ±5% 布局噪声下，任何 `X86Emitter` 改动都无法可靠测量。

### 三层定位

1. 两次构建的 **`.ll`（LLVM IR）本身就不同**（16955569 vs 16955829 字节）
   ⇒ 非确定在 Aura 前端，**不在 LLVM**。
2. 逐行 diff 定位到 `HatSerializer_typeName` 的
   `mir.types.kindOf(typeId)`：在 `TypeRegistry_kindOf`（正确）与
   `Hir_kindOf`（错误）之间漂移。
3. 追到 `seed/compiler/src/codegen/hir.rs` 的 `resolve_method_owner`
   末尾「全表唯一候选」兜底：

   ```rust
   let mut cands: Vec<String> = table.iter()...   // table = CLASS_TABLE: HashMap
   ...
   // 2.6) 仍多候选：取第一个
   cands.into_iter().next()
   ```

   `kindOf` 在 Hir / Mir / TypeRegistry / Ast 等多个类上同名 ⇒ 多候选；
   arity 过滤后仍多候选 ⇒ **取 `cands[0]`，而顺序来自 `HashMap::iter`**。
   Rust 的 `HashMap` 用 `RandomState`，**迭代顺序每进程随机化**。

### 修复（两处，62 行）

**① 语义修复**（`resolve_receiver_type`）：新增 `局部变量.<字段>` 分支——
先取局部/参数的声明类型，再沿继承链查 `ClassEntry.field_types`，返回字段
类型名。`field_receiver_method`（步骤 1.5）只覆盖 `this.<字段>` 与类内
裸字段，**不覆盖「局部变量/参数的字段访问」**，而这正是 `mir.types` 的形态。
修好后接收者类型可正确解析为 `TypeRegistry`，走正常路径而非兜底。

**② 确定性护栏**（2.6 兜底前的 `cands.sort()`）：即使仍有未覆盖的歧义形态，
「取第一个」也从「随机取一个」变成「确定取字典序最小」。

### 验证

| 检查项 | 修复前 | 修复后 |
|---|---|---|
| 3 次构建 `.ll` | 行数与内容均不同 | **完全一致（298469 行 / 0 差异）** |
| 3 次构建 `.obj` | 不同 | **完全一致（同 md5）** |
| 3 次构建 `.exe` | md5 全不同 | **仅差 1 字节 @0x80**（PE TimeDateStamp，链接器写入的构建时间，纯装饰） |
| 用新驱动编译宿主 | 五门 0~2/5 | **五门全绿** |

### 附带修正两个工具链盲区

- seed 二进制（10-04 15:14）曾早于 `emit.rs` 最后提交（10-05 10:38）。
  重建命令必须带 `llvm` feature，否则 `codegen::aot` 被 cfg 掉：
  `cd seed && CARGO_TARGET_DIR=../rust/target cargo build --release --bin aura --features llvm`
- `scripts/photon/vmopt.sh` 新增 `driver` / `full` 子命令。**改发射器侧代码
  必须用 `full`**（`build` 不重建驱动，宿主仍用旧发射器）。

---

## 三、P1a——已落地：消除相邻 store→load 纯冗余对

### 目标

P412 分析实测：生成代码中 **31~48% 的指令是栈往返**，其中 961 对是
「相邻的 `mov [rbp+X], r` + `mov r, [rbp+X]`（同槽同寄存器）」——
两条合起来对寄存器与内存都无净效果，是**纯 no-op**。

### 实现

`X86Encoder` 新增四个只读计数（`byteLen`/`relocCount`/`labelCount`/
`pendingJumpCount`）+ 受限 `rollback(n)`。
`X86Emitter` 在 `storeDstReg` 落槽后记录「四重快照 + 槽 + 寄存器」，
在 `loadDstReg` 读槽前比对——四项全等且槽/寄存器都相同 ⇒ 回滚刚发射的
7 字节 store 并跳过 load。

**为什么用四重计数而非「字节数不变」单一判据**：`emitLabel` 只登记偏移、
**不发射字节**——只判字节数会让标签位置在回滚后错位，跳转跑飞。

### 两个必须绕过的坑（前两版因此完全没生效）

| # | 绕过点 | 后果 |
|---|---|---|
| 1 | `emitSetcc` 栈槽分支**直接** `enc.emitStackStore(...)` | 落槽不登记快照 |
| 2 | `resolveToReg` 栈槽分支**直接** `enc.emitStackLoad(...)` | 读槽不查快照 |

这两处正是实测 961 对的**主要来源**（`setcc` 落槽 → `emitCmpImm`
经 `resolveToReg` 读回）。均改为走 `storeDstReg`/`loadDstReg`（语义逐字
等价），并把 `emitMovImm`/`emitMovRR` 的栈槽分支一并统一。

### 实测

| 指标 | 改前 | 改后 |
|---|---|---|
| main.obj | 501645 B | **486877 B**（−14768 ≈ 2110 对） |
| 八个热点函数的存→读对 | 961 | **677**（消除 29.6%） |
| 同寄存器对（8 热点函数） | 677 | **1**（基本清零） |

### 附带负结果（已归档）

在 P413 之上尝试扩展「异寄存器时用 `mov rB, rA` 代替内存读回」：
只触发 19 次、7 轮交替 A/B 仅 **0.09%**（噪声内）——剩余 622 对异寄存器
读数**不经 `loadDstReg`**（主要来自 `emitCall` 直接
`enc.emitStackStore(..., "rax")` 落槽）。已回退。

---

## 四、P1c——已判定：低价值，暂不做

用 `[FSZ]` 探针 + 反汇编实测**帧超配**：

| 函数 | 帧 | 最大实际用偏移 | 浪费 |
|---|---|---|---|
| VmStack_push | 2728 B | 576 B | 2152 (79%) |
| VmLocals_getSlot | 2472 B | 376 B | 2096 (85%) |
| Vm_incRef | 2792 B | 528 B | 2264 (81%) |
| Vm_run | 2952 B | 784 B | 2168 (73%) |
| Vm_dispatch | 18120 B | 9840 B | 8280 (46%) |
| Vm_callOrBuiltin | 24568 B | 14120 B | 10448 (43%) |

**但收益极低**，放弃：

1. 栈探测（`sub rsp,0x1000; cmpb`）只在帧 > 4096 时出现，而
   `[FSZ]` 统计 **302 个函数里只有 5 个**超过 4096。
2. 两个真正热的大帧——`dispatch`(18120) 与 `callOrBuiltin`(24568)——
   即使**按实际用量精确收敛**（→9840 / 14120）**仍 > 4096**，探测照样发生。
3. `sub rsp, N` 是 1 条指令，N 变小不省指令数；未触及的帧页本来就不占
   cache。要真正消掉探测，必须让 `dispatch` 的帧降到 4096 以下 —— 那需要
   **按活跃区间（liveness）复用槽**，即 P1b 的一部分。

---

## 五、P2——已尝试，被真实阻断（后端 phi 缺陷）

### 前置链

P2（启用 `fuseHotPairs` 融合）的前置是 **P406**：`SsaBuilder.buildIf`
的 else 分支**无条件**回滚 `varMap`（§10.118 定案的真根因）。

### 实测：P406 单独应用即破坏五门

```
micro1  → 0           （期望 10）
micro2  → 0           （期望 7）
micro3  → 0           （期望 49995000）
fib28   → 317893      （期望 317811，错 +82）
loop1M  → -1000000    （期望 999998000000）
```

**这正是 §10.118 记载的「部署被后端 phi 缺陷阻塞」，本轮取得可复现见证**：
P406 让 `varMap` 在 else 前回滚 ⇒ 生成的 phi 入边集合改变 ⇒ 后端在
phi 合并处产出错码。fib28 只错 +82（微妙偏差）与 micro 全 0（彻底错）
并存，指向 phi 入边/版本映射的局部错配。

### 结论

P2 需要**先修后端 phi 缺陷**，这是独立且深入的一环，本轮不动。
P406 已完整回退，工作区干净。

**建议的下一步**：以本轮的 5 条失败输出（上表）作为 phi 缺陷的
最小复现基线，从 `SsaBuilder` 的 `insertPhisAtMerge` 与后端
`Lowering.applyPhi` 两侧做双向 dump 对照。

---

## 六、方案更新（P0-P2 之后）

| 优先级 | 项 | 状态 | 预期 |
|---|---|---|---|
| ~~P0~~ | 修非确定构建 | ✅ **已完成** | 解锁发射器侧全部改动 |
| ~~P1a~~ | 相邻存→读 peephole | ✅ **已完成** | 8.4% |
| ~~P1c~~ | 帧/序言收敛 | ✅ 已判定低价值 | 暂不做 |
| **P1b** | **寄存器分配质量** | 未做（工程量大） | **1.5~2×** |
| P2 | 融合 | ⛔ 被后端 phi 缺陷阻断 | 5~9% |
| P1b-0 | 按活跃区间复用槽（P1b 的子集） | 未做 | 可消 `dispatch` 栈探测 |

### 为什么 P1b 是下一步唯一的高杠杆

- 实测 IPC ≈ 2，**31~48% 的指令是栈往返**，且形成串行 store-forwarding
  依赖链 —— 这是与 Seed VM 差距的**结构性根因**。
- P413 只消除了「相邻同槽同寄存器」这一小类（2110 对）；**跨指令的
  栈往返仍是主体**。
- P1b 的收益量级（1.5~2×）远大于本轮的两项之和。
- 现在 P0 已修，**P1b 的每一步都可可靠测量**——这是本轮最重要的解锁。

### 本轮确立的可复用能力

- **`scripts/photon/vmopt.sh full`**：驱动 → 宿主 → 五门 → 计时，一条命令。
- **`tools/photon-prof/`**：per-opcode 计数器 + 斜率法成本测量。
- **P411 的确定性判据**：`diff .ll` 应 0 差异、`.exe` 仅差 1 字节 @0x80。
  若 `.ll` 又出现差异 ⇒ 又引入了 HashMap 迭代顺序依赖，查
  `resolve_method_owner` 类兜底路径。
