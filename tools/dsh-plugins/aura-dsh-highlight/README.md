# @aura-lang/dsh-highlight-aura

DSH (DeepSeek Harness) Web GUI client plugin: syntax highlighting for the
[Aura](https://github.com/aura-lang/auralang) language.

DSH's built-in document preview already renders code, but it ships grammars for
the common languages and knows nothing about `.aura`. This plugin registers
itself as the Aura renderer and colors the code the same way the Aura VSCode
extension does, in the same light and dark themes DSH is already using.

The grammar is not a re-implementation: it is the VSCode extension's
`aura.tmLanguage.json`, copied byte-for-byte into this package and diff-checked
on every build, so the two editors never drift.

---

## Contents

1. [Install](#install)
2. [Build and test](#build-and-test)
3. [How it works](#how-it-works)
4. [Theming](#theming)
5. [API](#api)
6. [Troubleshooting](#troubleshooting)
7. [Layout](#layout)

---

## Install

The plugin is consumed as a client plugin by the Web profile. It is listed as a
profile *bundle* because that is the only way its Loader row gets mounted, and
`@deepseek-ai/dsh-client-modules` builds the browser roster by scanning the
mounted rows for `dsh.client` declarations — a client-only plugin still ships a
host half (`lib/index.js`) whose `apply` does nothing. The package declares
`dsh.bundle.patch` (`cordis.patch.yml`) so the profile can mount that row.

### 1. Register it in the web profile

Edit `~/.dsh/profiles/web/package.json` and add the package under
`dependencies`, then add it to the bundle list:

```json
{
  "dependencies": {
    "@aura-lang/dsh-highlight-aura": "link:D:/Code/AuraLang/tools/dsh-plugins/aura-dsh-highlight"
  },
  "dsh": {
    "profile": {
      "bundles": [
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-web-app",
        "@aura-lang/dsh-highlight-aura"
      ]
    }
  }
}
```

`link:` keeps the plugin live from the checkout, so a rebuild is picked up
without re-installing. Use a real version range instead when publishing.

### 2. Install and restart

```bash
cd ~/.dsh/profiles/web
pnpm install
dsh web
```

### 3. Open a `.aura` file

Open any `*.aura` file in the DSH sidebar. The tab opens directly into the
highlighted view; if another renderer also claims the suffix, the Aura one wins
because it registers at `extension` priority, which beats the built-in band.

---

## Build and test

```bash
pnpm install     # install dev deps (types, esbuild, tsc, react)
pnpm build       # emit lib/
pnpm test        # build, then run the node:test suite
```

`pnpm build` produces five artifacts:

| Artifact            | Purpose                                                   |
| ------------------- | --------------------------------------------------------- |
| `lib/client.js`     | Browser half, wrapped in DSH's lazy-module envelope       |
| `lib/highlight.js`  | DOM-free highlighting API for Node and tests              |
| `lib/index.js`      | Host half: the Loader row's empty `apply`                 |
| `lib/highlight.js.map` / `lib/index.js.map` | Linked source maps                        |
| `lib/types/**`      | Declarations for all three entries                        |

`lib/client.js` is wrapped by hand, not by esbuild: DSH's module loader does not
consume a bare CJS module, it wants the `window.__ModuleLoader__.load({id, factory})`
envelope. `react` and `react/jsx-runtime` stay external so the plugin shares the
host's own React instance instead of shipping a second copy.

### Grammar synchronization

The TextMate grammar lives upstream in the VSCode extension. Three scripts keep
this copy honest:

```bash
pnpm bundle:grammar   # copy the grammar from the repo into grammar/ + src/
pnpm check:grammar    # diff this copy against upstream; fails on drift
```

`pnpm prepublishOnly` runs the build and the grammar diff, so a drifted grammar
cannot ship.

Point the sync at a different checkout with `AURA_GRAMMAR_PATH=/path/to/aura.tmLanguage.json`.

---

## How it works

The host mounts one row for this package (`cordis.patch.yml`). Its host half does
nothing; the row exists so `dsh-client-modules` sees the `dsh.client` declaration
and serves `lib/client.js`, whose `apply` makes the four registrations below.

```
                        DSH client context
                                 │
                 ┌───────────────┼────────────────┐
                 ▼               ▼                ▼
          locale service    documentPreviews   sidebar slot
          ctx.locale.*      registry           sidebar.right.tab.document
                 │               │                │
                 │   registers   │    registers   │
                 │   t()         │    .aura def   │
                 └───────┬───────┘                │
                         │                        │
                         ▼                        ▼
                   ┌──────────────────────────────────────┐
                   │        src/client.ts (apply)          │
                   │  inject: ['slots','locale',           │
                   │          'documentPreviews']          │
                   └───────────────────┬──────────────────┘
                                       │ hands each .aura tab
                                       ▼
                        ┌──────────────────────────────┐
                        │      src/AuraCodeBody.tsx     │
                        │  <div data-aura-code>          │
                        │    <div data-aura-banner>      │
                        │    <div data-aura-scroll>      │
                        │      <div data-aura-grid>      │
                        │        <span data-aura-num>    │
                        │        <span data-aura-text>   │
                        └───────────────┬──────────────┘
                                        │ per frame
                     ┌──────────────────┼──────────────────┐
                     ▼                                      ▼
          ┌─────────────────────────┐         ┌──────────────────────────┐
          │   src/incremental.ts    │         │      src/fallback.ts     │
          │  tokenizeNextChunk      │         │  tokenizeFallback        │
          │  codeToTokens +         │         │  regex state machine     │
          │  grammarState carry-    │         │  (no Shiki at all)       │
          │  forward, 32 KB /       │         │                          │
          │  4000 lines per frame   │         │                          │
          └────────────┬────────────┘         └──────────────────────────┘
                       ▼
          ┌──────────────────────────────────────────────────┐
          │              src/highlighter.ts                   │
          │  one lazy singleton: createHighlighterCore with   │
          │  the bundled grammar + both Aura themes           │
          │                                                   │
          │             src/engine.ts                          │
          │  javascript (default) │ oniguruma (opt-in wasm)   │
          └──────────────────────────────────────────────────┘
```

### The renderer is progressive

DSH delivers a document as an accumulated text prefix that grows on every
frame. Re-tokenizing the whole file each frame is quadratic, so the renderer:

1. paints plain text immediately, the moment content arrives;
2. warms the Shiki highlighter in the background;
3. when it is ready, tokenizes only the newly appended tail, carrying Shiki's
   `grammarState` across frames;
4. if the highlighter cannot be built at all, falls back to the regex tokenizer.

Nothing throws into the preview pane. A host without WebAssembly, a blocked
grammar load, or a future Shiki API change degrades to the fallback; a throwing
fallback degrades to plain text.

### Chunk boundaries

A chunk that ends in a newline makes Shiki emit a trailing empty line. That line
does not exist in the source yet — it is the seam with the next chunk — so
`incremental.ts` drops it at every non-final chunk or each boundary would print
a spurious blank line. The final chunk keeps it, so the last line count still
matches `code.split('\n').length`.

### Rewrite detection

`grammarState` is only valid for the text it was derived from. The renderer
keeps a 4096-character tail fingerprint of what it has consumed and resets the
whole state when the new prefix no longer extends it, so a reloaded file or an
edit does not leak stale grammar state into the render.

---

## Theming

Token colors are CSS variables with a hex fallback, so the renderer follows DSH's
theme switch without re-tokenizing:

```css
/* light, on :root */
--aura-keyword: #cf222e;
/* dark, under body[data-ds-dark-theme] */
--aura-keyword: #f85149;
```

Tokens render as `var(--aura-keyword, #cf222e)`: DSH's attribute toggle flips the
palette, and the fallback keeps the code readable if the variables never load.

Override any color from your own CSS to rebrand the viewer:

```css
[data-aura-code] { --aura-keyword: #7c4dff; }
```

The full vocabulary is `ColorKey` in `src/theme.ts`: `foreground`, `comment`,
`keyword`, `storage`, `type`, `function`, `string`, `number`, `boolean`,
`constant`, `variable`, `parameter`, `operator`, `punctuation`, `tag`, `attribute`,
`annotation`, `regexp`, `namespace`, `module`, `enum`, `class`, `interface`,
`struct`, `builtin`, `label`, `property`.

---

## API

The DOM-free API is exported from the `./highlight` subpath and works in Node,
CLI tools, and tests:

```ts
import { tokenizeAura, tokenizeFallback, isAuraPath } from '@aura-lang/dsh-highlight-aura/highlight';

const tokens = await tokenizeAura(source, { theme: 'dark' });
// ThemedToken[][] — one array per line

tokenizeFallback(source, AURA_DARK_PALETTE);  // synchronous, no Shiki
isAuraPath('examples/basics/hello.aura');      // true
```

Other exports: `getHighlighter`, `tokenizeAuraWithState`,
`createIncrementalState` / `tokenizeNextChunk`, `createEngine`, both palettes,
both `ThemeRegistration` objects, `themeVariableCss`, and `AURA_GRAMMAR`.

### Engine selection

```ts
// pure JS, no WebAssembly, no network — the default
const tokens = await tokenizeAura(source, { theme: 'light' });

// oniguruma: about an order of magnitude faster, but the caller supplies the wasm
const tokens = await tokenizeAura(source, {
  theme: 'light',
  kind: 'oniguruma',
  wasm: '/assets/onig.wasm',
});
```

The default is the pure-JS engine on purpose: a document-preview plugin has to
work in hosts with restrictive CSP, offline installs, and sandboxed iframes. The
per-frame budget in `incremental.ts` keeps the slower engine interactive.

---

## Troubleshooting

**`.aura` files still open in the plain text viewer.**
The document preview registry ranks implementations by priority band first, then
by longest suffix, then by registration order. This plugin registers `aura` at
`extension` priority, so it wins over the built-in band. If another plugin also
registers `aura` at `extension`, the later registration loses and its id is
logged; check the DSH client logs for a `documentPreviews` conflict.

**Colors look wrong or are all one color.**
Check whether the host applied `body[data-ds-dark-theme]`. The dark palette is
only installed under that selector, so a host that signals dark mode differently
will render with the light palette and its fallbacks.

**Nothing is highlighted at all.**
The viewer degrades silently rather than throwing. Open the browser console and
look for `[aura-dsh-highlight]`. The most common cause is a host that blocks
both WebAssembly and the pure-JS engine; the second most common is a grammar
that fails to parse, which then drops the render to the regex fallback.

**Highlighting lags on a very large file.**
Raise the per-frame budget in `src/AuraCodeBody.tsx` (`FRAME_CHARS`,
`FRAME_LINES`), or switch to oniguruma. Both are tradeoffs: a larger budget
means a longer frame, a faster engine means a larger bundle.

---

## Layout

```
package.json           name, exports, dsh.bundle + dsh.client declaration
cordis.patch.yml       bundle patch: inserts the host row the roster scan reads
tsconfig.json          strict TS config, noEmit
tsconfig.build.json    declaration emit into lib/types
pnpm-workspace.yaml    self-contained install; esbuild build-script allowlist
grammar/
  aura.tmLanguage.json  the VSCode grammar, byte-identical copy
  SYNC-SHA256            upstream hash + provenance for the diff check
scripts/
  sync-grammar.mjs      copy the grammar from the repo checkout
  bundle-grammar.mjs    grammar JSON -> src/aura-grammar.ts
  diff-grammar.mjs      fail the build on grammar drift
  build.mjs             tsc declarations + three esbuild entries
  dump-tokens.mjs       dev aid: print token colors for a sample
src/
  types.ts              TextmateGrammar type, isAuraPath, AURA_EXTENSION
  aura-grammar.ts       generated from grammar/
  engine.ts             javascript | oniguruma engine factory
  highlighter.ts        lazy singleton, tokenizeAura, tokenizeAuraWithState
  incremental.ts        chunked tokenization with grammarState carry-forward
  fallback.ts           regex tokenizer, zero Shiki dependency
  theme.ts              both palettes, both themes, CSS variable emitter
  locale.ts             zh/en dictionaries and the namespace key
  plugin-context.ts     structural ctx slice, assertPluginContext, ids
  styles.ts             viewer CSS + theme variables
  declarations.ts       LocaleNamespaceMap merge
  AuraCodeBody.tsx      the renderer
  client.ts             apply(ctx): the DSH client plugin entry
  index.ts              host half: the Loader row's empty apply
  highlight/index.ts    the DOM-free public API
test/
  highlight.test.mjs    25 scope, theme, shiki, and incremental tests
  client-bundle.test.mjs 4 envelope and registration-contract tests
```

### License

Apache-2.0. The bundled grammar is the Aura language's own VSCode grammar and is
retained here under the same terms as the upstream extension.
