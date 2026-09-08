# AOT 机器码嵌入 —— Phase 1 实现检查报告

> 依据：`docs/AOT机器码嵌入方案-详细设计.md`（§3 段布局 / §4 调用约定 / §5 描述符 /
> §7 VM 分发 / §8 安全模型 / §9 任务表 / §11 文件清单与不变量）
>
> 结论：**Phase 1 核心目标已达成并可运行** —— `fun add(a: Int, b: Int): Int` 经
> AOT 机器码嵌入 `.auc` v4，VM 加载后**真实执行 mmap 的机器码**，结果与解释器一致。

---

## 0. 验证结果汇总

| 项目 | 结果 |
|------|------|
| `cargo check -p compiler`（默认） | ✅ 通过 |
| `cargo check -p compiler --features jit` | ✅ 通过 |
| `cargo check -p compiler --features llvm` | ✅ 通过 |
| `cargo check -p cli --features llvm` | ✅ 通过 |
| `cargo test -p compiler --lib` | ✅ 197 passed / 0 failed |
| `cargo test -p compiler --lib --features llvm` | ✅ 234 passed / 0 failed |
| `cargo test -p compiler --lib --features jit` | ✅ 198 passed / 1 failed（`jit_opt::test_fold_constants`，**既有**缺陷，与本次改动无关，见 §6） |
| `cargo test -p compiler --features llvm --test aot_embed_tests` | ✅ 4 passed —— **真实机器码执行**：Int / Float / Bool / 循环内多次 AOT 调用 |
| `cargo test -p compiler --test vm_tests` | ✅ 18 passed |
| CLI 冒烟：`aura build x.aura --aot-embed` | ✅ 生成 `.auc` v4（含 AOT 段，8613 字节） |

新增单元/集成测试 34 项（基线 197 → llvm 234），其中 Phase 1 新增：
- `vm::abi` 5 项（JitValue 编解码 / 标签 / 对齐）
- `vm::mmap_util` 4 项
- `vm::aot_runtime` 7 项（描述符对齐 / 段解析 / 分发表 / 卸载）
- `codegen::opcode` 8 项（CALL_AOT 字节 / AuraFuncDesc C 布局 §5.1 / 段表 / header_flags）
- `codegen::serialize` +2 项（.auc v4 含 AOT 段完整往返 + AotModule 可消费；**v3 字节流手工构造向后兼容读取**）
- `codegen::aot::linker` 2 项（符号名元数据解析 / arg_tags 编码）
- `codegen::aot_embed` 3 项（段组装 / 填充 / AotRuntime 可消费）
- `tests/aot_embed_tests.rs` 4 项端到端（§1.14）

---

## 1. 任务符合性矩阵（设计文档 §9 Phase 1 任务表）

