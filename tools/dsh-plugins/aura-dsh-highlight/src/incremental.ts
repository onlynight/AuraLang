/**
 * Incremental tokenization for the streaming document-preview stream.
 *
 * DSH's document preview delivers the file as an accumulated text prefix
 * (see DocumentContent in dsh-client-ui-sidebar-documentpreview): each frame
 * the same prefix grows by one chunk. Re-tokenizing the whole document on every
 * frame is quadratic. Instead this module carries Shiki's `grammarState` across
 * frames and only tokenizes the newly appended tail.
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
  // The fingerprint is anchored to the cursor, so it is always a suffix of the
  // consumed prefix. A negative origin would mean the fingerprint overran the
  // cursor, which is a corrupt state: treat it as a rewrite rather than trying
  // to compare it, or every frame resets and the stream stalls forever.
  const from = state.length - state.tail.length;
  if (from < 0) return false;
  return code.startsWith(state.tail, from);
}

/**
 * Tokenize the next frame of `code` into `state`.
 *
 * @param highlighter - a ready Shiki highlighter.
 * @param code - the full accumulated source, not just the delta.
 * @param state - mutable state from the previous frame.
 * @param budget - how much to do this frame.
 * @param theme - theme name to color with.
 * @param lang - language id. Default `aura`.
 * @returns the frame's result; `state` is updated in place.
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
  lang: string = 'aura',
): ChunkResult {
  if (!isAppendOnly(code, state)) resetIncrementalState(state);

  const remaining = code.length - state.length;
  if (remaining <= 0) {
    return finish(state, code);
  }

  const maxChars = budget.chars ?? DEFAULT_CHARS;
  const maxLines = budget.lines ?? DEFAULT_LINES;

  // Grow to the next newline so we never stop mid-line when avoidable.
  let end = Math.min(code.length, state.length + maxChars);
  if (end < code.length) {
    const newline = code.indexOf('\n', end);
    if (newline !== -1) end = newline + 1;
  }

  // Enforce the line budget by backing up to an earlier newline.
  const chunkLines = countLines(code, state.length, end);
  if (chunkLines > maxLines) {
    end = backUpToLineBoundary(code, state.length, maxLines);
  }

  if (end <= state.length) {
    // A single line exceeds every budget; take it whole rather than stall.
    const newline = code.indexOf('\n', state.length);
    end = newline === -1 ? code.length : newline + 1;
  }

  const chunk = code.slice(state.length, end);
  const result = highlighter.codeToTokens(chunk, {
    lang,
    theme,
    ...(state.grammar !== undefined ? { grammarState: state.grammar } : {}),
  });

  // A chunk that ends in a newline yields a trailing empty line. That line does
  // not exist in the source yet - it is the seam with the next chunk - so drop
  // it here or every chunk boundary prints a spurious blank line. The final
  // chunk keeps it, so the last-line count matches `code.split('\n')`.
  const tokens = result.tokens;
  const last = tokens[tokens.length - 1];
  if (end < code.length && tokens.length > 0 && last !== undefined && last.length === 0) {
    tokens.pop();
  }

  if (tokens.length > 0) state.lines.push(...tokens);
  state.grammar =
    result.grammarState ?? highlighter.getLastGrammarState(result.tokens);
  state.length = end;
  // The fingerprint is the tail of the *consumed* prefix only. Slicing without
  // an end bound would run past the cursor to the end of the document, so the
  // next frame's origin lands negative, `isAppendOnly` reports a rewrite, and
  // the state resets to zero every frame - the file then never scrolls past the
  // first chunk and the bottom of the preview stays cut off.
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
 * Find the start offset such that `[from, end)` holds at most `maxLines` lines.
 * Returns `from` when there are not enough newlines to shrink by.
 */
function backUpToLineBoundary(
  code: string,
  from: number,
  maxLines: number,
): number {
  let end = code.length;
  let lines = 1;
  for (let i = end - 1; i >= from; i -= 1) {
    if (code.charCodeAt(i) === 10) {
      lines += 1;
      if (lines > maxLines) return i;
      end = i;
    }
  }
  return from;
}
