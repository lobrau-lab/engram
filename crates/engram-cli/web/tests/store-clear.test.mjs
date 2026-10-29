import assert from 'node:assert/strict';

globalThis.localStorage = {
  data: new Map(),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); }
};

const { clearWorkspace, getWorkspace, updateSettings } = await import('../src/core/store.js');
updateSettings({ model: 'mutated-model' });
assert.equal(getWorkspace().settings.model, 'mutated-model');

clearWorkspace();
assert.equal(getWorkspace().settings.model, '');
assert.equal(getWorkspace().conversations.length, 0);
console.log('store clear tests passed');
