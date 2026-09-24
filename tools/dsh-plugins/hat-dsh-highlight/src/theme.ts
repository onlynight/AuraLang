/**
 * HAT color themes for Shiki.
 *
 * Two themes (`hat-light`, `hat-dark`) are built from one shared rule table
 * over one shared color vocabulary. Every color is emitted as a CSS variable
 * reference with a literal fallback:
 *
 *   var(--HAT-keyword, #d6336c)
 *
 * That single choice does three jobs:
 *
 *   1. The viewer can redefine `--HAT-*` on its own element (or on `:root`,
 *      which is where this plugin installs them) and every token follows.
 *   2. DSH's existing `--dsw-*` tokens can be aliased onto `--HAT-*` by a
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
  | 'keyword'
  | 'keyword-operator'
  | 'keyword-constant'
  | 'storage'
  | 'type'
  | 'type-entity'
  | 'function'
  | 'function-call'
  | 'variable'
  | 'variable-parameter'
  | 'variable-constant'
  | 'variable-property'
  | 'string'
  | 'string-key'
  | 'number'
  | 'boolean'
  | 'regexp'
  | 'punctuation';

/** The resolved light/dark value for every {@link ColorKey}. */
export type Palette = Readonly<Record<ColorKey, string>>;

export type ThemeMode = 'light' | 'dark';

/** CSS variable name for a vocabulary entry. */
export function colorVar(key: ColorKey): string {
  return `--HAT-${key}`;
}

/** Emit `var(--HAT-<key>, <fallback>)`. */
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
 * Scope names follow the grammar's own `\.hat` suffix convention
 * (see grammar/hat.tmLanguage.json).
 */
