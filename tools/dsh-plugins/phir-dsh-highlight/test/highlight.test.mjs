/**
 * Scope snapshot tests for the PHIR Highlighter.
 *
 * These import the *built* artifact (`../lib/highlight.js`), so a green run
 * means the shipped bytes work, not just the source. `pretest` builds first.
 *
 * Assertions favor token *content* over token boundaries: Shiki may merge or
 * split tokens differently between engine versions, but the colored text must
 * stay the same.
 */

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  PHIR_DARK_PALETTE,
  PHIR_DARK_THEME,
  PHIR_LIGHT_PALETTE,
  PHIR_LIGHT_THEME,
  THEME_BY_MODE,
  colorRef,
  PHIR_EXTENSION,
  createIncrementalState,
  decodePHIRSource,
  getHighlighter,
  resetHighlighter,
  themeVariableCss,
  tokenizePHIR,
  tokenizeFallback,
  tokenizeNextChunk,
  isPhirPath,
  PHIR_GRAMMAR,
} from '../lib/highlight.js';

const here = dirname(fileURLToPath(import.meta.url));

/**
 * The round-trip fixture ships with the package.
 *
 * It used to be found by walking up from the plugin directory into the host
 * checkout's `examples/`, which coupled the test to a repository the plugin is
 * not necessarily installed in, and broke silently whenever that tree moved
 * (`<repo>/tools/dsh-plugins/...` vs `<repo>/rust/tools/dsh-plugins/...`).
 */
const FIXTURE = resolve(here, 'fixtures', 'mini_struct.phir');

const HELLO = `# a line comment
fun main() {
    println("Hello, PHIR!")
}`;

const SAMPLE = `# basics/mini_struct.phir
struct Point(val x: Int, val y: Int)

fun dist(p: Point): Int {
    return p.x + p.y
}

fun main() {
    val p = Point(3, 4)
    println(p.x)
}`;

/** Flatten the token text of one line. */
function lineText(line) {
  return line.map((token) => token.content).join('');
}

/** Every color string appearing in a token array. */
function colorsOf(tokens2d) {
  const colors = new Set();
  for (const line of tokens2d) {
    for (const token of line) {
      if (token.color !== undefined) colors.add(token.color);
    }
  }
  return colors;
}

/** Does any token carry a color whose var() name contains `needle`? */
function usesVar(tokens2d, needle) {
  return [...colorsOf(tokens2d)].some((color) => color.includes(needle));
}

test('isPhirPath accepts \.phir and rejects look-alikes', () => {
  assert.equal(isPhirPath('hello\.phir'), true);
  assert.equal(isPhirPath('dir/nested/hello\.phir'), true);
  assert.equal(isPhirPath('dir\\nested\\hello\.phir'), true);
  assert.equal(isPhirPath('HELLO\.phir'), true);
  assert.equal(isPhirPath('PHIR'), false);
  assert.equal(isPhirPath(''), false);
  assert.equal(isPhirPath(undefined), false);
  assert.equal(isPhirPath('PHIR.ts'), false);
  assert.equal(isPhirPath('\.phir'), false);
  assert.equal(isPhirPath('name.'), false);
  assert.equal(isPhirPath('dir/'), false);
  assert.equal(isPhirPath('a\.phir.bak'), false);
});

test('grammar is intact and carries the VSCode identity', () => {
  assert.equal(PHIR_GRAMMAR.scopeName, 'source.phir');
  assert.equal(PHIR_GRAMMAR.name, 'PHIR');
  assert.equal(PHIR_GRAMMAR.displayName, 'PHIR');
  assert.ok(PHIR_GRAMMAR.patterns.length > 0, 'root patterns');
  const repo = PHIR_GRAMMAR.repository;
  assert.ok(repo && typeof repo === 'object', 'repository');
  const keys = Object.keys(repo);
  assert.ok(keys.length >= 50, `repository rules (${keys.length})`);
  // A sample of the rule families the renderer depends on. `sync-grammar.mjs`
  // rewrites only `name` and `displayName`, so these are the upstream names.
  for (const key of [
    'comments',
    'module-header',
    'native-function-declaration',
    'function-declaration',
    'basic-block',
  ]) {
    assert.ok(keys.includes(key), `repository has #${key}`);
  }
  // The document preview claims `PHIR_EXTENSION`; the grammar declares the
  // suffix it belongs to. A drift between the two silently disables the
  // highlight registration, so pin them to each other.
  assert.ok(
    (PHIR_GRAMMAR.fileTypes ?? []).some((type) => type.toLowerCase() === PHIR_EXTENSION),
    `grammar fileTypes ${JSON.stringify(PHIR_GRAMMAR.fileTypes)} covers .${PHIR_EXTENSION}`,
  );
});

