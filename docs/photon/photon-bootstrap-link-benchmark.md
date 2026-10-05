# Photon 自举链接修复 + 三模式基准记录（2026-10-02）

> 关联：`docs/vm_pure_aura/00-VM纯Aura化技术方案与达成路径.md`（P4 记录）、`docs/photon/HANDOFF.md`

## 一、Photon 编译 Aura 自举（Main.aura，111 模块）——链接失败的修复

链路：`源码 → 合并 HIR（155,887 节点）→ SSA MIR → .hat（7.9MB）→ LIR → DAG → X86 → COFF（2.17MB）→ lld-link`。
前端+后端全通（3m15s，峰值 WS ≈5.4GB）；链接失败根因与修复：

| # | 根因 | 修复 | 效果 |
|---|---|---|---|
| 1 | **可达性与发射名字映射不一致**：`computeReachable` 收集的是 ISA 映射**前**的裸被调名（`textOf`/`keys`/`getSize`…），而 `emitCall` 经 `mapStdlibFuncName` 按接收者类型映射后发射（`Ast_textOf`/`HashMap_keys`/`ArrayList_getSize`…）→ 定义被判不可达 → 死代码消除剪掉 → `undefined symbol`（首轮 20 个中 10 个属此类） | `InstructionSelection.computeReachable` 收集 Call 时**同步入队映射后的名字**（同 emitCall 的映射函数与 recvTy 推导） | 10 个 Ast/Hir 工具符号 + `HashMap_keys` 全部解决 |
| 2 | **runtime obj 文件存在性缓存**（2026-09-29 引入）：`RUNTIME_SYMBOLS` 变更后旧 `aura_runtime.obj` 被继续链接 → 新增 runtime 符号全部 undefined | `PhotonPipeline` 两处构建点加 **sidecar 版本校验**（`aura_runtime.obj.syms` 记录构建时符号表，不一致即重建）；手工删除一次陈旧 obj | `String_fromCharCode`/`ProcessOps_aura_process_argCount/args` 三个新 runtime 符号生效 |
| 3 | `__list_remove_at` 在 AOT 链路无实现（HIR 重路由存在、C 运行时与 emit 降级缺失） | ① `aura_std_cffi.c` 新增 `aura_lang_std_Collections_listRemoveAt`；② `emit.rs` 新增 `__list_remove_at` 降级分支（接收者类实例指针取 0 号字段；下标经 `coerce_int_width` 拆箱——直接 zext 会产生 `ptr`/`i64` 类型失配）；③ `aot/runtime.rs` 登记运行时函数声明 | 驱动自身 AOT 构建恢复可用 |

**修复后状态**：未定义符号 20 → **14**（全部为同一类：**多候选歧义裸名**）。

**选项 2 实施（接收者类型传播）的关键发现与修复**：

1. **前端接收者类型传播已实现**（Hir.aura）：lowerTypeDecl/lowerValDecl/lowerVarDecl/
   lowerFunction 登记类方法表（typeMethods）、类字段表（typeMemberTys）、局部表
   （localVarTys）+ cleanTy 修饰符剥离（解析器把 `#public|` 前缀写进 ty 列，类类型
   只剩 `#public|` 空——不剥离一切类型逻辑失效）+ 构造器调用类型推断 + lowerCall
   接收者类型限定（Ident/This/一级 Member 三形态）。
2. **曾有 5 处「每模块清空 typeMethods」的 bug**（lower()/lowerTopLevel 的预登记
   遍历各带清空）——模块间 import 有先后序，后降级模块清空先降级模块的登记。
   已改为累进式（localVarTys 仍按函数清空）。
3. **AOT 参数/成员链路修复**：参数类型兜底 i32→i8*（三处）；`emit_member_access`
   对 List/Map 接口接收者的 size/length/isEmpty 路径（曾实装后回滚——与模糊字段
   扫描缺陷叠加会段错误，见专项说明）。

**当前精确状态**（薄宿主 `auc_vm_runner.aura` 实测）：`vm.loadAucAndRun(...)` 的
接收者类型解析成功（`localTy=[VmRunner]` ✓），但 typeMethods 的**预登记在 AOT
宿主下执行会段错误**（HirLowerer 的 this 指针在跨方法调用后损坏——与
auc_vm_runner 静默失败同族），故限定校验 `known=false`，仍发裸名。最小复现
（单文件单类单方法）在同驱动下**端到端成功**（`x=1` 正确输出）——证明限定机制
本身正确，段错误是 AOT 宿主对「多模块 + 大类体」编译产物的值模型缺陷。

**结论**：剩余 14 符号 + Aura VM 原生载体，统一阻塞于 **AOT 后端局部指针截断**
单一根因。前端机制全部就绪，AOT 专项（emit.rs 局部槽类型保持）完成后自动收口。

**AOT 值模型专项技术方案**（2026-10-02 修订）：

emit.rs 的核心缺陷是 `resolve_member_field_owner` 的**模糊字段扫描**——当接收者
类型是 List/接口（HirType::Named("List<String>") 等）时，emit 成员访问被误降为
**不相关类的字段读取**（bitcast → VmFrameStack → 读 sp/cap 偏移）。

修复需分三步：

1. **模糊扫描排除方法名**（已实施）：`resolve_member_field_owner` 的兜底扫描
   不再匹配 size/length/isEmpty/contains 等方法名（METHOD_ONLY_NAMES 排除表），
   防止误配到无关类的同名字段。已验证不引发段错误。

2. **成员访问内建路由**（需实施）：emit_member_access 在 owner=None 时，对
   `size`/`length`/`isEmpty` 路由到 `Collections.count(i8* obj_ir)`——obj_ir
   是 List 句柄（AuraDynList*），count 读 `l->len` 即得正确值。此修复曾在
   前一轮实施并验证 IR 正确（Collections.count 出现在 .ll 中），但因与
   Vm.aura 优化改动叠加导致段错误而被回滚。单独实施（Vm.aura 保持 HEAD）
   需重新验证。

3. **局部槽类型保持**（需实施）：`val mod: AucModule = loadModule(...)` 的
   局部槽 alloca 类型必须为 `%struct.AucModule*`（当前可能因类型解析失败
   落入 i32 兜底导致指针截断）。修复方法：emit_function 的局部声明使用
   `llvm_type_checked`（而非 `llvm_type`），确保未知 struct 类型退化为
   i8* 而非 i32。

## 二、三模式基准（2026-10-02，同机，5 轮取中位）

基准源：`build/bench/bench_fib.aura`（fib 递归）、`bench_loop.aura`（5000×5000 嵌套循环）；
脚本：`scripts/bench-photon-vs-vm.ps1`。

| 基准 | Photon HAT AOT | Rust VM（种子字节码解释） | Aura VM（纯 Aura 实现，种子宿主元循环） |
| --- | --- | --- | --- |
| fib(30)（≈160 万次调用） | **10 ms** | 2,129 ms（212.9×） | >600s 未完成 |
| 嵌套循环 2500 万次迭代 | **30 ms** | 18,437 ms（614.6×） | 远超 1 小时（按 fib 比率外推） |
| fib(20) | 26 ms/轮（含 ~25ms 进程启动） | ≈110 ms/轮 | >580s 未完成 |
| **fib(15)** | **≈21 ms/轮**（含启动） | **≈133 ms/轮** | **>300s 未完成**（300s 超时截断） |

> Aura VM 元循环通道在 fib(15)（约 2 千次调用、6 万条指令）上即超 300 秒——
> 单条 Aura VM 指令的元循环代价 ≈ 5ms（种子 VM 解释 Vm.aura 的分派链 + 字符串
> 分配 + O(n) 常量/函数表扫描的叠加）。**AOT 原生载体**（`auc_vm_runner.exe`）
> 是正解：本轮已把装载链改为字段传递（绕开 List 参数损坏），`opCodes` 成功构建
> （9001 条），但执行期仍失败 —— 根因收窄为 **AOT 后端的局部结构体指针损坏**
> （`val mod = loadModule(...)` 的局部指针被截断，从它读取的一切字段都是垃圾；
> 与参数 i32 兜底同族，本轮已修参数侧，局部侧待专项）。

输出逐字节一致（`fib(30)=832040`、`loop acc=124950000000`）✓。

## 三、Aura VM / Rust VM 性能分析与优化（本轮已实施）

### Aura VM（`Vm.aura`）——「为什么慢」的三个实锤热点与修复

1. **常量加载 O(|常量池|) 全池扫描**（头号热点）：`getConst` 每次都 `getFieldAt`
   逐字符扫描整份常量池字符串找第 idx 行。循环体里的每个 `CONST_INT 2` 都是一次全池扫描。
   **修复**：加载期 split 一次建 `constLines` 行缓存，`getConst` O(1) 取行（越界回退旧扫描）。
2. **每指令 `toI32` 现场解析参数字符串**：`LOAD_LOCAL/STORE_LOCAL/DECL_LOCAL/JUMP/JUMP_IF_*`
   每条都解析槽号/行号字符串（分配+解析×每指令）。**修复**：加载期预解析 `arg0Ints`，
   分派臂 O(1) 取用（`arg0Fast`，缓存未覆盖时回退解析）。
3. **每指令一次字符串比较 + opName 字符串传参**：兜底空文本检查与未知指令诊断把字符串
   拉进热路径。**修复**：空文本行在加载期已编号为 OP_NOP（两检查等价），删除比较；
   `dispatch` 签名去 opName，仅在未知指令异常分支按需取。

正确性验证：`vm_p2_tests`/`phase5_vm_tests`/`diff-test 6/6` 全绿（种子宿主与元循环宿主下
行为一致，helloworld 端到端正常）。

### Rust VM（`seed/compiler/src/vm/interp.rs`）——分析与微优化（保留）

- 每指令 `OnceLock::get_or_init` 原子读（看门狗配置）→ 改为 `Vm::new` 读一次的普通字段；
- `pop()` 成功路径也读 `frames[top].ip`（仅下溢错误需要）→ 移入错误分支；
- `bin_op`/`LoadVar`/`LoadConst` 单借用化减少带边界检查的重复索引。
- **实测影响 ≈0**（fib 2105→2129ms、loop 18681→18437ms，均在噪声内）——瓶颈不在这些
  微观开销，而在架构：`Value` 枚举 56 字节（Map 变体撑大，栈压弹即 56 字节搬移）、
  逐指令函数调用 + 多重边界检查、无内联缓存/超级指令、`Rc<str>` 引用计数原子操作。
  **结论**：Rust VM 要再上一个台阶需要结构改造（Value 装箱瘦身、超级指令、
  字节码预解码为裸指针循环），收益上限约 3-5×；与 Photon AOT 的 200-600× 差距
  是解释执行与原生代码的架构差，靠解释器优化无法弥合。

### Aura VM 的 AOT 载体（已知边界 → 根因收窄至单一 emit 缺陷）

优化后的 `Vm.aura` 在 **种子宿主与元循环宿主下验证正确**，但 AOT 原生载体
（`auc_vm_runner.exe`）静默失败/段错误。经多轮二分（arg0Ints 禁用、constLines 禁用、
装载链字段化改造均复现/部分缓解），**根因收窄至 AOT 后端的「局部指针截断」缺陷**：

- `val mod: AucModule = loadModule(...)` 这类**局部结构体指针**在跨方法调用后截断，
  从它读取的一切字段都是垃圾（实测 `pendingNames.size()` = -1884394416，且同一
  值的重复读数不稳定）；
- Photon 驱动自身的 HirLowerer 前端（同为 AOT 宿主代码）新增的成员登记
  预遍历也在此崩溃（`this` 指针损坏 → 写实例字段即段错误）；
- 本轮已修其**参数侧**（参数类型兜底 i32→i8*，三处）与 i8 存储宽度、listRemoveAt
  全链；**局部侧**（返回值/局部槽的类型保持）是最后一块，修复后同时解锁：
  ① 自举链接剩余 14 符号的前端预登记（接收者类型传播已实现、空表时安全降级）；
  ② Aura VM 的 AOT 原生载体（装载链已打通至 opCodes 构建 ✓）。

**最终基准（修复后，5 轮中位）**：Photon AOT fib(30)=11ms / loop=33ms；
Rust VM 2285ms / 20628ms（208× / 625×）。

**堆 arena 溢出（2026-10-02 重大发现）**：Photon 运行时的 `heapArena` 仅 1MB
（`dataSymbolName = "heapArena:1048576"`），AucLoader 装载 26KB `.auc` 需要
分配 9001+ 子串 + 列表结构 ≈ 数百 KB——加上 Vm.aura 本身的 HashMap/VmStack
分配，总需求轻则超过 1MB，重则触发 **bump 指针回绕到 0**（`cmp rax, 1048000;
jg → 新 base 归零重分配`）→ 新分配覆盖旧对象 → 所有句柄指向被踩内存。
**修复**：扩容至 8MB（`heapArena:8388608`），溢出检查常量同步更新。
修复后薄宿主 `auc_vm_runner.exe` **首次链接成功**（`===RESULT===success`）。

**运行时仍静默**：链接成功但 Vm.aura/AucLoader 的执行路径需要 Photon 运行时
尚不完整的 std 支持（HashMap 操作、字符串子串边界等）——这是 **Photon 后端
成熟度问题**，与 AOT emit 缺陷是不同的修复轨道。薄宿主链接成功是重要里程碑：
Photon 编译的 Aura VM 首次通过了编译+链接，运行时补齐后即可参与三方基准。

**Vm.aura 优化回退说明**：本轮为 AOT 原生载体做的 Vm.aura 优化（constLines O(1)、
arg0Ints 预解析、loadPredecodedFromFields 字段化改造等）在种子 VM 路径引发回归
（vm_p2 FAIL 13 项、phase5 FAIL 9 项），已全部回退到 HEAD 恢复全绿。这些优化
逻辑本身正确（孤立验证通过），但与种子 VM 的 List/字段/参数处理存在未预见的
交互，需以「每步改动 + 即时全量回归」的方式逐步重新引入。

**参数兜底 i32 回滚说明**：emit.rs 参数类型兜底 i32→i8* 在 AOT 路径修复了
List 参数截断，但在 **VM 解释路径**引发回归（vm_p2 FAIL 13 / phase5 FAIL 9）。
已回滚为 i32 恢复全绿。

**根因分析**：i8* 兜底引发了 AOT 编译的 runner 段错误（Vm.aura 原始代码 +
i8* 参数 = 新的越界模式）。这不是参数兜底本身的问题，而是 AOT emit 的
**值模型系统缺陷**——IR 级证据：`.size()` 零参方法调用被降为 Member →
模糊字段扫描误配 `VmFrameStack.field_1(sp)` → 读偏移垃圾装箱。修复需要
在 emit.rs 的成员解析层**统一重做 List/接口类型的方法分派**（将
`.size()`/. `length`/. `isEmpty` 路由到 `Collections.count` 等 runtime 调用，
而非模糊字段匹配），并确保局部槽 alloca 类型与参数类型一致（i8* 而非 i32）。

## 四、复现命令

```bash
# Photon 编译自举（前端+后端+链接尝试）
powershell -File scripts/photon/run-hat-on-main.ps1 -BudgetSecs 1800
# 三方基准
powershell -File scripts/bench-photon-vs-vm.ps1 -Runs 5 -ThreeWay
# 隔离守卫 / 回归
bash scripts/check-dual-impl-isolation.sh
powershell -File scripts/diff-test.ps1
```

---

## 五、2026-10-03：Rust emit 值模型专项修复——AOT 载体全绿

> 本轮把上一轮「IR 级铁证已入档但未能完全修复」的 emit.rs 值模型缺陷
> 一次修完，`auc_vm_runner.exe`（纯 Aura VM 的 AOT 原生载体）端到端 PASS。

### 修复清单（按依赖序）

| # | 缺陷 | 修复 | 实测证据 |
|---|---|---|---|
| 1 | **HIR 接收者字段链解析缺口**：`resolve_receiver_type_deep` 只认 `this.<field>`/类内裸字段，`局部变量.字段`（`b.names.add`、`mod.numOpNames.size`）恒 None → 全表唯一候选模糊匹配（`add` 被配到 `ArrayList.add`：堆句柄被当 `%struct.ArrayList*` GEP → 堆损坏/段错误） | 补 `MemberAccess` 递归臂 + `field_type_in_class`（沿继承链查字段类型）；`field_type_of` 复用同表 | bisect6（类字段 List）从段错误 → boxsize=2 ✓ |
| 2 | **链式下标接收者类型丢失**：`args.get(0).size()` 的接收者是 Call/Index 节点 → 恒 None | `Index` 与 `x.get(i)` Call 形态 → 取容器泛型参数为元素类型 | probe_loop args0size 0 → 1 ✓ |
| 3 | **集合工厂类型推断缺失**：`val l2 = arrayListOf<String>()` 无标注 → `.add` 落不进列表拦截分支 | `infer_decl_type` 加 arrayListOf/arrayOf/listOf/mapOf/… → 集合类型名（只进 LOCAL_TYPE_SCOPES，不影响 LLVM 槽类型） | bisect5 l2size 0 → 1 ✓ |
| 4 | **`x == null` 被降为字符串内容比较**：`Any.hashCode` 的判空走 `String_equals(this, null)` → 装箱整数按字符串解引用 → 段错误 | null 字面量比较一律指针 `icmp eq i8*, null` | HashMap<Int,Any>.get 首次取桶崩溃 → 修复 ✓ |
| 5 | **i8*==i8* 统一走 String_equals**：`HashMap.valuesEqual` 的 `a == b`（Any）在装箱整数（奇数伪指针）上 strcmp → 段错误 | 新增 Plan A 感知 C 运行时 `aura_any_equals`（奇数比标记值、偶数比内容），emit 的 i8* 比较统一改走它 | probe_hash 全过 ✓ |
| 6 | **字符串方法改派劫持点分调用**：`VmOps.split(ft,"\n")` 按末段名被改写成 `aura_string_split(VmOps_instance, …)`——单例指针被当字符串切分 → AucLoader funcTable 全错 → "Unknown function: fibonacci" | 改派加 owner 守卫：仅裸名或 `String`/`*.String` owner 才落到 C 符号 | offMain 0 → 25 ✓ |
| 7 | **VmOps.sub 重载折叠**：算术 `sub(a,b)` 与字符串切片 `sub(text,start,len)` 同名 → AOT 签名表取 3 参版（末参 Int→i32），运行时链到 2 参版 → SUB 结果恒错（fib 无限递归） | 删除零调用的 3 参切片 `sub`（vm 模块扫描确认无其它同名重载） | fib(10)=55 ✓ |
| 8 | **局部槽类型残留 unsized 结构体**：显式标注局部用 `llvm_type`（未知 struct → `%struct.X*`） | 改用 `llvm_type_checked`（未定义结构体退化为 i8*） | — |
| 9 | **`__list_contains/__list_index_of/__list_is_empty` 无 emit 分支**：HIR 拦截后发射裸名 → undefined symbol（ArrayList.distinct） | 补三个降级分支（Collections.contains/indexOf/count） | bisect5 链接恢复 ✓ |
| 10 | **常量池「一行一常量」不变式可被值内换行打穿**：stdlib 的 `"\n"` 字符串常量使后续全部常量错位 +1（loop50 的 const[41] 读成 `}` → 循环条件恒假 → 循环整段跳过） | AucLoader 装载期转义（`\`→`\`、换行→`\n` 两字符），`Vm.getConst` 读取时反转义 | loop50 双路输出一致 ✓ |
| 11 | **VmOps.isString 对装箱整数误真**：AOT 下 `typeof` 按静态类型折叠（Any 恒报 "String"）→ `100 + 23` 走拼接得 "10023" | 新增 `__is_tagged_int` 内建（AOT：原始 ptrtoint+奇偶；种子 VM：常量 false，其 typeof 本就准确），`isString`/`intOf` 用它守卫 | helloworld 算术 123 ✓ |
| 12 | **三处 GB 级泄漏**（基准 25M 迭代触发 8GB 守卫）：① `HashMap.put` 对已存在键仍追加记录（locals 键集固定 → 桶链无限增长）→ 改原地 `vs.set(i, value)`；② `Any.hashCode` 对装箱整数 toStr 分配（每次 Map.put 键哈希）→ 装箱整数直接返回值本身；③ `incRef/decRef/dropRef` 的 toStrAny 键构造 → 装箱整数早退 | 同左 | bench_loop 25M 迭代 3GB 限制内跑完 ✓ |
| 13 | **Windows 主线程栈默认 1MB**：递归下降程序（Parser/link 递归 import）深层即栈溢出 → 段错误且零输出（Photon 驱动此前「任意输入瞬崩」的根因之一） | AOT 链接统一加 `/STACK:67108864`（clang `-Wl` 与 lld-link 两条路径） | probe_link 1MB 崩 / 64MB 过 ✓ |
| 14 | **VM 指令上限 10M** 太低（bench_loop 需 ~2 亿条） | MAX_INSTRUCTIONS 提到 2e9（仍在 Int 范围） | — |

### 结果

- **AOT 载体端到端**：`auc_vm_runner.exe` 跑 `helloworld.auc` 全部正确
  （算术 123 / 循环 15 / fib(10)=55 / factorial(10)=3628800，`Phase A-E 全流程验证: PASS`）。
- **三方基准（fib(30)）**：Aura VM 原生载体首次完成（此前 >600s 未出）；
  loop 基准正确性修复后可跑完（acc 按 Int32 回绕——与种子 VM 的 64 位累加
  存在语义差异，见「遗留」）。
- **全量回归**：diff-test 6/6、vm_p2 PASS、phase5 PASS、AOT 并发 9/9、
  隔离守卫双脚本 PASS、language-test 26/26。

### 遗留（按优先级）

1. **Photon 驱动宿主崩溃（photon 编译 VM 的最后阻塞）**：宿主二进制
   （seed AOT 编译）的 import 闭包**超过 probe_link 的确切规模**后，
   前端 `loadPath` 内稳定段错误——`import Cpu`（纯 rdtsc 包装、零调用）
   即可复现；512MB 栈 / O0 均不消失；与 Env/Process/Clock 无关
   （它们只是碰巧加尺寸）。矩阵：photon 闭包+前端 ✓；前端+任一额外
   std 模块 ✓；photon 闭包+任一额外 std 模块 ✗。定位为**布局敏感的
   内存踩踏**（死代码不影响运行时，嫌疑在发射器常量/全局层），需要
   真调试器（cdb 不可用）拿故障地址后符号化。复现探针：
   `build/bisect/probe_link.aura`（基线，通过）/
   `probe_gc.aura`（+Cpu import，崩）。
   PhotonHatCompile.aura 已瘦身到最小安全形态（无 Env/exists/readText/
   Process，固定路径通道 `build/hat-vmhost/current.aura`），宿主 bug
   解除后即可直接跑通。
2. **Int32 回绕语义差异**：bench_loop 的 acc（124950000000）种子 VM 按
   64 位累加、Aura VM（AOT）按 32 位回绕。基准程序应改用 Long 累加。
3. **Aura VM 执行速度**：fib(30) 原生载体 ~104s——CALL 路径的
   O(表长) 函数表扫描仍在（优化方案 A1/A2/B 未实施），实施后预期
   进入 1-10s 量级。
4. **`__is_tagged_int` 前端守卫**：若用户代码定义同名函数会撞名
   （`__` 前缀为编译器保留字，建议前端加保留名校验）。

### 五.1 2026-10-03 追加：前端崩溃已修复，后端崩溃已精确定位

**前端「闭包尺寸阈值」崩溃 = 字符串方法名劫持（已修复，§五 #6 的延伸）**：
宿主二进制的 import 闭包含 photon 模块时，前端大量 `X.substring(a,b)` 类
调用的接收者类型未知 → HIR「全表唯一候选」把 `substring` 配到闭包里**唯一**
定义了该方法的类（`PhotonPipeline.substring`）——字符串被按无关类结构体
GEP。用 `AURA_CRASH_DUMP=1`（本轮修复为 kernel32 的
`RtlCaptureStackBackTrace`，此前 dbghelp 恒取不到）+ 帧 RVA ↔ obj 符号表
映射拿到 Aura 级调用链：
`main → AotModuleLinker.link → loadPath → Parser.__ctor1 → Lexer.scanAll
→ Lexer.scanIdentifier → Lexer.sliceOf → PhotonPipeline.substring(!)
→ aura_string_length(1)`。修复（hir.rs 唯一候选守卫
`is_string_builtin_method`）后，前端 link→SSA→HAT 在 photon 闭包下**全部
通过**（11165 节点 / 406 函数 / 374KB HAT）。

**后端崩溃（新定位，预存在，下一轮专攻）**：compileFromSsa 的指令选择/
COFF 产出阶段段错误。证据：
- 新旧 seed、O0/O2 宿主、tiny/bench 输入均复现（`[CALLDST]` 打印后崩）；
- verbose 尾部：`RELCHK-FINAL funcs=116 strConsts=1 groups=161 syms=171
  kept=83 dropped=0` 后崩；
- 更早的 `[rt] all constructors appended, funcs=3201615632431411824`
  ——外部读 `writer.functions.length` 得指针量级的垃圾，而 writer 内部
  `splitComma(this.functions).size=116` 正常 ⇒ **外部成员读取解析到了
  错误类的同名字段**（值模型家族，与 §五 #1 同族但发生在
  PhotonObjectWriter 上）；
- 复现：任意含对象字段访问的 HAT（tiny.aura 即可）；
  `AURA_CRASH_DUMP=1 + minidbg.py（本目录）+ llvm-nm obj 符号映射` 三件套
  可在 3 分钟/轮的迭代里继续收窄。

### 五.2 2026-10-03 续：三项遗留的处置结果

**遗留 3（Aura VM 执行速度）——已完成，fib(30) 104s → 18.6s（5.6×）**：
- **方案 A2（函数表索引）**：Vm 加载期把 funcTable 一次性解析进
  `funcOffsetMap/funcParamMap`（HashMap<String,Int>），`funcOffset`/
  `funcParamCount` 执行路径 O(1) 查表（未命中回退旧扫描）；CALL 路径的
  每次全表 split+substr 分配归零。
- **方案 E（getConst 行缓存）**：常量池装载期按行缓存 + 预反转义
  （`constLines`），getConst 从 O(池长) 扫描 → O(1) 下标。
- **locals 帧池**：`callOrBuiltin`/`callTail` 从 `localsPool` 弹出复用帧
  映射，`returnFromFunc` 退役归还；配套 `HashMap.resetForReuse()`
  （零分配置空，区别于 clear() 的桶重建）。此前每调用
  `HashMap<Int,Any>()` 触发 allocBuckets(16) ≈ 48 次堆分配 × 160 万次
  调用 = GB 级泄漏（OOM 归因表：aura_dynlist_new 992 万次 / 529MB）。
- 内存：fib(30) 8GB OOM → 3GB 内完成；fib(24) 800MB OOM → 600MB 内完成。
- 排障基建（本轮新增，可复用）：`build/bisect/minidbg.py`（Windows 微型
  调试器：DEBUG_ONLY_THIS_PROCESS + 异常地址/寄存器）+ C 运行时
  `AURA_CRASH_DUMP=1`（修复为 kernel32 RtlCaptureStackBackTrace，并新增
  帧 RVA 打印与 `AURA_ATTR_DUMP` 分配调用点归因表）+ obj 符号表映射脚本
  ——三层可在 3 分钟/轮内把任何宿主崩溃落到 Aura 函数级。

**遗留 2（Int32 回绕）——基准已修，语义统一判为语言迁移**：
- `bench_loop.aura` 累加器改 Long（语义正确的基准）；种子 VM 侧补
  `aura_cast` 内建臂（`x as T` 的运行时转换此前元循环缺实现）。
- 整树把 Rust VM 的 Int 算术统一为 32 位回绕**否决**：Rust VM 的
  `Value::Int` 兼作指针句柄（`Memory.alloc` 返回 Int，地址可达 2^47），
  盲目回绕会打断「指针即 Int」ABI。Int 宽度语义统一需要「句柄迁 Long」
  的语言级迁移，单独立项。
- 随带修复：种子 VM `aura_cast/aura_cast_safety` 对**泛型实例化目标**
  （`HashMap<Int, Any>` → 基类名比对）与**占位类型名**（`<type>`/空 →
  透传，对齐 AOT 指针 cast 不检查的既有语义）的处理——此前 locals 帧池
  的合法 cast 在元循环下误报失败（diff-test 04 曾因此红）。

**遗留 1（Photon 宿主后端崩溃）——前端半已修复，后端半精确定位待攻**：
- 前端半（字符串方法名劫持）已修复，photon 闭包下 link→SSA→HAT 全通
  （见 §五.1）。
- 后端半收敛到**单一预存在缺陷**：`buildRuntimeObject` 外部读
  `writer.functions.length` 得指针量级垃圾（3.2e18），而 writer 内部
  `splitComma(this.functions).size=116` 正常——外部成员读取解析到了错误
  类的同名字段（与 §五 #1 同族的值模型缺陷，落在 PhotonObjectWriter 上）。
  旧 seed 驱动（Oct 2）同样复现 ⇒ 与本轮全部改动无关。
- 下一轮工具链已就位：minidbg.py + AURA_CRASH_DUMP（含 ra1 AOT 调用点
  追踪与 oom-attr 归因表）+ obj 符号映射，任何崩溃 3 分钟/轮落到函数级。

### 五.3 2026-10-03 终：Photon AOT 已恢复（根因修正 + 驱动二进制回滚）

**根因修正（推翻 §五.1 的「闭包尺寸阈值」判断）**：Photon 腿失效的真因有二：
1. **基准脚本接口断裂**（本轮引入，已修复）：驱动曾改为固定路径通道，而
   基准脚本按环境变量接口准备产物路径 → 链接步骤找不到 .obj。已把
   `scripts/bench-photon-vs-vm.ps1` 回退到环境变量接口。
2. **新 seed 编译宿主驱动的回归**（真实存在，本轮定位未竟）：用本轮修复后
   的 seed 重建 PhotonHatCompile.exe，`compileFromSsa` 里
   `this.outDir`（或同类 String 字段）读到装箱整数（`pipeline.outDir = ...`
   的写入疑似被截断进 i32 字段），后续 strlen 崩溃。已做排除实验：
   hir/emit 的六处行为改动全部门控关闭后仍崩 ⇒ 触发点在这六处之外
   （嫌疑收敛到 emit 变量槽/结构体布局相关路径）。

**处置**：`build/hat-native/PhotonHatCompile.exe` 已回滚为经验证的
Oct 2 驱动二进制（`PhotonHatCompile_oldseed.exe` 同体副本保留为
`PhotonHatCompile_stable.exe`）——实测今日仍可正确编译 bench_fib /
bench_loop / tiny（COFF-MAIN/RUNTIME 齐全、success、产物可链接运行、
tiny 输出 "tiny ok" ✓）。基准脚本回退后 Photon 腿恢复出数：
**bench_fib 10ms / bench_loop 36ms**（与失效前一致）。

**Photon 编译产物正确性验证**：tiny_aura.exe（Photon 编译）stdout
"tiny ok" ✓ 退出码 0 ✓。fib 产物（bf2.exe）静默退出——photon 后端对
toStr/concat 组合的运行时支持仍在「Photon 运行时 std 补齐」轨道
（§四 遗留 2）。

**新 seed 宿主回归的复现命令**（下一轮专攻）：
```
AURA_FIX_OFF="assign_ty,nullcmp,anycmp,slotchecked" AURA_NO_DEEPFIELD=1 \
  seed/target/release/aura.exe build --aot \
  aura/photon/aura/lang/compiler/photon/PhotonHatCompile.aura \
  --output build/hat-native/PhotonTest.exe
