# Photon AOT 自举编译 · 第九轮：内存第二梯队清剿（R143–R145）

> 测量时间：2026-10-20 17:05–17:10　驱动/产物：`build/hat-bootstrap-r145/run1..run2`

## 结论

| 指标 | R93 基线 | 上轮 (R142) | 本轮 (R145) |
|---|---:|---:|---:|
| 峰值私有内存（全局） | 21.78 GB | 5.89 GB | **5.20 GB（−11.7%）** |
| **前端峰值**（RegionReset 前） | — | 6.03 GB | **3.85 GB（−36%）** |
| 总墙钟（2 连测中位） | 739.5 s | 147.6 s | 151.5 s（噪声带内） |
| 结果 | success | success | **success / exit 0 / BOOT-4** |

累计相对 R93 基线：**时间 −79.5%，内存 −76.1%**。`.hat`（md5 `0306b11b`）与
`.obj`（`660b7080`）两轮逐字节一致。**全局峰值点已从前端（t=55s）移到后端
writer 尾部（t=151s）**——前端内存治理阶段性完成。

## 本轮做法：新鲜归因 → tail-first 批量变换

用开关化的归因设施（AURA_ATTR_DUMP=1 时才记录）重跑前端/后端各一次，
拿到修复后的新 Top 榜。前端剩余大项全部是同一个反模式：

**`X = X + a + b + c + d` 多段 concat 链——每一段都把整条 growing 串拷贝一遍。**

「tail-first」变换：先拼出小尾巴 `val t = a + b + c + d`（N−1 次小分配），
再一次 `X = X + t`（1 次大拷贝）——大拷贝次数从 N−1 降到 1，**省 ~75%**。

批量应用 10 处（SsaBuilder）：

| 函数 | 旧体积 | 形态 |
|---|---:|---|
| `varMapAdd` | 761MB | `varMap + name + "\|" + vid + "\n"` 4 段链 |
| `synthesizeCallStubs`（seen/have） | 664MB | 3 段链 × 55KB 串 |
| `addClassMethodReturnType` | ~200MB | 3 段链 |
| `scanOverloads`（stats） | ~330MB | 4 段链 |
| `setVarVersion` | ~190MB | 4 段链 |
| varMap 追加（merge/loop 路径 ×4） | — | 4 段链 |

## 后端归因的新发现（下一轮主线）

后端 5.32GB 的构成（skip-front + 归因）：

1. **`aura_dynlist_push` 1,076,923 次 realloc / 2,958MB（净增 avg 2.9KB）**——
   几何增长的中间数组垃圾（无 GC 下结构性存在），活列表约 490 万个、
   共 ~1.5GB 指针数组（4.9M 次 `dynlist_new`）。要消这个需要**列表预分配容量**
   或**紧凑分配区**，属架构级改动。
2. `scr_alloc` 29.8M 次 / 680MB（avg 23B 小临时物）。
3. `dynlist_new` 4.9M 次 / 281MB（24B+36B 头 + 初始数组）。
4. 已解析的 Aura 级 concat 点：`MirBlockUtils.instrsAdd`（318K/113MB——**注意：
   SsaBuilder 本体从不写 `MirBlock.instrs`，这批来自别处的 `addValueWithChain`
   路径，待查**）、`InstructionSelector.computeReachable`（4202 次 avg 23KB
   = **可达集用 concat 串累积**，92MB）、`X86Emitter.joinPairwise`（75MB）。
5. `writer=49.7s`（比 r142 的 37.7s 高 12s，超噪声带；emit 反而 13.7→7.4s——
   怀疑与 .hat 微小内容变化相关，下轮核实）。

## 工程备注

- `__builtin_return_address(1)` 在本 AOT 下**不可靠**（拿到的是被内联函数自身
  内部地址，甚至 0x80）——Aura 级调用者归因只能走「公开入口 ra0」路线
  （concat/substring/strdup/`listAppend` 类 C 导出函数的 ra0）。
- 归因记录已全部开关化（AURA_ATTR_DUMP），常跑零开销；本轮两个归因跑
  （开开关）各 ~1.5 分钟。

## 产出

- 报告：`docs/photon/photon-aot-round9-memory-r145.md`（本文件副本）
- 数据：`build/hat-bootstrap-r145/run1..run2/`
- 归因原始数据：`build/hat-r143a|b/`、`build/hat-r144/`、`build/attr_front.txt`、`build/attr_back.txt`
