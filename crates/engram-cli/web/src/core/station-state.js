export function createStationJob(type, prompt) {
  return { id: crypto.randomUUID(), type, prompt, params: {}, status: 'queued' };
}

export function dispatchPayload(job, station) {
  return { ...job, station };
}

export function updateStationJob(jobs, id, patch) {
  return jobs.map(job => job.id === id ? { ...job, ...patch } : job);
}

export function statusLabel(status) {
  return String(status || 'queued').replace(/^./, character => character.toUpperCase());
}
