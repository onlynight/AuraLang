# AUC 按需动态加载方案（诊断 + 详细设计）

> 状态：方案设计（基于 2026-10-11 代码事实，未实施）
> 关联文档：
> - `docs/系统-多文件编译打包方案设计.md`（下称"多文件方案"，本方案是其落地实施版）
> - `docs/photon/hat-modular-compilation-design.md`（下称"HAT 方案"，photon AOT 侧镜像问题）
> - `docs/系统-库导出与包格式设计方案.md`（.auz 容器）

---

## 一、诊断：.auc"偏离初衷"的准确位置

### 1.1 结论先行

**.auc 格式本身（v7）就是按"多模块动态加载"设计的，但编译模型停在了"源码文本内联 → 单模块"，格式里为动态加载准备的机制全部落空。** 问题不在格式，在编译管线与发射器。

### 1.2 格式层：动态加载的骨架已经建好（初衷证据）

| 机制 | 位置 | 状态 |
|---|---|---|
| 模块身份（name/version/UUID） | `serialize.rs:75-77`，`AucSerializer.aura:12` | ✅ 格式支持 |
| 导出表 exports（func_idx/sig_id） | `serialize.rs:126-147` | ✅ 格式支持 |
| 导入表 imports（module+symbol） | `serialize.rs:150-162` | ✅ 格式支持 |
| 显式依赖表 deps（module+uuid+version） | `serialize.rs:165-170` | ✅ 格式支持 |
| 跨模块调用指令 `CALL_EXTERNAL(mod_idx, sym_idx)` | opcode 71 | ✅ seed VM `interp.rs:897-921` 已实现执行语义 |
| 模块注册表 `ModuleRegistry`（UUID/名字索引/导出查找） | `vm/mod.rs:110-157` | ✅ 已实现 |
| 签名系统 sig_ids + Ed25519 签名 | `serialize.rs:1078-1223` | ✅ 已实现（库分发故事的一部分） |
| 包格式 .auz（checksum/verify） | `auz/builder.rs`、`auz/reader.rs` | ⚠️ 已实现但**只能装一个模块**（`PackageBuilder::new(manifest, module)` 单 module） |
| `entry_kind = "library"`（库入口语义） | `opcode.rs:1073` | ✅ 格式支持 |

### 1.3 编译模型层：三处"合并点"把一切打回单模块

1. **seed 编译器**：`main.rs:272-276` — `resolve_aura_imports(source)` 把所有 import 的 `.aura` **源码文本内联**成一个巨型字符串（同包自动内联 `mod.rs:438-465`、`Any` 无条件内联 `mod.rs:371-397`）→ `compile_source(单一字符串)` 产出一个 `BytecodeModule` → `write_auc` 输出一个 `.auc`。
2. **自举编译器（Aura 侧）**：`aot/ModuleLink.aura:65,252` — 同一模型，在 SSA 之前把 111 个模块合并成单一扁平 HIR arena（HAT 方案 §1.1 实测：全量自举 3m15s、峰值 5.4GB）。
3. **seed VM 的 stdlib 加载**：`vm/mod.rs:1195-1196` — 即使 stdlib 已是"每文件一个 .auc"（`stdlib-compile` 目录镜像 + `include_bytes!` 嵌入），VM 加载时仍把 stdlib 函数 **flat merge 进主模块 `funcs`**，调用不走 registry。

### 1.4 空壳清单：格式机制在用户构建中的实际状态

| 机制 | 用户构建中的实际状态 | 根因 |
|---|---|---|
| imports 表 | **恒空**（`emit.rs:423` `imports: Vec::new()`） | 发射器从不生成跨模块引用 |
| `CALL_EXTERNAL` | 从未被发射（`emit.rs` grep 无结果） | 同上 |
| `CallModule`（多文件方案 §3.6.2） | 连 opcode 都不存在于 `opcode.rs` | 方案未实施 |
| deps / sig_ids | 恒空 | 无模块边界，无依赖图 |
| exports 表 | 仅 `--cabi/--shared` 有意义 | 全部符号已在本模块内 |
| registry 注册 | 仅注册静态合并的 stdlib 模块 | "有路无车"：interp 的 CallExternal 通路无人触发 |