# 六处门控全关仍崩 ⇒ 触发点在门控之外
AURA_CRASH_DUMP=1 + 上述 env 运行即得崩溃现场（target=0x233 装箱整数
被当字符串 strlen；帧 #9 = compileFromSsa+0x26a1 的 writeText 调用点）
```

**2026-10-03 终更新**：本轮六项 emit/hir 修复后复测——仍崩，但崩溃点
从 Step-4 后的 RELCHK/[rt] 阶段推进到 compileFromSsa 的 Step-4
vprintln eager 参数（`this.substring(machineCodeHex, 0, min(...))`，
machineCodeHex 读出装箱整数 0x171=184 → strlen 崩）。**崩溃点随构建
漂移**（此前 RELCHK 后、现 Step-4）⇒ 根因是更早的内存踩踏，崩点只是
首个受害者。另注意 vprintln 参数是急切求值（verbose 关闭时
substring/concat 仍执行），本身是无谓分配源。下一轮：ra1 AOT 调用点
追踪 + 在 X86Emitter/PhotonObjectWriter 的 textSection 累加路径加
canary 定位首次越界写。

---

## 六、2026-10-03（晚）：B1 驱动回归修复 + B2 @native 探测——双双收口

> 按四阶段方案的阶段 1（B1）+ 阶段 2.1（B2）执行。**B1 已修复**，
> **B2 判定达成**：`@native(asm=…)` 在 Photon 轨端到端可用。

### 6.1 B1 根因（推翻「内存踩踏」假设）：单例字段读取退化为 i32@偏移0 + 装箱

「崩溃点随构建漂移」的真因不是更早的内存踩踏，而是**确定性的字段解析
缺陷**——不同构建的内联/死代码变化只决定「哪个错误读取先执行」。

**定位链**（崩点 → obj 符号映射 → 反汇编 → 重定位表，约 3 轮）：

1. 新 seed 驱动 + tiny 输入：`read target=0x171`（装箱整数 184），
   `rcx=-0x171`（strlen 签名）；调用链
   `main → compileHat → compileFromSsa+0x26a1 → File_writeText →
   Stdio.stringToBuffer → strlen(0x171)`；
2. compileFromSsa+0x26a1 的重定位表：`File___ctor1 → File_writeText`
   ——即第 778 行 `FileUtils.writeText(runtimeObjPath + ".syms",
   PhotonRuntime.RUNTIME_SYMBOLS)`（内联展开）；
3. 实参准备反汇编（铁证）：
   ```asm
   movslq (%rip), %rax        ; 32 位加载 PhotonRuntime_instance+0
   leaq   0x1(%rax,%rax), %rdx ; (v<<1)|1 —— Plan A 整数装箱！
   call   File_writeText
   ```

**机制**：`PhotonRuntime.RUNTIME_SYMBOLS`（object 单例的 String 字段）
的接收者被 `emit_variable_load` 发成 i8*（不透明）→
`resolve_member_field_owner` 拿不到类名候选 → 2026-10-02 加的
「i8* 不透明句柄不做模糊匹配」守卫（emit.rs:7220）返回 None →
调用方兜底 `("i32", 0)`（emit.rs:7053）→ 32 位加载 + 按整数装箱。

**修复**（`seed/compiler/src/codegen/aot/emit.rs`，+8 行）：
`resolve_member_field_owner` 的 `HirExpr::Var` 分支增加单例候选——
`ctx.singleton_structs.contains(var)` 时把该名字推入 candidates。
字段读（7049）/字段写（2520）/嵌套字段地址（4577）三个调用点同时受益。
候选未命中时行为与修复前一致（无回归风险）。

### 6.2 B2 判定：@native(asm=…) 在 Photon 轨可用（缺的只是运行时定义）

- **调用点侧 ✓**：Photon 前端/指令选择把 `Cpu.rdtsc()` 正确降为
  `@Cpu_rdtsc` 限定名调用（InstructionSelection 无需 NativeAttr 特殊
  处理）；
- **定义侧**：`PhotonRuntime.buildRuntimeObject()` 补 4 个手写 X86
  发射器（`emitCpuRdtsc/MemFence/Cpuid/AtomicAdd`）+ RUNTIME_SYMBOLS
  追加 4 名（sidecar 缓存自动失效重建）；
- **微探针全绿**：`build/bisect/probe_native.aura` →
  RDTSC-OK / FENCE-OK / CPUID-OK / `atomic old=41 new=42` / ADD-OK。

### 6.3 新发现（预存）：extern interface 成员调用的 self 占位 ABI 错位

探针反汇编实测：extern interface 成员调用点把「self 占位 null=0」放
**rcx**，真实实参从 **rdx** 起（`Memory_alloc`: rdx=n；
`Memory_write64`: rdx=addr, r8=val）。而既有 Memory/io 发射器一律按
「rcx=首实参」编写——**整整错位一格**（P4 套件未暴露：其
test_memory_alloc 用的是普通类 `MemoryManager.alloc`，从未走过
extern interface 路径）。

**修复**：12 处发射器入口插入寄存器右移（rcx←rdx←r8←r9，函数体不动）：
Memory_read/write×8 宽度变体、alloc、atomicAdd、arcIncrement/Decrement、
copy、io_open/close/read/write。Collections/ARC **不在修复面**（它们是
真单例 object，`Collections.listAppend(list,v)` 调用点实测无 self 占位，
rcx=list 直接成立 ✓）。

> 注意：`emitMemoryAtomicAdd` 体内 `xadd [rcx], r8` 字节本就正确；
> 本轮新写的 `emitCpuAtomicAdd` 曾两度手写编码错误（ModRM 0x0A→r9、
> `mov rax,r8` 误编 4C 89 C8→r9），以 objdump 反汇编汇编产物为终审
> 修正为 `F0 4C 0F C1 02` + `4C 89 C8`→`4C 89 C0`。**教训：手写字节
> 必须以产出的 obj 反汇编复核**。

### 6.4 验证结果（新 seed 驱动 `PhotonB5.exe`，固定路径通道）

| 项 | 结果 |
|---|---|
| tiny（编译+内置链接+运行） | `tiny ok` EXIT=0 ✓ |
| bench_fib | `fib(30) = 832040` ✓ |
| bench_loop | `loop acc = 124950000000` ✓ |
| @native 探针（probe_native.aura） | 5/5 全绿 ✓ |
| diff-test（p2_diff 6 用例） | 6/6 PASS ✓ |
| vm_p2_tests / phase5_vm_tests | PASS / PASS ✓ |
| AOT 并发（run-concurrent-aot.sh） | 9/9 ✓ |
| 双实现隔离守卫 | PASS ✓ |
| diff-test-full | 8 mismatch（**A/B 证实全部预存**，见 6.5） |

### 6.5 预存问题 A/B 排除（与本轮改动无关，均已复现于未修复 seed）

1. **seed AOT 对 `Memory.alloc` 发裸名 `@alloc`** → llc
   `use of undefined value`（probe_native 的 seed AOT 构建失败；
   stash 对照复现。方案 B2 所述「种子 AOT 已支持」仅覆盖 Cpu.rdtsc）；
2. **diff-test-full 的 8 个 mismatch**（test_lambda6 等，元循环宿主
   42→0）：stash 对照复现，lambda 值模型预存缺陷；
3. **FileUtils.exists/readText 在 Photon 产物内段错误**（fs 探针）：
   崩入 `runtime_emitObjectAlloc` 清零循环——上游把垃圾值当分配大小
   （「外部成员读取」值模型家族在 stdlib 内部的实例）。**阶段 2.2
   「Photon 运行时 std 补齐」轨道首个收口项**（此前该症状曾迫使驱动
   改固定路径通道；与 6.3 的 ABI 错位是否同一层，待专项探针矩阵判定）。

### 6.6 对总方案的影响

- **阶段 1 关闭**（1-2 轮预估 → 实际 1 轮）：新 seed 驱动可用，
  G2 的 Photon 编译轨解锁；
- **阶段 2 提速**：@native 支持面确认为「只需补运行时定义」，阶段 2.2
  的缺口矩阵（HashMap.get/containsKey、String.concat/equals/indexOf、
  Collections.set、Clock_gettime）按 probe_native 模式复制即可；
  **附带发现**：Memory/io 的 self 占位错位若不修，阶段 3/4 的 VM 闭包
  （重度使用 Memory.read/write 与 File.readText 装载 .auc）必然踩雷
  ——本轮已排掉；
- **驱动二进制口径**：`build/hat-native/PhotonHatCompile.exe` 仍是
  Oct 2 稳定二进制（env-var 接口，bench 脚本依赖）；新 seed 驱动为
  `PhotonB5.exe`（固定路径通道 `build/hat-vmhost/current.aura`）。
  接口统一（env-var vs 固定路径）列入阶段 2 清理项；
- mmap/munmap 发射器（6 参 + 栈参）本轮**未**右移，ArenaAllocator 依赖
  它，留待阶段 2.2 专项。


## 七、2026-10-03（夜）：阶段 2.2 —— Photon 轨 ABI 定版 + 修复 8 项

> 承接 §六。目标：以 fs/io 探针为向导修复 FileUtils 路径，补缺口原语。
> 结果：**崩溃全清零**（fs/io/native 三探针 + tiny/fib/loop 全部正常退出），
> `FileUtils.exists()` 端到端 ✓；`readText` 仍取到垃圾（io_open 失败，见 7.5）。

### 7.1 ABI 定版（本轮最重要的架构结论）

extern interface 成员调用点 = **普通函数形状（rcx=首实参）**。达成方式：

1. **Hir.aura lowerCall**：大写 Ident 接收者（`Memory.write(...)` 的 `Memory`）
   保留 `HirVar(name)` 原样入 kids[0]——旧实现 `lowerExpr` 把模块名降成
   `@i32_const 0`，self 占位进实参、整体错位一格；
2. **SsaBuilder recvIsSingleton** 形状兜底：大写 + 非局部变量 ⇒ 视为模块名
   引用丢掉接收者（不依赖 externCsv 登记完整性——实测 Cpu/Memory 缺席
   externCsv，根因未深挖，已留档）；
3. **回滚 §六的 12 处发射器右移** + Cpu 系/FileOps_lseek 发射器改 rcx 系
   （调用点既然统一普通形状，发射器回到 rcx=arg1）。

Collections/ARC（真单例 object）不受影响（调用点本就无 self 占位）。

### 7.2 修复清单（按依赖序）

| # | 缺陷 | 修复 | 证据 |
|---|---|---|---|
| 1 | `File(path).exists()` 构造器调用结果作接收者 → `receiverClassOf` 返回空 → 「隐式 this」兜底限定成 `FileUtils_exists` **自递归**（每层 alloc 直至爆栈） | `receiverClassOf`/`receiverTypeNameOf` 增 HirCall 分支：被构造类名即接收者类 | 产物此前缺 `File_exists`/`File_readText` 符号（死代码剪除）；修复后出现 ✓ |
| 2 | 运行时缺 `FileOps_lseek` → 链接失败 | 新发射器：whence==2 → kernel32 `GetFileSize`；whence==0 → 返回 0（装载流 open 后位置本就在 0，无需真 seek） | 链接通过 ✓ |
| 3 | `streq(x, null)` 对地址 0 做内容比较 | 双保险：SsaBuilder.buildBinary + ISA Case-2 对 **null/0 常量比较** 豁免 streq（走指针相等）；runtime streq 增加 **null 安全前置**（null==null→1，null==x→0） | 崩点从 streq 循环消失 ✓ |
| 4 | runtime streq 循环索引 r8 未清零（本轮新写的 null 守卫跳过了 `xor r8,r8`）| 索引清零移到所有进入路径的必经点 | minidbg 寄存器：r8 残留 rcx 指针 → [rcx+r8] 越界 |
| 5 | io_open（NtCreateFile 直编版）fd 恒 -1 | 换道 kernel32：CreateFileA/ReadFile/CloseHandle + 既有 GetFileSize —— 但 **fd 仍 -1**（见 7.5，未解决） | 机器码反汇编逐参核对无误 |
| 6 | `object Allocator` 等单例实例欠量分配（classSizeOf=16，字段写 [inst+0x50]）→ 堆相邻分配被踩（fs/io 的字符串缓冲区内容随机损坏） | synthesizeSingletonGetter 尺寸下限 128（单例每进程一份，过量无害） | 缓冲区内容不确定性问题还在（见 7.5），但崩溃消失 |
| 7 | 尝试：空串字面量 `""` 的 SSA Const（ty=19 不被 isString 认可）物化成 `mov 0` | emitConst `|| constVal == ""` 走字符串分支 —— **已回滚**：触发布局敏感回归（见 7.3） | bench_loop acc=-1 |
| 8 | seed AOT 值模型缺陷家族的又一实例：**布局敏感** —— ISA 无害改动（加几行）令 bench_loop 循环变量错乱 | 回滚触雷改动；Case-2 豁免保留（无雷） | 二分：SsaBuilder/Hir 无罪，ISA 加行即坏、减行即好 |

### 7.3 布局敏感回归的二分记录（重要教训）

- P44（全部 Aura 改动 stash 掉）：bench_loop ✓
- P45（+SsaBuilder）：✓ → P46（+Hir）：✓ → **P47（+ISA）：✗**
- ISA 内两处编辑（Case-2 豁免 / emitConst 空串重路由）逐一验证：
  撤 emitConst 重路由 → bench_loop ✓，且 fs 探针不崩（streq null 前置兜底）。
- 结论：**emitConst 的空串重路由本身正确，但移动代码布局后触发潜在值模型
  缺陷**（与 §五.3「崩溃点随构建漂移」同族）。这类「修复撞上布局雷」的
  现象在本仓库已是第三次 —— 根治只能靠 seed AOT 值模型专项。

### 7.4 状态矩阵（PhotonP49.exe，固定路径通道）

| 项 | 结果 |
|---|---|
| native 探针（rdtsc/fence/cpuid/atomic/list.add） | 5/5 ✓ |
| fs 探针（FileUtils.exists + readText） | 无崩溃，EXISTS-OK ✓，READLEN ✗（7.5） |
| io 探针（open/lseek/read/close） | 无崩溃，fd=-1 ✗（7.5） |
| tiny / bench_fib / bench_loop | `tiny ok` / `fib(30)=832040` / `acc=124950000000` ✓ |
| diff-test 6/6、vm_p2、phase5、隔离守卫 | 全 PASS ✓ |

### 7.5 遗留（下一轮首项）

**io_open 返回 -1**：CreateFileA 版发射器机器码逐参核对无误
（GENERIC_READ/share=0/OPEN_EXISTING/attr=128），仍失败。下一步：
① 加 GetLastError 诊断发射器（一次拿到失败码）；② 排查 stringToBuffer
产出的缓冲区内容（fs 探针实测路径缓冲区**首字节为 0**、内容被后续分配
覆盖——堆损坏的余波，7.2#6 的下限修复可能不覆盖全部损坏源）；
③ 或许直接绕过：loadAucAndRun 的固定路径通道改为启动时经
`GetModuleFileName`/环境注入。

**布局敏感值模型缺陷**（7.3）：影响所有「碰布局」的修复 —— seed AOT
值模型专项的优先级应提升。

### 7.6 诊断基建增量（可复用）

- **minidbg.py 增强**：AV 时转储 rsp 栈 0x800 字节 + rbp 帧回溯
  （frame[i] rbp/ret）——本轮靠它把崩点定位收敛到函数级；
- 崩点→符号映射脚本化流程（llvm-readobj 拿 .text RawDataSize →
  主 obj/runtime obj 分段 → llvm-nm 双分）已在本轮完全跑通。


---

## 八、2026-10-03（深夜）：io/文件链路攻坚 —— EXISTS-OK 首通，stringToBuffer 定性为最后拦路石

> 承接 §七。目标：修通 readText ⇒ .auc 装载 ⇒ Photon VM 跑通。
> 结果：**fs 探针的段错误风暴全部清零**，`FileUtils.exists()` 成为首个在
> Photon 原生产物里端到端工作的文件 API；readText 的阻塞点已收敛到
> **单一定性缺陷**（§8.3，有反汇编铁证）。

### 8.1 新修复

| # | 缺陷 | 修复 |
|---|---|---|
| 8.1 | io_open 失败原因不可见 | 失败路径改返 GetLastError 码（诊断期；fd=2 = ERROR_FILE_NOT_FOUND ⇒ 路径串损坏而非调用问题） |
| 8.2 | io_open 写标志恒 OPEN_EXISTING ⇒ .hat 首写必败 | flags≠0 → CREATE_ALWAYS（第 5 参按 flags 二选一） |
| 8.3 | `File.readText` 在 Photon 轨段错误 | 见 §8.3：**stringToBuffer 写循环的嵌套调用实参被 SSA 折叠** |
| 8.4 | FileOps_lseek 的 -1 只写 eax（高位残留）| 改 `emitMovRI("rax", -1)`（64 位全 1）—— 否则 size=4294967295 骗过 `size<=0` 守卫 ⇒ malloc(4G) 段错误 |
| 8.5 | emitRead 返回字节数错拿 r9（易失，调用后已冲）| 已知并保持（AucLoader 按内容解析；改 StackLoad 会移动布局触发 8.6） |
| 8.6 | **布局敏感启动栈溢出**：在 stringToBuffer 里加诊断 println ⇒ 驱动启动即 0xC00000FD | 定性：控制台写出路径回调 stringToBuffer ⇒ println 诊断 = 无限递归；且任意源码位移都可能触发随机雷（§7.3 家族） |
| 8.7 | minidbg.py 增强：STACK_OVERFLOW 分支加 rbp 帧回溯、rsp 栈转储 0x800 | 复用 |

### 8.2 实验矩阵（stringToBuffer 变体 × 驱动构建）

- 原实现（调用结果作循环边界）：seed 轨 ✓ / Photon 轨写丢失 ✗
- 常量边界重写（4096+NUL 哨兵）：seed 轨 ✗（missing imports——布局雷）
- 手动循环+charCodeAt（main 作用域探针）：**✓ open/read 全通**
  （fd=句柄、tiny.aura 内容 "im" 正确读出）—— 形态正确性取决于**所在函数
  与编译布局**，而非模式本身。

### 8.3 最后拦路石（反汇编铁证）

`Stdio.stringToBuffer` 的写循环在 Photon 编译产物里：
```asm
; 源码：Memory.write(buf + i, s.charCodeAt(i))
4e4: r12 = r15 + rsi      ; buf + i ✓
4ea: rdx = rsi            ; ← 实参2 = i（应为 charCodeAt(i) 的返回值）
4ed: rcx = rdi            ; ← 实参1 = rdi（应为 buf + i）
4f0: call Memory_write    ; 循环体内 charCodeAt 调用消失
```
嵌套调用实参（`charCodeAt(i)` 作 `Memory.write` 的实参）在 SSA/指令选择
层被折叠为循环变量 ⇒ 写入不落地 ⇒ 缓冲区全零 ⇒ open("") ⇒ 文件链路断。
main 作用域同模式正常 ⇒ 与函数上下文/布局相关（§7.3 家族）。

**下一步**：AURA_PHOTON_DEBUG_HIR=1 对比该循环的 SSA/DAG（实参节点
从何而来）→ 定位 InstructionSelection.emitCall 的实参节点配对在循环体
内的选择错误。修复后 readText 通 → AucLoader 通 → VM runner 通。

### 8.4 验证矩阵（PhotonFinal.exe）

| 项 | 结果 |
|---|---|
| fs 探针 | EXISTS-OK ✓ / readText 段错误 ✗（§8.3） |
| native 探针 | 5/5 ✓ |
| tiny / fib(30) / bench_loop | ✓ / 832040 ✓ / 124950000000 ✓ |
| diff-test 6/6、vm_p2、phase5、隔离守卫 | 全 PASS ✓ |

> ⚠️ 运行保护约定：驱动运行一律 `AURA_MEM_LIMIT_MB=6144 timeout 240`
> （§7.3 的分配循环曾致 22GB 内存驻留）。


---

## 九、2026-10-03（续）：`.length` 误读修复 + 到达值模型冰山底部

### 9.1 `.length` 属性误读修复（新缺陷 + 修复）

`[len]` 诊断（buildMember）实测：`.length` 的接收者类型判定大面积失败——
`recvTy=[]`（localTypes 未登记，HIR 参数 ty 列为空 ⇒ localTypeAdd 的
「纯可见性不登记」规则跳过）甚至 `recvTy=[FileUtils_readText]`（簿记污染）。
旧路径落到「唯一属主字段扫描」⇒ `@field(@s, …)` 把字符串前 8 字节当长度。

**修复**：buildMember 的 length/size 分支——`recvTy == "String" || recvTy == ""`
都走 strlen（Aura 里属性式 `.length` 接收者实际只有字符串；字段误读必然
垃圾，strlen 严格更优）。

### 9.2 冰山底部（本轮最终定性）

strlen 兜底后 fs 探针仍段错误——minidbg 帧回溯定位：

    crash: strlen+0xa（rcx=0）
    caller: Stdio_stringToBuffer+0xa7（即 `s.length` 的 strlen 调用）

**stringToBuffer 的参数 `s` 在运行期就是 0**。结合本轮全部证据
（bench_loop 变量错乱、Memory_write 实参折叠为 i、File_init 递归），
收敛为一句话：**Photon 管线（seed AOT 编译的 SsaBuilder/ISA 后端）对
「单例方法参数 / 循环体内嵌套调用实参 / 跨块变量」的值模型与寄存器指派
存在系统性缺陷**——与种子 AOT 的 §五 值模型家族同源，是 Photon 轨的
项目级深坑。

**它直接阻塞**：stringToBuffer（参数=0）→ File.init pathBuf → readText
→ AucLoader → **VM runner**。G3 的最后一段路必须先修它。

### 9.3 下一轮首攻方案（按性价比）

1. **最小复现缩圈**：`fun f(s: String): Long { return s.length }` 单函数
   探针 → AURA_PHOTON_DEBUG_HIR=1 看 Param 节点的 SSA/LIR/DAG 与寄存器
   指派（预期：Param 值被指派为 0 或与 mem/enry 常量共用节点）；
2. **对拍 seed 轨**：同一函数 seed AOT 编译后行为正确 ⇒ 差异即 Photon
   管线的缺陷点；
3. 修复方向预判：emitCall/resolveNode 对 `Param` 值的 DAG 节点注册，或
   RA 对入口 Param 的初始寄存器/栈槽分配（对照 §1.2 的 loadDstReg 家族）。

### 9.4 验证矩阵（PhotonP64.exe）

| 项 | 结果 |
|---|---|
| fs 探针 | EXISTS-OK ✓；readText 段错误 ✗（§9.2，strlen(NULL)） |
| native 探针 | 5/5 ✓ |
| tiny / fib(30) / bench_loop | 全 ✓ |
| diff-test 6/6、vm_p2、phase5、隔离守卫 | 全 PASS ✓ |

> 本轮所有改动均有独立价值（EXISTS-OK、strlen 兜底、io 三件套、lseek、
> ABI 定版、布局雷证据链），即使 §9.3 深坑未修也不回退。

---

## 十、2026-10-03（深夜Ⅱ）：§9.3 首攻执行 —— 修复 9 项，fs 探针从段错误到端到端跑通

> 承接 §九。按 §9.3 计划用 AURA_PHOTON_DEBUG_HIR=1 全量转储追 Param/实参链。
> 结果：**fs 探针 rc=0**（EXISTS-OK + READLEN 输出，此前 0xC0000005）；
> tiny / fib(30)=832040 / bench_loop / R 探针组全绿无退化。
> 驱动二进制口径：`build/hat-native/PhotonP68.exe`（seed AOT 构建，
> `seed/target/release/aura.exe build --aot PhotonHatCompile.aura --output …`）。

### 10.1 本轮修复清单（按定位顺序，全部有转储/反汇编/运行期三重证据）

| # | 缺陷 | 修复 | 证据 |
|---|---|---|---|
| 1 | **ISA `emitStreqSetcc` 极性反转**：streq 返回 1=相等，`cmp res,0` 后 ICmpEq 应取 `setne`，旧代码按比较符字面映射成 `sete` ⇒ 所有走到该路径的判等整体取反。`if (s == null)` 变成「非空才进 null 分支」 ⇒ strlen(NULL) | cond 交换（Eq→ne / Ne→e） | fs 探针反汇编 `Stdio_stringToBuffer+0x30..0x50`；修复后 EXISTS-OK 恢复 |
| 2 | **HatParser 丢 `@null` 的值文本**：序列化端把 `Const aux="null"` 编成 `@null`（不带值），解析端 mapOp 后 aux="" ⇒ ISA Case-2 的 nullish 判定（aux=="null"\|\|"0"）永远不命中 ⇒ 判空被当字符串比较发 streq | `opRaw=="null"` → `aux="null"`；配套 ISA `emitConst` 数值路径 "null"→"0"（字符串分支之后，不伤 `"null"` 字面量） | fs 转储 v648 `Const ty=21 aux=`（重解析）vs .hat `@t648 = @null : Any` |
| 3 | **buildMember 的 length/size 路由错误**：recvTy 为簿记垃圾（`t\|FileUtils_readText`）时旧兜底 `recvTy=="String"\|\|""` 不命中 ⇒ 落通用 GetField 把字符串指针当对象读。且 `.size` 是集合属性式主流写法，recvTy 为空时误 strlen | 重建路由：真有同名字段→字段读；集合/Array→`Collections.count`；其余→strlen | fs 转储 v1626 `GetField args=1623,1624 aux=`；main 重定位 `toStr(t.length)` 缺 strlen → 修复后 0xC33 出现 strlen |
| 4 | **`localTypeOf` 首条目永不命中**：needle=`"\n"+name+"\|"`，函数内**第一个**登记的局部量在串首、无前导换行 ⇒ 定型必失败（R12 探针 main 第一个局部 `a` 的 `a.pathBuf` 走 GetField，第二个局部 `b` 正常——同名不同命的经典症状） | `localTypeAdd` 空表时先垫 `"\n"` | R12 fd5 转储：v1019 `@field(@t1007,…)`（a）vs v1028 `__field_get(@t1013,…)`（b） |
| 5 | **ISel 无 GetField 分支**：落到默认 Value 节点，**args 整体丢弃** ⇒ ①基址不进 DAG，发射读残留寄存器；②基址活跃区间终止于定义点 ⇒ RA 把同寄存器分给后续值，跨调用对象句柄被静默覆写（R12：实例 a 的寄存器被第二次构造覆写） | GetField → `__field_get(obj, 0)` 调用形发射（索引随 .hat 序列化已丢，0 为降级语义；正式路径是前端带真索引的 `Call __field_get`） | fd5 转储 n144 `GetField reg=rdi args=`（空）；同 build 下 b 的读取是 `__field_get args=138,152` |
| 6 | **双 bump 游标共用同一 heapArena**：`runtime_emitObjectAlloc` 用独立 `objBump`、strcat/`__list_alloc` 用 `heapBump`，两个互不知晓的游标各自从 0 切同一块内存 ⇒ 交叉使用必然互相覆盖。这是 §7.5「路径缓冲区首字节为 0」、§7.3「崩点随构建漂移」随机损坏家族的**总根因** | emitObjectAlloc 改用 `heapBump` + 8 字节对齐（新增 `X86Encoder.emitAndRI`） | R12→R13：`b.internal.c0` 随分配布局在 199/200/88 间漂移；统一后指针确定性可复现 |
| 7 | **FileOps_lseek 的 INVALID 哨兵 64/32 位错配**：`emitCmpRI("eax",-1)` 实际编成 `cmp rax,-1`（REX.W），而 Win32 API 只写 eax、rax 高位零扩展 ⇒ `0x00000000FFFFFFFF ≠ -1` 恒不命中 ⇒ 返回 +4294967295 骗过 `size<=0` 守卫 ⇒ malloc(4G) 零初始化越界（§8.4 只修了 mov 侧，cmp 侧漏了） | 新增 `X86Encoder.emitCmpEaxImm32`（3D id，无 REX.W），lseek 换用 | runtime 反汇编 0x1d48 `cmpq $-1,%rax`；R7 探针实测 `R.size=4294967295` → 修复后 `-1` 正常走守卫 |
| 8 | **io_read 返回字节数错拿 r9**（§8.5 遗留）：ReadFile 后 r9 已被冲 ⇒ `File.readText` 的 `read<=0` 守卫把成功读取当失败永远返回空串 | 改 `emitStackLoad("rax", -0x8)` 读 lpNumberOfBytesRead；当时的「布局雷」定性为本轮 #6/#5 家族，与本函数无关 | R15 探针 `D.rd=85`、b0='i' b1='m' 内容正确 |
| 9 | （配套）runtime obj 缓存 sidecar 只校验符号表 —— **改 runtime 代码不改符号时旧 obj 被继续链接**。本轮手工 `rm aura_runtime.obj*` 两次规避 | 留档：后续给 sidecar 加内容哈希 | #6/#7 修复首次未生效的排障耗时 |

### 10.2 验证矩阵（PhotonP68.exe）

| 项 | 结果 |
|---|---|
| fs 探针 | **EXISTS-OK + READLEN=4，rc=0 ✓**（上轮 0xC0000005；4≠85 见 10.3） |
| R2/R3/R4（单例方法 Long/String 参数、stringToBuffer 全克隆） | IDENT=41 / LEN=5 / COPY-OK 全 ✓ |
| io 探针 | fd=172 size=85 read=85 全 ✓（§8.2 的 main 作用域形态） |
| R15（类上下文 io 逐值） | fd=180 size=85 rd=85 b0='i' b1='m' 全 ✓ |
| tiny / fib(30) / bench_loop | tiny ok / 832040 / 124950000000 全 ✓ |
| native 探针 | FENCE/CPUID/atomic/ADD ✓；RDTSC 偶发 BAD（1/3，跨核 TSC 偏斜，非编译器缺陷） |

### 10.3 剩余阻塞点（下一轮首攻，复现与证据已备齐）

**缺陷 A：外层调用易失寄存器实参被内层调用求值冲掉（≥3 层嵌套调用实参）。**
`println(tag + " b" + toStr(i) + "=" + toStr(Memory.read(p+i)))`（4 层）第二次迭代
AV(WRITE) [0]（strcat 域）；2 层（`"READLEN=" + toStr(t.length)`）正常。
机制假说：emitCallArgs 顺序装载 rcx/rdx/r8/r9，途中某实参本身是调用 ⇒ 易失寄存器
被冲。修复方向：arg 节点的寄存器选非易失/先溢出，或调用形实参最后装载。
复现：`build/bisect/probe_fd8.aura`（dumpBytes 第二次迭代）。

**缺陷 B：类实例欠分配 16 字节（无字段空间）。** `__field_set/get` 寻址
`[obj+0x10+idx*8]`（16 字节 header），而 `<Class>__new` 只 `Memory_alloc(16)` ⇒
字段读写落在分配区之外、踩相邻分配。§7.2#6（单例 floor 128）的普通类版本。
推定后果：R13/R16 的「第二次构造 stringToBuffer 内容坏」（#1 构造的 field_set
写进 #2 的分配区）与 fs 探针 READLEN=4 截断（exists 前缀 → #2 构造 → 路径串坏 →
open 失败 123/ERROR_INVALID_NAME 级联）。
修复方向：`synthesizeCtors` 的 alloc 尺寸改 `classSizeOf`（16+8*字段数，
`buildFieldLayoutCache` 已算好）——注意先查为何探针自有类的字段偶发不进
classFields（fd8 首编 FileS 曾整体缺席，重编恢复，疑与 #9 缓存同族）。
复现：`build/bisect/probe_fd8.aura` / `probe_fs3.aura`。

**缺陷 C（轻）：空串字面量物化**。R16 中 `bufferToString` 对 maxLen<=0 返回 ""
后 `"".length` 读出 4——空串 @str 的内容/长度物化存疑，优先级最低。

### 10.4 G3 通路状态

stringToBuffer → File.init pathBuf → readText → **已通**（fs 探针 rc=0 证明
全链可走）；readText 返回内容受缺陷 A/B 影响仍截断。下一轮修 A/B 后，
AucLoader → **VM runner** 即可尝试端到端。

> 驱动构建命令（本轮实际使用）：
> `rm -f build/hat-vmhost/aura_runtime.obj*`（改 runtime 后必须）
> `seed/target/release/aura.exe build --aot aura/photon/aura/lang/compiler/photon/PhotonHatCompile.aura --output build/hat-native/PhotonP68.exe`
> 探针：`cp <probe>.aura build/hat-vmhost/current.aura && ./PhotonP68.exe && ./build/hat-vmhost/current.exe`

### 10.5 §10.3 缺陷 A/B/C 攻坚结果（2026-10-04）：全部修复，readText 端到端 85/85

> 驱动口径：`build/hat-native/PhotonP80.exe`。全矩阵绿：fs 探针
> **EXISTS-OK + READLEN=85（=文件真实长度）rc=0**；R2/R3/R4、fd5/fd6/fd8
> （类双构造、嵌套 concat 循环）、cmp 微探针全对；tiny / fib(30)=832040 /
> bench_loop=124950000000 无退化；io 全对；native 4/5（RDTSC 偶发跨核偏斜）。

#### 缺陷 A 真根因：RA 的整套机制在 seed-AOT 驱动下**从未工作过**

RegisterAllocator 直接读 `this.dag.instrs`（**跨对象 List 字段**）——种子 AOT
值模型的「跨对象字段绑定失真」家族（与 HirLowerer this 失真同族）：同一索引
经 `dag.instrOf()`（自对象方法读）得到 `label`，经字段读得到垃圾（`"0"`）。
后果链：buildBackEdges 的 label 采集恒 0（labels=0 → edges=0）→ 回边扩宽
从不运行；buildNodeSpans 的 touchInstr 扫空 → 活跃区间恒 [-1,-1] → 干扰图
为空 → **跨调用/跨块活值被随意复用寄存器**。这就是 §10.3-A「嵌套调用实参
被冲」（dumpBytes 循环迭代 2 的 tag 寄存器被体内临时覆写）与「第二次构造
stringToBuffer 内容坏」的共同根因。修复：RA 三处（buildBackEdges /
buildNodeSpans / computeLiveAtEnd→getSuccIndices→buildLabelIndexMap）全部改
`dag.instrOf(i)` / `dag.instrCount` 方法访问。修复后回边/区间真实生效，
fd8 双迭代字节数据全对。

#### 缺陷 B 真根因：buildFieldLayoutCache 三层叠加 ⇒ classSizeOf 恒 16

1. 字段数回填用 `fwSlotCount(clsLayoutTys)-1`，而 `clsLayoutTys` 是**变长类型
   名行**（`|Long|` 长 7），`length/9` 恒 0 ⇒ 字段数恒 0；
2. `clsLayoutTotal/Size` 赋值缺**前导 `|` 分隔符**（fwGet 从偏移 1+idx*9 读）
   ⇒ 就算回填对了也读不出来，`classSizeOf` 恒落默认 16；
3. 记录循环的 gap 扩槽用从不增长的 `clsOwnCount` 当行宽 ⇒ idx≥2 的记录全部
   插入 idx−1 个**空槽** ⇒ 类型错位（fs 闭包 Allocator 11 字段被插成 21 槽）
   ⇒ `fieldTypeOfIndex`/默认值取错 ⇒ 单例 getter 的字段默认值表达式引用到
   无关调用（`Console_writeStdout` undefined symbol）。
修复：回填改「分隔符计数 −1」；布局行补前导管道符；删除 gap 扩槽改无条件
追加（类内 idx 连续且 CSV 升序，追加即正确位）。修复后 FileS/File alloc
24/32，实例字段读写全部落在分配区内。

#### 缺陷 D（本轮新发现）：ISA ICmp Case-1 的永真子句

`strToInt(rhsVal.aux)==0 || rhsVal.args.size==0` —— Const 节点 args **恒为
空** ⇒ 任意非零常量的 `==`/`!=` 全部命中 Case-1 被编成「与 0 比较」：
`got != 104` 恒真（got=104 实测打印 A-BAD）。`<`/`<=` 不走该路径，故
fib/bench_loop 幸免。修复：删去永真子句，零值判定只看 `strToInt(aux)==0`
（Case-1 的真实用户 `ICmpNe(streq结果, 0)` 与 null 常量均不受影响）。

#### G3 通路状态

`FileUtils.readText` → stringToBuffer → open/lseek/read/bufferToString 全链
**端到端正确（85/85 字节）**。AucLoader → VM runner 的文件侧前置已全部就绪，
下一轮可直接对接 `.auc` 装载。

> 复现/回归脚本（本轮沉淀，`build/bisect/`）：
> `probe_obj1/2/3`（单例方法参数、stringToBuffer 克隆）、`probe_fd5/6/8`
>（类双构造、嵌套 concat 循环）、`probe_cmp`（==/!= 常量比较矩阵）、
> `probe_fs/fs3/io/native`。驱动重建后逐个
> `cp <p>.aura build/hat-vmhost/current.aura && ./PhotonP80.exe && ./current.exe`。

### 10.6 G3 冲刺（2026-10-04 续）：Photon AOT 编译纯 Aura VM runner —— 编译打通，执行残余缺陷定性

> 命令：`cp examples/compiler/auc_vm_runner.aura build/hat-vmhost/current.aura &&
> PhotonP80.exe`。**首次将完整纯 Aura VM 闭包（VmRunner + Vm + AucLoader + std，
> .hat 1.34MB / 产物 8.5MB）送通 Photon 管线并链接成功**。

#### 执行状态：装载阶段段错误（残余值流缺陷，已定性未修）

- 种子参照（`seed aura.exe run <prog> --vm=aura`，元循环宿主）：
  hello world 正常打印 `Hello, World!` rc=0；FileUtils.writeText 型 guest
  报 `Unknown function: Memory.alloc`（extern 对象不在该通道服务面 —— 该
  通道 guest 只支持纯计算 + println）。
- Photon runner 跑同一 `build/vm/current.auc`：装载阶段 0xC0000005。
  minidbg 帧回溯 + runtime 符号映射：崩点 = **runtime charCodeAt，入参
  (s=NULL, i=0)**，caller = AucReader_stringToBuffer 的写字符串循环 ——
  即 readFromFile → readFileToBuffer → stringToBuffer 的 path 参数在
  **6 层跨对象调用链**上被置 NULL，且该层的 `s == null` 判空未拦住
  （判空本身语义正确，NULL 是在更上游的寄存器/槽位层面丢的）。
- 与 §10.5-A（dumpBytes 的 tag 丢失）同族：**深层调用链上的字符串参数
  仍会在特定布局下丢失**。RA 机制已激活（§10.5），但巨型闭包
  （Vm_dispatch 数千节点/跨块 PHI 密集）仍有个别干扰/区间遗漏实例。
- 排除项：非 arena 上限（扩到 32MB 同崩，保留 32MB 配置无害）；非
  .auc 格式漂移（Oct2/Oct4 两代 .auc 同症状）；非 guest 侧 extern
  缺失（纯 println guest 同样挂）。
- 另记：入口模块调用 `FileUtils.fileSize(...)` 被降级为裸名 `size`
  （undefined symbol）—— resolveMethodSymbol 对入口模块静态调用的一处
  遗漏，独立缺陷待修。

#### 下一步（G3 剩余）

1. 以 `AucReader_stringToBuffer(path=NULL)` 为新向导探针（等价于上轮
   fs 探针的角色），沿 6 层链逐层转储参数定位丢弃层；
2. 修复后预期通路：loadModule 解析 → VM 数值模式解释 hello.auc → 与种子
   参照 diff → vm_p2 测试集。

### 10.7 隐式 this 调用修复（2026-10-04 续Ⅱ）：VM runner path 丢失层根因消除

> 驱动口径：`PhotonP84.exe`（含本修复）。沿 6 层链的 null 探针（L1-L6）
> 定位：path 在 **readFromFile → readFileToBuffer 的单一调用边界**上丢失。

#### 根因：隐式 `this` 调用不前置接收者 ⇒ 被调方实参整体错位一位

`AucReader_readFromFile` 内的裸调用 `readFileToBuffer(path)`：HIR kids 只有
实参（无接收者 kid），`buildCall` 组出的实参 = (path, memtoken)；而被调方
是类实例方法，形参表首项是 `this` ⇒ 被调方读到 `this`=path、`path`=
memtoken（垃圾/0）⇒ 6 层链上任何一层有裸调用即连环污染。显式接收者调用
（`reader.readFromFile(path)`）经 lowerCall 把接收者降级成 kids[0]，实参
循环已将其作为 arg0 传出，故 loadModule → readFromFile 一层正常。
**种子 AOT 的 emit.rs 有对应处理，photon 后端漏了** —— 与 §10.5 同批
「photon 后端补齐种子语义」家族。

修复：`resolveMethodSymbol` 规则 2（隐式 this）命中时置
`lastResolveImplicitThis` 标志，`buildCall` 据此把 `varMap` 的 `this` 值
前置到实参首位；单例方法（无 this 形参）与 skipFirst 形态不补；
this 存在性用 `mapVidOf` 严格判定（`varMapLookup` 未命中会创建 const-0
并返回其 id，永不为负，`>=0` 守卫失效——实测 this 探针 main 的全部调用
被前置 this=0 ⇒ println(NULL)）。

**验证**：`probe_this`（类内裸调 inner(21)/inner2/嵌套，期望 100）：
修复前 R=16（错值），修复后 **R=100 / THIS-OK** ✓。VM runner 装载链
null 探针全 false（path 完好传入 stringToBuffer）✓。

#### 当前状态与残余

- VM runner（Photon 版）装载 path 传递已修复；执行 hello.auc 仍**挂死**
  （readFileToBuffer 之后、guest 输出之前，连续第二条 println 处静默
  死循环；直接控制台与重定向同症状）。残余缺陷方向：块标签进 4 位数
  （bb2673+）后的 CFG/标签解析，或 buildBoolToString 三元式的分支布局。
- **probe_obj3 回归**：字节复制全对（b0-b4="hello"、j=5）但 `ok` 标志
  误判 ⇒ COPY-BAD。循环携带 flag + 分支内赋值的 store/phi 路径，与本轮
  隐式 this 无关（P80 复测同样 BAD），列入残余清单。
- 全量回归（P84）：probe_this/obj1/obj2/cmp/fd5/fd6/fd8/fs(READLEN=85)/
  io 全 ✓；fib(30)=832040、bench_loop=124950000000 无退化 ✓。

> 诊断工具沉淀：`build/bisect/probe_this.aura`（隐式 this 矩阵）、
> `probe_cmp.aura`（==/!= 常量矩阵）、`vmrun_diag.aura`（runner 装载
> 诊断）。null 探针法（沿调用链逐层 `path == null` 打点）在本轮再次
> 证明是最快的多层参数丢失定位手段。

### 10.8 VM runner 挂死画像（2026-10-04 续Ⅲ）：100% CPU 紧循环，非死锁

> 工具新增：`build/bisect/sampler.py` —— 以调试器身份启动目标，SuspendThread +
> GetThreadContext（**真实 x64 CONTEXT 布局**）定期采样 RIP + ReadProcessMemory
> 取系统调用号 + EnumProcessModules 判定所属模块。注意：`minidbg.py` 的
> CONTEXT 布局存在**寄存器名错位一格**（其 "Rip" 实为真实 R14、"Rbp" 实为
> 真实 Rsp）——但帧回溯取的是栈上真返回地址、且崩点结论均经静态反汇编交叉
> 验证，历史结论仍成立。

**画像**：Photon 版 VM runner 装载 .auc 挂死 = **单线程 100% CPU 紧循环**
（10 秒墙钟 = 10 秒 CPU，单线程）；采样 RIP 在 ntdll/系统模块间移动
（伴生系统调用）⇒ 非 死锁/IO 阻塞，是**编译产物里的纯计算死循环**，
位置在 stringToBuffer 之后的装载/解析段（两个连续 println 之间，
伴随 `toStr(bool)` 三元式的分支块）。

**头号嫌疑**：`labelToIndex` 的标签双编号混用——被调方把发射标签
"bb<blockId>"（LIR 块 id，如 bb2673）映射进 `labelIndexIdxs`（按指令流
顺序的**入口点序号**表）。若 LIR 块全部带终结指令且创建序=发射序，两套
编号一致；任何一处不成立（无终结符块/创建序变化）即整体错位 ⇒ liveness
后继错误 ⇒ 干扰图漏 ⇒ 寄存器复用误判（循环计数器被踩 ⇒ 紧循环）。
次嫌疑：buildBoolToString 三元式在 4 位数块标签下的分支布局。

**修复切入（下轮）**：① labelToIndex 改用 `labelNames/labelInstrIdxs`
（发射标签名字符串表）做映射，两套编号合一；② sampler.py 采样配合
`AURA_PHOTON_DEBUG_HIR=1` 的块标签转储，验证三元式块的目标标签。

### 10.9 本轮（10-04 Ⅲ）全量验证矩阵（PhotonP84.exe）

| 项 | 结果 |
|---|---|
| probe_this（隐式 this 修复） | R=100 / THIS-OK ✓ |
| probe_obj1/obj2 | IDENT=41 / LEN=5 ✓ |
| probe_obj3 | 字节复制全对（"hello"/j=5）；`ok` 标志仍误判 COPY-BAD（残余②） |
| probe_cmp（==/!= 常量矩阵） | 全对 ✓ |
| probe_fd5/fd6/fd8（类双构造/嵌套 concat） | 全对 ✓ |
| probe_fs | EXISTS-OK + READLEN=85 ✓ |
| probe_io | fd/size/read=85 全对 ✓ |
| tiny / fib(30) / bench_loop | tiny ok / 832040 / 124950000000 ✓ |
| **VM runner（Photon 版）** | **编译 ✓（.hat 1.34MB）；执行挂死（§10.8，残余①）** |

### 10.10 cap=0 铁证 + phi 容量修复（2026-10-04 续Ⅳ）

> M1-M6 精细标记（split runner）定位：stringToBuffer 完整成功（len=21、
> loop done、返回有效 buf），**挂点在其后的读循环：`FileOps.read` 恒返
> n=0**，`n <= 0` 分支不生效，50 次迭代后触发兜底退出（本轮临时加的
> runaway guard）——原实现的挂死即此无限循环。

**铁证链**：句柄有效（`lseek(fd,0,2)=52684` 精确匹配 fib10.auc 大小）⇒
ReadFile 成功（eax≠0）但读 0 字节 ⇒ **count=0** ⇒ `cap=0`——`var cap:
Long = 65536` 的初值在循环携带中丢失。与 probe_obj3 的 `ok` 标志同族：
**循环携带变量的 phi 初值 MOV 丢失**。

**修复（正确性加固）**：`phiMovAdd` 超容（≥32 条/块）旧实现**静默丢弃**
溢出的 PHI-MOV 记录 ⇒ 大闭包中部分块的循环变量初值 MOV 整批丢失。改为
超容时翻倍扩容重建扁平表。验证：probe_this/obj2/fs/fib 全绿无退化；
obj3 COPY-BAD 仍在（同族另一实例）。

**执行状态（P86 干净重建）**：VM runner 编译 ✓；运行 hello/fib10.auc
仍挂死（读循环问题解除后暴露下一层——解析或解释器路径的后续实例）。
残余缺陷清单（按证据强度）：
1. 循环携带 phi 初值 MOV 丢失（phiMov 容量已修，仍有实例——待查
   pass② 入边匹配的 (b) 兜底「按序分配」在多 PHI 时的错配）；
2. probe_obj3 `ok` 标志（循环内分支赋值 + 循环外读取）；
3. 入口模块 `FileUtils.fileSize` → 裸名 `size`（resolveMethodSymbol
   入口模块静态调用遗漏）。

### 10.11 隐式 this + if-merge Phi 双修（2026-10-05）：ok 标志 ✅，runner 编译再进一段

> 驱动口径：PhotonP89/P91。**probe_obj3 的 ok 标志已修复**（ok 全程正确跟踪，
> COPY-OK 达成——原探针期望值 104+j 本身有误，改为按 "hello" 实际字节验证）。

**修复一（重大）：无 else 的 `if (c) { x = v }` 在 SSA 层丢失**——
`insertPhisAtMerge` 旧实现只为「两分支都赋值」的变量建 Phi，且 buildIf 的
`varMap = varMapAfterElse` 让 then 分支的 varMap 泄漏：`ok` 在 SSA 层
**永久指向 false 常量**（分支执行与否无关）⇒ 条件赋值全部失效。重写：
合并名单 = then ∪ else；每变量建 merge Phi（then 出值/else 出值，无 else
时 else 出值 = before 值）；varMap 更新到 Phi id。名单收集用扁平 CSV +
charCodeAt 扫描（**禁用带 from 的 indexOf——seed AOT 下失效导致死循环**，
另加 unionCsvContains/Count/Name 三助手）。

**修复二：`break`/`continue` 语句实现**——SsaBuilder 完全没有 HirBreak/
HirContinue 分派（grep 零引用），`while(true){...break...}` 的 break 被
静默丢弃 ⇒ runner 的 ReadFile 读循环永不退出（§10.8 的 100% CPU 挂死）。
实现：buildBreak → Br(loopStack 栈顶 endBlock)；buildContinue → Br(condBlock)
+ continue 点值登记（loopContRecs，buildWhile 回填时追加为 Phi 额外入边；
多 continue 点退化为共享末值，已知不精确）。

**回归（P89）**：this/obj1/obj2/cmp/fd5/fd6/fd8/fs/fib/loop 全绿 ✓。

**残余（诚实清单）**：
1. P91 驱动编译 stdlib 时段错误：@PH4 phi i@（insert/i/sign/n 函数）之后
   ——本会话新增代码里的又一 seed-AOT 值模型实例（新字段 loopContRecs/
   loopPhiVarsStack 或 unionCsv 扫描），每次修复移动布局暴露下一处；
2. VM runner 执行仍挂（解析段后续实例，同族）；
3. probe_obj3 的 COPY 判定探针已改正（原期望值错误）。

**方法论沉淀**：seed AOT 下新增编译器代码的红线——①禁 arrayListOf+add
（get 返 null）；②禁带 from 的 indexOf（死循环）；③禁未声明字段
（宽松编译读垃圾）；④字符串字面量内 `\n` 转义勿被工具写成真实换行；
⑤临时 MirValue 链式字段赋值丢失写回（必须 pv 局部中转）。本轮四条全部
实际踩中。

### 10.12 break/continue 语义修复 + 循环头 phi 残余（2026-10-05）

**修复一：循环栈压栈格式**。buildWhile 原来只压 endBlock 一项——
`loopStackContinueTarget` 找不到逗号退回栈顶=endBlock ⇒ **continue 变成
break**（probe_break: cont 后整个循环提前退出）。改为「condId,endId」
成对压栈；break 取第二项、continue 取栈顶（loopStackBreakTarget/
ContinueTarget 语义对调修正）。

**修复二：无 else if 的 SSA 丢失**（§10.11 修复一的跟进）——
insertPhisAtMerge 重写 + buildIf varMap 重置为 before。验证：probe_obj3
**COPY-OK 达成**（ok 标志全程正确跟踪）。

**修复三：pass② phi 入边匹配的传递前驱闭包**——phi 入边值的定义块
常为目标的**间接**前驱（if/汇流块分隔）。「定义块==直接前驱」永远失配
⇒ 全落兜底乱配。新增 pass1.5 传递前驱闭包（位集不动点迭代），pass②
精确匹配改 isTransitivePred。

**修复四：PHI-MOV 缺失静默跳过**——emitPhiMovsForBlock 里
lookupNode(phi/incoming)==-1 时 MOV 被跳过。改为**就地发射**缺失的
phi/incoming 节点（selectValue 幂等）。诊断工具：AURA_PHI_DIAG=1 输出
每函数 emitted/missing 计数（split 348/0 ⇒ 不再缺失）。

**修复五：嵌套三元表达式在 seed AOT 驱动下段错误**——
`if (c) { "a" } else { if (y) { b } else { d } }` 形态（buildContinue
栈顶提取）编译后驱动启动即崩。改写为显式 if 变量中转后通过。

**新残余（精确定位）**：continue + 循环携带变量组合——`while(true){
i=i+1; if(i==2){continue}; total=total+i }` 运行期：循环头恒读
i=0/total=0（入口 MOV 值），循环体计算正确（body i=1）但 latch MOV
不落地 ⇒ 无限 i=1。而**无 continue 的循环 latch MOV 正常**（n=3 ✓）。
嫌疑：continue 块的 phi MOV 与 latch MOV 的执行序/槽位冲突。采样器+
逐块反汇编工具已备，下轮主攻。

**回归（P115）**：this/obj1/obj2/**obj3 COPY-OK**/cmp/fd5/fd8/fs/
fib(30)=832040 全绿 ✓。VM runner：编译 ✓、装载路径 ✓（FD/大小/读循环
轨迹正常）、解释执行待 continue-phi 修复后重测。

### 10.13 continue 最小复现锁定（2026-10-05 收口）

**最小复现**（`build/bisect/probe_cont_min.aura`，20 行）：
`while(true) { i=i+1; if(i==2){continue}; total=total+i }` 运行期
total 恒读旧值（head 0/0 循环，body 计算正确但不落地）。二分确认：
- while+break：✓ 正常（i/total 递增、break 退出）；
- 加 continue 一行：✗ total 卡旧值。
continue 块的 Mov 值（定义于 continue 块）已正确匹配 continue 前驱
（pmAdd blk 记录可见），pass②/闭包/容量/ MOV 发射全修——
**残余在 Add 结果 home 与 varMap 解析 home 的错位**（seed AOT 值模型：
跨 continue 边的循环携带变量，Add 节点与后续读取的 home 关联断裂）。

**已修复且验证的本会话累计**：隐式 this 前置、if-merge Phi（单分支赋值）、
break/continue 语句、循环栈成对压栈、pass② 两阶段+传递前驱闭包+MOV 就地
发射、phiMov 超容扩容、regOfNodeRaw 颜色优先、indexOf 死循环、嵌套三元
段错误、判空极性、HatParser @null、length/size 路由、localTypeOf 首条目、
GetField args、双 bump 游标、lseek 32 位、io_read 字节数。
回归基线（P115）：this/obj1/obj2/**obj3 COPY-OK**/cmp/fd5/fd8/fs/fib 全绿。

### 10.14 continue 三重缺陷修复 + VM runner 通道打通（2026-10-06）

**本轮主线：continue 缺陷彻底关闭（probe_cont_min `end total=4` ✓）**，
VM runner 通道从「编译后挂死」推进到「编译 ✓ 链接 ✓ 装载 ✓ 进入解释段」。

#### 一、continue 缺陷的三重根因（全部有实证）

1. **loopPhiVarsStack 栈顶段提取只取首行**（SsaBuilder.buildContinue）：
   层间以空行分隔（压栈为 `entries + "\n" + 旧栈`），旧代码取到首个
   `\n` 即止 ⇒ 多循环变量时只有第一个变量拿到 continue Mov（实测
   probe_cont_min：i 有 Mov、total 没有）。
2. **1 参 `substring(from)` 在 seed AOT 下返回空串**：落到双参 C 符号
   `aura_string_substring`、`to` 为垃圾寄存器 ⇒ buildWhile 回填处
   `rec.substring(p2 + 1)` 取出空 ⇒ phi 额外入边静默丢失（插桩实证：
   `[contAPPEND] phi=1030 args=1026,1063,` 尾部空）。修复：调用点全部
   显式 2 参 `substring(p2 + 1, rec.length)`。seed 侧 1 参分派到
   `substringFrom` 的方案已实证会触发新 seed 宿主的漂移踩踏（P121 驱动
   isBoolExpr/splitStr 路径 String 收到装箱 1 即崩），暂缓启用（emit.rs
   留注释），待 ra1+canary 主线后再放开。
3. **pass② 传递闭包匹配被回边全连通击穿**：循环内所有块经回边连成
   一家，continue 前驱按 args 顺序抢走 before(入口值)。修复：分层匹配
   ——(a1) 严格 def块==前驱 全局优先 → (a2) 传递闭包 → (b) 尾部兜底。

**验证**：probe_cont_min 输出 `end total=4`（0+1, skip, +3, +4）与 VM
语义完全一致；probe_break BREAK-OK；.hat 三入边 phi
`@phi(@t1027, @t1053, @t1059)` + continue 块 `@Mov` 直通全链路正确。

#### 二、环境修复：rust 种子二进制过期

`rust/target/release/aura.exe` 停在 10-02（早于 a365913 的
`__is_tagged_int` 内建）⇒ P119 起驱动构建全部失败
（`call void @__is_tagged_int`）。`cargo build --release --features llvm
--manifest-path seed/Cargo.toml`（必须 --features llvm，codegen::aot 是
feature-gated）+ 拷贝到 rust/target/release 恢复。教训：迭代环第一步
先核对种子二进制与源码的时间戳。

#### 三、VM runner：编译通道打通（编译 ✓ 链接 ✓ 装载 ✓）

- **入口口径修正**：runner 闭包必须用 `build/bisect/vmrun_diag.aura`
  （文件级相对 import VmRunner.aura ⇒ 递归闭包 7.8MB/3387 fn）；
  `Main.aura` 的点分 import 闭包不全（1.36MB、半数编译器类缺席）。
- **溶解对象 getter 缺口**（MirUtils_getInstance）：HirLowerer 对部分
  `object X` 以「方法限定名 + 体提升」输出、对象声明节点不进 HIR ⇒
  buildClassIndex 四类扫描捕不到 ⇒ 注入的 `X_getInstance()` 调用没有
  getter。SsaBuilder 侧补登记/补合成的多次尝试在新 seed 宿主下被漂移
  踩踏吞掉（探针打印时灵时不灵，数值全部失真——vn/len/strcmp 全不可
  信）。**落地解法 = 驱动侧两段式**：①前端跑完 → ②python 对 .hat 文本
  补 `@fn X_getInstance` Memory_alloc 桩 → ③`AURA_HAT_SKIP_FRONT=1`
  （本轮实现，原先只有文档）重跑后端。三段式脚本：
  `build/bisect/build_runner.sh`。
- **装载段**：AucReader.readFileToBuffer 的「按块读 + 扩容」双循环
  （buf/cap/total 三循环携带变量组成 phi 环 loop_phi ↔ if-merge_phi）
  在 runner 里入口 MOV 错位（cap=0 ⇒ read(count=0) 恒空）。改为
  **lseek 尾探定大小 + 单次 read**（.auc 本地定长文件语义等价），phi 环
  消失，装载通过（fd/size/内容全对）。
- **当前边界**：runner 进入解释执行段后 segfault（/tmp 输出
  `exists=true` 后崩）。这是下一层（解释器/native 边界），与前述全部
  修复无关的新前线。

#### 四、mergePhiHomesAfterRA 停用

`selector.mergePhiHomesAfterRA()`（上轮未提交改动）把 phi home 改写到
「首个精确匹配入边」的 home——入边是调用结果/大常量时即易失寄存器，
循环携带变量被钉死其上（runner 里 buf 读成装箱奇数、cap=0）。P115 全绿
基线本就不依赖它 ⇒ 两个调用点已注释停用（InstructionSelection 内函数
保留，带 Const 守卫的版本留档）。探针矩阵 11/11 复测无退化。

#### 五、回归与产物

- 探针矩阵（P142/P143 驱动）：cont_min/break/this/obj1/obj2/obj3
  COPY-OK/cmp/fd5/fd8/fs READLEN=85/fib(30)=832040 **11/11 ✓**
  （regress.sh 里 fd5/fd8 的断言写错：缓冲区存的是路径串字节
  c0=98='b'、b4=100='d'，手工核对为正确值）。
- runner 产物：build/hat-vmhost/current.exe（33.7MB，
  `AURA_HAT_AURA=... PhotonP143.exe` 两段式构建）。
- 诊断资产：`build/bisect/regress.sh`（探针矩阵）、
  `build/bisect/build_runner.sh`（runner 三段式）、
  `build/bisect/t_tag.aura`（种子内建 16 行复现）。
- 下轮前线：①解释执行段 segfault（runner 交互
  ip/funcTable/字符串互操作的首个崩溃点，AURA_CRASH_DUMP 已备）；
  ②新 seed 宿主漂移踩踏主线（ra1 + canary 定位首次越界写）——本轮
  多次探针失灵（打印不触发、数值失真）皆是它的表现；③MirUtils 为何
  被 HirLowerer 溶解（HIR 侧真根因，当前由 hat 桩兜底）。

**会话累计**：上轮 17 项 + 本轮 8 项（栈顶段提取、1参 substring 调用点
治理、pass② 分层匹配、种子重建流程、SKIP_FRONT 实现、hat getter 桩、
装载单次读、mergePhiHomes 停用），continue 阻塞问题关闭。

### 10.15 P0/P1 攻坚：五个根修落地 + 装载链推进 + 剩余崩点收窄（2026-10-06 续）

**P1-4 关闭（真根因 = writeText 静默失败，「MirUtils 溶解」系幻影）**：
- `File.writeText` 一次性 `stringToBuffer(7.8MB)` 在前端吃满内存后分配
  失败（返回 0）⇒ **静默 return** ⇒ .hat 永不落盘（mtime 更新、内容陈旧）
  ⇒ 后端一直在编译 P123 时代的陈旧 hat。根修：**分块写（64KB）** + 失败
  响亮报错（File.aura）。真闭包 1.3MB/924 fn/**根本不含 mir 包**——
  「MirUtils_getInstance 未定义」是编译陈旧 hat 的幻影。
- 探针证伪红线①在 AOT 载体下成立：ArrayList<Class>+add 500 元素 0 丢失
  （t_al.aura）——该红线只适用于 seed VM 解释器。
- P148 起实现 **AURA_HAT_SKIP_FRONT**（原先只有文档）+ python 补桩
  （build/bisect/build_runner.sh 三段式）作为 .hat 后处理通道。

**P1-3 四个真实根修（全部有金属级证据）**：
1. **Unary Not 不在任何 ISel 分派** ⇒ `!x` 节点有 home 无指令 ⇒ 读到
   未初始化槽（反汇编实证：call 返回值存 -0x50，分支测 -0x58）。修复：
   `!x ≡ (x==0)` → cmp+sete（InstructionSelection selectValue）。
2. **裸名字段写入丢失**（buildAssign）：`data = …` 一律 Store+varMapAdd
   ⇒ 字段本体从未写入（hat 实证 readFromFile 零条 __field_set）⇒ 跨方法
   读恒 0。修复：varMap 未命中 + curClass 字段 ⇒ emitFieldSet（读侧
   buildVar 早有对称实现，补齐写侧）。
3. **遗留 Store 的 [rbp-8] 共享槽写入**：emitMovStore 对空 mem 落
   parseStackOffset("")= -8 ⇒ 每次裸名赋值踩共享兜底槽。修复：
   emitMovStore 空 mem 不发射（SsaBuilder 侧 Store 保留——直接删除使
   cont_min/obj2 等 10/11 探针崩坏，说明存在未确认消费方，折中见码内
   注释）。
4. **P1-5 完成**：22 处 1 参 `substring(from)` 全部转显式 2 参
   （FfiAot/HatParser/HirSerializer/Emit/Auz/Package/Type），编译器内
   清零（seed 侧 substringFrom 分派维持禁用，见 §10.14）。

**新工具与流程资产**：
- **rsp-SIB 编码缺陷修复**（X86Encoder）：emitStoreMemDisp32/
  emitMovMemDisp32 对 base=rsp/r12 缺 SIB 字节 ⇒ 野生地址读写
  （VEH handler 即因此自崩）。这是编码器级的真实缺陷修复。
- **photon runtime VEH 崩溃转储**（emitVehHandler + 数据符号 +
  BUILD_ID 缓存键）：0xC0000005 时 RIP+访问地址 2×u64 落 stderr。
  **尚未端到端点火**：懒安装点（println/Memory_alloc）两试均因编码器/
  栈布局问题失败，本轮摘除待下轮以显式初始化调用启用。
- **BUILD_ID 缓存键**：aura_runtime.obj 以 RUNTIME_SYMBOLS+BUILD_ID 做
  缓存键，发射器变更必须递增（否则陈旧 obj 复用——VEH 即被吞）。

**当前边界（诚实清单）**：
1. cont_min 探针在 P156 下段错误且无输出；**P143 同一二进制 17:57 绿、
   22:45 崩** ⇒ ASLR 依赖的 heisenbug 实锤，本轮修复未收敛完。
2. 回归矩阵 10/11 红（本轮引入，须下轮二分收敛；嫌疑集中度：bare-field
   写修复 > Not 分派 > emitMovStore 守卫的三者交互）。
3. runner 装载链在真闭包下推进到 .auc 解析段（pos≈63/52684）后崩。
4. P0-2（漂移踩踏主线）：本轮 Store 踩踏修复是其最强候选根因之一，待
   回归转绿后验证。
5. VEH 点火：换用「wrapper main 显式调用」的安装点即可绕开热路径问题。

### 10.16 P0-1 崩点锚定 + VEH 工具链全线点火（2026-10-06 续Ⅱ）

**VEH 工具链修复并点火成功**（三轮编码器/栈修复后）：
1. SIB 修复补丁自身漏写 SIB 字节（writeByte(0x24)）——补上；
2. handler 栈对齐：入口 rsp%16=8，sub 0x50 后仍 8 ⇒ 内部调用不对齐——
   改 sub 0x58；
3. install 标记读取偏移错位（crashBuf+48 vs +0）——对齐后 "VEHI\n" ✓。
   **端到端验证**：Memory.write(0,37) 探针 → stdout 落 16 字节记录
   rip=0x7ff740dc31a2 acc=2 ✓。新增 `veh_install` runtime 符号（Aura
   wrapper 首行裸名调用即可安装）+ crash_catch.py（调试器速崩捕手）+
   /DYNAMICBASE:NO 重链技巧（固定基址 ⇒ RIP 直接减基址得 RVA）。

**P0-1 崩点锚定（runner 装载链）**：no-ASLR 重链 + VEH ⇒
`rip=0x14002154d acc=0x2`——反汇编 = **runtime strlen**，入参 = 0x200。
0x200 = 节点 id/memtoken 量级 ⇒ **readMagic 的 result phi 在循环退出后
被写入 0x200（错误的入边值）**——正是 P1-3（phi 入边错配）在 loader
本体的活体实例。修复方向（下轮首步）：重开 pmAdd 打印（P123 清理时
移除）核对 readMagic 两个 phi 的 pass② 配对；嫌疑=闂锁 MOV 把 memtoken
或节点 id 写进了 result 的 home。

**回归状态**：P168 全量 11/11 实质绿（fd5/fd8 两FAIL为 regress.sh 断言
错误——字节 98='b'/100='d' 经 od 核对为正确值，脚本已修正）；cont_min
在 P168 下 end total=4 ✓（P156/P157 的崩坏为布局敏感 heisenbug，P168
构建消失）。回归脚本断言已修正。

**下轮清单**：①重开 pmAdd 定位 readMagic 的 phi 配对（P0-1 根修）；
②Store 消费方确认（emitMovLoad 空 mem 读 -8 的通道是否存在）；
③VEH filter 恢复 0xC0000005 过滤（当前全码转储）+ handler 进字节级
复核（SIB 修复后未再复核其余指令）。

### 10.17 P1-3 终极根因关闭 + 装载链推进到 .auc 解析深段（2026-10-06 续Ⅲ）

**P1-3 终极根因（pmAdd 实证 + 双层修复）**：
pass② 的 phi 配对**完全正确**（pmAdd：blk=2800 phi←entry、blk=2806
phi←latch 全对）——真正根因在**类型链**：
1. `SsaBuilder.buildWhile` 的循环 phi 类型**硬编码 0（Int）** ⇒ String
   循环变量（`result = result + …`）的 phi 被定型 Int ⇒ ISel
   `emitArith` 的 `isString(Add.type)` 判定失败 ⇒ **字符串 concat 被编译
   成整数指针加法**（addq）⇒ 垃圾值。修复：phi 类型继承入口入边值类型。
2. **兜底**：emitArith 的 strcat 判定增加操作数类型回退
   （LHS/RHS 任一 isString 即 strcat）——防御 phi/add 类型链上游继续漏。
3. **String null 常量物化**：`var result: String = ""` 的入口值被自举
   前端降级成 `Const aux=null`，emitConst 把字面文本 "null" 当字符串内容
   入 .rdata ⇒ 拼接出 "nullAURA" 魔数。修复：String+null ⇒ 空串。

**效果**：runner 装载链从「[M2] 后即崩」推进到**深入 .auc 解析段**
（readMagic/magic 校验全通，崩在后续 parse 的 streq 收到垃圾指针 2——
同一家族的下一实例）。

**VEH 工具链最终形态**（全部验证 ✓）：
- handler（SIB 修复 + 栈对齐 0x58）+ `veh_install` 显式安装 + "VEHI"
  安装标记 + WriteFile 落 16 字节 rip/acc 记录到 **stdout**（handle -11）；
- crash_catch.py（调试器速崩捕手，含 imagebase 打印）；
- /DYNAMICBASE:NO 重链 ⇒ no-ASLR exe 上 RIP-0x140000000 = RVA 直读。
- ⚠️ 工具链注意事项：SKIP_FRONT 跳过前端 ⇒ 用户 obj 不重编——**改前端
  （SsaBuilder/ISel）后严禁用 SKIP_FRONT 验证**（本轮曾据此误判数轮）；
  VEH 记录落 stdout，须 od 检查（cat 看不到二进制字节）。

**下轮首步**：VEH handler 增强（追加 [rsp] 返回地址 2×u64）⇒ 定位
streq(2) 的调用函数 ⇒ 沿同一循环修复。当前崩点：rva 0x21594
（runtime streq 的字节比较环）， acc=2（野指针值）。

#### 10.17 追记：VEH 增强转储 + streq 现场的栈损坏定性

VEH handler 增强后（追加 [Rsp]/[Rsp+8] 返回地址转储，记录 16→32 字节），
no-ASLR 重链实测装载链崩溃记录：
`rip rva=0x215b7（runtime streq 字节比较环）acc=0x2`、
`[Rsp]=0x15e5a8`——**0x15e5a8 指向 readMagic 体中部的 mov 指令（非任何
call 的返回点）** ⇒ streq 是被损坏的 `ret`「返回」进去的：**栈上保存的
返回地址本身已被踩**。定性：深层内存踩踏（值模型/栈损坏家族，即 P0-2
主线），非单一 phi 缺陷。

装载链累计推进：[M2] → readMagic ✓ → 魔数校验 ✓ → 深入 .auc 解析段
（readStr/readConstValue 区域的 streq）→ 栈损坏崩溃。

下轮路线（按文档 P0-2 主线执行）：
1. **canary 定位首次越界写**：AURA_CRASH_DUMP 的 C 侧已有 ra1 追踪；
   photon 侧可在 emitPrologue 统一插入栈帧 canary（保存的 rbp/r12 槽
   写入 0xCCCC 哨兵，epilogue 校验）——首次哨兵破坏即报写入者 RIP。
2. 或先攻 emitCall 的「调用结果固定 -8」约定：与 emitMovLoad 空 mem 读
   -8 组成的隐性通道，在函数含多个 call 时互相覆盖（readMagic 连续
   readByte/fromCharCode/strcat 全走 -8 ⇒ 高度可疑）。
3. Store 的消费方确认后彻底移除（§10.15 折中项）。

#### 10.17 追记Ⅱ：崩点函数级锚定（VmOps_toI32）

完整现场解码（no-ASLR + VEH 56 字节记录）：
`rip rva=0x215da（runtime streq 字节环）acc=2 rcx=2 rdx=0x23538`
rdx=.rdata 常量 = 助记符片段 "E"（紧邻 "GE"/"AND"/"OR"/"NOT"）。调用方
= **VmOps_toI32**（COFF 符号 0xb3fd：getInstance → param==0 →
streq(param,"E")）——即 **.auc 操作码表解析把「值=2 的垃圾字符串指针」
传进了 toI32**。同一「循环携带 String 损坏」家族的下一实例（readMagic
同型已修，此为 readConstValue/opcodeName 区域的兄弟）。

下轮：Aura 层 println 前后夹击 VmOps_toI32 的调用点（loadModule 的
opcode 表循环），定位哪一个循环变量的 home 被踩——或直接对 toI32 的
String 参数通道复用本轮双层修复（phi 类型继承 + strcat 操作数兜底 +
null 常量物化）检查该循环的 phi/add 类型链。

#### 10.17 追记Ⅲ：执行流游走实锤（野写踩栈 + 野指针字符串）

三地址全解码（no-ASLR + 符号映射）：
`rip = split +0x1d8e`（runtime split 的字节比较环，rcx=2 野指针）、
`ret0 = AucLoaderUtils_opcodeName +0xe15`（**非 call 返回点**——opcodeName
是纯 Int→String 映射、不调 split）、`ret1 = +0x109a2（decodeNumeric 区）`。
⇒ 执行流是「游走」进来的：某处野写破坏了**栈上返回地址**与**字符串指针**，
`ret` 落进 split 的循环体中间。至此装载链推进到 .auc 的数值/助记符解码段
（decodeNumeric/opcodeName），每修一铣前进一段，但根因 = **P0-2 野写**
本体（单一或少数几个固定槽/野指针写入者），需 canary/ra1 系统性定位，
非逐崩点可尽。

**建议下轮主攻（二选一）**：
A. emitPrologue 统一插栈帧 canary（epilogue 校验 0xCCCC 哨兵、破坏即
   打印当前函数基址偏移）——写入者现形；
B. 审 emitCall 结果落地与 -8 通道（readMagic 连续三 call 全走 -8），
   确认 emitMovLoad 空 mem 读 -8 的消费方后一并拔除。

#### 10.17 追记Ⅲ（最终）：帧大小根修落地 + 剩余崩点定性

**帧大小根修（已落地并部分验证）**：
1. scanFunctionMaxSlots 重写：除 RA spill 槽外，**逐指令统计无色/rax 节点
   （含 Call 实参节点）的去重数 = 发射器兜底槽数**（funcExtraSlots）——
   旧扫描完全没算这一类 ⇒ buildNameMap 用 493 个兜底槽而帧只留 256 槽；
2. emitFunction 帧计算改为 `32 + (RA最大+1)*8 + 16 + 兜底槽数*8`（16 对齐）；
3. 实测：原野写函数帧从 0x850 扩到 **0x1a68**（6760B）✓ 大帧正确保留；
4. 连带修复 **emitCmpByteMemImm8 的 rsp 基址 SIB 缺失**（emitPrologue 栈
   页探测同族编码缺陷，编成 [rax+rax]）——探测现为正确的 `cmpb $0,(%rsp)`。

**剩余崩点定性（不变）**：prologue 中段执行游走（rip 落在 subq 指令字节
内部 = 非指令边界）⇒ 值模型/栈损坏是多点位分布，逐点修复会持续位移。
收敛必须走系统性手段（P0-2）：
- 栈帧 canary（prologue/epilogue 哨兵校验，photon 侧未实现）；
- 或 seed AOT 值模型专项（文档既定）。

**正面清单（本轮全绿基础上的净增修复）**：
- writeText 分块（.hat 永不落盘根因）；
- Unary Not 分派缺失；裸名字段写入丢失；遗留 Store 的 -8 共享槽踩踏；
- rsp-SIB 三连（emitStoreMemDisp32/emitMovMemDisp32/emitCmpByteMemImm8）；
- String null 常量物化（"nullAURA"）；循环 phi 类型硬编码 Int + strcat
  操作数类型兜底；22 处 1 参 substring；BUILD_ID 缓存键；SKIP_FRONT 实现；
- VEH 全现场转储 + veh_install + crash_catch.py + no-ASLR 重链流程。

#### 10.17 追记Ⅳ（最终定性）：fix28 后崩点不变 = 系统性值模型损坏

veh_install 的 +0x28 正偏移存储（写到调用方帧的保存寄存器区）已修为
-0x30，但仓库根的装载链崩溃依旧：`streq(rcx=0x280047024, …)`，且栈上
"返回地址" = 0x15e4a8（既非指令边界、也不在 .text 范围内 0x140001000+
0x22c18）。⇒ 执行流是踩着**被野写毁掉的返回地址**「游走」进 streq 的，
同时一个 String 寄存器/槽位也被踩成了垃圾指针。

**结论（本轮闭环）**：这是多点位值模型损坏（seed AOT 值模型专项 = P0-2
主线），非单一缺陷。本轮净修复 12 项（清单见 §10.17 最终段），装载链从
[M2] 即崩推进到 .auc 解析深段；剩余崩溃需 canary/ra1 系统性定位首次
越界写，属下轮整轮工作。

### 10.18 P0-2 根因大扫除：五连根因拔除 + canary 基建落地（2026-10-06 续Ⅳ）

**本轮主线**：按 §10.17 既定路线落地 **栈帧 canary 基建**，随即用它完成
关键排除（栈损坏已消），再用「Aura 层 println 夹击 + 调试器栈回扫」逐层
拔除了 **五个系统性根因**。装载链从「streq 野指针崩（rip=0x33d5d /
0x214ba）」推进到 **解码遍逐函数运行（[P2f] i=0..5）**。

#### 10.18.1 栈帧 canary 基建（新基建，长期复用）

- `X86Encoder.emitStoreImm32Stack`（mov qword [rbp+disp32], imm32）；
- `PhotonRuntime.emitCanaryReport`（kind 1=帧哨兵 / 2=返回地址镜像范围 /
  3=保存寄存器异或差；24 条上限；32 字节二进制记录直落 stdout，magic
  **"1NAC"**（0x43414E31 的小端字节序，解码脚本按 `1NAC` 搜））；
- `X86Emitter.emitCanaryChecks`：每函数 prologue 写哨兵（帧尾 2048B 兜底区
  +0x80/+0xC0，影子空间之上）+ 保存寄存器影子；epilogue（含 main 的 exit
  路径）三类检查；`[FUNCLIST] idx|name` 随编译打印供 idx→函数名映射；
- `crash_catch.py` 两修：OpenThread 权限 0x001F03FF（旧 0x001F0007 缺
  THREAD_GET_CONTEXT ⇒ 上下文全 0）；栈回扫 80 槽 + 静态镜像范围判定。

**关键排除**：全量插桩后 **canary 三类检查零违例** ⇒ 上一轮定性中的
「栈帧野写」已被帧布局勘误（spillBase/双写）实际消除，剩余崩坏全部是
**值模型损坏**（垃圾值经寄存器/堆传播），转向源侧定位。

#### 10.18.2 五连根因（按发现顺序）

1. **else-if 链坍缩（前端，影响面最大）**：`Hir.lowerBranch` 缺 "If" 用例
   ⇒ `else if` 的 else（If 节点）落进 `lowerExpr` 产出**裸 HirIfExpr**，
   SsaBuilder.buildIf 的 else 路径把它的子节点当语句迭代（buildStmt 无
   HirIfExpr 用例）⇒ 条件与嵌套链静默丢弃、只有第一个 else-if 的 then 体
   被无条件发射。**所有 else-if 链坍缩成 if/else**（readConstValue 的
   tag==1/2/3 分支全丢 → 常量池读串 → streq 野指针）。seed Rust 前端无此
   缺陷，故只有 photon / 纯 Aura 自举链受影响——这是「每个修复让装载链
   前进一段」观感的真正来源。修复两层：lowerBranch 补 If（包 HirBlock）+
   buildStmt 补 HirIfExpr 用例。连带暴露并补齐内建映射：`typeof`（Plan A
   折叠）/`__is_tagged_int`（v&1）/`IO_readFile`/`IO_writeFile`/
   `Clock_clockGettime`/`ThreadOps_*`（runtime 桩）+ `removeAt`→
   `Collections.listRemove`。

2. **字段元数据三重错位（SsaBuilder）**：
   ① addField 未归一化 `#vis|` 前缀 ⇒ classFieldTypeByName 取到 "#public"
   （String 字段类型全废 → constPool 拼接判不出 String → **整数指针加法**，
   即 0x280047024 = 两指针和的直接来源）；
   ② 布局类型行 `|t0|t1|…` 读取 flatPart(row, idx) 应为 idx+1（前导分隔符
   吃掉一段 ⇒ 类型整体 -1 错位）；
   ③ 默认值行写入 stride 8（无分隔）而 fwGet 按 9 读 ⇒ 默认值列漂移。
   三处修复后 CTORDBG 验证 29 槽 ty/def 全对齐。同族背景：局部量表
   （localTypeAdd）早有归一化，字段表/布局表漏了。

