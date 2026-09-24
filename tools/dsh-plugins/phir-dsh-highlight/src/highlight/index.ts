/**
 * Public highlighting API: `import { tokenizePHIR } from '@phir-lang/dsh-highlight-phir/highlight'`.
 *
 * Deliberately DOM-free and React-free, so the same grammar, themes, and
 * tokenizer run in Node tests, a CLI, and an LSP without dragging the browser
 * half along. The client half (`./client`) composes on top of this.
 *
 * @module
 */

export {
  PHIR_GRAMMAR,
  getHighlighter,
  resetHighlighter,
  resolveThemeMode,
  tokenizePHIR,
  tokenizePHIRWithState,
  type PHIRHighlighter,
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

export { decodePHIRSource } from '../decode.js';

export {
  DEFAULT_ENGINE_KIND,
  createEngine,
  type EngineKind,
  type EngineOptions,
  type WasmSource,
} from '../engine.js';

export {
  PHIR_DARK_PALETTE,
  PHIR_DARK_THEME,
  PHIR_LIGHT_PALETTE,
  PHIR_LIGHT_THEME,
  THEME_BY_MODE,
  colorRef,
  colorVar,
  themeVariableCss,
  type ColorKey,
  type Palette,
  type ThemeMode,
} from '../theme.js';

export {
  PHIR_ALIASES,
  PHIR_EXTENSION,
  isPhirPath,
  type PhirLanguage,
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






