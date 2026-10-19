export const EXECUTION_STATES = Object.freeze(['proposal', 'validation', 'authorization', 'execution', 'result_validation', 'completed', 'denied', 'failed', 'cancelled']);

export function validateConnectorManifest(manifest) {
  const required = ['id', 'version', 'capabilities', 'effect', 'risk', 'destination', 'dataClasses', 'timeoutMs', 'idempotency', 'executorId'];
  const missing = required.filter(key => manifest?.[key] == null);
  if (missing.length) return { valid: false, code: 'incomplete_manifest', missing };
  if (!Number.isInteger(manifest.version) || manifest.version < 1) return { valid: false, code: 'invalid_version' };
  if (!Array.isArray(manifest.capabilities) || manifest.capabilities.some(value => typeof value !== 'string' || !value)) return { valid: false, code: 'invalid_capabilities' };
  if (!['read', 'local_write', 'external_read', 'external_write', 'delete', 'app_control'].includes(manifest.effect)) return { valid: false, code: 'unknown_effect' };
  if (!['low', 'medium', 'high', 'critical'].includes(manifest.risk)) return { valid: false, code: 'invalid_risk' };
  if (!['local', 'remote'].includes(manifest.destination)) return { valid: false, code: 'unknown_destination' };
  if (manifest.destination === 'remote' && !manifest.destinationLabel) return { valid: false, code: 'missing_remote_destination' };
  if (!Array.isArray(manifest.dataClasses) || manifest.dataClasses.some(value => typeof value !== 'string')) return { valid: false, code: 'invalid_data_classes' };
  if (!Number.isSafeInteger(manifest.timeoutMs) || manifest.timeoutMs < 1 || manifest.timeoutMs > 120000) return { valid: false, code: 'invalid_timeout' };
  if (!['idempotent', 'non_idempotent', 'unknown'].includes(manifest.idempotency)) return { valid: false, code: 'invalid_idempotency' };
  return { valid: true };
}

export async function executeGuardedInvocation({ proposal, connector, validateInput, authorize, signal }) {
  const trace = [{ state: 'proposal', at: new Date().toISOString() }];
  const manifestResult = validateConnectorManifest(connector?.manifest);
  if (!manifestResult.valid) return { ok: false, state: 'denied', trace: [...trace, { state: 'denied', reason: manifestResult.code }], error: manifestResult };
  trace.push({ state: 'validation', at: new Date().toISOString() });
  const inputError = validateInput?.(proposal.args);
  if (inputError) return { ok: false, state: 'denied', trace: [...trace, { state: 'denied', reason: 'invalid_input' }], error: inputError };
  if (signal?.aborted) return { ok: false, state: 'cancelled', trace: [...trace, { state: 'cancelled' }] };
  trace.push({ state: 'authorization', at: new Date().toISOString() });
  if (authorize && !(await authorize({ proposal, manifest: connector.manifest }))) return { ok: false, state: 'denied', trace: [...trace, { state: 'denied', reason: 'authorization' }] };
  if (signal?.aborted) return { ok: false, state: 'cancelled', trace: [...trace, { state: 'cancelled' }] };
  trace.push({ state: 'execution', at: new Date().toISOString() });
  let output;
  try { output = await connector.execute(proposal.args, { signal }); }
  catch (error) { return { ok: false, state: 'failed', trace: [...trace, { state: 'failed' }], error: { code: 'connector_failed', message: String(error?.message || error).slice(0, 300) } }; }
  trace.push({ state: 'result_validation', at: new Date().toISOString() });
  const resultError = connector.validateOutput?.(output);
  if (resultError) return { ok: false, state: 'failed', trace: [...trace, { state: 'failed', reason: 'invalid_result' }], error: resultError };
  return { ok: true, state: 'completed', trace: [...trace, { state: 'completed', at: new Date().toISOString() }], output };
}
