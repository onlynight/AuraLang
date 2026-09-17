# DSH 插件：Aura 代码高亮 — 方案设计

> 目标：让 Aura 语言（`.aura`）在 DeepSeek Harness（DSH）Web GUI 的文档预览中获得与 VSCode 插件同等质量的语法高亮。
>
> 本文档只做设计，不修改任何代码。

---

## 1. 目标与非目标

### 1.1 目标

| 编号 | 目标 |
|------|------|
| G1 | 在 DSH Web GUI 的右侧文档预览（Sidebar Files → 打开 `.aura` 文件）中获得完整语法高亮 |
| G2 | 高亮语义与 VSCode 插件一致：复用同一份 TextMate 语法（`ide-extension/vscode-extension/syntaxes/aura.tmLanguage.json`） |
| G3 | 颜色随 DSH 应用亮/暗主题自动切换，不写死颜色 |
| G4 | 以**插件**形式交付：`dsh plugin add` 安装、可卸载、可独立版本化，不侵入 DSH 源码 |
| G5 | 支持大文件的增量/流式渲染，不因文件增大而卡顿 |

### 1.2 非目标（本期不做）

| 编号 | 非目标 | 理由 |
|------|--------|------|
| N1 | DSH 聊天/Markdown 内联 ```aura 代码块的高亮 | 受 DSH 平台模块表限制，见 §3.4、§5.8；需要上游改动 |
| N2 | Aura 语义能力（跳转定义、诊断、补全） | 属 LSP 范畴，DSH 无 LSP 客户端框架 |
| N3 | 修改 VSCode 插件 | 作为语法的唯一真源（single source of truth）保持不动 |

---

## 2. 现状与根因

`.aura` 文件目前在 DSH 文档预览中**可打开、但不高亮**（纯文本渲染）。根因有三个层次，必须全部解决。

### 2.1 后缀未被代码渲染器认领

`@deepseek-ai/dsh-client-ui-sidebar-documentpreview` 内定义了一张后缀→语言表，`aura` 不在其中：

```js
// dsh-client-ui-sidebar-documentpreview/lib/client.js:26751-26824
const languages = new Map(Object.entries({
    typescript: ["ts","tsx","mts","cts"],
    javascript: ["js","jsx","mjs","cjs"],
    // ... 27 个内置语言 ...
    kotlin: ["kt","kts"],
    lua: ["lua"]
}));
const CODE_EXTENSIONS = [...languages.keys()];
```

`languageForPath("foo.aura")` 返回 `undefined`，因此 `.aura` 不会被选入"代码"渲染器。

### 2.2 回落到纯文本渲染器

文档预览注册表里存在一个通配兜底实现，`.aura` 命中它：

```js
// client.js:1347-1355  PLAIN_BODY_ID
function textBodyDefinition(title) {
    return {
        id: PLAIN_BODY_ID,
        extensions: [],          // 空 = 匹配任何路径
        priority: "builtin",
        loading: "text-pages",
        wrap: true
    };
}
```

`TextBody` 用 `<pre>` 逐行输出纯文本，无任何着色。

### 2.3 无法向内置高亮器注入新语言（决定性约束）

即使 `.aura` 被选入代码渲染器也无济于事。DSH 的 `CodeBlock` 通过两个**模块私有** `Map` 解析语言：

```js
// dsh-web-frontend/dist/assets/index-BKQ_L1z6.js
const Sm = new Map([                     // languageId -> 惰性 chunk loader
    ["python", () => import("./langs/python-B6aJPvgy.js")],
    /* ... 27 个 ... */
]);
const Ro = new Map(/* 别名 -> languageId */);

function A3(t) {                         // ensure language loaded
    const r = Sm.get(t);
    return r === void 0 || Jt().getLoadedLanguages().includes(t) ? true :
        (T5.has(t) || (T5.add(t), r().then(i => {
            Jt().loadLanguageSync(i.default);   // Shiki 实例
            t8 += 1; for (const s of k3) s();   // 通知订阅者重渲染
        }), !1));
}
function Om(t, r) {                      // 完整高亮
    const i = r === void 0 ? void 0 : Ro.get(r.toLowerCase());
    if (i !== void 0 && A3(i)) return Jt().codeToHtml(t, { lang: i, theme: "css-variables" });
}
```

`Sm` 与 `Ro` 都是应用 bundle 内的 `const`，**没有公开的注册 API**。`Sm` 未命中时 `A3` 返回 `true` 但 `Om` 的 `Ro.get` 返回 `undefined`，`CodeBlock` 退化为无着色的 `<pre>`。

> **结论：必须自带高亮引擎，不能寄生在 DSH 的 Shiki 实例上。** 这是本方案最重要的架构约束。

---

## 3. DSH 高亮与插件体系关键发现

以下发现直接决定了方案形态，每条都带证据位置。

### 3.1 技术栈：Shiki + vscode-textmate + oniguruma WASM

`dsh-web-frontend/dist/assets/vendor-CCJJTK99.js` 内含完整 Shiki 与 vscode-textmate 实现（`createOnigString`、`_tokenTypeMatchers`、`balancedBracketSelectors`、`Token` 类均可见）。

### 3.2 语言资产就是 TextMate grammar JSON —— Aura 可直接复用

DSH 的每个语言 chunk 形如：

```js
// dist/assets/langs/kotlin-BdnUsdx6.js
const e = Object.freeze(JSON.parse('{"displayName":"Kotlin","fileTypes":["kt","kts"],' +
    '"name":"kotlin","patterns":[{"include":"#import"}],"repository":{...}}'));
