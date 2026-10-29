import assert from 'node:assert/strict';
import { validateToolArguments } from '../src/core/tool-registry.js';

const mockTool = {
  parameters: {
    type: 'object',
    required: ['reqString'],
    properties: {
      reqString: { type: 'string', minLength: 2, maxLength: 5 },
      optBool: { type: 'boolean' },
      optInt: { type: 'integer' }
    }
  }
};

assert.equal(validateToolArguments(null, {}), 'Arguments must be a JSON object.');
assert.equal(validateToolArguments(mockTool, []), 'Arguments must be a JSON object.');
assert.equal(validateToolArguments(mockTool, null), 'Arguments must be a JSON object.');
assert.equal(validateToolArguments(mockTool, "string"), 'Arguments must be a JSON object.');

assert.equal(validateToolArguments(mockTool, { reqString: 'hi', extra: 1 }), 'Unsupported argument: extra.');
assert.equal(validateToolArguments(mockTool, {}), 'Missing required argument: reqString.');

assert.equal(validateToolArguments(mockTool, { reqString: 123 }), 'reqString must be a string.');
assert.equal(validateToolArguments(mockTool, { reqString: 'a' }), 'reqString is too short.');
assert.equal(validateToolArguments(mockTool, { reqString: 'toolong' }), 'reqString is too long.');

assert.equal(validateToolArguments(mockTool, { reqString: 'ok', optBool: 'yes' }), 'optBool must be a boolean.');
assert.equal(validateToolArguments(mockTool, { reqString: 'ok', optInt: '5' }), 'optInt must be an integer.');

assert.equal(validateToolArguments(mockTool, { reqString: 'ok', optBool: true, optInt: 5 }), null);

console.log('tool validation tests passed');
