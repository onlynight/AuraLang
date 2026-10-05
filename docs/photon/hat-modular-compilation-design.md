# HAT 模块化编译方案（分包 / 分阶段 / 拆分编译）

> 状态：**设计提案，未实施**。本文档基于对代码库的结构性分析与实测瓶颈数据，不改任何代码。
>
> 核心结论：**单文件 HAT 把"整程序"当成了最小编译单元，导致缓存、并行、按需加载三者全部不可能**。解法对标 LLVM 的四件套：每翻译单元一个 IR 模块 + declare/define 链接模型 + 二进制容器惰性加载 + 内容哈希增量缓存。
>
> 前置讨论：与 LLVM IR 的逐维对比、问题机理分析见本文 §一；落地路线见 §三。

---

## 一、问题机理：为什么"整程序一个 .hat"在超大项目下会异常缓慢

### 1.1 现状事实

自举编译（`aura/compiler/aura/lang/compiler/Main.aura`，111 模块）实测：

| 环节 | 现状 | 证据 |
|---|---|---|
| 模块组织 | `ModuleLink.link()` 在 SSA 之前把 111 模块**合并成单一扁平 HIR arena**，模块边界丢失，靠名称前缀约定避免冲突 | `aura/compiler/aura/lang/compiler/aot/ModuleLink.aura:65`，头注释明确"Emit 只接受一个 HIR，须先合并" |
| IR 文件 | 整程序一个 `.hat`：7.9MB / ~19 万行，无符号表段、无导入导出段、无常量池段（常量内联在指令里） | `HatSerializer.aura:100`，格式见 `hat-format-design.md` |
| 解析 | 全文 `split("\n")` 逐行解析，全量驻留内存；前向引用 `?name` 占位后全文件回填 | `HatParser.aura:118`、`:286`、`:867` |
| 后端 | 五个 Phase 串行单遍，全程序共享一个 MachineDag arena；`compileHat` 仅 3.76s | `PhotonPipeline.aura:561-592` |
| 真瓶颈 | 前端 SSA 占 93.9%：`synthesizeCctors` 71.7s + `synthesizeSingletons` 59.9s + `buildClassIndex` 19.9s；总 3m15s、峰值内存 ~5.4GB | `photon-bootstrap-perf-analysis.md` |

### 1.2 与 LLVM IR 的逐维对照

| 维度 | LLVM IR | HAT 现状 | 超大项目下的后果 |
|---|---|---|---|
| 编译单元 | 每翻译单元一个 `.ll/.bc`，独立编译 | 整程序一个文件（源头是 HIR 就已合并） | 无并行单元、无缓存粒度 |
| 链接模型 | `declare`/`define` + linkage 类型（external/internal/weak），每模块符号表 | 无导出/导入概念，单一全局命名空间 | 分模块编译没有符号解析依据 |
| 容器格式 | bitcode 二进制，块结构 + 函数偏移索引，**惰性加载** | 纯文本逐行，全量加载 | 1000 模块级 ~80MB，内存与解析时间线性膨胀且无法按需取 |
| 增量 | `.bc`/ThinLTO cache 按内容哈希缓存 | 无任何缓存 | 改一行 = 全量 3 分钟+，日常开发不可用 |
| 全程序优化 | ThinLTO：每模块摘要 + 链接期分布式 | `computeReachable` 全程序 DCE 依赖内存中单一 DAG | 语义上绑死"必须整程序在内存里" |
| 常量 | 全局常量段 + 串表，跨模块去重 | 常量内联指令，指令选择时才集中收集 | 字符串随模块数近似平方级膨胀（收集点 `InstructionSelection.aura:130`） |

### 1.3 一个重要的诚实校正

当前 profile 里 **`compileHat` 只占 2.4%**，慢的主因在前端三个串行 pass。但单文件 HAT 模型正是它们无法拆分的原因——三个 pass 都跑在"合并后的整程序 HIR/SSA"上。所以本方案必须覆盖整条链，不能只改 `.hat` 文件本身。

---

## 二、方案总览

对标 LLVM 的成熟结构，按依赖关系分四步，每步独立可交付：

```
现状:  N 源文件 ──ModuleLink合并──> 单一HIR ──> 单一SSA ──> 一个.hat ──> 单进程后端 ──> 1个obj ──> lld-link

目标:  N 源文件 ──per-module──> N个.hat（带exports/imports/常量池/类型表段）
                          │            │
                          │            ├──(缓存命中则跳过)──> 复用 .hat/.obj
                          │            v
                          │      N 个并行worker: .hat ──> N 个 .obj
                          v
                     链接期: lld-link N个.obj + aura_runtime.obj + 链接期可达性分析
```

