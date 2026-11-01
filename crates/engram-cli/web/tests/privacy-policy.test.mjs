import assert from 'node:assert/strict';
import { canSendVaultContext, classifyEndpoint, requestCategories, supportsFunctionTools } from '../src/core/privacy-policy.js';

assert.equal(classifyEndpoint('engram://app/v1').kind, 'local');
assert.equal(classifyEndpoint('engram://localhost/v1').allowed, false);
assert.equal(classifyEndpoint('engram://evil.example/v1').allowed, false);
assert.equal(canSendVaultContext('engram://app/v1', true), true);
assert.equal(canSendVaultContext('engram://localhost/v1', true), false);
assert.equal(classifyEndpoint('http://localhost:8000/v1').kind, 'local');
assert.equal(classifyEndpoint('http://127.0.0.1:8080/v1').kind, 'local');
assert.equal(classifyEndpoint('http://[::1]:8080/v1').kind, 'local');
assert.equal(classifyEndpoint('https://provider.example/v1').kind, 'remote');
assert.equal(classifyEndpoint('http://provider.example/v1').allowed, false);
assert.equal(classifyEndpoint('http://localhost.example/v1').allowed, false);
assert.equal(classifyEndpoint('not a URL').allowed, false);
assert.equal(canSendVaultContext('http://localhost:8000/v1', true), true);
assert.equal(canSendVaultContext('https://provider.example/v1', true), false);
assert.equal(supportsFunctionTools({ model: 'apple-on-device', endpointOrigin: 'http://127.0.0.1:8000', appOrigin: 'http://127.0.0.1:8000' }), false);
assert.equal(supportsFunctionTools({ model: 'apple-on-device', endpointOrigin: 'engram://app', appOrigin: 'engram://app' }), false);
assert.equal(supportsFunctionTools({ model: 'qwen-local', endpointOrigin: 'http://127.0.0.1:8000', appOrigin: 'http://127.0.0.1:8000' }), true);

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
assert.equal(classifyEndpoint('http://192.168.1.42:8080/v1').allowed, true);
assert.equal(classifyEndpoint('http://10.0.0.5:8080/v1').allowed, true);
assert.equal(classifyEndpoint('http://desktop.local:8080/v1').allowed, true);
assert.equal(classifyEndpoint('http://169.254.1.9:8080/v1').allowed, true);
assert.equal(classifyEndpoint('http://8.8.8.8:8080/v1').allowed, false);
console.log('privacy policy tests passed');