const RULES: readonly Rule[] = [
  // --- Comments -----------------------------------------------------------
  { scope: 'comment', key: 'comment' },
  { scope: 'comment.line', key: 'comment' },
  // `;@span` annotation (the only comment-shaped token in HAT)
  { scope: 'comment.block.span', key: 'comment' },

  // --- Keywords -----------------------------------------------------------
  { scope: 'keyword', key: 'keyword' },
  // `@fn` / `@extern` / `@struct` / `@enum`
  { scope: 'keyword.declaration', key: 'keyword' },
  { scope: 'keyword.other', key: 'keyword' },
  { scope: 'keyword.other.target', key: 'keyword' },
  { scope: 'keyword.other.meta', key: 'keyword' },
  { scope: 'keyword.other.bb', key: 'keyword' },
  { scope: 'keyword.other.arrow', key: 'keyword-constant' },
  { scope: 'keyword.other.assignment', key: 'punctuation' },
  // Instruction opcodes. The broad `.instruction` rule must precede the
  // narrower `.phi` / `.constant` refinements below.
  { scope: 'keyword.other.instruction', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.arithmetic', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.bitwise', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.call', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.collection', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.comparison', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.conversion', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.logical', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.memory', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.object', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.string', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.terminator', key: 'keyword-operator' },
  { scope: 'keyword.other.instruction.phi', key: 'keyword-constant' },
  { scope: 'keyword.other.instruction.constant', key: 'keyword-constant' },

  // --- Declarations -------------------------------------------------------
  { scope: 'storage', key: 'storage' },
  { scope: 'storage.type', key: 'type' },
  { scope: 'storage.type.basic', key: 'type' },
  { scope: 'storage.type.special', key: 'type' },
  { scope: 'storage.type.user', key: 'type-entity' },

  // --- Types and entities -------------------------------------------------
  { scope: 'entity', key: 'type-entity' },
  { scope: 'entity.name.type', key: 'type-entity' },
  { scope: 'entity.name.type.module', key: 'type-entity' },
  { scope: 'entity.name.type.label', key: 'type-entity' },
  { scope: 'entity.name.function', key: 'function' },
  { scope: 'entity.name.function.call', key: 'function-call' },

  // --- Variables ----------------------------------------------------------
  { scope: 'variable', key: 'variable' },
  { scope: 'variable.parameter', key: 'variable-parameter' },
  { scope: 'variable.ssaval', key: 'variable' },
  { scope: 'variable.ssaval.literal', key: 'string' },
  { scope: 'variable.field', key: 'variable' },
  // Attribute keys inside `{ … }` (size / align / count): colored like the
  // instruction that owns them, matching the pre-v3.0 rendering.
  { scope: 'variable.attribute', key: 'keyword-operator' },
  // Enum variants: declaration-shaped, like types and functions.
  { scope: 'variable.other.enummember', key: 'type-entity' },

  // --- Literals -----------------------------------------------------------
  { scope: 'string', key: 'string' },
  { scope: 'string.quoted', key: 'string' },
  { scope: 'string.quoted.double', key: 'string' },
  { scope: 'string.quoted.single', key: 'string' },
  // Unquoted metadata values: `schema=HAT/2.0`, `source=<path>`
  { scope: 'string.unquoted', key: 'string-key' },
  { scope: 'constant', key: 'number' },
  { scope: 'constant.numeric', key: 'number' },
  { scope: 'constant.numeric.integer', key: 'number' },
  { scope: 'constant.numeric.float', key: 'number' },
  { scope: 'constant.language', key: 'boolean' },
  { scope: 'constant.character', key: 'string' },

  // --- Punctuation --------------------------------------------------------
  { scope: 'punctuation', key: 'punctuation' },
  { scope: 'punctuation.separator', key: 'punctuation' },
  { scope: 'punctuation.terminator', key: 'punctuation' },
  { scope: 'punctuation.definition', key: 'punctuation' },
  { scope: 'punctuation.accessor', key: 'punctuation' },
];

/** Light palette: VSCode Light+ hue identity. */
export const HAT_LIGHT_PALETTE: Palette = {
  foreground: '#1f2328',
  comment: '#6a737d',
  keyword: '#d6336c',
  'keyword-operator': '#0550ae',
  'keyword-constant': '#0550ae',
  storage: '#cf222e',
  type: '#0969da',
  'type-entity': '#8250df',
  function: '#8250df',
  'function-call': '#8250df',
  variable: '#1f2328',
  'variable-parameter': '#1f2328',
  'variable-constant': '#0550ae',
  'variable-property': '#1f2328',
  string: '#0a3069',
  'string-key': '#0a3069',
  number: '#0550ae',
  boolean: '#0550ae',
  regexp: '#0a3069',
  punctuation: '#6a737d',
};

/** Dark palette: VSCode Dark+ hue identity. */
export const HAT_DARK_PALETTE: Palette = {
  foreground: '#e6edf3',
  comment: '#8b949e',
  keyword: '#ff7b72',
  'keyword-operator': '#79c0ff',
  'keyword-constant': '#ffa657',
  storage: '#ff7b72',
  type: '#79c0ff',
  'type-entity': '#d2a8ff',
  function: '#d2a8ff',
  'function-call': '#d2a8ff',
  variable: '#e6edf3',
  'variable-parameter': '#e6edf3',
  'variable-constant': '#79c0ff',
  'variable-property': '#e6edf3',
  string: '#a5d6ff',
  'string-key': '#a5d6ff',
  number: '#79c0ff',
  boolean: '#79c0ff',
  regexp: '#a5d6ff',
  punctuation: '#8b949e',
};

export const HAT_LIGHT_THEME: ThemeRegistration = buildTheme(
  'hat-light',
  'light',
  HAT_LIGHT_PALETTE,
);

export const HAT_DARK_THEME: ThemeRegistration = buildTheme(
  'hat-dark',
  'dark',
  HAT_DARK_PALETTE,
);

/** Theme name to use for a display mode. */
export const THEME_BY_MODE: Readonly<Record<ThemeMode, string>> = {
  light: 'hat-light',
  dark: 'hat-dark',
};

function buildTheme(
  name: string,
  type: 'light' | 'dark',
  palette: Palette,
): ThemeRegistration {
  return {
    name,
    displayName: name === 'hat-dark' ? 'HAT Dark' : 'HAT Light',
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
 * re-render. Consumers that prefer their own colors redefine `--HAT-*` on any
 * ancestor of the viewer.
 */
export function themeVariableCss(): string {
  const light = HAT_LIGHT_PALETTE;
  const dark = HAT_DARK_PALETTE;
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
