/**
 * The HAT document body: an incrementally highlighted code view for the
 * `sidebar.right.tab.document` slot.
 *
 * Rendering is progressive in two senses:
 *
 *   - Plain text appears the instant content arrives; tokens are layered on as
 *     the highlighter warms up.
 *   - The source is tokenized in budgeted chunks over successive frames,
 *     carrying Shiki's `grammarState` from one chunk to the next
 *     (incremental.ts), so a large dump never blocks a frame.
 *
 * Delivery is `text-pages`, the owner's ordinary path for code: the text
 * arrives as an accumulated prefix that grows page by page. The body also
 * renders `bytes-complete` content, which is what an owner delivers when a
 * registration asks for complete bytes; `decode.ts` turns either shape into the
 * same source string.
 *
 * Nothing here throws into the preview pane. A highlighter that fails to build
 * falls back to the regex tokenizer in fallback.ts, and a tokenizer that throws
 * renders plain text.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import type {
  DocumentContent,
  DocumentPreviewProps,
} from '@deepseek-ai/dsh-client-ui-sidebar-documentpreview/client';
import type { PropsLocale } from '@deepseek-ai/dsh-client-ui-slots';

import './declarations.js';

import type { HATHighlighter } from './highlighter.js';
import { getHighlighter, resolveThemeMode } from './highlighter.js';
import { decodeHatSource } from './decode.js';
import { tokenizeFallback } from './fallback.js';
import {
  createIncrementalState,
  tokenizeNextChunk,
  type IncrementalState,
} from './incremental.js';
import { LOCALE_NAMESPACE } from './locale.js';
import {
  HAT_DARK_PALETTE,
  HAT_LIGHT_PALETTE,
  THEME_BY_MODE,
} from './theme.js';

/** Tokens as Shiki emits them, flattened to the fields this viewer reads. */
interface CodeToken {
  readonly content: string;
  readonly color?: string;
  readonly fontStyle?: number;
}

/** Lines of tokens; `null` means "render the raw text". */
type CodeLines = CodeToken[][] | null;

/** Highlighter lifecycle inside the component. */
type Phase = 'warming' | 'ready' | 'failed';

/** Per-frame tokenization budget. */
const FRAME_CHARS = 32 * 1024;
const FRAME_LINES = 4000;

/**
 * Hard render cap, the safety net behind the tokenizer budget.
 */
const MAX_LINES = 500_000;

export type HatCodeBodyProps = DocumentPreviewProps &
  PropsLocale<typeof LOCALE_NAMESPACE>;

