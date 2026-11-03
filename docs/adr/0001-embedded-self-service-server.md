# ADR 0001: Embedded Self-Service Server & Supervised Worker Architecture

- **Status**: Proposed
- **Date**: 2026-11-02
- **Context**: Transitioning Engram from an external-runner reliance (SSH remote station, external model servers) to an embedded self-service control plane.

---

## 1. Context and Problem Statement

Engram's Threadline workspace currently serves its web interface over local HTTP, but delegates heavy tasks (media generation, model inference) to external services:
- Media rendering requires an external machine running `runner.py` reached via SSH.
- Non-Apple model inference requires an external OpenAI-compatible server (e.g. oMLX, Ollama, vLLM).
- Static assets are compiled individually into the binary using `include_str!`, creating coupling and silent 404 runtime traps when new assets are introduced.
- Security relies heavily on browser-provided headers (`Host`, `Origin`, and `X-Threadline-Local-Control`), which does not scale safely to third-party tooling or multi-process local orchestration.

We need to embed a self-service server that can execute background tasks and heavy runtimes locally, without compromising stability, vault safety, or system responsiveness.

---

## 2. Decision Drivers

1. **Vault & Database Integrity**: Heavy compute (inference, diffusion, indexing) must never crash the main server, corrupt SQLite databases, or leave stale locks on vault notes.
2. **Resource Arbitration**: Unified Memory and GPU VRAM on host machines (especially Apple Silicon) are finite; concurrent execution must be budgeted centrally.
3. **Painless Local Onboarding**: Zero configuration for ports, authentication, and worker discovery; multiple open vaults must run simultaneously without port collisions.
4. **Resilience**: Workers are expendable and restartable; crashes yield structured error events, not app exit.
5. **Security by Default**: Cryptographic bearer tokens per session, capability-bounded workers, and strictly loopback-bound endpoints.

---

## 3. Architecture Decision

We adopt a **Supervised Control Plane with Expendable Worker Processes**:
- **Supervisor (Rust Core Binary)**: Owns vault files, SQLite indices, the HTTP/SSE server, memory budgeting, job scheduling, and bearer authentication.
- **Workers (Expendable Child Processes)**: Isolated processes spawned on demand for heavy runtimes (inference engines, diffusion runners, bulk embedding). Communication occurs over private IPC (Unix Domain Sockets or stdio JSON-RPC).
- **Assets**: Transition from manual `match` routing to an embedded manifest (`rust-embed`) with live filesystem fallback during development.
- **Runtime Discovery**: Ephemeral `vault/.engram/runtime.json` descriptor with atomic writes and `0600` permissions.

---

## 4. Schemas

### 4.1. Runtime Descriptor Schema (`vault/.engram/runtime.json`)

Written atomically by the supervisor on server startup; removed cleanly on shutdown.

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "EngramRuntimeDescriptor",
  "type": "object",
  "required": ["protocol_version", "vault_id", "pid", "port", "token", "started_at"],
  "properties": {
    "protocol_version": {
      "type": "integer",
      "minimum": 1
    },
    "vault_id": {
      "type": "string",
      "description": "Unique identifier of the opened vault"
    },
    "pid": {
      "type": "integer",
      "description": "Process ID of the supervisor"
    },
    "port": {
      "type": "integer",
      "minimum": 1024,
      "maximum": 65535
    },
    "token": {
      "type": "string",
      "description": "Cryptographically secure 256-bit hex bearer token for API authorization"
    },
    "ipc_socket": {
      "type": "string",
      "description": "Filesystem path to the supervisor IPC Unix domain socket"
    },
    "started_at": {
      "type": "string",
      "format": "date-time"
    }
  },
  "additionalProperties": false
}
```

### 4.2. Supervisor-Worker IPC Protocol Schema (JSON-RPC 2.0)

Transported over local Unix Domain Socket or newline-delimited stdio.

#### Requests & Commands (Supervisor -> Worker)

1. **`worker.init`**: Configure runtime limits, device allocation, and logging.
```json
{
  "jsonrpc": "2.0",
  "id": "init-1",
  "method": "worker.init",
  "params": {
    "worker_id": "inference-mlx-01",
    "capability": "chat_completions",
    "device": "metal:0",
    "memory_budget_bytes": 17179869184,
    "log_level": "info"
  }
}
```

2. **`job.submit`**: Dispatch a computation task to the worker.
```json
{
  "jsonrpc": "2.0",
  "id": "job-sub-101",
  "method": "job.submit",
  "params": {
    "job_id": "job-20261102-abc1234",
    "action": "render_media",
    "params": {
      "prompt": "orbital sunrise over frozen earth",
      "type": "video",
      "duration_seconds": 5,
      "resolution": "1080p"
    },
    "output_path": "/path/to/vault/Media/render_abc1234.mp4"
  }
}
```

3. **`job.cancel`**: Terminate a running job immediately.
```json
{
  "jsonrpc": "2.0",
  "id": "cancel-101",
  "method": "job.cancel",
  "params": {
    "job_id": "job-20261102-abc1234",
    "reason": "user_cancelled"
  }
}
```

#### Notifications & Events (Worker -> Supervisor)

1. **`job.progress`**: Streaming execution status.
```json
{
  "jsonrpc": "2.0",
  "method": "job.progress",
  "params": {
    "job_id": "job-20261102-abc1234",
    "stage": "sampling_frames",
    "progress": 0.45,
    "step": 18,
    "total_steps": 40,
    "estimated_remaining_seconds": 12.4
  }
}
```

2. **`job.completed`**: Successful job outcome.
```json
{
  "jsonrpc": "2.0",
  "method": "job.completed",
  "params": {
    "job_id": "job-20261102-abc1234",
    "artifacts": [
      {
        "filename": "render_abc1234.mp4",
        "path": "/path/to/vault/Media/render_abc1234.mp4",
        "bytes": 14285810,
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
      }
    ],
    "duration_seconds": 22.8
  }
}
```

3. **`job.failed`**: Structured failure report.
```json
{
  "jsonrpc": "2.0",
  "method": "job.failed",
  "params": {
    "job_id": "job-20261102-abc1234",
    "code": "vram_allocation_failed",
    "message": "Out of memory during model compilation",
    "retryable": true
  }
}
```

---

## 5. Security & Isolation Model

1. **No External Network Default**: Workers do not inherit network sockets. Local loopback requests require the bearer token.
2. **Capability Token Gates**: Workers cannot issue supervisor commands or read arbitrary vault notes. Supervisor writes inputs and collects outputs.
3. **Crash Supervision**: On worker crash or OOM, the supervisor logs worker stderr, releases reserved budget, notifies the UI with `worker_crashed`, and delays restart via exponential backoff.
