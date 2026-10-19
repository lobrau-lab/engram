import { applicationTools, getRegisteredTool, listRegisteredTools, validateToolArguments, validateToolManifest } from './tool-registry.js';
import { authorizeToolInvocation, formatToolDisclosure } from './permission-gate.js';

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


/** Run narrowly scoped project filesystem actions through the local Engram server. */
export async function executeProjectAction(name, rawArguments, callbacks = {}) {
  const invocationId = callbacks.actionId || crypto.randomUUID();
  if (completedInvocations.has(invocationId)) return completedInvocations.get(invocationId);
  let args;
  try { args = typeof rawArguments === 'string' ? JSON.parse(rawArguments || '{}') : (rawArguments ?? {}); }
  catch { return { ok: false, code: 'invalid_arguments', error: 'Arguments are not valid JSON.' }; }
  const tool = getRegisteredTool(name);
  if (!tool || !(name.startsWith('project_') || name === 'run_project_check')) return { ok: false, code: 'unknown_tool', error: `Project action not allowed: ${name}` };
  const validationError = validateToolArguments(tool, args);
  if (validationError) return { ok: false, code: 'invalid_arguments', error: validationError };
  if (typeof callbacks.confirm !== 'function') return { ok: false, code: 'approval_unavailable', error: 'This project action requires an approval interface.' };
  let confirmed = false;
  try { confirmed = await callbacks.confirm(formatToolDisclosure(tool, args)) === true; }
  catch { return { ok: false, code: 'approval_unavailable', error: 'The approval interface failed; the action was blocked.' }; }
  if (!confirmed) {
    receipt(invocationId, tool, 'cancelled', { code: 'approval_denied' });
    return { ok: false, code: 'approval_denied', cancelled: true, error: 'The user declined this action.', invocationId };
  }
  const authorization = authorizeToolInvocation(tool, args, { confirm: () => true });
  if (!authorization.allowed) {
    receipt(invocationId, tool, 'denied', { code: authorization.code });
    return { ok: false, code: authorization.code, cancelled: Boolean(authorization.cancelled), error: authorization.message, invocationId };
  }
  try {
    const query = new URLSearchParams();
    let response;
    if (name === 'project_list_files') {
      query.set('path', args.path || '.');
      response = await fetch(`/api/v1/project/files?${query}`);
    } else if (name === 'project_read_file') {
      query.set('path', args.path);
      response = await fetch(`/api/v1/project/read?${query}`);
    } else if (name === 'project_write_file') {
      response = await fetch('/api/v1/project/write', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(args) });
    } else if (name === 'project_create_directory') {
      response = await fetch('/api/v1/project/directory', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(args) });
    } else if (name === 'run_project_check') {
      response = await fetch('/api/v1/project/check', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(args) });
    } else return { ok: false, code: 'unimplemented_tool', error: 'This project action has no executor.' };
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || `Local project action failed (${response.status}).`);
    const completed = { ok: true, ...result };
    completedInvocations.set(invocationId, completed);
    receipt(invocationId, tool, 'executed');
    return completed;
  } catch (error) {
    receipt(invocationId, tool, 'failed', { message: String(error.message || error).slice(0, 300) });
    return { ok: false, code: 'project_action_failed', error: String(error.message || error).slice(0, 500), invocationId };
  }
}
