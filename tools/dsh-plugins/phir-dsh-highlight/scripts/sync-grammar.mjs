/**
 * scripts/sync-grammar.mjs
 *
 * Sync the PHIR TextMate grammar from the VSCode extension into this plugin.
 *
 * The VSCode extension stays the single source of truth. This script is the only
 * place that touches the grammar: it copies the file, applies two semantic
 * no-ops required by Shiki, and records the upstream fingerprint.
 *
 * Normalization (exactly two fields, no semantic change):
 *   1. `name`      "PHIR" -> "PHIR"  Shiki derives the language id from `name`;
 *                                     every DSH language chunk uses a lowercase
 *                                     id, and a mismatch breaks
 *                                     `getLoadedLanguages()` matching.
 *   2. `displayName` absent -> "PHIR"  Cosmetic parity with DSH's own chunks.
 *
 * Outputs:
 *   grammar/phir.tmLanguage.json  the normalized grammar
 *   grammar/SYNC-SHA256           fingerprint of the untouched upstream file
 *
 * Usage: node scripts/sync-grammar.mjs
 */

import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { findGrammar } from './_grammar-source.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');

/** The VSCode extension copy, located independently of checkout depth. */
const { path: UPSTREAM, root: CHECKOUT } = findGrammar(ROOT);

const LANGUAGE_ID = 'PHIR';
const DISPLAY_NAME = 'PHIR';

const OUT = resolve(ROOT, 'grammar/phir.tmLanguage.json');
const OUT_SHA = resolve(ROOT, 'grammar/SYNC-SHA256');

/**
 * Apply the two normalizations. Throws on a grammar that is missing a field we
 * depend on, rather than silently producing a broken bundle.
 * @param {object} grammar - parsed upstream grammar.
 * @returns {object} a new grammar object.
 */
function normalize(grammar) {
  if (!grammar || typeof grammar !== 'object') {
    throw new Error('sync-grammar: upstream grammar is not a JSON object');
  }
  if (grammar.scopeName !== 'source.phir') {
    throw new Error(
      `sync-grammar: unexpected scopeName ${JSON.stringify(grammar.scopeName)} ` +
        `(expected "source.phir")`,
    );
  }
  if (!Array.isArray(grammar.patterns) || grammar.patterns.length === 0) {
    throw new Error('sync-grammar: upstream grammar declares no root patterns');
  }
  if (!grammar.repository || typeof grammar.repository !== 'object') {
    throw new Error('sync-grammar: upstream grammar has no repository');
  }
  return {
    ...grammar,
    name: LANGUAGE_ID,
    displayName: DISPLAY_NAME,
  };
}

async function main() {
  const raw = await readFile(UPSTREAM, 'utf8');
  const upstream = JSON.parse(raw);
  const fingerprint = createHash('sha256').update(raw).digest('hex');

  const next = normalize(upstream);
  // Two-space indent matches the upstream style, so `git diff` stays readable.
  const text = `${JSON.stringify(next, null, 2)}\n`;

  await mkdir(dirname(OUT), { recursive: true });
  await writeFile(OUT, text, 'utf8');
  await writeFile(
    OUT_SHA,
    `${fingerprint}  phir.tmLanguage.json\n` +
      `upstream      ${UPSTREAM}\n` +
      `checkout      ${CHECKOUT}\n` +
      `language id   ${LANGUAGE_ID}\n` +
      `displayName   ${DISPLAY_NAME}\n`,
    'utf8',
  );

  const ruleCount = Object.keys(next.repository).length;
  console.log(
    `[sync-grammar] ${UPSTREAM}\n` +
      `[sync-grammar]   sha256      ${fingerprint}\n` +
      `[sync-grammar]   rules       ${ruleCount}\n` +
      `[sync-grammar]   name        "${upstream.name}" -> "${LANGUAGE_ID}"\n` +
      `[sync-grammar]   -> ${OUT}\n` +
      `[sync-grammar]   -> ${OUT_SHA}`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});


