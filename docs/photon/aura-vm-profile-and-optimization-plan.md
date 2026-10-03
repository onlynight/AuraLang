# Aura VM 性能剖析与优化方案（2026-10-02，供决策）

> 结论先行：Aura VM 的"慢"**不是解释器固有的慢，而是 97% 的执行时间耗在 4 个
> O(表长) 字符串扫描函数上**（每次函数调用都全量 split+扫描函数表）。
> 这属于算法问题，有明确的小改动高收益修法；修掉后预期元循环性能提升 **50-200 倍**。

## 一、剖析方法与数据（可复现）

元循环通道执行 `bench_fib10`（fib(10)，约 4 万条 Aura VM 指令 + 939 函数的 std 内联），
用种子 VM 看门狗采样（`AURA_VM_WATCH=20000000`，每 2000 万条 Rust VM 指令采样一次
Aura 级帧栈），360 秒采到 **368 个样本 ≈ 36.8 亿条 Rust VM 指令**：

| 热点函数 | 样本占比 | 说明 |
| --- | --- | --- |
| `VmOps.split` | 61% | **被 findFuncParamCount/findFuncOffset 每次调用**，对整张函数表逐行 split（每行分配子串！） |
| `VmOps.findFuncParamCount` | 23% | 每次 CALL 都全量扫描函数表查参数个数 |
| `VmOps.vmSubstr` | 13% | split/findFuncOffset 内部的子串分配 |
| `VmOps.findFuncOffset` | 13% | 每次 CALL 都全量扫描函数表查入口偏移 |

阶段分布：**99.6% 在执行期**（loadPredecoded/dispatch/callOrBuiltin），装载期仅 0.6%。
内存持续增长 = split/vmSubstr 每次 CALL 分配约 940 行 × 数个临时子串的垃圾。

**量化**：每条 CALL ≈ 2 次全表 split + 扫描 ≈ 9 万+ 条 Rust VM 指令（元循环放大
20-50 倍 × O(表长) 扫描 + 每行字符串分配）。fib(10) 约 3000 次 CALL → 数百亿条
指令 → 数百秒。**算术/比较/跳转等"真正的解释"在采样里几乎不可见**——它们不是瓶颈。

> 此前已修复的两个同类热点（本轮已合入）：`getConst` 的 O(|常量池|) 全池扫描 →
> O(1) 行缓存；`LOAD_LOCAL` 等的每指令 `toI32` 字符串解析 → 加载期预解析 `arg0Ints`。
> 修复后常量池扫描已从热点榜消失，验证了"O(表长) 扫描"就是正确的病灶模型。

## 二、优化方案（按收益/风险排序，供决策）

### 方案 A：加载期建函数表索引（强烈推荐——改动小、收益 ~50-200×）

`loadPredecodedFromFields` 里把 funcTable **一次性**解析成三个平行列表：
`funcNames / funcOffsets / funcParamCounts`（加一个 `name→index` 的解析）。
CALL 执行路径改为：
- `funcParamCount(name)` → 查表 O(1)（或加载期把 argc 解析进 `argInts`，零运行时成本）；
- `funcOffset(name)` → 查表 O(1)。
- name→index 解析实现二选一：
  - **A1（保守）**：保留逐行字符扫描但**零分配**（不 split，直接 charCodeAt 比较），
    收益约 10-30×，AOT 载体同样安全；
  - **A2（彻底）**：`HashMap<String, Int>` O(1)（种子宿主可靠；AOT 宿主下 HashMap
    属已修复大半的路径，需回归验证）。
- 预期：元循环 fib(10) 从 >400s → **约 10-30s**；fib(30) 从"不可行" → 约 10-30 分钟；
  内存增长（split 垃圾）同步消失。
- 改动量：Vm.aura 约 60-100 行；风险低（纯加载期新增，执行路径收窄）。

### 方案 B：加载期把 CALL 解析成函数下标（与 A 叠加，消灭调用路径全部字符串）

`.auc` 的 CALL 首参已是函数名——在 `loadPredecodedFromFields` 里**一次性**把
`CALL name` 重写为 `CALL <index>`（写回 argCache），执行期纯整数分派。
与 A2 组合后调用路径零字符串。改动量：约 30 行；需注意文本模式的回退兼容。

### 方案 C：接收者类型传播（解锁 AOT 原生载体——收益最大的"架构"项）

Aura VM 真正的速度天花板在 **AOT 原生载体**（同一份 Vm.aura 编译为原生，无元循环
20-50× 放大）。当前 AOT 载体被两个已定位的 AOT 后端缺陷卡住：
1. **局部结构体指针损坏**（`val mod = loadModule(...)` 的局部指针截断，从它读的
   字段全是垃圾——本轮已把装载链改为字段传递绕开一半，执行期仍需根治）；
2. **多候选歧义裸名**（14 个未定义符号，需前端接收者类型传播）。
两项都在 `seed/compiler/src/codegen/`（**不在 D6 冻结区**），有明确的修复路径；
完成后 Aura VM 获得原生速度（预期 fib(10) 亚秒级），且惠及所有 AOT 程序。