test('fallback: line count matches the input', () => {
  const code = 'a\nb\n\nc\n';
  assert.equal(tokenizeFallback(code, PHIR_LIGHT_PALETTE).length, code.split('\n').length);
  assert.equal(tokenizeFallback('', PHIR_LIGHT_PALETTE).length, 1);
  assert.equal(tokenizeFallback('x', PHIR_LIGHT_PALETTE).length, 1);
});

test('fallback: colors comments, strings, numbers, and keywords', () => {
  const code = [
    '# note',
    'val s = "text"',
    'val n = 42',
    'if true else false',
  ].join('\n');
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--PHIR-comment'), true, 'comment');
  assert.equal(usesVar(tokens, '--PHIR-string'), true, 'string');
  assert.equal(usesVar(tokens, '--PHIR-number'), true, 'number');
  assert.equal(usesVar(tokens, '--PHIR-keyword'), true, 'keyword');
  assert.equal(usesVar(tokens, '--PHIR-boolean'), true, 'boolean');
});

test('fallback: text round-trips exactly', () => {
  const code = '# x\nfun f(a: Int): Int { return a }\n# y\n';
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: comment syntax mirrors the grammar, which only knows #', () => {
  // `comment-line` in the grammar is `#` to end of line; the fallback colored
  // `//` and `/* */` for a language that never had them, which left every real
  // comment - starting with the `# module ...` header - uncolored.
  const comment = tokenizeFallback('# module simple target x86_64\n', PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(comment, '--PHIR-comment'), true, '# marks a comment');

  for (const notAComment of ['// note\n', '/* note */\n']) {
    assert.equal(
      usesVar(tokenizeFallback(notAComment, PHIR_LIGHT_PALETTE), '--PHIR-comment'),
      false,
      `${JSON.stringify(notAComment.trim())} is not PHIR comment syntax`,
    );
  }
});

