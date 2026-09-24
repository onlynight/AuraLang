/**
 * scripts/bundle-grammar.mjs
 *
 * Turn the synced grammar into a TypeScript module the browser bundle can
 * import, in exactly the shape DSH's own language chunks use:
 *
 *   const e = Object.freeze(JSON.parse('{"name":"hat", ...}'));
 *
 * Using `JSON.parse(<string literal>)` rather than an object literal keeps the
 * JSON semantics byte-faithful (no key-order drift, no escaping surprises) and
 * is safe even though the grammar contains `${` inside regex patterns.
 *
 * Usage: node scripts/bundle-grammar.mjs
 */

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '..');

const IN = resolve(ROOT, 'grammar/hat.tmLanguage.json');
const OUT = resolve(ROOT, 'src/hat-grammar.ts');

const BANNER = `/**
 * GENERATED FILE - do not edit by hand.
 *
 * Produced by scripts/bundle-grammar.mjs from grammar/hat.tmLanguage.json,
 * which scripts/sync-grammar.mjs copies from the HAT VSCode extension
 * (\`syntaxes/hat.tmLanguage.json\`, located by scripts/_grammar-source.mjs).
 *
 * Regenerate with:
 *   node scripts/sync-grammar.mjs && node scripts/bundle-grammar.mjs
 *
 * The \`JSON.parse(string literal)\` form mirrors DSH's own language chunks
 * (dsh-web-frontend/dist/assets/langs/*.js) so the bundle behaves identically.
 */

import type { TextmateGrammar } from './types';

 `;

async function main() {
  const text = await readFile(IN, 'utf8');
  const grammar = JSON.parse(text);

  // `fileTypes` may be a bare string in VSCode grammars; Shiki accepts both, but
  // a stable array keeps the bundled shape predictable.
  const bundled = {
    ...grammar,
    fileTypes: Array.isArray(grammar.fileTypes) ? grammar.fileTypes : [grammar.fileTypes],
  };

  // JSON.stringify yields a double-quoted, fully escaped JS string literal.
  const literal = JSON.stringify(bundled);
  const body = BANNER +
    `/** The HAT TextMate grammar, frozen: Shiki reads it, nobody writes it. */\n` +
    `export const HAT_GRAMMAR = Object.freeze(\n` +
    `  JSON.parse(${JSON.stringify(literal)}) as TextmateGrammar,\n` +
    `);\n`;

  await mkdir(dirname(OUT), { recursive: true });
  await writeFile(OUT, body, 'utf8');

  console.log(
    `[bundle-grammar] ${IN}\n` +
      `[bundle-grammar]   rules  ${Object.keys(bundled.repository ?? {}).length}\n` +
      `[bundle-grammar]   json   ${literal.length} bytes\n` +
      `[bundle-grammar]   -> ${OUT}`,
  );
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
