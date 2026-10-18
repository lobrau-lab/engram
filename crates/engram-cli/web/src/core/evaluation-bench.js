import { recordAndUpdate, retrieveEvidence, relevantConflicts } from './memory-manager.js';
import { packContext } from './context-packer.js';
import { emptyMemory } from './store.js';

export const BENCH_SETTINGS = Object.freeze({
  contextLimit: 512,
  outputReserve: 96,
  safetyMargin: 32,
  recentTurns: 4,
  memoryEnabled: true,
  strategy: 'hybrid',
  vaultSearchEnabled: false,
  systemPrompt: 'Answer from the supplied conversation evidence. If evidence is absent, say that clearly.'
});

const turn = (id, content, role = 'user') => ({ id, role, content, timestamp: `2026-01-01T00:00:${id.slice(-2)}Z` });

export const MEMORY_BENCH_FIXTURES = Object.freeze([
  { id: 'single-recall', query: 'What editor did I choose?', turns: [turn('s01', 'I chose Helix as my editor.'), turn('s02', 'The weather is rainy.'), turn('s03', 'What editor did I choose?')], expectedEvidence: ['s01'], expectedExcluded: ['s02'] },
  { id: 'correction', query: 'What editor is current?', turns: [turn('c01', 'I use Vim.'), turn('c02', 'Correction: I use Helix now.'), turn('c03', 'What editor is current?')], expectedEvidence: ['c02'], expectedExcluded: [] },
  { id: 'contradiction', query: 'Which deployment target should be considered?', turns: [turn('x01', 'The project deployment target is Linux.'), turn('x02', 'The project deployment target is Windows.'), turn('x03', 'Which deployment target should be considered?')], expectedEvidence: ['x01', 'x02'], expectedExcluded: [] },
  { id: 'scope-time', query: 'What database do we use for Engram?', turns: [turn('t01', 'Last year, the prototype used Redis for experiments.'), turn('t02', 'For Engram now, SQLite is the database.'), turn('t03', 'What database do we use for Engram?')], expectedEvidence: ['t02'], expectedExcluded: ['t01'] },
  { id: 'irrelevant', query: 'What is the release license?', turns: [turn('i01', 'Engram uses Apache licensing.'), turn('i02', 'My favorite color is green.'), turn('i03', 'I had coffee this morning.'), turn('i04', 'What is the release license?')], expectedEvidence: ['i01'], expectedExcluded: ['i02', 'i03'] },
  { id: 'absent', query: 'What is the production database password?', turns: [turn('a01', 'The project has a staging database.'), turn('a02', 'We value secure credentials.'), turn('a03', 'What is the production database password?')], expectedEvidence: [], expectedExcluded: ['a01', 'a02'] },
  { id: 'budget', query: 'What is the selected deployment platform?', turns: Array.from({ length: 18 }, (_, index) => turn(`b${String(index).padStart(2, '0')}`, index === 2 ? 'The selected deployment platform is Linux.' : `Unrelated planning detail ${index} about a different concern.`)).concat([turn('bq', 'What is the selected deployment platform?')]), expectedEvidence: ['b02'], expectedExcluded: [] }
]);

export function replayFixture(fixture, settings = BENCH_SETTINGS) {
  let memory = emptyMemory();
  for (const currentTurn of fixture.turns.slice(0, -1)) memory = recordAndUpdate(memory, currentTurn);
  const history = fixture.turns;
  const evidence = retrieveEvidence(history, fixture.query, memory, 5);
  const pack = packContext({ message: fixture.query, history, memory, evidence, vaultNotes: [], settings });
  const selectedIds = [...new Set(pack.audit.selected.flatMap(item => item.sourceTurnIds || []))];
  const expectedFound = fixture.expectedEvidence.every(id => selectedIds.includes(id));
  const irrelevantSelected = fixture.expectedExcluded.filter(id => selectedIds.includes(id));
  const conflicts = relevantConflicts(memory, fixture.query).map(conflict => conflict.assertionIds);
  return {
    id: fixture.id,
    query: fixture.query,
    expectedEvidence: fixture.expectedEvidence,
    expectedExcluded: fixture.expectedExcluded,
    selectedEvidence: selectedIds,
    expectedFound,
    irrelevantSelected,
    conflicts,
    withinBudget: pack.audit.estimatedInputTokens <= pack.audit.usableInput,
    estimatedInputTokens: pack.audit.estimatedInputTokens,
    usableInput: pack.audit.usableInput,
    audit: pack.audit,
    packedMessages: pack.messages
  };
}

export function runMemoryBench(settings = BENCH_SETTINGS) {
  const results = MEMORY_BENCH_FIXTURES.map(fixture => replayFixture(fixture, settings));
  return {
    settings,
    results,
    evidenceRecall: results.filter(result => result.expectedFound).length / results.length,
    irrelevantSelectionRate: results.reduce((sum, result) => sum + result.irrelevantSelected.length, 0) / Math.max(1, results.reduce((sum, result) => sum + result.expectedExcluded.length, 0)),
    budgetCompliance: results.filter(result => result.withinBudget).length / results.length,
    conflictCases: results.filter(result => result.conflicts.length > 0).map(result => result.id)
  };
}