### 方案 A：HAT 分包——一模块一个 .hat（对标 LLVM 每翻译单元一个 .bc）

**格式升级为 HAT v3.0**，函数体语法不动（SSA + Phi 已与 LLVM 同构，这是最不需要改的部分），只加"段"：

1. **imports 段**：外部函数声明，对标 LLVM `declare`。形如 `; declare @fn aura.lang.compiler.hir.Hir.textOf(...)`。
2. **exports 段**：本模块定义的可外部引用符号，对标 `define` + 默认 external linkage。
3. **linkage 修饰**：`@fn public` / `@fn internal`。internal 函数可在模块内被 DCE，对标 LLVM internal——这是把全程序 `computeReachable` 拆成"模块内 + 链接期"的前提。
4. **常量池独立成段**：字符串/大常量集中管理，段内按内容哈希去重，指令引用 `@str#<hash>`。直接解决超大项目字符串膨胀（现在字符串到指令选择阶段才收集，`InstructionSelection.aura:130`）。
5. **类型表独立成段**：类型按结构哈希排规范序，跨模块类型合并退化为哈希合并，不需要中心化 TypeRegistry。

**编译模型改造**：`ModuleLink` 不再合并 HIR，而是每模块独立走 parse → HIR(module) → SsaBuilder(module) → `module.hat`。跨模块调用的解析依据（现在的只读表 `methodOwners`/`methodRetTypes`/`freeFuncs`）改为从依赖模块的 exports 段装载——`photon-multithread-design.md` §2.2 已确认这些表在 lowering 期间只读，天然支持这个改法。

### 方案 B：分阶段——四个可缓存、可并行的独立阶段（对标 `clang -c` 的编译/链接分离）

| 阶段 | 内容 | 粒度 | 并行/缓存 |
|---|---|---|---|
| Stage 1 | 源码 → `module.hat`（含 cc ctor/singleton 合成、类索引） | 每模块 | 进程级并行 + 内容哈希缓存 |
| Stage 2 | `module.hat` → `module.obj`（Lowering → ISel → RA → Emitter） | 每模块 | 同上。四个子阶段内部本就 per-function，唯一的全程序耦合是 `computeReachable`（改为模块内 DCE + 链接期剔除，见 §四.1）和字符串池（方案 A 第 4 条解决） |
| Stage 3 | 链接 | 全程序 | `lld-link` 已是外部进程，只需从链接 2 个 obj 改为 N 个 obj——现成能力，几乎零成本 |
| Stage 4（远期可选） | 跨模块优化 | — | 需要时再做 IR 级 HatLink 合并或 ThinLTO 式摘要，不阻塞前三步 |

**增量缓存设计**（对标 ThinLTO cache，日常开发收益最大的一项）：

- 依赖图以 `aura.lock` 为基础（`Package.aura` 已有 SemVer/lock 机制，不用新建）。
- 模块指纹 = hash(源文件内容 + **依赖模块的接口指纹** + 编译器自身版本哈希)。接口指纹只含 exports 段 + 类型表 + 常量池哈希，**不含函数体**——改实现不改接口时，依赖者不重编。
- 自举编译器频繁变版，编译器版本必须进缓存键，否则会出现新旧 IR 混链（现有 `aura_runtime.obj` 的 `.syms` sidecar 版本校验已是同一问题的先例，见 `photon-bootstrap-link-benchmark.md`）。
- 效果：改一行 → 重编 1 个模块的 .hat + .obj + 链接，**从 3m15s 降到秒级**。

### 方案 C：二进制容器 + 惰性加载（对标 bitcode；文本降级为 dump 格式）

- 新容器直接**复用 AUC 的段机制**——AUC 已具备魔数/版本头/uuid + consts/natives/funcs/exports/imports/deps 段（`AucSerializer.aura:8-54`），是最现成的骨架，避免发明第二套二进制格式（也顺带统一 VM 链路与 Photon 链路的模块容器）。
- 关键新增：**函数偏移索引表**，使链接期/HatLink 只读符号表 + 索引即可做符号解析与可达性分析，按需才加载函数体——对标 bitcode 的惰性 materialize。
- 文本 `.hat` 保留为调试/dump 输出，对标 `llvm-dis`。

### 方案 D：并行执行框架（进程级优先，不上线程）

