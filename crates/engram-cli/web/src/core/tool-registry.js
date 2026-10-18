const registeredTools = [
  {
    name: 'open_workspace_view',
    description: 'Open a view in the chat application.',
    effect: 'ui_navigation',
    scope: 'threadline:views:read',
    approval: 'none',
    parameters: {
      type: 'object',
      properties: { view: { type: 'string', enum: ['chat', 'memory', 'rules', 'settings'] } },
      required: ['view'],
      additionalProperties: false
    }
  },
  {
    name: 'update_workspace_settings',
    description: 'Change supported chat, memory, or context settings at the user’s request.',
    effect: 'local_settings_write',
    scope: 'threadline:settings:write',
    approval: 'confirm',
    confirmation: 'Apply the requested changes to Threadline settings?',
    parameters: {
      type: 'object',
      properties: {
        memoryEnabled: { type: 'boolean' },
        vaultSearchEnabled: { type: 'boolean' },
        showEvidence: { type: 'boolean' },
        contextLimit: { type: 'integer', minimum: 1024, maximum: 1000000 },
        outputReserve: { type: 'integer', minimum: 128, maximum: 100000 },
        safetyMargin: { type: 'integer', minimum: 0, maximum: 50000 },
        recentTurns: { type: 'integer', minimum: 2, maximum: 16 },
        strategy: { type: 'string', enum: ['hybrid', 'recent', 'state'] }
      },
      additionalProperties: false
    }
  },
  {
    name: 'inspect_conversation_memory',
    description: 'Read the current conversation’s derived memory variables and unresolved conflicts.',
    effect: 'conversation_read',
    scope: 'threadline:memory:read',
    approval: 'none',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  },
  {
    name: 'start_new_conversation',
    description: 'Create and switch to a new empty local conversation.',
    effect: 'local_state_write',
    scope: 'threadline:conversations:write',
    approval: 'none',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  },
  {
    name: 'clear_local_workspace',
    description: 'Delete all locally stored conversations, memory, and settings. This is destructive and requires explicit user confirmation.',
    effect: 'destructive_local_delete',
    scope: 'threadline:workspace:delete',
    approval: 'confirm',
    confirmation: 'Clear all locally stored conversations, memory, and settings? This cannot be undone.',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  }
].map(tool => Object.freeze(tool));

const registry = new Map(registeredTools.map(tool => [tool.name, tool]));

// Only the portable function-tool contract is sent to inference providers.
export const applicationTools = registeredTools.map(({ name, description, parameters }) => ({
  type: 'function',
  function: { name, description, parameters }
}));

export function getRegisteredTool(name) {
  return registry.get(name) || null;
}

export function listRegisteredTools() {
  return registeredTools.map(({ name, description, effect, scope, approval }) => ({
    name, description, effect, scope, approval
  }));
}

export function validateToolArguments(tool, args) {
  if (!tool || !args || typeof args !== 'object' || Array.isArray(args)) {
    return 'Arguments must be a JSON object.';
  }

  const properties = tool.parameters.properties || {};
  const allowed = new Set(Object.keys(properties));
  const unknown = Object.keys(args).find(key => !allowed.has(key));
  if (unknown) return `Unsupported argument: ${unknown}.`;

  for (const required of tool.parameters.required || []) {
    if (!Object.hasOwn(args, required)) return `Missing required argument: ${required}.`;
  }

  for (const [name, value] of Object.entries(args)) {
    const schema = properties[name];
    if (schema.type === 'boolean' && typeof value !== 'boolean') return `${name} must be a boolean.`;
    if (schema.type === 'string' && typeof value !== 'string') return `${name} must be a string.`;
    if (schema.type === 'integer' && !Number.isSafeInteger(value)) return `${name} must be an integer.`;
    if (schema.enum && !schema.enum.includes(value)) return `${name} has an unsupported value.`;
    if (schema.minimum != null && value < schema.minimum) return `${name} is below the supported minimum.`;
    if (schema.maximum != null && value > schema.maximum) return `${name} is above the supported maximum.`;
  }

  return null;
}