### 方案 D：解释器结构优化（收益 3-5×，改动大，可后置）

分派链按操作码频次重排 / 超级指令融合（LOAD+ADD+STORE）/ VmStack 值去字符串化。
在 A/B 之前做这些是**本末倒置**——采样证明分派本身几乎不在热点里。

## 三、建议组合

- **最小闭环**：A1 + B（约 1-2 小时改动，元循环可用性质变，可立即基准验证）；
- **正解闭环**：A2 + B + C（Aura VM 以原生速度运行，fib 基准预计进入亚秒-秒级，
  需要额外处理 AOT 局部指针损坏专项）。

决策后按所选组合实施，实施完用 `scripts/bench-photon-vs-vm.ps1 -ThreeWay` 复测对比。

---

## 四、2026-10-03 状态更新：AOT 载体已打通，优化方案前置条件就绪

上一轮的「正解闭环」前置（方案 C 的两项 AOT 后端缺陷）已在本轮修复
（详见 `photon-bootstrap-link-benchmark.md` §五的 14 项修复清单）：

- **方案 C-1（局部结构体指针损坏）**：HIR 接收者字段链解析 + 局部槽
  `llvm_type_checked` + null 判空/`aura_any_equals` 值模型修复后，
  `auc_vm_runner.exe` 端到端 PASS。
- **方案 C-2（多候选歧义裸名）**：字符串方法改派 owner 守卫修复后，
  helloworld/fib/loop 的 .auc 全部正确执行。

**当前实测**（同机）：Aura VM 原生载体 fib(30) ≈ 104s 完成（此前 >600s
不出）；瓶颈已回到**方案 A/B 的 CALL 路径 O(表长) 函数表扫描**——
方案 A1/A2 + B 实施后预期进入 1-10s 量级。

**新增的两个方案项**（2026-10-03 探针发现）：

- **方案 E：`getConst` 快路径**——常量池行扫描 `getFieldAt` 是
  O(|常量池|) 逐字符扫描；`intOf` 快路径已就位，可加载数值常量
  预解析数组直接消除。
- **方案 F：Int32/Int64 语义对齐**——bench_loop 的 acc 在 AOT 载体按
  32 位回绕（种子 VM 64 位累加），基准程序应显式用 Long，或两侧统一
  算术宽度语义（需决策）。

**Photon 编译的 VM 的最后阻塞**：宿主二进制闭包尺寸阈值触发的
loadPath 段错误（复现矩阵与探针见 `photon-bootstrap-link-benchmark.md`
§五遗留 1），需要调试器定位故障地址。

---

## 五、2026-10-03 续二：方案 A2/D 落地 —— fib(30) 18.5s → 7.2s（相对最初 104s 累计 14.4×）

### 已实施（Vm.aura / VmStack.aura / Opcodes.aura）

| 项 | 内容 | 效果 |
| --- | --- | --- |
| **D-1 调用栈并行列** | 新类 `VmCallStack`：ips/frames/funcs/closures 四并行列 + sp 复用指针，取代 `toStr(ip)+"|"+...` 竖线串编码——每次调用省 ~11 次字符串分配（4×toStr + 3×concat + 2×fieldAt substr） | 调用/返回零分配 |
| **D-2 局部槽平铺数组** | 新类 `VmLocals`：slots/marks 平铺列表 + **代际标记**（reset() 仅 gen+1，零分配；get 仅当槽位标记==当前代才返回值，等价空表语义），取代 `HashMap<Int,Any>` 的哈希+桶链；帧池 retire/pop 复用容量 | 访存指令 O(1) 直取，零哈希 |
| **D-3 快速分派块** | dispatch 头部前置热指令臂（LOAD_LOCAL/STORE_LOCAL/LOAD_CONST_I/JUMP_IF_FALSE/LDIC_STL/LDL_JIF/ADD/SUB/LT/CALL/RETURN/JUMP），未命中落入原链 | 高频指令省 ~40 次整数比较 |
| **D-4 装载期超级指令** | `fuseHotPairs()` 对预解码流就地融合：`LOAD_CONST_I;STORE_LOCAL → LDIC_STL`、`LOAD_LOCAL;JUMP_IF_FALSE → LDL_JIF`，**次行 OP_NOP 占位**（⚠ 跳转目标是原始行号，任何压缩都会错位——首版压缩方案实测死循环，NOP 占位方案行号全稳定） | 循环初始化/条件分支各省一次分派 |

### 随带修复：emit 混合类型比较误折叠（值模型家族第 15 项）

`Any`（i8*）与数值的 `==` 被折叠成编译期常量 false——对「持有装箱数值的 Any」
是错的。实测 `VmLocals.getSlot` 的 `marks.get(k) == this.gen` 恒 false → 所有
局部槽恒判未初始化 → AOT 载体死循环（fib(24) 3GB/30s 不终）。
修复：混合比较改走 `ptrtoint + aura_to_int_any` 拆箱后的 i64 比较
（真字符串 vs 数值拆箱后仍恒不等，与旧折叠行为一致）。

