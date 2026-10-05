### TD-EDITOR-UTF8. Text editors reject non-UTF-8 files (`fs::read_to_string`) — LOW PRIORITY / graceful failure 2026-07-02

**Where:** `apps/editor/src/main.rs` and `apps/markdowneditor/src/main.rs`
(`Document::from_file`), which load file content via
`std::fs::read_to_string(path)`.

**The limitation.** `read_to_string` returns `Err` for any file whose bytes are
not valid UTF-8, so both editors *refuse to open* a non-UTF-8 (e.g. Latin-1,
UTF-16, or binary) file. This is a **graceful failure** (the open is rejected
cleanly — there is no silent `from_utf8_lossy` corruption, which CLAUDE.md rule 7
forbids), so it is a limitation rather than a correctness bug. Discovered while
implementing the external-change merge feature (todo2.txt line 1), which reuses
the same String/`Vec<String>` line model.

**Why deferred, not fixed now.** Both editors store the document as
`lines: Vec<String>` and operate on `&str` throughout (rendering, cursor math,
find/replace, the diff/merge engine `diffcore` which is `String`-based). Truly
handling arbitrary bytes would require converting the whole editor + `diffcore`
to a byte-oriented buffer model with an explicit encoding/decoding layer — a
large refactor. For a *plain-text* editor, UTF-8 is a defensible domain
assumption and clean rejection of non-text files is acceptable behavior, so the
refactor is disproportionate to the value. CLAUDE.md rule 7's core concern
(OS-boundary metadata: paths, env, pipe data handled as bytes; no
`from_utf8_lossy`) is already honored — this is document *content*, and the
failure mode is safe.

**Proper fix (if a concrete need appears — e.g. editing config files in a legacy
encoding):** give the editor a byte buffer + a detected/selectable encoding
(UTF-8 default, with a lossless round-trip for at least Latin-1/UTF-16), decode
for display, re-encode on save, and thread the same through `diffcore`
(byte-slice diff). Trigger: a user report of a real file that won't open, or an
explicit requirement to edit non-UTF-8 documents.
