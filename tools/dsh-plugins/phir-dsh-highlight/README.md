# @phir-lang/dsh-highlight-phir

DSH (DeepSeek Harness) Web GUI client plugin: syntax highlighting for the
[PHIR](https://github.com/phir-lang/phir) language.

DSH's built-in document preview already renders code, but it ships grammars for
the common languages and knows nothing about `\.phir`. This plugin registers
itself as the PHIR renderer and colors the code the same way the PHIR VSCode
extension does, in the same light and dark themes DSH is already using.

The grammar is not a re-implementation: it is the VSCode extension's
`phir.tmLanguage.json`, copied byte-for-byte into this package and diff-checked
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
mounted rows for `dsh.client` declarations 鈥?a client-only plugin still ships a
host half (`lib/index.js`) whose `apply` does nothing. The package declares
`dsh.bundle.patch` (`cordis.patch.yml`) so the profile can mount that row.

### 1. Register it in the web profile

Edit `~/.dsh/profiles/web/package.json` and add the package under
`dependencies`, then add it to the bundle list:

```json
{
  "dependencies": {
    "@phir-lang/dsh-highlight-phir": "link:D:/Code/AuraLang/tools/dsh-plugins/phir-dsh-highlight"
  },
  "dsh": {
    "profile": {
      "bundles": [
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-web-app",
        "@phir-lang/dsh-highlight-phir"
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

### 3. Open a `\.phir` file

Open any `*\.phir` file in the DSH sidebar. The tab opens directly into the
highlighted view; if another renderer also claims the suffix, the PHIR one wins
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

The TextMate grammar lives upstream in the VSCode extension, and this package
ships a copy. Three scripts keep that copy honest:

```bash
pnpm sync:grammar     # copy the grammar from the extension into grammar/
pnpm bundle:grammar   # grammar/phir.tmLanguage.json -> src/phir-grammar.ts
pnpm check:grammar    # diff the copy against upstream; fails on drift
```

`scripts/_grammar-source.mjs` locates the extension in either layout — the
`ide-extension/vscode-extension/` tree this plugin calls home, or
`tools/ide-extension/phir-vscode-extension/` where AuraLang keeps it — so no
environment variable is needed in a normal checkout. Point it elsewhere with
`PHIR_GRAMMAR_PATH=/path/to/phir.tmLanguage.json`.

`sync:grammar` is the only writer: it copies the file and rewrites exactly two
fields, `name` and `displayName`, to `PHIR` so Shiki's language id matches the
one this plugin registers. Everything else, including every scope name, is
upstream's. `check:grammar` re-derives the same normalization from upstream and
fails on any other difference, and `pnpm prepublishOnly` runs it, so a drifted
grammar cannot ship.

A synced grammar is a *normalized* copy, not a byte-identical one; the upstream
fingerprint is recorded in `grammar/SYNC-SHA256`.

---

## How it works

The host mounts one row for this package (`cordis.patch.yml`). Its host half does
nothing; the row exists so `dsh-client-modules` sees the `dsh.client` declaration
and serves `lib/client.js`, whose `apply` makes the four registrations below.

```
                        DSH client context
                                 鈹?
                 鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹尖攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                 鈻?              鈻?               鈻?
          locale service    documentPreviews   sidebar slot
          ctx.locale.*      registry           sidebar.right.tab.document
                 鈹?              鈹?               鈹?
                 鈹?  registers   鈹?   registers   鈹?
                 鈹?  t()         鈹?   \.phir def   鈹?
                 鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹攢鈹€鈹€鈹€鈹€鈹€鈹€鈹?               鈹?
                         鈹?                       鈹?
                         鈻?                       鈻?
                   鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                   鈹?       src/client.ts (apply)          鈹?
                   鈹? inject: ['slots','locale',           鈹?
                   鈹?         'documentPreviews']          鈹?
                   鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                                       鈹?hands each \.phir tab
                                       鈻?
                        鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                        鈹?     src/PHIRCodeBody.tsx     鈹?
                        鈹? <div data-phir-code>          鈹?
                        鈹?   <div data-phir-banner>      鈹?
                        鈹?   <div data-phir-scroll>      鈹?
                        鈹?     <div data-phir-grid>      鈹?
                        鈹?       <span data-phir-num>    鈹?
                        鈹?       <span data-phir-text>   鈹?
                        鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                                        鈹?per frame
                     鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹尖攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                     鈻?                                     鈻?
          鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?        鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
          鈹?  src/incremental.ts    鈹?        鈹?     src/fallback.ts     鈹?
          鈹? tokenizeNextChunk      鈹?        鈹? tokenizeFallback        鈹?
          鈹? codeToTokens +         鈹?        鈹? regex state machine     鈹?
          鈹? grammarState carry-    鈹?        鈹? (no Shiki at all)       鈹?
          鈹? forward, 32 KB /       鈹?        鈹?                         鈹?
          鈹? 4000 lines per frame   鈹?        鈹?                         鈹?
          鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?        鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
                       鈻?
          鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
          鈹?             src/highlighter.ts                   鈹?
          鈹? one lazy singleton: createHighlighterCore with   鈹?
          鈹? the bundled grammar + both PHIR themes           鈹?
          鈹?                                                  鈹?
          鈹?            src/engine.ts                          鈹?
          鈹? javascript (default) 鈹?oniguruma (opt-in wasm)   鈹?
          鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹?
```

### Delivery: complete bytes

The renderer declares `loading: 'bytes-complete'`: the owner hands the file over
whole as a `Uint8Array` and the renderer decodes it (`src/decode.ts`). Both modes
are first-class in the owner's contract — PDF, HTML and images are byte-mode too
— but for a source-file renderer this is the unusual choice, and it is made for
one reason.

PHIR is a compiler IR, and an IR dump is commonly a textual header followed by a
binary payload. The owner's *paged* text read refuses any page whose text holds a
NUL byte (`workspace-file/not-text`) and its filesystem backend refuses malformed
UTF-8. `text-pages` scans the first page — the first 5000 lines by default —
before any renderer is consulted, so such a file fails the read and the pane
reports a non-text file. No renderer-side work can change that: the refusal
happens upstream of the registry. Byte delivery is the only way in.

`src/decode.ts` is what makes the bytes readable: malformed UTF-8 degrades to
U+FFFD, NUL bytes are dropped, and the textual header survives.

**What it costs.** Paged transfer, and with it the owner's source-line
navigation: byte-mode renderers do not consume `?line=` deep links, so a
`file.phir:42` reference from chat does not scroll. The anchors are still emitted
(`data-textpreview-line`), which is what a renderer-side navigation would use.

**Switching.** One line in `src/client.ts` — `loading` in the
`documentPreviews.register` call — and the body renders either shape, so nothing
else moves. `test/client-bundle.test.mjs` pins the value, so the mode cannot
change silently. Note that the registry selects by file *extension* only, never
by content, so the two modes cannot be combined per file: whichever one the
registration declares is what every `.phir` file gets.

`FRAME_CHARS` / `FRAME_LINES` bound the per-frame tokenizer work and `MAX_LINES`
bounds what is rendered, so a pathological multi-megabyte dump cannot hang the
pane.

### The renderer is progressive

Re-tokenizing the whole document every frame is quadratic, so the renderer:

1. paints plain text immediately, the moment content arrives;
2. warms the Shiki highlighter in the background;
3. when it is ready, tokenizes the source in budgeted chunks over successive
   frames, carrying Shiki's `grammarState` from one chunk to the next;
4. if the highlighter cannot be built at all, falls back to the regex tokenizer.

Nothing throws into the preview pane. A host without WebAssembly, a blocked
grammar load, or a future Shiki API change degrades to the fallback; a throwing
fallback degrades to plain text.

### Chunk boundaries

A chunk that ends in a newline makes Shiki emit a trailing empty line. That line
does not exist in the source yet 鈥?it is the seam with the next chunk 鈥?so
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
--PHIR-keyword: #cf222e;
/* dark, under body[data-ds-dark-theme] */
--PHIR-keyword: #f85149;
```

Tokens render as `var(--PHIR-keyword, #cf222e)`: DSH's attribute toggle flips the
palette, and the fallback keeps the code readable if the variables never load.

Override any color from your own CSS to rebrand the viewer:

```css
[data-phir-code] { --PHIR-keyword: #7c4dff; }
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
import { tokenizePHIR, tokenizeFallback, isPHIRPath } from '@phir-lang/dsh-highlight-phir/highlight';

const tokens = await tokenizePHIR(source, { theme: 'dark' });
// ThemedToken[][] 鈥?one array per line

tokenizeFallback(source, PHIR_DARK_PALETTE);  // synchronous, no Shiki
isPHIRPath('examples/basics/hello\.phir');      // true
```

Other exports: `getHighlighter`, `tokenizePHIRWithState`,
`createIncrementalState` / `tokenizeNextChunk`, `decodePHIRSource`,
`createEngine`, both palettes, both `ThemeRegistration` objects,
`themeVariableCss`, and `PHIR_GRAMMAR`.

### Engine selection

```ts
// pure JS, no WebAssembly, no network 鈥?the default
const tokens = await tokenizePHIR(source, { theme: 'light' });

// oniguruma: about an order of magnitude faster, but the caller supplies the wasm
const tokens = await tokenizePHIR(source, {
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

**`\.phir` files still open in the plain text viewer.**
The document preview registry ranks implementations by priority band first, then
by longest suffix, then by registration order. This plugin registers `PHIR` at
`extension` priority, so it wins over the built-in band. If another plugin also
registers `PHIR` at `extension`, the later registration loses and its id is
logged; check the DSH client logs for a `documentPreviews` conflict.

**Colors look wrong or are all one color.**
Check whether the host applied `body[data-ds-dark-theme]`. The dark palette is
only installed under that selector, so a host that signals dark mode differently
will render with the light palette and its fallbacks.

**Nothing is highlighted at all.**
The viewer degrades silently rather than throwing. Open the browser console and
look for `[phir-dsh-highlight]`. The most common cause is a host that blocks
both WebAssembly and the pure-JS engine; the second most common is a grammar
that fails to parse, which then drops the render to the regex fallback.

**Highlighting lags on a very large file.**
Raise the per-frame budget in `src/PHIRCodeBody.tsx` (`FRAME_CHARS`,
`FRAME_LINES`), or switch to oniguruma. Both are tradeoffs: a larger budget
means a longer frame, a faster engine means a larger bundle.

**A `.phir` file reports a non-text file.**
`workspace-file/not-text` comes from the owner's *paged* text read, before a
renderer runs: it fires when a page holds a NUL byte or the bytes are not UTF-8.
This plugin takes complete bytes instead, so seeing the message means the PHIR
renderer was not the selected one — the plugin is not mounted, or the tab's
renderer dropdown is sitting on plain text. Pick the PHIR renderer.

---

## Layout

```
package.json           name, exports, dsh.bundle + dsh.client declaration
cordis.patch.yml       bundle patch: inserts the host row the roster scan reads
tsconfig.json          strict TS config, noEmit
tsconfig.build.json    declaration emit into lib/types
pnpm-workspace.yaml    self-contained install; esbuild build-script allowlist
grammar/
  phir.tmLanguage.json  the VSCode grammar, normalized copy (name/displayName)
  SYNC-SHA256            upstream hash + provenance for the diff check
scripts/
  _grammar-source.mjs   locate the extension in either checkout layout
  sync-grammar.mjs      copy the grammar from the extension checkout
  bundle-grammar.mjs    grammar JSON -> src/phir-grammar.ts
  diff-grammar.mjs      fail the build on grammar drift
  build.mjs             tsc declarations + three esbuild entries
  dump-tokens.mjs       dev aid: print token colors for a sample
src/
  types.ts              TextmateGrammar type, isPHIRPath, PHIR_EXTENSION
  phir-grammar.ts       generated from grammar/
  engine.ts             javascript | oniguruma engine factory
  highlighter.ts        lazy singleton, tokenizePHIR, tokenizePHIRWithState
  incremental.ts        chunked tokenization with grammarState carry-forward
  fallback.ts           regex tokenizer, zero Shiki dependency
  decode.ts             complete-byte -> text (NUL bytes, malformed UTF-8)
  theme.ts              both palettes, both themes, CSS variable emitter
  locale.ts             zh/en dictionaries and the namespace key
  plugin-context.ts     structural ctx slice, assertPluginContext, ids
  styles.ts             viewer CSS + theme variables
  declarations.ts       LocaleNamespaceMap merge
  PHIRCodeBody.tsx      the renderer
  client.ts             apply(ctx): the DSH client plugin entry
  index.ts              host half: the Loader row's empty apply
  highlight/index.ts    the DOM-free public API
test/
  highlight.test.mjs    25 scope, theme, shiki, and incremental tests
  client-bundle.test.mjs 4 envelope and registration-contract tests
  fixtures/
    mini_struct.phir     round-trip fixture, shipped with the package
```

### License

Apache-2.0. The bundled grammar is the PHIR language's own VSCode grammar and is
retained here under the same terms as the upstream extension.






