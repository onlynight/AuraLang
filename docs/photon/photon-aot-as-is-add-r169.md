# R169：`as`/`is` 被编成 `strcat` + Any 操作数被 `refToStr` —— 两个语言级产码缺陷

日期：2026-10-22　分支：dev　前置：R168（`as` 内联最小复现）

## 一、结论

R168 的「cast 内联打 `<ref#-1>`」不是一个缺陷，是**两个**叠加：

| # | 缺陷 | 修复 | 验证 |
|---|---|---|---|
| A | `mapBinaryOp` 没有 `As`/`Is` 分支 ⇒ 落到默认 **`"Add"`**，`x as T` 被编成 `strcat(x, "T")` / 指针加法 | `buildBinary` 产出真 `Cast`（位等价 mov，类型=目标类型）；`is` → `aura_isOfType(v, "T")` | `cast_inline_probe` 第 7/8 行 `[hello]`/`[lit]` ✅ |
| B | 字符串转换分类器把类型名为 **`Any`** 的操作数落进「非标量 ⇒ 对象类型」臂 → `refToStr` ⇒ 打 `<ref#-1>` | `tn == "Any"` → `toString`（runtime 恒等桩，字符串透传） | `retprobe` [a]~[e] 全绿 ✅ |

两者都是**语言级**缺陷（与 VM 无关）：任何 Aura 程序里的 `as` 都在拼字符串。

## 二、定位过程（压缩版）

### A：`as` → strcat

R168 的判据用例修好后仍打 `<ref#-1>`；查 `.hat` 发现 `[7]` 比可行路径多了一层
`@toString(@Cast(..))`，而 `@Cast` 的类型被记成 `Lit` / `Any`。继续查：

1. `Parser.aura:1791`：`x as T` 产出 **`Binary`（op="As"）**，kids[1] 是承载类型名的 Leaf；
2. `SsaBuilder.mapBinaryOp` 只有算术/比较/位运算，**默认返回 "Add"**（注释里自己写过
   `&&`/`||` 漏掉的后果就是 strcat 崩溃——`As`/`Is` 是同一类遗漏）；
3. 编译期诊断 `[AS]` 显示 kids[1] 经 HIR 降级后 `kidText=[Lit: String]`（`<kind>: <类型名>`
   拼接），直接 `fromTypeName` 会造出叫 "Lit: String" 的类型 ⇒ 取最后一个 `": "` 之后的部分。

修法（`SsaBuilder.buildBinary`）：

```aura
if (op == "As") {                       // 位等价 mov，类型=目标类型
    ... typeName = kidText 尾段 ...
    return addValue("Cast", tgtTy, toStr(left), "", curBlock)
}
if (op == "Is") {                       // runtime 桩（AOT 无类型头，恒 1）
    return addValue("Call", boolTy, left + "," + nameVal, "aura_isOfType", curBlock)
}
```

### B：Any → refToStr

修完 A 后 `retprobe` 仍是 `[a] retStr=<ref#-1>`，但 `[e] t=hello`（先 as 成 String 再打印）是对的
⇒ 值是好的，坏在**打印分类**。`InstructionSelection` 的字符串转换分类器：

```aura
} else if (tn != "Int" && tn != "Long" && ... && tn != "Basic" && tn != "" && ...) {
    fname = "refToStr"        // 非标量 ⇒ 对象类型
}
```

`"Any"` 不在标量名单里 ⇒ 落进对象臂。而 Plan A 值模型是「`aura_typeof` 对任意非空值恒
折叠为 String」⇒ Any 非空即字符串，应透传。runtime 的 `toString` 恰是恒等桩，所以：

```aura
} else if (tn == "Any") {
    fname = "toString"        // 恒等透传
}
```

## 三、验证

| 用例 | 结果 |
|---|---|
| `tests/photon/debug/cast_inline_probe.aura`（R168 判据） | 8/8 行全对（原 6/8） |
| `tests/photon/debug/retprobe.aura` | [a]~[e] 全绿：`Any` 返回值打印、`Long as Any` 经裸内存往返、`as String` 后打印 |
| `tests/photon/debug/anyprobe.aura`（单参形态） | [1]~[5] 全绿：`Any → as Long → write64 → read64 → as Any` 完整往返 |
| 语言测试三模式（26 例） | VM 26/26、JIT 7/26、AOT 25/26 —— **零回归** |

## 四、遗留（下一轮）

宿主 `.auc` 仍打 `0`：`[K7] c7raw=<指针>` → push（`[K7b] depth=1 peek=<指针>`）→ pop 得 0。
反汇编证明 push/pop 的读写序列都正确 ⇒ 剩余丢失点在大程序的**多参用户调用**：

- `anyprobe` 的双参形态 `roundTrip(v: Any, mc: Long)` 打出内存垃圾；改单参（mc 函数内自分配）
  立即全绿；
- 这与 docs/pure_aura_jit/09 记载的 P5-①「字面量多参调用正确而方法内多参错位」同族。

⇒ 下一轮靶点：**用户定义多参调用的实参传递**。VM 宿主里 `Vm.dispatch` / `callOrBuiltin` /
`collectArgs` 恰好全是 2~5 参调用，修好它 `.auc` 侧即通。

## 五、本轮改动

| 文件 | 改动 |
|---|---|
| `aura/compiler/aura/lang/compiler/mir/SsaBuilder.aura` | `buildBinary` 增加 `As`/`Is` 分支（不再落 `Add`） |
| `aura/photon/aura/lang/compiler/photon/InstructionSelection.aura` | 字符串转换分类器：`Any` → `toString`（不再 `refToStr`） |
| `tests/photon/debug/{cast_inline_probe,retprobe,anyprobe}.aura` | 三个判据用例（已入库） |