| 任务 | 设计文件 | 实现位置 | 状态 | 说明 |
|------|---------|---------|------|------|
| 1.1 扩展 JitValue 标签 | `vm/jit.rs` | `vm/abi.rs`（新建）+ `vm/jit.rs` 重导出 | ✅ | TAG_INT…TAG_CSTRING 12 种；jit.rs 删除本地重复定义，`pub use` 自 abi，`JitEntry = AotEntry` 别名保持兼容 |
| 1.2 定义 AuraFuncDesc | `vm/aot_runtime.rs` | `codegen/opcode.rs` | ✅* | 32 字节 `#[repr(C)]`；偏移经 `offset_of!` 测试逐字段验证（§5.1）；*偏离：放 opcode.rs 使 serialize 与 runtime 共用同一类型、避免 codegen→vm 循环依赖（见 §5.1） |
| 1.3 新增 AotEntry | `vm/aot_runtime.rs` | `vm/abi.rs` | ✅ | `unsafe extern "C" fn(*const JitValue,*mut JitValue,usize,*const ())`，与 JitEntry 同签名 |
| 1.4 AotModule/AotRuntime | `vm/aot_runtime.rs` | `vm/aot_runtime.rs` | ✅ | 加载（mmap 机器码）/解析描述符/重建分发表/卸载/调用 |
| 1.5 mmap 封装 | `vm/mmap_util.rs` | `vm/mmap_util.rs` | ✅ | Unix `mmap/mprotect`、Windows `VirtualAlloc/VirtualProtect`；W^X 顺序：RW 映射 → 拷贝 → RX 保护 |
| 1.6 .auc v4 | `codegen/serialize.rs` | `codegen/serialize.rs` | ✅ | `VERSION=4`；函数级 `aot_mode u8 + aot_desc_idx u32`；段表（count + id/off/size/flags）+ 段数据区；**v3 文件仍可读**（version<4 时跳过新字段） |
| 1.7 OutputFormat::Blob | `codegen/aot/mod.rs` | `codegen/aot/mod.rs` | ✅ | `Blob` 变体 + `AotOutput.blob_path/.descriptors` |
| 1.8 link_to_blob | `codegen/aot/linker.rs` | `codegen/aot/linker.rs` | ✅ | 用 `object` crate 解析 ELF/COFF；提取 `.text` 写 blob；遍历 `aura_aot_*` 符号 → 描述符（entry_offset 相对 .text 基址） |
| 1.9 JitValue ABI 包装 | `codegen/aot/emit.rs` | `codegen/aot/emit.rs` | ✅ | `blob_mode` 下为每个非原生函数生成 `aura_aot_<名>!<nargs>!<rettag>!<tags…>` 包装：解包 args[2i+1] payload → 转真实类型 → call → 打包 ret[0/1]（§6.3-§6.5） |
| 1.10 OP_CALL_AOT | `codegen/opcode.rs` | `codegen/opcode.rs` | ✅* | 指令/byte=**77**/operand_size=2/from_byte/write/Display 全链路 + VM `Instr::CallAot` 解码；*偏离：设计写 74，但 74 已被 `EnumConstruct` 占用，改用空闲字节 77（见 §5.2） |
| 1.11 emit_call_aot | `codegen/emit.rs` | ——（改为分发器级） | ⚠️* | 未在发射器把 Call 改写为 CallAot；改为 VM `do_call` **先查 AOT 分发表**（aot_mode>0 的函数命中即 call 机器码，未命中回退字节码）——与 §7.2 分发模型语义一致且更强（纯字节码模块也可受益），无需改写调用点（见 §5.3） |
| 1.12 VM 分发器 | `vm/interp.rs` | `vm/interp.rs` | ✅ | `Instr::CallAot → do_call_aot`；`do_call` 内置 AOT 优先派发；`try_call_aot` 经 `AotRuntime::call_func_by_idx` 查多模块分发表 |
| 1.13 CLI | `cli/src/main.rs` | `cli/src/main.rs` | ✅ | `aura build <f> --aot-embed`：字节码编译 → HIR → AOT Blob 编译 → 段组装 → 写 `.auc` v4；失败回退纯字节码并告警 |
| 1.14 集成测试 | `tests/aot_embed_tests.rs` | `tests/aot_embed_tests.rs` | ✅ | 4 项 E2E（机器码执行 vs 解释器） |


---

## 2. 设计文档关键章节逐条检查

### 2.1 §4 共享调用约定（JitValue ABI）
- `JitValue { tag: i64, payload: i64 }` `#[repr(C)]` —— ✅
- Float 用 `to_bits()` 编码进 payload —— ✅（`JitValue::from_value/to_value`，abi.rs）
- `AotEntry` 签名与 `JitEntry` 完全一致 → VM 分发分支对称 —— ✅（类型别名验证：`type JitEntry = AotEntry`）
- AotCallContext（runtime/module_id/func_idx/call_depth/exception）作为第 4 参 ctx —— ✅（`vm/abi.rs`；call_func 传入）

### 2.2 §5 AuraFuncDesc 32 字节布局
`test_aura_func_desc_c_layout_matches_design` 用 `offset_of!` 验证：
0x00 name_offset / 0x04 name_len / 0x06 pad / 0x08 entry_offset(u64) / 0x10 num_args /
0x11 arg_tags / 0x12 return_tag / 0x13 flags / 0x14 source_line / 0x18 source_file_offset / 0x1C pad —— ✅ 全部命中
`AuraFuncDesc::SIZE == 32` 编译期断言 —— ✅

### 2.3 §3.5 `.auc` v4 段表
- 段表紧随 v3 布局；条目 `{id u32, offset u32, size u32, flags u32}`（16B）；段数据区在表后 —— ✅ serialize.rs
- 段 ID：SEG_BYTECODE=0 / MACHINE=1 / DESC_TABLE=2 / DEBUG=3 / STRING_POOL=4 / SIGNATURE=5 —— ✅
- header_flags 位：HAS_MACHINE_CODE=1<<0 … HAS_AOT_EXPORTS=1<<3；`compute_header_flags()` 自动置位 —— ✅
- v3 兼容：v4 读取器对 version<4 文件按 0/空处理 —— ✅（`test_roundtrip_basic` 仍过）
- 测试 `test_roundtrip_v4_with_aot_segments`：含 AOT 段的模块 to_bytes → from_bytes 无损，且段数据可直接交给 `AotModule::load` —— ✅

