/**
 * The Aura document body: an incrementally highlighted code view for the
 * `sidebar.right.tab.document` slot.
 *
 * Rendering is progressive in two senses:
 *
 *   - Plain text appears the instant content arrives; tokens are layered on as
 *     the highlighter warms up.
 *   - While the file streams in, only the newly appended tail is tokenized,
 *     carrying Shiki's `grammarState` across frames (incremental.ts).
 *
 * Nothing here throws into the preview pane. A highlighter that fails to build
 * falls back to the regex tokenizer in fallback.ts, and a tokenizer that throws
 * renders plain text.
 */

import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import type {
  DocumentContent,
  DocumentPreviewProps,
} from '@deepseek-ai/dsh-client-ui-sidebar-documentpreview/client';
import type { PropsLocale } from '@deepseek-ai/dsh-client-ui-slots';

import './declarations.js';

import type { AuraHighlighter } from './highlighter.js';
import { getHighlighter, resolveThemeMode } from './highlighter.js';
import { tokenizeFallback } from './fallback.js';
import {
  createIncrementalState,
  tokenizeNextChunk,
  type IncrementalState,
} from './incremental.js';
import { LOCALE_NAMESPACE } from './locale.js';
import {
  AURA_DARK_PALETTE,
  AURA_LIGHT_PALETTE,
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

/** Hard render cap; the owner paginates, this is a safety net. */
const MAX_LINES = 50_000;

export type AuraCodeBodyProps = DocumentPreviewProps &
  PropsLocale<typeof LOCALE_NAMESPACE>;

export function AuraCodeBody({
  content,
  wrap,
  scrollportRef,
  t,
}: AuraCodeBodyProps): ReactNode {
  const [phase, setPhase] = useState<Phase>('warming');
  const [lines, setLines] = useState<CodeLines>(null);
  const [dark, setDark] = useState(() => readDark());
  const [copied, setCopied] = useState(false);

  const highlighterRef = useRef<AuraHighlighter | null>(null);
  const stateRef = useRef<IncrementalState>(createIncrementalState());

  const text: string = content.kind === 'text' ? content.text : '';
  const isText = content.kind === 'text';

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

  // Tokenize. Coalesced to one rAF per frame.
  useEffect(() => {
    if (!isText) {
      setLines(null);
      stateRef.current = createIncrementalState();
      return;
    }

    if (phase === 'failed') {
      const palette = dark ? AURA_DARK_PALETTE : AURA_LIGHT_PALETTE;
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

    let raf = 0;
    let cancelled = false;
    raf = requestAnimationFrame(() => {
      if (cancelled) return;
      try {
        const theme = THEME_BY_MODE[dark ? 'dark' : 'light'];
        const result = tokenizeNextChunk(
          highlighter,
          text,
          stateRef.current,
          { chars: FRAME_CHARS, lines: FRAME_LINES },
          theme,
        );
        setLines(capLines(result.lines));
      } catch {
        // Never throw into the preview pane.
        const palette = dark ? AURA_DARK_PALETTE : AURA_LIGHT_PALETTE;
        setLines(tokenizeFallback(text, palette));
      }
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
    };
  }, [text, phase, dark, isText]);

  const onCopy = (): void => {
    void copyText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    });
  };

  if (!isText) return null;

  const rawLines: readonly string[] =
    lines === null ? text.split('\n') : lines.map((line) => joinLine(line));

  return (
    <div data-aura-code data-wrap={wrap ? 'true' : 'false'}>
      <div data-aura-banner>
        <span data-aura-lang>aura</span>
        <button
          type="button"
          data-aura-copy
          data-copied={copied ? 'true' : 'false'}
          onClick={onCopy}
        >
          {copied ? t('copied') : t('copy')}
        </button>
      </div>
      <div data-aura-scroll ref={scrollportRef}>
        <div data-aura-grid>
          {rawLines.map((lineText, index) => {
            const lineTokens = lines?.[index] ?? null;
            return (
              <FragmentPair
                key={index}
                num={String(index + 1)}
                tokens={lineTokens}
                raw={lineText}
              />
            );
          })}
        </div>
        {rawLines.length === 0 ? <div data-aura-empty> </div> : null}
      </div>
    </div>
  );
}

/** One gutter cell and one line cell. */
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
      <span data-aura-num>{num}</span>
      <span data-aura-text>{tokens === null ? raw : renderTokens(tokens)}</span>
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

/** Rejoin a token line into text for the wrap/no-highlight path. */
function joinLine(tokens: CodeToken[]): string {
  let out = '';
  for (const token of tokens) out += token.content;
  return out;
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
