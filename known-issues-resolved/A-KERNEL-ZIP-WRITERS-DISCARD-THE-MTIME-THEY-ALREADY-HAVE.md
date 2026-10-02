## `A-KERNEL-ZIP-WRITERS-DISCARD-THE-MTIME-THEY-ALREADY-HAVE` (lane A, 2026-08-27)

**Status: FIXED 2026-08-27 for `kshell`** (steps 1 and 2 below); the
`archive.rs` half is **split out** as its own entry,
`A-CREATEENTRY-HAS-NO-MTIME-SO-EVERY-ARCHIVE-FORMAT-LOSES-IT`, because it is a
different change to a different struct shared with the tar/cpio/ar writers.

`tzrules::dos_datetime_from_unix` landed in `37c04848e`; the `kshell` wiring and
its boot self-test rung 96 follow. Boot test PASS with rung 96 green, which is
what proves it: the rung recomputes the expected DOS pair from what
`Vfs::metadata` reports for the same file and demands the archive match, in the
recursive arm as well as the plain one.

**In short:** SlateOS can now record, for each file it puts into a ZIP archive,
when that file was last changed — the slot in the file format exists and the
writer fills it in. But the two places in the kernel that actually create
archives still passed "not recorded", so a `zip` command run on the shell
produced an archive whose Date column is empty, and it did not have to be.

**Where it lives.**

| Site | Has an mtime? | Passes |
|---|---|---|
| `kernel/src/kshell.rs` (`zip` command, ~line 131500) | can get one for the cost of a second stat | `dos_datetime: 0` |
| `kernel/src/fs/archive.rs` `create_zip` (~line 696) | no — `CreateEntry` has no time field at all | `dos_datetime: 0` |

The two are not the same problem. `kshell` already stats every input, so the
time is one call away. `archive.rs` genuinely has nothing to pass — `CreateEntry`
is `{ name, data, kind }` — so fixing it means widening that struct, which the
tar and cpio writers alongside it would also want to honour.

**Correction, 2026-08-27, found while fixing this.** The sentence originally
here — that `kshell` "is discarding a value it holds", because `Vfs::lstat(&abs)`
returns a `FileMeta` whose `modified_ns` the `Ok(meta)` arm ignores — was
**wrong**, and so was the title's "THEY ALREADY HAVE". `Vfs::lstat` returns a
`DirEntry` (`vfs.rs:3458`), which is `{ name, entry_type, size }` and carries no
timestamps at all; the only call that yields an mtime is `Vfs::metadata`
(`vfs.rs:2996`), which nothing on this path made. The defect is real and the fix
below is unchanged in substance, but it costs an extra stat per member rather
than being free. Recorded rather than silently edited because the mistake is
instructive: the entry was written from a grep for `lstat` plus an assumption
about what it returns, and a reader who trusted step 2 verbatim would have
written `meta.modified_ns` and hit `E0609`. It did — see the compile error in
the fixing session.

**Why it was not already fixed.** The missing piece was an *encoder*: something
that turns nanoseconds-since-the-Unix-epoch into the packed MS-DOS date/time
pair the format wants. That needs a calendar, and the decision recorded in
design-decisions.md §621 is that `ziparchive` must not own one — it is `no_std`
and linked into the kernel. The right home is `tzrules`, the shared
dependency-free calendar crate that already backs the taskbar clock,
`guitk::datetime` and lane C's ZIP *decoder*, and which the kernel already
depends on (`kernel/Cargo.toml:26`).

**What the proper fix looks like.**

1. Add `tzrules::dos_datetime_from_unix(secs: i64) -> u32` (and its inverse, if
   a second caller wants one), built on the existing `civil_from_days`. It must
   return `0` for anything before 1980-01-01 or after 2107-12-31, because those
   are unrepresentable in the DOS pair and `0` is the "not recorded" encoding —
   clamping to the minimum would re-create the exact fabrication that
   `A-ZIPARCHIVE-CREATE-STAMPED-EVERY-MEMBER-1980-01-01` was about. Seconds are
   stored halved, so an odd second must round consistently; round *down*, so a
   recorded time is never later than the real one.
   **Done** — `37c04848e`, with seven tests including both range edges, a leap
   day, odd-second rounding, and a walk over every representable day.
2. In `kshell.rs`, carry the mtime alongside the path in `input_files` and
   convert at the `ZipWriteEntry` construction. Note the recursive arm:
   `zip_collect_files` does not currently stat what it collects, so it needs the
   same treatment or it will silently keep passing `0` for everything under
   `-r`.

   **Done**, with three deviations from the plan as written, all forced by the
   correction above:

   - The `(PathBuf, PathBuf)` tuple became a `ZipInput { name, source,
     dos_datetime }` struct. A third element would have made every call site
     read `.0`/`.1`/`.2` over two paths that are not interchangeable.
   - Both arms call `Vfs::metadata` explicitly, since `lstat` has no mtime to
     carry. In `cmd_zip` that is a second call next to the `lstat`, which is
     still needed to classify *without* following a symlink — `metadata` does
     follow, and recursing into a symlinked directory could leave the tree the
     user named or loop.
   - That `metadata` follows symlinks is right rather than merely tolerable
     here: the member's bytes come from `read_file`, which follows the link
     too, so the recorded time describes the same bytes that get stored.

   A stat that fails leaves the member timeless rather than dropping it —
   the same outcome as a filesystem that does not track mtimes.
3. Decide separately whether `CreateEntry` grows a `modified_ns`; that is a
   wider change touching tar/cpio/ar and is worth its own task. **Split out**
   as `A-CREATEENTRY-HAS-NO-MTIME-SO-EVERY-ARCHIVE-FORMAT-LOSES-IT`.

**Note on timezone.** DOS timestamps are *local* time with no zone recorded.
The kernel has no user timezone, so a kernel-side encoder will be writing UTC
into a slot that readers interpret as local. That is a real, known inaccuracy
of the format rather than of this fix, and every OS writing ZIPs has it; it is
worth a comment at the conversion site so the next reader does not "fix" it.
Done — `zip_dos_time`'s doc comment says so, and says why shifting by a guessed
offset would turn a known-imprecise time into a confidently wrong one.

**How it was found.** Not by looking for it. While adding
`ZipWriteEntry::dos_datetime` for lane C, the mechanical step of putting
`dos_datetime: 0` at each construction site meant reading each one — and
`kshell.rs` turned out to be stat-ing every input already.