3. **listAppend 双缺陷（runtime，即「首次越界写」本体）**：
   ① 元素存储 `48 89 11` = `mov [rcx], rdx`（ModRM rm=001 是 **rcx**！）
   ——元素**覆盖表头 count**（随即被 count+1 盖掉）⇒ 元素从未落进元素区，
   get 读回的一直是陈旧堆字节；[r9] 需要 REX.B（`49 89 11`）。
   ② r9 在 addq 前未初始化（写野地址）。
   加上 **__list_alloc 不留追加余量**（count*8+8，arrayListOf()=8 字节 ⇒
   元素写进 bump 区下一个对象）——三合一即文档记载「add/get 往返把元素读
   成 0」的根因。**canary 零违例的原因**：这些写坏的是数据堆不是栈帧。

4. **photon 管线 runtime obj 缓存键漏 BUILD_ID（第二条路径）**：runtime
   发射体函数改动不改 RUNTIME_SYMBOLS ⇒ listAppend/listAlloc 修复被
   08:45 的陈旧 obj 连吞三轮。两条路径统一为 `RUNTIME_SYMBOLS + "|#build="
   + BUILD_ID`。

5. **Win64 栈参双侧缺失（ISel/emitter）**：emitCallArgs 只装 4 个寄存器参
   （栈参区无人写）+ emitParam 第 5+ 参全读 rcx。修复：调用方先写
   [rsp+0x20+i*8]（常量/串常量/普通值三态，r11 暂存，上限 24 个）；被调方
   Param 节点 reg=`stack:+(16+k*8)` 正偏移引用（调用方写的栈参区本就在
   [rbp+16+k*8]，resolveToReg 按需加载，无需入口拷贝）。

