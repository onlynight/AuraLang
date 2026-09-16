/**
 * Scope snapshot tests for the Aura highlighter.
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
  AURA_DARK_PALETTE,
  AURA_DARK_THEME,
  AURA_LIGHT_PALETTE,
  AURA_LIGHT_THEME,
  THEME_BY_MODE,
  createIncrementalState,
  getHighlighter,
  resetHighlighter,
  themeVariableCss,
  tokenizeAura,
  tokenizeFallback,
  tokenizeNextChunk,
  isAuraPath,
  AURA_GRAMMAR,
} from '../lib/highlight.js';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '..');

const HELLO = `// a line comment
fun main() {
    println("Hello, Aura!")
}`;

const SAMPLE = `// basics/mini_struct.aura
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

test('isAuraPath accepts .aura and rejects look-alikes', () => {
  assert.equal(isAuraPath('hello.aura'), true);
  assert.equal(isAuraPath('dir/nested/hello.aura'), true);
  assert.equal(isAuraPath('dir\\nested\\hello.aura'), true);
  assert.equal(isAuraPath('HELLO.AURA'), true);
  assert.equal(isAuraPath('aura'), false);
  assert.equal(isAuraPath(''), false);
  assert.equal(isAuraPath(undefined), false);
  assert.equal(isAuraPath('aura.ts'), false);
  assert.equal(isAuraPath('.aura'), false);
  assert.equal(isAuraPath('name.'), false);
  assert.equal(isAuraPath('dir/'), false);
  assert.equal(isAuraPath('a.aura.bak'), false);
});

test('grammar is intact and carries the VSCode identity', () => {
  assert.equal(AURA_GRAMMAR.scopeName, 'source.aura');
  assert.equal(AURA_GRAMMAR.name, 'aura');
  assert.equal(AURA_GRAMMAR.displayName, 'Aura');
  assert.ok(AURA_GRAMMAR.patterns.length > 0, 'root patterns');
  const repo = AURA_GRAMMAR.repository;
  assert.ok(repo && typeof repo === 'object', 'repository');
  const keys = Object.keys(repo);
  assert.ok(keys.length >= 50, `repository rules (${keys.length})`);
  for (const key of ['package-declaration', 'comments', 'keywords', 'function-declaration']) {
    assert.ok(keys.includes(key), `repository has #${key}`);
  }
});

test('fallback: line count matches the input', () => {
  const code = 'a\nb\n\nc\n';
  assert.equal(tokenizeFallback(code, AURA_LIGHT_PALETTE).length, code.split('\n').length);
  assert.equal(tokenizeFallback('', AURA_LIGHT_PALETTE).length, 1);
  assert.equal(tokenizeFallback('x', AURA_LIGHT_PALETTE).length, 1);
});

test('fallback: colors comments, strings, numbers, and keywords', () => {
  const code = [
    '// note',
    '/* block */',
    'val s = "text"',
    'val n = 42',
    'if true else false',
  ].join('\n');
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-comment'), true, 'comment');
  assert.equal(usesVar(tokens, '--aura-string'), true, 'string');
  assert.equal(usesVar(tokens, '--aura-number'), true, 'number');
  assert.equal(usesVar(tokens, '--aura-keyword'), true, 'keyword');
  assert.equal(usesVar(tokens, '--aura-boolean'), true, 'boolean');
});

