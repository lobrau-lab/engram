const registeredTools = [
  {
    name: 'project_list_files',
    description: 'List non-hidden text-oriented source files in the explicitly granted local project root.',
    effect: 'local_file_read', scope: 'threadline:project:read', capability: 'List files in the granted project workspace',
    risk: 'medium', destination: 'local', dataClasses: ['Project file names and sizes'], approval: 'confirm',
    confirmation: 'Allow Threadline to list files in the project folder granted when the server was started?',
    scopeResolver: 'granted-project-root',
    parameters: { type: 'object', properties: { path: { type: 'string', maxLength: 500 } }, additionalProperties: false }
  },
  {
    name: 'project_read_file',
    description: 'Read a UTF-8 text file inside the explicitly granted local project root. Hidden and sensitive paths are blocked.',
    effect: 'local_file_read', scope: 'threadline:project:read', capability: 'Read a project source file',
    risk: 'medium', destination: 'local', dataClasses: ['Contents of the requested project source file'], approval: 'confirm',
    confirmation: 'Allow Threadline to read this project file? Its contents may be included in the next model request.',
    scopeResolver: 'granted-project-root',
    parameters: { type: 'object', properties: { path: { type: 'string', minLength: 1, maxLength: 500 } }, required: ['path'], additionalProperties: false }
  },
  {
    name: 'project_create_directory',
    description: 'Create a directory under the explicitly granted local project root. Hidden paths and symbolic links are blocked.',
    effect: 'local_file_write', scope: 'threadline:project:write', capability: 'Create a project directory',
    risk: 'medium', destination: 'local', dataClasses: ['Project directory structure'], approval: 'confirm',
    confirmation: 'Allow Threadline to create this directory inside the granted project folder?',
    scopeResolver: 'granted-project-root',
    parameters: { type: 'object', properties: { path: { type: 'string', minLength: 1, maxLength: 500 } }, required: ['path'], additionalProperties: false }
  },
  {
    name: 'run_project_check',
    description: 'Run one fixed local check in the granted project root. Cargo tasks use --offline and cannot download dependencies; arbitrary shell commands are unavailable.',
    effect: 'shell_execution', scope: 'threadline:project:check', capability: 'Run an allowlisted offline project check',
    risk: 'medium', destination: 'local', dataClasses: ['Check command and its output'], approval: 'confirm',
    confirmation: 'Run this fixed local check in the project folder? Cargo will be forced offline.',
    scopeResolver: 'granted-project-root',
    parameters: { type: 'object', properties: { task: { type: 'string', enum: ['git_status', 'git_diff_check', 'cargo_check_cli_offline', 'cargo_test_cli_offline'] } }, required: ['task'], additionalProperties: false }
  },
  {
    name: 'project_write_file',
    description: 'Create or replace a UTF-8 text file inside the explicitly granted local project root. Hidden and sensitive paths are blocked.',
    effect: 'local_file_write', scope: 'threadline:project:write', capability: 'Create or replace a project source file',
    risk: 'high', destination: 'local', dataClasses: ['Content written to the requested project file'], approval: 'confirm',
    confirmation: 'Allow Threadline to write this project file? Review the path and complete proposed content below.',
    scopeResolver: 'granted-project-root',
    parameters: { type: 'object', properties: { path: { type: 'string', minLength: 1, maxLength: 500 }, content: { type: 'string', maxLength: 524288 } }, required: ['path', 'content'], additionalProperties: false }
  },
  {
    name: 'inspect_local_application',
    description: 'Inspect an absolute path to a local executable or macOS .app bundle. Records its version, usage, and flags as a capability digest under the vault; runs the program only with --help and --version.',
    effect: 'application_control', scope: 'threadline:apps:inspect', capability: 'Inspect and digest a local application',
    risk: 'high', destination: 'local', dataClasses: ['Application path', 'Help and version output', 'Application metadata'], approval: 'confirm',
    confirmation: 'Inspect this local application? Threadline will run it with only --help and --version and store the resulting capability digest in the vault.',
    parameters: { type: 'object', properties: { path: { type: 'string', minLength: 1, maxLength: 2000 } }, required: ['path'], additionalProperties: false }
  },
  {
    name: 'list_digested_applications',
    description: 'List applications already digested for this vault, including their reviewed actions and risk level.',
    effect: 'local_file_read', scope: 'threadline:apps:read', capability: 'List digested local applications',
    risk: 'low', destination: 'local', dataClasses: ['Application names, paths, and capability metadata'], approval: 'none',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  },
  {
    name: 'run_digested_application',
    description: 'Run one reviewed action for an application that has already been digested. The exact binary hash is re-checked before every run; arbitrary arguments and shell text are not accepted.',
    effect: 'application_control', scope: 'threadline:apps:run', capability: 'Run a reviewed application action',
    risk: 'high', destination: 'local', dataClasses: ['Application path', 'Action output'], approval: 'confirm',
    confirmation: 'Run this reviewed application action? The exact program path and digest are checked before execution.',
    parameters: { type: 'object', properties: { id: { type: 'string', minLength: 1, maxLength: 120 }, action: { type: 'string', enum: ['help', 'version', 'open'] } }, required: ['id', 'action'], additionalProperties: false }
  },
  {
    name: 'open_workspace_view',
    description: 'Open a view in the chat application.',
    effect: 'ui_navigation',
    scope: 'threadline:views:read',
    capability: 'Navigate between Threadline views',
    risk: 'low',
    destination: 'local',
    dataClasses: [],
    approval: 'none',
    parameters: {
      type: 'object',
      properties: { view: { type: 'string', enum: ['chat', 'memory', 'rules', 'apps', 'settings'] } },
      required: ['view'],
      additionalProperties: false
    }
  },
  {
    name: 'update_workspace_settings',
    description: 'Change supported chat, memory, or context settings at the user’s request.',
    effect: 'local_settings_write',
    scope: 'threadline:settings:write',
    capability: 'Change Threadline settings',
    risk: 'medium',
    destination: 'local',
    dataClasses: ['Threadline settings'],
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
    capability: 'Read derived memory for the active conversation',
    risk: 'low',
    destination: 'local',
    dataClasses: ['Conversation memory and assertion provenance'],
    approval: 'none',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  },
  {
    name: 'start_new_conversation',
    description: 'Create and switch to a new empty local conversation.',
    effect: 'local_state_write',
    scope: 'threadline:conversations:write',
    capability: 'Create and switch to an empty conversation',
    risk: 'low',
    destination: 'local',
    dataClasses: ['Conversation index'],
    approval: 'none',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  },
  {
    name: 'clear_local_workspace',
    description: 'Delete all locally stored conversations, memory, and settings. This is destructive and requires explicit user confirmation.',
    effect: 'destructive_local_delete',
    scope: 'threadline:workspace:delete',
    capability: 'Delete Threadline conversations, memory, and settings',
    risk: 'high',
    destination: 'local',
    dataClasses: ['All Threadline conversations, memory, and settings'],
    approval: 'confirm',
    confirmation: 'Clear all locally stored conversations, memory, and settings? This cannot be undone.',
    parameters: { type: 'object', properties: {}, additionalProperties: false }
  }
].map(tool => Object.freeze({
  ...tool,
  id: tool.id || tool.name,
  version: tool.version || 1,
  capabilityId: tool.capabilityId || tool.scope,
  capabilities: tool.capabilities || [tool.capabilityId || tool.scope],
  approvalPolicy: tool.approval,
  reversibility: tool.reversibility || (tool.effect === 'destructive_local_delete' ? 'irreversible' : 'reversible'),
  executorId: tool.executorId || tool.name,
  timeoutMs: tool.timeoutMs || 5000,
  idempotency: tool.idempotency || 'idempotent',
  scopeResolver: tool.scopeResolver || 'static-threadline-scope',
  dataClasses: Object.freeze([...tool.dataClasses])
}));

