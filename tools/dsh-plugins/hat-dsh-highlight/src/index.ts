/**
 * Package root: the plugin's *host* half.
 *
 * DSH mounts one Loader row per plugin, and `@deepseek-ai/dsh-client-modules`
 * composes the browser roster by scanning those mounted rows for packages that
 * declare `dsh.client`. A browser-only plugin therefore still ships a host half:
 * without a mounted row there is no entry to scan, the bundle is never served,
 * and the preview silently never registers.
 *
 * The host half contributes nothing to the host tree (`apply` is empty). All of
 * the plugin's behaviour lives in the browser export (`./client`), and the
 * DOM-free highlighting API stays on the `./highlight` subpath so importing the
 * root does not pull Shiki into the host process.
 *
 * @module
 */

export { PLUGIN_ID, PREVIEW_ID, DOCUMENT_SLOT } from './plugin-context.js';

/**
 * Host plugin body: the HAT preview contributes nothing to the host tree.
 *
 * The row exists so `dsh-client-modules` discovers the `dsh.client` declaration
 * and serves `./client` to the browser, which is where registration happens.
 */
export function apply(): void {}
