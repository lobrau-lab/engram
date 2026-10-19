import assert from 'node:assert/strict';

globalThis.localStorage = {
  data: new Map(),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); }
};
const { packContext } = await import('../src/core/context-packer.js');

const privateMarker = 'CONFIDENTIAL_VAULT_MARKER_7f2a';
const settings = {
  systemPrompt: 'Answer accurately.',
  contextLimit: 2048,
  outputReserve: 256,
  safetyMargin: 64,
  memoryEnabled: false,
  recentTurns: 0,
  vaultSearchEnabled: true
};
const input = {
  message: 'What is in my private note?',
  history: [],
  memory: { currentGoal: [], decisions: [], constraints: [], preferences: [], activeTopics: [], openQuestions: [], assertions: [], conflicts: [] },
  evidence: [],
  vaultNotes: [{ title: 'Private note', id: 'private.md', snippet: privateMarker }]
};

const remotePack = packContext({ ...input, vaultContextAllowed: false, settings });
assert.equal(JSON.stringify(remotePack.messages).includes(privateMarker), false,
  'remote prompt packing must exclude vault snippets even when vault search is enabled');
assert.equal(remotePack.audit.selected.some(item => item.kind === 'vault source'), false);

const localPack = packContext({ ...input, vaultContextAllowed: true, settings });
assert.equal(JSON.stringify(localPack.messages).includes(privateMarker), true,
  'authorized local prompt packing should retain retrieved vault snippets');
console.log('context packer privacy boundary tests passed');
