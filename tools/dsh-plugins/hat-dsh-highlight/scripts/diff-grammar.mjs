/**
 * scripts/diff-grammar.mjs
 *
 * Drift check for the synced HAT grammar. Answers three questions, in order:
 *
 *   1. Upstream drift   - has the VSCode extension's grammar changed since the
 *                         last `sync-grammar` run? (fingerprint vs SYNC-SHA256)
 *   2. Local drift      - has `grammar/hat.tmLanguage.json` been hand-edited
 *                         away from what normalizing upstream would produce?
 *   3. Theme coverage   - does every scope the grammar emits have a matching
 *                         rule in `src/theme.ts`? A renamed scope that no rule
 *                         covers renders in the default foreground, i.e. it
 *                         silently loses its highlight. That is the exact
 *                         failure mode this check exists to catch.
 *
 * Exit code is non-zero on any drift, so `pnpm run check:grammar` and the
 * `prepublishOnly` hook can gate on it.
 *
 * Usage: node scripts/diff-grammar.mjs
 */

import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { findGrammar } from './_grammar-source.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');

const LOCAL = resolve(ROOT, 'grammar/hat.tmLanguage.json');
const LOCAL_SHA = resolve(ROOT, 'grammar/SYNC-SHA256');
const THEME = resolve(ROOT, 'src/theme.ts');

const LANGUAGE_ID = 'HAT';
const DISPLAY_NAME = 'HAT';

/** Same normalization as sync-grammar.mjs; kept in sync by the identity check. */
function normalize(grammar) {
  return { ...grammar, name: LANGUAGE_ID, displayName: DISPLAY_NAME };
}

/** sha256 of the raw upstream bytes, exactly as sync-grammar records it. */
function fingerprint(text) {
  return createHash('sha256').update(text).digest('hex');
}

/**
 * Collect every scope name the grammar can emit (`"name": "x.hat"` and
 * `"name"` inside `captures`), de-duplicated and sorted.
 * @param {unknown} node - any JSON value.
 * @param {Set<string>} out - accumulator.
 */
function collectScopes(node, out) {
  if (Array.isArray(node)) {
    for (const item of node) collectScopes(item, out);
    return out;
  }
  if (node && typeof node === 'object') {
    for (const [key, value] of Object.entries(node)) {
      if (key === 'name' && typeof value === 'string' && value.endsWith('.hat')) {
        out.add(value);
      } else {
        collectScopes(value, out);
      }
    }
  }
  return out;
}

/**
 * Extract the scope selectors from the theme's rule table.
 *
 * The table is a plain literal (`{ scope: 'x', key: 'y' }` / `scope: ['a', 'b']`),
 * so a source scan is enough and avoids importing TypeScript here.
 * @param {string} source - contents of src/theme.ts.
 * @returns {string[]} every declared scope selector.
 */
function themeScopes(source) {
  const out = new Set();
  const literal = /scope:\s*'([^']+)'/g;
  for (const match of source.matchAll(literal)) out.add(match[1]);
  const list = /scope:\s*\[([^\]]+)\]/g;
  for (const match of source.matchAll(list)) {
    for (const item of match[1].matchAll(/'([^']+)'/g)) out.add(item[1]);
  }
  return [...out];
}

/** Segment-wise prefix test, mirroring how Shiki/TextMate match theme rules. */
function covers(selector, scope) {
  const pattern = selector.split('.');
  const parts = scope.split('.');
  if (pattern.length > parts.length) return false;
  return pattern.every((segment, index) => segment === parts[index]);
}

/** Read the fingerprint recorded by the last sync. */
async function recordedFingerprint() {
  try {
    const text = await readFile(LOCAL_SHA, 'utf8');
    const line = text.split('\n', 1)[0].trim();
    return line.split(/\s+/)[0] || '';
  } catch {
    return '';
  }
}

async function main() {
  const problems = [];

  const { path: UPSTREAM } = findGrammar(ROOT);
  const upstreamText = await readFile(UPSTREAM, 'utf8');
  const upstreamHash = fingerprint(upstreamText);
  const upstream = JSON.parse(upstreamText);
  const localText = await readFile(LOCAL, 'utf8');
  const local = JSON.parse(localText);
  const recorded = await recordedFingerprint();

  // 1. Upstream drift ------------------------------------------------------
  if (upstreamHash !== recorded) {
    problems.push(
      `upstream drift: ${UPSTREAM}\n` +
        `      recorded ${recorded || '(none)'}\n` +
        `      current  ${upstreamHash}\n` +
        `      -> run: node scripts/sync-grammar.mjs && node scripts/bundle-grammar.mjs`,
    );
  }

  // 2. Local drift ---------------------------------------------------------
  const expected = `${JSON.stringify(normalize(upstream), null, 2)}\n`;
  if (expected !== localText) {
    problems.push(
      'local drift: grammar/hat.tmLanguage.json is not the normalized upstream\n' +
        '      -> do not hand-edit it; re-run node scripts/sync-grammar.mjs',
    );
  }

  // 3. Theme coverage ------------------------------------------------------
  const scopes = [...collectScopes(local, new Set())].sort();
  const selectors = themeScopes(await readFile(THEME, 'utf8'));
  const uncovered = scopes.filter((scope) => !selectors.some((s) => covers(s, scope)));
  if (uncovered.length > 0) {
    problems.push(
      'theme coverage: src/theme.ts has no rule for\n' +
        uncovered.map((scope) => `      ${scope}`).join('\n'),
    );
  }

  const ruleCount = Object.keys(local.repository ?? {}).length;
  console.log(`[diff-grammar] upstream  ${UPSTREAM}`);
  console.log(`[diff-grammar] sha256    ${upstreamHash}`);
  console.log(`[diff-grammar] rules     ${ruleCount}`);
  console.log(`[diff-grammar] scopes    ${scopes.length} (${selectors.length} theme selectors)`);

  if (problems.length > 0) {
    console.error('\n[diff-grammar] FAIL\n');
    for (const problem of problems) console.error(`  - ${problem}`);
    process.exitCode = 1;
    return;
  }
  console.log('[diff-grammar] OK: upstream, local copy and theme coverage all in sync');
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
