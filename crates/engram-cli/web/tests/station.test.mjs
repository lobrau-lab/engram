import assert from 'node:assert/strict';

const WORKSPACE_KEY = 'threadline.workspace.v1';
const PENDING_KEY = 'threadline.workspace.v1.pending';
globalThis.localStorage = {
  data: new Map([[WORKSPACE_KEY, JSON.stringify({ settings: {}, conversations: [], activeId: null })]]),
  getItem(key) { return this.data.get(key) ?? null; },
  setItem(key, value) { this.data.set(key, String(value)); },
  removeItem(key) { this.data.delete(key); }
};

const { getWorkspace, updateSettings, getWorkspaceStatus } = await import('../src/core/store.js');
const { createStationJob, dispatchPayload, statusLabel, updateStationJob } = await import('../src/core/station-state.js');

assert.deepEqual(getWorkspace().settings.station, {
  host: '', user: '', port: 22, key_path: '', remote_workspace: '~/.engram-station', prefer_pinokio: false
});
updateSettings({ station: { host: '192.168.1.100', user: 'renderbot', port: 2222 } });
assert.equal(getWorkspace().settings.station.host, '192.168.1.100');
assert.equal(getWorkspace().settings.station.port, 2222);
assert.equal(localStorage.getItem(PENDING_KEY), '1');

const job = createStationJob('image', 'A test render');
assert.equal(typeof job.id, 'string');
assert.deepEqual(dispatchPayload(job, getWorkspace().settings.station), { ...job, station: getWorkspace().settings.station });
let jobs = [job];
jobs = updateStationJob(jobs, job.id, { status: 'running' });
assert.equal(jobs[0].status, 'running');
jobs = updateStationJob(jobs, job.id, { status: 'completed' });
assert.equal(statusLabel(jobs[0].status), 'Completed');
console.log('station state tests passed');
