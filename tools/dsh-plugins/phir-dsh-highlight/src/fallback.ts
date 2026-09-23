/**
 * Degraded tokenizer: colors PHIR source without Shiki at all.
 *
 * Used only when the Shiki highlighter cannot be built - a host that blocks
 * WebAssembly and refuses the pure-JS engine, a grammar that fails to load, or
 * a future Shiki version with a breaking constructor change. It is a real
 * tokenizer, not a set of loose regexes: it carries comment and string state
 * across lines, honors escape sequences, and emits the same `ThemedToken[][]`
 * shape Shiki produces, so the renderer cannot tell the two apart.
 *
 * Coverage is deliberately partial - comments, strings, numbers, keywords,
 * literals, and punctuation. Types, function calls, and variables render in
 * the default foreground. That is still far better than the plain `<pre>` a
 * plugin failure would otherwise leave behind.
 *
 * Comment syntax mirrors the grammar, not Aura: PHIR marks a comment with `#`
 * (`comment.line.number-sign.phir`) and defines no block or doc comment. A
 * degraded tokenizer that colored slash-slash or slash-star comments would
 * disagree with the real one on every file, and would leave the `# module ...`
 * header - the first line of every dump - uncolored.
 *
 * This module must stay free of Shiki runtime imports: it is the last resort.
 */

import { colorRef, type Palette } from './theme.js';

/** Tokens this tokenizer colors. Empty string means "default foreground". */
type Kind =
  | ''
  | 'comment'
  | 'string'
  | 'number'
  | 'keyword'
  | 'literal'
  | 'punctuation';

/** Lexical mode carried across lines. */
type Mode = 'code' | 'line-comment' | 'string';

interface TokenShape {
  readonly content: string;
  readonly offset: number;
  readonly color?: string;
}

const KEYWORDS = new Set([
  'abstract', 'actor', 'annotation', 'as', 'as?', 'await', 'by', 'catch',
  'channel', 'class', 'companion', 'comptime', 'const', 'constructor',
  'continue', 'data', 'defer', 'delegate', 'delegated', 'do', 'else',
  'enum', 'external', 'export', 'final', 'finally', 'for', 'future', 'fun',
  'get', 'if', 'in', 'infix', 'init', 'inline', 'inner', 'inout', 'internal',
  'interface', 'is', 'isnull', 'it', 'lazy', 'macro', 'macroable', 'mutable',
  'noinline', 'nonlocal', 'nonnull', 'object', 'operator', 'out', 'override',
  'package', 'private', 'protected', 'public', 'reified', 'receive', 'return',
  'scope', 'sealed', 'select', 'send', 'set', 'static', 'struct', 'super',
  'suspend', 'this', 'throw', 'try', 'typealias', 'typeof', 'var', 'vararg',
  'val', 'when', 'where', 'while', 'yield',
]);

const LITERALS = new Set(['true', 'false', 'null', 'undefined', 'NaN', 'Infinity']);

const HEX = /[0-9a-fA-F_]/;
const BIN = /[01_]/;
const DEC = /[\d_]/;
const DIGIT = /\d/;
const WORD = /[\w$]/;
const IDENT_START = /[A-Za-z_$]/;

/**
 * Tokenize PHIR source without Shiki.
 *
 * @param code - full source text.
 * @param palette - colors to emit.
 * @returns one array per line; empty lines are `[]`.
 */