- **用进程而非线程**：`PhotonHatCompile.aura` 已是环境变量驱动的独立 worker（读 `AURA_HAT_AURA`/`AURA_HAT_OUT`/`AURA_HAT_MODULE`），spawn N 个几乎零改造；而线程方案会撞上 `photon-multithread-design.md` §3.2 记录的全部八个坑（自旋锁无退避、HashMap 构造即崩、`Atomic.load/store` 非原子、无 GC 导致 8 并行内存膨胀到 30GB+ 等）。
- 前提是方案 A 拆掉合并-HIR——否则每个 worker 都会重复编译全部 111 模块（`photon-multithread-design.md` §2.3 已指出这一点）。
- 并行同时吃掉当前真瓶颈：前端三个串行 pass（占 95.3%）在分包后变成 per-module 工作，天然可并行；顺带修掉该设计稿阶段 1 列的两个 O(n²) 病灶（`freeFuncs` 字符串拼接 `SsaBuilder.aura:2765`、`localTypeOf` 全量 split `:2583-2590`）。

---

## 三、落地顺序（依赖关系决定，每步独立验收）

| 优先级 | 内容 | 依赖 | 预期收益 |
|---|---|---|---|
| P0 | HAT v3.0 格式加段：imports/exports/linkage + 常量池 + 类型表。向后兼容：无段的老文件 ≡ "单模块无导入导出" | 无 | 为后续所有步骤铺路；本身即消除字符串/类型膨胀 |
| P1 | 方案 A：一模块一 .hat，`ModuleLink` 改 per-module HIR/SSA，跨模块符号从 exports 装载 | P0 | 模块边界恢复；前端三个大 pass 变 per-module |
| P2 | 方案 B+D：per-module `.obj` + N 进程并行 + 内容哈希缓存 + lld-link 链 N 个 obj | P1 | 全量构建 ~8×（受最大模块限制）；**增量构建秒级**；峰值内存从 5.4GB 降到"单模块 × 并行度" |
| P3 | 方案 C：二进制容器 + 函数偏移索引 + 惰性加载（与 AUC 统一） | P1（与 P2 无依赖冲突） | 链接期不再全量加载 IR；超大项目内存可控 |
| P4（远期） | 跨模块优化 / ThinLTO 式摘要索引 | P3 | 全程序优化不再需要整程序在内存 |

**验收口径**：沿用 `AURA_PHOTON_TIME=1` 分段输出；全量构建以自举 `Main.aura` 总时长为准，增量构建以"改单文件后重编"为准（目标秒级）。

---

## 四、关键风险与决策点

1. **全程序 DCE 语义变化**：`computeReachable` 目前依赖内存中单一 DAG。拆分后 internal 函数模块内剔除，跨模块死代码交链接期（lld-link `/REF` 或链接期二次 reachability），入口根集合（main + 全部运行时符号）必须在链接期显式传入——`photon-bootstrap-link-benchmark.md` 记录过 DCE 与发射名字映射不一致误删定义的事故，这里要重点防回归。
2. **跨模块静态初始化顺序**：`synthesizeCctors`/`synthesizeSingletons` 现在跑在整程序 HIR 上。分包后每模块给自己的类合成构造器，若存在跨模块静态字段初始化依赖，需要链接期排序机制（对标 LLVM `llvm.global_ctors` + 优先级）。这是 P1 最需要先验证的语义问题。
3. **名称修饰正式化**：模块边界恢复后，前缀约定必须升级为确定性 mangling 规则（或哈希后缀）并写进 exports/imports 段，否则链接期符号冲突。
4. **内存模型**：无 GC、AOT 分配即泄漏的现状下，进程级并行天然隔离，比线程方案安全——这是选进程不选线程的核心理由。
5. **两套格式统一**：容器设计应与 AUC 对齐（AUC 本就是按模块的二进制格式），避免 VM 链路和 Photon 链路各自维护一套模块容器。
6. **与多线程设计稿的关系**：`photon-multithread-design.md` 的"模块级并行不可行"结论是在合并-HIR 模型下成立的；本方案 P1 恰好拆掉该前提，结论不冲突，但该文档的"不要做"清单（不给 `addValue` 加锁、不引 `HashMap`、共享状态不挂 `object` 单例等）在本方案实施中仍然全部有效。

---

## 五、一句话总结

**HAT 单文件慢的本质是它把"整程序"当成了最小编译单元，而 LLVM 用"每模块一个 IR 文件 + declare/define 链接模型 + bitcode 惰性加载 + 内容哈希缓存"四件套解决了同一问题。** 按 P0（格式加段）→ P1（一模块一 .hat）→ P2（分阶段 + 进程并行 + 增量缓存）→ P3（二进制容器）推进，每步独立可交付，且 P2 直接把日常开发的构建时间从分钟级降到秒级。
