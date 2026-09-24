/**
 * DSH client plugin entry: register the HAT code preview.
 *
 * Four registrations, all inside `ctx.effect` so they tear down cleanly when the
 * plugin is unmounted:
 *
 *   1. locale dictionaries - so `t()` resolves `title`/`copy`/`copied`;
 *   2. stylesheets - the `--HAT-*` variables and the viewer layout;
 *   3. the preview *definition* - tells the document owner that `.hat` files
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

import { HatCodeBody } from './HatCodeBody.js';
import { HAT_EXTENSION } from './types.js';
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
    'hat-highlight: dictionaries',
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
      `hat-highlight: ${name}`,
    );
  }

  // 3. Preview definition - metadata only, evaluated by the toolbar at render
  //    time, which is why `title` is a closure over `t` and not a frozen string.
  plugin.effect(
    () =>
      plugin.documentPreviews.register({
        id: PREVIEW_ID,
        extensions: [HAT_EXTENSION],
        priority: 'extension',
        title: () => t('title'),
        loading: 'bytes-complete',
        wrap: true,
      }),
    'hat-highlight: preview definition',
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
            HatCodeBody,
          ),
      ),
    'hat-highlight: document body',
  );
}
