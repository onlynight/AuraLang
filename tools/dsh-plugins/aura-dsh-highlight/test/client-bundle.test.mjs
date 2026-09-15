/**
 * Bundle-contract test: the client half must materialize exactly what DSH's
 * module loader expects - a `factory` returning an object with `apply` and
 * `inject` - and it must only `require` modules the platform seed provides.
 *
 * This is the seam a plugin breaks most easily: a bundle that forgets to
 * `return module.exports`, or one that accidentally pulls in a Node-only
 * dependency, passes every unit test and then dies silently in the browser.
 */

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { createContext, runInContext } from 'node:vm';
import test from 'node:test';

const here = dirname(fileURLToPath(import.meta.url));
const bundlePath = resolve(here, '..', 'lib', 'client.js');
const bundle = readFileSync(bundlePath, 'utf8');

/** Require the real packages the bundle depends on, from the plugin's tree. */
const localRequire = createRequire(resolve(here, '..', 'package.json'));

test('client bundle: loads as a DSH lazy module and exports apply + inject', () => {
  const registrations = [];
  const requireCounts = new Map();

  const require = (specifier) => {
    requireCounts.set(specifier, (requireCounts.get(specifier) ?? 0) + 1);
    return localRequire(specifier);
  };

  const sandbox = {
    window: {
      __ModuleLoader__: {
        load: (descriptor) => {
          registrations.push(descriptor);
        },
      },
    },
    console,
  };
  sandbox.globalThis = sandbox;
  createContext(sandbox);

  runInContext(bundle, sandbox, { filename: 'client.js' });

  assert.equal(registrations.length, 1, 'registers exactly one module');
  const { id, factory } = registrations[0];
  assert.equal(id, '@aura-lang/dsh-highlight-aura');
  assert.equal(typeof factory, 'function');

  const exports = factory(require);
  assert.equal(typeof exports.apply, 'function', 'exports.apply');
  assert.ok(Array.isArray(exports.inject), 'exports.inject is an array');
  assert.ok(
    exports.inject.includes('documentPreviews'),
    `waits on documentPreviews (${exports.inject.join(', ')})`,
  );
  assert.ok(
    exports.inject.includes('slots') && exports.inject.includes('locale'),
    'waits on the slot and locale services',
  );
});

test('client bundle: only requires platform-seeded modules', () => {
  const required = new Set();
  const registrations = [];
  const require = (specifier) => {
    required.add(specifier);
    return localRequire(specifier);
  };
  const sandbox = {
    window: { __ModuleLoader__: { load: (descriptor) => registrations.push(descriptor) } },
    console,
  };
  sandbox.globalThis = sandbox;
  createContext(sandbox);
  runInContext(bundle, sandbox, { filename: 'client.js' });
  const exports = registrations[0].factory(require);
  // Touch apply so any lazily-evaluated export is exercised too.
  assert.equal(typeof exports.apply, 'function');

  const expected = new Set(['react', 'react/jsx-runtime']);
  for (const specifier of required) {
    assert.ok(
      expected.has(specifier),
      `bundle requires "${specifier}", which the platform seed does not provide`,
    );
  }
});

test('client bundle: apply runs against a mock context and registers everything', () => {
  const registrations = [];
  const require = (specifier) => localRequire(specifier);
  const sandbox = {
    window: { __ModuleLoader__: { load: (descriptor) => registrations.push(descriptor) } },
    document: {
      head: { appendChild() {} },
      createElement: () => ({ setAttribute() {}, remove() {}, style: {} }),
      body: { hasAttribute: () => false },
    },
    console,
    MutationObserver: class {
      observe() {}
      disconnect() {}
    },
    navigator: {},
  };
  sandbox.globalThis = sandbox;
  createContext(sandbox);
  runInContext(bundle, sandbox, { filename: 'client.js' });
  const { apply } = registrations[0].factory(require);

  const effects = [];
  const ctx = {
    effect(effect, label) {
      effects.push({ label, value: effect() });
    },
    reflect: {},
    locale: {
      register() {
        return () => {};
      },
      bind: () => (key) => key,
    },
    slots: {
      inject: () => {
        return () => {};
      },
      register() {
        return () => {};
      },
    },
    documentPreviews: {
      register(definition) {
        assert.equal(definition.priority, 'extension');
        assert.equal(JSON.stringify(definition.extensions), '["aura"]');
        assert.equal(definition.loading, 'text-pages');
        assert.equal(definition.wrap, true);
        assert.equal(typeof definition.title, 'function');
        assert.equal(typeof definition.id, 'string');
        return () => {};
      },
    },
  };

  apply(ctx);

  assert.ok(effects.length >= 4, `registered ${effects.length} effects`);
  const labels = effects.map((entry) => entry.label);
  for (const expected of [
    'aura-highlight: dictionaries',
    'aura-highlight: preview definition',
    'aura-highlight: document body',
  ]) {
    assert.ok(labels.includes(expected), `has effect "${expected}" (${labels.join(' | ')})`);
  }
});

test('client bundle: apply fails loud when a service is missing', () => {
  const registrations = [];
  const require = (specifier) => localRequire(specifier);
  const sandbox = {
    window: { __ModuleLoader__: { load: (descriptor) => registrations.push(descriptor) } },
    console,
  };
  sandbox.globalThis = sandbox;
  createContext(sandbox);
  runInContext(bundle, sandbox, { filename: 'client.js' });
  const { apply } = registrations[0].factory(require);

  assert.throws(
    () => apply({ effect: () => {}, locale: { register() {}, bind() { return () => 'x'; } } }),
    /missing the "slots" service/,
    'names the first missing capability',
  );
});

