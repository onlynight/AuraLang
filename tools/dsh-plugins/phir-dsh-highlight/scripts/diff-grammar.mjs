/**
 * scripts/diff-grammar.mjs
 *
 * CI guard: prove the committed grammar still matches the VSCode extension.
 *
 * "Referencing the VSCode plugin's highlighting" must never drift silently.
 * This re-runs the same normalization the sync script applies to the live
 * upstream file and compares it to the committed grammar, field by field.
 *
 * Exit codes: 0 = in sync, 1 = drifted (with a repair hint).
 *
 * Usage: node scripts/diff-grammar.mjs
 */

import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { findGrammar } from './_grammar-source.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');

const { path: UPSTREAM } = findGrammar(ROOT);
const LOCAL = resolve(ROOT, 'grammar/phir.tmLanguage.json');

const LANGUAGE_ID = 'PHIR';
const DISPLAY_NAME = 'PHIR';

function normalize(grammar) {
  return { ...grammar, name: LANGUAGE_ID, displayName: DISPLAY_NAME };
}

function diff(a, b, path, problems) {
  const keys = new Set([...Object.keys(a ?? {}), ...Object.keys(b ?? {})]);
  for (const key of keys) {
    const here = `${path}.${key}`;
    const left = a?.[key];
    const right = b?.[key];
    if (Array.isArray(left) || Array.isArray(right)) {
      if (left.length !== right.length) {
        problems.push(`${here}: array length ${left.length} != ${right.length}`);
        continue;
      }
      left.forEach((value, index) => diff(value, right[index], `${here}[${index}]`, problems));
      continue;
    }
    if (left && right && typeof left === 'object' && typeof right === 'object') {
      diff(left, right, here, problems);
      continue;
    }
    if (left !== right) problems.push(`${here}: ${JSON.stringify(left)} != ${JSON.stringify(right)}`);
  }
}

async function main() {
  let upstream;
  let local;
  try {
    upstream = JSON.parse(await readFile(UPSTREAM, 'utf8'));
  } catch (error) {
    console.error(
      `[diff-grammar] cannot read upstream grammar at ${UPSTREAM}\n` +
        `[diff-grammar]   ${error.message}\n` +
        `[diff-grammar]   Run this from a full phirLang checkout, or run\n` +
        `[diff-grammar]   ` +
        '`node scripts/sync-grammar.mjs && node scripts/bundle-grammar.mjs` first.',
    );
    process.exitCode = 1;
    return;
  }
  try {
    local = JSON.parse(await readFile(LOCAL, 'utf8'));
  } catch (error) {
    console.error(
      `[diff-grammar] cannot read committed grammar at ${LOCAL}\n` +
        `[diff-grammar]   ${error.message}\n` +
        `[diff-grammar]   Run \`node scripts/sync-grammar.mjs && node scripts/bundle-grammar.mjs\`.`,
    );
    process.exitCode = 1;
    return;
  }

  const expected = normalize(upstream);
  const actual = normalize(local);
  const problems = [];
  diff(expected, actual, 'grammar', problems);

  if (problems.length === 0) {
    console.log(
      `[diff-grammar] in sync: grammar/phir.tmLanguage.json matches\n` +
        `[diff-grammar]   upstream ${UPSTREAM}\n` +
        `[diff-grammar]   rules    ${Object.keys(expected.repository ?? {}).length}`,
    );
    return;
  }

  console.error(
    `[diff-grammar] DRIFT detected (${problems.length} difference${problems.length === 1 ? '' : 's'}):\n` +
      problems.slice(0, 40).map((problem) => `[diff-grammar]   - ${problem}`).join('\n') +
      (problems.length > 40
        ? `\n[diff-grammar]   ... and ${problems.length - 40} more`
        : '') +
      `\n[diff-grammar] Repair: node scripts/sync-grammar.mjs && node scripts/bundle-grammar.mjs`,
  );
  process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});




