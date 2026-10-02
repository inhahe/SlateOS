## TD-C-DRAGGED-FILE-PATHS-CANNOT-CARRY-A-NAME-THAT-IS-NOT-TEXT -- FIXED 2026-09-16

**In short:** the format used to carry files between applications during a
drag holds each path as text. A file whose name is not text cannot be dragged
at all -- and worse, one such file in a selection makes the receiving
application see *nothing*, not even the other nine files that were fine.

**Date:** 2026-09-16. **Lane:** C.

**Where.** `gui/toolkit/src/dnd.rs` -- `DataObject::with_files(paths: &[&str])`
and `DataObject::get_file_paths(&self) -> Option<Vec<&str>>`.

**The all-or-nothing part is the sharp edge.** `get_file_paths` runs
`core::str::from_utf8` over the *whole* blob and returns `None` if it fails. So
the failure is not "the odd file is missing"; it is "the drop did nothing",
with no indication which file was responsible. A user dragging a folder's worth
of files would see the drop silently do nothing and have no way to find out
why.

**Why it is not urgent.** Nothing uses this format yet. The only callers are
the file's own tests -- explorer's internal drag carries `Vec<PathBuf>` through
`DropZoneManager` and is byte-correct already. This is the *cross-application*
format, and it bites the first program that reaches for it.

**The separator half is already fixed (2026-09-16).** Paths were joined with a
newline, and a newline is legal in a SlateOS name, so a file called
`notes<LF>draft.txt` arrived as two paths that do not exist. Now NUL, the one
byte a name cannot hold -- the same reasoning behind `find -print0`. Pinned by
`a_name_containing_a_newline_is_one_path_not_two`, and sabotage-checked:
restoring the newline splits that name into `["/home/user/notes",
"draft.txt"]`.

**FIXED the same day, and the fix was a decision that already existed.**
`DataObject::with_files` takes `&[&[u8]]`, `get_file_paths` returns
`Option<Vec<&[u8]>>`, and nothing validates UTF-8 anywhere on path data.

**The part worth reading is where the answer came from.** This entry said the
fix needed a decision about the host that it could not make. That was wrong --
not in the reasoning below, which still holds, but in the conclusion that the
decision was open. `kernel/src/fs/clipboard.rs` had already settled it on
2026-09-07: `set_files(&[&[u8]])`, NUL-separated, `get_files() ->
Vec<Vec<u8>>`. **Returning raw bytes is what makes it sound on both** -- the
caller converts, and the caller is the side that knows whether its `OsStr` is
bytes or WTF-8.

And the request that produced it was filed by **lane C**, to lane A:
`requests/c-a-the-system-clipboards-file-list-cannot-carry-our-own-paths.md`.
So this lane asked the question, got a complete answer nine days ago, and then
re-derived half of it -- the NUL separator -- from scratch while recording the
other half as undecided. The answer was sitting in `requests/`, in this
worktree, the whole time.

The lesson is not "read `requests/`", which is already a rule. It is narrower
and worth stating: **when a problem looks like it needs a new decision, check
whether the same problem has already been decided somewhere else in the
system.** A clipboard and a drag are the same problem wearing different names
-- a list of files crossing a process boundary -- and nothing about the phrase
"drag and drop" suggests looking in the clipboard. What suggested it was
scanning `requests/` for the word *byte*.

**The original reasoning, kept because it is still the argument for why the
accessor hands back bytes:**

* On SlateOS, and on any unix, an `OsStr` **is** bytes, so the fix is
  `OsStrExt::from_bytes` and it is safe.
* On the Windows machine the tests run on, `OsStr` is WTF-8, and arbitrary
  incoming bytes are **not** necessarily valid. `OsStr::from_encoded_bytes_unchecked`
  requires bytes that came from `as_encoded_bytes`, which bytes arriving from
  another process have not. So the conversion that is safe on the target is
  unsound on the host, and the tests run on the host.

That is the same host-versus-target split `gui/toolkit/src/osbytes.rs`,
`dialog::parent_path` and `apps/explorer/src/search.rs` all navigate, but with
the sign flipped: those control where the bytes came from, and this one does
not.

**The proper fix,** for whoever wires the first consumer: keep the wire format
as NUL-separated raw bytes, which is already true, and split the *accessor* the
way `quoting::os_bytes` splits -- `#[cfg(unix)]` returning `PathBuf` via
`OsStrExt::from_bytes`, and a `#[cfg(not(unix))]` host arm that validates as
UTF-8 and is honest that it is the host arm. `scripts/lossy-decode.py` already
recognises that shape and classifies it `HOST` rather than `VALUE`, which is
the evidence that it is the house pattern rather than an excuse.

**Do not "fix" this by making `get_file_paths` lossy.** Returning
`to_string_lossy` for the undecodable names would turn a visible nothing-happens
into an invisible wrong-file -- a drop that silently operates on a path the
user never selected.