### 2.4 §7 VM 加载器与分发器
- Vm 新增 `aot_runtime: AotRuntime`；`Vm::new` 在 `module.has_aot()` 时自动加载（mmap 机器码、解析描述符、重建分发表）；失败仅告警、回退解释 —— ✅
- `Instr::CallAot → do_call_aot` —— ✅
- `do_call` 内置 AOT 优先派发（命中分发表直接 call，未命中解释）—— ✅（§7.2 语义超集）
- 卸载 API `unload_module`（供 Phase 3 热重载）—— ✅

### 2.5 §8 安全模型（W^X）
- 机器码段映射权限序列：`read_write` 映射 → 拷贝字节 → `protect(read_exec)` —— ✅（aot_runtime.rs `load`）
- 机器码段置 PROT_EXEC、其余段 PROT_READ —— ✅
- 测试在全套件中真实执行 mmap 机器码（Windows VirtualAlloc + VirtualProtect 路径）—— ✅

### 2.6 §11 关键不变量
| 不变量 | 状态 |
|--------|------|
| `vm/ffi.rs` 的 CFuncPtr / resolve_static_symbol / aura_callback_trampoline 保留 | ✅ 未触碰 |
| `JitEntry` 签名不变，仅扩展标签值 | ✅ `JitEntry = AotEntry`，标签由 5 种扩到 12 种 |
| `OutputFormat::Executable` 独立进程路径继续工作 | ✅ 未改该分支；llvm 套件 234 项含既有 AOT 测试全过 |
| v3 `.auc` 在 v4 VM 可加载 | ✅ 版本分支读取 |


---

## 3. 实现中发现并修复的既有 Bug（AOT 后端激活后暴露）

| Bug | 位置 | 修复 |
|-----|------|------|
| 浮点二元运算缺逗号：`fmul float %a float, %b` | `codegen/aot/emit.rs` | 去除多余类型占位（fadd/fsub/fmul/fdiv 4 处） |
| 浮点字面量 `2.0` 打印为 `2`，llc 报错 | `codegen/aot/emit.rs` `Literal::Float` | 无小数点/指数时补 `.0` |
| 包装函数名含 `!`（元数据编码）非法 | `codegen/aot/emit.rs` | LLVM 引号标识符 `@"aura_aot_…!2!0!0!0"`（COFF 符号名保留 `!`，link_to_blob 可解析） |
| llc 输出机器码段长度非 16 对齐导致运行时拒载 | `vm/aot_runtime.rs` | 段长度只要求 >0（mmap 按页对齐已满足）；**函数入口保留 16 字节对齐检查** |

## 4. 端到端证据

```
$ aura build build_test.aura --aot-embed --output build_test_aot.auc
✓ AOT 嵌入: 机器码 130 字节, 2 个函数描述符
已生成 build_test_aot.auc (8613 字节, 2 函数, 1 常量)
```

**CLI 里程碑闭环**（设计 §9 里程碑：`aura compile --aot-embed` → `aura run foo.auc`）：
```
$ aura build clitest.aura --aot-embed --output clitest.auc
✓ AOT 嵌入: 机器码 226 字节, 3 个函数描述符
已生成 clitest.auc (8790 字节, 3 函数, 1 常量)
$ aura run clitest.auc        # 加载含 AOT 段 .auc，经分发表执行机器码
42
$ aura run clitest.aura       # 对照：纯解释执行
42
```

`tests/aot_embed_tests.rs` 四项测试在 **llvm 23.1.0** 工具链下通过：
1. `aot_add_matches_interpreter`：`add(20,22) == 42`（main 字节码 → 分发表命中 → 机器码）
2. `aot_float_matches_interpreter`：`mul(1.5, 2.0) == 3.0`
3. `aot_bool_matches_interpreter`：`gt(7,3) == true`
4. `aot_repeated_calls_match_interpreter`：循环内每次调用都命中 AOT 机器码

## 5. 与设计的偏差（有意为之，均已注明）