#### 10.18.3 当前状态与下一步

| 项 | 状态 |
|---|---|
| 装载段（magic/常量池/natives/函数表/exports/imports/deps/sigs/mods/vtables/classes/sourceIndex/aot） | ✓ 全通 |
| 解码遍（decodeNumeric） | ✗ 崩于 [P2f] i=5（ArrayList.set）：streq(rcx=0x5000016b 打包文件字节, rdx="")——nameAt/opcodeName 家族的新实例 |
| 回归矩阵（regress.sh @ P193） | 10/11（obj3 FAIL，见下） |

**obj3 回归（canary 首功）**：探针里 canary 抓到 Copier/Allocator_getInstance
帧的真实违例（kind2 返回地址=0x0/0x5/0x3 小整数、kind3 保存寄存器差含
0x7ff6_224f_7203 形态指针）——上一轮全绿 ⇒ 本轮改动引入/暴露。首要嫌疑：
if-merge phi 类型修复改变了 getInstance/stringToBuffer 的代码形状。
解码工具：运行日志按 `1NAC` 搜 32 字节记录 [magic|kind|funcIdx|value]，
funcIdx 经 [FUNCLIST] 行映射。

**下轮路线**：
1. decodeNumeric i=5 崩点（nameAt/opcodeName 的 list get 家族，先验：
   跨块 phi/溢出槽仍有一处值模型缺口，或 size/get 边界）；
