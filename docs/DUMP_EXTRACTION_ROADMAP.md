# Dump Inbox Extraction Roadmap

## Current milestone

Engram Dump accepts pasted text and explicitly selected `.md`, `.markdown`,
and `.txt` files. It stores immutable local intake records before analysis.
The core now validates and stores versioned, source-hash-linked structured
suggestions after an explicit local **Analyze** call. Review and apply UI/API
The review page now persists accept/reject decisions and supports a safe
`create_note` apply operation with source-hash checks, destination conflict and
symlink checks, atomic writes, provenance frontmatter, audit records, and
idempotent retries. Append/link/tag proposals remain visible but are not yet
eligible for application.

The import store now preserves explicitly selected files under
`.engram/imports/originals/`, records per-file status/provenance, and supports
local Markdown, plain text, and HTML extraction. Successful extraction creates
a linked Dump source item; it does not write a vault note. PDF, including
scanned or encrypted PDF, is currently preserved as `unsupported` because no
validated local PDF adapter is enabled.

Each item reports its own extraction and analysis state. Pending data and
derived analysis remain under `.engram/dump/` and are not indexed as normal
vault notes. Failed analysis leaves the raw item unchanged and retryable.

## Planned adapters

| Phase | Inputs | Status shown |
|---|---|---|
| MVP | Pasted text, Markdown, plain text | Extracted, size estimate, truncation warning |
| Documents | Selectable-text PDF, DOCX, HTML, RTF | Extracted, partial, or failed with page/section warnings |
| Structured data | CSV, XLSX, JSON | Tables/sheets, row limits, selected columns, skipped rows |
| Images and scans | PNG/JPEG, scanned PDF | OCR/vision coverage, confidence, original source |
| Audio and video | Common audio/video | Model, duration, timestamps, skipped segments |
| Archives and mail | ZIP/TAR, EML/MBOX | Included/excluded entries and safe-path results |

No format is considered supported merely because a library can open it. Each
adapter must report extraction behavior, limits, malformed-input handling,
platform availability, and provenance before it is enabled.

## Required per-item record

Every intake item retains an immutable ID, content hash, original filename and
source type, timestamp, byte size, extractor/version, status, extracted text,
warnings, truncation details, source map, model request IDs, and related vault
note IDs. Extraction status and analysis status remain separate.

## Safety rules

- Only explicitly selected files or an explicitly approved local intake folder
  are processed.
- No network fetching, macro execution, embedded-script execution, or
  unrestricted filesystem crawling.
- Size, batch, page/row, archive-entry, decompressed-size, and processing-time
  limits are visible before processing.
- Path traversal, symlink escapes, malformed files, unsafe archives, and
  unexpected binary content are rejected without deleting the source.
- Analyze identifies the configured endpoint before sending content.
- Approval creates a new Markdown note with provenance; it never modifies the
  original intake record.