| # | 设计原文 | 实现 | 理由 |
|---|---------|------|------|
| 5.1 | AuraFuncDesc 定义在 `vm/aot_runtime.rs` | 定义在 `codegen/opcode.rs` | 描述符需要被 codegen（serialize、embed）与 vm 两侧消费；vm 依赖 codegen，故放在 opcode.rs 避免反向依赖 |
| 5.2 | OP_CALL_AOT 分配 opcode 74 | 分配 **77** | 74 已被 EnumConstruct 占用；77 为空闲字节（44–65、其余已用除外） |
| 5.3 | 任务 1.11 发射器改写为 OP_CALL_AOT | 未改发射器；VM `do_call` 分发器级 AOT 优先 | §7.2 模型一致：所有 Call 先查 AOT 分发表；无需字节码重写，纯字节码模块可无缝获得 AOT 提速；`CallAot` 指令 + `do_call_aot` 已实现，保留给显式路径 |
| 5.4 | Phase 1 里程碑只列 Int/Float | 实现已覆盖 Int/Float/**Bool/Unit** | 包装器生成时对 Bool/Unit 一并处理（§9 Phase2 任务 2.2 提前完成） |
| 5.5 | 符号名元数据 `!` 分隔由 emit 编码 | linker 侧 `parse_aot_symbol_name` 解析（且去 `aura_aot_` 前缀返回原始函数名） | 与设计 §6.2「链接器提取描述符」一致，便于 embed 按名匹配函数 |

## 6. 未解决 / 既有问题（非本次引入，均已用 HEAD worktree 复现确认）

**默认 feature：250 passed / 4 failed**（失败全在 `p10_concurrency_tests` 的 channel/actor select ×3-4，
HEAD 上同样失败 4 项，与本次改动无关）。

**`--features llvm`：314 passed / 1 failed**（`ffi_type_safe_tests::test_ctype_ptr_pack_unpack`，位于
用户未提交的 `vm/ffi`/`dynamic_ffi` 改动范围内；本阶段不变量明确不动 FFI）。

**`--features jit`：`jit_opt::tests::test_fold_constants`**（jit_opt.rs 与本次改动零耦合，HEAD 亦失败）。

修复情况：
- `p7_memory_tests` / `vm_tests` / `p4_aot_std_tests` 等既有测试文件的字段缺失已**全部补齐**（MirFunction.closures、
  BytecodeFunction.aot_mode/aot_desc_idx、BytecodeModule `..Default::default()`、load_lib 双参、p4 加 llvm 门控）——
  vm_tests 18 项全绿，编译问题清零。

其他已知边界（Phase 1 限定，属设计内延期）：
- String/闭包/集合等 TAG 可经 ABI 传递但 `to_value` 暂回退 Null（设计 §9 Phase 2 处理）

---

## 7. 后续修复记录（Phase 4 收尾）

本轮在 LLVM 23.1.0（`D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc`）下实际跑通了
AOT 链路，修复了此前因 LLVM 未安装而未被执行的 AOT 测试所暴露的 IR 生成缺陷：

| # | 问题 | 修复 |
|---|------|------|
| 1 | 预置内置函数（`equals`/`hashCode`/`typeOf`/`aura_*`）签名缺失，返回值被当成 `i8*`，生成 `icmp ne i8* %x, 0` / `xor i1 %ptr, 1` 等非法 IR | `hir.rs` 补齐 prelude 内置函数签名（equals→Boolean、hashCode→Int、typeOf→String 等） |
| 2 | 调用点实参类型与 `declare` 不一致（`call void @println(i32 …)` vs `declare void @println(i8*)`） | `aot/emit.rs` 新增 `func_param_types` + `coerce_arg`，按声明类型插入 `inttoptr`/`ptrtoint`/`zext`/`trunc`/`sitofp` 等转换 |
| 3 | 成员访问 `load %struct.X, %var.N*`（把值当指针类型用） | 先 `bitcast` 到 `%struct.X*` 再 `load` |
| 4 | `extractvalue … i32 0` 索引写成带类型常量 | 改为裸整数字面量 `extractvalue …, 0` |
| 5 | 字段赋值 `emit_member_assign` 是空实现，且会生成 `getelementptr i8, i8* %struct值` | 实现两种形态：结构体局部变量 `load/insertvalue/store` 回写；指针对象 `bitcast + gep + store` |
| 6 | 字符串拼接返回裸 `i8*`，而函数签名声明 `{ i8*, i64 }` | 拼接后补齐长度字段构造结构体值；非字符串操作数先经 `@toString` 转换 |
| 7 | 包装函数对结构体返回值做 `ptrtoint { i8*, i64 }` | 先 `extractvalue` 取第 0 字段再 `ptrtoint` |
| 8 | 虚调用 `CallVirtual` 直接以裸方法名 `@area` 调用 | 按接收者类型解析为 `Class.method` 后再发静态调用 |
| 9 | `Float` 字面量恒为 `double`，与 `float` 返回类型/字段类型不符；比较/运算两侧类型不统一 | 新增 `emit_numeric_convert`；return 语句按函数签名转换；比较运算统一类型并修正 `fcmp` 谓词（原恒为 `one`） |

验证：`cargo test --features llvm,jit --test phase4_integration_tests` 19/19 通过（含 AOT 嵌入真机执行）；
真实 Aura 程序（类 + 虚方法 + 字段赋值 + 浮点运算）经 `aura build --aot` 生成原生 exe 并运行退出码 0。

---

*报告生成于 Phase 1 开发完成后；随实现演进可增量更新。*
