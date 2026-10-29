# Evaluation: Reintroducing a Native Wrapper

## Overview
This document evaluates the architectural implications and process requirements for reintroducing a native wrapper (such as Tauri or Electron) to the Threadline application.

## Current Architecture
The current architecture strictly relies on the Threadline web client served directly from the core Rust application (`engram-cli`), which is opened in the system's default browser. As documented in `docs/HANDOFF.md`, `README.md`, and `docs/superpowers/specs/2026-10-20-threadline-personal-agent.md`, there is intentionally no Tauri layer or native wrapper in the current path.

The decision to exclude a native wrapper minimizes dependencies, reduces build complexity, and maintains the security boundaries within a single Rust process.

## Architectural Implications of a Native Wrapper
Reintroducing a native wrapper (e.g., Tauri) would introduce significant architectural shifts:
1.  **Dependency Overhead:** It would require adding heavy UI framework dependencies (like Tauri) to the build process.
2.  **Security Boundaries:** The security model, which currently relies on standard browser sandbox mechanisms and isolated server endpoints, would need to be re-evaluated to ensure the native wrapper respects the same boundaries without introducing new vulnerabilities.
3.  **Delivery Model:** It changes the delivery model from a simple binary that launches a browser to a packaged desktop application, requiring additional signing, notarization, and distribution workflows.
4.  **Consistency:** If a native wrapper is added, it must strictly call the existing HTTP endpoints and reuse the current session/token model, ensuring the vault, inference, and capability execution remain within the existing process boundaries.

## Product Invariant & Owner Approval
As explicitly stated in `docs/COMPLETION_PLAN.md` (Section 3.6 - Stop and escalate):

> Stop and ask the owner before:
> - Changing a product invariant (Threadline-first, no Tauri, broad armed takeover, review-only Workshop, no cloud).

Furthermore, `docs/HANDOFF.md` reinforces:

> Reintroducing a native wrapper is a product decision, not a bug fix.

Therefore, no engineering action can be taken to implement a native wrapper without explicit, prior approval from the product owner. This evaluation serves to document the required escalation path.

## Next Steps
1.  Escalate the request to the product owner.
2.  Await explicit authorization before proceeding with any implementation.
