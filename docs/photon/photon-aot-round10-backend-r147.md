# Photon AOT 自举编译 · 第十轮：后端 concat 清剿 + 提交（R146–R147）

> 测量时间：2026-10-20 17:21–17:26　驱动/产物：`build/hat-bootstrap-r147/run1..run2`

## 结论

| 指标 | R93 基线 | 上轮 (R145) | 本轮 (R147) |
|---|---:|---:|---:|
| 总墙钟（2 连测中位） | 739.5 s | 151.5 s | **131.3 s（2m11s，−13%）** |
| 后端合计 | 459.7 s | 96–97 s | **77.4 / 84.3 s** |
| 峰值私有内存 | 21.78 GB | 5.20 GB | **5.05–5.16 GB** |
| 结果 | success | success | **success / exit 0 / BOOT-4** |

累计相对 R93 基线：**时间 −82.2%，内存 −76.8%**。`.hat`（md5 `263a3bbd`）与
`.obj`（`54b33c02`）两轮逐字节一致。writer 恢复 35.1/41.7s（**证实 r145 的
49.7s 是 .hat 内容方差而非回归**——emit/writer 存在此消彼长的内容敏感性）。

## 代码提交

`8299917` perf(photon-hat): R90-R145（57 文件，+3717/−459）——涵盖修复/性能/内存
三轮全部改动 + 10 篇轮次报告 + 测量/重构工具。

## 本轮修的（后端 concat 三处）

1. **`SsaMir.instrsAdd`/`succsAdd`**：`list + "," + vid` 两次整串拷贝 → tail-first
   （31.8 万次 / 113MB 滞留，省一半）。
2. **`InstructionSelection.computeReachable` 结果拼接**：`out = out + "," + nm`
   O(k²) 链（2101 次 × avg 23KB ≈ 92MB）→ 两两归并 O(R·log k)。
3. 移除 dynlist_push 的 ra2 诊断残留（`__builtin_return_address(1)` 不可靠已证）。

## 后端剩余构成（已到收益边界）

- **dynlist 增长 ~3GB**：活列表 ~1.5GB 指针数组（490 万个），堆模式下 realloc
  的旧块由 C 堆管理（非垃圾）——live 数据本体，消除需预分配/紧凑区（架构级）。
- concat/substring 临时物 ~1.2GB（无 GC 滞留）。
- 活结构（MIR/LIR/DAG/HIR 重建）~1GB。

5.05GB 峰值 ≈ 上述之和，已接近「无 GC + 值语义字符串」架构下的合理水位；
再往下需要架构级手段（后端区域化 / 字符串Arena / 列表预分配）。

## 产出

- 报告：`docs/photon/photon-aot-round10-backend-r147.md`（本文件副本）
- 数据：`build/hat-bootstrap-r147/run1..run2/`