### 1.5 代价（为什么必须改）

- **增量编译不可能**：改一行 = 全量重编（自举 3m15s）。
- **按需加载不可能**：没有模块边界，Aura VM 启动即全量解码单个 2MB `.auc`（约 20 万指令，见 `AucLoader.aura:69` 注释）。
- **分发粒度错误**：库更新 = 重发整个程序；`.auz` 无法承载"多模块库"。
- **自相矛盾**：stdlib 已经是"每文件一个 .auc"的正确形态，用户程序反而是整块合并。

---

## 二、目标模型

对标 JVM（classpath + ClassLoader）与 LLVM（翻译单元 + 链接器）：

```text
现状:  N 个 .aura ──resolve_aura_imports 文本内联──> 1 个源 ──> 1 个 BytecodeModule ──> 1 个 .auc ──> VM 全量解码

目标:  N 个 .aura ──编译+链接切分──> N 个 .auc（exports/imports 实装）
                                        │
                                        ▼
                          index.json（模块索引：名字/路径/exports/imports/deps）
                                        │
                                        ▼
                          ModuleRegistry 按需加载 + 加载期预绑定
```

五条原则：

1. **语义分析保持全程序**。sema/typecheck 继续跑在合并文本上（现状质量基线不动），只有发射后的产物按模块切分——这是改动量最小、且不牺牲类型安全的路线。
2. **emit.rs 主流程不动**，新增独立"链接切分 pass"（对标 `clang -c` / `lld-link` 的编译/链接分离）。
3. **.auc 格式 v7 零改动**。所需段全部已存在，不升版本。
4. **两个 VM 对称改造**：seed Rust VM 先行（迭代快），自举 Aura VM 跟进（复用已证原语）。
5. **合并模式永久保留为回退路径**（`--single`），也是 photon 已知解码脆弱性（AucLoader 内大量 P-编号缺陷）的兜底。

---

## 三、关键设计决策

### D1 模块单元 = 一个 .aura 源文件

模块名 = 包路径（`aura.lang.std.String`）。与 `stdlib-compile` 现状、多文件方案 D4 一致。

### D2 跨模块调用 = 复用 `CALL_EXTERNAL(mod_idx, sym_idx)`，**不新增 opcode**

- `mod_idx` 是本模块 imports 表下标，运行期经 registry 按 `module + symbol` 名字解析——语义上已经是"运行期解析"，等价于多文件方案里的 `CallExternal`，还免去了 `CallModule` 的"编译期模块树解析"。
- 新增 opcode 的代价被严重低估：`aucToVmOp`/`operandSizeLookup`/`opcodeName` 三张手写表 × 2 个 VM，且 photon 侧这些表正是历史缺陷密集区（多个 P-编号 bug）。
- **裁掉多文件方案的 `CallModule` 指令提案。**

### D3 模块内调用保持 `CALL(函数下标)`；切分时下标重映射

跳转指令操作数是**函数内字节偏移**，函数体字节不动即不受函数表重排影响；只有 `CALL/CALL_EXPORT/MAKE_CLOSURE/CALL_AOT` 的函数下标需要链接期重写。宽度表 `operandSizeLookup` 可直接复用做指令流扫描。

### D4 类系统 = "类型全局、代码模块"

类 ID 是模块内编译期分配的（`emit.rs:246-252` 每编译单元递增），拆分后跨模块类引用（`NEW_OBJECT`/`INSTANCE_OF`/`CHECK_CAST`/`PUSH_HANDLER` 的 catchType）会悬空。方案：

- **链接 pass** 把本模块引用到的外部类**追加到本模块 classes 表尾部**（全限定名，保证 `.auc` 文件内部类 ID 不悬空）。
- **加载期**把各模块类表**按全限定名去重合并**为全局类表，全局重分配类 ID，并 fixup 上述指令的类 ID 操作数（seed 侧解码后改 `Instr` 操作数；Aura 侧 `decodeNumeric` 本就经 `classNames` 表把类 ID 解析为类名 `classNameAt`，只需让它输出全限定名——天然适配）。
- 类名必须全限定唯一（`package.Class`）；加载期名字冲突即报错（registry 已有 name conflict 先例）。

