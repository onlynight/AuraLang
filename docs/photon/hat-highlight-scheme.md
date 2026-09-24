# HAT 语法高亮规则方案 v3.0（sigil + 令牌架构）

> **状态**: 已实施
> **日期**: 2026-09-24
> **适用**: `tools/ide-extension/hat-vscode-extension`（VS Code）
> **前版**: v2.0「整行大正则」方案（本文档第 1 节记录了它失效的原因）

---

## 1. v2.0 方案的失效原因（实测）

用 `vscode-textmate` + `vscode-oniguruma` 对 v2.0 的 `hat.tmLanguage.json` 实际分词，得到两个叠加的缺陷：

### 1.1 13 条规则的正则被破坏

v2.0 文件里 `\(` `\)` 被全局替换成了 `[(` `)]`，例如：

```json
"match": "(?:;\\s*)?(@fn)\\s+(\\w+)\\s*[((.*?)[)]]\\s*->\\s*(\\w+)"
```

`[((.*?)[)]]` 不是分组，而是**字符类** `[((.*?)[)]` 加一个**字面量 `]`**，要求行内真的存在 `]` 才可能命中，因此永不匹配。

受影响的 13 条：`@extern`、`@fn`、`@phi`、`@icmp`、`@fcmp`、`@add/sub/mul/div/rem/neg`、`@and/or/xor/not`、`@band/bor/bxor/shl/shr/ushr`、类型转换、`@str_*`、`@list_*/@map_*`、`@call`（两种形态）。

### 1.2 末尾多了一条 `fallback-comment`

```json
"fallback-comment": { "match": "^;\\s.+.*$", "name": "comment.line" }
```

当时 HAT 序列化器给**每一行**都加 `;` 前缀（该约定已于 v3.0 取消，见 §5）。于是所有未被具体规则命中的行（正好就是 1.1 里那 13 类）被整行染成灰色注释 —— 这就是「指令没高亮、注释反而高亮」的直接原因。

### 1.3 v2.0 本身的其它缺陷

| 缺陷 | 影响 |
|---|---|
| `@fn`/`@extern`/`@call`/`@alloc` 关键字在设计上就不高亮 | 关键字无色、函数名无高亮 |
| `@alloc { } : !stackslot` 用 `:\s*(\w+)` 匹配类型 | `!` 前缀类型不匹配 → 整行退化为注释 |
| 裸 `@call @fn(...)`（无返回值）无规则 | 文档 §5.1 的写法不匹配 |
| `\s*=\s*@(add\|sub)` 把 `@` 放在捕获组外 | 同一个 `@` 有时高亮有时不高亮 |
| `(?:;\s*)?` 复制到 30+ 条规则 | 冗余、且不解决行形态变化 |

---

## 2. v3.0 架构

### 2.1 设计原则

| 原则 | 说明 |
|---|---|
| **P1 无整行兜底** | 不存在任何 comment 规则。HAT 没有行注释；`;` 只用于元数据头三行与行内 `;@span` |
| **P2 令牌优先** | 指令/值/类型/字面量各由独立令牌规则匹配，与行的整体形状解耦 |
| **P3 行级规则只做命名** | 行级规则只负责「这行是什么声明 / 标签是什么」，绝不吃下整行 |
| **P4 指令表显式枚举** | 操作码来自设计文档附录 B（封闭集合）；`@` 必须在捕获组内 |
| **P5 类型支持 `!` 前缀** | `!stackslot` / `!list` / `!map` / `!ptr` 一等公民 |
| **P6 前缀不重复** | 元数据头行的 `;` 由**一条** sigil 规则处理 |

### 2.2 匹配层次

```
第 0 层  sigil          ^;[ \t]*              → punctuation.definition.meta.hat（暗灰；只命中元数据头行）
第 1 层  行级（4 条）    module-header / meta-key-value / decl-line / bb-label
第 2 层  令牌（按序）    span → br_if 目标 → br 目标 → call 目标 → 指令表（16 组）
                        → 字面量 → 类型 → 属性键 → 字段 → SSA 值（兜底）→ 标点
```

规范产出（见 `build/sample.hat`）只有元数据头三行带 `;`，其余行（`@fn` / `  bb ...:` / `    @t0 = ...`）都是裸行；行级规则用 `(?:;[ \t]*)?` 前缀，因此历史遗留的 `;@fn` / `;  bb` 写法同样能高亮。