```

即 **Shiki 的 language registration 就是 TextMate grammar JSON 本体**。因此 VSCode 插件的 `aura.tmLanguage.json`（1114 行，`scopeName: "source.aura"`）**无需任何语法转写**，只需序列化打包。这是"参考 VSCode 插件高亮"最直接、零漂移的实现路径。

### 3.3 主题机制：`css-variables` 主题 + CSS 变量

DSH 不内联颜色，而是使用名为 `"css-variables"` 的 Shiki 主题，颜色来自 `dsh-client-ui-theme` 注入的 `<style>`：

```css
/* dsh-client-ui-theme/lib/client.js:1061-1072 */
:root{
  --shiki-foreground: var(--dsw-alias-label-primary);
  --shiki-background: var(--dsw-alias-markdown-code-block);
  --shiki-token-constant:#1c7ed6;  --shiki-token-string:#2f9e44;
  --shiki-token-comment:#868e96;   --shiki-token-keyword:#d6336c;
  --shiki-token-parameter:#e8590c; --shiki-token-function:#6741d9;
  --shiki-token-string-expression:#2b8a3e; --shiki-token-punctuation:#495057;
  --shiki-token-link:#1971c2
}
body[data-ds-dark-theme]{ --shiki-token-constant:#4dabf7; /* ...暗色覆盖... */ }
```

**含义：Shiki 的 token 颜色可以是任意 CSS 值（含 `var(...)`），因此主题可完全跟随 DSH 应用主题切换。** 这是 G3 的实现基础（§5.5）。

### 3.4 平台模块种子表只有 9 项 —— Shiki 不可 import

Web 外壳构建的冻结模块表：

```js
// dsh-web-frontend/dist/assets/index-BKQ_L1z6.js  function by()
return {
  react, "react/jsx-runtime", "react-dom", "react-dom/client",
  "@deepseek-ai/cordis",
  "@deepseek-ai/dsh-client-store",
  "@deepseek-ai/dsh-client-ui-slots",
  "@deepseek-ai/dsh-client-ui-primitives",   // ← 含 CodeBlock / FileTypeIcon
  "@deepseek-ai/dsh-client-ui-dockkit"
};
```

插件只能从这 9 项 + 自己声明的 `dsh.client.external` 取模块。**Shiki 不在其中**，所以插件必须自带 Shiki（§5.3）。

### 3.5 `DocumentPreviewRegistry` 是官方扩展点

```ts
// dsh-client-ui-sidebar-documentpreview/lib/types/client/document/registry.d.ts
export interface DocumentPreviewDefinition {
  readonly id: string;
  readonly extensions: readonly string[];
  readonly priority?: 'builtin' | 'extension';   // extension 优先于 builtin
  readonly title: () => string;
  readonly loading: 'text-pages' | 'bytes-complete';
  readonly wrap?: boolean;
}
export declare function matchingDocumentPreviews(
  definitions, path): readonly DocumentPreviewDefinition[];
// 排序：extension band 优先 → 最长后缀 → 注册顺序
```

`.aura`（4 字符后缀）在同 band 内长于兜底的空后缀，配合 `priority: "extension"` 可**双保险**稳定胜出。

### 3.6 渲染器通过 keyed slot 注册

```js
// client.js:26906  代码渲染器的注册方式（可照抄）
ctx.effect(() => ctx.slots.inject("sidebar.right.tab.document",
  () => ctx.slots.register({ name: "sidebar.right.tab.document", key: ID, locale: NS$1 }, CodeBody)));
```

文档内容由 owner 通过 `DocumentPreviewProps` 下发（`resourceAddress`、`content`、`wrap`、`scrollportRef`、`t`），渲染器只管呈现 —— 插件无需触碰文件读取逻辑。

### 3.7 文件标签页已覆盖 `.aura`

```js
// client.js:1766-1775
function textDefinition() {
    return { id: TEXTPREVIEW_ID, kind: "text",
             patterns: ["dsh-resource://file/**"], priority: "fallback",
             canOpen: (a) => parseFileAddress(a)?.scope === "session",
             title: basenameOf };
}
```

`.aura` 今天就能打开成标签页，插件只需替换其 body，无需注册新 tab 类型。

### 3.8 `CodeBlock` 不能接收预渲染 HTML

`CodeBlock` 的 props 只有 `code / lang / streaming / className / contentRef / lineNumbers / copyLabel / copiedLabel`。它不接受已高亮的 HTML 或 token 数组，且 `lang="aura"` 会因 §2.3 退化为纯文本。

> **结论：插件需自带等价的代码块 chrome（语言标签 + 复制按钮 + 行号 + 滚动容器）。** 视觉可对齐 primitives 的 `md-code-block` 类名，但组件必须自实现。

---

## 4. 方案比选

### 方案 A（推荐）：自包含客户端插件

插件内嵌 `@shikijs/core` + `@shikijs/engine-oniguruma` + Aura grammar，自实现代码块 chrome。

- **依赖**：`@shikijs/core`、`@shikijs/engine-oniguruma`、React、cordis、`dsh-client-ui-slots`
- **产物体积**：约 250–300 KB（oniguruma wasm + Shiki 引擎 + 35 KB grammar）
- **优点**：零上游依赖；完整 TextMate 语义（begin/end 跨行、字符串嵌套、模板插值）；颜色可通过 CSS 变量完全贴合 VSCode；今天就能做
- **缺点**：重复承载一份 Shiki；oniguruma wasm 重复初始化；N1 目标不可达

### 方案 B：请求 DSH 上游开放扩展点（长期最优）

向 DSH 提 PR：把 `@shikijs/core` 加入 `PLATFORM_MODULES`，并在 reflect 上暴露 `ctx.highlighter`（含 `registerLanguage(id, grammar)` 与 `getLoadedLanguages()`）。插件则只剩 grammar + 渲染器，体积几 KB，且能顺带解决 N1（Markdown 内联代码块）。

- **优点**：插件极小；无重复引擎；覆盖聊天/工具输出中的 ```aura 代码块
- **缺点**：依赖上游接受与发布周期；在本方案落地前不可用
- **策略**：**并行推进** —— A 先交付，B 同步提 issue/PR

### 方案 C：轻量正则着色器（无 Shiki）

把 `aura.tmLanguage.json` 中的 `match:` 规则移植为正则链。

- **优点**：约 15 KB，零依赖，加载即时
- **缺点**：**丢掉 begin/end 跨行状态**。Aura 语法含 12+ 个 begin/end 规则（多行字符串、块注释、class/struct/interface/actor/object 声明体、extern 块、注解参数、函数调用、字符串模板表达式），纯正则会在这些结构上产生明显错误着色
- **结论**：不推荐作为主线；仅可作为降级兜底（Shiki wasm 加载失败时）

### 决策

**主线走方案 A**，同时向 DSH 上游提交方案 B 所需的最小扩展点提案。方案 C 仅实现为 A 的运行时降级路径。

---

## 5. 推荐方案详细设计

### 5.1 包结构

```
aura-dsh-highlight/
├── package.json                        # dsh.client 声明 + dsh.bundle（必需）
├── cordis.patch.yml                    # bundle patch：insert 本插件 host 行
├── src/
│   ├── index.ts                        # host half：空 apply（行只需存在）
│   ├── client.ts                       # 浏览器入口：导出 apply(ctx) 与 inject 列表
│   ├── highlighter.ts                  # Shiki 实例懒创建 + Aura 注册 + 主题注册
│   ├── aura-grammar.ts                 # 生成物：Object.freeze(JSON.parse(...))
│   ├── aura-theme.ts                   # 生成物：Aura CSS-变量主题 JSON
│   ├── AuraCodeBody.tsx                # keyed slot 组件（代码块 chrome）
│   ├── incremental.ts                  # 流式增量高亮（grammarState 续接）
│   ├── locale.ts                       # zh / en 文案
│   └── styles.css                      # 代码块样式，对齐 md-code-block 视觉
├── grammar/
│   └── aura.tmLanguage.json            # 从 vscode-extension 同步（构建期拷贝）
├── scripts/
│   ├── sync-grammar.mjs                # 从 ide-extension 拷贝并规范化 grammar
│   ├── bundle-grammar.mjs              # grammar JSON → aura-grammar.ts
│   └── diff-grammar.mjs                # 与 vscode-extension 版本比对（CI 用）
├── test/
│   ├── fixtures/*.aura                 # 取自 examples/ 与 tests/ 的真实样本
│   └── highlight.spec.ts               # scope 快照测试
└── README.md
```

### 5.2 `package.json` 清单

```jsonc
{
  "name": "@aura-lang/dsh-highlight-aura",
  "version": "0.1.0",
  "description": "DSH Web GUI client plugin: Aura language syntax highlighting (TextMate grammar shared with the VSCode extension)",
  "license": "Apache-2.0",
  "type": "module",
  "exports": {
    ".":       { "types": "./lib/types/index.d.ts", "default": "./lib/index.js" },
    "./client":{ "types": "./lib/types/client/index.d.ts", "default": "./lib/client.js" },
    "./package.json": "./package.json"
  },
  "dsh": {
    "bundle": { "patch": "./cordis.patch.yml" },
    "client": {
      "platform": "web",
      "inject": [
        "@deepseek-ai/dsh-client-ui-sidebar-documentpreview",
        "@deepseek-ai/dsh-client-ui-slots",
        "@deepseek-ai/dsh-client-ui-primitives"
      ],
      "external": []
    }
  },
  "dependencies": {
    "@shikijs/core": "^4.0.0",
    "@shikijs/engine-oniguruma": "^4.0.0"
  },
  "peerDependencies": {
    "react": "^18.2.0",
    "@deepseek-ai/cordis": "^4.0.2"
  },
  "scripts": {
    "build":  "pnpm run sync:grammar && pnpm run bundle:grammar && tsdown",
    "sync:grammar":    "node scripts/sync-grammar.mjs",
    "bundle:grammar":  "node scripts/bundle-grammar.mjs",
    "check:grammar":   "node scripts/diff-grammar.mjs",
    "test":    "vitest run",
    "test:watch": "vitest"
  }
}
```

**要点**

- `dsh.client.platform: "web"` 使其被 `dsh-client-modules` 扫描为浏览器插件。
- `inject` 列出必须先到厂的包行：`dsh-client-ui-sidebar-documentpreview` 提供 `ctx.documentPreviews`（reflect 服务）与 `sidebar.right.tab.document` slot 定义；`ui-slots` 提供 `ctx.slots`；`ui-primitives` 提供 `FileTypeIcon` / `classifyFileType` 与共享 CSS 变量。
- `external: []` —— Shiki 作为 `dependencies` 打进自身 bundle，不依赖平台种子表（§3.4）。
- **需要 `dsh.bundle`（2026-09-15 修正）**：原文写作"不需要 `dsh.bundle`：那是 profile 配置补丁层，客户端插件只靠 `dsh.client` 生效"，这是错的。`dsh-client-modules` 的浏览器名册来自**扫描已挂载的 Loader 行**（`for (const entry of ctx.loader.entries())`，见 `dsh-client-modules/lib/index.js:475`），而 `dsh.profile.bundles` 里的每个包都必须声明 `dsh.bundle.patch`（`dsh-app-boot/lib/index.js:852` 对缺失者直接抛错）。因此纯客户端插件也必须自带一份 `cordis.patch.yml`，通过 `insert` 挂载自己的行：行存在 → 扫描到 `dsh.client` → `./client` 被打包下发。同时包根（host half）必须导出一个 `apply`（可为空实现，对齐 `dsh-client-ui-sidebar-documentpreview` 的 `function apply() {}`），否则 `unwrapExports` 得到的命名空间对象不是合法 plugin，会以 `invalid plugin` 报错。

### 5.3 Grammar 资产流水线

**单一真源原则**：`ide-extension/vscode-extension/syntaxes/aura.tmLanguage.json` 是唯一定义处，插件构建期同步，禁止手改副本。

`scripts/sync-grammar.mjs` 做三件事：

1. 从 `ide-extension/vscode-extension/syntaxes/aura.tmLanguage.json` 拷贝。
2. **规范化**（仅两处，不改语义）：
   - `"name": "Aura"` → `"name": "aura"`。DSH 各语言 chunk 的 `name` 都是小写 languageId（`"kotlin"`、`"python"`、`"c"`），Shiki 用它派生语言标识；不一致会导致 `getLoadedLanguages()` 匹配失败。
   - 增补 `"displayName": "Aura"`（对齐 DSH 现有 chunk 形态，非必需）。
3. 输出到 `grammar/aura.tmLanguage.json` 并记录源文件 SHA-256 到 `grammar/SYNC-SHA256`，供 CI 与 VSCode 插件版本对齐校验。

`scripts/bundle-grammar.mjs` 生成：

```ts
// src/aura-grammar.ts（生成物）
export const AURA_GRAMMAR = Object.freeze(JSON.parse(`{"name":"aura",...}`));
```

采用模板字符串而非对象字面量，与 DSH 自身 chunk 形态一致，并保证 JSON 语义 100% 保真（避免构建期 JSON→JS 对象的键序/转义漂移）。

**Grammar 修订建议**（记录为对 VSCode 语法本身的改进项，非插件职责）：

| 项 | 现状 | 建议 |
|----|------|------|
| `enum-variant` | `\b([A-Z]\w*)\b` 作为 `#code` 末尾兜底，会吞掉所有大写字开头标识符（类名、类型参数） | 收敛为仅枚举体内生效，或加 `(?!:)` / `(?!\s*\()` 负向断言 |
| 软/硬关键字 | `soft-keywords` 与 `hard-keywords` 均含 `to`，重复 | 合并去重 |
| 注解白名单 | 含 Kotlin JVM 注解（`JvmStatic`、`JvmName`） | 清理为 Aura 实际注解集 |
| `for` 循环变量 | 仅匹配 `for (x: T in ...)` | 补齐 `for ((k, v) in ...)`、范围迭代 |
| Aura 特有 | `comptime`、`select` 分支、actor `!` 发送、`await` 已覆盖；`try` 块、`init` 块未建模 | 补充 |

### 5.4 高亮引擎（`highlighter.ts`）

```ts
import { createHighlighter } from '@shikijs/core';
import { createOnigurumaEngine } from '@shikijs/engine-oniguruma';
import { AURA_GRAMMAR } from './aura-grammar';
import { AURA_THEME } from './aura-theme';

let inst: Promise<HighlighterGeneric> | undefined;

export function getHighlighter() {
  inst ??= createHighlighter({
    engine: createOnigurumaEngine(),
    langs: [{ id: 'aura', name: 'aura', scopeName: 'source.aura',
              patterns: AURA_GRAMMAR.patterns, repository: AURA_GRAMMAR.repository }],
    themes: [{ ...AURA_THEME }],
    defaultColor: false,          // 允许 var(...) 作为颜色值原样下发
  });
  return inst;
}

export function auraCodeToHtml(code: string, theme: 'aura-light'|'aura-dark'): string | undefined {
  return getHighlighter().then(h => h.getLoadedLanguages().includes('aura')
    ? h.codeToHtml(code, { lang: 'aura', theme, wrap: true })
    : undefined);
}
```

**惰性单例 + 订阅**：wasm 首次加载约 50–150 ms。组件层用一个 `useSyncExternalStore` 订阅"引擎就绪"信号，未就绪时先渲染纯文本骨架，就绪后重渲染 —— 与 DSH 自身 `A3()`/`k3`/`t8` 的机制同构，交互上无突兀跳变。

**错误兜底**：`createHighlighter` 失败（wasm 被 CSP/网络阻断）时降级到方案 C 的正则着色器，绝不让预览面板报错。

### 5.5 Aura 主题：CSS 变量化，贴合 VSCode 配色

VSCode 插件 `package.json` 的 `configurationDefaults."[aura]".textMateRules` 定义了权威配色。插件将其转成**双主题 JSON + CSS 变量**，既保留 VSCode 观感又跟随 DSH 主题：

```ts
// src/aura-theme.ts（生成物，节选）
export const AURA_THEME = {
  name: 'aura-light',
  type: 'light',
  colors: {
    'editor.foreground': 'var(--aura-fg)',
    'editor.background': 'transparent',
  },
  tokenColors: [
    // 关键字族
    { scope: ['keyword.control.aura','keyword.soft.aura','keyword.hard.aura','keyword.map.aura'],
      settings: { foreground: 'var(--aura-keyword)' } },
    { scope: 'keyword.operator.elvis.aura',
      settings: { foreground: 'var(--aura-operator-special)' } },
    { scope: 'keyword.operator', settings: { foreground: 'var(--aura-operator)' } },
    // 声明修饰符
    { scope: ['storage.modifier.other.aura','storage.type.package.aura','storage.type.import.aura'],
      settings: { foreground: 'var(--aura-package)' } },
    { scope: 'storage.type.extern.aura', settings: { foreground: 'var(--aura-extern)' } },
    { scope: ['storage.type.class.aura','storage.type.struct.aura','storage.type.enum.aura',
              'storage.type.interface.aura','storage.type.actor.aura','storage.type.object.aura',
              'storage.type.alias.aura','storage.type.function.aura','storage.type.function.arrow.aura'],
      settings: { foreground: 'var(--aura-keyword)', fontStyle: 'italic' } },
    { scope: ['storage.type.variable.aura','storage.type.variable.readonly.aura'],
      settings: { foreground: 'var(--aura-keyword)' } },
    // 类型与实体
    { scope: ['entity.name.type.aura','entity.name.type.superclass.aura','entity.name.package.aura'],
      settings: { foreground: 'var(--aura-type)' } },
    { scope: ['entity.name.type.class.aura','entity.name.type.struct.aura','entity.name.type.enum.aura',
              'entity.name.type.interface.aura','entity.name.type.actor.aura','entity.name.type.object.aura'],
      settings: { foreground: 'var(--aura-class)' } },
    { scope: 'entity.name.type.annotation.aura', settings: { foreground: 'var(--aura-annotation)' } },
    { scope: 'entity.name.enum.variant.aura', settings: { foreground: 'var(--aura-enum-variant)' } },
    { scope: ['entity.name.function.declaration.aura','entity.name.function.call.aura',
              'entity.name.function.reference.aura'], settings: { foreground: 'var(--aura-function)' } },
    { scope: 'support.function.std.aura', settings: { foreground: 'var(--aura-std-fn)' } },
    // 变量族
    { scope: 'variable.other.constant.aura', settings: { foreground: 'var(--aura-constant)' } },
    { scope: ['variable.other.readwrite.aura','variable.other.object.aura','variable.field.aura',
              'variable.other.property.aura'], settings: { foreground: 'var(--aura-variable)' } },
    { scope: 'variable.parameter.aura', settings: { foreground: 'var(--aura-parameter)' } },
    { scope: 'variable.language.this.aura', settings: { foreground: 'var(--aura-keyword)', fontStyle: 'italic' } },
    // 字面量
    { scope: 'constant.numeric', settings: { foreground: 'var(--aura-numeric)' } },
    { scope: ['constant.language.boolean.aura','constant.language.null.aura'],
      settings: { foreground: 'var(--aura-boolean)' } },
    { scope: 'constant.character.escape.aura', settings: { foreground: 'var(--aura-escape)' } },
    // 字符串
    { scope: ['string.quoted.double.aura','string.quoted.single.aura'],
      settings: { foreground: 'var(--aura-string)' } },
    { scope: 'variable.string-escape.aura', settings: { foreground: 'var(--aura-string-expr)' } },
    { scope: 'punctuation.definition.template-expression', settings: { foreground: 'var(--aura-string-expr)' } },
    // 注释
    { scope: ['comment.line.double-slash.aura','comment.block.aura'], settings: { foreground: 'var(--aura-comment)' } },
    { scope: 'keyword.other.documentation.javadoc.aura', settings: { foreground: 'var(--aura-doc-tag)' } },
    // 标点
    { scope: 'punctuation', settings: { foreground: 'var(--aura-punctuation)' } },
    { scope: 'entity.name.label.aura', settings: { foreground: 'var(--aura-label)' } },
  ],
};
```

配套样式注入（插件 `apply()` 内经 `<style data-plugin>` 注入，与 DSH 自身 CSS 注入机制一致）：

```css
/* 亮色 —— 取自 VSCode 插件 textMateRules */
:root{
  --aura-fg:#000000; --aura-keyword:#AF00DB; --aura-package:#569CD6;
  --aura-extern:#C586C0; --aura-type:#4EC9B0; --aura-class:#4EC9B0;
  --aura-annotation:#BC83FF; --aura-enum-variant:#4FC1FF;
  --aura-function:#795E26; --aura-std-fn:#4EC9B0;
  --aura-constant:#AAA0FA; --aura-variable:#9CDCFE; --aura-parameter:#CE9178;
  --aura-numeric:#098658; --aura-boolean:#098658; --aura-escape:#393A34;
  --aura-string:#04513E; --aura-string-expr:#098658;
  --aura-comment:#008000; --aura-doc-tag:#808000;
  --aura-operator:#000000; --aura-operator-special:#D7BA7D;
  --aura-punctuation:#000000; --aura-label:#9B2D5E;
}
body[data-ds-dark-theme]{
  --aura-fg:#D4D4D4; --aura-keyword:#C586C0; --aura-package:#569CD6;
  --aura-extern:#C586C0; --aura-type:#4EC9B0; --aura-class:#4EC9B0;
  --aura-annotation:#BC83FF; --aura-enum-variant:#4FC1FF;
  --aura-function:#DCDCAA; --aura-std-fn:#4EC9B0;
  --aura-constant:#9CDCFE; --aura-variable:#9CDCFE; --aura-parameter:#9CDCFE;
  --aura-numeric:#B5CEA8; --aura-boolean:#569CD6; --aura-escape:#D7BA7D;
  --aura-string:#CE9178; --aura-string-expr:#D7BA7D;
  --aura-comment:#6A9955; --aura-doc-tag:#6A9955;
  --aura-operator:#D4D4D4; --aura-operator-special:#D7BA7D;
  --aura-punctuation:#D4D4D4; --aura-label:#9B2D5E;
}
```

主题名随 `body[data-ds-dark-theme]` 切换：组件读取该属性决定调用 `codeToHtml(..., {theme: 'aura-light'})` 还是 `'aura-dark'`。由于颜色本身是 CSS 变量，即使主题名选错，视觉也只会退化为对应 CSS 变量集的取值，不会白屏 —— 这是把颜色放到 CSS 层而非主题 JSON 的直接收益。

### 5.6 渲染组件（`AuraCodeBody.tsx`）

复用 §3.6 的 `CodeBody` 形态：

```tsx
import type { DocumentPreviewProps } from '@deepseek-ai/dsh-client-ui-sidebar-documentpreview/client';

export function AuraCodeBody({ resourceAddress, content, wrap, scrollportRef, t }: DocumentPreviewProps) {
  if (content.kind !== 'text') return null;
  const file = parseFileAddress(resourceAddress);
  if (!file) throw new Error(`dsh-highlight-aura: not a file address "${resourceAddress}"`);
  return (
    <div className="aura-code-viewer" data-code-preview data-wrap={wrap} data-language="aura">
      <CodeBanner label="aura" {...copyProps(t)} />
      <div ref={scrollportRef} data-code-block-content>
        <HighlightedPre code={content.text} streaming={!content.eof}
                        wrap={wrap} lineNumbers theme={useDarkTheme() ? 'aura-dark' : 'aura-light'} />
      </div>
    </div>
  );
}
```

`HighlightedPre` 内部按 §3.8 自建 chrome：语言标签 + 复制按钮 + 行号 gutter + `pre/code` 容器，类名对齐 `md-code-block` / `data-code-block-content` 以继承 DSH 的滚动条与 wrap 行为。

### 5.7 增量高亮（`incremental.ts`）

文档预览以 `text-pages` 模式累积读取，`content.eof` 为 `false` 时是流式状态。复用 DSH 在 `Pm` 类中验证过的策略：

1. 维护 `prefix`（已完成到最后一个换行处的已着色前缀）与 Shiki `grammarState`。
2. 新文本到达时只对 `prefix` 之后的增量调用 `codeToTokensBase(code, { lang, theme, grammarState })`。
3. 已冻结的行不再重算；仅对最后一行（尾块）重算。
4. 文本被回退重写（`!text.startsWith(prefix)`）时重置状态。

`grammarState` 续接能正确处理跨行结构（多行字符串、块注释、类体），这是纯正则方案做不到的一点。

工程上以 `requestAnimationFrame` 节流，每帧最多推进若干行；大文件（>1MB）先渲染纯文本骨架，再逐段着色。

### 5.8 注册与优先级

```ts
const BODY_ID  = '@aura-lang/dsh-highlight-aura/code';
const LOCALE   = 'sidebarAuraCodePreview';

export function apply(ctx) {
  injectStyles();                                             // §5.5 CSS 变量
  ctx.effect(() => ctx.locale.register(LOCALE, { zh, en }));
  const t = ctx.locale.bind(LOCALE);

  ctx.effect(() => ctx.documentPreviews.register({
    id: BODY_ID,
    extensions: ['aura'],
    priority: 'extension',        // 双保险 1：band 优先
    title: () => t('title'),      // 双保险 2：后缀长于兜底空后缀
    loading: 'text-pages',
    wrap: true,
  }));

  ctx.effect(() => ctx.slots.inject('sidebar.right.tab.document',
    () => ctx.slots.register(
      { name: 'sidebar.right.tab.document', key: BODY_ID, locale: LOCALE },
      AuraCodeBody)));
}

export const inject = ['slots', 'locale', 'documentPreviews'];
```

**能力边界说明**：由于 §2.3 与 §3.4，本方案只影响**文档预览面板**。聊天消息或 Markdown 文件中的 ```aura 围栏代码块仍会退化（`Ro.get('aura')` 未命中 → 纯文本 `<pre>`）。这是 N1，需方案 B 解决。

---

## 6. 分阶段实施计划

| 阶段 | 内容 | 交付物 | 验收标准 |
|------|------|--------|----------|
| M0 资产校验 | grammar 同步脚本 + 与 VSCode 插件 SHA 比对 | `scripts/*.mjs`、`grammar/SYNC-SHA256` | 与 VSCode 插件语法逐字节一致（除 `name`/`displayName` 两处规范化） |
| M1 高亮内核 | Shiki 单例 + Aura grammar + 双主题；命令行/Node 侧可跑出 HTML | `highlighter.ts`、`aura-grammar.ts`、`aura-theme.ts` | `examples/language-test/01-lexer.aura` 高亮输出中关键字/类型/字符串/注释各自落在预期 scope |
| M2 插件骨架 | `package.json`、`apply(ctx)`、locale、CSS 注入、无渲染 | `src/client.ts` | `dsh plugin add` 安装后 `dsh web` 启动无报错，控制台无插件加载异常 |
| M3 渲染集成 | `AuraCodeBody` + chrome（行号/复制/语言标签/wrap） | `AuraCodeBody.tsx`、`styles.css` | 打开任一 `.aura` 文件即高亮；wrap 开关、行号、复制功能与内置代码预览一致 |
| M4 增量与性能 | `incremental.ts`、rAF 节流、骨架降级 | `incremental.ts` | 100KB `.aura` 文件首屏 < 200 ms（纯文本骨架），着色完成 < 2 s；滚动不掉帧 |
| M5 健壮性 | 降级路径、错误兜底、主题切换即时生效 | 降级着色器 | CSP 阻断 wasm 时仍可预览（正则降级）；切换亮/暗主题无白屏 |
| M6 测试与文档 | scope 快照测试、fixtures、README、CI 校验 | `test/**`、`README.md` | 快照测试覆盖 examples/ 下 ≥10 个样本；grammar 漂移 CI 失败 |
| M7 上游推动 | 向 DSH 提 issue/PR（方案 B） | issue 链接 | 获得 DSH 维护方对扩展点的答复 |

---

## 7. 风险登记

| ID | 风险 | 影响 | 概率 | 缓解 |
|----|------|------|------|------|
| R1 | 重复承载 Shiki 造成包体积与 wasm 重复初始化开销 | 首次预览延迟 50–150 ms | 高 | 惰性单例 + 订阅就绪；纯文本骨架先行（M4）；方案 B 落地后移除 |
| R2 | `css-variables` 主题名是 DSH 内部约定，未来可能重命名 | 主题解析失败、颜色全失 | 中 | 插件自带 aura-light/aura-dark 双主题，不依赖 `css-variables`；颜色走 CSS 变量，主题名错了也不白屏 |
| R3 | Grammar 的 `enum-variant` 兜底正则过度着色 | 高亮噪声 | 高 | 已在 §5.3 记录为 grammar 改进项；插件侧可用额外 scope 覆盖降噪，不改语义 |
| R4 | 平台种子表 9 项是硬编码，Shiki 始终不可 import | 方案 A 无法瘦身 | 高 | 接受；方案 B 并行推进 |
| R5 | 本地插件带构建脚本，pnpm 默认拦截 prepare | 安装失败 | 中 | 提前在 profile 的 `pnpm-workspace.yaml` 加入 `allowBuilds`；或发布预构建包避免 prepare |
| R6 | `scopeName`/`name` 不一致导致 Shiki 语言标识错乱 | grammar 完全无效 | 中 | `sync-grammar.mjs` 强制规范化为 `"aura"`；M1 以 `getLoadedLanguages()` 断言 |
| R7 | 依赖 DSH 内部 reflect 服务名 `documentPreviews` | 上游改名则插件失效 | 低 | 服务名已在 `.d.ts` 中作为契约暴露；插件以 `ctx.documentPreviews === undefined` 做启动期探测并给出明确错误 |
| R8 | 与内置代码渲染器发生注册冲突 | 渲染器错乱 | 低 | `priority: 'extension'` + 后缀长度双保险；`id` 用包名唯一化，不占用内置 ID 空间 |

---

## 8. 验证方案

### 8.1 语法正确性（不依赖 UI）

Node 侧直接调 `auraCodeToHtml`，对 fixtures 做 **scope 快照测试**：

| Fixture | 断言重点 |
|---------|----------|
| `examples/language-test/01-lexer.aura` | 数字/字符串/转义/插值 scope |
| `examples/language-test/02-types-variables.aura` | `val`/`var`、类型注解、常量大写 |
| `examples/language-test/03-functions.aura` | `fun`、参数、返回类型、方法引用 `::` |
| `examples/language-test/04-control-flow.aura` | `if/when/for/select`、`?:`、`??`、`!!` |
| `examples/language-test/08-concurrency.aura` | `actor` 声明、`await`、channel |
| `examples/ffi/p8_c_ffi_demo.aura` | `extern "C" {...}` 块、`extern object` |
| `examples/basics/test_lambda6.aura` | 多行字符串、`${...}` 模板表达式 |
| `examples/language-test/12-annotations.aura` | `@aot`/`@native` 注解与参数 |

每条断言形如：`expect(tokenScopes(line, col)).toContain('keyword.control.aura')`。

### 8.2 端到端（DSH Web GUI）

```bash
dsh --profile web plugin add ./aura-dsh-highlight
dsh --profile web web
```

1. 打开 `D:\Code\AuraLang\examples\basics\hello-world.aura`，确认高亮且行号/复制/wrap 可用。
2. 切换亮/暗主题，确认配色随 CSS 变量更新，无白屏。
3. 打开 `tests/self_bootstrap/performance_test.aura` 等大文件，验证骨架先行与滚动流畅度。
4. `dsh --profile web plugin remove @aura-lang/dsh-highlight-aura` 后确认 `.aura` 优雅退回纯文本（不报错）。

### 8.3 回归护栏

- CI 执行 `pnpm run check:grammar`：grammar 与 `ide-extension/vscode-extension` 副本比对，漂移即失败，保证"参考 VSCode 插件"永不静默失真。
- 快照测试锁住 scope 输出；grammar 有意修改时必须显式更新快照。

---

## 9. 附录：证据索引

| 结论 | 位置 |
|------|------|
| 内置后缀表不含 `aura` | `dsh-client-ui-sidebar-documentpreview/lib/client.js:26751-26824` |
| `languageForPath` 返回 undefined 的路径 | 同上，`client.js:26830-26833` |
| 纯文本兜底渲染器 `extensions: []` | `client.js:1347-1355` |
| `TextBody` 无着色实现 | `client.js:1315-1337` |
| 代码渲染器注册方式与 `CodeBlock` 调用 | `client.js:26851-26872`、`client.js:26892-26911` |
| 文档预览定义契约与排序规则 | `dsh-client-ui-sidebar-documentpreview/lib/types/client/document/registry.d.ts` |
| 文档内容契约 `DocumentContent` / slot owner | `.../document/contract.d.ts` |
| 标签页定义覆盖所有 `file:` 地址 | `client.js:1766-1775` |
| Shiki + vscode-textmate 实现 | `dsh-web-frontend/dist/assets/vendor-CCJJTK99.js` |
| 语言 chunk = TextMate JSON | `dsh-web-frontend/dist/assets/langs/kotlin-BdnUsdx6.js` |
| `Sm`/`Ro`/`A3`/`Om` 私有语言表 | `dsh-web-frontend/dist/assets/index-BKQ_L1z6.js` |
| `CodeBlock` props 无 HTML 入口 | `index-BKQ_L1z6.js`，`function a8(...)` |
| 平台模块种子表 9 项 | `index-BKQ_L1z6.js`，`function by()` |
| `css-variables` 主题 CSS | `dsh-client-ui-theme/lib/client.js:1061-1072` |
| 客户端插件声明与惰性加载语义 | `dsh-client-modules/README.md`、`lib/types/client/manifest.d.ts` |
| 插件安装为 profile 依赖并 reconcile | `dsh/lib/plugin-Ddi42qoW.js` |
| profile 模板与 `dsh-base` 依赖图 | `dsh-app-boot/lib/index.js:328-357` |
| VSCode 侧语法与权威配色 | `ide-extension/vscode-extension/syntaxes/aura.tmLanguage.json`、`package.json` 的 `configurationDefaults` |