### D5 vtable 加载期合并

vtables 的 `type_tag` 按全局类 ID 重映射后合并进全局 vtable；`CALL_METHOD` 槽位操作数相应做加载期重映射（Aura VM 侧 loader 已把槽位解析为方法名 `slotMethodNames`，per-module 解码天然安全；seed 侧需查证 `interp` 的槽位消费方式——见 R4）。全局槽位表按"逐模块追加"构建。

### D6 全局变量 = 模块内槽位 + 加载期基址 fixup（待 spike 确认）

`LOAD_GLOBAL/STORE_GLOBAL`(110/111) 的 u16 槽位若为编译期内分配，加载期按模块基址偏移重映射；若现状已全局分配则免改。**Phase 1 第一项 spike 查证**（见 R4）。

### D7 加载策略两阶段：先 eager 保正确，再 lazy 提速

- **M-eager**：入口模块加载 → 读 imports 闭包 → 拓扑序全量加载注册 → **加载期预绑定**：遍历 imports 表把 `(mod_idx, sym_idx)` 预解析为 `(target_module, target_func_idx)` 存绑定表，`CALL_EXTERNAL` 执行时 O(1) 查表（顺带修掉 seed interp 现状"每次执行都哈希查找"的性能问题）。
- **M-lazy**（Phase 2）：首次 `CALL_EXTERNAL` 绑定未命中时触发 `load_module`，加载中标记 + 环检测。

### D8 产物布局与索引

```text
build/
└── lib/
    ├── index.json              # 模块索引
    ├── main.auc
    └── aura/lang/std/String.auc   # 目录镜像（同 stdlib-compile 约定）
```

```json
{
  "version": 1,
  "entry": "main",
  "modules": [
    { "name": "main", "path": "main.auc", "imports": ["aura.lang.std.String"], "hasEntry": true },
    { "name": "aura.lang.std.String", "path": "aura/lang/std/String.auc", "imports": [], "hasEntry": false }
  ]
}
```

`.auz` 扩展为多模块容器：`lib/*.auc + index.json + ref/`，现有 checksum/verify 不变。

---

## 四、分阶段实施计划

### Phase 0：基线护栏（0.5 天）

- 固化现网全链路测试基线：helloworld / fib / classes 等示例 ×（seed VM / `aura run --vm=aura` / photon build）。
- 记录当前单 `.auc` 的字节数/函数数/类数作为对照物。

### Phase 1：seed 链路切分——"多 .auc 产出 + eager 加载，行为等价"（1.5-2 周）

| # | 任务 | 改动点 |
|---|---|---|
| 1.1 | spike：查证 `LOAD_GLOBAL/STORE_GLOBAL` 槽位分配现状与 seed interp 的 `CALL_METHOD` 槽位消费方式（决定 D5/D6 细节） | 读 `emit.rs` / `interp.rs` |
| 1.2 | `resolve_aura_imports` 在每个内联文件前后插哨兵注释 `// aura:module-begin <pkg.path> <file>`——词法/语法/sema **零改动** | `codegen/mod.rs` |
| 1.3 | 新增链接切分器：按哨兵把 functions/classes/consts 归属模块；`CALL` 下标重映射；跨模块 `CALL` → `CALL_EXTERNAL` + imports/exports 表填充；外部类追加到本模块类表 | 新增 `codegen/link.rs` |
| 1.4 | `build` 输出多 `.auc` + `index.json`；`--single` 走旧逻辑 | `main.rs` |
| 1.5 | VM 多模块装配：读 index → 拓扑序 `read_auc` → `registry.register` → 类表/vtable/全局槽合并 fixup → imports 预绑定表 | `vm/mod.rs`、`interp.rs` |

**验收**：现有全部示例/测试在分包模式与单文件模式下输出逐字节一致；跨模块调用（`CALL_EXTERNAL` 首次真正被执行）专项测试通过。

### Phase 2：按需 lazy 加载 + .auz 多模块（1 周）

