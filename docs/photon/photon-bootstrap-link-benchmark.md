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
