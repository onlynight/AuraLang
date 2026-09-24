/**
 * Build the plugin's distributable artifacts.
 *
 * Produces five files under `lib/`:
 *
 *   client.js      the browser half, wrapped in DSH's lazy-CJS
 *                  `window.__ModuleLoader__.load({id, factory})` envelope.
 *                  `react` and `react/jsx-runtime` stay external so the host's
 *                  own React instance is shared.
 *
 *   index.js       the Node-side package root (ESM).
 *
 *   highlight.js   the DOM-free highlighting API (ESM).
 *
 *   lib/types/     the declaration tree for the three entries above.
 *
 * Run `node scripts/build.mjs`. Add `--clean` to remove `lib/` first; the build
 * is already idempotent, so the flag only exists for parity with `build:clean`.
 *
 * The client bundle is written by hand rather than by esbuild alone because
 * DSH's module loader does not consume a bare CJS module: it wants the factory
 * envelope, and the envelope is what makes `require('react')` inside the bundle
 * resolve through DSH's seed table instead of the ambient loader.
 */

import {
  mkdirSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(here, '..');
const LIB = resolve(ROOT, 'lib');
const SRC = resolve(ROOT, 'src');

/** The module id the DSH loader keys this bundle by. */
const PLUGIN_ID = '@phir-lang/dsh-highlight-phir';

/** Modules served by the DSH platform seed; never bundled. */
const CLIENT_EXTERNAL = ['react', 'react/jsx-runtime'];

const args = new Set(process.argv.slice(2));
const CLEAN_FIRST = args.has('--clean');

function log(message) {
  process.stdout.write(message + '\n');
}

/** Run one labelled step, awaiting it. */
async function step(label, work) {
  const started = process.hrtime.bigint();
  log(`\n== ${label}`);
  await work();
  const seconds = Number(process.hrtime.bigint() - started) / 1e9;
  log(`   ${seconds.toFixed(2)}s`);
}

/**
 * Emit the declaration tree.
 *
 * `tsc` is invoked through node rather than through `.bin/tsc` so the script
 * has one process model and no Windows `.cmd` shell requirement; a failure is
 * reported with tsc's own stderr rather than swallowed.
 */
function emitDeclarations() {
  const tsc = resolve(ROOT, 'node_modules', 'typescript', 'bin', 'tsc');
  const result = spawnSync(
    process.execPath,
    [tsc, '--pretty', 'false', '-p', resolve(ROOT, 'tsconfig.build.json')],
    { cwd: ROOT, stdio: ['ignore', 'pipe', 'pipe'] },
  );
  if (result.error !== undefined) {
    throw new Error(`could not start tsc: ${result.error.message}`);
  }
  if (result.status !== 0) {
    const stderr = result.stderr?.toString('utf8') ?? '';
    throw new Error(`tsc failed (exit ${result.status}):\n${stderr.trim()}`);
  }
}

/** Build one entry with esbuild and return its output text. */
async function buildEntry(entry, options) {
  const { build } = await import('esbuild');
  const result = await build({
    absWorkingDir: ROOT,
    entryPoints: [resolve(SRC, entry)],
    bundle: true,
    write: false,
    logLevel: 'warning',
    format: options.format,
    platform: options.platform ?? 'browser',
    target: 'es2022',
    external: options.external ?? [],
    // An external map needs a path to reference; `outfile` supplies it even
    // when `write` is false.
    outfile: options.outfile,
    sourcemap: options.sourcemap ?? 'linked',
    define: { 'process.env.NODE_ENV': '"production"' },
    ...(options.jsx === 'automatic' ? { jsx: 'automatic' } : {}),
    ...options.extra,
  });
  const files = result.outputFiles;
  // esbuild returns the sourcemap before the code when `outfile` is set with a
  // linked map, so select by path rather than by index.
  const code = files.find((file) => !file.path.endsWith('.map'));
  const map = files.find((file) => file.path.endsWith('.map'));
  if (code === undefined) throw new Error(`esbuild produced no code output`);
  return { text: code.text, map: map === undefined ? undefined : map.text };
}

/** Wrap a CJS body in DSH's lazy-module envelope. */
function wrapClient(body) {
  const header = [
    'window.__ModuleLoader__.load({',
    `  id: ${JSON.stringify(PLUGIN_ID)},`,
    '  factory: (require) => {',
    '    var module = { exports: {} };',
    '    var exports = module.exports;',
    '    Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });',
  ].join('\n');
  const footer = ['    return module.exports;', '  }', '});'].join('\n');
  return `${header}\n${indent(body)}\n${footer}\n`;
}

/** Indent a block so the esbuild body reads as the factory body. */
function indent(source) {
  return source
    .split('\n')
    .map((line) => (line === '' ? line : `    ${line}`))
    .join('\n');
}

async function main() {
  if (CLEAN_FIRST) {
    log('cleaning lib/');
  }
  rmSync(LIB, { recursive: true, force: true });
  mkdirSync(LIB, { recursive: true });

  await step('declarations (tsc)', emitDeclarations);

  await step('client bundle', async () => {
    const { text } = await buildEntry('client.ts', {
      format: 'cjs',
      external: CLIENT_EXTERNAL,
      jsx: 'automatic',
      outfile: resolve(LIB, 'client.js'),
      // The map is skipped: the lazy-module envelope prepended below shifts
      // every line, so a "linked" map would point at the wrong source rows.
      sourcemap: false,
    });
    writeFileSync(resolve(LIB, 'client.js'), wrapClient(text), 'utf8');
  });

  await step('highlight entry', async () => {
    const { text, map } = await buildEntry('highlight/index.ts', {
      format: 'esm',
      outfile: resolve(LIB, 'highlight.js'),
    });
    writeFileSync(resolve(LIB, 'highlight.js'), text, 'utf8');
    if (map !== undefined) {
      writeFileSync(resolve(LIB, 'highlight.js.map'), map, 'utf8');
    }
  });

  await step('package root', async () => {
    const { text, map } = await buildEntry('index.ts', {
      format: 'esm',
      outfile: resolve(LIB, 'index.js'),
    });
    writeFileSync(resolve(LIB, 'index.js'), text, 'utf8');
    if (map !== undefined) {
      writeFileSync(resolve(LIB, 'index.js.map'), map, 'utf8');
    }
  });

  log('\nartifacts:');
  for (const entry of walk(LIB)) {
    log(`  ${String(statSync(entry).size).padStart(8)}  ${entry.slice(LIB.length + 1)}`);
  }
}

/** List emitted files, declaration tree last. */
function walk(dir) {
  const files = [];
  for (const entry of readdirSync(dir)) {
    const full = resolve(dir, entry);
    if (statSync(full).isDirectory()) files.push(...walk(full));
    else files.push(full);
  }
  return files;
}

main().catch((error) => {
  console.error(error.stack ?? String(error));
  process.exitCode = 1;
});

