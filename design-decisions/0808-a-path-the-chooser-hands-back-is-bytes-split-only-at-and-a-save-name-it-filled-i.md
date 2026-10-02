## 808. A path the chooser hands back is bytes, split only at `/`, and a save name it filled in keeps a copy of the bytes beside the text

**Date:** 2026-09-03
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** a filename on SlateOS is a string of bytes, not text — every byte
but `/` and NUL is allowed, so plenty of legal filenames cannot be written down
in a `String` at all. The file chooser used to store them as `String`s anyway,
which meant a name it could not read was quietly replaced with question-mark
characters and the file the user picked was no longer the file that got opened.
Fixing that raised two questions with real arguments on both sides: what to use
to cut a path into a directory and a filename, and what to do about the Save
box, where the name genuinely *is* text because the user types into it.

### Decision 1 — cut paths with our own byte code, not with `std::path`

Rust's `Path::parent` and `Path::join` are the obvious tools and they were
rejected. The whole test suite runs on a Windows *host*, and on Windows those
functions also treat `\` as a separator. `\` is a perfectly legal character in
a SlateOS filename, so `Path::parent("/docs/a\b.txt")` answers `/docs/a` on the
host and `/docs` on the target — the tests would pin the wrong behaviour and
pass.

| | `std::path` | hand-written byte split |
|---|---|---|
| *What changes:* a file named `a\b.txt` | its directory is reported as `a` | its directory is reported correctly |
| Amount of code | none | ~30 lines, three functions |
| `unsafe` | none | three `from_encoded_bytes_unchecked` calls |
| Agreement between host tests and target | none — they differ | exact |

The `unsafe` is the price and it is a narrow one: every cut is made at an ASCII
`/`, which cannot occur inside a multi-byte sequence in any encoding `OsStr`
uses, so both halves of the cut are always valid. Each call carries a `SAFETY:`
comment saying exactly that. `PathBuf` is still the public type — only the
*splitting* is ours.

`apps/diskimager` keeps its own `parent_directory`, which deliberately splits on
both separators, because it is fed paths a *host* handed the program; that is a
different job and its own comment says so.

### Decision 2 — the Save box remembers the bytes it was filled with

The Save box's text field has to be a `String`: the user edits it a character at
a time. But clicking a listed file to overwrite it fills that field from the
listing, and a name with no UTF-8 spelling would come back out as a *different*
name — creating a new file beside the one the user pointed at rather than
replacing it.

| Option | *What changes* |
|---|---|
| Leave it lossy | overwriting a file with an unusual name silently creates a second file next to it |
| Make the field an `OsString` | the text editing code (insert, backspace, cursor) has to work on bytes it cannot index safely |
| **Keep a second copy of the exact bytes** (chosen) | overwrite hits the clicked file; typing anything at all falls back to the typed text |

The chosen shape is `filename_exact: Option<OsString>`, set when the field is
filled from a listing and cleared by `edited_filename()` on the first keystroke
that actually changes the text. Clearing it on edit is not a detail: kept past
an edit, it would overwrite the clicked file regardless of what the user then
typed — a worse failure than the one being fixed, because it ignores an explicit
instruction rather than mangling an implicit one.

The cost is a second field that has to be invalidated in every place the text
can change. There are two such places — the backspace arm and the
typed-character arm of the key handler — both go through the one
`edited_filename()` helper, and
`typing_in_the_name_field_drops_the_remembered_bytes` drives a real keystroke
through each of them and fails if either forgets. (The typed-character arm
invalidates only when the text actually changed: most keys that reach it — a
bare Shift, an unhandled function key — type nothing, and dropping the exact
name on one of those would turn a harmless keypress into a silently different
save target.)

### Reversing either

Decision 1 should not be reversed while the tests run on a Windows host; if they
ever move to the target, `std::path` becomes correct there and the byte helpers
could go. Decision 2 is contained: deleting `filename_exact` and its two setters
restores the old behaviour, and the named test above says what is lost.
