/**
 * The slice of the DSH client context this plugin consumes.
 *
 * `ctx` is a Cordis `Context` whose members are added by declaration merging
 * across many packages. This plugin deliberately types the slice it uses
 * structurally rather than pulling in each augmenting package as a devDep,
 * because:
 *
 *   - the platform module seed this plugin declares guarantees only `react`,
 *     `@deepseek-ai/cordis`, `dsh-client-store`, `dsh-client-ui-slots`,
 *     `dsh-client-ui-primitives`, and `dsh-client-ui-dockkit`;
 *   - the augmenting packages version independently, so a pinned devDep can be
 *     older than the host that is actually running;
 *   - a structural mismatch is a compile error here, and the runtime probe in
 *     {@link assertPluginContext} names the first missing capability.
 *
 * Types only: nothing here is imported at runtime.
 */

import type { Context as ClientContext } from '@deepseek-ai/cordis';
import type {
  DocumentPreviewDefinition,
} from '@deepseek-ai/dsh-client-ui-sidebar-documentpreview/client';

/** The registry `documentpreview` publishes on the client context. */
export type DocumentPreviewRegistry = ClientContext['documentPreviews'];

/** Re-exported so consumers do not need the documentpreview subpath. */
export type { DocumentPreviewDefinition };

/** Locale service subset: register a namespace, bind a translate function. */
export interface LocaleService {
  /** Register dictionaries for a namespace. May return a disposer. */
  readonly register: (
    namespace: string,
    dictionaries: Record<string, unknown>,
  ) => void | (() => void);
  /** Get a translate function for a registered namespace. */
  readonly bind: (
    namespace: string,
  ) => (key: string, params?: Record<string, unknown>) => string;
}

/** Slot service subset: deferred registration against a keyed or plain slot. */
export interface SlotsService {
  /**
   * Register once `slotKey` has been declared. The callback runs at the first
   * moment the slot exists, which is why plugins call it instead of `register`
   * directly when the owning plugin may load later.
   */
  readonly inject: (
    slotKey: string,
    callback: () => void | (() => void),
  ) => void | (() => void);
  /** Register a component against a slot. */
  readonly register: (
    options: Record<string, unknown>,
    component: unknown,
  ) => void | (() => void);
}

/**
 * Effect runner: subscribe and dispose.
 *
 * The runner takes a body and a stable label. The body's return value is the
 * disposer - either a function or an object with `dispose()`.
 */
export type EffectService = <R>(
  effect: () => R | (() => void) | { dispose: () => void },
  label?: string,
) => void;

/** The full slice this plugin consumes. */
export interface PluginContext {
  readonly effect: EffectService;
  readonly locale: LocaleService;
  readonly slots: SlotsService;
  readonly documentPreviews: DocumentPreviewRegistry;
}

/**
 * Fail loud, naming the first missing capability.
 *
 * A plugin that misses a service should not fail silently with "undefined is
 * not a function" deep in a render; it should say what it needs.
 *
 * @param ctx - the client context handed to `apply`.
 * @param who - the plugin name, for the message.
 * @throws when any capability is missing.
 */
export function assertPluginContext(
  ctx: unknown,
  who = '@aura-lang/dsh-highlight-aura',
): void {
  const need: ReadonlyArray<readonly [string, (value: unknown) => boolean]> = [
    ['effect', isFunction],
    ['locale.register', isFunction],
    ['locale.bind', isFunction],
    ['slots.inject', isFunction],
    ['slots.register', isFunction],
    ['documentPreviews.register', isFunction],
  ];
  const record = ctx as Record<string, unknown> | null | undefined;
  for (const [path, isOk] of need) {
    let current: unknown = record;
    let walked = '';
    for (const segment of path.split('.')) {
      if (current !== null && typeof current === 'object') {
        walked = walked === '' ? segment : `${walked}.${segment}`;
        current = (current as Record<string, unknown>)[segment];
      } else {
        current = undefined;
      }
    }
    if (!isOk(current)) {
      throw new Error(
        `[${who}] host is missing the "${walked}" service; cannot register ` +
          'the Aura code preview.',
      );
    }
  }
}

function isFunction(value: unknown): boolean {
  return typeof value === 'function';
}

/**
 * This plugin's identity, used in the `data-plugin` attributes of the stylesheets
 * it injects so HMR bookkeeping can claim them.
 */
export const PLUGIN_ID = '@aura-lang/dsh-highlight-aura';

/** The document slot this plugin renders into. */
export const DOCUMENT_SLOT = 'sidebar.right.tab.document';

/** The preview implementation id, also used as the document slot key. */
export const PREVIEW_ID = '@aura-lang/dsh-highlight-aura/code';

/** The service name the host exposes the registry under. */
export const PREVIEW_SERVICE = 'documentPreviews';
