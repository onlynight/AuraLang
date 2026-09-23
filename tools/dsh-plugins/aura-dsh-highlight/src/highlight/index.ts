/**
 * Public highlighting API: `import { tokenizeAura } from '@aura-lang/dsh-highlight-aura/highlight'`.
 *
 * Deliberately DOM-free and React-free, so the same grammar, themes, and
 * tokenizer run in Node tests, a CLI, and an LSP without dragging the browser
 * half along. The client half (`./client`) composes on top of this.
 *
 * @module
 */

export {
  AURA_GRAMMAR,
  getHighlighter,
  resetHighlighter,
  resolveThemeMode,
  tokenizeAura,
  tokenizeAuraWithState,
  type AuraHighlighter,
  type HighlightOptions,
} from '../highlighter.js';

export {
  DEFAULT_CHARS,
  DEFAULT_LINES,
  createIncrementalState,
  resetIncrementalState,
  tokenizeNextChunk,
  type ChunkBudget,
  type ChunkResult,
  type IncrementalState,
} from '../incremental.js';

export { tokenizeFallback } from '../fallback.js';

export {
  DEFAULT_ENGINE_KIND,
  createEngine,
  type EngineKind,
  type EngineOptions,
  type WasmSource,
} from '../engine.js';

export {
  AURA_DARK_PALETTE,
  AURA_DARK_THEME,
  AURA_LIGHT_PALETTE,
  AURA_LIGHT_THEME,
  THEME_BY_MODE,
  colorRef,
  colorVar,
  themeVariableCss,
  type ColorKey,
  type Palette,
  type ThemeMode,
} from '../theme.js';

export {
  AURA_ALIASES,
  AURA_EXTENSION,
  isAuraPath,
  type AuraLanguage,
  type TextmateCaptures,
  type TextmateGrammar,
  type TextmatePattern,
} from '../types.js';

export {
  LOCALE_KEYS,
  LOCALE_NAMESPACE,
  en,
  zh,
  type LocaleDict,
  type LocaleKey,
} from '../locale.js';
