/**
 * PHIR color themes for Shiki.
 *
 * Two themes (`phir-light`, `phir-dark`) are built from one shared rule table
 * over one shared color vocabulary. Every color is emitted as a CSS variable
 * reference with a literal fallback:
 *
 *   var(--PHIR-keyword, #d6336c)
 *
 * That single choice does three jobs:
 *
 *   1. The viewer can redefine `--PHIR-*` on its own element (or on `:root`,
 *      which is where this plugin installs them) and every token follows.
 *   2. DSH's existing `--dsw-*` tokens can be aliased onto `--PHIR-*` by a
 *      single CSS rule, so the viewer tracks the host's light/dark switch
 *      without a re-render or a theme swap.
 *   3. The fallback keeps highlighting legible if the styles never load.
 *
 * The hex defaults below mirror DSH's built-in `--shiki-token-*` defaults so a
 * viewer rendered outside DSH still looks like DSH.
 *
 * Theme names carry the light/dark marker because Shiki theme identity is a
 * plain string; `THEME_BY_MODE` selects at render time.
 */

import type { ThemeRegistration } from '@shikijs/core';

/** One entry of the shared color vocabulary. */
export type ColorKey =
  | 'foreground'
  | 'comment'
  | 'comment-doc'
  | 'keyword'
  | 'keyword-control'
  | 'keyword-special'
  | 'storage'
  | 'storage-modifier'
  | 'type'
  | 'type-entity'
  | 'type-alias'
  | 'annotation'
  | 'function'
  | 'function-call'
  | 'variable'
  | 'variable-parameter'
  | 'variable-constant'
  | 'variable-property'
  | 'variable-self'
  | 'string'
  | 'string-key'
  | 'string-interp'
  | 'number'
  | 'boolean'
  | 'regexp'
  | 'punctuation';

/** The resolved light/dark value for every {@link ColorKey}. */
export type Palette = Readonly<Record<ColorKey, string>>;

export type ThemeMode = 'light' | 'dark';

/** CSS variable name for a vocabulary entry. */
export function colorVar(key: ColorKey): string {
  return `--PHIR-${key}`;
}

/** Emit `var(--PHIR-<key>, <fallback>)`. */
export function colorRef(key: ColorKey, palette: Palette): string {
  return `var(${colorVar(key)}, ${palette[key]})`;
}

type FontStyle = 'italic' | 'bold' | 'underline' | 'strikethrough';

interface Rule {
  /** Scope pattern or patterns. */
  readonly scope: string | string[];
  /** Vocabulary entry to color from. */
  readonly key: ColorKey;
  /** Optional TextMate font style. */
  readonly fontStyle?: FontStyle;
}

/**
 * The rule table shared by both themes. Ordered deliberately: broad scopes
 * first, narrow ones later. TextMate theme matching lets a later rule refine an
 * earlier one, so `keyword.control` below narrows `keyword`.
 *
 * Scope names follow the grammar's own `\.phir` suffix convention
 * (see grammar/phir.tmLanguage.json).
 */