`patterns` 数组顺序（行级规则在前，sigil 在最后，避免抢占行首）：

```json
["#module-header", "#meta-key-value", "#decl-line", "#bb-label", "#tokens", "#meta-sigil"]
```

关键点：即使一行完全没有命中行级规则，`@t1` / `@call` / `@printf` / `@t0` / `Unit` / `=` / `(` `)` 也会各自被令牌规则命中，**永远不会整行退化**。

### 2.3 scope 表

| Scope | 语义 | 主题色（Catppuccin Mocha） |
|---|---|---|
| `punctuation.definition.meta.hat` | 元数据头行首的 `;`（`; module` / `; schema=` / `; source=`） | `#585B70` |
| `keyword.declaration.hat` | `@fn` `@extern` `@struct` `@enum` | `#89B4FA` 粗 |
| `keyword.other.target.hat` | target 三元组 | `#89DCEB` 粗 |
| `keyword.other.meta.hat` | `schema=` / `source=` | `#89DCEB` 粗 |
| `keyword.other.bb.hat` | `bb` | `#89DCEB` 粗 |
| `keyword.other.instruction.call.hat` | `@call` | `#89B4FA` 粗 |
| `keyword.other.instruction.phi.hat` | `@phi` | `#89B4FA` 粗 |
| `keyword.other.instruction.terminator.hat` | `@br` `@br_if` `@ret` | `#89B4FA` 粗 |
| `keyword.other.instruction.constant.hat` | `@i32_const` `@f64_const` `@const_str` `@bool_const` `@null` | `#FAB387` 粗 |
| `keyword.other.instruction.conversion.hat` | `@i32_to_f64` 等 | `#89DCEB` 粗 |
| `keyword.other.instruction.comparison.hat` | `@icmp` `@fcmp` + `@i32_slt` 等子操作 | `#A6E3A1` 粗 |
| `keyword.other.instruction.arithmetic.hat` | `@add` 等 | `#CBA6F7` 粗 |
| `keyword.other.instruction.logical.hat` | `@and` 等 | `#CBA6F7` 粗 |
| `keyword.other.instruction.bitwise.hat` | `@band` 等 | `#CBA6F7` 粗 |
| `keyword.other.instruction.string.hat` | `@str_*` | `#FAB387` 粗 |
| `keyword.other.instruction.memory.hat` | `@alloc` `@store` `@load` | `#89B4FA` 粗 |
| `keyword.other.instruction.collection.hat` | `@alloc_list` `@list_*` `@map_*` | `#89B4FA` 粗 |
| `keyword.other.instruction.object.hat` | `@new` `@field` `@field_set` | `#CBA6F7` 粗 |
| `keyword.other.arrow.hat` | `->` `=>` | `#FAB387` |
| `keyword.other.assignment.hat` | `=` | `#9399B2` |
| `entity.name.function.hat` | 声明函数名 | `#F9E2AF` 粗 |
| `entity.name.function.call.hat` | `@call` 的被调名 | `#F9E2AF` 粗 |
| `entity.name.type.hat` | `@struct @Point` | `#F9E2AF` 粗 |
| `variable.other.enummember.hat` | `RED` | `#F9E2AF` |
| `entity.name.type.module.hat` | 模块名 | `#89B4FA` |
| `string.unquoted.hat` | `HAT/2.0`、源路径 | `#F9E2AF` |
| `comment.block.span.hat` | `;@span L2:26-35` | `#6C7086` 斜 |
| `entity.name.type.label.hat` | BB 标签、跳转目标 | `#FAB387` 粗 |
| `variable.parameter.hat` | 签名参数 | `#A6E3A1` |
| `variable.ssaval.hat` | `@t0` `@sum@1` | `#89B4FA` |
| `variable.ssaval.literal.hat` | `@"text"` | `#A6E3A1` |
| `variable.field.hat` | `.name` | `#CBA6F7` |
| `variable.attribute.hat` | `size` `align` `count` | `#9399B2` 斜 |
| `storage.type.basic.hat` | `Int` `Float` `Bool` `Unit` `String` `Any` `Char` `Void` | `#89DCEB` 斜 |
| `storage.type.special.hat` | `!stackslot` `!list` `!map` `!ptr` `!fn` | `#89DCEB` 斜 |
| `storage.type.user.hat` | `@Point`（用类型位置） | `#F9E2AF` 粗 |
| `constant.numeric.integer.hat` / `.float.hat` | 数字 | `#FAB387` |
| `constant.language.hat` | `true` / `false` | `#F38BA8` 粗 |
| `string.quoted.double.hat` | 字符串 | `#A6E3A1` |
| `punctuation.section.hat` / `punctuation.separator.hat` | 括号 / `,` `:` | `#9399B2` |

