/**
 * The plugin's Shiki instance: one lazy singleton per page, preloaded with the
 * Aura grammar and both Aura themes.
 *
 * Everything downstream - the incremental renderer, the fallback tokenizer, and
 * the `./highlight` test entry - goes through {@link getHighlighter}, so the
 * singleton never leaks into the render path.
 *
 * @module
 */

import { createHighlighterCore } from '@shikijs/core';
import type {
  Awaitable,
  HighlighterGeneric,
  LanguageRegistration,
  RegexEngine,
  ThemedToken,
  TokensResult,
} from '@shikijs/core';

import { AURA_GRAMMAR } from './aura-grammar.js';
import { createEngine, type EngineOptions } from './engine.js';
import { AURA_DARK_THEME, AURA_LIGHT_THEME, THEME_BY_MODE } from './theme.js';
import type { ThemeMode } from './theme.js';

/** The highlighter instance this plugin owns. */
export type AuraHighlighter = Awaited<
  ReturnType<typeof createHighlighterCore>
>;

/**
 * Resolve the display mode for a highlight request.
 *
 * `auto` reads `document.body[data-ds-dark-theme]`, the attribute DSH toggles
 * to switch themes. When the attribute is absent the light theme is used, which
 * is also the safe choice outside a DSH host.
 */
export function resolveThemeMode(
  mode: ThemeMode | 'auto' = 'auto',
): ThemeMode {
  if (mode !== 'auto') return mode;
  if (typeof document === 'undefined') return 'light';
  return document.body.hasAttribute('data-ds-dark-theme') ? 'dark' : 'light';
}

export interface HighlightOptions extends EngineOptions {
  /** Which palette to color with. Default `auto`. */
  readonly theme?: ThemeMode | 'auto';
  /** Tokenize a maximum number of characters per line. Default 0 (unlimited). */
  readonly tokenizeMaxLineLength?: number;
  /** Per-line tokenize time budget in milliseconds. Default 500. */
  readonly tokenizeTimeLimit?: number;
}

let instance: AuraHighlighter | null = null;
let pending: Promise<AuraHighlighter> | null = null;

/**
 * Get (building on first use) the shared highlighter.
 *
 * The build is memoized: concurrent callers await one promise rather than racing
 * to construct two grammars.
 */
export function getHighlighter(
  options: HighlightOptions = {},
): Promise<AuraHighlighter> {
  if (instance !== null) return Promise.resolve(instance);
  if (pending !== null) return pending;

  const build = async () => {
    const engine = await resolveEngine(options);
    const highlighter = await createHighlighterCore({
      engine,
      // AURA_GRAMMAR is structurally a LanguageRegistration; the cast hides the
      // local TextmateGrammar index signature from Shiki's exact types.
      langs: [AURA_GRAMMAR as unknown as LanguageRegistration],
      themes: [AURA_LIGHT_THEME, AURA_DARK_THEME],
      warnings: false,
    });
    instance = highlighter;
    return highlighter;
  };

  pending = build().catch((error) => {
    pending = null;
    throw error;
  });
  return pending;
}

async function resolveEngine(options: HighlightOptions): Promise<RegexEngine> {
  const engine = await createEngine(options);
  return engine;
}

/**
 * Tokenize code into per-line token arrays.
 *
 * @param code - source to tokenize.
 * @param options - engine and theme selection.
 * @returns one array per input line, preserving empty lines as `[]`.
 */
export async function tokenizeAura(
  code: string,
  options: HighlightOptions = {},
): Promise<ThemedToken[][]> {
  const highlighter = await getHighlighter(options);
  const theme = THEME_BY_MODE[resolveThemeMode(options.theme)];
  const result = highlighter.codeToTokens(code, {
    lang: 'aura',
    theme,
    ...(options.tokenizeMaxLineLength !== undefined
      ? { tokenizeMaxLineLength: options.tokenizeMaxLineLength }
      : {}),
    ...(options.tokenizeTimeLimit !== undefined
      ? { tokenizeTimeLimit: options.tokenizeTimeLimit }
      : {}),
  });
  return result.tokens;
}

/**
 * Tokenize and report the terminal grammar state.
 *
 * Useful for callers that want to resume tokenization later without going
 * through incremental.ts.
 */
export async function tokenizeAuraWithState(
  code: string,
  options: HighlightOptions = {},
): Promise<TokensResult> {
  const highlighter = await getHighlighter(options);
  const theme = THEME_BY_MODE[resolveThemeMode(options.theme)];
  return highlighter.codeToTokens(code, {
    lang: 'aura',
    theme,
  });
}

/** Drop the cached highlighter. Primarily for tests. */
export function resetHighlighter(): void {
  instance = null;
  pending = null;
}

/** Narrow Shiki's HighlighterGeneric to the members this plugin calls. */
export type { HighlighterGeneric };

/** The grammar this plugin registers, re-exported for introspection. */
export { AURA_GRAMMAR };