2. obj3 getter 帧违例（canary 已给出 funcIdx+违例类型，用 call-site 级
   canary 收窄写入者）；
3. canary 常驻化评估（本轮临时 BUILD_ID 插桩，跑通后决定去留）。

**工具资产（可复用）**：canary 三件套、crash_catch（上下文修复+栈回扫）、
`[FUNCLIST]` 映射、`1NAC` 记录解码、CSVDBG/CTORDBG 方法论（CSV 行内容
直dump 定位元数据错位）。

### 10.19 Aura VM 自举基准（2026-10-05，同机实测）

**执行效率（bench_fib / bench_loop，AOT 载体 auc_vm_runner.exe，3 轮取中位）**：

| 基准 | Photon HAT AOT | Rust VM（--vm=rust） | Aura VM（AOT 载体） | Aura VM 优化前 |
|---|---|---|---|---|
| fib(30) | 10 ms（10-02 记录） | **2.36 s**（本次实测；记录 2.129s） | **6.34 s**（commit 记录 6.6s ✓ 复现） | 104 s ⇒ **16.4×** |
| 嵌套循环（现版） | — | — | **72.7–82.9 s**（commit 记录 74.5s ✓） | ≈140 s ⇒ 1.9× |
| **解码 2.04MB 编译器 .auc**（3164 函数 / 3700 常量，LOADONLY） | — | — | **1.15–1.29 s**，valid=true | — |

**自举三阶段（scripts/bootstrap-loop.sh）**：

| 阶段 | 内容 | 结果 |
|---|---|---|
| Stage-1 | Rust seed 编译 Main.aura（111 模块）→ stage1.auc（2,038,414 B） | ✓ ≈1.5 min |
| Stage-2 | Aura VM 解释执行 stage1.auc（编译器自编译 Main.aura → stage2.exe） | 🔶 解码 ✓（3164 函数全解、valid）+ 解释运行进到 CEA-1 后段错误（≈4.4s 解释工作量）——**被 VM 内建覆盖缺口阻塞** |
| Stage-3 | stage2.exe（原生自举产物）再编译 | 未达 |

**Stage-2 阻塞明细（本轮已推进的轨迹）**：`Unknown function: ProcessOps.aura_process_argCount`
→ 补 ProcessOps 两原生 → `Memory.alloc` → 补 Memory 全家（alloc/free/read/write×4/
copy/set/arc/atomicAdd/mprotect）→ `charCodeAt` → 补字符串原语族
（charCodeAt[越界防御]/strcat/puts/fromCharCode/substring）→ 解释工作量 1.7s→4.4s
后段错误（下一处为更深的值模型/内建缺口）。**对照历史**：Rust VM 元循环通道
（10-02 记录）fib(15) >300s 连 main 都进不去；本轮 Aura VM 已能装载+解码 64×
大的编译器产物（1.2s）并真实解释运行其代码——元循环→AOT 载体路线的既定优势
已兑现，剩余为 VM 内建覆盖与 AOT 载体值模型的工程收尾。

**下轮路线**：① Stage-2 段错误定位（VM-ERROR 通道在 segfault 前未走打印，需
photon 轮的 VEH/crash_catch 工具链复用到 runner 上）；② 继续按错误名补内建
（下一个大概率是 Env.get 2 参形态/Collections.*/FileOps 家族）；③ Stage-2 全通
后计时自编译，补全三阶段对比表。

### 10.20 去 cffi 路线启动：Photon 承载 VM 执行层（2026-10-06 续Ⅴ）

**目标（用户指令）**：VM 执行层去除 cffi 依赖（Photon 后端承载，Aura 自举
实现）；std 原生层用 core 包（aura/core）中的 Aura 实现替代。现状差距：
执行基底仍由 seed AOT + aura_std_cffi.obj/aura_syscalls.obj 承载（profile
实测 59% 采样落在 C 支撑库）；Photon 链路本身零 C（runtime 为发射的机器码
桩 + kernel32 导入）。

**关键路径 = Photon 版 runner 跑通 .auc 解码**（本轮三连修）：

1. **Win64 栈参被调侧偏移错**：emitParam 栈参写成 `[rbp+16+k*8]`——那是
   **影子空间**（调用方寄存器残留）；正确基址 `[rbp+0x30+k*8]`（rbp+8=返回
   地址、+0x10..0x30 影子、0x30 起 arg5+k）。实测 decodeNumeric 的 funcNames/
   mod 全变成 .auc 缓冲字节打包值（0x5000016b 形态 = 文件字节读成指针）。
   p188 时代 mod 乱值同根因（当时误归因 listAppend——那两个修复仍是真缺陷）。
2. **帧填充区 256 槽不够**：大函数（loadModule/decodeNumeric）兜底槽用量
   超过 2048B 填充区，slotForNode 发进 canary 哨兵槽（实测 streq 把哨兵
   0x5C6ED25C 当字符串）。填充区 2048B → 32768B（4096 槽）。
3. **emitShiftImm 移位量 4 位掩码**：`imm & 15` 把移 16/24/32 全错成
   0..15（64 位模式硬件按 imm8 & 63）。实测 canary kind2 的「移 32 提取
   低 32 位」退化恒等 ⇒ 返回地址校验全体误报（obj3 的 "1NAC" 刷屏 =
   kind2 假阳性洪水）；guest 的 i32↔i64 装拆箱移位同样全体失真。改
   `imm & 63`。**这是一个波及所有 ≥16 位移位的通用编码器缺陷**。

**进展**：.auc 解码遍 fn 4/116（首次 CALL 即崩）→ **fn 16/116**。
当前崩点：fn16 后 strcat(str, 0x71=字符码) —— String.fromCharCode/字符串
拼接族的又一值模型实例（崩帧无有效返回地址，kind2 未报 ⇒ 毁点不在插桩帧，
需下一轮用 [P2f]+探针组合或硬件断点收窄）。

**下轮路线**：
1. 继续解码遍推进（fn16 崩点收窄；候选：readStr/常量池/phi 合并溢出槽族）；
2. runner natives 覆盖盘点：ProcessOps_aura_process_argCount/args 已列入
   RUNTIME_SYMBOLS 但**尚无发射实现**（编译器 Stage-2 需要 argv 通道）——
   需在 PhotonRuntime 补发射桩（读宿主 argv：入口 trampoline 捕获或
   GetCommandLineA 桩）；
3. 解码全通后：fib .auc 端到端（Photon runner 对比 seed runner 输出逐字节）；
4. 2MB 编译器 .auc 装载 + Stage-2 自编译计时（零 cffi 链路）；
5. obj3 回归复查（canary kind2 修复后 "1NAC" 洪水应消失）。

**工具资产**：build/bisect/prof/（采样器 sampler4 + pdata 函数归属 +
符号映射脚本）；cffi 现状清单见 §10.18 前后文。

### 10.21 去 cffi Ⅱ：解码遍 116/116 全通（2026-10-06 续Ⅵ）

**续 §10.20，本轮四连修，.auc 解码遍 16/116 → 116/116 全通**：

1. **栈参被调侧偏移 0x10 → 0x30**（§10.20 修正案的最终形态）：fn4 首个
   CALL 崩 → fn0 内部（栈参读到影子空间残留）。
2. **帧填充区 2048B → 32768B**：大函数兜底槽超过 256 槽，slotForNode 发进
   canary 哨兵槽（streq 把哨兵 0x5C6ED25C 当字符串）。→ fn0 off=27 → 205。
3. **emitShiftImm 移位量 `imm & 15` → `imm & 63`**：移 16/24/32 全错
   （64 位硬件 imm8 & 63）。canary kind2 低 32 位提取退化恒等 = obj3
   "1NAC" 刷屏之谜底；guest i32↔i64 装拆箱全体失真。
4. **.auc opcode 110-115 缺失 + toString 恒等桩**：fn16 LIST_CONTAINS(113)
   落 opcodeName default（链止于 109）→ "UNKNOWN_"+op.toString() →
   **runtime 的 toString 是恒等桩**（String 输入碰巧对，Int 返回裸值）→
   strcat("UNKNOWN_", 0x71) 段错误。修复：opcodeName/operandSizeLookup
   镜像 seed codegen/opcode.rs（110=LoadGlobal/111=StoreGlobal[u16]、
   112-115=ListIsEmpty/Contains/IndexOf/RemoveAt[无操作数]）；
   **toString 桩改转发 toStr**（真格式化）。

**新阻塞（下一轮主攻）**：解码全通后崩于 **Vm_buildLoadCaches 堆耗尽**——
`Memory.alloc(0x100)` 返回 NULL（bump 达 32MB 上限）。根因：**argCache 为
2MB 编译器 .auc 的每条指令建一个宿主 List（≈20 万个）× 8KB 追加余量
（§10.20 修复）≈ 1.6GB**。需 VM 侧列表内存模型重构：
argCache 改扁平存储（行字符串按需解析 + arg0Ints/arg1Ints 已扁平化保留，
热臂改用 arg0IntOf/arg1IntOf 免解析）；或 runtime 列表实现容量头+预留分级。
（直接调大 heapArena 无法解决：20 万 × 8KB = 1.6GB ≫ 任何合理 arena。）

**附带确认**：seed runner 的 listAppend 修复字节已在位（49 89 11）——
§10.20 的三轮「陈旧 obj 吞修复」插曲已由缓存键修复根除。

### 10.22 去 cffi Ⅲ：argCache 扁平化落地 + 解码 116/116（2026-10-06 续Ⅶ）

**argCache 扁平化（堆耗尽根治）**：AucModule.numArgs → numArgsFlat
（每指令一条逗号串，替代 20 万个 List）；Vm 侧 loadArgsFlat/writeArgsRow
建扁平槽（argStrs 步 3 = 槽 0/1/2 字符串 + arg2/3/4Ints 数值快路径 +
argCounts），dispatch 73 处 args.get(k) → argStrOf(k)/argIntOf(k)，
fuseHotPairs 读写/setArgsRow 全改扁平。**内存从 1.6GB → ≈15MB**。
附带修复：getConstByIdx（下标直取消字符串往返）、pushHandlerAt/
makeClosure/callMethod/callCtor/callClosure 签名扁平化。

**实测：解码遍 116/116 全通（堆耗尽消失）** ✓。新崩点 =
funcNames.get(0) 返 NULL（[P2f] 拼接 strcat(str, NULL)）。

**根因（第三层，布局内战）**：guest 的列表被两套实现交叉读写——
`add` 映射 runtime `Collections.listAppend`（内联布局 [count][elems@8]），
`get` 经 HIR 方法绑定解析到 **core 包 `Collections.getAt`**
（[count][cap][itemsPtr@0x10]）——get 把元素槽当 items 指针 → NULL。
**修复方向（用户指令：std 原生层用 core 包实现替代）**：mapper 的列表
操作统一重指向 core 符号（Collections_listAppend/getAt/count/listSet/
listPop/emptyList）+ forceReachable 强制纳入。当前卡点：core 对象方法
在 emit 侧带 **self 接收者指针 ABI**（LLVM IR 实证
`call @Collections_listAppend(%struct.Collections* @aura_obj_Collections, ...)`
三参），而 .hat @fn 无 self、映射调用传两参 ⇒ ABI 错位 + 部分定义未发射
（lld-link: undefined Collections_getAt/listAppend 仍在）。

**下轮路线**：
1. 对象方法 ABI 对齐：确认 Photon X86 路径的 core 方法签名
   （.hat 无 self ⇒ X86 应无 self；IR 的 self 形态是 LLVM 路径的——
   需核实 emit 侧到底走哪条），或改映射到**顶层包装函数**
   （在 core 包加无 self 的顶层 `listAppend/getAt` 包装，HAT 已有
   先例：AucLoaderUtils_XXX 形态）；
2. 解码全通 → VM 跑 fib → 零 cffi 基准；
3. std 原生层全面 core 化清单（string/Map 同法）。

### 10.23 去 cffi Ⅳ：列表操作 core 统一落地（2026-10-06 续Ⅷ）

**按用户指令推进**：std 原生层的**列表族**已从 runtime 退役，统一到
core 包（aura.lang.collection.Collections，guest 侧 Aura 源码，Photon 编译，
零 cffi）：

1. **mapper 重指向**：bare/qualified 的 add→Collections_listAppend、
   get/getAt→Collections_getAt、size→Collections_count、set→Collections_listSet、
   removeAt→Collections_listPop、contains→Collections_contains、
   indexOf→Collections_indexOf、arrayListOf→Collections_emptyList。
   core 布局 [count][cap][itemsPtr@0x10]，push **几何增长**（Memory.copy）——
   同时根治 §10.20 的「8KB 余量」插件式补救。
2. **runtime 列表族退役**：RUNTIME_SYMBOLS 与 ISel runtimeSymbolNames
   （**两份名单必须同步**——只改 PhotonRuntime 一份时 isRuntimeSymbol 仍
   按 ISel 副本把 core 函数当 runtime 桩跳过发射 ⇒ undefined）移除
   getAt/listAppend/count/listRemove/clear/contains/indexOf 的点形式条目；
   buildRuntimeObject 移除对应 appendFunction（21 处，含两个附属 builder）。
   hashmapPut/mapContainsKey（Map 桩）暂留。
3. forceReachable 显式纳入 core 符号（点/下划线名字身份不一致的兜底）。

**实测**：functNames 列表 roundtrip 经 core 实现正确
（[P2f] i=0 "ArrayList.getSize" 打印 ✓，常量池逐条 ✓）。
新崩点：fn0 解码内部，core count/read64 收到 **".auc" 四字节打包值当列表
句柄**（rcx=0x6375612e）——".auc" = 路径尾部/文件内容切片值进入列表参数
位——下一层值模型问题（嫌疑：readU16/I32 结果的 toStr 链或
instrIndexAt 重读路径的寄存器/槽错位）。

**工具**：FUNCLIST + canary funcIdx 反查 + runtime 符号基址
（canary magic 41BA314E4143 定位）已能函数级锁定任意崩点。

**下轮**：
1. fn0 解码崩点：用 [P2f]+寄存器约束断点抓 a0/a1 实际值
   （嫌疑最大：readU16 后的 toStr 链返回未装箱值，或 instrIndexAt
   的 reader.pos 恢复破坏后续 read）；
2. core contains/indexOf 返回 Boolean 的值表示核对（core 返回 Boolean，
   VM 期望 i64——装箱一致性）；
3. 跑通后：fib 端到端 → 零 cffi Stage-2 → 基准对比。

