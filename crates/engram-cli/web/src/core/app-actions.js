import { applicationTools, getRegisteredTool, listRegisteredTools, validateToolArguments } from './tool-registry.js';
import { authorizeToolInvocation } from './permission-gate.js';

export { applicationTools, listRegisteredTools };

/** Dispatch only registered app actions after schema validation and policy checks. */
export function executeApplicationAction(name, rawArguments, callbacks = {}) {
  let args;
  try {
    args = typeof rawArguments === 'string'
      ? JSON.parse(rawArguments || '{}')
      : (rawArguments ?? {});
  } catch {
    return { ok: false, code: 'invalid_arguments', error: 'Arguments are not valid JSON.' };
  }

  const tool = getRegisteredTool(name);
  if (!tool) return { ok: false, code: 'unknown_tool', error: `Action not allowed: ${name}` };

  const validationError = validateToolArguments(tool, args);
  if (validationError) return { ok: false, code: 'invalid_arguments', error: validationError };

  const authorization = authorizeToolInvocation(tool, args, callbacks);
  if (!authorization.allowed) {
    return {
      ok: false,
      code: authorization.code,
      cancelled: Boolean(authorization.cancelled),
      error: authorization.message
    };
  }

  switch (name) {
    case 'open_workspace_view':
      if (typeof callbacks.openView !== 'function') return unavailable('openView');
      callbacks.openView(args.view);
      return { ok: true, opened: args.view };

    case 'update_workspace_settings':
      if (!Object.keys(args).length) return { ok: false, code: 'invalid_arguments', error: 'Provide at least one supported setting.' };
      if (typeof callbacks.updateSettings !== 'function') return unavailable('updateSettings');
      callbacks.updateSettings(args);
      return { ok: true, updated: args };

    case 'inspect_conversation_memory':
      if (typeof callbacks.getMemory !== 'function') return unavailable('getMemory');
      return { ok: true, memory: callbacks.getMemory() };

    case 'start_new_conversation':
      if (typeof callbacks.newConversation !== 'function') return unavailable('newConversation');
      callbacks.newConversation();
      return { ok: true, action: 'new_conversation_created' };

    case 'clear_local_workspace':
      if (typeof callbacks.clearWorkspace !== 'function') return unavailable('clearWorkspace');
      callbacks.clearWorkspace();
      return { ok: true, action: 'workspace_cleared' };

    default:
      // The registry is the allowlist; fail closed if a handler is missing.
      return { ok: false, code: 'unimplemented_tool', error: 'This registered action has no executor.' };
  }
}

function unavailable(callback) {
  return { ok: false, code: 'executor_unavailable', error: `This action is unavailable (${callback}).` };
}
