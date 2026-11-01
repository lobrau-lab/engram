import assert from 'node:assert/strict';

const { resolveModelProfile, MODEL_PROFILES } = await import('../src/core/model-profiles.js');
const { packContext } = await import('../src/core/context-packer.js');

// 1. Profile auto-detection across target model families
assert.equal(resolveModelProfile('Qwen/Qwen2.5-Coder-32B-Instruct').id, 'qwen');
assert.equal(resolveModelProfile('qwq-32b-preview').id, 'qwen');
assert.equal(resolveModelProfile('qwen2.5-7b-instruct').id, 'qwen');

assert.equal(resolveModelProfile('deepseek-ai/DeepSeek-V3').id, 'deepseek');
assert.equal(resolveModelProfile('deepseek-r1-distill-qwen-32b').id, 'deepseek');
assert.equal(resolveModelProfile('DeepSeek-Coder-V2-Lite').id, 'deepseek');

assert.equal(resolveModelProfile('google/gemma-2-9b-it').id, 'gemma');
assert.equal(resolveModelProfile('gemma-2-27b').id, 'gemma');
assert.equal(resolveModelProfile('gemma-it').id, 'gemma');

assert.equal(resolveModelProfile('apple-on-device').id, 'apple');
assert.equal(resolveModelProfile('meta-llama/Llama-3.1-8B-Instruct').id, 'generic');
assert.equal(resolveModelProfile('mistral-7b').id, 'generic');

// 2. Explicit profile override takes precedence over model name
assert.equal(resolveModelProfile('meta-llama/Llama-3.1-8B-Instruct', 'qwen').id, 'qwen');
assert.equal(resolveModelProfile('Qwen2.5-Coder', 'gemma').id, 'gemma');
assert.equal(resolveModelProfile('unknown-model', 'deepseek').id, 'deepseek');
assert.equal(resolveModelProfile('unknown-model', 'auto').id, 'generic');

// 3. Harness parameters and temperature calibration
assert.equal(MODEL_PROFILES.qwen.defaultTemperature, 0.2);
assert.equal(MODEL_PROFILES.deepseek.defaultTemperature, 0.3);
assert.equal(MODEL_PROFILES.gemma.defaultTemperature, 0.4);
assert.equal(MODEL_PROFILES.generic.defaultTemperature, 0.7);

// 4. DeepSeek R1 <think> block parsing and separation
const rawDeepSeekOutput = '<think>\nAnalyze the request.\nStep 1: check files.\nStep 2: formulate response.\n</think>\nHere is the verified answer.';
const parsed = MODEL_PROFILES.deepseek.parseContent(rawDeepSeekOutput);
assert.equal(parsed.thinking, 'Analyze the request.\nStep 1: check files.\nStep 2: formulate response.');
assert.equal(parsed.content, 'Here is the verified answer.');

const plainOutput = 'Direct response without think tags.';
const plainParsed = MODEL_PROFILES.deepseek.parseContent(plainOutput);
assert.equal(plainParsed.thinking, null);
assert.equal(plainParsed.content, 'Direct response without think tags.');

// 5. Tool recovery prompts
assert.match(MODEL_PROFILES.qwen.formatRecovery('project_read_file', 'file not found'), /re-execute immediately/i);
assert.match(MODEL_PROFILES.deepseek.formatRecovery('run_project_check', 'syntax error'), /formulate a corrected tool call/i);
assert.match(MODEL_PROFILES.gemma.formatRecovery('station_render', 'invalid prompt'), /Retry with corrected inputs/i);

// 6. Context-packer injection of family-specific prompt overlays
const baseSettings = {
  systemPrompt: 'Base instructions.',
  contextLimit: 8192,
  outputReserve: 1200,
  safetyMargin: 400,
  recentTurns: 4,
  memoryEnabled: false,
  vaultSearchEnabled: false
};

const qwenPacked = packContext({
  message: 'Test message',
  history: [],
  memory: { currentGoal: [], activeTopics: [], decisions: [], constraints: [], preferences: [], openQuestions: [] },
  evidence: [],
  settings: { ...baseSettings, model: 'Qwen/Qwen2.5-Coder-32B' }
});
assert.match(qwenPacked.messages[0].content, /Qwen Profile/);
assert.match(qwenPacked.messages[0].content, /Emit strict, minimal JSON/);
assert.equal(qwenPacked.audit.modelProfile, 'qwen');

const deepseekPacked = packContext({
  message: 'Test message',
  history: [],
  memory: { currentGoal: [], activeTopics: [], decisions: [], constraints: [], preferences: [], openQuestions: [] },
  evidence: [],
  settings: { ...baseSettings, model: 'deepseek-r1' }
});
assert.match(deepseekPacked.messages[0].content, /DeepSeek Profile/);
assert.match(deepseekPacked.messages[0].content, /Formulate a deliberate step-by-step plan/);
assert.equal(deepseekPacked.audit.modelProfile, 'deepseek');

const gemmaPacked = packContext({
  message: 'Test message',
  history: [],
  memory: { currentGoal: [], activeTopics: [], decisions: [], constraints: [], preferences: [], openQuestions: [] },
  evidence: [],
  settings: { ...baseSettings, model: 'google/gemma-2-9b-it' }
});
assert.match(gemmaPacked.messages[0].content, /Gemma Profile/);
assert.match(gemmaPacked.messages[0].content, /Provide direct, factual, and concise responses/);
assert.equal(gemmaPacked.audit.modelProfile, 'gemma');

console.log('model profiles tests passed');
