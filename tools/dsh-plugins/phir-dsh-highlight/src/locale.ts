/**
 * Locale dictionaries for the PHIR code viewer.
 *
 * The key set below is the single source of truth: it is mirrored into the
 * `LocaleNamespaceMap` augmentation in client.ts, so a missing or extra key is
 * a compile error at the registration site.
 */

export const LOCALE_NAMESPACE = 'sidebarPHIRCodePreview';

export const LOCALE_KEYS = ['title', 'copy', 'copied'] as const;

export type LocaleKey = (typeof LOCALE_KEYS)[number];

export type LocaleDict = Record<LocaleKey, string>;

/** Simplified Chinese. */
export const zh: LocaleDict = {
  title: '代码',
  copy: '复制',
  copied: '已复制',
};

/** English. */
export const en: LocaleDict = {
  title: 'Code',
  copy: 'Copy',
  copied: 'Copied',
};
