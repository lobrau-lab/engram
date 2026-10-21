import assert from 'node:assert/strict';

const WORKSPACE_KEY = 'threadline.workspace.v1';
const PENDING_KEY = 'threadline.workspace.v1.pending';
const saved = JSON.stringify({
  settings: { model: 'legacy-model', apiKey: 'legacy-session-key' },
  conversations: [{ id: 'legacy-conversation', title: 'Legacy', createdAt: new Date().toISOString(), turns: [] }],
  activeId: 'legacy-conversation'
});

globalThis.localStorage = {
  data: new Map([[WORKSPACE_KEY, saved]]),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); },
  removeItem(key) { this.data.delete(key); }
};

let putBody = null;
let conflict = false;
globalThis.fetch = async (_url, options = {}) => {
  if (!options.method || options.method === 'GET') {
    return new Response(JSON.stringify({ workspace: null, revision: null }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  }
  putBody = JSON.parse(options.body);
  if (conflict) {
    return new Response(JSON.stringify({ error: 'workspace changed in another tab', revision: 'r2' }), { status: 409, headers: { 'Content-Type': 'application/json' } });
  }
  return new Response(JSON.stringify({ ok: true, revision: 'r1' }), { status: 200, headers: { 'Content-Type': 'application/json' } });
};

const { hydrateWorkspace, updateSettings, flushWorkspace, getWorkspaceStatus, getSessionApiKey } = await import('../src/core/store.js');

const boot = await hydrateWorkspace();
assert.equal(boot.mode, 'ready');
assert.equal(boot.pending, false);
assert.equal(putBody.baseRevision, null);
assert.equal(putBody.workspace.schemaVersion, 1);
assert.equal(putBody.workspace.settings.model, 'legacy-model');
assert.equal(Object.hasOwn(putBody.workspace.settings, 'apiKey'), false);
assert.equal(JSON.stringify(putBody).includes('legacy-session-key'), false);
assert.equal(getSessionApiKey(), 'legacy-session-key');
assert.equal(localStorage.getItem(PENDING_KEY), null);

conflict = true;
updateSettings({ model: 'second-model' });
const failed = await flushWorkspace();
assert.equal(failed.mode, 'conflict');
assert.equal(failed.conflict, true);
assert.equal(failed.pending, true);
assert.equal(failed.revision, 'r2');
assert.equal(localStorage.getItem(PENDING_KEY), '1');
assert.equal(putBody.workspace.settings.model, 'second-model');
assert.equal(Object.hasOwn(putBody.workspace.settings, 'apiKey'), false);
assert.equal(getWorkspaceStatus().mode, 'conflict');

console.log('workspace store tests passed');