所有 scope 采用「标准名 + `.hat` 后缀」：VS Code 按前缀匹配主题，因此默认主题下仍能落到 `keyword.*` / `entity.name.*` 上，自定义主题可精确覆盖。

### 2.4 规则要点

- **声明行**（`#decl-line`）用 `begin/end: (?=$)` 而非整行正则：`begin` 只吃 `@fn|@extern|@struct|@enum`，行内再分别匹配「函数名（后随 `(`）」「`@Type`」「参数（后随 `:`）」「`->`」，最后 include `#tokens`。
- **跳转目标**必须是标签而不是 SSA 值：`#br-target`（`@br @label`）与 `#br-if-target`（`@br_if @cond => @then, @else`）必须排在 `#op-terminator` **之前**。
- **`@call` 的两种形态**：`#call-target`（`@call @fn(...)`，被调名 → `entity.name.function.call.hat`）在前，`#op-call`（裸 `@call`）在后。
- **指令表顺序约束**：`#op-const` → `#op-convert` → `#op-cmp-sub` → `#op-cmp`，否则 `@i32_const` 会被 `@i32_*` 子操作规则抢走。
- **元数据头的 `;`** 由 `#meta-sigil`（`^;[ \t]*`）处理；行级规则用 `^[ \t]*(?:;[ \t]*)?` 同时兼容裸行与历史 `;` 前缀写法。

### 2.5 主题回退与内置配色（重要）

grammar 本身只负责**分词**，最终颜色由主题决定；主题按 scope 前缀匹配，取「最长匹配」，无匹配则用 `editor.foreground`。`.hat` 后缀的细分 scope 在**默认主题**下会落到很粗的规则上，其中几个的色值恰好接近正文色，于是「分词正确但看起来没高亮」：

| scope | 默认主题（2026 Dark / Dark Modern）实际颜色 | 观感 |
|---|---|---|
| `keyword.operator.*`（全部指令关键字） | `#d4d4d4` | 与正文 `#BBBEBF` 接近 → 像没高亮 |
| `variable.other.ssaval.hat`（`@t0` `@sum@2`） | `#c9d1d9` | 接近正文 → 像没高亮 |
| `entity.name.label.hat`（`entry` `exit`） | `#C8C8C8` | 接近正文 → 像没高亮 |
| `punctuation.*` | 主题未定义 | 直接用正文色 |
| `keyword.declaration.hat`（`@fn`） | `#569cd6` | 正常 |
| `entity.name.function.call.hat`（`@println`） | `#DCDCAA` | 正常 |
| `storage.type.basic.hat`（`Int`） | `#569cd6` | 正常 |

**修法：把 scope 落在主题必然着色的前缀链上**（v3.0.1 起）。曾尝试用 `configurationDefaults` 做语言级 `editor.tokenColorCustomizations` 兜底，**实测无效**——VS Code 主题服务读的是 `configurationService.getValue("editor.tokenColorCustomizations")`（不带 resource / 语言 override），`"[hat]"` 这类语言级覆盖根本不会被主题读到（该设置注册时甚至没有 `scope`）。因此只能从分词侧解决：