### 结果（同机，5 轮中位）

| 基准 | 优化前 | 优化后 | 加速 |
| --- | --- | --- | --- |
| fib(30) 递归 | 18,537 ms | **7,194 ms** | 2.6×（相对最初 104s：**14.4×**） |
| 嵌套循环 25M 次 | 149,783 ms | **98,896 ms** | 1.5× |

内存：fib(30) 3GB 限制内完成 ✓（本轮 VmLocals 平铺数组消除了剩余的
装载期/帧期分配风暴）。

回归：diff-test 6/6、vm_p2 PASS、phase5 PASS、language-test 26/26、
AOT 并发 9/9、隔离守卫双脚本 PASS。

### 调试基建补充

本轮新增两次关键定位均由 §五.1 的三层工具链完成（minidbg + CRASH_DUMP +
RVA↔obj 符号映射）。注意：**shell 工作目录漂移会产生「exe 不存在/空日志」
假象**（本轮多轮假阴性均源于此），长命令务必先回仓库根。

### 六、2026-10-03 终：方案 D 全量落地 —— fib(30) 7.2s → 6.7s（累计 15.5×）、loop 138s → 79s

| 项 | 内容 |
| --- | --- |
| **D-1 调用栈并行列** | `VmCallStack`（ips/frames/funcs/closures 四并列 + sp 复用） |
| **D-2 局部槽平铺数组** | `VmLocals`（slots/marks 平铺 + 代际标记；putSlot/getSlot —— 方法名刻意避开 get/put，emit 对裸名 get/put 有 Map 内建改派会把 VmLocals 指针送进 C mapSet） |
| **D-3 快速分派块** | 热指令臂前置 + 5 条超级指令臂 |
| **D-4 装载期融合扩展** | 2 连（LDIC_STL/LDL_JIF）+ 4 连算术入槽（LDLC_OP_STL：局部 OP 常量 → 槽；LDLB_OP_STL：局部 OP 局部 → 槽，OP ∈ ADD/SUB/MUL）+ 6 连循环守卫（LDLC_LTL_JIFS：局部 < 常量 → 槽 + 条件跳转，含种子编译器「条件物化临时槽」的 6 指令形态识别） |
| **E-2 参数预解析** | arg0Ints/arg1Ints（装载期 toI32 一次；非全数字 → 哨兵回退现场解析），访存/跳转/调用指令零字符串解析 |
| **第 15 项值模型修复** | emit 混合比较（i8* vs 数值）误折叠常量 false → Plan A 拆箱比较（`ptrtoint + aura_to_int_any` 后 icmp）——此缺陷使 VmLocals 的代际标记恒判未初始化（AOT 载体死循环根因） |

**插曲与教训**：一轮失败的 `git checkout` + 多次重叠 python 补丁曾把 Vm.aura
打回旧版（容器/融合/预解析全丢）并残留一处转义损坏（`"' + BS + 'n"`）。
恢复流程：以 `bin.ll` 的方法调用序列验证源文件完整性 → 按会话记录的补丁
清单用文件化 python 脚本重放 → 逐站点 grep 验证。**教训：大文件改用
Write 工具写补丁脚本，勿用超长 heredoc；涉及 `git checkout` 的实验必须
先备份工作树。**

### 结果（同机 5 轮中位）

| 基准 | 优化前 | 上一轮 | 本轮 | 累计 |
| --- | --- | --- | --- | --- |
| fib(30) | 18,537 ms | 7,194 ms | **6,735 ms** | **17.9×**（vs 最初 104s） |
| 嵌套循环 25M | 149,783 ms | 98,896 ms | **78,677 ms** | 1.9× |

回归：diff-test 6/6、vm_p2、phase5、language-test 26/26、AOT 并发 9/9、
隔离守卫双脚本 PASS。

### 七、2026-10-03 终（续）：融合正确性修复 —— 跳转目标守卫 + 5 连参数表

扩展融合（5 连/6 连守卫 + 2 连拷贝）首测 diff 03 红挂。两处根因：
1. **融合窗口吞跳转目标行**：跳到融合对中间行（如条件物化槽的 STORE）
   会命中 OP_NOP → 整条融合被跳过。修复：融合前扫描全流构建
   跳转目标位图（JUMP/JUMP_IF_FALSE/JUMP_IF_TRUE 的目标行），窗口
   [r+1, r+span) 内含目标行则放弃该处融合。
2. **5 连守卫参数表错位**：从 6 连分支复制时多带了 LT 操作码参数
   （5 参数），而 LDLB_LTL_JIFS 臂按 4 参读取（a/b/tmp/j）→ 把临时
   槽号当跳转目标。修复：只推 4 参。

修复后 diff 6/6、全量回归绿。fib(30) 6.6s、loop 74.5s。
