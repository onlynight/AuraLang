/**
 * Declaration-merging side effects only.
 *
 * This file is imported for its types, never for its value. It exists because
 * `PropsLocale<'sidebarHATCodePreview'>` only type-checks once the namespace is
 * declared in `LocaleNamespaceMap`, and the augmentation must be lexically
 * present somewhere in the compilation.
 */

import type { LocaleKey } from './locale.js';

declare module '@deepseek-ai/dsh-client-ui-slots' {
  interface LocaleNamespaceMap {
    /** HAT code-viewer banner and copy control. */
    sidebarHATCodePreview: LocaleKey;
  }
}