| 语义 | v3.0.0（坏） | v3.0.1（改后） | 默认主题解析 |
|---|---|---|---|
| 指令操作码（全部类别） | `keyword.operator.<cat>.hat` | `keyword.other.instruction.<cat>.hat` | 经 `keyword` → 蓝 |
| `->` / `=>` | `keyword.operator.arrow.hat` | `keyword.other.arrow.hat` | 蓝 |
| `=` | `keyword.operator.assignment.hat` | `keyword.other.assignment.hat` | 蓝 |
| SSA 值 / 字符串字面量 | `variable.other.ssaval.*.hat` | `variable.ssaval.*.hat` | 经 `variable` → 浅蓝 |
| 字段 / 属性键 | `variable.other.field/attribute.hat` | `variable.field/attribute.hat` | 浅蓝 |
| BB 标签、跳转目标 | `entity.name.label.hat` | `entity.name.type.label.hat` | 经 `entity.name.type` → 青 |
| 模块名 | `entity.name.module.hat` | `entity.name.type.module.hat` | 青 |
| 枚举变体 | `entity.name.enum.variant.hat` | `variable.other.enummember.hat` | 绿 |
| 元数据值（schema/source） | `entity.other.meta-value.hat` | `string.unquoted.hat` | 橙 |
| `;@span` 标注 | `entity.other.span.hat` | `comment.block.span.hat` | 绿（它就是注释） |

`.hat` 细分保留在最后一段，自带主题仍可精确控制（`colors/hat-colors.json` 已同步改名）。

**实测**（`build/grammar-check/checkall.js`，跨 4 套 VS Code 内置主题 + CodeBuddy CN 的 `IDE Night`/`IDE Dark`）：42 个 scope 中 39 个在全部主题下都有可见颜色；剩余 3 个是 `punctuation.*`——各内置主题本来就不给标点配色（括号用正文色，除非用户开了括号着色），属正常。

**两个 IDE 的安装位置不同**（都需装同一份 vsix，且不要同时留旧版本，否则旧 grammar 会抢 `.hat`）：

| IDE | 扩展目录 | 默认暗色主题 |
|---|---|---|
| VS Code | `%USERPROFILE%\.vscode\extensions` | 2026 Dark / Dark Modern |
| CodeBuddy CN | `%USERPROFILE%\.codebuddycn\extensions` | 内置 `theme-genie` 的 IDE Night（`night_genie_modern.json`） |

安装后需**重载窗口 / 重启 IDE**；旧扩展即使卸载后目录仍在磁盘上，也要等重启才会被清掉。

---

## 3. 验收

验证脚本：`build/grammar-check/verify.js`（依赖 `build/grammar-check/node_modules` 里的 `vscode-textmate` + `vscode-oniguruma`）

```
node build/grammar-check/verify.js
```

覆盖 36 行典型 HAT（规范裸行形态 + 历史 `;` 前缀形态各若干），断言每个片段的期望 scope，并强制检查**任何行都不得出现 `comment.*` scope**；随后整文件分词 `build/sample.hat`（由 `HatSerializer` 实际产出），要求同样无 comment scope、且指令行的每个 `@xxx` 都被分类。当前结果：`PASS 36 lines + sample.hat (18 lines), all assertions ok`。

`build/sample.hat` 由 `build/hat_scratch.aura` 生成（解析一段样例 HAT → 序列化落盘），可用于人工核对高亮效果。

---

## 4. 文件

| 文件 | 说明 |
|---|---|
| `syntaxes/hat.tmLanguage.json` | v3.0 重写 |
| `package.json` | 补 `publisher: aura-lang`/`license`/`repository`；版本 3.0.1（曾试过 `configurationDefaults` 兜底，实测无效已移除，见 §2.5） |
| `colors/hat-colors.json` | `tokenColors` 按 §2.3 重写、去重（可选的整体皮肤） |
| `build/grammar-check/{tokenize.js,verify.js}` | 分词/验收脚本（构建产物目录） |
| `build/grammar-check/check-st4-sync.py` | ST4 语法同步校验（结构 + scope/opcode 集合 + 分词断言） |
| `build/grammar-check/dsh-highlight-smoke.mjs` | dsh **编译产物**运行时冒烟（真实 Shiki 分词 → 反解命中的 `ColorKey`） |
| `build/grammar-check/{scanfile.js,probe.js,themeprobe.js,checkall.js}` | 诊断脚本：跨行状态泄漏 / 多 grammar 对比 / 主题配色推算（构建产物目录） |
| `build/hat_scratch.aura` | 生成 `build/sample.hat` 的样例脚本（解析 → 序列化落盘） |
| `build/sample.hat` | `HatSerializer` 实际产出的样例 HAT（验收用） |

---

## 5. 关联的格式规范化（v3.0 同期完成）

