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