const RULES: readonly Rule[] = [
  // --- Comments -----------------------------------------------------------
  { scope: 'comment', key: 'comment' },
  { scope: 'comment.block.documentation', key: 'comment-doc', fontStyle: 'italic' },
  { scope: ['comment.line.documentation', 'comment.block.documentation.keyword'], key: 'comment-doc' },

  // --- Keywords -----------------------------------------------------------
  { scope: 'keyword', key: 'keyword' },
  { scope: 'keyword.control', key: 'keyword-control' },
  { scope: 'keyword.operator', key: 'keyword-special' },
  { scope: 'keyword.operator.assignment', key: 'keyword-special' },
  { scope: 'keyword.operator.comparison', key: 'keyword-special' },
  { scope: 'keyword.operator.logical', key: 'keyword-special' },
  { scope: 'keyword.operator.arithmetic', key: 'keyword-special' },
  { scope: 'keyword.operator.bitwise', key: 'keyword-special' },
  { scope: 'keyword.operator.range', key: 'keyword-special' },
  { scope: 'keyword.operator.spread', key: 'keyword-special' },
  { scope: 'keyword.operator.nullish', key: 'keyword-special' },
  { scope: 'keyword.operator.elvis', key: 'keyword-special' },
  { scope: 'keyword.operator.increment', key: 'keyword-special' },
  { scope: 'keyword.operator.decrement', key: 'keyword-special' },
  { scope: 'keyword.operator.assignment', key: 'keyword-special' },

  // --- Declarations -------------------------------------------------------
  { scope: 'storage', key: 'storage' },
  { scope: 'storage.type', key: 'storage' },
  { scope: 'storage.modifier', key: 'storage-modifier' },
  { scope: 'storage.type.builtin', key: 'type' },
  { scope: 'storage.type.class', key: 'storage' },
  { scope: 'storage.type.struct', key: 'storage' },
  { scope: 'storage.type.enum', key: 'storage' },
  { scope: 'storage.type.actor', key: 'storage' },
  { scope: 'storage.type.object', key: 'storage' },
  { scope: 'storage.type.import', key: 'storage' },
  { scope: 'storage.type.package', key: 'storage' },
  { scope: 'storage.type.extern', key: 'storage' },

  // --- Types and entities -------------------------------------------------
  { scope: 'entity', key: 'type-entity' },
  { scope: 'entity.name.type', key: 'type-entity' },
  { scope: 'entity.name.type.class', key: 'type-entity' },
  { scope: 'entity.name.type.struct', key: 'type-entity' },
  { scope: 'entity.name.type.enum', key: 'type-entity' },
  { scope: 'entity.name.type.enum.variant', key: 'type-entity', fontStyle: 'italic' },
  { scope: 'entity.name.type.typedef', key: 'type-alias' },
  { scope: 'entity.name.type.alias', key: 'type-alias' },
  { scope: 'entity.name.type.builtin', key: 'type' },
  { scope: 'entity.name.type.module', key: 'type-entity' },
  { scope: 'entity.name.package', key: 'type-entity' },
  { scope: 'entity.name.namespace', key: 'type-entity' },
  { scope: 'entity.other.attribute-name.annotation', key: 'annotation' },
  { scope: 'entity.name.tag', key: 'annotation' },

  // --- Functions and opcodes ----------------------------------------------
  { scope: 'entity.name.function', key: 'function' },
  // `support.*` is the grammar's bucket for things the language supplies rather
  // than the program defines; it is colored like a function name unless a
  // narrower rule below says otherwise.
  { scope: 'support', key: 'function' },
  { scope: 'support.function', key: 'function' },
  // Opcode mnemonics - `add`, `call`, `ret`, `gep`, `syscall`, ... The grammar
  // marks them `support.instruction`, which is a leaf scope on every IR
  // instruction, and the VSCode extension colors them with the function hue.
  { scope: 'support.instruction', key: 'function' },
  { scope: 'support.constant', key: 'variable-constant' },
  { scope: 'meta.function-call', key: 'function-call' },
  { scope: 'meta.function-call.entity.name.function', key: 'function-call' },
  { scope: 'meta.method-call', key: 'function-call' },

  // --- Variables ----------------------------------------------------------
  { scope: 'variable', key: 'variable' },
  { scope: 'variable.parameter', key: 'variable-parameter' },
  { scope: 'variable.parameter.function', key: 'variable-parameter' },
  { scope: 'variable.other.constant', key: 'variable-constant' },
  { scope: 'variable.other.object.property', key: 'variable-property' },
  { scope: 'variable.other.property', key: 'variable-property' },
  { scope: 'variable.other.object', key: 'variable-property' },
  { scope: 'variable.language', key: 'variable-self' },
  { scope: 'variable.language.this', key: 'variable-self' },
  { scope: 'variable.language.super', key: 'variable-self' },

  // --- Literals -----------------------------------------------------------
  { scope: 'string', key: 'string' },
  { scope: 'string.quoted', key: 'string' },
  { scope: 'string.quoted.double', key: 'string' },
  { scope: 'string.quoted.single', key: 'string' },
  { scope: 'string.quoted.triple', key: 'string' },
  { scope: 'string.quoted.double.multiline', key: 'string' },
  { scope: 'string.quoted.single.multiline', key: 'string' },
  { scope: 'string.quoted.other', key: 'string' },
  { scope: 'string.regexp', key: 'regexp' },
  { scope: 'string.interpolation', key: 'string-interp' },
  { scope: 'string.other.interpolation', key: 'string-interp' },
  { scope: 'punctuation.definition.string.interpolation', key: 'string-interp' },
  { scope: 'string.unquoted.label', key: 'string-key' },
  { scope: 'constant', key: 'number' },
  { scope: 'constant.numeric', key: 'number' },
  { scope: 'constant.numeric.integer', key: 'number' },
  { scope: 'constant.numeric.decimal', key: 'number' },
  { scope: 'constant.numeric.hex', key: 'number' },
  { scope: 'constant.numeric.binary', key: 'number' },
  { scope: 'constant.numeric.float', key: 'number' },
  { scope: 'constant.numeric.exponent', key: 'number' },
  { scope: 'constant.language', key: 'boolean' },
  { scope: 'constant.language.boolean', key: 'boolean' },
  { scope: 'constant.language.null', key: 'boolean' },

  // --- Punctuation --------------------------------------------------------
  { scope: 'punctuation', key: 'punctuation' },
  { scope: 'punctuation.separator', key: 'punctuation' },
  { scope: 'punctuation.terminator', key: 'punctuation' },
  { scope: 'punctuation.accessor', key: 'punctuation' },
  { scope: 'punctuation.definition', key: 'punctuation' },
  { scope: 'punctuation.section', key: 'punctuation' },
  { scope: 'punctuation.definition.parameters', key: 'punctuation' },
  { scope: 'punctuation.definition.block', key: 'punctuation' },
];

