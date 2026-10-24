# Photon AOT 自举编译 · 第七轮：`ssaBuild` 平方项清剿（R104–R130）

> 测量时间：2026-10-20 13:29–13:35　驱动/产物：`build/hat-bootstrap-r130/run1..run2`

## 结论

| 指标 | R93 基线 | 上轮 (R103) | 本轮 (R130) |
|---|---:|---:|---:|
| 总墙钟（2 连测中位） | 739.5 s | 215.1 s | **155.2 s（2m35s）** |
| `ssaBuild` | 101.6 s | 104.0 s | **35–39 s** |
| `buildClassIndex`（内含） | — | ~25 s | **0.87 s（28×）** |
| 峰值私有内存 | 21.78 GB | 8.46 GB | **8.46–8.52 GB** |
| 结果 | success | success | **success / exit 0 / BOOT-4** |

累计相对 R93 基线：**时间 −79.0%，内存 −61.2%**。hello world 冒烟输出 `Hello, World!`；
`.hat`（md5 `739b4504…`）与 `.obj`（`6b20b2bb…`）两轮**逐字节一致**（确定性保持）。

分阶段（run1/run2）：`link 16.4/15.8 · ssaBuild 38.7/35.2 · hatWrite 0.56/0.62`；
后端 `phaseC 12.3 · ra 23.7 · emit 13.0 · writer 39.9/38.2 · backend 96.2/95.4`。

## 定位过程（本轮的方法论样本）

上轮遗留三个热点：`buildClassIndex` 25.1s、`synthSingletons` 42.7s、`emitCall`/`callOrBuiltin`
（已修）。本轮用「**逐层计数器 + 分段打戳**」把 `buildClassIndex` 拆到语句级：

1. **计数器归因（证伪两个假设）**
   - `TypeRegistry.dedupLookup` 线性扫描：实测 260K 次调用 × 平均 **146 项**（表只有 ~150 条）
     ⇒ **dedup 不是瓶颈**，计数器救了一个错误结论（上一轮曾以为它是 40s 的主因）。
   - `registerClass` 的 kid 循环：221 次调用总共只有 **310 次迭代** ⇒ 也不在里面。
2. **逐类计时**（`[rc-in]/[rc-out]` 包住每次调用）：221 个类**均匀地**各花 ~110–330ms，
   与类大小无关 ⇒ 固定开销形态，指向「每调用一次就全表扫一遍」的模式。
3. **函数内分段打戳**（5 段 → 10 段）：热点收敛到两处——
   `classBaseIndex`（≈12–16s）与函数尾部 `flatCount`（7.4s）。
4. **代码审查 + 数量级核对**：`flatCount` 对 26KB 文本逐位滑动、每个位置调
   `flatStartsWith(text, i, key)`，而后者体内有 `pos + key.length > text.length`——
   **`.length` 就是 `strlen`** ⇒ 每位置 2 次全串扫描 ≈ **1.4G 字节/次调用**。
   108 次单例字段判定 + 210 次 `ownFieldCount` ≈ 22s，与实测吻合。

## 根因与修法

**`flatCount` 重写**（循环内零 strlen、零函数调用）：

- 长度（`text.length`/`key.length`）提升到循环外（各 strlen 一次）；
- **首字符过滤**：先比 `key[0]`（即 `\n`），只有命中才比较其余字符；
- 内联逐字符比较，语义与旧实现逐位一致（key 首字符 `\n` 不在 key 其余位置出现，
  可重叠匹配不可能发生，计数不变）。

这一个修复同时消灭了：

- `ownFieldCount`（classBaseIndex 继承链）→ **classIndex 24.6 → 0.87s**；
- 单例尾部字段判定（108 次）→ **synthSingletons 42.7s 基本清零**；
- `classSizeOf/classFieldTotal` 的 `classBaseIndex` 回退路径（getter 合成热点）；
- **ssaBuild 整体 96.9 → 32.4s（−67%）**。

**附带修复：`TypeRegistry` dedup 哈希索引**（本轮早期完成，非热点但属同族预防）：
`dedupLookup` 从「CSV 行线性扫描」改为开放寻址哈希（`dedupKeys/dedupIds/dedupTable`
并行数组 + 容量翻倍重哈希，`dedupAppend` 同步维护），查询 O(1)；CSV 本体照旧生成，
序列化输出不变。曾一度引入 `CsvIndex` 通用类但因 AOT 后端对「同文件后向引用的类 +
内联初始化器」生成坏 IR（`load i32, i32* null`，`FileUtils_fileSize` 假崩溃）而回退；
TypeRegistry 版本用**字段内联声明 + init() 赋值**范式、放在既有类内，工作正常。

## 探针方法备注

- `ClockImpl.timeNs()/1000` 以 **µs** 打印，超过 Int32 上限（~2147s）会回绕为负数，
  长跑解析需按 2^32 展开；两戳相同时差可能小于时钟粒度（~16ms）。
- 分段打戳的区间归属极易错位：`stamp[i] → stamp[i+1]` 的区间应归属**前一个戳**，
  且漏掉中间某个戳的名字会把区间错记给别的段（本轮 r128 的 "B-lists=12s" 实为
  classBaseIndex）。解析脚本务必显式列出全部段名。
- `println` 本身不进入「out 戳 − in 戳」区间（时间戳在参数求值时取），可放心用。

## 剩余热点（下一轮）

1. **writer 38–40s**：`buildSymbols` 里 5.7 万字符串常量逐条 `toStr` + 4 次 concat（已知）；
2. **ra 23.7s**：step0/step2 的着色与溢出决策仍有逐节点扫描；
3. **ssaBuild 剩余 ~35s**：`buildAllFunctions` 主体（3274 函数的指令级降级），属算法常数项；
4. `link 15–16s` 与 `phaseC 12.3s` 已接近合理水平。

## 产出

- 报告：`docs/photon/photon-aot-round7-ssa-flatcount-r130.md`（本文件副本）
- 数据：`build/hat-bootstrap-r130/run1..run2/`（summary.json / timeline.csv / 产物三件套）
- 代码：`SsaBuilder.flatCount` 重写；`TypeRegistry` dedup 哈希索引
