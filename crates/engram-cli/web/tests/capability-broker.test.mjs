import assert from 'node:assert/strict';
import { executeApplicationAction, listAuditReceipts } from '../src/core/app-actions.js';

const opened = [];
const callbacks = { openView: view => opened.push(view), confirm: () => true };

assert.equal((await executeApplicationAction('unknown', '{}', callbacks)).code, 'unknown_tool');
assert.equal((await executeApplicationAction('open_workspace_view', '{"view":"not-a-view"}', callbacks)).code, 'invalid_arguments');
assert.equal((await executeApplicationAction('open_workspace_view', '{"view":"memory","extra":true}', callbacks)).code, 'invalid_arguments');
assert.equal((await executeApplicationAction('open_workspace_view', '{"view":"memory"}', callbacks)).ok, true);
assert.deepEqual(opened, ['memory']);

let changed = 0;
assert.equal((await executeApplicationAction('update_workspace_settings', '{"memoryEnabled":false}', { ...callbacks, updateSettings: () => { changed += 1; }, confirm: () => false })).cancelled, true);
assert.equal(changed, 0);
assert.equal((await executeApplicationAction('update_workspace_settings', '{"memoryEnabled":false}', { ...callbacks, updateSettings: () => { changed += 1; }, confirm: async () => true })).ok, true);
assert.equal(changed, 1);
assert.equal((await executeApplicationAction('update_workspace_settings', '{"memoryEnabled":false}', { ...callbacks, updateSettings: () => { changed += 1; }, confirm: async () => false })).cancelled, true);
assert.equal(changed, 1);
const invocation = 'duplicate-action';
const first = await executeApplicationAction('open_workspace_view', '{"view":"rules"}', { ...callbacks, actionId: invocation });
const second = await executeApplicationAction('open_workspace_view', '{"view":"settings"}', { ...callbacks, actionId: invocation });
assert.deepEqual(second, first);
assert.ok(listAuditReceipts().some(event => event.invocationId === invocation && event.outcome === 'executed'));
console.log('capability broker tests passed');
