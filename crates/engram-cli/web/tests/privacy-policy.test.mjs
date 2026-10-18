import assert from 'node:assert/strict';
import { canSendVaultContext, classifyEndpoint, requestCategories } from '../src/core/privacy-policy.js';

assert.equal(classifyEndpoint('http://localhost:8000/v1').kind, 'local');
assert.equal(classifyEndpoint('http://127.0.0.1:8080/v1').kind, 'local');
assert.equal(classifyEndpoint('http://[::1]:8080/v1').kind, 'local');
assert.equal(classifyEndpoint('https://provider.example/v1').kind, 'remote');
assert.equal(classifyEndpoint('http://provider.example/v1').allowed, false);
assert.equal(classifyEndpoint('http://localhost.example/v1').allowed, false);
assert.equal(classifyEndpoint('not a URL').allowed, false);
assert.equal(canSendVaultContext('http://localhost:8000/v1', true), true);
assert.equal(canSendVaultContext('https://provider.example/v1', true), false);

const categories = requestCategories({
  messages: [
    { role: 'system', content: 'instructions' },
    { role: 'user', content: 'older memory decision' },
    { role: 'user', content: 'current question' },
    { role: 'tool', content: '[note.md] retrieved vault excerpt' }
  ],
  tools: [{ type: 'function' }]
});
assert.ok(categories.includes('current message'));
assert.ok(categories.includes('system/custom instructions'));
assert.ok(categories.includes('retrieved history or vault snippets'));
assert.ok(categories.includes('registered tool definitions and possible tool results'));
console.log('privacy policy tests passed');