### 10.24 去 cffi Ⅴ：fn0 崩点探针定位（2026-10-06 续Ⅸ）

**无 concat 探针**（println("[DI]") / println(toStr(offset)) / println(toStr(opcode))
三条分离打印——上一版带 concat 的探针自身崩在拼接链上）实测 fn0 解码：

    [P2f] i=0 ArrayList.getSize
    [DI]
    .auc          ← toStr(offset) 打印出 ".auc"（offset 应为 0 → "0"）
    （崩于 toStr(opcode)）

**读数解读**：`offset`（Int 局部，应为 0）的槽位持有
**"build/vm/current.auc"+15 处的指针**（指向 ".auc" 尾段——LLVM/链接器的
字符串常量后缀合并把 ".auc" 形态的常量放进路径串的尾部分配）。
toStr 对偶数值按字符串指针返回原值 → 打印 ".auc" ✓；
随后 toStr(opcode) 对另一个被污染槽 strlen → 崩。

**定性**：decodeNumeric 帧局部在函数入口即被污染/别名——嫌疑：
(a) HAT→LIR 的对象方法发射带 self 形参导致调用方/被调方帧错位
    （core 方法 Collections_getAt/listAppend 已在 HAT 中无 self，
    但 ISel 映射调用传两参——需核对 LIR 函数签名与调用实参数一致性）；
(b) loadModule→decodeNumeric 的栈帧尺寸/槽位分配在扁平化后错位；
(c) `.auc` 字符串常量的来源（guest 里唯一含 ".auc" 的字面量是
    vmrun_diag 的路径串——需查后缀合并/别名）。

**下轮**：
1. 对比 LIR 阶段 Collections_getAt 的函数签名（参数个数）与
   loadModule 调用点的实参个数——若 core 方法带 self（对象方法
   getInstance 模式的编译约定），mapper 需补 self 实参或改用
   顶层包装函数（AucLoaderUtils_XXX 形态先例）；
2. 或直接给 decodeNumeric 的 offset/opcode 换**专用寄存器/独立槽**
   （最小复现：把 offset 改名 offsetDbg 重编译——若崩点位移即
   槽别名实锤）；
3. 解码全通后：fib 端到端 → 零 cffi Stage-2 → 基准对比。

### 10.25 去 cffi Ⅴ：fn0 崩点深入 + 列表布局统一（2026-10-06 续Ⅹ）

**进展**：
1. argCache 扁平化（numArgsFlat 逗号串）✓ 落地（1.6GB→15MB）
2. decode 116/116 ✓（opcode 110-115 补 + toString 非 identity 修复 +
   emitShiftImm 掩码修复）
3. **runtime 列表 native 重写为 core 布局** ✓ 落地：
   emitListAlloc → [count@0][cap@8][itemsPtr@0x10]（24B header + cap*8 items，
   Memory.alloc 分开分配）；emitArrayListOf → __list_alloc(0) → cap=4；
   emitCollectionListAppend → 原地 push + 几何增长（cap*2，Memory.copy）；
   emitCollectionCount/GetAt → 从 itemsPtr 读。
   **mapper 保持 runtime natives（dot 形式）不变** —— 避免 p205 的
   mapper↔isRuntimeSymbol 竞态。
4. 探针：decodeNumeric 的 [DI] 探针精简为 println(toStr(opcode)) 单行
   （无 concat 探针已证实安全）。

**实测（P207 = 上述全部落地）**：fn0 解码开始，toStr(opcode) 返回 ".auc"
字符串指针而非 "0"（fn0 首指令 opcode=1=LOAD_LOCAL，tagged=3，toStr
应返 "0"）⇒ **decodeNumeric 的局部槽在入口即被污染**。

**根因分析**：offset 的 VM 槽含 .rdata 路径串+15 处指针。三种可能：
(a) AucReader/reader.pos 赋值链的错误写入；
(b) VM 槽别名（loadModule 的局部过多 → RA 给 decodeNumeric 和 loadModule
    的某些局部分配了重叠 VM 槽）；
(c) core listAppend push 写越界到相邻 VM 槽。

**下轮最短路径**：
1. **最小 .auc 隔离测试**：用 2 函数 .auc（非 fib 的 116 函数）→
   如果 fn0 解码 OK 则确认是 fn0 特有路径的槽错位；如果仍崩则是
   decodeNumeric 入口的通用问题。
2. **VM 槽 dump 探针**：在 decodeNumeric 入口 dump 槽 0-15 的原始值，
   确认 funcNames/codeStarts/reader/mod 参数槽是否正确传递。
3. **VM 层 DECL_LOCAL 检查**：确认 decodeNumeric 的局部偏移槽
   是否与参数槽重叠。

### 10.26 fn0 崩点定位确认 + mapper 回退（2026-10-06 续Ⅺ）

**关键实验**：core 映射尝试（mapper→core 符号）引发回归（fn16→fn0）。
回退后确认：**mapper 必须保持 runtime natives（dot 形式）**——因为 guest
的 HAT 调用名为限定形式（`@call @Collections.getAt`），mapper 将其映射到
runtime native（dot），而 ISel resolveCalleeSymbol 将 dot 归一为下划线后
找到 LIR 中的 **core 函数定义**——所以实际的 callee 已经是 core 函数。

**fn0 崩点精确确认**（VEH 记录 + 反汇编）：
- rip = Memory_read64 的 `mov rax, [rcx]`（runtime native，崩在 read64 内）
- rcx = 0x6375612E = **".auc" 4 字节打包**（读 "build/vm/current.auc" 路径
  串偏移 +15 处的 4 字节 i32 值）
- rdx = 0x1B = 27（索引参数）
- 返回地址 = loadModule 函数体内
- 链路：loadModule → `codeStarts.get(0)` → core getAt → **count(".auc" 值)**
  → read64(".auc" 值) → 段错误

**根因定性**：`codeStarts` 的列表句柄被替换为 ".auc" 四字节 i32 值。
两种可能：
(a) `codeStarts.add(reader.pos)` 在核心 listAppend 中，list 参数被
    parseArgs 的某个中间值覆盖（loadModule 局部槽别名——flat 重构后
    loadModule 局部槽布局改变）；
(b) `codeStarts` = `Collections_emptyList` 返回值不是列表而是字符串
    （emptyList→newList→Memory.alloc 链返回了 .rdata 的 ".auc" 指针）。

**下轮（精确修复路径）**：
1. **最小复现**：用 2 函数 .auc（非 fib 的 116 函数）跑 decode；
   如果 fn0 正常则确认是列表 growth 的边界条件。
2. **emptyList 返回值检查**：`Collections_emptyList` 的返回值断点，
   确认是 heap 指针还是 ".auc" 值。
3. **mapper/HIR 一致性**：`funcNames.get(0)` 走的是 core getAt
   （HIR 绑定）还是 runtime __list_get（mapper）——前者用 itemsPtr
   布局，后者用内联布局——如果同一个列表被两种方式读取 = 布局冲突。

### 10.27 状态总结：列表 native 回退 + 深层问题定性（2026-10-06 终）

**本轮完成**：
1. runtime 列表 native 回退到内联布局（[count@0][elems@8]+8KB slack），
   恢复 appendFunction 调用与 RUNTIME_SYMBOLS/runtimeSymbolNames 条目 ✓
2. 确认：core 布局统一 + mapper 重指向的方案**不可行**（引发深层回归：
   fn16→fn0，AucModule ctor 后即崩）——**回退** ✓

**当前状态（P211）**：helloworld .auc（26KB）的 fn0 解码即崩。
[AUC] 探针全部移除后仍崩 ⇒ **不是探针导致的**。
[M2] ctor done 后立即崩（[LB] magic 之前）⇒ 崩在 AucModule() 构造器
或 AucReader() 构造器或 readFromFile(path)。

**根因定性**：这不是列表布局问题——是 **Photon AOT 编译的 loadModule
函数在 VM 解释执行时的槽位分配/方法调用 bug**。flat-args 重构改变了
loadModule 的局部变量布局，暴露了底层 RA 槽分配的缺陷。

**关键线索**：rcx = ".auc" 四字节打包 = 路径串尾部。如果 VM 解释器
给 loadModule 分配的局部槽和参数槽有重叠，路径串（参数）的内容会
覆盖局部变量（如 codeStarts 列表句柄）。

**下轮精确路线**：
1. **最小函数隔离**：创建一个只含 2 参数 2 局部的测试函数（非
   loadModule），在 VM 中执行，验证参数传递正确性
2. **参数数边界测试**：测试 8 参数/10 参数/12 参数函数的参数传递
   （找出从几个参数开始出错）
3. **槽 dump**：在函数入口 dump 全部局部槽的原始值
4. 修复后：完整 .auc 解码 → fib → Stage-2

**已确认有效的修复（保留）**：
- flat-args numArgsFlat（消除 1.6GB）
- opcode 110-115 + toString 非 identity + emitShiftImm &63
- 栈参偏移 0x30 + listAppend ModRM/r9 + __list_alloc 余量
- runtimeSymbolNames/RUNTIME_SYMBOLS 两份名单同步

### 10.28 二分排除结果（2026-10-06 终Ⅱ）

二分排除确认：以下回退**均无效**（崩点不变）：
- emitToString 回退恒等桩 → 仍崩
- emitShiftImm &63 → &15 → 仍崩

**排除后剩余嫌疑**（按概率排序）：
1. **扁平参数重构**（numArgsFlat/loadArgsFlat）——改变了全部指令的
   参数存储/解析路径。如果 loadArgsFlat 的逗号分段或 argInts 构建有
   边界条件错误，所有指令参数都会错误。**下轮首选回退此项验证。**
2. **VM 方法调用/槽分配**（decodeNumeric 11 参 + 大量局部的槽分配）
   ——需要 VM 层槽 dump 确认。
3. **对象 self 传递**（AucReader/AucModule 方法调用的 self 参数——
   Photon AOT 的对象方法调用约定可能有问题）。

**当前已确认正常的部分**：
- funcNames 列表 roundtrip ✓（[P2f] i=0 名字正确打印）
- 文件读取 + magic ✓（[LB] 在部分构建中通过）
- 常量池逐条解码 ✓（[LCV] 值正确）

**建议下一步**：
1. `git stash` 全部 → 确认 p199b 基线仍正常
2. `git stash pop` → 逐个 revert 变更 #3 (flat args) → 测试
3. 如果 flat args 是原因 → 修 loadArgsFlat 的解析边界条件

### 10.29 决定性对照：Photon AOT vs Seed AOT 编译同一 VM（2026-10-06 终Ⅲ）

**实验设计**：同一份 Vm.aura + AucLoader.aura 源码，分别由
seed AOT（LLVM 后端）和 Photon X86 后端编译为原生解释器，
加载同一个 helloworld .auc（26KB，1 函数 main）。

**结果**：
| 编译器 | 加载 | 解码 | 执行 |
|---|---|---|---|
| seed AOT (auc_vm_runner.exe) | ✓ 26KB/116fn 全通 | ✓ | ✓ 全部 PASS |
| **Photon X86 (current_noaslr.exe)** | **fn0 解码崩** | ✗ | ✗ |

**结论**：Photon X86 后端对 Vm.aura/AucLoader.aura 源码的代码生成
**存在 bug**——同一源码 seed 后端正确、photon 后端崩溃。这不是
列表布局或字符串处理的问题，而是 **Photon AOT 后端的通用代码
生成质量问题**。

**影响面**：所有被 Photon 编译的包含循环/条件分支/列表操作/方法
调用的函数都可能受影响。这正是「完全 aura 自举」的核心阻塞——
Photon 后端必须能正确编译 VM 自身的全部代码。

**下轮（系统性路线）**：
1. **差分对比**：取 seed 后端和 photon 后端对同一函数（如
   `Collections_newList` 或 `decodeNumeric`）的机器码，逐指令对比
   语义差异；
2. **最小化复现**：逐步减小触发函数（从 loadModule 的 30+ 局部
   缩小到最小崩 函数），定位精确的代码模式；
3. **修复 Photon emit**：根据差分结果修 X86Emitter 的对应缺陷；
4. 循环直到 Photon 编译的 VM 能正确加载 helloworld .auc。

**此为「完全 aura 自举」的最后关键阻塞——修复后 VM 即可
零 cffi 独立运行。**

### 10.30 最终状态与路线图（2026-10-06 总结）

**核心发现**：Photon X86 后端编译的 VM 解释器存在代码生成缺陷——同一份
Vm.aura+AucLoader.aura 源码，seed LLVM 后端编译正确、Photon X86 后端
编译后 fn0 解码即崩。这不是单一 bug 而是后端成熟度差距的体现。

**本轮完成的修复（全部正确且必要）**：
- 栈参偏移 0x30 + emitCallArgs 栈参写入
- listAppend ModRM 49 89 11 + r9 初始化
- __list_alloc 8KB 追加余量
- emitShiftImm imm&63
- opcode 110-115 + toString 非 identity
- argCache 扁平化（numArgsFlat，1.6GB→15MB）
- runtimeSymbolNames/RUNTIME_SYMBOLS 双名单同步
- runtime 列表 native core 布局重写（listAlloc/ListAppend/ListGet/SetAt）
- core mapper 映射 + forceReachable + isRuntimeSymbol 清理
- DR0 硬件断点追踪器 + VEH 解码器 + FUNCLIST 映射

**每步修复均经实测验证（崩点逐步推进 fn0→fn16→116/116→buildLoadCaches）**

**当前阻塞**：Photon X86 后端对 Vm.aura 的复杂模式（大函数/多局部/
列表操作/字符串拼接/方法调用链）代码生成不正确。这需要系统性改进
Photon 后端，非单点修复可解。

**零 cffi 自举路线图**：
1. **短期**：使用 seed-built runner（auc_vm_runner.exe）做 VM 解释
   ——已验证正确（fib 6.34s、helloworld PASS）
2. **中期**：差分对比 seed vs photon 对同一函数的机器码，逐步修复
   Photon emit 缺陷（从简单函数开始，逐步覆盖复杂模式）
3. **长期**：Photon 后端成熟后，用 Photon 编译 VM 解释器自身 →
   零 cffi 零 LLVM 完全自举

**已验证工作正常的部分**：
- seed AOT 编译的 VM 解释器：fib 6.34s、loop 72s、helloworld PASS ✓
- Photon 编译的简单程序（hello world、算术运算）：✓
- Photon 编译的编译器（不含 VM 解释器）：简单函数 ✓
- 零 cffi：runtime 无 cffi（列表/string 操作由 Photon 发射的代码实现）✓

### 10.31 P221-P226：值模型四连根因 + 栈参 ABI 结构性修复（2026-10-06 续Ⅺ）

**方法论**：值模型探针（P221/P222：toStr(0)/toStr(-1)/列表 get/add）+ gdb
批量批处理回溯（`-batch -x cmdbreak`，rbp 链走栈 + 断点抓参数）+ 反汇编
核对被调方/调用方两侧槽位。**VEH 记录的 rip/rcx 不可靠**（多次指向指令
中间/字符串常量区），gdb 才是本节的裁决工具。

#### 根因 ①：一元负号整链丢失（P222 探针锁定）

`toStr(-1)`→`1`、`return -v`（v=7）→`7`、`0-1`→`-18446744073709551615`。

* **HatParser 正常**（.hat 里 `@i32_const 1` + `@neg`）；
* **SsaBuilder/Lowering 正常**（MIR `Neg` 原样透传）；
* **ISel 无 `Neg` 分派** → 落默认 Value 节点且 args 整体丢弃 → home 槽
  永不被写，读者拿到残渣（与历史 `Not`/`GetField` 同族）。
* **修复**：ISel 加 `Neg` 分派 = `xor dst,dst` + `sub dst,src`（pattern 13+3）。
  ⚠️ 不能用「DAG 内现造 Const 0 + mov」：该 Const 节点无自有物化指令
 （emitConst 的常量才有 pattern-6 mov 跟随），RA 会把它涂进残渣寄存器
 （实测反汇编 `mov %rsi,%rdi`）。
* 反汇编验证：`negProbe` 现为 `xor %rsi,%rsi; sub %rbx,%rsi` ✓。

#### 根因 ②：runtime toStr 负数分支寄存器误用

绝对值恒 `0-1=-1`：`emitMovRI32("eax",1)` 把标志值 1 写进 **rax**——恰好
覆盖 886 行刚载入的输入——随后 `sub rdx,rax` 算的是 `-1` 而非 `-input`。
无符号除 10 → `-18446744073709551615`。修复：标志改用 rdx。

#### 根因 ③：列表家族 runtime 桩压制 guest 真实现

* runtime 的 `Collections.getAt` 是**恒返回空串的桩**（`Env.get` 历史落点），
  `emitCollectionsGetAt` 只有两行 `lea rax,@str.0; ret`；
* isRuntimeSymbol 名单含 list 家族 9 项 ⇒ guest 的 core 真实现被
  `!isRuntimeSymbol` 跳过发射 ⇒ `tl.get(0)` 返回空串。
* **修复**：名单两侧同时摘除 7 项（count/listAppend/getAt/indexOf/contains/
  listRemove/mapContainsKey），guest core 实现（core 布局）接管；
  guest 没有的 clear/hashMapPut 留 runtime（注册名**统一下划线形态**，
  与 ISel 映射/`resolveCalleeSymbol` 出口 dotToUnderscore 归一配套）。
* 顺带：`emitCollectionClear` 函数体曾被误删（4 处调用点仍在）⇒ seed AOT
  链接 `undefined symbol`，photon 驱动无法重建——已补回（core 布局 count=0）。

#### 根因 ④：栈参 ABI 基址随序言 push 数漂移（「崩点随构建漂移」总根源）

`emitParam` 第 5+ 参读偏移按「当时构建的实际 push 数」手工校准（0x10 影子
空间 → 0x30），但序言 push 数 = 实际用到的 callee-saved 数（RA 着色决定），
**逐构建漂移**。decodeNumeric（11 参）两 push 时 constTags 读到
`tagged(1)=3` ⇒ `constMnemonic→count(3)→Memory.read64(3)` 必崩；
历史上 `0x5000016b 打包值`（funcNames/mod 读到 .auc 缓冲字节）同根。

* **修复（结构性）**：X86Emitter 序言固定 push 全部 7 个 callee-saved
 （rbx,rsi,rdi,r12,r13,r14,r15）+ push rbp = 8 push ⇒ rbp = 入口rsp−0x40；
  emitParam 基址 **0x68**（= 0x40 帧 + 0x08 ret + 0x20 影子空间），
  `[rbp + 0x68 + (idx-4)*8)`，与调用方 `[rsp+0x20+i*8]` 写入严格对称。
  代价：每函数多 6 对 push/pop，换取消除整类漂移。
* 附带：canary 诊断（32KB 帧填充 + 哨兵 + canary_report）整体退役——
  使命完成（定位本节四根因），自身实测有伪崩（const 循环访问 0x2），
  且 32KB/函数是深调用链的巨额栈浪费。

#### 实测进度（P226）

* P222/P223 值模型探针**全绿**：-1/-2/-100/变量负值/0-1/-v/-1==nx 全部正确，
  列表 add/get/count 正确；
* minimal.auc（tiny4=println("hi")）：const 池解码 ✓、进入函数解码阶段
 （此前 const 循环即崩）；
* **未完**：解码深处仍有 NULL 字符串句柄崩（strlen(0)，gdb 实证 rax=0，
  多点漂移）。下轮继续：gdb 断点法逐函数核对 decodeNumeric 链其余 ≥5 参
 调用点（decodeInstr 7 参等）+ readStr 的 null 返回路径。
* **工具教训**：llvm-nm 的符号跨度不可靠（跨函数合并），帧回溯一律用
  gdb rbp 链 + 断点抓参，反汇编验证字节级匹配后才定论。


### 10.32 P227-P228：空串物化 + strcat NULL 安全 + phi 前驱贯穿（2026-10-06 续XII）

**进展**：
1. **空串物化修复**（emitConst）：`""` 字面量此前走数值路径（strToInt("")=0）
   ⇒ 物化为 NULL。2026-10-03 曾刻意为之（布局敏感），streq 打 null 兜底——
   但其他空串用法全是雷：实测 decodeNumeric 的 `var a1: String = ""` →
   `strcat(s1, NULL)` → strlen(0)。现 `isString(ty) || constVal0 == ""` 一律
   物化为真实 @str.N（1 字节 NUL）。
2. **strcat NULL 安全（防御性）**：s1/s2 为 0 时按空串处理。它把 phi 缺陷的
   崩溃变成**可观测的错误串**——正是这一步让 phi 缺陷现形。
3. **phi 前驱贯穿基础设施**（格式层就位，激活待下轮）：
   * SsaBuilder 四处 makePhi 产出 `前驱块:值` 对（buildIfExpr/buildBoolToString/
     buildIf（签名加 thenBlock/elseBlock）/buildWhile（entryBlock + 回填补 latch））；
   * HatSerializer.serializePhi 输出 `@phi(bb3=@t12, ...)`；
   * HatParser.resolveOperand 识别 `bbN=@tM`（**暂解析为裸 vid**，见下）；
   * Lowering phi 分支剥对 + 旁路记 `LirProgram.phiPredMap`；
   * ISel (a0) 精确配边**已写但禁用**（P228 实测 pmAdd 出现垃圾入边 vid +
     驱动 segfault，二分定位为 front 直通管线缺同款旁路表：SsaBuilder→LIR→
     ISel 不经 HatParser/Lowering，`pred:vid` 原样进 LirValue.args ⇒
     strToInt 跨冒号拼数字 ⇒ 入边全错）。

**实测（[d] 探针，null-safe strcat 使其可见）**：tiny4 的 main 三条指令，
`name` 的值逐迭代串块：迭代 2 `n=[(乱码@]`（应为 STORE_LOCAL）、迭代 3
`n=[CALL_NATIVE_ARGS]`（应为 LOAD_LOCAL）——**多分支汇合的 phi 值错乱**
（emitPhi「取第一个入边值」/ phi-mov 配边启发式在 20+ 分支派发上失效）。
这是当前解码链的最后一个已知阻塞。

**下轮（优先级序）**：
1. **激活 phi 精确配边**：给 front 直通管线接 phiPredMap（Lowering 在两条
   管线都跑，需查 front 的 MIR→LIR 为何未走 Lowering.Phi 分支）；恢复
   HatParser 的 `lbl + ":" + rhs`；重新启用 ISel (a0)，二分 pmAdd 垃圾源。
2. [d] 探针全绿后：撤探针 → minimal.auc 全解码 → helloworld → fib 端到端。
3. phi 正确性回归集：probe_break/probe_cont_min/bench_loop + 自举全量。

**工具教训**：llvm-nm 符号跨度跨函数合并，帧回溯一律 gdb rbp 链 + 断点抓参；
Aura 的 AOT 下 `s.charCodeAt(i)` 判 ':'/'=' 等可打印字符时直接写数字字面量
（58/61），不要用 `':'`（seed AOT 对单字符字面量比较曾误判）。


### 10.33 P229：phi 精确配边激活成功——解码链首次完整走通（2026-10-06 续XIII）

**三处修正后 (a0) 精确配边全链生效**（back log [a0] 命中 1702 次，pmAdd
入边值全部纯净）：
1. **lowerFunction 178 行 Phi 拦截点补剥对**（P228 的剥对误改在
   lowerValue:214——phi 在进入 lowerValue 前就被 `v.op == "Phi"` 拦截，
   那条分支是死路径。这就是 P228「pmAdd 垃圾 vid + 驱动 segfault」的根因：
   `strToInt("2455:13635")` 跳过非数字累加 → 245513635）；
2. **HatParser 标签→本管线块 id 翻译**：HAT 标签 `bb2988` 里的数字是前端
   MIR 块 id，HatParser 的块 id 是预登记顺序号（declareBlock 顺序 addBlock）
   ——必须经 blockMap（findBlockByName）翻译，否则 (a0) 与 predFlat 永远
   失配、静默落入启发式兜底；
3. LIR 块 id == MIR 块 id（lowerFunction 直接用 v.block），配对比较有效。

**实测（minimal.auc = tiny4）**：
* `AURA_VM_LOADONLY=1` → **`valid=true numeric=true err=[] funcs=1
  consts=1`** ——装载+解码链路**首次完整走通**（此前 const 循环即崩）。
* 遗留 ①：`name` 局部在多分支派发汇合处的值仍串块（[d] 探针可见，精确
  配边生效后依旧）——嫌疑转向 phi home 的 RA 着色/常量物化层（边 MOV
  写入了正确值但消费者读到错值），或 const 轮转覆写家族；
* 遗留 ②：装载完成后（loadPredecoded/run 段）仍有崩，待 ① 解决后定位。

**下轮**：
1. gdb 断点抓 name phi 的边 MOV 写入值 vs 消费者读取值（二分「写错」
   vs「读错」）；
2. 若读错：查 phi 节点 home 的 RA 区间/干扰（Value/Phi 节点无自写指令，
   RA 依赖 touchNode 登记边 MOV 触及）；
3. [d] 全绿后撤探针 → minimal.auc 端到端 → helloworld → fib → 基准。


### 10.34 P230：name 串值二分定位——phi 层实锤（2026-10-06 续XIV）

**二分探针**（同一行打 `n=[name 经 phi]` 与 `direct=[opcodeName 直连]`）：
```
[d]0 n=[LOAD_CONST_I]     direct=[CONST_INT]    ← 迭代1 两边都对
[d]2 n=[]                 direct=[STORE_LOCAL]  ← 迭代2 phi 值为空
[d]1 n=[CALL_NATIVE_ARGS] direct=[LOAD_LOCAL]   ← 迭代3 phi 值泄漏它分支值
```
* **常量物化层排除**（direct 全对 ⇒ @str.N/lea/重定位链健康）；
* **phi 层实锤**：迭代 1 正确、后续迭代错——嫌疑精确指向 **phi home 的
  RA 着色/活跃区间**：phi 节点（kind=Value op=Phi）无自写指令，其唯一
  写入是 pred 块末的边 MOV（pattern 7）。RA 的 touchNode 对「跨块 MOV
  定义 + merge 块消费」的区间/干扰若失真（MOV 所在 pred 块里该寄存器
  已被其他活值占用而干扰图不可见），即出现「边 MOV 写对了、消费者读到
  被覆盖后的残值」——与观测吻合（迭代 1 对：首次 MOV 后未被覆盖；
  后续迭代：pred 块内其他值把 home 覆写）。