test('fallback: text round-trips exactly', () => {
  const code = '// x\nfun f(a: Int): Int { return a }\n/* y */\n';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: block comment spans lines', () => {
  const code = '/* one\n   two\n*/ after';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-comment'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: triple-quoted string spans lines', () => {
  const code = 'val s = """line one\nline two"""\n';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-string'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: escaped quote does not close the string', () => {
  const code = 'val s = "say \\"hi\\""\n';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-string'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: numbers with prefixes, exponents, and suffixes', () => {
  const code = 'val a = 0x1F\nval b = 0b1010\nval c = 1.5e3\nval d = 1_000L\n';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-number'), true);
  assert.equal(tokens.map(lineText).join('\n'), code);
});

test('fallback: punctuation is its own token, not glued to identifiers', () => {
  const code = 'val a = b + c\n';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-punctuation'), true);
  assert.equal(usesVar(tokens, '--aura-keyword'), true);
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

test('fallback: doc comment is colored as a comment', () => {
  const code = '/** docs */\n';
  const tokens = tokenizeFallback(code, AURA_LIGHT_PALETTE);
  assert.equal(usesVar(tokens, '--aura-comment'), true);
});

test('fallback: an empty document yields one empty line', () => {
  const tokens = tokenizeFallback('', AURA_LIGHT_PALETTE);
  assert.equal(tokens.length, 1);
  assert.equal(tokens[0].length, 0);
});

test('theme: both palettes define every shared color key', () => {
  const lightKeys = Object.keys(AURA_LIGHT_PALETTE).sort();
  const darkKeys = Object.keys(AURA_DARK_PALETTE).sort();
  assert.deepEqual(darkKeys, lightKeys);
  assert.ok(lightKeys.length >= 24, `palette has ${lightKeys.length} keys`);
});

test('theme: token colors are CSS variable references with hex fallbacks', () => {
  const colors = AURA_LIGHT_THEME.tokenColors ?? [];
  assert.ok(colors.length >= 40, `tokenColors (${colors.length})`);
  const foregrounds = colors
    .map((entry) => entry.settings?.foreground)
    .filter((value) => typeof value === 'string');
  assert.ok(foregrounds.length > 0, 'at least one foreground');
  for (const color of foregrounds) {
    assert.match(
      color,
      /^var\(--aura-[a-z0-9-]+, #[0-9a-f]{6}\)$/i,
      `unexpected color form: ${color}`,
    );
  }
});

test('theme: light and dark resolve the same scope table', () => {
  const lightScopes = AURA_LIGHT_THEME.tokenColors.map((entry) => JSON.stringify(entry.scope));
  const darkScopes = AURA_DARK_THEME.tokenColors.map((entry) => JSON.stringify(entry.scope));
  assert.deepEqual(darkScopes, lightScopes);
});

test('theme: THEME_BY_MODE maps onto the registered theme names', () => {
  assert.equal(THEME_BY_MODE.light, AURA_LIGHT_THEME.name);
  assert.equal(THEME_BY_MODE.dark, AURA_DARK_THEME.name);
});

test('themeVariableCss: installs light and dark blocks with every key', () => {
  const css = themeVariableCss();
  assert.ok(css.includes(':root{'), 'light block');
  assert.ok(css.includes('body[data-ds-dark-theme]{'), 'dark block');
  for (const key of Object.keys(AURA_LIGHT_PALETTE)) {
    assert.ok(css.includes(`--aura-${key}:`), `emits --aura-${key}`);
  }
});

test('shiki: tokenizes hello-world line by line', async () => {
  const tokens = await tokenizeAura(HELLO, { theme: 'light' });
  assert.equal(tokens.length, HELLO.split('\n').length);
  assert.equal(usesVar(tokens, '--aura-comment'), true, 'comment');
  assert.equal(usesVar(tokens, '--aura-string'), true, 'string');
  assert.equal(usesVar(tokens, '--aura-storage'), true, 'fun is a storage type');
  assert.equal(usesVar(tokens, '--aura-function'), true, 'function names');
});

test('shiki: dark mode selects the dark palette', async () => {
  const light = await tokenizeAura(HELLO, { theme: 'light' });
  const dark = await tokenizeAura(HELLO, { theme: 'dark' });
  assert.ok(light.length === dark.length);
  const lightColors = [...colorsOf(light)];
  const darkColors = [...colorsOf(dark)];
  assert.ok(lightColors.length > 0 && darkColors.length > 0);
  assert.notDeepEqual(lightColors.sort(), darkColors.sort());
});

test('shiki: real fixture file round-trips', async () => {
  const path = resolve(root, '..', '..', '..', 'examples', 'basics', 'mini_struct.aura');
  const code = await readFile(path, 'utf8');
  const tokens = await tokenizeAura(code, { theme: 'light' });
  assert.equal(tokens.length, code.split('\n').length);
  const rebuilt = tokens.map(lineText).join('\n');
  assert.equal(rebuilt, code);
});

test('shiki: getHighlighter memoizes one instance', async () => {
  resetHighlighter();
  const first = await getHighlighter();
  const second = await getHighlighter();
  assert.equal(first, second);
});

test('incremental: chunked tokenization matches whole-document tokenization', async () => {
  const code = SAMPLE;
  const whole = await tokenizeAura(code, { theme: 'light' });
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
  const unit = 'fun main() { println("Hello, Aura!") }\n';
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
  const code = 'fun main() { println("Hello, Aura!") }\n'.repeat(1200);
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

test('incremental: grammar state carries a block comment across chunks', async () => {
  const code = '/* opened\nnever closed here\nfun main() {}\n';
  const highlighter = await getHighlighter();
  const state = createIncrementalState();
  const first = tokenizeNextChunk(
    highlighter,
    code.slice(0, 10),
    state,
    { chars: 10, lines: 50 },
    THEME_BY_MODE.light,
  );
  const second = tokenizeNextChunk(
    highlighter,
    code,
    state,
    { chars: 4096, lines: 50 },
    THEME_BY_MODE.light,
  );
  assert.equal(second.complete, true);
  const colors = [...colorsOf(second.lines)];
  assert.ok(
    colors.some((color) => color.includes('--aura-comment')),
    `comment survives the chunk boundary (${colors.join(' | ')})`,
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
