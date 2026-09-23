/**
 * scripts/_grammar-source.mjs
 *
 * Shared grammar-source resolution for the sync and drift-check scripts.
 *
 * The VSCode extension is the single source of truth for the Aura grammar. This
 * resolver finds it without depending on a fixed checkout depth:
 *
 *   1. `AURA_GRAMMAR_PATH` (env) - explicit override, wins unconditionally.
 *   2. Walk upward from this plugin until
 *      `ide-extension/vscode-extension/syntaxes/aura.tmLanguage.json` is found.
 *
 * Imported by scripts/sync-grammar.mjs and scripts/diff-grammar.mjs.
 */

import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const RELATIVE = ['ide-extension', 'vscode-extension', 'syntaxes', 'aura.tmLanguage.json'];

/**
 * Locate the upstream grammar.
 * @param {string} startDir - directory to walk up from (this plugin's root).
 * @returns {{path: string, root: string}} grammar path and the checkout root it came from.
 */
function findGrammar(startDir) {
  const override = process.env.AURA_GRAMMAR_PATH;
  if (override) {
    const path = resolve(override);
    if (!existsSync(path)) {
      throw new Error(`AURA_GRAMMAR_PATH points at a missing file: ${path}`);
    }
    return { path, root: dirname(dirname(dirname(dirname(path)))) };
  }

  let dir = resolve(startDir);
  for (let depth = 0; depth < 8; depth += 1) {
    const candidate = join(dir, ...RELATIVE);
    if (existsSync(candidate)) return { path: candidate, root: dir };
    const parent = dirname(dir);
    if (parent === dir) break; // filesystem root reached
    dir = parent;
  }
  throw new Error(
    'Cannot locate ide-extension/vscode-extension/syntaxes/aura.tmLanguage.json ' +
      `above ${resolve(startDir)}.\n` +
      'Set AURA_GRAMMAR_PATH=/path/to/aura.tmLanguage.json to override.',
  );
}

export { findGrammar };
