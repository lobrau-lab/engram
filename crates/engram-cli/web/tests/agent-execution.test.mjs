import assert from 'node:assert/strict';
import { executeGuardedInvocation, validateConnectorManifest } from '../src/core/agent-execution.js';

const manifest = { id: 'fake.local.read', version: 1, capabilities: ['fake:read'], effect: 'read', risk: 'low', destination: 'local', dataClasses: ['synthetic'], timeoutMs: 1000, idempotency: 'idempotent', executorId: 'fake' };
assert.equal(validateConnectorManifest(manifest).valid, true);
assert.equal(validateConnectorManifest({ ...manifest, destination: 'remote' }).code, 'missing_remote_destination');
let executed = 0;
const connector = { manifest, execute: async () => { executed += 1; return { value: 'synthetic' }; }, validateOutput: result => result?.value ? null : 'missing value' };
const success = await executeGuardedInvocation({ proposal: { args: {} }, connector, validateInput: () => null, authorize: async () => true });
assert.equal(success.state, 'completed'); assert.equal(executed, 1);
const denied = await executeGuardedInvocation({ proposal: { args: {} }, connector, validateInput: () => null, authorize: async () => false });
assert.equal(denied.state, 'denied'); assert.equal(executed, 1);
const invalid = await executeGuardedInvocation({ proposal: { args: {} }, connector, validateInput: () => 'extra fields', authorize: async () => true });
assert.equal(invalid.state, 'denied'); assert.equal(executed, 1);
console.log('agent execution tests passed');