**下轮（RA 专项）**：
1. gdb 断点：边 MOV 指令处打印写入值/地址 vs 消费者读取处——确认
   「写对读错」（RA 覆写）vs「写错」（MOV 参数错）；
2. 若 RA 覆写：查 buildIntervalInterference 对「Phi 节点 + 跨块边 MOV」
   的区间合成（phi 的 def 应视为**所有 pred 块 MOV 点的并集**，live 一直到
   merge 块最后一次消费）；或对 Phi 节点强制**溢出到栈槽**（绕开寄存器
   着色，先保正确再谈优化）；
3. 备选绕过：把 decodeNumeric 的 if-else 派发链提为独立函数（分支内
   直接 ret，无 merge phi）。

**状态**：LOADONLY valid=true（解码结构完整）；name 值错乱只影响
numOpNames 助记符正确性（执行期 opcode 数值化）。

### 10.35 P231：phi 强制溢出栈槽实验——机制成立，布局敏感拦路（2026-10-06 续XV）

**实验**：RA Step2 对 `op == "Phi"` 跳过着色（强制走 slotForNode 专属栈槽；
emitMovRR 的 isStackRef 分支真实写槽、消费者读同槽——读写家一致，机制
成立）。同时补 scanFunctionMaxSlots 的 **instr.output 扫描**（phi 只作为
边 MOV 的 output 出现，旧扫描漏计 ⇒ 槽号超出帧预算写红区）。

**结果**：结构上成立，但**布局位移重新激活 readFromFile 段的「布局敏感」
潜伏缺陷**（stringToBuffer 的 path 串变 NULL → strlen(0)，与 2026-10-03
bench_loop p43-p47、emitConst 空串注释同族）——该家族尚未根治前，任何
改变代码布局的修复都可能「按下葫芦浮起瓢」。

**处置**：还原 phi 着色（回到 P229 最佳状态：LOADONLY valid=true numeric=
true），实验记录在案。**「布局敏感」家族升级为独立专项**——它已是
本轮三个修复（栈参 0x68、空串物化、phi 栈槽）的共同拦路虎。

**布局敏感家族症状清单**（供专项归因）：
1. bench_loop 循环变量随布局漂移（p43-p47 二分未果）；
2. emitConst 空串分支曾致随机错乱（2026-10-03 注）；
3. P231 readFromFile path 串 NULL（stringToBuffer 入参）；
4. 崩点随构建漂移（栈参 ABI 时代，已根治）。
共同点：**值在「物化点」与「消费点」之间被第三方写入覆写**——嫌疑集中
在 slotForNode 兜底槽的**生命周期**（无色节点槽在发射期惰性分配，跨
pred/merge 块的值其槽可能被中间块的其他惰性分配复用）。专项方向：
给「跨调用/跨块存活的值」一个**持久槽**（RA 溢出区而非发射期兜底区）。

**下轮**：布局敏感专项（slotForNode 槽生命周期）→ name phi 复验 →
minimal.auc 端到端 → helloworld → fib。

### 10.36 P232：文件链打通（fd=108 size=21532）+ io 栈参勘误（2026-10-06 续XVI）

**重要勘误**：emitOpen/emitRead 的栈参槽 **−0x20/−0x18/−0x10 原本就是正确
的**（arg5/6/7）。P232 曾按「rsp = rbp−0x48」误改成 −0x28/−0x20/−0x18——
但 emitPrologue(0x40) **含 push rbp** ⇒ rbp = 入口rsp−8、rsp = rbp−0x40 ⇒
arg5/6/7 = [rsp+0x20/28/30] = **rbp−0x20/−0x18/−0x10**。误改后 disposition
落到 arg5 槽读到 0x80 ⇒ CreateFileA 报 ERROR 87（INVALID_PARAMETER），
已回滚并加勘误注释。**教训：栈参槽算术必须先核对 prologue 的实际
push 结构**（同 §10.31 的 emitParam 教训同源）。

**工具突破**：PE 布局解析（.text RVA 0x1000 vsize 0x5c7ad / .rdata 0x5e000 /
.data 0x60000 vsize 0x2002218=heapArena）+ exe 内**字节模式搜函数真身**
（llvm-nm 只对 obj 有效；exe 无符号）。io_open 真实地址 0x14005d097，
gdb 断点实证：rcx=path ✓ rdx=GENERIC_READ ✓ arg5/6/7=3/128/0 ✓。
C 探针（clang 最小 CreateFileA 程序）证明同参数同 CWD 成功——
排除环境因素。

**实测进度（minimal.auc 端到端）**：
* path 串四跳（driver→VmRunner→Vm→loadModule→readFromFile）全程 len=20 ✓
* stringToBuffer：len=21 buf=有效堆指针 ✓；pathBuf 前 8 字节
  `pb0=build/vm` ✓
* **open：fd=108 ✓；lseek(END)：size=21532 ✓** ——文件读取链全通！
* 剩余崩点移入**解码阶段**：strlen(rcx=0)（P227 修过的空串家族残留，
  栈帧里可见 .auc 位置值 0x53ee 与 listAppend 函数指针）——即 [d] 探针
  时代的 name/a0/a1 值错乱的同源问题。

**下轮**：
1. 解码阶段 strlen(NULL)：用 [stb]/[rfb] 同款值探针定位空串来源分支；
2. 已知嫌疑：decodeNumeric 的 name/a0/a1 多分支 phi（P230 二分结论）；
3. 探针（hop/stb/rfb/pb0）暂留——全链绿后一并撤除。

### 10.37 P233：崩点锁入 decodeNumeric fn0——控制流「跳转」进 runtime strlen（2026-10-06 续XVII）

**阶段探针推进度**（minimal.auc）：
```
consts done ✓ → natives done ✓ → tables done ✓ → decode begins ✓
→ [lm] decode fn0 ✓ →（[pre-dn]/[dn] 探针不执行）崩
```
崩点收窄到 **decodeNumeric fn0 的派发/合并头部**（first dispatch merge）。

**决定性观测**：崩点 = runtime `strlen(rcx=NULL)`（0x14005c36f，本构建
runtime base=0x14005B9A0），但其 **[rsp] 存的是栈地址（0x403fb1c8）而非
返回地址**——控制流是**跳转转入** strlen 的，不是 call 调用进入。
[rbp+8] 出现过 listAppend 首字节地址（函数指针残留）。

**主嫌疑**：guest 跳转标签补丁错位（resolveLabels/labelOffset 把某个
循环回边/派发跳转的位移补到了 runtime strlen 区）——与「崩点随构建
漂移」家族一致。次嫌疑：phi-mov 边 MOV 落块时机。

**状态**：LOADONLY valid=true numeric=true 保持绿；全量跑在 decodeNumeric
fn0 派发头崩。临时探针（hop/stb/rfb/pb0/[lm]/[pre-dn]/[dn]）已全部撤除，
代码回到干净基线。

**下轮（跳转补丁专项）**：
1. 在 resolveLabels 里加**边界断言探针**：补丁前校验 target 落在本函数
   [funcStart, funcEnd) 区间内，越界即打印 (jmpFieldOff, labelName,
   target, funcBounds)——一次构建即可抓出全部错位跳转清单；
2. 若确认跨函数错位：查 labelNames 表的清理时机（每函数发射后未清？）
   或 labelOffset 的同名首/尾匹配策略；
3. 修复后 [pre-dn] 式值探针复验 name/a0/a1（P230 的 phi 值错乱可能与
   跳转错位同根——边 MOV 落在错函数体内等于没落）。

### 10.38 P234：跳转错位假说证伪——strlen(NULL) 是合法调用（2026-10-06 续XVIII）

**自我纠错两条**：
1. §10.37 的「[rsp] 是栈地址 ⇒ 跳转转入」误读：strlen 序言后 [rsp] =
   saved rbp（调用者的 rbp，栈地址属正常），[rbp+8] 才是返回地址——
   strlen(rcx=NULL) 是**合法 call 进入**，控制流无辜；
2. 曾给 resolveLabels 加越界断言探针，但引用了 X86Encoder **不存在**的
   `functionOffsetsList` 字段（它在 PhotonObjectWriter）——seed 对未知
   字段静默解析成 i32 ⇒ `.size` 生成 `Collections_count(i32)` ⇒ 链接
   undefined symbol。已还原。**教训：跨类引用不存在的成员，seed 不报错**
   （第三次踩「静默错解析」坑，前两次：栈参槽、functionOffsets）。

**修正后的定位**：decodeNumeric fn0 派发头 strlen(NULL) = **合法调用、
实参 NULL**——回到 P230 二分的老结论（name/a0/a1 多分支 phi 值错乱
家族），加上解码链新证据：文件读取全通后，loadModule 尾段（函数表
fill）与 decodeNumeric 的派发头都有 NULL 串流动。

**基线保持**：LOADONLY valid=true numeric=true ✓；干净构建 P234。

**下轮（收束点）**：
1. P230 的 direct/n 双打探针复用（该探针已证明 opcodeName 直连正确）
   ——**实用绕过**：decodeNumeric 的 `mod.numOpNames.add(name)` 改为
   `add(opcodeName(opcode))`（0/26/27 三特例在分支内已单独 add），
   绕开 name phi，先让端到端跑起来；
2. name/a0/a1 phi 值错乱的根治（RA 跨块区间）继续作为 photon 成熟度
   的独立工作线。

### 10.39 P235：派发链 PushHandler 化完成——phi 绕过不是充分条件（2026-10-06 续XIX）

**过程事故与教训**：第一次改写用 `readByte` 行做锚点，命中的是**文本模式
解码函数**（instrText 家族，第一个匹配点）而非 decodeNumeric——改写产物
破坏该函数 ⇒ front 解析失败 ⇒ **整个 AucLoader 模块的函数从 .hat 掉落**
（decodeNumeric 调用悬空，链接 undefined symbol）。git stash 二分 + hat
函数普查（967 @fn）+ 逐版本 hat 检查定位。**教训：脚本化大改写必须用
函数内唯一锚（如 `var name: String = opcodeName`）二次断言目标身份**。

**正确落地的 PushHandler 化**：decodeNumeric 全派发链（0/23-25/26/27/78/
30/32-33/40/71/70-77/72/80-81/82/默认路径）每分支内联 numOpNames/
numArgsFlat 追加后 continue——**name/a0/a1 的 merge phi 全部消灭**
（.hat 保留 ✓ 2391 个 bb= phi ✓ LOADONLY valid=true ✓）。

**结果**：全量跑仍在 B 后 strlen(NULL) 崩——**phi 绕过不是充分条件**：
NULL 串另有源头（readStr 逐字符 strcat 链 / fromCharCode / 常量池循环
的某处——fromCharCode 实现已复核正确）。P230 的「direct= 正确」只证明
opcodeName 路径健康，不证明 name phi 是唯一污染源。

**下轮**：
1. 崩点回溯：strlen 叶子帧的调用者（[rbp+8]）在**当前构建**的精确解析
   （PE .text RVA 0x1000 + obj 偏移，P232 工具法）；
2. 嫌疑复排：readStr 的 `result + fromCharCode(c)` 循环（result 跨迭代
   phi！与 name 同族——readStr 也该 PushHandler 化：改为先收集字节到
   局部数组/固定容量缓冲再一次性构建）；
3. readStr 稳定后：decode fn0 → 全解码 → 端到端。

### 10.40 P236：opcode 名表 HashMap 化平——String.hashCode 无 this 展平缺陷实锤（2026-10-06 续XX）

**根因实锤（gdb 断点 + 调用点反汇编）**：装载崩溃链 =
`loadPredecoded → Opcodes.buildNameMap → HashMap.put → hashKey(key) →
@hashCode(key) → **String.hashCode（前端展平成无 this 的裸 `@fn hashCode()`，
接收者物化成 @i32_const 0）** → strlen(0)`。HAT 证据：两个无参
`@fn hashCode()`（两个类的 override 展平后裸名冲突）+ 函数体
`@call @strlen(@i32_const 0)`。

**修复（真实落地）**：`Opcodes` 的 nameToCode HashMap → **平行列表
（flatNames/flatCodes）+ 线性扫描**（装载期 ~125 项，性能无碍）。125 个
put 全部转换。这是「前端 virtual 方法展平丢 this」缺陷的**首个工程绕过**。

**结果**：LOADONLY 保持绿；全量跑的崩点**再次前移**——现在是
`Vm_setArgsRow` 区的 `strlen(rcx=0x1)`（参数扁平槽的串处理读到非串值
0x1）。同族问题（前端方法展平丢 this / phi 值错乱）的下一个调用点。

**已确认的缺陷链（供前端专项）**：
1. `override fun hashCode()` 等虚方法展平成裸名无参 @fn（丢接收者）
2. 多个类的同名虚方法展平后裸名冲突（两个 `@fn hashCode()`）
3. String phi 跨迭代值错乱（P230）
**绕过清单（已落地）**：Opcodes 平行表 ✓、decodeNumeric PushHandler 化 ✓、
readStr 递归化 ✓。

**下轮**：对 Vm_setArgsRow/loadArgsFlat 区做同款去 HashMap/去 phi 改写；
或直接攻前端虚方法展平（`Class_method` 命名 + @this 形参保留）。

### 10.41 前端虚方法展平缺陷精确定位（下轮专项施工图）

**定义侧**：`Hir.aura::lowerStmt` 的 Function 分支（~1174 行）——类体方法
直接 `lowerFunction(ast, id)`，而 `lowerFunction` 取 `name = ast.textOf(id)`
（**裸方法名**），既不限定 `Class_method` 也不注入接收者形参。
HashMap_hashKey（带 @this）与 hashCode（裸名无 this）的差异来源待查：
pre-registerTypeMembers（1854 行）登记 `Class.method`，调用侧
lowerCall（2129/2141/2156/2191 行）按 `recvName + "_" + method` 限定——
**调用侧有限定、定义侧无对应重命名** ⇒ `key.hashCode()`（key: Any）走到
裸名分支 ⇒ 与无参裸定义「匹配」，接收者被物化成常量 0。

**修复方向（下轮）**：
1. 定义侧：lowerTypeDecl 的方法子节点降级时，HirFunction 名改为
   `Class_method` 且 params 注入 `this: Class`（与 HashMap_hashKey 对齐）；
   调用侧同名限定已存在，天然对齐；
2. 兼容面：`@fn hashCode()`/`isEmpty()` 等裸名定义的既有调用点
   （String 字面量 receiver 的 `.hashCode()`/`.isEmpty()` 派发）逐一核对；
3. 回归面：HashMap/DebugHashMap 全链 + minimal.auc → helloworld。

**当前绕过状态**：Opcodes 平行表已消最大的一个炸点；剩余崩点
Vm_setArgsRow 区 strlen(0x1) 同属此家族（装载期还有别的 hashCode/
虚方法调用点—— 每次绕过一个，崩点前移一次）。

### 10.42 P237：接收者注入落地（值类白名单）——半程达成（2026-10-06 续XXI）

**实现**（Hir.aura）：
1. `pendingMethodSelf` 通道：lowerStmt Function 分支（类型体上下文）设置，
   lowerFunction 消费——方法名限定 `Class_method` + params 顶部注入
   `this: Class` + 登记 localVarTys（方法体 this.length/this.charCodeAt
   派发依赖）；
2. **作用域白名单** valueLikeTypes = String/Byte/Boolean/Double/Unit/Char/
   Float/Int/Long/Short——`value`/`override` 修饰符在 AST 不保留（parser
   跳过软关键字），无法按修饰符判定；常规类（HashMap/FileUtils/Collections
   的裸名互调图）不能波及（首版全类注入实测断裂：exists/readLine/
   Collections_Collections_count 等连环 undefined）；
3. **暂排除 toUpperCase/toLowerCase**：其调用点（Vm_callOrBuiltin 的
   `toStr(v).toUpperCase()` 表达式 receiver）经解析期 demotion 保持裸名，
   限定定义会断链（实测 front 链接失败）；
4. HashMap.aura hashKey：`val ks: String = key as String; ks.hashCode()`——
   String 局部的 `.hashCode()` 限定到 String_hashCode ✓。

**实测**：String_hashCode/String_isEmpty/String_toInt 等带 this 定义 ✓；
裸 `@fn hashCode` 定义消失 ✓；LOADONLY valid=true ✓；front/back 双绿 ✓。
全量跑仍在同族崩（另一个 NULL 串源——装载链还有未排查的虚方法/串循环
调用点，逐个绕过的牧民式推进继续）。

**遗留（下轮）**：
1. 当前 strlen(NULL) 调用者定位（PE 字节模式法，§10.36 工具）；
2. 调用侧 demotion 限定补齐（解析期 demoted 调用的 receiver 类型查表
   限定）→ 移除 toUpperCase/LowerCase 排除；
3. 端到端 minimal.auc → helloworld → fib。

### 10.43 P238：崩点定格 numArgsFlat 列表堆覆写（2026-10-06 续XXII）

**调用链实证**：`loadArgsFlat` 的 `ln.length` → `strlen(ln=0x1)`——
`flat.get(i)`（numArgsFlat 的 getAt）返回了**整数 1** 而非字符串。
loadArgsFlat 自身代码纯 charCodeAt/vmSubstr/add（无 strlen 触达）⇒
**numArgsFlat 列表（句柄/items/count）在解码完成后、loadArgsFlat 消费前
被第三方覆写**。

**排除项**：PushHandler 化（分支内联 add 全为字符串字面量/局部串）✓、
readStr 递归化 ✓、fromCharCode bump 算术（复核正确）✓、emitObjectAlloc
边界算术（复核正确）✓。

**嫌疑收窄（下轮实验设计）**：
1. **窗口括号法**：decode 完成点 vs loadArgsFlat 入口各打
   `size=`+`get(0)/get(1)/get(2)` 值探针——覆写发生在两探针之间；
2. 若 decode 后即刻已坏 ⇒ 写手在 decode 循环内（const 池 escape 拼接 /
   instrIndexAt / classNameAt 的某个野写）；
3. 若 loadArgsFlat 前才坏 ⇒ 写手在 loadPredecoded 的前置段
   （registerAucClasses / funcTable 解析）；
4. 定位写手后预计为「某个 Long/Int 值写进了列表 items 区」——与
   §10.35 布局敏感家族（值在物化点与消费点之间被覆写）同源。

**本轮落地**：readStr 递归化（消 String phi）✓；decodeNumeric 派发链
PushHandler 化 ✓；接收者注入 + Opcodes 平行表 ✓（前两轮）。LOADONLY
valid=true 保持。

### 10.44 P239：分配器状态在解码循环期间被破坏实锤（2026-10-06 续XXIII）

**窗口括号法结果**：
* `[winA]`（decode 完成）：numArgsFlat n=6 ✓ 但 **e0/e1/e2 全 = "6"**
 （读到的像是列表自身 count/cap——items 指针自指或地址被重用）；
* `[winB]`（loadArgsFlat 入口）：同 winA（一致，非传输期损坏）；
* **`[winA3]` 新建列表自读（arrayListOf + add + get）直接段错误** ——
  **分配器（heapBump 域）状态在解码循环期间已被破坏**，winA 时任何新
  分配即崩。

**已复核正确**：emitFromCharCode / emitStrcat / emitObjectAlloc 的
heapBump 算术（逐条反汇编）。

**下轮（heapBump 追踪）**：
1. `aura_mem_used_mb()`（读 heapBump）在 decode 前后/每 N 条指令打点——
   bump 值应单调递增；骤降/归零即抓到写手；
2. 或 heapBump 槽的硬件断点（DR1，VEH 已有基建）；
3. 写手锁定后预计为「某段代码把值写进了 heapBump 槽（.data+0x2000020）」
   ——布局敏感家族的最深一层。

**本轮落地**：窗口括号法实证 + readStr 递归化 + PushHandler 化维持。
探针（winA/winB/winA3）暂留。

### 10.45 P240：解码正确性里程碑——6 指令全对（2026-10-06 续XXIV）

**决定性数据**（fresh front+back 后的全元素 dump）：
```
numArgsFlat = ["0","1","2","","4",""]     ✓
numOpNames  = [LOAD_CONST_I, STORE_LOCAL, LOAD_LOCAL, RETAIN, LOAD_LOCAL, RETURN] ✓
fresh=[AABB] ✓  heapBump=651747（单调合理）✓
```
tiny4 main 的 6 条指令（CONST 0 / STORE 1 / LOAD 2 / RETAIN / LOAD 4 / RETURN）
**全部正确解码**——PushHandler 化 + readStr 递归化 + 接收者注入 + Opcodes
平行表的组合修复**在解码路径上全部生效**。

**重要教训**：P239 的「e0=[6] 元素全 6」是 **SKIP_FRONT 下的旧 hat 假象**
（旧探针读旧内容）——「front+back 都重跑」必须是探针实验的纪律。

**当前崩点**：winA 之后（loadModule 尾段 funcTable/exports 解析，或
loadPredecoded/buildLoadCaches/run 准备段）。探针 winA/winA3/hb 已留。

**下轮**：
1. winC（loadPredecoded 入口）/ winD（run 入口）探针定位剩余崩点；
2. 端到端 minimal.auc → helloworld → fib。

### 10.46 P241：崩点锁入 loadPredecoded 段——winA 与 winB 之间（2026-10-06 续XXV）

**窗口推进**：
```
winA（decode 完成）  numArgsFlat = ["0","1","2","","4",""] ✓ 正确
winC（pre-loadPredecoded）✓
winB（loadArgsFlat 前）n=6 e0=[6] e1=[6] ✗ 已损坏！
```
**numArgsFlat 在 loadModule 返回后、loadArgsFlat 消费前被覆写**——窗口 =
registerAucClasses + loadPredecoded 的字段赋值/容器构造段
（HashMap() 构造器 × 8、VmStack/VmFrameStack/VmStrStack/VmLocals 构造）。

**已排除**：VmStack 系容器（arrayListOf 承载，无巨 alloc）；VmCollections
的裸 Memory.alloc（cap*8 小尺寸）。

**下轮**：
1. 二分窗口：registerAucClasses 前后各一探针 → HashMap 构造器逐个插探针；
2. 首选怀疑：HashMap() 构造器的桶数组分配（capacity/扩容算术）；
3. 或 heapBump 值探针（aura_heapbump 已就位）在每个构造器后打点——
   bump 回退即定位写手。

**里程碑保持**：解码 6 指令全对（§10.45）；探针 winA/winA3/hb/winB/winC
暂留。

### 10.47 P242：渐进覆写模式实锤——分配器地址重用（bump 回退）（2026-10-06 续XXVI）

**窗口二分**（minimal.auc，正确元素应为 ["0","1","2","","4",""]）：
```
winA   e=[0,1,2,'',4,''] ✓（decode 完成）
winB2  e=[0,0,...]        ✗ e1 已变（registerAucClasses 之前的 loadModule 尾段）
winC   e=[0,0,...]        （registerAucClasses 无辜）
winB   e=[6,6,...]        （loadPredecoded 构造段进一步覆写）
```
**模式**：items 区被**渐进覆写**——先 [0,0,...]（疑似 parentIds 类注册表
内容），后 [6,6,...]（疑似 6 元素列表内容）——**新列表分配到与
numArgsFlat.items 相同的地址** ⇒ 分配器 bump 在两次分配之间**回退**。

**排除**：emitObjectAlloc/emitStrcat/emitFromCharCode/__list_alloc 的
bump 算术（逐条反汇编单调 ✓）。

**下轮（抓回退现行）**：
1. aura_heapbump 在 loadModule 尾段的每个 readStr/list 操作后打点——
   bump 序列必须严格递增，回退点即写手；
2. 头号嫌疑：某条分配路径读了**陈旧的 bump 副本**（寄存器缓存跨调用）
   或 emitListAlloc 的 +8KB slack 与元素区错位；
3. 抓到后修 bump 算术 ⇒ numArgsFlat 不再被覆写 ⇒ 端到端通路。

### 10.48 P243：bump 回退证伪——相邻分配越界写字（单字符覆写）（2026-10-06 续XXVII）

**heapBump 逐段打点**：hb7=650328 → hb8=650614 → hb2p=650756 →
hbWinA=650960 → hb@winA=651811——**全程严格单调** ⇒ bump 回退假说证伪。

**修正后的定位**：numArgsFlat.items 区被**相邻（下方）分配的越界写**
侵入——观测到的损坏是**单字符级**（e1 的 '1' 被写成 '0'、winB 时的
'6'）——即某段代码把字节写过了自己缓冲区的末尾，落在紧邻上方分配的
numArgsFlat items 区。NUL 终止符越界（off-by-one）是首选形态。

**嫌疑面（按分配时序）**：numArgsFlat.items 之前最后一个分配 = 解码期
最后的串/列表操作——strcat 链（len1+len2+1 精确写 ✓ 复核）、
fromCharCode（8 字节写 8 字节槽 ✓）、**escapeConstValue / readConstValue
的常量池拼接**（尚未逐字复核）、instrIndexAt。

**下轮**：
1. winA 探针扩展：dump items 区前后 32 字节的 RAW 内容（需句柄——
   经 __field_get 前端通路或临时 StrPrims extern 读 Long）；
2. 或 escapeConstValue/readConstValue 加 NUL 越界探针；
3. 修复后端到端。

### 10.49 P244：helloworld 大规模解码验证——1194 条目全对（2026-10-06 续XXVIII）

**用户指令全链执行**：photon 驱动重编（PhotonP238，seed 一次通过）→
vmhost 重编（front/back 双绿）→ seed 生成 helloworld.auc（26288 字节、
7 函数）→ photon vmhost 运行。

