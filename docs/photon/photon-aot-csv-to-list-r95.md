# Photon AOT 自举编译 —— 第三轮：CSV → List 数据模型重构（R95）

> 2026-10-20 00:50–02:10
> 承接 R94（后端 446.1s）。本轮把后端操作数模型从「逗号串 + 每次访问重新 split」
> 改成「一次性解析的 `List<Int>`」，并消除 RA 的三处高频繁殖 split。

---

## 一、结果

| 指标 | R94 基线 | R95 | 变化 |
|---|---:|---:|---:|
| 后端合计 | 446.1 s | **406.5 s** | **−8.9%** |
| 　phaseC（DAG/指令选择） | 210.5 s | **174.3 s** | **−17.2%** |
| 　ra（寄存器分配） | 94.0 s | 97.7 s | +4%（噪声） |
| 　emit（x86 编码） | 88.7 s | 85.0 s | −4.2% |
| 　writer（COFF 输出） | 45.8 s | 42.7 s | −6.7% |
| 正确性 | success | **success** | 不变 |

验证：
- 后端单跑 `===RESULT===success`，`aura-compiler.exe` 3.05 MB；
- 自编译产物运行 **`BOOT-1 → BOOT-1a → … → BOOT-4`**（比 R94 的 `BOOT-1a` 走得更远，exit 0）；
- 干净驱动冒烟 hello world：`success` + `Hello, World!`。

> 注：测量跑带了 substring 长度直方图探针（每次调用一个分支），实际干净驱动会略快。

---

## 二、重构内容

### 2.1 `DagNode.args` / `DagInstruction.nodes`：CSV → List

```aura
class DagNode {
    var args: String = ""                       // 保留：HAT/调试/序列化形态
    var argIds: List<Int> = arrayListOf<Int>()  // 新增：解析结果（唯一读取源）
    fun setArgs(csv: String) { this.args = csv; this.argIds = MachineDagUtils.parseIdCsvLenient(csv, false) }
}

class DagInstruction {
    var nodes: String = ""
    var nodeIds: List<Int> = arrayListOf<Int>()
    fun setNodes(csv: String) { this.nodes = csv; this.nodeIds = MachineDagUtils.parseIdCsvLenient(csv, true) }
}
```

- `argAt/argCount/nodeAt/nodeCount` 变成 **O(1)、零分配**（原来每次调用整串 split）；
- 解析器 `MachineDagUtils.parseIdCsvLenient(csv, strict)` 一次扫描、零字符串分配：
  - `strict=true`（nodes）：token 必须「可选 `-` + 全数字」，否则记 **-1** ——
    与旧 `isNumericToken` 语义逐字一致（标签 token `bb4` 必须被识别为 -1，
    否则活跃区间会把标签当节点，寄存器全判占用）；
  - `strict=false`（args）：按旧 `strToInt` 语义提取数字（非数字字符忽略）。

**写入纪律**：所有直接赋值点都改成走 setter（否则列表会过期）——
`InstructionSelection` 7 处 `.args =`、2 处 `.nodes =`；`PeepholeOptimizer` 12 处 `.nodes =`；
构造入口 `addNode/addInstr` 也走 setter。字段处已加 ⚠ 注释说明。

### 2.2 `RegisterAllocator` 的三处高频繁殖 split

| 位置 | 旧 | 新 |
|---|---|---|
| `dfs(node.args)` | 每次访问 split 整串 → 递归中 O(N²) | `node.argIds` 直取 |
| `buildInterference(node.args)` | 同上（干扰图按节点反复构建） | `node.argIds` |
| `getSuccIndices` ×2 | `splitComma(instr.nodes)` 只为取首个标签 | 新增 `firstCsvToken`，原地取一次 |

---

## 三、一个重要的负结果

用临时 substring 长度直方图（已移除）测得后端 substring 共 **3.877 亿次**，分布：

| 长度桶 | 次数 |
|---|---:|
| 1 B | 5,605,503 |
| 2–3 B | 395,334 |
| **4–7 B** | **364,615,558** |
| 8–15 B | 1,264,286 |
| 16–31 B | 12,395,978 |
| 32–63 B | 3,423,246 |
| 其余 | < 8 万 |

而 **4–7 B 桶在 R94/R95 全部改动前后逐字节相同**（364,615,558 三次完全一致）⇒
这批 substring **不来自** `args/nodes` 访问器、`splitArgs`、RA 的 node/args split。
它们另有来源（长度像节点 id / 标签 / 短标识符）。

已排除：`isRuntimeSymbol`、`nodeAt/nodeCount/argAt/argCount`、`splitArgs(args|nodes)`、
`dfs/buildInterference/getSuccIndices`。

**下一步定位手段**（一次 8 分钟单跑即可）：
在 `aura_string_substring` 内按 `__builtin_return_address(0)` 分桶（C 层调用者是
`String_substring` / `String_substringAt` / `String_split` 等，可符号化），
若命中 `String_split`，再在其入口按「入参长度 + 元素数」分桶即可锁定 Aura 调用点。

---

## 四、结论

- CSV → List 的核心收益已兑现：**phaseC −17.2%、后端 −8.9%**，且访问路径不再分配。
- 但后端仍有 406s，说明**字符串分配只是成本的一半**：`phaseC 174s / ra 98s / emit 85s`
  的算法与常量因子（140 臂分派、每指令重复解析同类结构）仍是主体。
- 剩余分配热点（3.64 亿次 4–7B substring + `dynlist_push` 2.9GB）需要按调用者归因后再动。

### 下一轮建议（按杠杆）
1. 用「调用者分桶」定位 3.64 亿次 substring（方法见上）。
2. `dynlist_push` 2.9 GB：关键链表按预估容量预分配（`newList(cap)`）。
3. `phaseC` 内部：把「每条指令都重新查找的符号/模板表」改为一次构建的索引表。
4. `hatWrite 45.7s / 10.3MB` 与后端峰值 21.8 GB 仍未处理（见 R94 报告）。

---

## 五、改动文件

| 文件 | 改动 |
|---|---|
| `aura/photon/aura/lang/compiler/photon/MachineDag.aura` | `argIds/nodeIds` + `setArgs/setNodes` + `parseIdCsvLenient`；四个访问器 O(1) |
| `aura/photon/aura/lang/compiler/photon/InstructionSelection.aura` | 7×`.args=`/2×`.nodes=` 改 setter（R94 的 `isRuntimeSymbol` 原地比较保留） |
| `aura/photon/aura/lang/compiler/photon/PeepholeOptimizer.aura` | 12×`.nodes=` 改 setter |
| `aura/photon/aura/lang/compiler/photon/RegisterAllocator.aura` | `dfs`/`buildInterference` 用 `argIds`；`getSuccIndices` 用 `firstCsvToken` |
| `seed/compiler/src/std/cffi/aura_std_cffi.c` | 临时直方图探针（已移除，净改动为零） |

产物：`build/hat-be6/`（本次测量）、`build/hat-native/smoke-r95/`（冒烟）。
