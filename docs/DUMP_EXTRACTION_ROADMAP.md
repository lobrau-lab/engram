# Dump Inbox Extraction Roadmap

## Current milestone

Engram Dump accepts pasted text and explicitly selected `.md`, `.markdown`,
and `.txt` files. It stores immutable local intake records before analysis.
The organizer model receives only successfully extracted text after the user
explicitly selects **Analyze**.

Each item reports its own extraction and analysis state. Pending data remains
under `.engram/dump/` and is not indexed as a normal vault note.

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
