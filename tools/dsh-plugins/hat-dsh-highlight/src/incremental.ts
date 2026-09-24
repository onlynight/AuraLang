/**
 * Incremental tokenization for the document preview.
 *
 * The source arrives whole (the renderer asks for complete bytes; see
 * decode.ts), but re-tokenizing all of it in one frame would block the pane.
 * Instead this module carries Shiki's `grammarState` from one budgeted call to
 * the next and only tokenizes the text the previous call did not reach.
 *
 * The same carry-forward makes a growing prefix cheap: whatever a caller hands
 * over that merely extends the consumed text is tokenized from the frontier.
 *
 * Grammar state is not serializable - `GrammarState` holds live
 * `StateStack` getters - so all of this runs on the main thread. That is a
 * deliberate trade: document preview is interactive, not a build pipeline.
 *
 * Rewrite detection is bounded: only a tail fingerprint of the consumed text is
 * retained, so appending to a multi-megabyte file stays O(fingerprint).
 */

import type { GrammarState, ThemedToken } from '@shikijs/core';

/** Default per-frame character budget: ~16 KB of source. */
export const DEFAULT_CHARS = 16 * 1024;

/** Default per-frame line budget. */
export const DEFAULT_LINES = 2000;

/** Consumed-text tail retained for rewrite detection. */
const FINGERPRINT_CHARS = 4096;

/** Mutable per-document tokenization state. */
export interface IncrementalState {
  /** Tokenized lines, in source order. */
  lines: ThemedToken[][];
  /** Grammar state at the end of the consumed text. */
  grammar: GrammarState | undefined;
  /** Characters of `code` already consumed. */
  length: number;
  /** Tail of the consumed text, used to detect rewrites. */
  tail: string;
}

/** Per-frame budget. Either may be omitted. */
export interface ChunkBudget {
  /** Maximum characters to tokenize this call. Default {@link DEFAULT_CHARS}. */
  chars?: number;
  /** Maximum lines to tokenize this call. Default {@link DEFAULT_LINES}. */
  lines?: number;
}

/** One frame's result. */
export interface ChunkResult {
  /** All tokenized lines so far. */
  lines: ThemedToken[][];
  /** Grammar state at the end of the consumed text. */
  grammar: GrammarState | undefined;
  /** Characters consumed so far. */
  length: number;
  /** `true` when the whole input has been tokenized. */
  complete: boolean;
}

/** Create an empty state. */
export function createIncrementalState(): IncrementalState {
  return { lines: [], grammar: undefined, length: 0, tail: '' };
}

/** Forget everything and start over. Call when the document is replaced. */
export function resetIncrementalState(state: IncrementalState): void {
  state.lines.length = 0;
  state.grammar = undefined;
  state.length = 0;
  state.tail = '';
}

/** Decide whether `code` still extends the consumed prefix. */
function isAppendOnly(code: string, state: IncrementalState): boolean {
  if (code.length < state.length) return false;
  if (state.tail === '') return true;
  const from = state.length - state.tail.length;
  if (from < 0) return false;
  return code.startsWith(state.tail, from);
}

/**
 * Tokenize the next frame of `code` into `state`.
 */
export function tokenizeNextChunk(
  highlighter: {
    codeToTokens: (
      code: string,
      options: {
        lang: string;
        theme: string;
        grammarState?: GrammarState;
      },
    ) => { tokens: ThemedToken[][]; grammarState?: GrammarState };
    /** Derive the terminal grammar state from a finished token array. */
    getLastGrammarState: (tokens: ThemedToken[][]) => GrammarState | undefined;
  },
  code: string,
  state: IncrementalState,
  budget: ChunkBudget = {},
  theme: string,
  lang: string = 'HAT',
): ChunkResult {
  if (!isAppendOnly(code, state)) resetIncrementalState(state);

  const remaining = code.length - state.length;
  if (remaining <= 0) {
    return finish(state, code);
  }

  const maxChars = budget.chars ?? DEFAULT_CHARS;
  const maxLines = budget.lines ?? DEFAULT_LINES;

  let end = Math.min(code.length, state.length + maxChars);
  if (end < code.length) {
    const newline = code.indexOf('\n', end);
    if (newline !== -1) end = newline + 1;
  }

  const chunkLines = countLines(code, state.length, end);
  if (chunkLines > maxLines) {
    end = lineBudgetEnd(code, state.length, maxLines);
  }

  if (end <= state.length) {
    const newline = code.indexOf('\n', state.length);
    end = newline === -1 ? code.length : newline + 1;
  }

  const chunk = code.slice(state.length, end);
  const result = highlighter.codeToTokens(chunk, {
    lang,
    theme,
    ...(state.grammar !== undefined ? { grammarState: state.grammar } : {}),
  });

  const tokens = result.tokens;
  const last = tokens[tokens.length - 1];
  if (end < code.length && tokens.length > 0 && last !== undefined && last.length === 0) {
    tokens.pop();
  }

  if (tokens.length > 0) state.lines.push(...tokens);
  state.grammar =
    result.grammarState ?? highlighter.getLastGrammarState(result.tokens);
  state.length = end;
  state.tail = code.slice(Math.max(0, end - FINGERPRINT_CHARS), end);

  return finish(state, code);
}

/** Wrap the accumulated state into a result. */
function finish(
  state: IncrementalState,
  code: string,
): ChunkResult {
  return {
    lines: state.lines,
    grammar: state.grammar,
    length: state.length,
    complete: state.length >= code.length,
  };
}

/** Count lines in `code[from, to)`, counting a trailing partial line. */
function countLines(code: string, from: number, to: number): number {
  if (to <= from) return 0;
  let count = 1;
  for (let i = from; i < to; i += 1) {
    if (code.charCodeAt(i) === 10) count += 1;
  }
  return count;
}

/**
 * End offset just past the `maxLines`-th line break at or after `from`, or
 * `code.length` when fewer lines remain.
 */
function lineBudgetEnd(code: string, from: number, maxLines: number): number {
  let lines = 0;
  for (let i = from; i < code.length; i += 1) {
    if (code.charCodeAt(i) === 10) {
      lines += 1;
      if (lines >= maxLines) return i + 1;
    }
  }
  return code.length;
}
