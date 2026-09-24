/**
 * Public highlighting API: `import { tokenizeHAT } from '@aura-lang/dsh-highlight-hat/highlight'`.
 *
 * Deliberately DOM-free and React-free, so the same grammar, themes, and
 * tokenizer run in Node tests, a CLI, and an LSP without dragging the browser
 * half along. The client half (`./client`) composes on top of this.
 *
 * @module
 */

export {
  HAT_GRAMMAR,
  getHighlighter,
  resetHighlighter,
  resolveThemeMode,
  tokenizeHAT,
  tokenizeHATWithState,
  type HATHighlighter,
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

export { decodeHatSource } from '../decode.js';

export {
  DEFAULT_ENGINE_KIND,
  createEngine,
  type EngineKind,
  type EngineOptions,
  type WasmSource,
} from '../engine.js';

export {
  HAT_DARK_PALETTE,
  HAT_DARK_THEME,
  HAT_LIGHT_PALETTE,
  HAT_LIGHT_THEME,
  THEME_BY_MODE,
  colorRef,
  colorVar,
  themeVariableCss,
  type ColorKey,
  type Palette,
  type ThemeMode,
} from '../theme.js';

export {
  HAT_ALIASES,
  HAT_EXTENSION,
  isHatPath,
  type HatLanguage,
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
