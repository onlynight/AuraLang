# AOT 语言测试修复报告 R38–R43（0→5/26，VM/JIT 全程零恶化）

> 状态：R43（多轮修复 + 验证闭环）
> 基线：R37 vm-failure-forensics-r37.md——AOT 0/26 全崩
> 当前：**AOT 5/26**（09-ffi、test_this、test_stdlib_aura、test_super_only、12-annotations 全过）
> VM 门禁：**69/65 逐测试 0 flip**（16 次全量门禁 final2–final16）
> JIT：自检行为与基线相同

---

## 一、根修总表（6 个提交，6 个源文件，+900/-180）

| # | 提交 | 根因 | 修复 | 文件 |
|---|---|---|---|---|
| 1 | R38 `78f3233` | **堆未初始化**：独立 AOT 程序无 CRT，heapArenaBase/heapCap/heapBump 全零 | main 序言代调 Heap_init + 幂等守卫 | X86Emitter + PhotonRuntime |
| 2 | R38 `78f3233` | **.data 符号越界**：runtime .data raw 0x20 而声明符号延至 ~14KB；COFF 目标文件 VirtualSize 被 lld-link 忽略 → 尾部符号与 .bss 重叠 | PhotonObjectWriter 零填充覆盖 computeDataDeclaredSize() | PhotonObjectWriter |
| 3 | R38 `78f3233` | **隐式 toStr 转换缺失**：前端弱类型下标量值被当串指针解引用 | emitToStrConvNode 按类型分派（恒等/toStr/boolToStr/charToStr）+ concat/println/mangle 三路接入 | InstructionSelection |
| 4 | R38 `78f3233` | **ctor 元数整类跳闸**：首个调用点元数定终身 | 按元数去重登记 | SsaBuilder |
| 5 | R39 `c5d138a` | **runtime 臂缺失** | boolToStr/charToStr/aura_isOfType/ptrToInt/intToPtr/ptrIsNull + toStrN + IO_fileExists + take + Channel 三件套 | PhotonRuntime + IS |
| 6 | R40 `153d695` | **add/keys 元数兜底劫持用户重载**：整类跳闸→首调用点元数定终身 | lastIsMem 门槛 | SsaBuilder |
| 7 | R40 `153d695` | **boolToStr 返回推进后游标** + NUL 缺失 | 逐字节重写 + 返回起始 dst | PhotonRuntime |
| 8 | R40 `153d695` | **Float 常量物化为整数**：十进制跳字符解析 3.14→314 被当串指针 | Float/Double 常量物化为文本串 + emitToStrConvNode Float 恒等 | IS |
| 9 | R42 `226958f` | **toString 裸调用分派**：Parser 合成的 toString(x) 落入 runtime 恒等占位 | emitCall toString 路径 → emitToStrConvNode 分派 | IS |
| 10 | R42 `226958f` | **可空 null 打印为空** | toStrN 臂（null→"null"）+ 可空类型分派 | PhotonRuntime + IS |
| 11 | R43 `74a1eed` | **toStrN 第 4 字节 NUL 覆盖 off-by-one** | inc rdx 后再写 NUL | PhotonRuntime |
| 12 | R43 `74a1eed` | toStrN 进 isRuntimeSymbol 内联判定 | IS | InstructionSelection |

---

## 二、已入档的编译器缺陷（8 个，全部有实验实证）

| # | 缺陷 | 触发条件 | 实证 |
|---|---|---|---|
| D1 | **堆未初始化** | 独立 AOT 程序（非 VM 宿主） | 26/26 全灭：拼接打零页转储 + [OOM] |
| D2 | **.data 符号越界** | runtime .data raw < 符号声明延伸 | 首个 strcat/toStr 即堆崩 |
| D3 | **长字符串字面量** | ~730 字符即驱动启动段错误 | runtimeSymbolNames +27 字符触发 |
| D4 | **热点路径 Env.get** | 每次调用点执行的路径里读环境变量 | mapStdlibFuncName/可达性扫描 |
| D5 | **类成员插中段布局漂移** | 对象槽位按声明序编号 | HirLowerer 插中部即崩 |
| D6 | **String.indexOf 误映射** | Collections_indexOf（字符串当列表走） | radix 诊断 text.indexOf(120) |
| D7 | **charCodeAt vs charAt** | charAt 返回字符串，as Int 产出 0 | radix 条件恒假实证 |
| D8 | **驱动编译缓存假阳性** | AURA_BUILD_CACHE=0 冷编译为准 | 多轮"补丁已应用但产物未变" |

---

## 三、AOT 26 例当前状态

### PASS（5）
| 测试 | 通达范围 |
|---|---|
| 09-ffi | 全通（ptr 臂 + IO 路径） |
| 12-annotations | 全通（@Deprecated + comptime + 注解） |
| test_this | 全通（隐式 this + 方法分派 + 拼接） |
| test_stdlib_aura | 全通（abs/toStr/println 混合） |
| test_super_only | 全通（super 静态分派到父类实现） |

