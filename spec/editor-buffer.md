# Editor Buffer and Save Contract

**English** · [简体中文](./editor-buffer.zh-CN.md)

This contract applies to the shared Rust editor. Consult the [user guide](../docs/user-manual.md) for the current desktop integration.

## Source and preview

- Keep all UTF-8 input, including whitespace, line endings, key order, and unknown extension fields. Reject non-UTF-8 data without replacing bytes.
- Change the last valid document only after JSON and the pinned résumé schema both pass. Keep invalid input and its diagnostics in the buffer. It must never replace the last valid preview or be saved as the résumé.
- An invalid file can open in a repair buffer with no valid preview. After repair, the original byte revision is still used for the save.
- Compare the current bytes with the last saved SHA-256 to determine whether edits are unsaved. Undoing to the same bytes returns the buffer to a clean state.

## Formatting and undo

Formatting uses two spaces and a single final LF. It preserves key order and extension fields. Syntactically valid JSON can be formatted before schema repair. Invalid syntax must leave the text intact. Validation and formatting are capped at 16 MiB. Formatting cannot add fields, coerce types, introduce styling, or sanitize user-authored strings. Parse, validate, and format numbers with arbitrary precision. Equivalent numeric spellings are acceptable after formatting. Unedited text and save snapshots retain exact bytes.

Formatting and each structure operation form their own undo transaction, including quick successive clicks. A new edit after undo clears the old redo branch. The input component owns history, caret, selection, and IME state. The buffer has no second history. Formatting and structure edits wait for IME composition to finish. A platform IME commit through unmark alone must update the buffer, preview, and recovery draft immediately.

The read-only gutter index uses UTF-8 byte offsets for standard sections and entries. It recognizes escaped keys, CJK, emoji, CRLF, compact text, and extension fields. Brackets inside strings and arrays inside Basics are not mistaken for structure. Hide gutter actions for invalid syntax or schema, duplicate root keys, more than 50,000 anchors, or active composition.

Use the input component's current layout coordinates for soft wraps, scrolling, and clipping. Actions sharing a visual row appear in named add/remove groups. Menus bind to the input entity and display generation. Changing files or text, entering composition, or invalidating the document invalidates old actions. Insertions can begin at the source anchor or in the sections panel. Esc restores text focus without a history entry.

Structure edits trim the common prefix and suffix and replace only the changed UTF-16 range at Unicode character boundaries. Do not split emoji or surrogate pairs. This keeps undo close to the edited location. Validation, revision checks, and whole-text snapshots still apply. See [third-party notices](../licenses/THIRD_PARTY_LICENSES.md) for the vendored input component.

## Save an existing document

1. Capture valid source text and the last saved revision.
2. GUI full-text saves and CLI patches normalize the target and share a sidecar lock. Cooperating writers cannot commit the same revision concurrently.
3. Read and check the disk revision while holding the lock and check again before replacement. Reject external changes, unreadable files, and deletions rather than recreating or overwriting them.
4. Write the captured bytes by atomic replacement in the same directory. Keep normal Unix permission bits and save through a symlink to the actual target rather than replacing the link.
5. Update the baseline to the committed snapshot. Later input remains unsaved, and an old completion event cannot roll back a newer baseline.

Keep `.<filename>.resume-one.lock` after releasing it. Invalid content and revisions fail before lock creation. Lock acquisition is nonblocking. A busy lock returns `RESUME_BUSY`, leaving bytes, baseline, and draft intact. The GUI exits its saving state, and the CLI returns exit code 5. Read the revision again before retrying. This limit applies to lock contention. OS file I/O can still take time. Sync tools may ignore the lock. Checking right before replacement narrows the race without providing filesystem compare-and-swap against other programs. Windows replacement and permission handling require their own acceptance checks.

## Create and save as

`create_resume` validates the document, writes and syncs a complete temporary file in the chosen directory, then commits without overwriting. An existing target, including a dangling link, a nonwritable parent, or invalid data causes failure without affecting other files. At most one concurrent creation may succeed. An existence check followed by an overwriting rename is insufficient.

## Local draft recovery

Drafts store original UTF-8, including temporarily invalid input, and the original file revision in app data, separately from the résumé. The limit is 32 MiB. Slots derive from SHA-256 of native path bytes, with a separate slot for the unnamed sample. The Unix recovery directory uses mode `0700`.

A store serializes writes by increasing generation. Save and explicit discard invalidate older queued writes so a removed draft cannot reappear. Recover only after asking the user, keeping the draft revision so later external changes still cause a conflict. Desktop recording, prompts, and a pre-exit flush are implemented. Unexpected exits and sync-folder conflicts require platform-specific acceptance.
