/**
 * scripts/_grammar-source.mjs
 *
 * Shared grammar-source resolution for the sync and drift-check scripts.
 *
 * The VSCode extension is the single source of truth for the HAT grammar. This
 * resolver finds it without depending on a fixed checkout depth:
 *
 *   1. `HAT_GRAMMAR_PATH` (env) - explicit override, wins unconditionally.
 *   2. Walk upward from this plugin until one of {@link RELOCATIONS} is found.
 *
 * Imported by scripts/sync-grammar.mjs and scripts/diff-grammar.mjs.
 */

import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

/** Known locations of the grammar relative to a checkout root. */
const RELOCATIONS = [
  ['ide-extension', 'hat-vscode-extension', 'syntaxes', 'hat.tmLanguage.json'],
];

/** How deep to walk before giving up. */
const MAX_DEPTH = 8;

/**
 * Locate the upstream grammar.
 * @param {string} startDir - directory to walk up from (this plugin's root).
 * @returns {{path: string, root: string}} grammar path and the checkout root it came from.
 */
function findGrammar(startDir) {
  const override = process.env.HAT_GRAMMAR_PATH;
  if (override) {
    const path = resolve(override);
    if (!existsSync(path)) {
      throw new Error(`HAT_GRAMMAR_PATH points at a missing file: ${path}`);
    }
    return { path, root: dirname(dirname(dirname(dirname(path)))) };
  }

  let dir = resolve(startDir);
  for (let depth = 0; depth < MAX_DEPTH; depth += 1) {
    for (const relative of RELOCATIONS) {
      const candidate = join(dir, ...relative);
      if (existsSync(candidate)) return { path: candidate, root: dir };
    }
    const parent = dirname(dir);
    if (parent === dir) break; // filesystem root reached
    dir = parent;
  }
  throw new Error(
    `Cannot locate the HAT grammar above ${resolve(startDir)}. Looked for:\n` +
      RELOCATIONS.map((relative) => `  ${relative.join('/')}`).join('\n') +
      '\nSet HAT_GRAMMAR_PATH=/path/to/hat.tmLanguage.json to override.',
  );
}

export { findGrammar };
