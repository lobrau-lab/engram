import { applicationTools, getRegisteredTool, listRegisteredTools, validateToolArguments, validateToolManifest } from './tool-registry.js';
import { authorizeToolInvocation } from './permission-gate.js';

export { applicationTools, listRegisteredTools };

const completedInvocations = new Map();
const auditReceipts = [];

function receipt(invocationId, tool, outcome, extra = {}) {
  const event = { invocationId, tool: tool?.name || 'unknown', version: tool?.version || null, outcome, capability: tool?.capabilityId || null, destination: tool?.destination || null, timestamp: new Date().toISOString(), ...extra };
  auditReceipts.push(event); if (auditReceipts.length > 200) auditReceipts.shift();
}

export function listAuditReceipts() { return auditReceipts.map(item => ({ ...item })); }

/** Dispatch only registered app actions after schema validation and policy checks. */
export function executeApplicationAction(name, rawArguments, callbacks = {}) {
  const invocationId = callbacks.actionId || crypto.randomUUID();
  if (completedInvocations.has(invocationId)) return completedInvocations.get(invocationId);
  let args;
  try {
    args = typeof rawArguments === 'string'
      ? JSON.parse(rawArguments || '{}')
      : (rawArguments ?? {});
  } catch {
    return { ok: false, code: 'invalid_arguments', error: 'Arguments are not valid JSON.' };
  }

  const tool = getRegisteredTool(name);
  if (!tool) { receipt(invocationId, null, 'denied', { code: 'unknown_tool' }); return { ok: false, code: 'unknown_tool', error: `Action not allowed: ${name}` }; }
  const manifestError = validateToolManifest(tool);
  if (manifestError) { receipt(invocationId, tool, 'denied', { code: 'invalid_manifest' }); return { ok: false, code: 'invalid_manifest', error: manifestError }; }

  const validationError = validateToolArguments(tool, args);
  if (validationError) { receipt(invocationId, tool, 'denied', { code: 'invalid_arguments' }); return { ok: false, code: 'invalid_arguments', error: validationError }; }

  const authorization = authorizeToolInvocation(tool, args, callbacks);
  if (!authorization.allowed) {
    receipt(invocationId, tool, authorization.cancelled ? 'cancelled' : 'denied', { code: authorization.code });
    return {
      ok: false,
      code: authorization.code,
      cancelled: Boolean(authorization.cancelled),
      error: authorization.message,
      invocationId
    };
  }

  switch (name) {
    case 'open_workspace_view':
      if (typeof callbacks.openView !== 'function') return unavailable('openView');
      callbacks.openView(args.view);
      return complete(invocationId, { ok: true, opened: args.view }, tool);

    case 'update_workspace_settings':
      if (!Object.keys(args).length) return { ok: false, code: 'invalid_arguments', error: 'Provide at least one supported setting.' };
      if (typeof callbacks.updateSettings !== 'function') return unavailable('updateSettings');
      callbacks.updateSettings(args);
      return complete(invocationId, { ok: true, updated: args }, tool);

    case 'inspect_conversation_memory':
      if (typeof callbacks.getMemory !== 'function') return unavailable('getMemory');
      return complete(invocationId, { ok: true, memory: callbacks.getMemory() }, tool);

    case 'start_new_conversation':
      if (typeof callbacks.newConversation !== 'function') return unavailable('newConversation');
      callbacks.newConversation();
      return complete(invocationId, { ok: true, action: 'new_conversation_created' }, tool);

    case 'clear_local_workspace':
      if (typeof callbacks.clearWorkspace !== 'function') return unavailable('clearWorkspace');
      callbacks.clearWorkspace();
      return complete(invocationId, { ok: true, action: 'workspace_cleared' }, tool);

    default:
      // The registry is the allowlist; fail closed if a handler is missing.
      return { ok: false, code: 'unimplemented_tool', error: 'This registered action has no executor.' };
  }
}

function complete(invocationId, result, tool) {
  completedInvocations.set(invocationId, result);
  receipt(invocationId, tool, 'executed');
  return result;
}

function unavailable(callback) {
  return { ok: false, code: 'executor_unavailable', error: `This action is unavailable (${callback}).` };
}
