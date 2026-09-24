/**
 * Degraded tokenizer: colors HAT source without Shiki at all.
 *
 * Used only when the Shiki highlighter cannot be built - a host that blocks
 * WebAssembly and refuses the pure-JS engine, a grammar that fails to load, or
 * a future Shiki version with a breaking constructor change. It is a real
 * tokenizer, not a set of loose regexes: it carries comment and string state
 * across lines, honors escape sequences, and emits the same `ThemedToken[][]`
 * shape Shiki produces, so the renderer cannot tell the two apart.
 *
 * Coverage is deliberately partial - strings, numbers, keywords,
 * literals, and punctuation. Types, function calls, and variables render in
 * the default foreground. That is still far better than the plain `<pre>` a
 * plugin failure would otherwise leave behind.
 *
 * The `;` prefix in HAT is a format marker, not a comment character: the new
 * grammar design (hat-highlight-scheme.md) removed the comments wildcard rule
 * entirely. This tokenizer treats `;` as regular punctuation.
 *
 * This module must stay free of Shiki runtime imports: it is the last resort.
 * </p>
 */

import { colorRef, type Palette } from './theme.js';

/** Tokens this tokenizer colors. Empty string means "default foreground". */
type Kind =
  | ''
  | 'string'
  | 'number'
  | 'keyword'
  | 'literal'
  | 'punctuation';

/** Lexical mode carried across lines. */
type Mode = 'code' | 'string';

interface TokenShape {
  readonly content: string;
  readonly offset: number;
  readonly color?: string;
}

const KEYWORDS = new Set([
  // Type keywords
  'Int', 'Float', 'Bool', 'Unit', 'String', 'Any', 'Char',
  // Composite types
  '!stackslot', '!list', '!map', '!ptr',
  // Module
  'module', 'target', 'schema', 'source',
  // Function
  'fn', 'extern',
  // Struct / Enum
  '@struct', '@enum',
  // Basic block
  'bb',
  // Constants
  '@i32_const', '@f64_const', '@const_str', '@bool_const', '@null',
  // Phi
  '@phi',
  // Arithmetic
  '@add', '@sub', '@mul', '@div', '@rem', '@neg',
  // Comparison (icmp / fcmp + sub-operators)
  '@icmp', '@fcmp',
  '@i32_slt', '@i32_sle', '@i32_sgt', '@i32_sge', '@i32_eq', '@i32_ne',
  '@i32_ult', '@i32_ule', '@i32_ugt', '@i32_uge',
  '@f64_lt', '@f64_le', '@f64_gt', '@f64_ge', '@f64_eq', '@f64_ne',
  // Logical
  '@and', '@or', '@xor', '@not',
  // Bitwise
  '@band', '@bor', '@bxor', '@shl', '@shr', '@ushr',
  // Memory
  '@alloc', '@store', '@load', '@alloc_list', '@list_set', '@list_get',
  '@list_len', '@alloc_map', '@map_set', '@map_get',
  // String
  '@str_concat', '@str_len', '@str_sub', '@str_char',
  // Object
  '@new', '@field', '@field_set',
  // Call
  '@call',
  // Conversion
  '@i32_to_f64', '@f64_to_i32', '@i32_to_str', '@f64_to_str', '@str_to_i32',
  // Terminators
  '@br', '@br_if', '@ret',
  // Span annotation
  '@span',
]);

const LITERALS = new Set(['true', 'false', 'null', 'undefined', 'NaN', 'Infinity']);

const HEX = /[0-9a-fA-F_]/;
const BIN = /[01_]/;
const DEC = /[\d_]/;
const DIGIT = /\d/;
const WORD = /[\w$@]/;
const IDENT_START = /[A-Za-z_$@]/;

/**
 * Tokenize HAT source without Shiki.
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
      if (mode === 'string' && quote.length === 1) {
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
 */
function at(code: string, i: number): string {
  return i < code.length ? code.charAt(i) : '';
}

function colorFor(kind: Kind, palette: Palette): string | undefined {
  switch (kind) {
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
