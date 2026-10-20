import assert from 'node:assert/strict';
import { executeApplicationAction } from '../src/core/app-actions.js';
import { authorizeToolInvocation } from '../src/core/permission-gate.js';
import { getRegisteredTool, listRegisteredTools, validateToolArguments } from '../src/core/tool-registry.js';

const tool = getRegisteredTool('computer_use');
assert.ok(tool, 'computer_use is registered');
assert.equal(tool.effect, 'application_control');
assert.equal(tool.risk, 'critical');
assert.ok(listRegisteredTools().some(item => item.name === 'computer_use'));

assert.equal(validateToolArguments(tool, { action: 'focus', app: 'Safari' }), null);
assert.match(validateToolArguments(tool, { action: 'not-an-action' }), /unsupported value/);
assert.match(validateToolArguments(tool, { action: 'click', x: 1.5, y: 2 }), /integer/);
assert.match(validateToolArguments(tool, { action: 'key', key: 'a', extra: true }), /Unsupported argument/);

const trusted = await authorizeToolInvocation(tool, { action: 'focus', app: 'Safari' }, {
  trustedTakeover: true,
  confirm: () => { throw new Error('an armed takeover session must not prompt'); }
});
assert.equal(trusted.allowed, true);
assert.equal(trusted.trusted, true);

let confirmations = 0;
const cancelled = await executeApplicationAction('computer_use', JSON.stringify({ action: 'focus', app: 'Safari' }), {
  confirm: () => { confirmations += 1; return false; }
});
assert.equal(cancelled.cancelled, true);
assert.equal(confirmations, 1);

let received = null;
const executed = await executeApplicationAction('computer_use', JSON.stringify({ action: 'focus', app: 'Safari' }), {
  trustedTakeover: true,
  confirm: () => { throw new Error('an armed takeover session must not prompt'); },
  computerUse: async args => { received = args; return { focused: 'Safari' }; }
});
assert.equal(executed.ok, true);
assert.deepEqual(received, { action: 'focus', app: 'Safari' });
assert.equal(executed.focused, 'Safari');

const unavailable = await executeApplicationAction('computer_use', JSON.stringify({ action: 'screenshot' }), {
  trustedTakeover: true,
  confirm: () => true
});
assert.equal(unavailable.ok, false);
assert.equal(unavailable.code, 'connector_failed');

console.log('takeover tool tests passed');
