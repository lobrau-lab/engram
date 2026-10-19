import assert from 'node:assert/strict';

const saved = JSON.stringify({ settings: { model: 'synthetic-model', apiKey: 'legacy-session-key' }, conversations: [], activeId: null });
globalThis.localStorage = {
  data: new Map([['threadline.workspace.v1', saved]]),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); }
};

const { getWorkspace, getSessionApiKey } = await import('../src/core/store.js');
const settings = getWorkspace().settings;
assert.equal(settings.model, 'synthetic-model');
assert.equal(settings.baseUrl, 'http://localhost:8000/v1');
assert.equal(settings.contextLimit, 8192);
assert.equal(getSessionApiKey(), 'legacy-session-key');
assert.equal(Object.hasOwn(settings, 'apiKey'), false);
assert.equal(Object.hasOwn(JSON.parse(localStorage.getItem('threadline.workspace.v1')).settings, 'apiKey'), false);
console.log('store migration tests passed');
