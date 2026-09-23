/**
 * DSH client plugin entry: register the PHIR code preview.
 *
 * Four registrations, all inside `ctx.effect` so they tear down cleanly when the
 * plugin is unmounted:
 *
 *   1. locale dictionaries - so `t()` resolves `title`/`copy`/`copied`;
 *   2. stylesheets - the `--phir-*` variables and the viewer layout;
 *   3. the preview *definition* - tells the document owner that `.phir` files
 *      exist and who renders them, at `extension` priority so it beats the
 *      built-in Code preview;
 *   4. the keyed document *body* - the component that actually draws the code.
 *
 * 1 and 3 are independent of any slot, so they register immediately. 4 goes
 * through `slots.inject` because the owning plugin (`documentpreview`) declares
 * the slot, and `inject` defers until it exists.
 *
 * @module
 */

import type { Context } from '@deepseek-ai/cordis';

import { PhirCodeBody } from './PhirCodeBody.js';
import { PHIR_EXTENSION } from './types.js';
import { en, LOCALE_NAMESPACE, zh } from './locale.js';
import { PLUGIN_CSS } from './styles.js';
import {
  assertPluginContext,
  DOCUMENT_SLOT,
  PLUGIN_ID,
  PREVIEW_ID,
  type PluginContext,
} from './plugin-context.js';

/**
 * Services this plugin consumes.
 *
 * `slots` and `locale` are the DSH client services that nearly every client
 * plugin needs; `documentPreviews` is the registry `documentpreview` provides,
 * which is what makes this plugin wait for it rather than racing it.
 */
export const inject: readonly string[] = ['slots', 'locale', 'documentPreviews'];

/**
 * Register the plugin.
 *
 * @param ctx - client root context carrying the registries and the effect
 *              runner.
 */
export function apply(ctx: Context): void {
  assertPluginContext(ctx);
  const plugin = ctx as unknown as PluginContext;
  const t = plugin.locale.bind(LOCALE_NAMESPACE);

  // 1. Dictionaries.
  plugin.effect(
    () => plugin.locale.register(LOCALE_NAMESPACE, { zh, en }),
    'phir-highlight: dictionaries',
  );

  // 2. Stylesheets.
  for (const [name, css] of PLUGIN_CSS) {
    plugin.effect(
      () => {
        const tag = document.createElement('style');
        tag.setAttribute('data-plugin', PLUGIN_ID);
        tag.setAttribute('data-plugin-css', `${PLUGIN_ID}/${name}`);
        tag.textContent = css;
        document.head.appendChild(tag);
        return () => {
          tag.remove();
        };
      },
      `phir-highlight: ${name}`,
    );
  }

  // 3. Preview definition - metadata only, evaluated by the toolbar at render
  //    time, which is why `title` is a closure over `t` and not a frozen string.
  plugin.effect(
    () =>
      plugin.documentPreviews.register({
        id: PREVIEW_ID,
        extensions: [PHIR_EXTENSION],
        priority: 'extension',
        title: () => t('title'),
        // Complete bytes, not pages. The owner refuses any *page* whose text
        // holds a NUL byte (`workspace-file/not-text`) and its backend refuses
        // malformed UTF-8, both before a renderer is consulted; a `.phir` IR
        // dump with a binary payload always trips that, so `text-pages` would
        // make those files unopenable rather than merely unhighlighted.
        // `bytes-complete` hands the file over whole and `decode.ts` turns it
        // into text. See README, "Delivery".
        loading: 'bytes-complete',
        wrap: true,
      }),
    'phir-highlight: preview definition',
  );

  // 4. The keyed body.
  plugin.effect(
    () =>
      plugin.slots.inject(
        DOCUMENT_SLOT,
        () =>
          plugin.slots.register(
            {
              name: DOCUMENT_SLOT,
              key: PREVIEW_ID,
              locale: LOCALE_NAMESPACE,
            },
            PhirCodeBody,
          ),
      ),
    'phir-highlight: document body',
  );
}