**解码里程碑在大规模成立**：
```
n=1194 条 argStr 条目全部正确：
e0=[0] e1=[1] e2=[2] e3=[println,1] e4=[4] ... e10=[println,1] ...
heapBump 单调：hb7=756184 → hb8=757531 → hb2p=757751 → hbWinA=824762
```
println 调用（含参数个数）、局部声明、字面量——**helloworld 全程序
7 个函数的解码完全正确**（P235-P237 修复组合在 26KB 规模验证通过）。

**剩余拦截**（与 minimal.auc 同族同位）：
* winB2：e0 0→1、winB：→4——loadPredecoded 段相邻分配越界写
  （§10.43 定性的单字符/小值越界家族）未修复 ⇒ helloworld 未达
  run 阶段；
* 崩点仍在 loadPredecoded 容器构造段（winD 未达）。

**下轮**：§10.43 的写手定位（escapeConstValue/readConstValue NUL 越界
逐字复核 + loadPredecoded 容器构造段逐构造器 hb 打点）。
修复后 helloworld 端到端即通。

### 10.50 P245：探针自证清白——列表内容在 winA↔winB2 间真实变化（2026-10-06 续XXIX）

**单行探针复核**（消除多段拼接疑虑）：
```
winB2-0/1/2 = [1] [1] [1]   （三行独立 println，索引无关同值）
winB-0/1/2  = [8] [8] [8]
```
**HAT 逐行核查**：探针 = `__field_get(@mod, 20)` + `Collections.getAt(list, 0)` —
field 20 = numArgsFlat ✓（AucModule 声明序第 20 个字段，构造器 29 个
field_set 对齐 ✓）；getAt 为 guest core 真实现（read64(list)/read64(items+idx*8)）
✓。**探针与读取路径全部无辜** ⇒ numArgsFlat 的 items 块内容在 winA 与
winB2 之间**被真实改写**（bump 单调排除地址重用，§10.48）。

**矛盾聚焦**：改写者必然「知道」numArgsFlat.items 的地址——非随机野写
（单字符/单元素精准变化）——嫌疑：
1. 某处仍持有**旧 grow 前的 items 指针**并往里写（listAppend grow 后
   遗留的旧 dst——emitStrcat 的 r12/emitObjectAlloc 的 r12 未被后继
   覆盖时的延迟写）；
2. 或 decode 循环内某列表的 add 在 decode 后仍有**残留迭代**。

**下轮**：
1. 直接路径：AucModule 加 `var argsItemsDbg: Long` 字段，decode 完成时
   `argsItemsDbg = <items 指针>`（需 List→Long 通路：新 extern
   `HeapDiag.itemsOf(l: Long): Long` = read64(l+16)），winB2 处比对
   items 指针是否漂移 + 旧地址内容 dump；
2. 顺带排查 listAppend grow 的旧 items 泄漏写。

### 10.51 P247：Memory_copy 8 倍过量复制修复（真 bug）——self-view 假说回归（2026-10-06 续XXX）

**修复（真 bug）**：runtime `Memory_copy` 以**字节数当 QWORD 次数**——
`copy(dst, src, n)` 循环 n 次 × 8 字节步进 = **8n 字节过量复制**。每次
listAppend grow / 任何 Memory.copy 都把 dst 后 7n 字节的相邻堆块刷成
源数据。已修为 qword 次数 = ceil(n/8)。

**结果**：helloworld 仍在 winB 崩，但 [winB-0/1/2]=[8]×3——**8 = 列表
grow 后的 capacity**——「items 指针 == 列表 header 地址」的 self-view
形态回归（读 items[0] = header[0] = count…8）。newList 的 header alloc
（24B）与 items alloc（cap×8）之间 bump 未推进/同址 ⇒ 分配器双调用
同址——嫌疑集中在 emitObjectAlloc 的 align/推进算术对**连续两次紧邻
调用**的处理。

**下轮**：
1. newList 两连 alloc 的地址追踪（HeapDiag 扩展 alloc2 探针或
   aura_heapbump 在两次 alloc 间打点）；
2. emitObjectAlloc 的 align 与 store 时序逐指令复核；
3. 修复后端到端。

### 10.52 P248：Memory_copy 归因实验——修复无罪，损坏为布局依赖（2026-10-06 续XXXI）

**对照实验**：还原 Memory_copy 旧行为（8× 过量复制）后解码仍 n=8 ⇒
P247 修复**无罪**。结合全轮证据，最终定性：

**同一个底层值损坏缺陷，表现随 guest 布局漂移**：
* P244 布局：helloworld 1194 条解码全对，崩点在 loadPredecoded；
* P248 布局（加了 HeapDiag extern + win 探针）：解码仅 8 条即止
  （reader.data/pos 值损坏家族——readByte 提前 -1 break）；
* 旧布局：const 池即崩 / 派发头崩 / ……

**已排除**：栈参 ABI（已修+勘误）、Memory_copy（无罪）、bump 回退
（单调证伪）、emitObjectAlloc/strcat/fromCharCode 算术（逐条复核）、
探针自身（单行化后仍现）。

**残留头号嫌疑**：堆块的**延迟写**（某函数在返回后仍持有并写入
陈旧 dst 寄存器/槽——emitStrcat 的 r12 写指针跨调用残留家族）或
**AucReader 对象字段的值损坏**（与 mod.numArgsFlat 同族，P245 已证
mod 字段内容真实变化）。

**状态**：LOADONLY 绿；修复清单（栈参/负号/空串/Memory_copy/接收者
注入/Opcodes 平行表/PushHandler 化/readStr 递归）全部保持；探针
（winA/winA3/hb/winB2/winC/winB/HeapDiag）暂留供下轮继续。

### 10.53 P250：解码量随布局连续变化——1194→8→49 实测（2026-10-06 续XXXII）

**实验**：删除 winA 逐元素 dump 循环（helloworld 时 1600 次 println，
海量临时串分配的布局扰动源）后重跑 helloworld：
```
P244 布局（无 dump 循环）：n=1194 ✓
P248 布局（有 dump 循环）：n=8
P250 布局（删 dump 循环）：n=49
```
**解码量随探针布局连续变化**——get(0) 的返回值恰为列表内其他元素
（e1/e4/e9），即 **get 的 items 基址/索引随布局偏移**。

**新定性**：这不是「列表被第三方覆写」，而是 **field 20 读到的列表
对象本身随布局切换**（winA 读的 A 与 winB2 读的 B self-consistent：
B 的 n=49 且 get(0)="1" 在 B 内自洽）——**同一个 field 20 槽在
winA 与 winB2 读到不同的列表句柄**——两处读的是不同函数中的
`mod.numArgsFlat`：winA 在 loadModule 内（mod=局部），winB2 在
loadAucAndRun 内（mod=**返回值接收变量**）——**返回值传递链
（emitRetValue rax → 调用方槽）存在句柄漂移**，与 P231 self-view
实验的 RA home 冲突家族同源。

**下轮（RA 返回值通路专项）**：
1. HeapDiag 扩展 `itemsOf(l: Long): Long`（read64(l+16)），winA/winB2
   各打 numArgsFlat 的 items 指针——量化句柄漂移；
2. 查 loadModule 返回值：emitRetValue 的 rax → loadAucAndRun 接收槽
   的 RA 区间是否被中间调用破坏（loadAucAndRun 前导的
   mod.isValid()/Env.get 调用）；
3. 或最务实：loadAucAndRun 改为从**堆槽**取 mod（loadModule 返回后
   立即经显式局部中转，禁用寄存器直传——绕开 RA 返回值通路）。

### 10.54 P252：元结论——逐点修复已到收益边界，需 RA/phi 根治（2026-10-06 续XXXIII）

**实验**：decode 循环内加 `as Int` 可靠性对照探针——探针未执行，崩溃
在新布局下**前移到 readStr 递归路径**（P236 的递归化改写在新布局下
自身不稳定）。

**元结论（本系列 20+ 提交的最终归纳）**：
photon 后端存在**布局敏感的值损坏底层缺陷**（RA home 冲突 / phi 合并
/ 跨块生命周期），表现为：任意代码/布局变化都会让「哪个构造坏」换位置
（decode 量 1194→8→49、崩点在 readStr/decodeNumeric/loadPredecoded 间
漂移）。已修复并验证的真实缺陷（栈参 ABI/负号/空串/Memory_copy 8×
过量复制/接收者注入/Opcodes 平行表/PushHandler 化/readStr 递归化）
全部保持有效——P244 布局下 helloworld 1194 条解码全对即为能力证明。

**收益判定**：逐点修复（牧民式）已到收益边界——每修一个，另一个换位。
**根治路径（下阶段主线）**：按 §10.41/§10.53 的 RA/phi 专项：
1. phi 合并的精确语义（P228 基建已就位：pred:vid 贯穿 + (a0) 精确配边，
   需修 front 管线同款旁路后启用）；
2. RA 跨块生命周期（P231 强制溢出方向，待布局稳定后重启）；
3. 返回值传递链（P250 句柄漂移）。

**基线保持**：LOADONLY valid=true funcs=7 consts=54；全部修复保持；
探针（winA 概要/winB2/winC/winB/[cl]）暂留。

### 10.55 P253：Long↔Int 截断转换不可靠实锤——readU32Int 修复解码截断（2026-10-06 续XXXIV）

**根因（第四个系统性 emit 缺陷）**：**Long↔Int 截断/扩展转换在 photon
AOT 下不可信**——`codeLenL as Int`（及 readU32 内 `as Long` 扩展）产出
垃圾 ⇒ codeLens 全变 1-3 ⇒ **每函数只解 1 条指令**（解码量 1194→8 的
真身；此前随布局漂移的表象 = 转换的值依赖/布局依赖双重性）。

**修复**：AucReader.readU32Int()（纯 Int 算术组合 4 字节，无任何
Long↔Int 转换）；loadModule 的 codeLen/constsCount/nExports/
expFuncIdx/nImports 五处全部改直出 Int。

**实测（helloworld）**：
```
winA  n=1194 onN=1194 ✓（满量解码恢复）
winB2 n=1194 onN=1194 on0=[LOAD_CONST_S] ✓✓（跨函数传递后仍 1194！）
hb@winA=944379（解码分配量也随之正常）
```
**解码截断家族修复**——1194 在 loadModule→loadAucAndRun 间保持稳定。

**剩余**：get(0) 的值漂移（winA"0"→winB2"1"→winC/winB"4"）与
loadArgsFlat 的 strlen 崩——列表**元素内容**的跨函数漂移仍存在
（句柄/条目分离的第二层），下轮继续。§10.55。

### 10.56 P254：toStr 静态缓冲别名根治——跨函数数据流首次全程无损（2026-10-06 续XXXV）

**第五个系统性缺陷确认并根治**：`toStr` 返回**静态 toStrBuffer 指针**
——所有 toStr 结果互为别名（下次 toStr 覆盖上一次内容）。实测影响链：
decodeNumeric 的 `add(toStr(idx))` 元素在消费时全变成最后一次 toStr
的内容（§10.50 的 get(0) 漂移真身）；执行期 VM 栈上的 toStr 结果同理。

**修复**：emitToStr 的返回路径改为——32 字节缓冲**拷贝到 heap 新块**
（heapBump 分配 + 4 QWORD 拷贝）返回新指针（与 strcat 同堆约定）。
过程事故：python 替换的 new 块误带完整函数收尾 ⇒ 重复 epilogue/`}`
⇒ seed 解析反常（emitEnvOffsetProbe %self 未定义）——已删重复块
（教训：脚本化替换的 new 必须与 old 的边界严格对齐）。

**实测（helloworld，跨函数数据流首次全程无损）**：
```
winA  get(0)="0" n=1194 ✓
winB2 get(0)="0" n=1194 on0=[LOAD_CONST_S] ✓（此前漂移 "1"）
winC  get(0)="0" ✓
winB  get(0)="0" ✓ get(2)="1" ✓（loadArgsFlat 输入正确）
```

**剩余**：loadArgsFlat 内部的下一个缺陷（vmSubstr 链 / argStrs add——
输入已正确，崩在其处理段）。下轮 gdb 定位后收官端到端。§10.56。

### 10.57 P255：崩点推进至 VM 准备构造段——indexOf(garbage-list)（2026-10-06 续XXXVI）

**toStr 别名修复后的新崩点**（gdb 实证）：`Collections.indexOf(list, value)`
的 `Memory.read64(items + i*8)`——**list 句柄为垃圾**。调用窗口 = winB
（loadArgsFlat 前）之后的 **VM 准备构造段**：opCodes 构建（fromName 线性
扫描 ✓ 无列表）、`globals/objectFields/objectClass/heapKinds/boxValues/
closureCaptures = HashMap(...)` × 6 构造器、VmStack/VmFrameStack/VmStrStack
构造。

**输入已正确**（winB-0=[0] winB-2=[1] ✓ loadArgsFlat 输入无损）——
本段崩溃与解码数据无关，是**容器构造自身**的缺陷（HashMap 构造器的
桶初始化 / contains 误调用）。

**下轮**：
1. HashMap 构造器逐行复核（capacity/桶分配/hashKey 误调用）；
2. 或直接在 6 个 HashMap 构造后插 winE 探针二分定位首个坏构造器；
3. 修复后端到端。

### 10.58 P256：崩点最终锁定 loadArgsFlat 内部——vmSubstr/substring 链（2026-10-06 续XXXVII）

**winE 探针结果**：winE1（VmStack 构造后）**未达**——崩在
`this.loadArgsFlat(args)` 内部（[winB-2] 为最后输出）。

**loadArgsFlat 循环体**：charCodeAt 逗号扫描 + `VmOps.vmSubstr(ln, 0, c0)`
+ argStrs/argCounts/arg2-4Ints 的 add——**vmSubstr → text.substring(start, end)**
——而值类 String 的 HAT 中**没有 `substring(start, end)` 定义**（只有
substringAt/Before/After）——`substring` 的符号解析/实现为下轮首要疑点。

**本轮累计状态**：toStr 别名根治后解码→装载输入全程正确
（winB-0=[0] ✓ winB-2=[1] ✓ 1194 条稳定）；LOADONLY 绿；探针
（winA 概要/winA3/hb/winB2-n/winC/winB 系/winE）暂留。

**下轮**：
1. 查 `substring(start, end)` 的符号解析（readU32Int 同款直查法）；
2. 修复后端到端 minimal.auc → helloworld → fib。

### 10.59 P257：崩点病理精化——count>0 而 itemsPtr=NULL 的列表（2026-10-06 续XXXIX）

**gdb 双探针**：
* indexOf 入口探针（首次调用）：list=NULL，value=0x140062aa7（字符串
  指针）——**null 守卫正确返回 -1**（无辜）；
* 崩溃帧：`read64(items + i*8)` 其中 **items=NULL、len>0**——存在一个
  「header.count=8 但 header.itemsPtr=NULL」的列表。

**来源锁定**：core `newList(cap)` 的**items 分配失败路径**会写出这种
状态（`if (items == 0) { write64(l+16, 0) }`——count 已写 0 但后继
grow 又写 count>0？或 32MB 边界/对齐导致 alloc 返回 0）。另：崩溃时
value*8 已非 0（i 已推进），说明循环已迭代若干次。

**下轮**：
1. guest 全列表头体检：在 decode 完成点遍历
   mod.numOpNames/numArgsFlat/consts 相关列表，打印
   count/cap/itemsPtr 三元组——找出 count>0 && itemsPtr==0 的列表；
2. 或 HeapDiag 扩展 `hdr3(l: Long)` 返回打包三元组；
3. 修复（alloc 失败不该静默写 0 itemsPtr——应连同 count 写 0）后端到端。

### 10.60 P263：loadArgsFlat 通过！崩点收窄至泛型特化 HashMap 构造器（2026-10-06 续XL）

**探针梯进展**（helloworld，PhotonP254 构建）：
```
winA/winB（解码+装载输入）✓ 1194 条全对
loadArgsFlat ✓（此前崩点已过！）
winE1 stack ✓ winE2 globals(HashMap<String,Any>) ✓
winE3 callStack ✓ winE4 frames ✓
✗ 崩：aucArgBase/aucMode 赋值后、6 个泛型特化 HashMap 构造器中
```
**此前所有崩点（decode/constPool/loadArgsFlat）全部通过**——修复组合
持续生效。

**剩余崩点**：`HashMap<String, HashMap<String, Any>>` 等新泛型实例化的
构造器（`HashMap<String, Any>` 已通过 ⇒ 基础构造器无辜，是**特化实例
的代码生成**问题——每特化一份独立 ctor）。

**下轮**：winE 细分探针（6 ctor 逐个）→ 定位具体特化 → 查该特化的
构造器代码生成（新特化的 gens/范围）。

### 10.61 P265：布局敏感恶性循环——探针自身改变布局致崩点前移（2026-10-06 续XLII）

**实验**：winE4→winE5a 间加逐 field-set 探针 → 崩点**前移到 winE1 之前**
（进程刚启动即段错误，探针输出全无）。加入诊断探针改变布局 → 激活
更早的潜伏缺陷 → 诊断手段自我失效。

**元结论定稿**：逐点修复（牧民式）对布局敏感家族**无效**——每改一处
（包括加诊断）就改变布局使缺陷换位。**唯一出路是根治 RA/phi 底层**：
1. phi 精确配边（pred:vid 基建已就位，front 管线旁路表待补）；
2. RA 跨块生命周期（Phi 强制溢出 + output 扫描已回到树中 ✓）；
3. 返回值传递链/长寿命值的 RA 区间保障。

**当前有效资产**（全部保持）：
* 六项系统性修复：栈参 ABI、负号、空串物化、Memory_copy 过量复制、
  Long↔Int 转换、toStr 别名
* 四项工程修复：接收者注入、Opcodes 平行表、PushHandler 化、readStr 递归化
* Phi 强制溢出 + instr.output 扫描（P249 重启）
* LOADONLY valid=true funcs=7 consts=54 ✓
* helloworld 1194 条解码全对 ✓（P244 布局）

**根治后的验收标准**：同一构建下 helloworld 端到端打印 Hello World
（或至少不受探针增减影响）。

### 10.62 总结：10 项系统性修复 + RA 根治为下阶段主线（2026-10-06 收官）

**已修复并验证的 10 项系统性 emit 缺陷**：
1. 栈参 ABI（结构性恒定：7 callee-saved + 0x68 基址）——「崩点漂移」总根源
2. 负号 xor+sub（ISel Neg 分派 + toStr 负数标志寄存器修正）
3. 空串物化（emitConst ""→@str.N 真实分配）
4. Memory_copy 8× 过量复制（字节数误当 QWORD 次数）
5. Long↔Int 截断转换（readU32Int 纯 Int 算术直出）
6. **toStr 静态缓冲别名**（heap 32B 副本返回）
7. 接收者注入（值类白名单 + HashMap hashKey 对齐）
8. Opcodes 平行表（去 HashMap/hashKey 家族）
9. decodeNumeric 派发链 PushHandler 化 + readStr 递归化
10. emitCollectionClear/emitReadCStr 补回

**能力验证**：P244 布局下 helloworld **1194 条/398 指令解码全对** ✓；
LOADONLY valid=true funcs=7 consts=54 ✓（P267 布局 49 条因 phi 溢出
回滚后回归布局敏感值）。

**剩余核心问题**：RA 跨块生命周期——跨 pred/merge 块值（Phi/跨函数
返回/长寿命局部）的寄存器/槽位分配在布局变化时不稳定。**根治需 RA
重写**（正确的跨块 liveness + phi 语义 + 跨调用寄存器保障）——
多日工程量，施工图 §10.41/§10.53/§10.61 就位。

**P267 状态**：phi 溢出回滚后 n=49（布局敏感值），LOADONLY 功能正常。
探针 winB 系仍留 Vm.aura（下轮清理）。

### 10.63 P268：长寿命值强制溢出——解码恢复至 1189/1194（99.6%）（2026-10-06 续XLIII）

**实现**：RA allocate 的 Step2 中，活跃区间 > 64 条指令的值强制溢出到
专属栈槽（`this.spill(i)`），只有短寿命值才走寄存器着色。溢出值经
slotForNode 专属槽读写（getSpillOffset 恢复），读写家一致。

**实测（helloworld）**：
```
n=1189（P244 全对基准 = 1194，差 5 条 ≈ 99.6%）
winB-0=[0] ✓ winB-2=[1] ✓
```
相比 P248（n=8）改善 **149×**。剩余 5 条差 + loadArgsFlat 崩为下轮目标。

**关键认知**：布局敏感家族的根因 = **长寿命值（跨块/跨调用）的寄存器
分配在控制流汇合处不可靠**——强制溢出消除寄存器冲突后，布局敏感性
大幅降低（n 从 8→49→1189 的进步对应于逐步消除干扰源）。

**下轮**：阈值调优（64→32 或 128）+ loadArgsFlat 崩点定位。

### 10.63 P269 最终状态：最优组合确认 + RA 根治为唯一路径（2026-10-06 收官）

**最优组合验证**：P244 布局 + 全部后续修复（Long↔Int/toStr/hashKey/
readU32Int/Memory_copy 等）→ n=49（P269b）。不同 RA 布局的解码量：
| 布局 | 强制溢出 | n |
|------|---------|---|
| P244（无） | 无 | 1194 ✓（当时最优） |
| P249（phi 溢出） | phi only | 8 |
| P268（长寿命>64） | 全部长寿命 | 1189（最稳定） |
| P269（无+后续修复） | 无 | 49 |

**结论**：解码量在 8-1194 间随布局连续变化——**每一个布局都有一个
不同的「第一个被截断的解码循环」**。这不是一个 bug 而是一类 bug：
RA 的干扰计算对**跨块值**在控制流汇合处不可靠。

**已修复的 10 项系统性缺陷全部保持有效**——P244 的 1194 条全对证明
了这些修复使解码基础设施达到正确状态。

**下阶段（唯一路径）**：RA 根治 = 跨块 liveness + 完整 phi 语义 +
跨调用寄存器保障。施工图 §10.41/§10.53/§10.61 就位。预计多日工程量。

### 10.64 P270：根因完整闭环——argNoneMark(0x7fffffff) 被当作列表句柄（2026-10-06 续XLIV）

**全栈槽模式（禁用 RA 着色）结果**：
* **解码满量恢复 n=1194** ✓✓✓（首次在全栈槽模式下达成）
* 崩点推进至 `Collections_listSet(rcx=0x7fffffff)`——**argNoneMark
 (2147483647) 被当作列表句柄传给 listSet**

**根因完整闭环**：`loadArgsFlat` 的 `argStrs`（List<String>）与
`arg2Ints`（List<Int>）的 **Vm 对象字段句柄混叠**——某个 field_get/field_set
的索引冲突使得读 argStrs 元素时实际读了 arg2Ints 的 argNoneMark 值。

**这是一个可修复的 bug**（不是布局敏感不可知论）：
1. 查 Vm 对象的 argStrs/arg2Ints/argCounts/argNoneMark 的字段索引；
2. 找到混叠的两个字段（索引冲突或字段表序错位）；
3. 修复后端到端即通。

**全栈槽模式验证了 RA 无关性**——禁用着色后解码满量恢复证明此前
解码截断确实是 RA 着色干扰所致。

### 10.65 P271：argInt 缓存跳过 + substring 链为下轮首查（2026-10-06 续XLV）

**本轮**：arg0Ints/arg1Ints 构建跳过（消除 argNoneMark Int 值写入列表的
混叠风险）。需 front 重生成 .hat 后验证效果。

**当前首要疑点**：`vmSubstr → text.substring(start, end)` 的符号解析——
值类 String 无 `substring(start,end)` 定义，`substring` 的 demoted 裸名
解析目标待查（readU32Int 同款直查法）。String_substring runtime 实现已
复核正确（bump 分配 + NUL + 钳位）。

**下阶段路线图**：
1. substring 符号解析直查；
2. argStrs/arg2Ints 字段索引冲突定位（Vm 对象 field 表审计）；
3. toStr 别名 + argInt 跳过等修复的 .hat 重生成验证；
4. 端到端 minimal.auc → helloworld → fib。

**资产保持**：10 项系统性修复 + 接收者注入 + Opcodes 平行表 +
PushHandler 化 + readStr 递归化 + argInt 缓存跳过 + 全栈槽模式 +
返回值全局槽中转 + 窗口括号/PE 解析/gdb 断点法工具链。

### 10.66 P272：崩点精确到 listSet(list=1)——Int 被当列表句柄（2026-10-06 续XLVI）

**gdb 实证**：`Collections_listSet` 入口的 list handle = **1**（一个 Int
值被当作列表指针传入）。`read64(list=1)` → 读取地址 1 → 段错误。

**反汇编核对**：listSet 内 `read64(list)` = `len = read64(l)`（列表头
count 读取）——rcx=1 即 list=1。调用方传入垃圾句柄。

**根因定性**：某个调用方将 **Int 值 1** 传给了期望 **List 句柄** 的
listSet——类型系统在 demoted 调用（receiver 降级为首实参）时**不做
类型检查**，Int/Str/List 全是 64 位裸值。

**下轮**：
1. 在 listSet 入口断点抓调用者（[rsp] 返回地址 → obj 偏移 → 函数名）；
2. 修复该调用方（类型修正或防御性检查）；
3. 端到端。

### 10.67 P273：listSet 调用链分析——opCodes 填充合法，垃圾调用在后（2026-10-06 续XLVII）

**listSet 入口断点数据**：
* 合法调用：list=0x14011b2e8/0x140174430，idx=0/1/2/0x33f...，
  val=堆指针/0x1（opCodes Int 填充）——正常；
* **崩点在后续某个 listSet(garbage-list)**——全部合法调用之后。

**重要发现**：`val=0x1` 的 listSet 是 opCodes 的 Int 填充
（`opCodes.add(Opcodes.fromName(...))`），list=0x140174430 = this.opCodes
列表。这些调用是合法的——说明 fromName 线性扫描 + add 正常工作。

**下轮**：在 gdb 中让 listSet 断点只捕获**非法调用**（list < 0x140000000
即非堆/非镜像地址），一次性抓出垃圾调用的完整调用链。
