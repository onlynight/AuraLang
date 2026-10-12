# Photon VM 剩余失败与超时全量取证分析

> 状态：R37 分析轮（2026-10-12，零修改）
> 基线：v4 诚实套件 r34 —— **69 PASS / 65 FAIL / 4 TIMEOUT**（真实输出匹配 42）
> 取证方法：对全部 65 个失败逐测试采集退出码、末位原生调用（[62]/[A78]）、
> VM-ERROR 类型、首缺失行；TIMEOUT 采 45s 短窗自旋判定。
> 关联：`docs/pure_aura_jit/09-P1-2实施报告.md` 补二十七至六十一（完整证据链）。

---

## 一、总览

| 指标 | 本会话起点（R24 前） | 当前（R37） |
|---|---|---|
| 套件 PASS | 50 | **69**（突破 50% 大关） |
| TIMEOUT | 12 | **4** |
| 真实输出匹配 | 22 | **42** |
| 崩溃家族（段错误/OOM） | 遍地 | **清零** |

宿主侧三类结构性修复已系统性收尽：缺臂前置路由（13 次验证）、
HashMap→线性扁平化、核心 String 方法=value-class 调用窗掷骰族识别、
bare toStr→toStrAny 判决、TestRunner 重写（单轮 +16）、并发 24 臂、
容器分派（ptrKeyOf）、种子返回语义对齐。

---

## 二、4 个 TIMEOUT

| 测试 | 机制 | 归属 |
|---|---|---|
| language-test/01-lexer | 漂移条件循环：while 条件值骰坏，循环变量不推进（R30 ip 守卫零触发证明非野跳转） | 解释器执行骰 |
| language-test/02-types-variables | 同上（"0null" 观测同源：循环变量 STORE_LOCAL 骰） | 解释器执行骰 |
| concurrent/integration | Condvar.wait 需真阻塞——单线程 stub 即返使 barrier 语义失效重入自旋 | 并发语义缺口 |
| language-test/16-script-mode | 种子合成 main 顶层变量局部/全局混淆 → 74 万行跑飞打印（源文件头注释自述） | **种子侧 hir.rs 另案** |

---

## 三、65 个 FAIL：五档分类

### 档1｜并发缺原生臂（约 8-10 测试）——前置路由模式可修

`Thread.spawn/sleep/id/join`、`Promise.create/resolve/await` 缺臂
（实证：thread_basics 尾部 [62] = Thread.spawn/join；promise_test =
Promise.create/resolve/await）。
单线程语义可实现：**spawn = 立即同步执行闭包体、join = noop、
Promise.resolve = 句柄存储、await = 读回**。
涉及：concurrent/thread_basics、promise_test、future_chain、
memory_test、08-concurrency 等。

### 档2｜缺失原生（约 4-5 测试）——同模式可修

* 09-ffi：`ptrToInt` / `intToPtr` / Memory 全名变体（miss=§9.1
  ptrToInt(intToPtr(42))=42）；
* pure_aura/native_c2_asm：`Cpu.rdtsc`（miss=rdtsc1=0）；
* pure_aura/native_c2_memory：`Memory.alloc` 全名（miss=alloc=0）；
* self_bootstrap 4 测试：Memory/JitExec 分配打印行
  （miss=基本分配: 10 20 30 等）。

### 档3｜Float 域（约 4-5 测试）——整数域定标数学专项

phase_b math 三兄弟（sin/cos/pow/abs 输出通道）、
toString_implicit（Float 隐式 toString：miss=Float: 3.14）、
11-imports 剩余。
**终局约束（R36 判决）**：宿主无可靠 Float→Int 截断与 Float→text
路径（`as Int` 位点骰死，floatToTextF 实测 17→"0.0"+段错误）。
唯一出路 = 整数域定标数学：isqrt 牛顿迭代、Taylor 级数整数分数版，
输出走 floatTextOf(m, d)（R34 已证通道）。

### 档4｜.auc std 闭包骰（约 15-20 测试）——P5-① 或闭包重写

* HashMap 5 连：hashKey 闭包内 aura_cast→charCodeAt 自旋
  （T02 卡点实证：[62] fname=[aura_cast] fn=[HashMap.hashKey]）；
* ArrayList 7：构造器/方法闭包骰（simple_list 段错误在字段读写
  闭包；simple_test 尾部跳转骰 Unknown opcode -1）；
* string_builder 2：TestRunner 断言引擎骰（got==expected 仍 FAIL）；
* pure_aura hir_b2/b5：lambda/sigs 闭包。

### 档5｜解释器执行骰·散布型（约 20-25 测试）——P5-① 正身

1. **早期分支整段跳过**：03-functions（isPositive(5): true）、
   04-control-flow（§4.1）、lexer_control_flow（"positive" 行）、
   05-collections、15-advanced（filter 偶数个数）——if 条件值骰 →
   整段不打印；
2. **对象方法链早退**：05-classes（Point.manhattan: 7）、
   07-error-handling（Success(42).isOk: true）、07-test-classes；
3. **concat 前缀漂移**：array_1d（floats[0] = 3.14 → "3.14"，
   前缀 12 字符丢失）、string_methods（substring: hel）；
4. **对象字段写入骰**：10-memory（§10.2 ARC: a.id=0）、
   self_bootstrap gc_test——字段表已扁平化（R33），剩余 = 写入
   闭包内部；
5. **字符串插值 lowering**：14-string-interp（§14.2 表达式插值）；
6. **函数重载分派**：17-function-overload（add(1,2,3) = 6）。

### 幻影/噪声（1-2 个）

* helloworld：距离 0（全部参考行都在宿主输出中）却套件内 FAIL；
  手工复刻 + 前 6 测试同序迷你循环 + .auc 逐字节比对全 PASS——
  需完整 134 测试上下文才复现，进程/环境残留存疑，低优先悬案。

---

## 四、可修性总账

| 档 | 测试数 | 路径 | 成本 |
|---|---|---|---|
| 档1 并发缺臂 | ~8-10 | 前置路由模式（已验证 13 次） | **小时级，最后一轮红利** |
| 档2 缺失原生 | ~4-5 | 同上 | 小时级 |
| 档3 Float 域 | ~4-5 | 整数域定标数学 | 天级 |
| 档4+档5 | ~35-45 | **P5-① 解释器/闭包位点根修** | 数周级 |
| TIMEOUT | 4 | 真阻塞语义 + 种子合成 main | 混合 |

---

## 五、下一阶段路线

1. **R38（小时级）**：档1+档2——Thread/Promise/Future 单线程语义
   前置路由 + rdtsc/ptrToInt/intToPtr 臂。预期 +12-15。
2. **R39（天级）**：档3——整数域定标数学（isqrt/整数 Taylor），
   phase_b + toString_implicit。预期 +4-5。
3. **R40+（数周级）**：P5-① 解释器/发射器位点根修——档4+档5 的
   正解。施工图 = 补二十七至六十一的完整字节级证据链 +
   「五探针二分 + 与已证同构模式对齐」方法学。
   并行可做：种子侧 hir.rs 合成 main 对齐（Rust 侧零骰）、
   TestRunner 闭包重写（档4 部分）。
