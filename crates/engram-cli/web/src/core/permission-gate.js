/**
 * Per-invocation capability and approval gate for registered tools.
 * Remote access and consequential actions fail closed unless each invocation
 * receives a fresh confirmation with its data and destination disclosed.
 */
const EFFECT_POLICIES = new Map([
  ['ui_navigation', 'none'],
  ['conversation_read', 'none'],
  ['local_state_write', 'none'],
  ['local_settings_write', 'confirm'],
  ['destructive_local_delete', 'confirm'],
  ['external_read', 'confirm'],
  ['external_write', 'confirm'],
  ['local_file_read', 'confirm'],
  ['local_file_write', 'confirm'],
  ['local_file_delete', 'confirm'],
  ['shell_execution', 'confirm'],
  ['application_control', 'confirm']
]);

const RISK_LEVELS = new Set(['low', 'medium', 'high', 'critical']);

export function authorizeToolInvocation(tool, args = {}, callbacks = {}) {
  if (!tool) return denied('unknown_tool', 'This tool is not registered.');
  const minimumApproval = EFFECT_POLICIES.get(tool.effect);
  if (!minimumApproval) return denied('unknown_effect', 'The tool has no recognized effect policy.');
  if (!['none', 'confirm'].includes(tool.approval)) {
    return denied('unknown_approval_policy', 'The tool has no recognized approval policy.');
  }
  if (!RISK_LEVELS.has(tool.risk) || !tool.capability || !Array.isArray(tool.dataClasses) ||
      !tool.dataClasses.every(item => typeof item === 'string' && item.trim())) {
    return denied('incomplete_capability', 'The tool is missing required capability, risk, or data-access metadata.');
  }
  if (!['local', 'remote'].includes(tool.destination)) {
    return denied('unknown_destination', 'The tool has no recognized destination.');
  }
  if (tool.destination === 'remote' && !tool.destinationLabel) {
    return denied('unknown_remote_target', 'The remote service is not identified.');
  }
  if (minimumApproval === 'confirm' && tool.approval !== 'confirm') {
    return denied('approval_policy_mismatch', 'This action requires confirmation under its capability policy.');
  }

  const mustConfirm = minimumApproval === 'confirm' || tool.destination === 'remote' || ['medium', 'high', 'critical'].includes(tool.risk);
  if (!mustConfirm) return { allowed: true };
  if (typeof callbacks.confirm !== 'function') {
    return denied('approval_unavailable', 'This action requires a confirmation interface, but none is available.');
  }

  const disclosure = formatToolDisclosure(tool, args);
  let accepted = false;
  try {
    accepted = callbacks.confirm(disclosure) === true;
  } catch {
    return denied('approval_unavailable', 'The confirmation interface failed; the action was blocked.');
  }
  if (!accepted) {
    return { allowed: false, code: 'approval_denied', cancelled: true, message: 'The user declined this action.' };
  }
  return { allowed: true, disclosure };
}

export function formatToolDisclosure(tool, args = {}) {
  const destination = tool.destination === 'remote'
    ? `Remote service: ${tool.destinationLabel}`
    : 'Destination: this device';
  const data = tool.dataClasses.length ? tool.dataClasses.join(', ') : 'No user data declared';
  let serialized;
  try { serialized = JSON.stringify(args); }
  catch { serialized = '[parameters could not be serialized]'; }
  const parameters = serialized.length > 1200 ? `${serialized.slice(0, 1200)}… [truncated]` : serialized;
  return [
    tool.confirmation || `Allow ${tool.name}?`,
    '',
    `Capability: ${tool.capability}`,
    `Risk: ${tool.risk}`,
    `Data accessed or sent: ${data}`,
    destination,
    `Requested parameters: ${parameters}`
  ].join('\n');
}

function denied(code, message) {
  return { allowed: false, code, message };
}