test('fallback: a comment ends at the line break', () => {
  const code = '# one\nval after = 1\n';
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--PHIR-comment'), true, 'the comment is colored');
  assert.equal(usesVar(tokens, '--PHIR-keyword'), true, 'the next line is code again');
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: triple-quoted string spans lines', () => {
  const code = 'val s = """line one\nline two"""\n';
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--PHIR-string'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: escaped quote does not close the string', () => {
  const code = 'val s = "say \\"hi\\""\n';
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--PHIR-string'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: numbers with prefixes, exponents, and suffixes', () => {
  const code = 'val a = 0x1F\nval b = 0b1010\nval c = 1.5e3\nval d = 1_000L\n';
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--PHIR-number'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: punctuation is its own token, not glued to identifiers', () => {
  const code = 'val a = b + c\n';
  const tokens = tokenizeFallback(code, PHIR_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--PHIR-punctuation'), true);
  assert.equal(usesVar(tokens, '--PHIR-keyword'), true);
  const firstLine = tokens[0];
  // Whitespace sticks to whichever side emits it, so compare trimmed text.
  const contents = firstLine.map((token) => token.content.trim());
  assert.ok(contents.includes('='), `operator tokenized on its own (${contents.join('|')})`);
  assert.ok(contents.includes('+'), `second operator on its own (${contents.join('|')})`);
  for (const content of contents) {
    assert.ok(
      !/^[A-Za-z_][\w$]*\s*[=+\-]/.test(content),
      `no word glued to an operator: ${content}`,
    );
  }
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: an empty document yields one empty line', () => {
  const tokens = tokenizeFallback('', PHIR_LIGHT_PALETTE);
  assert.equal(tokens.length, 1);
  assert.equal(tokens[0].length, 0);
});

test('theme: both palettes define every shared color key', () => {
  const lightKeys = Object.keys(PHIR_LIGHT_PALETTE).sort();
  const darkKeys = Object.keys(PHIR_DARK_PALETTE).sort();
  assert.deepEqual(darkKeys, lightKeys);
  assert.ok(lightKeys.length >= 24, `palette has ${lightKeys.length} keys`);
});

test('theme: token colors are CSS variable references with hex fallbacks', () => {
  const colors = PHIR_LIGHT_THEME.tokenColors ?? [];
  assert.ok(colors.length >= 40, `tokenColors (${colors.length})`);
  const foregrounds = colors
    .map((entry) => entry.settings?.foreground)
    .filter((value) => typeof value === 'string');
  assert.ok(foregrounds.length > 0, 'at least one foreground');
  for (const color of foregrounds) {
    assert.match(
      color,
      /^var\(--PHIR-[a-z0-9-]+, #[0-9a-f]{6}\)$/i,
      `unexpected color form: ${color}`,
    );
  }
});

test('theme: light and dark resolve the same scope table', () => {
  const lightScopes = PHIR_LIGHT_THEME.tokenColors.map((entry) => JSON.stringify(entry.scope));
  const darkScopes = PHIR_DARK_THEME.tokenColors.map((entry) => JSON.stringify(entry.scope));
  assert.deepEqual(darkScopes, lightScopes);
});

test('theme: THEME_BY_MODE maps onto the registered theme names', () => {
  assert.equal(THEME_BY_MODE.light, PHIR_LIGHT_THEME.name);
  assert.equal(THEME_BY_MODE.dark, PHIR_DARK_THEME.name);
});

test('themeVariableCss: installs light and dark blocks with every key', () => {
  const css = themeVariableCss();
  assert.ok(css.includes(':root{'), 'light block');
  assert.ok(css.includes('body[data-ds-dark-theme]{'), 'dark block');
  for (const key of Object.keys(PHIR_LIGHT_PALETTE)) {
    assert.ok(css.includes(`--PHIR-${key}:`), `emits --PHIR-${key}`);
  }
});

/** The color a token falls back to when no theme rule matches it. */
const LIGHT_FOREGROUND = colorRef('foreground', PHIR_LIGHT_PALETTE);

/** Color of the first token whose trimmed text is exactly `word`. */
function colorOfWord(tokens2d, word) {
  for (const line of tokens2d) {
    for (const token of line) {
      if (token.content.trim() === word) return token.color;
    }
  }
  return undefined;
}

/** Did the theme actually color this word, or did it fall through to the default? */
function isColored(tokens2d, word) {
  const color = colorOfWord(tokens2d, word);
  return color !== undefined && color !== LIGHT_FOREGROUND;
}

/**
 * Scopes the grammar can emit that no theme rule matches render in the editor
 * foreground - visually identical to unhighlighted text, which is exactly the
 * failure this guards. `meta.*` is exempt: those are enclosing container scopes
 * by convention, and the color belongs to the leaf scopes inside them.
 */
test('theme: every scope the grammar can emit is covered by a rule', () => {
  const emitted = new Set();
  const walk = (node) => {
    if (Array.isArray(node)) {
      for (const item of node) walk(item);
      return;
    }
    if (node === null || typeof node !== 'object') return;
    for (const [key, value] of Object.entries(node)) {
      if ((key === 'name' || key === 'contentName') && typeof value === 'string') emitted.add(value);
      else walk(value);
    }
  };
  walk(PHIR_GRAMMAR);
  emitted.delete(PHIR_GRAMMAR.name);
  emitted.delete(PHIR_GRAMMAR.displayName);

  const selectors = PHIR_LIGHT_THEME.tokenColors.flatMap((entry) =>
    Array.isArray(entry.scope) ? entry.scope : [entry.scope],
  );
  const covered = (scope) => {
    const parts = scope.split('.');
    return selectors.some((selector) => {
      const want = selector.split('.');
      return want.length <= parts.length && want.every((segment, i) => segment === parts[i]);
    });
  };

  const uncovered = [...emitted]
    .filter((scope) => !scope.startsWith('meta.') && !covered(scope))
    .sort();
  assert.deepEqual(uncovered, [], `uncovered scopes: ${uncovered.join(', ')}`);
});

test('shiki: a native declaration colors every word of it', async () => {
  // The shape PHIR emits for a runtime declaration, reported as unhighlighted:
  // `native`/`fun` as keywords, the qualified name, its parameters, the types.
  const declaration = 'native fun Syscalls.read(fd: Int, buf: Long) -> Long';
  const tokens = await tokenizePHIR(declaration, { theme: 'light' });
  for (const word of ['native', 'fun', 'Syscalls.read', 'fd', 'buf', 'Int', 'Long']) {
    assert.ok(isColored(tokens, word), `${word} fell through to the default color`);
  }
});

test('shiki: opcode mnemonics are colored', async () => {
  // `support.instruction.phir` is a leaf scope on the IR opcodes; without a
  // theme rule for it they render as plain text.
  const code = '    %0 = add %1, %2\n    call println(%0)\n    ret %0';
  const tokens = await tokenizePHIR(code, { theme: 'light' });
  for (const word of ['add', 'call', 'ret']) {
    assert.ok(isColored(tokens, word), `opcode ${word} fell through to the default color`);
  }
});

test('shiki: tokenizes hello-world line by line', async () => {
  const tokens = await tokenizePHIR(HELLO, { theme: 'light' });
  assert.equal(tokens.length, HELLO.split('\n').length);
  assert.equal(usesVar(tokens, '--PHIR-comment'), true, 'comment');
  assert.equal(usesVar(tokens, '--PHIR-string'), true, 'string');
  assert.equal(usesVar(tokens, '--PHIR-storage'), true, 'fun is a storage type');
  assert.equal(usesVar(tokens, '--PHIR-function'), true, 'function names');
});

test('shiki: dark mode selects the dark palette', async () => {
  const light = await tokenizePHIR(HELLO, { theme: 'light' });
  const dark = await tokenizePHIR(HELLO, { theme: 'dark' });
  assert.ok(light.length === dark.length);
  const lightColors = [...colorsOf(light)];
  const darkColors = [...colorsOf(dark)];
  assert.ok(lightColors.length > 0 && darkColors.length > 0);
  assert.notDeepEqual(lightColors.sort(), darkColors.sort());
});

test('shiki: real fixture file round-trips', async () => {
  const code = await readFile(FIXTURE, 'utf8');
  const tokens = await tokenizePHIR(code, { theme: 'light' });
  assert.equal(tokens.length, code.split('\n').length);
  const rebuilt = tokens.map(lineText).join('\n');
  assert.equal(rebuilt, code);
});

/**
 * An IR dump as the compiler emits it: a textual header followed by a binary
 * payload. The host's paged text read refuses any page containing a NUL byte,
 * which is why the renderer asks for complete bytes instead.
 */
const IR_HEADER = `# module Main target x86_64
# source tests/photon/simple.aura

native fun print(value: Any) -> Unit

fun main() {
    print("hello")
}
`;
const IR_BINARY_TAIL = [0x00, 0xff, 0xfe, 0x01, 0x0a, 0x00, 0x00, 0x7f];

test('decode: an IR dump with a binary payload still becomes source text', () => {
  const bytes = Uint8Array.from([
    ...Buffer.from(IR_HEADER, 'utf8'),
    ...IR_BINARY_TAIL,
  ]);
  const text = decodePHIRSource(bytes);

  assert.ok(text.startsWith(IR_HEADER), 'the textual header survives intact');
  assert.equal(text.includes('\u0000'), false, 'no NUL byte survives');
  assert.ok(text.includes('\uFFFD'), 'malformed UTF-8 degrades to U+FFFD');
});

test('decode: malformed UTF-8 never throws', () => {
  assert.equal(decodePHIRSource(Uint8Array.from([0xff, 0x41])), '\uFFFDA');
  assert.equal(decodePHIRSource(new Uint8Array(0)), '');
});

test('decode: the decoded dump tokenizes like any other source', async () => {
  const bytes = Uint8Array.from([
    ...Buffer.from(IR_HEADER, 'utf8'),
    ...IR_BINARY_TAIL,
  ]);
  const text = decodePHIRSource(bytes);
  const tokens = await tokenizePHIR(text, { theme: 'light' });
  assert.equal(tokens.length, text.split('\n').length);
  assert.equal(tokens.map(lineText).join('\n'), text);
});

test('shiki: getHighlighter memoizes one instance', async () => {
  resetHighlighter();
  const first = await getHighlighter();
  const second = await getHighlighter();
  assert.equal(first, second);
});

test('incremental: chunked tokenization matches whole-document tokenization', async () => {
  const code = SAMPLE;
  const whole = await tokenizePHIR(code, { theme: 'light' });
  assert.equal(whole.map(lineText).join('\n'), code);

  const highlighter = await getHighlighter();
  const state = createIncrementalState();
  const step = Math.max(16, Math.floor(code.length / 7));
  let result;
  let frames = 0;
  do {
    result = tokenizeNextChunk(
      highlighter,
      code,
      state,
      { chars: step, lines: 5000 },
      THEME_BY_MODE.light,
    );
    frames += 1;
  } while (!result.complete && frames < 1000);
  assert.equal(result.complete, true, 'finished within the frame cap');
  assert.ok(frames > 1, `expected multiple frames, took ${frames}`);
  assert.equal(
    result.lines.map(lineText).join('\n'),
    code,
    'chunked text matches',
  );
  assert.equal(result.lines.length, whole.length, 'chunked line count matches');
});

test('incremental: a single budget-limited call leaves the tail, so callers must loop', async () => {
  // Regression guard for the preview's truncation bug. `tokenizeNextChunk`
  // honours its character budget, so one call on a large document returns
  // `complete: false` with only the tokenized prefix. The preview must drive it
  // in a loop until `complete`, and must render the untouched tail from the raw
  // source rather than from the token array.
  const unit = 'fun main() { println("Hello, PHIR!") }\n';
  const code = unit.repeat(Math.ceil((32 * 1024 * 3) / unit.length));
  assert.ok(code.length > 32 * 1024 * 3, `fixture is ${code.length} chars`);

  const highlighter = await getHighlighter();
  const theme = THEME_BY_MODE.light;
  const budget = { chars: 32 * 1024, lines: 4000 };

  const oneShot = tokenizeNextChunk(
    highlighter,
    code,
    createIncrementalState(),
    budget,
    theme,
  );
  assert.equal(
    oneShot.complete,
    false,
    'one call does not cover a document larger than the budget',
  );
  assert.ok(oneShot.length < code.length, 'the un-tokenized tail exists');

  const state = createIncrementalState();
  let result;
  let frames = 0;
  do {
    result = tokenizeNextChunk(highlighter, code, state, budget, theme);
    frames += 1;
  } while (!result.complete && frames < 2000);

  assert.equal(result.complete, true, 'the loop finishes');
  assert.ok(frames > 1, `expected multiple frames, took ${frames}`);
  assert.equal(result.length, code.length, 'the whole document is consumed');
  assert.equal(
    result.lines.map(lineText).join('\n'),
    code,
    'looped output is the full source',
  );
});

test('incremental: the rewrite fingerprint never overruns the cursor', async () => {
  // The fingerprint must stay a suffix of the *consumed* prefix. If a slice
  // overruns the cursor, the next frame's origin lands negative,
  // `isAppendOnly` reports a rewrite, and the state resets to zero every frame:
  // the file stops scrolling past the first chunk and the bottom of the preview
  // is cut off at exactly the first budget's worth of lines.
  const code = 'fun main() { println("Hello, PHIR!") }\n'.repeat(1200);
  const highlighter = await getHighlighter();
  const state = createIncrementalState();
  let result;
  let frames = 0;
  do {
    result = tokenizeNextChunk(
      highlighter,
      code,
      state,
      { chars: 32 * 1024, lines: 4000 },
      THEME_BY_MODE.light,
    );
    frames += 1;
    assert.ok(
      state.tail.length <= 4096,
      `fingerprint grew to ${state.tail.length} on frame ${frames}`,
    );
    assert.equal(
      code.startsWith(state.tail, state.length - state.tail.length),
      true,
      `frame ${frames}: fingerprint is a suffix of the consumed prefix`,
    );
    assert.ok(
      result.length >= state.length,
      `frame ${frames}: the cursor did not move backwards`,
    );
  } while (!result.complete && frames < 2000);
  assert.equal(result.complete, true, `finished in ${frames} frames`);
  assert.ok(frames > 1, 'advanced past the first chunk');
});

test('incremental: the line budget caps a chunk at the requested lines', async () => {
  const code = 'line 0\nline 1\nline 2\nline 3\nline 4\nline 5\n';
  const highlighter = await getHighlighter();
  const state = createIncrementalState();

  // The character budget covers the whole document, so only the line budget can
  // stop a chunk. It used to search backwards from the end of the file and hand
  // back an offset near it, so one call swallowed almost everything.
  const first = tokenizeNextChunk(
    highlighter,
    code,
    state,
    { chars: 10_000, lines: 2 },
    THEME_BY_MODE.light,
  );
  assert.equal(first.complete, false);
  assert.equal(first.lines.map(lineText).join('\n'), 'line 0\nline 1');
  assert.equal(state.length, 'line 0\nline 1\n'.length);

  let result = first;
  let frames = 1;
  while (!result.complete && frames < 20) {
    result = tokenizeNextChunk(
      highlighter,
      code,
      state,
      { chars: 10_000, lines: 2 },
      THEME_BY_MODE.light,
    );
    frames += 1;
  }
  assert.equal(result.complete, true, `finished in ${frames} frames`);
  assert.equal(frames, 3, 'six lines at two per frame');
  const whole = await tokenizePHIR(code, { theme: 'light' });
  assert.equal(result.lines.map(lineText).join('\n'), whole.map(lineText).join('\n'));
});

test('incremental: grammar state carries a multi-line declaration across chunks', async () => {
  // `fun name` opens a declaration that the grammar only closes on a line that
  // is exactly `}`. A chunk boundary inside the body is the case the carry has
  // to survive: the second call never sees the `fun` that opened the scope.
  const code = 'fun main() {\n    val x = 1\n}\n';

  const highlighter = await getHighlighter();
  const state = createIncrementalState();
  // A character budget, not a line budget: the chunk grows to the next newline,
  // so the split lands on the boundary that opens the declaration body.
  const first = tokenizeNextChunk(
    highlighter,
    code,
    state,
    { chars: 8 },
    THEME_BY_MODE.light,
  );
  assert.equal(first.complete, false, 'the first chunk stops inside the declaration');
  assert.equal(state.length, code.indexOf('    val'), 'the cursor stopped after the opening line');

  const second = tokenizeNextChunk(
    highlighter,
    code,
    state,
    { chars: 4096 },
    THEME_BY_MODE.light,
  );
  assert.equal(second.complete, true);

  const whole = await tokenizePHIR(code, { theme: 'light' });
  assert.equal(
    second.lines.map(lineText).join('\n'),
    whole.map(lineText).join('\n'),
    'chunked tokenization matches whole-document tokenization',
  );
});

test('incremental: rewriting the prefix resets the state', async () => {
  const highlighter = await getHighlighter();
  const state = createIncrementalState();
  tokenizeNextChunk(highlighter, 'fun a() {}\n', state, {}, THEME_BY_MODE.light);
  assert.equal(state.length, 'fun a() {}\n'.length);
  const result = tokenizeNextChunk(
    highlighter,
    'completely different source\n',
    state,
    {},
    THEME_BY_MODE.light,
  );
  assert.equal(result.complete, true);
  assert.equal(
    result.lines.map(lineText).join('\n'),
    'completely different source\n',
    're-tokenized from scratch after the rewrite',
  );
});

test('incremental: shrinking the input resets the state', async () => {
  const highlighter = await getHighlighter();
  const state = createIncrementalState();
  tokenizeNextChunk(highlighter, 'a much longer document follows\n', state, {}, THEME_BY_MODE.light);
  const result = tokenizeNextChunk(highlighter, 'short', state, {}, THEME_BY_MODE.light);
  assert.equal(result.complete, true);
  assert.equal(result.lines.map(lineText).join('\n'), 'short');
});