/** Light palette: VSCode Light+ hue identity, matching the PHIR VSCode plugin. */
export const PHIR_LIGHT_PALETTE: Palette = {
  foreground: '#1f2328',
  comment: '#6a737d',
  'comment-doc': '#6a737d',
  keyword: '#d6336c',
  'keyword-control': '#cf222e',
  'keyword-special': '#0550ae',
  storage: '#cf222e',
  'storage-modifier': '#953800',
  type: '#0969da',
  'type-entity': '#8250df',
  'type-alias': '#8250df',
  annotation: '#8250df',
  function: '#8250df',
  'function-call': '#8250df',
  variable: '#1f2328',
  'variable-parameter': '#1f2328',
  'variable-constant': '#0550ae',
  'variable-property': '#1f2328',
  'variable-self': '#cf222e',
  string: '#0a3069',
  'string-key': '#0a3069',
  'string-interp': '#0550ae',
  number: '#0550ae',
  boolean: '#0550ae',
  regexp: '#0a3069',
  punctuation: '#6a737d',
};

/** Dark palette: VSCode Dark+ hue identity, matching the PHIR VSCode plugin. */
export const PHIR_DARK_PALETTE: Palette = {
  foreground: '#e6edf3',
  comment: '#8b949e',
  'comment-doc': '#8b949e',
  keyword: '#ff7b72',
  'keyword-control': '#ff7b72',
  'keyword-special': '#79c0ff',
  storage: '#ff7b72',
  'storage-modifier': '#ffa657',
  type: '#79c0ff',
  'type-entity': '#d2a8ff',
  'type-alias': '#d2a8ff',
  annotation: '#d2a8ff',
  function: '#d2a8ff',
  'function-call': '#d2a8ff',
  variable: '#e6edf3',
  'variable-parameter': '#e6edf3',
  'variable-constant': '#79c0ff',
  'variable-property': '#e6edf3',
  'variable-self': '#ff7b72',
  string: '#a5d6ff',
  'string-key': '#a5d6ff',
  'string-interp': '#ffa657',
  number: '#79c0ff',
  boolean: '#79c0ff',
  regexp: '#a5d6ff',
  punctuation: '#8b949e',
};

export const PHIR_LIGHT_THEME: ThemeRegistration = buildTheme(
  'phir-light',
  'light',
  PHIR_LIGHT_PALETTE,
);

export const PHIR_DARK_THEME: ThemeRegistration = buildTheme(
  'phir-dark',
  'dark',
  PHIR_DARK_PALETTE,
);

/** Theme name to use for a display mode. */
export const THEME_BY_MODE: Readonly<Record<ThemeMode, string>> = {
  light: 'phir-light',
  dark: 'phir-dark',
};

function buildTheme(
  name: string,
  type: 'light' | 'dark',
  palette: Palette,
): ThemeRegistration {
  return {
    name,
    displayName: name === 'phir-dark' ? 'PHIR Dark' : 'PHIR Light',
    type,
    colors: {
      'editor.foreground': colorRef('foreground', palette),
      // Never paint a background: the host owns the surface.
      'editor.background': 'transparent',
    },
    tokenColors: RULES.map((rule) => ({
      scope: rule.scope,
      settings: {
        foreground: colorRef(rule.key, palette),
        ...(rule.fontStyle ? { fontStyle: rule.fontStyle } : {}),
      },
    })),
  };
}

/**
 * CSS custom properties for every vocabulary entry, for both display modes.
 *
 * Installed on `:root` by installStyles(). The `body[data-ds-dark-theme]`
 * block overrides the light values, so tokens track DSH's theme switch with no
 * re-render. Consumers that prefer their own colors redefine `--PHIR-*` on any
 * ancestor of the viewer.
 */
export function themeVariableCss(): string {
  const light = PHIR_LIGHT_PALETTE;
  const dark = PHIR_DARK_PALETTE;
  const entries = Object.keys(light) as ColorKey[];
  const lightDecl = entries
    .map((key) => `  ${colorVar(key)}:${light[key]};`)
    .join('\n');
  const darkDecl = entries
    .map((key) => `  ${colorVar(key)}:${dark[key]};`)
    .join('\n');
  return [
    ':root{',
    lightDecl,
    '}',
    'body[data-ds-dark-theme]{',
    darkDecl,
    '}',
  ].join('\n');
}