const registry = new Map(registeredTools.map(tool => [tool.name, tool]));

// Only the portable function-tool contract is sent to inference providers.
export const applicationTools = registeredTools.map(({ name, description, parameters }) => ({
  type: 'function',
  function: { name, description, parameters }
}));

export function getRegisteredTool(name) {
  return registry.get(name) || null;
}

export function validateToolManifest(tool) {
  if (!tool || typeof tool.name !== 'string' || !Number.isInteger(tool.version) || !tool.capabilityId ||
      !tool.effect || !['low', 'medium', 'high', 'critical'].includes(tool.risk) ||
      !['local', 'remote'].includes(tool.destination) || !Array.isArray(tool.dataClasses) ||
      !tool.executorId || !Number.isSafeInteger(tool.timeoutMs) || tool.timeoutMs < 1 ||
      !tool.idempotency || !tool.scopeResolver) return 'incomplete manifest';
  return null;
}

export function listRegisteredTools() {
  return registeredTools.map(({ name, description, effect, scope, capability, risk, destination, dataClasses, approval }) => ({
    name, description, effect, scope, capability, risk, destination, dataClasses: [...dataClasses], approval
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
    if (schema.type === 'string' && schema.minLength != null && value.length < schema.minLength) return `${name} is too short.`;
    if (schema.type === 'string' && schema.maxLength != null && value.length > schema.maxLength) return `${name} is too long.`;
    if (schema.type === 'integer' && !Number.isSafeInteger(value)) return `${name} must be an integer.`;
    if (schema.enum && !schema.enum.includes(value)) return `${name} has an unsupported value.`;
    if (schema.minimum != null && value < schema.minimum) return `${name} is below the supported minimum.`;
    if (schema.maximum != null && value > schema.maximum) return `${name} is above the supported maximum.`;
  }

  return null;
}