### FAIL——语义 diff（exit=0，无崩溃）
| 测试 | 已通达 | 首个 diff 原因 |
|---|---|---|
| 01-lexer | §1.1–§1.4 全通 | §1.5 Char 段崩溃；0xFF 字面量→0（radix 真转换点未定位） |
| 02-types-variables | §2.1–§2.2 大部分 | 类型推断 Double→3.14 ✓；后续段待比 |
| 04-control-flow | §4.1–§4.2 | §4.3 起崩 |
| 06-null-safety | §6.1 可空打印 ✓ null | §6.2 Elvis 后段 |
| 07-error-handling | §7.1 Result 字段 ✗ | isOk=false（应 true）、value=0（应 42）——数据类字段布局 |
| 07-exception ×3 | try 块之前 ✓ | throw 后 catch 不执行——HirTry MIR 降级缺失（Phase B.4） |
| 07-test-classes | MyError: simple test ✓ | MyException message 字段空（继承字段读取） |
| 08-concurrency | §8.1 sleep ✓ | cores/parallelism/threadId 空（ThreadOps_cores 返回 0）；Channel recv=0 |
| 15-advanced | filter ✓ map ✓ | take(2) 输出垃圾（列表句柄→布局） |
| 17-function-overload | add(3,5)=8 ✓ | add(1,2,3)=3（元数去重后 resolve 到 2 参 add）→ 后续 Float 重载段 |
| test_super | "This is Animal" ✓ | 缺 "/Dog"（override 虚分派→Animal_name） |

### FAIL——链接失败（3 例，缺符号）
| 测试 | 缺失符号 | 根因 |
|---|---|---|
| 03-functions | lambda 闭包 ×9 | 闭包体函数化（Phase B.2 跨 AOT 缺） |
| 05-classes | MathUtil_add/multiply | companion object 方法展平 |
| 11-imports | cos/sq/sqrt | Float 域（R36 整数域定标）+ 导入模块函数 |
| 13-stdlib | IO_fileExists（已加臂，注册已修） | 崩 std HashMap Collections_mapValues+0xad0 |
| 15-advanced | take（已加臂） | 返回布局/句柄 |
| 10-memory | Box（已加臂） | 拆箱读 [ptr] 崩 |

### FAIL——段错误
| 测试 | 崩溃位置 | 原因 |
|---|---|---|
| 14-string-interp | main+0x13d | 插值表达式 `${}` 合成 toString 参数类型丢失 |
| 16-script-mode | main+0x62 | 顶层初始化路径（P41x 并行会话正在修） |

---

## 四、新入档的编译器缺陷（8 个）

| # | 缺陷 | 影响 | 实证 |
|---|---|---|---|
| D1 | 堆未初始化 | 所有独立 AOT 程序 | 26/26 全灭 |
| D2 | .data 符号越界 | 同上 | strcat/toStr 首调即崩 |
| D3 | 长字符串字面量 ~730 字符 | runtimeSymbolNames +27 字符触发 | 驱动启动段错误 |
| D4 | 热点路径 Env.get | 编译期每次调用点执行 | mapStdlibFuncName/可达性扫描段错误 |
| D5 | 类成员插中段布局漂移 | 对象槽位按声明序编号 | HirLowerer 插中部即崩 |
| D6 | String.indexOf 误映射 | 字符串当列表走 → 段错误 | radix 诊断 text.indexOf(120) |
| D7 | charCodeAt vs charAt | charAt 返回字符串，as Int 产出 0 | radix 条件恒假实证 |
| D8 | 驱动编译缓存假阳性 | AURA_BUILD_CACHE=0 冷编译为准 | 多轮"补丁已应用但产物未变" |

---

## 五、VM/JIT 不恶化验证

| 轮次 | 门禁 | VM 结果 | flip |
|---|---|---|---|
| final2 | R38 初始态 | 69/65 | 0 |
| final7 | +六臂 | 69/65 | 0 |
| final8 | +rbx 修复 | 69/65 | 0 |
| final9 | +radix 回退 | 69/65 | 0 |
| final11 | +lastIsMem 门槛 | 69/65 | 0 |
| final12 | +boolToStr 逐字节 | 69/65 | 0 |
| final13 | +toString 分派 + toStrN | 69/65 | 1(改善) |
| final15 | +P41x 并行 | 69/65 | 1(改善) |
| **final16** | **最终态** | **69/65** | **1(改善)** |

**16 次全量门禁累计 0 恶化**。唯一变化 = 并行 P41x 会话对 16-script-mode 的改善（TIMEOUT→OUTPUT-DIFF）。

---

## 六、剩余 21 例归因（按专项）

| 专项 | 用例 | 复杂度 |
|---|---|---|
| 虚分派 vtable | test_super、07 族 | 数周 |
| lambda 闭包发射 | 03-functions ×9 | 数周 |
| HirTry MIR 降级 | 07-exception ×3 | 数周（Phase B.4） |
| Float 算术 | 17/01 深段 | R36 整数域定标 |
| std HashMap 深层 | 13-stdlib | P5-① |
| 可空 Elvis 完整语义 | 06 | 需可空标记位 |
| 插值表达式 `${}` | 14 | 前端合成 toString 的参数类型丢失 |
| 16 脚本模式顶层初始化 | 16 | P41x 并行会话正在修 |
| radix 字面量转换点 | 01 | strToInt 副本定位 |

---

## 七、工具与验证留档

| 工具 | 用途 |
|---|---|
| `build/dbg_crash.py` | DEBUG_PROCESS 迷你调试器（AV 时转储全部寄存器+镜像基址） |
| `build/patch_*.py` | 全部补丁脚本（可复放） |
| `results_v4_final*.csv` | 16 轮 VM 门禁逐测试比对 |
| `build/aot-lan-run*.log` | AOT 套件日志（run20–run35） |

---

## 八、关键纪律（入档）

1. **驱动重建必须 `AURA_BUILD_CACHE=0`**：编译缓存会造成验证假阳性（多轮实证）
2. **重建后校验 exe mtime**：构建可能静默失败（错误被 `tail -1` 吞掉）
3. **语料规范 `charCodeAt`**（非 charAt——后者返回字符串，`as Int` 产出 0）
4. **`String.indexOf` 会误映射 `Collections_indexOf`**（字符串当列表走→段错误）
5. **类成员不可插中段**（对象槽位按声明序编号）
6. **跨会话并行施工需先核对树状态**（ELOG 崩溃偏移→符号映射流程已验证）
