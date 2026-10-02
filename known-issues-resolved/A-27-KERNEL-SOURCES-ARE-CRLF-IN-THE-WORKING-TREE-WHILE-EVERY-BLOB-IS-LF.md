## A-27-KERNEL-SOURCES-ARE-CRLF-IN-THE-WORKING-TREE-WHILE-EVERY-BLOB-IS-LF (lane A, 2026-08-18) — **CLOSED 2026-09-05** (`design-decisions.md` §911)

**In short:** twenty-seven of the kernel's `.rs` files end their lines with
carriage-return + newline; every other file, and every version of *all* of them
stored in git, ends with newline alone. Git is configured in a way that makes
both spellings look identical to it, so `git status` is clean, `git diff` is
empty, and the compiler does not care. Nothing is broken until a tool reads the
files line by line — at which point it silently sees a stray `\r` glued to the
end of every line.

### How the two spellings coexist without git noticing

`core.autocrlf` is `input` in these worktrees. That setting converts CRLF to LF
**on check-in** and never converts anything **on checkout**. So:

- every blob in the repository is LF (verified: `git show HEAD:kernel/src/fs/handle.rs`
  contains 2044 LF and zero CRLF);
- a working-tree file that is LF hashes to that blob — clean;
- a working-tree file that is CRLF *also* hashes to that blob, because the
  check-in conversion strips the `\r` before hashing — also clean.

Both therefore report clean, forever, and no amount of `git status`/`git diff`
will ever surface the difference. Normalising the working copies produces a
**zero-line diff** for the same reason.

The 27 files were almost certainly written by something that opened them in
Python text mode on Windows, where `open(p, "w")` translates `\n` to `\r\n` on
write. That is a guess about provenance; the file list is not:

`lockdep.rs`, `drm/driver.rs`, `drm/ati/backend.rs`, `drm/ati/tests.rs`, and
under `fs/`: `atime.rs`, `bookmarks.rs`, `clipboard.rs`, `columnview.rs`,
`directio.rs`, `dragdrop.rs`, `fileinfo.rs`, `fileops.rs`, `findex.rs`,
`freeze.rs`, `fstrim.rs`, `handle.rs`, `pathbar.rs`, `prefetch.rs`,
`preview.rs`, `profile.rs`, `recent.rs`, `sealing.rs`, `sparse.rs`,
`templates.rs`, `thumbcache.rs`, `viewstate.rs`, `ext4/driver.rs`.

Each is *uniformly* CRLF — no file mixes the two, which is what makes the tool
fix below safe.

### How it showed up

`scripts/split-frames.py` refused `kernel/src/fs/handle.rs` outright with
`AssertionError: file has CRLF; this transformer assumes LF`, blocking the split
of that file's 12 288-byte `self_test` frame. The assertion was doing its job —
the transformer indexes offsets produced by `text.split("\n")` and emits `"\n"`
at a couple of dozen sites, so running it on CRLF text would have produced a
file with mixed endings — but it stated an internal invariant as if it were a
limit on what the tool accepts.

### Fixed in the tool (2026-08-18)

`split-frames.py` now normalises at its I/O boundary: `main` strips CRLF on read
and restores it on write, leaving `rewrite()` the pure LF function its assertion
describes. A file that *mixes* the two endings is still refused, deliberately —
unifying it would bury a whole-file whitespace change inside a commit whose
subject is a stack frame. `--check` still passes 23/23 and `fs/handle.rs` is now
accepted.

`rustfmt` was never affected: its default `newline_style = "Auto"` preserves
whatever a file already uses.

### Still to do

The tool is fixed; the condition is not, and it can recur — anything that writes
a kernel source through Python text mode on Windows re-creates it, and the next
line-based tool will hit the same wall with a less obvious error message. Two
candidate fixes, neither done:

1. **Normalise the 27 files.** Costs nothing in git terms (zero-line diff, per
   above) and makes the crate internally consistent. Does not prevent
   recurrence.
