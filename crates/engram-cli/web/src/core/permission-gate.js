/**
 * Per-invocation approval gate for registered tools. There are no persistent
 * grants: a tool that requires approval must receive a fresh UI confirmation.
 */
const EFFECT_POLICIES = new Map([
  ['ui_navigation', 'none'],
  ['conversation_read', 'none'],
  ['local_state_write', 'none'],
  ['local_settings_write', 'confirm'],
  ['destructive_local_delete', 'confirm']
]);

export function authorizeToolInvocation(tool, callbacks = {}) {
  if (!tool) return { allowed: false, code: 'unknown_tool', message: 'This tool is not registered.' };
  const minimumApproval = EFFECT_POLICIES.get(tool.effect);
  if (!minimumApproval) {
    return { allowed: false, code: 'unknown_effect', message: 'The tool has no recognized effect policy.' };
  }
  if (!['none', 'confirm'].includes(tool.approval)) {
    return { allowed: false, code: 'unknown_approval_policy', message: 'The tool has no recognized approval policy.' };
  }
  if (minimumApproval === 'confirm' && tool.approval !== 'confirm') {
    return { allowed: false, code: 'approval_policy_mismatch', message: 'This action requires confirmation under its effect policy.' };
  }
  if (tool.approval === 'none') return { allowed: true };
  if (typeof callbacks.confirm !== 'function') {
    return { allowed: false, code: 'approval_unavailable', message: 'This action requires confirmation, but confirmation is unavailable.' };
  }
  if (!callbacks.confirm(tool.confirmation || `Allow ${tool.name}?`)) {
    return { allowed: false, code: 'approval_denied', cancelled: true, message: 'The user declined this action.' };
  }
  return { allowed: true };
}