export function HatCodeBody({
  content,
  wrap,
  scrollportRef,
  t,
}: HatCodeBodyProps): ReactNode {
  const [phase, setPhase] = useState<Phase>('warming');
  const [lines, setLines] = useState<CodeLines>(null);
  const [dark, setDark] = useState(() => readDark());
  const [copied, setCopied] = useState(false);

  const highlighterRef = useRef<HATHighlighter | null>(null);
  const stateRef = useRef<IncrementalState>(createIncrementalState());

  // Both delivery modes end up as one string. `content.data` keeps its identity
  // while the tab holds the file, so the decode runs once per load rather than
  // once per render.
  const source: string | Uint8Array =
    content.kind === 'text' ? content.text : content.data;
  const text: string = useMemo(
    () => (typeof source === 'string' ? source : decodeHatSource(source)),
    [source],
  );

  // Warm the highlighter once per mount.
  useEffect(() => {
    let alive = true;
    getHighlighter()
      .then((highlighter) => {
        if (!alive) return;
        highlighterRef.current = highlighter;
        setPhase('ready');
      })
      .catch(() => {
        if (alive) setPhase('failed');
      });
    return () => {
      alive = false;
    };
  }, []);

  // Follow the host's light/dark switch, re-tokenizing on change.
  useEffect(() => {
    const observer = new MutationObserver(() => {
      const next = readDark();
      setDark((prev) => (prev === next ? prev : next));
    });
    observer.observe(document.body, { attributes: true, attributeFilter: ['data-ds-dark-theme'] });
    return () => observer.disconnect();
  }, []);

  // Tokenize. `tokenizeNextChunk` is deliberately budget-limited so a frame
  // never blocks on a huge file, which means one call rarely finishes the
  // document. Drive it with a self-rescheduling rAF loop until it reports
  // `complete`: the effect only re-runs when the source text changes, so after
  // the stream reaches EOF a single call would leave the tail un-tokenized and
  // permanently absent from the render.
  useEffect(() => {
    if (phase === 'failed') {
      // The regex tokenizer is synchronous and covers the whole document, so no
      // continuation loop is needed here.
      const palette = dark ? HAT_DARK_PALETTE : HAT_LIGHT_PALETTE;
      setLines(tokenizeFallback(text, palette));
      return;
    }
    if (phase === 'warming') {
      setLines(null);
      return;
    }

    const highlighter = highlighterRef.current;
    if (highlighter === null) {
      setLines(null);
      return;
    }

    let cancelled = false;
    let raf = 0;
    const theme = THEME_BY_MODE[dark ? 'dark' : 'light'];

    const step = (): void => {
      if (cancelled) return;
      try {
        const result = tokenizeNextChunk(
          highlighter,
          text,
          stateRef.current,
          { chars: FRAME_CHARS, lines: FRAME_LINES },
          theme,
        );
        setLines(capLines(result.lines));
        if (!result.complete) raf = requestAnimationFrame(step);
      } catch {
        // Never throw into the preview pane.
        const palette = dark ? HAT_DARK_PALETTE : HAT_LIGHT_PALETTE;
        setLines(tokenizeFallback(text, palette));
      }
    };

    raf = requestAnimationFrame(step);
    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
    };
  }, [text, phase, dark]);

  const onCopy = (): void => {
    void copyText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    });
  };

  // The full source, always. Tokenized lines are layered on top of the raw
  // lines by index, so a document that is still mid-tokenization shows its
  // entire text with only the tokenized prefix colored rather than ending
  // early at the tokenizer's frontier.
  const sourceLines = useMemo<readonly string[]>(
    () => text.split('\n', MAX_LINES),
    [text],
  );

  return (
    <div data-hat-code data-code-preview data-wrap={wrap ? 'true' : 'false'}>
      <div data-hat-banner>
        <span data-hat-lang>HAT</span>
        <button
          type="button"
          data-hat-copy
          data-copied={copied ? 'true' : 'false'}
          onClick={onCopy}
        >
          {copied ? t('copied') : t('copy')}
        </button>
      </div>
      <div data-hat-scroll ref={scrollportRef}>
        <div data-hat-grid>
          {sourceLines.map((lineText, index) => (
            <FragmentPair
              key={index}
              num={String(index + 1)}
              tokens={lines?.[index] ?? null}
              raw={lineText}
            />
          ))}
        </div>
        {sourceLines.length === 0 ? <div data-hat-empty> </div> : null}
      </div>
    </div>
  );
}

/**
 * One gutter cell and one line cell.
 *
 * `data-textpreview-line` is the host's line-anchor contract:
 * `ui-sidebar-documentpreview` locates `[data-textpreview-line="N"]` inside the
 * scrollport to honour `?line=` deep links, so omitting it silently breaks
 * navigation from chat mentions to a specific line.
 */
function FragmentPair({
  num,
  tokens,
  raw,
}: {
  readonly num: string;
  readonly tokens: CodeToken[] | null;
  readonly raw: string;
}): ReactNode {
  return (
    <>
      <span data-hat-num data-textpreview-line={num}>{num}</span>
      <span data-hat-text>{tokens === null ? raw : renderTokens(tokens)}</span>
    </>
  );
}

/** Render a line's tokens as styled spans. */
function renderTokens(tokens: CodeToken[]): ReactNode {
  if (tokens.length === 0) return '';
  return tokens.map((token, index) => {
    const style: Record<string, string> | undefined = token.color === undefined
      ? undefined
      : token.fontStyle === 1
        ? { color: token.color, fontStyle: 'italic' }
        : token.fontStyle === 2
          ? { color: token.color, fontWeight: '600' }
          : token.fontStyle === 3
            ? { color: token.color, fontStyle: 'italic', fontWeight: '600' }
            : { color: token.color };
    return (
      <span key={index} style={style}>
        {token.content}
      </span>
    );
  });
}

/** Cap the rendered line count. */
function capLines(lines: CodeToken[][]): CodeToken[][] {
  return lines.length > MAX_LINES ? lines.slice(0, MAX_LINES) : lines;
}

/** Read the host's dark-mode flag. */
function readDark(): boolean {
  if (typeof document === 'undefined') return false;
  return document.body.hasAttribute('data-ds-dark-theme');
}

/** Copy to the clipboard, falling back to a hidden textarea. */
async function copyText(text: string): Promise<void> {
  const nav = navigator as Navigator & {
    clipboard?: { writeText?: (value: string) => Promise<void> };
  };
  if (nav.clipboard?.writeText) {
    await nav.clipboard.writeText(text);
    return;
  }
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.top = '0';
  area.style.left = '0';
  area.style.opacity = '0';
  document.body.appendChild(area);
  area.select();
  try {
    document.execCommand('copy');
  } finally {
    document.body.removeChild(area);
  }
}