2. **Add a root `.gitattributes`** (`*.rs text eol=lf`) so checkout enforces the
   policy rather than leaving it to whatever wrote the file last. This is the
   durable fix, but `.gitattributes` is a repository-root file shared by all
   three lanes, and adding it would make every lane's CRLF working copies
   convert on their next checkout. It needs to be coordinated, not dropped in by
   one lane — file a request or raise it in `open-questions.md` first.

**Severity:** low — invisible to git, the compiler, and rustfmt; it costs a
confusing failure in a line-based script roughly once per script.

### Note appended by lane B, 2026-09-04: item 1 is done and item 2 is half-obsolete

Appended rather than edited in, since this is lane A's entry.

**Item 1 (normalise) is done in all three worktrees.** Measured with the widened
`check-eol` on 2026-09-04: `os-lane-a` **0 of 13 907** tracked files carry a CR,
`os-lane-b` 0, `os-lane-c` 65 (4 of them fatal, and fatal under the *old* gate
too). The 27 named above had grown to 168 in lane A's tree before that; whatever
lane A ran on 2026-09-04 cleared them. The repair is content-neutral for exactly
the reason this entry gives — the clean filter means the blob never differed —
so it shows as a zero-line diff.

**Item 2 is superseded for visibility, still open for prevention.** A-27 wanted
`*.rs text eol=lf` primarily so the condition would stop being invisible.
`design-decisions.md` §769 achieves that differently: `scripts/check-eol.py` now
reads **every tracked file** rather than only the ones `.gitattributes` declares,
so a CRLF `.rs` is reported without any attribute existing. The measurement that
forced it is the one this entry predicted — of the declared files, 0 had a CR; of
the tracked `.rs`/`.toml` the declarations exclude, 27 did.

That removes A-27's *reason* for item 2 but not item 2's own merit: attributes
act at checkout, and the gate acts after the fact, so `*.rs text eol=lf` would
still prevent rather than detect. §769 explicitly declines to make that change —
it is a three-lane shared file and A-27's objection to one lane dropping it in
still stands. **It is lane A's call, and it is now a smaller one**, because with
every worktree at 0 the "every lane's CRLF working copies convert on their next
checkout" side-effect that made it risky no longer has anything to convert.
That window will not stay open on its own.

**[A] 2026-09-06 — first recurrence since the close, and the gate caught it.**
Four tracked files came back CRLF in the lane-A worktree, and the boot test
refused to build on `check-eol` after 424s of gates. Recorded because the close
above rests on the gate being the mitigation, and this is the evidence it works:
the condition recurred within a day and was stopped before it reached a build.

The cause is the one §911 and the gate's own message name — **a tool rewriting a
tracked file through Python's default text mode**, which on Windows turns
every newline into a carriage-return/newline pair. Here it was an agent
using `pathlib.Path.write_text()` to patch files in place. The four affected
were exactly the four written that way;
files edited through other means in the same session stayed LF, and the two
written later with `newline=""` were also clean. So the discriminator is the
call, not the file type: `write_text(s)` corrupts, `write_text(s, newline="")`
and `write_bytes(...)` do not.

**Nothing was committed wrong.** The clean filter normalises on `git add`, so
every blob stayed LF and the repair produced a zero-byte diff — which is the
entry's own point restated: `git status` called the tree modified, `git diff`
showed nothing, and `git add` staged nothing.

Worth naming the trap for whoever hits it next, because it defeats the obvious
check: a shell `grep` for a carriage return does **not** reliably detect one
here. When the shell leaves the escape uninterpreted, grep receives an
*empty pattern*, which matches every line — so a CRLF file and an LF file
both report a count equal to
the file's line count, and the LF file looks corrupt. That false positive was
read here as "CRLF is repo-wide and pre-existing, therefore harmless", which
delayed the fix until the gate found it. Detect it with `git ls-files --eol`
(`w/crlf` is unambiguous) or a binary read, never with `grep`.
