import assert from 'node:assert/strict';

globalThis.localStorage = {
  data: new Map([['threadline.workspace.v1', '{ invalid json }']]),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); }
};

const { loadWorkspace } = await import('../src/core/store.js');
const state = loadWorkspace();
assert.equal(Array.isArray(state.conversations), true);
assert.equal(state.settings.contextLimit, 8192);
console.log('store fallback tests passed');