原始序列化器给**每一行**都加 `;` 前缀，而设计文档 §3（格式规范）与 §5（示例）定义的是**裸行**形态——只有 §8.3/§9.4 的序列化器伪代码写成 `";@fn "` / `";  bb "` / `";    "`。这导致测试夹具与实际输出都带着这层冗余前缀。现按「只保留元数据头三行 + `;@span`」统一：

| 位置 | 改动 |
|---|---|
| `aura/.../hir/hat/HatSerializer.aura` | 去掉每条输出的 `;` 前缀；块间用空行而非裸 `;` 行 |
| `tests/hat_{parser,serializer,verification,pipeline}_tests.aura` | 夹具改为裸行（约 164 行） |
| `docs/photon/hat-format-design.md` | §3.1 增加 `;` 使用规则；§4.1/§7.x/§8.2/§8.3/§9.4 与示例统一为裸行 |

同期修复的两处实现缺陷：

1. **`HatSerializer` 载荷约定**：`MirValue.aux` 存的是载荷（Const 字面量 / Call 被调名 / ICmp 子操作），不是值名。已修正赋值左侧命名（`@t<id>`）、常量操作码（按类型分派 `@const_str`/`@f64_const`/`@bool_const`/`@null`）、比较子操作、Phi 命名，以及常量改为**以 `@t<id>` 引用 + 按需补发定义行**（不再内联 `@0`/`@1` 这种与临时编号歧义的写法）。回环（`serialize → parse → serialize`）现已稳定。
2. **`HatParser` 三处存量缺陷**：
   - `parseSingleParam` 取值顺序颠倒 → 参数类型恒为空（输出里出现 `@n: Basic`）；
   - 不支持前向引用（`@phi` 里引用后定义的 SSA 值、`@br_if` 引用后定义的块标签）；
   - `MirBlock` 无 label 字段，块标签丢失。

   修法：修正取值顺序；新增 `predeclareBlocks` 预登记全部块标签；操作数解析产出 `?name` 占位并由 `resolvePendingValues` 在函数解析完成后回填；`MirBlock` 增加 `label` 字段并在序列化时优先使用。

## 6. 跨插件同步（v3.0.1）

除 VS Code 扩展外，仓库里还有两份 HAT 高亮实现，**scope 名必须与 §2.3 完全一致**，否则同一个主题只能覆盖其中一个编辑器：

| 插件 | 文法文件 | 同步方式 |
|---|---|---|
| dsh 插件 | `tools/dsh-plugins/hat-dsh-highlight/grammar/hat.tmLanguage.json` | **脚本化**：`node scripts/sync-grammar.mjs` 从 VS Code 扩展复制并规范化 `name`/`displayName`，写 `grammar/SYNC-SHA256` 指纹；`node scripts/bundle-grammar.mjs` 重新生成 `src/hat-grammar.ts` |
| ST4 插件 | `tools/ide-extension/hat-st4/Hat.sublime-syntax` | **手工移植**：同一套 sigil + 令牌架构，逐 scope 对应（Sublime 用 `contexts` + `push`/`pop` 表达 `decl-line` 的 begin/end） |

dsh 侧另有 `scripts/diff-grammar.mjs` 做三重漂移检查——① 上游指纹是否变化、② 本地拷贝是否仍是规范化后的上游、③ **`src/theme.ts` 是否覆盖了文法里的每个 scope**（改 scope 名却忘了改主题，会让该 token 静默掉回正文色，这正是本轮踩过的坑）。`package.json` 的 `check:grammar` 与 `prepublishOnly` 早已引用它，但该文件此前**并不存在**（检查一直是坏的），本次补齐。

ST4 侧无法运行 Sublime 的分词器，改用 `build/grammar-check/check-st4-sync.py` 校验：YAML 结构 / `include` 可解析 / 正则可编译 / **scope 集合与 opcode 集合必须与 TextMate 版完全相等** / 再用一个迷你 leftmost-first 扫描器在 `Hat.sublime-syntax` 上重跑 §2.7 的验收表（`;` 前缀与裸行两种形态，142 条断言）。

当前状态（用 `name` 改为 `HAT v3.0` 后的指纹）：

```
[sync-grammar]   sha256  a605dfff2edfe18c130c8279e0c09ac82d4bc4d7856df761c5e26a1f549aed51
[diff-grammar] OK: upstream, local copy and theme coverage all in sync
[check-st4-sync] OK: 32 contexts, 42 scopes, 60 opcodes, 142 token assertions
[verify.js]      PASS 36 lines + sample.hat (18 lines)
```

