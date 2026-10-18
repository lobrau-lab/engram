# Crash-Safe Engram Metadata Writes

`sync::save_manifest` and `sync::save_local_state` serialize to a temporary
file in the destination’s parent directory, using exclusive creation. The
complete payload is flushed and `sync_all` is called before replacement.

Replacement is performed without deleting the existing destination:

- Unix uses same-directory `rename`, which atomically changes the visible name.
- Windows uses `MoveFileExW` with replace and write-through flags.
- The containing directory is synced on Unix. Windows has no portable
  directory-fsync equivalent in this helper; write-through is requested for
  the replacement instead.

If writing or replacement fails before the name change, the temporary file is
removed and the previous destination is left intact. Temporary names use the
strict `.engram-atomic-tmp-` marker and are not cleaned up at startup.

This guarantees atomic visibility after successful replacement: readers see
either the previous complete file or the new complete file. It is not an
absolute sudden-power-loss guarantee; durability depends on the operating
system, filesystem, storage hardware, and their handling of flushes and
directory metadata. Note-editor writes are outside this helper.