| # | 任务 |
|---|---|
| 2.1 | `ModuleRegistry.load_module(path)`：魔数/版本/UUID 校验 → 注册；加载中标记 + 循环依赖报错 |
| 2.2 | lazy 触发：`CALL_EXTERNAL` 绑定未命中 → 按模块名加载 → 绑定写回 |
| 2.3 | `auz/builder` 多模块打包（lib/*.auc + index.json + ref/）；reader 侧 index 读取 |
| 2.4 | 验收：懒加载只触达依赖子集的行为测试；`.auz` 打包→verify→运行闭环 |

### Phase 3：自举编译器 + Aura VM 对齐（2-3 周，风险最高）

| # | 任务 |
|---|---|
| 3.1 | 自举编译器 build 流程按模块输出 `.auc`（复用 stdlib-compile 目录镜像约定）；`ModuleLink` 拆分与 HAT 方案 P1 同构，**共用一套模块边界定义** |
| 3.2 | `AucLoader` 新增多模块装配：读 index/目录 → 依赖序逐模块 `loadModule` → 全局类表合并（`classNameAt` 输出全限定名） |
| 3.3 | `Vm.aura`/`VmRunner`：`CALL_EXTERNAL` 分派经模块注册表；绑定缓存表全部用已证原语实现（参照 P266 全局槽中转模式、readStrFrom 递归等） |
| 3.4 | photon 链路对齐 HAT 方案 P0-P3（容器统一：HAT 分包产物与 AUC 共享模块边界） |
| 3.5 | 验收：`aura run --vm=aura` 走多模块；终极形态 = 自举编译器自身以 111 模块 → 111 个 `.auc` 构建 |

**Phase 3 纪律**：每步用 `auc_diff_runner.aura` 对照 seed 输出；任何一步不过即回退合并模式，不带病推进。

### Phase 4：增量编译（1 周）

- 模块指纹 = hash(源文件内容 + 依赖模块**接口指纹**[exports+类表哈希，不含函数体] + 编译器版本)——改实现不改接口时依赖者不重编（对标 ThinLTO cache，HAT 方案 §方案B）。
- `aura build --watch / --clean / --no-cache`。
- 验收：改单文件后重编 < 5s（对照现状全量 3m15s）。

---

## 五、风险与对策

| # | 风险 | 对策 |
|---|---|---|
| R1 | photon 自举 AOT 解码脆弱（AucLoader 大量 P-编号缺陷：phi 环、Long↔Int 截断、循环推进等） | Phase 3 只用已证原语；每步 diff runner 对照 seed；合并模式随时回退 |
| R2 | 跨模块静态初始化顺序（object 单例 / 静态字段） | deps 拓扑序执行 `__singletonInit`（对标 LLVM `llvm.global_ctors`）；与 HAT 方案风险 2 共用一套排序 |
| R3 | 类名冲突 | 全限定名强制 + 加载期冲突报错 |
| R4 | 全局槽位/`CALL_METHOD` 槽位的模块内分配语义未查证 | Phase 1 第一项 spike；若是模块内分配则加载期基址 fixup，若已全局则免改 |
| R5 | `CALL_EXTERNAL` 性能劣于直接 `CALL` | 加载期预绑定表（D7），运行期 O(1)；热路径可再加调用点缓存 |
| R6 | 循环依赖 | 编译期拓扑排序检测环报错；运行期 lazy 加载环检测（加载中标记） |
| R7 | 双实现漂移（seed Rust ↔ 自举 Aura） | index.json schema 定为唯一契约并写入 docs；两侧测试共用 examples 套件 |

---

## 六、与现有文档的关系

- **本方案** = 多文件方案的落地实施版。采纳其 §3.6（跨模块调用）/ §3.9（ModuleRegistry）/ §3.5（编译流水线）骨架；针对本仓库 photon 现实新增 D2（裁掉 `CallModule`，复用 `CALL_EXTERNAL`）、D4/D5（类与 vtable 的"链接期追加 + 加载期合并 fixup"）、D7（eager→lazy 两阶段）；编译策略改为"sema 全程序 + 发射后切分"（多文件方案的逐模块独立编译留给 HAT 链）。
- **HAT 方案** = photon AOT 侧同一问题的镜像方案。其 P3（与 AUC 统一容器）与本方案 Phase 3.4 互为依赖；模块边界定义必须一致（同一份哨兵/模块名规则）。
