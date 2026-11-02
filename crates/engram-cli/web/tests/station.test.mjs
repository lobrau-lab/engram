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

const { getRegisteredTool, validateToolArguments } = await import('../src/core/tool-registry.js');
const { formatToolDisclosure } = await import('../src/core/permission-gate.js');
const { executeStationAction } = await import('../src/core/app-actions.js');

// Test tool registration
const dispatchTool = getRegisteredTool('station_dispatch_render');
assert.ok(dispatchTool, 'station_dispatch_render tool should be registered');
assert.equal(dispatchTool.name, 'station_dispatch_render');
assert.equal(dispatchTool.destination, 'remote');

const fetchTool = getRegisteredTool('station_fetch_artifact');
assert.ok(fetchTool, 'station_fetch_artifact tool should be registered');
assert.equal(fetchTool.name, 'station_fetch_artifact');
assert.equal(fetchTool.destination, 'remote');

const replaceTool = getRegisteredTool('project_replace_file_content');
assert.ok(replaceTool, 'project_replace_file_content tool should be registered');

// Test argument validation
assert.equal(validateToolArguments(dispatchTool, { prompt: 'sunrise', type: 'video' }), null);
assert.ok(validateToolArguments(dispatchTool, { prompt: '' }));
assert.ok(validateToolArguments(dispatchTool, { prompt: 'sunrise', type: 'unsupported' }));

assert.equal(validateToolArguments(fetchTool, { filename: 'out.mp4' }), null);
assert.ok(validateToolArguments(fetchTool, {}));

// Test formatToolDisclosure
const disclosure = formatToolDisclosure(dispatchTool, { prompt: 'orbital sunset', type: 'video', duration_seconds: 5 });
assert.ok(disclosure.includes('orbital sunset'));
assert.ok(disclosure.includes('Duration: 5s'));

// Test executeStationAction without configured station
const unconfigured = await executeStationAction('station_dispatch_render', { prompt: 'test', type: 'image' }, {
  confirm: async () => true,
  getStationConfig: () => ({ host: '' })
});
assert.equal(unconfigured.ok, false);
assert.equal(unconfigured.code, 'station_not_configured');

// Test executeStationAction rejection by user
const rejected = await executeStationAction('station_dispatch_render', { prompt: 'test', type: 'image' }, {
  confirm: async () => false,
  getStationConfig: () => ({ host: '192.168.1.50' })
});
assert.equal(rejected.ok, false);
assert.equal(rejected.code, 'approval_denied');

// Test executeStationAction dispatch with mock fetch
const origFetch = globalThis.fetch;
try {
  globalThis.fetch = async (url, opts) => {
    if (url === '/api/v1/station/dispatch') {
      const parsed = JSON.parse(opts.body);
      assert.equal(parsed.prompt, 'render cyber city');
      return {
        ok: true,
        json: async () => ({
          ok: true,
          job: {
            id: 'job-987',
            job_type: 'video',
            status: 'running',
            output_filename: 'render-job-987.mp4'
          }
        })
      };
    } else if (url === '/api/v1/station/fetch') {
      const parsed = JSON.parse(opts.body);
      assert.equal(parsed.filename, 'render-job-987.mp4');
      return {
        ok: true,
        json: async () => ({
          ok: true,
          path: 'Media/render-job-987.mp4',
          url: '/media/render-job-987.mp4',
          bytes: 4096,
          sha256: 'abc123def456'
        })
      };
    }
    throw new Error(`Unexpected fetch URL: ${url}`);
  };

  const dispatchResult = await executeStationAction('station_dispatch_render', {
    prompt: 'render cyber city',
    type: 'video',
    duration_seconds: 5
  }, {
    actionId: 'action-1',
    confirm: async () => true,
    getStationConfig: () => ({ host: '192.168.1.50' })
  });
  assert.equal(dispatchResult.ok, true);
  assert.equal(dispatchResult.success, true);
  assert.equal(dispatchResult.job_id, 'job-987');
  assert.deepEqual(dispatchResult.expected_artifacts, ['render-job-987.mp4']);

  const fetchResult = await executeStationAction('station_fetch_artifact', {
    filename: 'render-job-987.mp4'
  }, {
    actionId: 'action-2',
    confirm: async () => true,
    getStationConfig: () => ({ host: '192.168.1.50' })
  });
  assert.equal(fetchResult.ok, true);
  assert.equal(fetchResult.success, true);
  assert.equal(fetchResult.url, '/media/render-job-987.mp4');
  assert.equal(fetchResult.embed, '[/media/render-job-987.mp4]');
} finally {
  globalThis.fetch = origFetch;
}

console.log('station state and agent action tests passed');
