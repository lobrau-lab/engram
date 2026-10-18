import assert from 'node:assert/strict';

globalThis.localStorage = {
  data: new Map(),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); },
  removeItem(key) { this.data.delete(key); }
};
if (!globalThis.crypto?.randomUUID) {
  let n = 0;
  globalThis.crypto = { randomUUID: () => `synthetic-${++n}` };
}

const { runMemoryBench, MEMORY_BENCH_FIXTURES } = await import('../src/core/evaluation-bench.js');
const report = runMemoryBench();

assert.equal(report.results.length, MEMORY_BENCH_FIXTURES.length);
assert.ok(report.results.every(result => Number.isFinite(result.estimatedInputTokens)));
assert.ok(report.results.every(result => result.withinBudget || result.id === 'budget'));
assert.ok(report.results.find(result => result.id === 'single-recall'));
assert.ok(report.results.find(result => result.id === 'contradiction'));
assert.ok(report.results.find(result => result.id === 'absent'));
console.log(JSON.stringify({ evidenceRecall: report.evidenceRecall, irrelevantSelectionRate: report.irrelevantSelectionRate, budgetCompliance: report.budgetCompliance, conflictCases: report.conflictCases }));
