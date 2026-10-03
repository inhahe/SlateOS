## B-PATCH-REFUSES-EVERY-FILE-THAT-IS-NOT-VALID-UTF-8 (lane B, 2026-09-14)

**Status:** FIXED 2026-09-14 · `userspace/coreutils/src/bin/patch.rs`

**How it was closed.** `patch` carries lines as `Vec<u8>` end to end now:
`HunkLine`, `FilePatch`'s two paths and its header lines, and the ~14 `&str`
signatures between them. The three `fs::read_to_string` calls became
`fs::read`, `env::args()` became `args_os()`, and the ASCII structure a patch
is made of (`@@`, `---`, `+++`, the column-one marker) is matched on byte
literals. A private `mod bytes` supplies the six `str` operations that have no
slice counterpart; `coreutils::quote`'s `os_bytes`/`os_from_bytes` do the
syscall boundary, so the conversion added no `unsafe`.

**Measured, not asserted.** `scripts/patch-diff.sh` runs 68 cases against real
GNU patch 2.7.6 and compares the resulting *tree* — every file's mode, size
and checksum — not just the three streams: **68 passed, 0 differed**. Three of
those cases are new and are the bug itself, and the harness was probed in both
directions: restoring the UTF-8 requirement on the patch file turns exactly
those three red and nothing else. Six unit cases cover the same ground at the
function level, and reintroducing a decode inside `bytes::lines` turns three of
them red.

**One regression the harness caught that nothing else would have.** Routing
the strip-count error through `quote_glibc` printed `**** strip count 'abc' is
not a number`; GNU prints it unquoted. GNU quotes an option *name*
(`invalid option -- 'Q'`) and not this value, which is not a rule anyone would
guess — §371 again, and the reason the reference is run rather than recalled.

Measured, both sides, on the same two files:

    $ patch -i u.diff orig.txt          # orig.txt line 2 holds byte 0xE9
    patch: **** Can't open patch file u.diff : stream did not contain valid UTF-8
    exit 2                                                        # ours

    $ patch -i u.diff orig.txt
    patching file orig.txt
    exit 0, byte 0xE9 preserved                                   # GNU patch 2.7.6

`patch` cannot apply a patch to a file that is not valid UTF-8, and cannot
even *read* a patch file that is not — the 0xE9 above is in a context line, so
the refusal happens before any target is opened. A single Latin-1 byte
anywhere in a source file's comments is enough. `patch` is on the image.

### This is the bug the argv finding was a symptom of

Yesterday's entry (`B-FOUR-STAGED-UTILITIES-STILL-DIE-ON-A-LEGAL-FILENAME`)
scoped this as an argv problem and proposed routing the four bins through
`coreutils::getopt`. That scoping was too small, and the reason is worth
keeping: I found the argv panic with a detector that only looks at argv, so
the answer it gave was shaped like its question. Reading the downstream uses
to plan the conversion is what turned it up — `target_file` flows into a
`String` that comes from `fs::read_to_string`, which meant the `String` was
never really about argv at all.

Both faults are the same cause and the fix is one job, not two:

| | Chokepoint | Symptom |
|---|---|---|
| 1 | `env::args()` at line 1054 | panic on a non-Unicode *filename* |
| 2 | `fs::read_to_string` ×3 | refusal on non-UTF-8 *content*, the above |

Chokepoint 2 is the larger of the two by any measure a user would apply: a
non-Unicode filename is rare, a Latin-1 byte in a patched file is not.

### The proper fix, and why it is not a boundary patch

`patch` is `String`-typed through its spine, not just at its edges:
`HunkLine::{Context,Remove,Add}(String)`, `FilePatch { old_path: String,
new_path: String, header_lines: Vec<String> }`, and `apply_hunk(lines:
&[String])` / `try_hunk_at` / `strip_path` / `parse_file_path` and ~11 more
`&str` signatures. There is no encode/decode boundary to move, because the
line *contents* are what must stay exact and they are carried the whole way.
So the fix is to carry lines as `Vec<u8>`/`&[u8]` and keep the ASCII
structural parsing (`@@`, `---`, `+++`, `+`/`-`/space) on byte literals.

Per CLAUDE.md §7 this is what should have been written in the first place:
*"Never force UTF-8 on filesystem paths, environment variables, or pipe
data."* Patch content is file data, which is the same rule.

### Reproduction

    python -c "
    open('orig.txt','wb').write(b'first line\ncaf\xe9 comment\nthird line\n')
    open('u.diff','wb').write(b'--- orig.txt\n+++ orig.txt\n@@ -1,3 +1,3 @@\n first line\n caf\xe9 comment\n-third line\n+THIRD LINE\n')
    "
    patch -i u.diff orig.txt

GNU exits 0 and leaves `THIRD LINE` with the 0xE9 untouched. Keep this as the
acceptance case: it fails on the *patch* file, so it also covers chokepoint 1's
sibling — a patch whose own `---` path is not UTF-8.

### WITHDRAWN: the "GNU patch -Q exits 0" note that was here

An earlier revision of this entry recorded that GNU `patch -Q` prints
`patch: invalid option -- 'Q'` and **exits 0** where ours exits 2, flagged as
wanting deliberate re-measurement. It has now been re-measured and **there is
no such divergence** — GNU exits non-zero, same as us, and
`scripts/patch-diff.sh` case `u.patch -p1 -Q` passes on both sides
independently.

The note was an artifact of the instrument, not an observation. See
`TD-B-EVERY-EXIT-CODE-I-MEASURED-THROUGH-WSL-WAS-THE-SAME-ZERO`. Left in
place rather than deleted because a withdrawn finding is worth more than a
missing one: whoever reads this next would otherwise re-measure it.
