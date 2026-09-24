/**
 * Decode a `bytes-complete` document into renderable source text.
 *
 * HAT is a compiler IR, and an IR dump is commonly a textual header followed by
 * a binary payload. DSH's *paged* text read refuses any page whose text contains
 * a NUL byte (`workspace-file/not-text`) and its filesystem backend rejects
 * malformed UTF-8, so such a file cannot be read at all while the owner streams
 * pages: the pane reports a non-text file before any renderer is consulted.
 * Declaring `loading: 'bytes-complete'` routes the same file through here
 * instead, where it decodes:
 *
 *   - malformed UTF-8 becomes U+FFFD instead of failing the preview;
 *   - NUL bytes are dropped, so the readable text around a binary payload
 *     survives rather than taking the whole document down with it;
 *   - a leading BOM is consumed by the decoder.
 *
 * @module
 */

/** The byte DSH's text reader refuses a page over. */
const NUL = '\u0000';

/** Built on first use, so importing this module has no global prerequisites. */
let decoder: TextDecoder | undefined;

/**
 * Decode raw document bytes into text the viewer can render and copy.
 *
 * Malformed input degrades to U+FFFD; it is never a reason to fail a preview.
 *
 * @param data - complete file bytes, as the document owner hands them over.
 * @returns UTF-8 text with NUL bytes removed.
 */
export function decodeHatSource(data: Uint8Array): string {
  decoder ??= new TextDecoder('utf-8', { fatal: false });
  return decoder.decode(data).split(NUL).join('');
}