export function tokenizeFallback(
  code: string,
  palette: Palette,
): TokenShape[][] {
  const lines: TokenShape[][] = [];
  const newlineCount = countNewlines(code);
  for (let i = 0; i <= newlineCount; i += 1) lines.push([]);

  let mode: Mode = 'code';
  let quote = '';
  let kind: Kind = '';
  let buf = '';
  let start = 0;
  let line = 0;
  let i = 0;

  const flush = (): void => {
    if (buf === '') return;
    const color = colorFor(kind, palette);
    const token: TokenShape =
      color === undefined
        ? { content: buf, offset: start }
        : { content: buf, offset: start, color };
    const target = lines[line];
    if (target !== undefined) target.push(token);
    buf = '';
    kind = '';
    start = 0;
  };

  while (i < code.length) {
    const ch = at(code, i);

    if (ch === '\n') {
      flush();
      if (mode === 'line-comment' || (mode === 'string' && quote.length === 1)) {
        mode = 'code';
        quote = '';
      }
      line += 1;
      kind = '';
      i += 1;
      continue;
    }

    if (mode !== 'code') {
      if (mode === 'string') {
        if (quote.length === 3 && code.startsWith(quote, i)) {
          const closing = quote;
          buf += closing;
          flush();
          mode = 'code';
          quote = '';
          kind = '';
          i += closing.length;
          continue;
        }
        if (
          quote.length === 1 &&
          ch === at(quote, 0) &&
          at(code, i - 1) !== '\\'
        ) {
          buf += ch;
          flush();
          mode = 'code';
          quote = '';
          kind = '';
          i += 1;
          continue;
        }
      }
      buf += ch;
      i += 1;
      continue;
    }

    // mode === 'code'
    if (ch === '#') {
      flush();
      kind = 'comment';
      start = i;
      buf = '#';
      mode = 'line-comment';
      i += 1;
      continue;
    }
    const q = readQuote(code, i);
    if (q !== '') {
      flush();
      kind = 'string';
      start = i;
      buf = q;
      mode = 'string';
      quote = q;
      i += q.length;
      continue;
    }
    if (DIGIT.test(ch)) {
      flush();
      kind = 'number';
      start = i;
      i = readNumber(code, i);
      buf = code.slice(start, i);
      flush();
      kind = '';
      continue;
    }
    if (IDENT_START.test(ch)) {
      const wordEnd = readWord(code, i);
      const text = code.slice(i, wordEnd);
      const wordKind = KEYWORDS.has(text)
        ? 'keyword'
        : LITERALS.has(text)
          ? 'literal'
          : '';
      if (wordKind !== '') {
        flush();
        kind = wordKind;
        start = i;
        buf = text;
        flush();
        kind = '';
      } else {
        // An uncolored word must not glue onto an open punctuation run, or
        // `a = b` becomes one token instead of three.
        if (kind !== '') flush();
        buf += text;
      }
      i = wordEnd;
      continue;
    }
    if (ch === ' ' || ch === '\t' || ch === '\r') {
      buf += ch;
      i += 1;
      continue;
    }
    // Punctuation. Consecutive punctuation accumulates into one token (`==`),
    // but a punctuation run always starts fresh so an operator is never glued
    // onto the word or whitespace before it.
    if (kind === 'punctuation') {
      buf += ch;
      i += 1;
      continue;
    }
    flush();
    kind = 'punctuation';
    start = i;
    buf = ch;
    i += 1;
    continue;
  }

  flush();
  return lines;
}

/** Read an opening quote sequence: triple quotes win over single. */
function readQuote(code: string, i: number): string {
  if (code.startsWith('"""', i)) return '"""';
  if (code.startsWith("'''", i)) return "'''";
  const ch = at(code, i);
  if (ch === '"' || ch === "'" || ch === '`') return ch;
  return '';
}

/** Read a numeric literal starting at `i`. Returns the end offset. */
function readNumber(code: string, i: number): number {
  let j = i;
  if (at(code, j) === '0' && (at(code, j + 1) === 'x' || at(code, j + 1) === 'X')) {
    j += 2;
    while (j < code.length && HEX.test(at(code, j))) j += 1;
  } else if (at(code, j) === '0' && (at(code, j + 1) === 'b' || at(code, j + 1) === 'B')) {
    j += 2;
    while (j < code.length && BIN.test(at(code, j))) j += 1;
  } else {
    while (j < code.length && DEC.test(at(code, j))) j += 1;
    if (at(code, j) === '.') {
      j += 1;
      while (j < code.length && DEC.test(at(code, j))) j += 1;
    }
    if (at(code, j) === 'e' || at(code, j) === 'E') {
      j += 1;
      if (at(code, j) === '+' || at(code, j) === '-') j += 1;
      while (j < code.length && DIGIT.test(at(code, j))) j += 1;
    }
    if (
      at(code, j) === 'f' ||
      at(code, j) === 'F' ||
      at(code, j) === 'l' ||
      at(code, j) === 'L' ||
      at(code, j) === 'd' ||
      at(code, j) === 'D' ||
      at(code, j) === 'm' ||
      at(code, j) === 'M' ||
      at(code, j) === 'u'
    ) {
      j += 1;
    }
  }
  return j;
}

/** Read an identifier starting at `i`. Returns the end offset. */
function readWord(code: string, i: number): number {
  let j = i;
  while (j < code.length && WORD.test(at(code, j))) j += 1;
  return j;
}

/**
 * Read one character, returning the empty string past the end.
 *
 * `charAt` rather than `code[i]` because indexed access is `string | undefined`
 * under `noUncheckedIndexedAccess` and the tokenizer's hot loop does not want a
 * null check on every character.
 */
function at(code: string, i: number): string {
  return i < code.length ? code.charAt(i) : '';
}

function colorFor(kind: Kind, palette: Palette): string | undefined {
  switch (kind) {
    case 'comment':
      return colorRef('comment', palette);
    case 'string':
      return colorRef('string', palette);
    case 'number':
      return colorRef('number', palette);
    case 'keyword':
      return colorRef('keyword', palette);
    case 'literal':
      return colorRef('boolean', palette);
    case 'punctuation':
      return colorRef('punctuation', palette);
    default:
      return undefined;
  }
}

function countNewlines(code: string): number {
  let count = 0;
  for (let i = 0; i < code.length; i += 1) {
    if (code.charCodeAt(i) === 10) count += 1;
  }
  return count;
}


