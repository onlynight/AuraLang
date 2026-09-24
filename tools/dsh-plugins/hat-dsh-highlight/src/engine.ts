/**
 * RegExp engine selection.
 *
 * Two engines are supported:
 *
 *   - `javascript` (default) - pure JS port of Oniguruma. No WebAssembly, no
 *     network, no CSP interaction. Identical TextMate semantics to oniguruma,
 *     slower on large inputs.
 *
 *   - `oniguruma` - the real WebAssembly engine, roughly an order of magnitude
 *     faster. Its wasm is NOT bundled here: the caller must supply it, because
 *     Shiki ships it as a 620 KB base64 blob that would dominate a plugin
 *     bundle, and the wasm is only worth that weight when the host has already
 *     decided it can run WebAssembly.
 *
 * The default is `javascript` because a document-preview plugin has to work
 * everywhere - including hosts with restrictive CSP, offline installs, and
 * sandboxed iframes - and the incremental renderer in incremental.ts bounds the
 * cost of the slower engine to the newly appended tail of the document.
 */

import { createJavaScriptRegexEngine } from '@shikijs/engine-javascript';
import { createOnigurumaEngine } from '@shikijs/engine-oniguruma';
import type { Awaitable, RegexEngine } from '@shikijs/core';

/** Which RegExp engine to use. */
export type EngineKind = 'javascript' | 'oniguruma';

/** Everything {@link createOnigurumaEngine} accepts as a wasm source. */
export type WasmSource = Parameters<typeof createOnigurumaEngine>[0];

export interface EngineOptions {
  /** Default `javascript`. */
  readonly kind?: EngineKind;
  /**
   * Required when `kind` is `oniguruma`. A URL, an ArrayBuffer, a Response, a
   * wasm instantiation factory, or Shiki's full `LoadWasmOptions` shape.
   */
  readonly wasm?: WasmSource;
}

/** Default engine; see the module comment. */
export const DEFAULT_ENGINE_KIND: EngineKind = 'javascript';

/**
 * Build the RegExp engine.
 *
 * @returns a possibly-pending engine, matching Shiki's `Awaitable<RegexEngine>`
 *          contract so callers can hand it straight to `createHighlighterCore`.
 * </returns>
 */
export function createEngine(
  options: EngineOptions = {},
): Awaitable<RegexEngine> {
  const kind = options.kind ?? DEFAULT_ENGINE_KIND;
  if (kind === 'oniguruma') {
    if (options.wasm === undefined) {
      throw new Error(
        '[hat-dsh-highlight] engine "oniguruma" needs a wasm source: pass ' +
          '{ kind: "oniguruma", wasm: <url | ArrayBuffer | LoadWasmOptions> }.',
      );
    }
    return createOnigurumaEngine(options.wasm);
  }
  return createJavaScriptRegexEngine();
}