### 6.1 编译与部署

| 目标 | 命令 | 产物 / 落点 |
|---|---|---|
| VS Code 扩展 | `vsce package` → `code --install-extension <vsix> --force` | `hat-language-support-<ver>.vsix`；`~/.vscode/extensions/aura-lang.hat-language-support-<ver>/` |
| CodeBuddy CN | `"<CodeBuddy CN>/bin/buddycn.cmd" --install-extension <vsix> --force` | `~/.codebuddycn/extensions/...`（同一 vsix，两侧 grammar SHA 必须相等） |
| dsh 插件 | `pnpm install` → `pnpm run build` | `tools/dsh-plugins/hat-dsh-highlight/lib/{client.js,highlight.js,index.js,types/}` |
| DSH profile | 在 `~/.dsh/profiles/web/package.json` 加 `"@aura-lang/dsh-highlight-hat": "link:D:/Code/AuraLang/tools/dsh-plugins/hat-dsh-highlight"` 与同名 `dsh.profile.bundles` 条目，再 `dsh plugin --profile web install` | profile `node_modules` 里生成 junction；`dsh --profile web --dump-config` 出现 `- id: hat-highlight-hat` |
| ST4 插件 | `tools/ide-extension/hat-st4/deploy.ps1` | `%APPDATA%\Sublime Text\Packages\HatLanguage\` |

部署后验证（`build/grammar-check/dsh-highlight-smoke.mjs`）：直接 import 插件的 `lib/highlight.js`，走 Shiki → 文法 → 主题的真实运行路径分词。主题把颜色写成 `var(--HAT-<key>, …)`，所以能从 token 颜色反解出命中的 `ColorKey`——**没命中任何规则**的 token 会拿到主题前景色，被单独列出并断言。

```
[smoke] 17 lines, 39 token assertions
[smoke] unstyled by design: : | ; | ; module | = | @span | target
[smoke] OK: every meaningful token carries a theme rule
```

「by design 未着色」的只有规则内未被 capture 的字面量：

- `module` / `target`（元数据头里只 capture 了模块名与目标三元组）
- 行级规则自身吃掉的分隔符 `;`、`=`、`:`（例：`; module …` 的 `;`、`schema=` 的 `=`、`bb label:` 的 `:`）
- `@span` 标记本身（只 capture 了 `L<line>:<col>-<end>`）

> ⚠️ 由此存在一处观感不一致：`; module …` / `;@fn …` / `;  bb …` 行上的 `;` 被行级规则吃掉且未 capture → 显示为**正文色**；其余行由 `#meta-sigil` 匹配 → `punctuation.definition.meta`（暗灰）。要统一的话需把 `#meta-sigil` 提到 `patterns` 首位并让行级规则改用非 `^` 锚点（属 v3.1 结构调整，需重跑本条全部同步链）。

## 7. 已知遗留

- **`;@span` 尚未真正可忽略**：`HatParser` 只剥行首 `;`，行内 `@t0 = @i32_const 42 : Int ;@span L2:26-35` 的类型字段会把 `;@span ...` 一起吃进去（设计文档 §3.8 声称「解析器可选处理」）。序列化器当前不产出 `;@span`，故未影响现有用例。
- **`punctuation.*` 在各内置主题下都不着色**（§2.5）：括号/分隔符用正文色，这是 VS Code/Sublime 内置主题的既有行为，不是缺陷；需要彩色可开 `editor.bracketPairColorization`。
- **ST4 的 `Snippets/*.sublime-snippet` 内容还是坏格式**：例如 `Function.sublime-snippet` 展开为 `; @fn () ->`（`;` 与 `@fn` 之间有空格、无函数名）、`Module.sublime-snippet` 写的是 `; schema=hat.v2`（应为 `; schema=HAT/2.0`）、`Call.sublime-snippet` 是 `; @ = @call @ ()`。这些内容不符合 §3.1 的 `;` 使用规则（裸行 + 仅元数据头带 `;`），插入后是非法 HAT。与高亮无关，属独立待修项。

---

*文档结束 — HAT 语法高亮方案 v3.0*
