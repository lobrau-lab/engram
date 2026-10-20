import assert from 'node:assert/strict';

globalThis.localStorage = {
  data: new Map(),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); }
};
const { filterRemoteHistory, packContext } = await import('../src/core/context-packer.js');

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

const historyWithVaultDerivedAnswer = [
  { id: 'user-1', role: 'user', content: 'What is in my private note?' },
  { id: 'assistant-vault', role: 'assistant', content: privateMarker, vaultProvenanceKnown: true, vaultSourceNoteIds: ['private.md'] },
  { id: 'assistant-legacy', role: 'assistant', content: 'Legacy assistant answer with unknown provenance.' },
  { id: 'assistant-safe', role: 'assistant', content: 'A prior answer with known non-vault provenance.', vaultProvenanceKnown: true, vaultSourceNoteIds: [] }
];
const filteredHistory = filterRemoteHistory(historyWithVaultDerivedAnswer);
assert.deepEqual(filteredHistory.map(turn => turn.id), ['user-1', 'assistant-safe'],
  'remote history should exclude vault-derived and provenance-unknown assistant turns');
const historyPack = packContext({ ...input, history: filteredHistory, vaultNotes: [], vaultContextAllowed: false, settings });
assert.equal(JSON.stringify(historyPack.messages).includes(privateMarker), false,
  'vault-derived assistant content must not re-enter remote prompts through recent history');
assert.equal(JSON.stringify(historyPack.messages).includes('Legacy assistant answer'), false,
  'legacy assistant content with unknown provenance must fail closed');

const localPack = packContext({ ...input, vaultContextAllowed: true, settings });
assert.equal(JSON.stringify(localPack.messages).includes(privateMarker), true,
  'authorized local prompt packing should retain retrieved vault snippets');
console.log('context packer privacy boundary tests passed');
