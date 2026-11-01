/**
 * Harness Model Profiles for Threadline.
 *
 * Implements model-family calibrations for Gemma, Qwen, DeepSeek, Apple FM,
 * and Generic models. Treats the harness around frozen model weights as tunable
 * parameters (system prompt overlays, temperature defaults, tool schemas,
 * reasoning block handling, and recovery prompts).
 */

export const MODEL_PROFILES = Object.freeze({
  qwen: {
    id: 'qwen',
    name: 'Qwen (2.5 / Coder)',
    family: 'qwen',
    defaultTemperature: 0.2,
    maxOutputTokens: 2048,
    systemSuffix: `[Model Tuning: Qwen Profile]
- Tool Calling: When tools are available and relevant, invoke them immediately. Do not explain tool calls in chat before executing them.
- Arguments: Emit strict, minimal JSON conforming exactly to the function schemas.
- Code & Files: Keep diffs and edits concise, targeted, and syntactically complete.`,
    toolCallStyle: 'openai_strict',
    formatRecovery(toolName, error) {
      return `Tool '${toolName}' failed: ${error}. Inspect the schema, correct the arguments, and re-execute immediately.`;
    },
    parseContent(text = '') {
      return { thinking: null, content: text };
    }
  },

  deepseek: {
    id: 'deepseek',
    name: 'DeepSeek (V3 / R1 / Coder)',
    family: 'deepseek',
    defaultTemperature: 0.3,
    maxOutputTokens: 4096,
    systemSuffix: `[Model Tuning: DeepSeek Profile]
- Reasoning: Formulate a deliberate step-by-step plan before invoking tools or producing final answers.
- Parameters: Validate parameter types and file paths against known vault state before execution.
- Truthfulness: Clearly differentiate established vault evidence from deductive inference.`,
    toolCallStyle: 'openai_reasoning',
    formatRecovery(toolName, error) {
      return `Action '${toolName}' failed with: ${error}. Analyze what went wrong, re-read the required input, and formulate a corrected tool call.`;
    },
    parseContent(text = '') {
      if (!text || typeof text !== 'string') return { thinking: null, content: '' };
      const thinkMatch = text.match(/<think>([\s\S]*?)<\/think>/i);
      if (thinkMatch) {
        const thinking = thinkMatch[1].trim();
        const content = text.replace(/<think>[\s\S]*?<\/think>/i, '').trim();
        return { thinking, content };
      }
      return { thinking: null, content: text };
    }
  },

  gemma: {
    id: 'gemma',
    name: 'Gemma (2 / 3 / IT)',
    family: 'gemma',
    defaultTemperature: 0.4,
    maxOutputTokens: 2048,
    systemSuffix: `[Model Tuning: Gemma Profile]
- Directness: Provide direct, factual, and concise responses without conversational filler.
- Constraints: Follow formatting and citation constraints strictly. Use square brackets for note IDs.
- Consistency: Avoid repetitive loops; when answering grounded questions, anchor strictly to the supplied context.`,
    toolCallStyle: 'direct_compact',
    formatRecovery(toolName, error) {
      return `Tool execution '${toolName}' failed: ${error}. Retry with corrected inputs.`;
    },
    parseContent(text = '') {
      return { thinking: null, content: text };
    }
  },

  apple: {
    id: 'apple',
    name: 'Apple Foundation Models',
    family: 'apple',
    defaultTemperature: 0.7,
    maxOutputTokens: 1024,
    systemSuffix: `[Model Tuning: Apple FM]
- Built-in on-device assistant. Emphasize concise, natural dialogue grounded directly in vault notes.`,
    toolCallStyle: 'text_only',
    formatRecovery(toolName, error) {
      return `The built-in on-device model operates in text mode. Unable to run tool '${toolName}'.`;
    },
    parseContent(text = '') {
      return { thinking: null, content: text };
    }
  },

  generic: {
    id: 'generic',
    name: 'Generic OpenAI-Compatible',
    family: 'generic',
    defaultTemperature: 0.7,
    maxOutputTokens: 2048,
    systemSuffix: '',
    toolCallStyle: 'openai_standard',
    formatRecovery(toolName, error) {
      return `Tool '${toolName}' failed: ${error}.`;
    },
    parseContent(text = '') {
      return { thinking: null, content: text };
    }
  }
});

/**
 * Detect the optimal harness profile based on model ID, base URL, or explicit setting.
 */
export function resolveModelProfile(modelName = '', explicitProfileId = 'auto') {
  if (explicitProfileId && explicitProfileId !== 'auto' && MODEL_PROFILES[explicitProfileId]) {
    return MODEL_PROFILES[explicitProfileId];
  }

  const modelLower = String(modelName || '').toLowerCase().trim();

  // DeepSeek check takes priority over base architecture tags (e.g. deepseek-r1-distill-qwen)
  if (modelLower.includes('deepseek') || modelLower.includes('deep-seek') || modelLower.includes('r1') || modelLower.includes('v3')) {
    return MODEL_PROFILES.deepseek;
  }
  if (modelLower.includes('qwen') || modelLower.includes('qwq')) {
    return MODEL_PROFILES.qwen;
  }
  if (modelLower.includes('gemma')) {
    return MODEL_PROFILES.gemma;
  }
  if (modelLower.includes('apple') || modelLower.includes('foundation') || modelLower === 'apple-on-device') {
    return MODEL_PROFILES.apple;
  }

  return MODEL_PROFILES.generic;
}
