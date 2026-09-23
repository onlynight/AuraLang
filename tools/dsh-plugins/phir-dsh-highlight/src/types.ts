/**
 * Structural TextMate grammar types, mirroring VSCode's tmLanguage.json.
 *
 * Kept local rather than imported from Shiki so the plugin's grammar surface
 * stays stable across Shiki versions, and so the generated grammar module has
 * no runtime dependency on the highlighter at all.
 */

/** One rule: either a reference into the grammar's `repository`, or an inline match. */
export type TextmatePattern = {
  readonly include?: string;
} & TextmateCapture;

/** An inline grammar rule. Every field is optional; TextMate is highly flexible. */
export interface TextmateCapture {
  /** Plain-string scope name, or per-capture names. */
  readonly name?: string | Readonly<Record<string, string>>;
  /** The scope name applied to the text matched between `begin` and `end`. */
  readonly contentName?: string;
  readonly match?: string;
  readonly begin?: string;
  readonly end?: string;
  readonly beginCaptures?: TextmateCaptures;
  readonly endCaptures?: TextmateCaptures;
  readonly captures?: TextmateCaptures;
  /** Nested patterns active inside this rule's match. */
  readonly patterns?: readonly TextmatePattern[];
  /** Optional while-loop continuation for `begin`/`end` pairs. */
  readonly while?: string;
  /** Allow an unmatched `begin` to span to the end of the buffer. */
  readonly whileCaptures?: TextmateCaptures;
  readonly applyEndPatternLast?: boolean;
  readonly backtrackLimit?: number;
  readonly caseInsensitive?: boolean;
  readonly caseSensitive?: boolean;
  readonly inInject?: string;
  readonly injectionSelector?: string;
  readonly injectionPattern?: string;
  readonly injections?: unknown;
  readonly prefixes?: readonly string[];
  readonly unescapedPunctuation?: readonly string[];
  /** Escape any other VSCode-specific metadata as-is. */
  readonly [key: string]: unknown;
}

/** Capture group index 閳?scope name (or nested capture table). */
export type TextmateCaptures = Readonly<
  Record<string, string | TextmateCapture>
>;

/** A full TextMate grammar, as loaded from tmLanguage.json. */
export interface TextmateGrammar {
  /** Shiki's language id. Must be lowercase and match `grammarName`. */
  readonly name: string;
  /** Scope name, e.g. `source.phir`. */
  readonly scopeName: string;
  /** Display name for UIs. */
  readonly displayName?: string;
  /** File extensions, without the leading dot. */
  readonly fileTypes?: readonly string[];
  readonly patterns: readonly TextmatePattern[];
  readonly repository?: Readonly<Record<string, TextmateCapture>>;
  readonly aliases?: readonly string[];
  /** Escape any other VSCode-specific metadata as-is. */
  readonly [key: string]: unknown;
}

/** Alias accepted by {@link languageForPath}. */
export type PhirLanguage = 'phir';

/**
 * The one `.phir` extension this plugin owns.
 *
 * Held as a single constant so the document-preview registration, the path
 * matcher, and the tests cannot drift apart.
 */
export const PHIR_EXTENSION = 'phir';

/** Case-insensitive aliases this plugin answers to. */
export const PHIR_ALIASES: readonly string[] = ['phir'];

/**
 * Decide whether a decoded file path is a PHIR source file.
 *
 * Compound suffixes are rejected: `phir.ts` is TypeScript that happens to
 * mention "phir", not a PHIR source file.
 *
 * @param path - decoded path or bare filename, either slash style.
 * @returns `true` when this plugin should render the file.
 */
export function isPhirPath(path: string | undefined): boolean {
  if (!path) return false;
  const name = path.replace(/\\/g, '/').split('/').pop() ?? '';
  const dot = name.lastIndexOf('.');
  if (dot <= 0 || dot === name.length - 1) return false;
  return name.slice(dot + 1).toLowerCase() === PHIR_EXTENSION;
}





