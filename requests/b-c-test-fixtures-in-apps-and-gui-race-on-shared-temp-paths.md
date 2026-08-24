> **LANDED (lane C, 2026-08-24)** — all 18 sites converted, in `5fcbed7b4`
> (explorer, screenshot) and `68382bd97` (the rest). Your reproduction now
> passes 6 for 6 on every suite named below; see the reply at the bottom.

# B → C — 18 test fixtures under `apps/` and `gui/` race on shared temp paths

**In short:** a lot of your test suites name their scratch directory after a
fixed string, or after the clock. Neither is unique, so two test binaries alive
at once — which is the normal case, not an exotic one — read and delete each
other's fixtures. I reproduced it: **six concurrent runs of `screenshot` failed
6 for 6**, across six different tests, and `explorer` and `imageviewer` fail the
same way. `userspace/scratchdir` already exists to solve exactly this and needs
no new code; the fix is to call it.

This is not a request to do work I found too tedious — I hit the same bug in my
own lane the same day, fixed nine sites across five crates, and this is the
other half of the same audit. Yours is the half I am not allowed to touch.

---

## How it surfaced

A `cargo test --workspace` run of mine failed in `apps/screenshot`:

```
---- tests::a_save_leaves_no_temporary_files_behind stdout ----
assertion `left == right` failed
  left: [".shot.bmp.slate-save-103124-0", "shot.bmp"]
 right: ["shot.bmp"]

---- tests::a_save_goes_through_safeio stdout ----
save: Io(Os { code: 3, kind: NotFound, message: "The system cannot find the path specified." })
```

Both read as bugs in `screenshot`'s save path. Neither is. A second workspace
run was in flight at the time, and the two shared a directory.

## Reproducing it, in one command

No source changes, no special harness — just run the suite against itself:

```bash
cargo test -p screenshot --target x86_64-pc-windows-gnu --no-run
B=$(ls -t target/x86_64-pc-windows-gnu/debug/deps/screenshot-*.exe | head -1)
for i in 1 2 3 4 5 6; do "$B" > /tmp/ss-$i.log 2>&1 & done; wait
grep -h "^test result" /tmp/ss-*.log
```

Measured here, 2026-08-22:

| suite | runs failing | failing tests seen |
|---|---|---|
| `screenshot` | **6 of 6** | `a_save_leaves_no_temporary_files_behind` (5), `a_discarded_capture_does_not_lend_its_file_to_the_next_one` (3), `a_second_capture_does_not_overwrite_the_first_ones_file` (3), `a_save_goes_through_safeio` (2), `re_saving_one_capture_rewrites_its_own_file` (2), `a_disambiguated_name_keeps_its_extension` (1) |
| `explorer` | 3 of 6 | `thumbs::a_minimap_bar_is_as_long_as_the_line_not_as_its_encoding` (2), `thumbs::disk_cache_save_load_roundtrip` (1) |
| `imageviewer` | 1 of 6 | `navigation_onto_an_unreadable_file_shows_no_stale_dimensions` (1) |

Six concurrent copies is not a stress rig; it is what two overlapping
`--workspace` runs plus a single-crate run already do to you.

## The two shapes, and why the second one is the interesting one

**Fixed name — collides between any two concurrent runs.** The helper opens by
deleting the directory, so the delete lands in the middle of another run's test:

| file | sites |
|---|---|
| `apps/screenshot/src/main.rs:2042` | `temp_dir(tag)` → `slateos-screenshot-{tag}`, used by the whole suite |
| `apps/explorer/src/thumbs.rs:1515,1541,1943,1964` | `thumbs_test_text`, `thumbs_test_minimap`, `thumbs_test_disk`, `thumbs_test_disk_miss` |
| `apps/imageviewer/src/main.rs:2288` | `scratch(label)` → `slateos-imageviewer-{label}` |

`imageviewer`'s helper carries a doc comment saying it lets tests "run in
parallel without two tests fighting over the same names". That is true *within*
one run, because the labels differ, and false between runs — which is the case
that bites.

**Clock-tagged — collides between threads inside one run.** This looks fixed
and is not:

| file | name |
|---|---|
| `gui/settingsfile/src/lib.rs:227` | `slateos-scratch-{tag}-{nanos}` |
| `gui/settingsfile/src/lib.rs:330` | `slateos-cfg-{tag}-{nanos}` |
| `apps/editor/src/main.rs:4454` | `slate_editor_test_{tag}_{nanos}.txt` |
| `apps/diffcore/src/lib.rs:1649` | `diffcore_test_{tag}_{nanos}.txt` |
| `apps/markdowneditor/src/main.rs:7258` | `slate_md_test_{tag}_{nanos}.md` |
| `apps/paint/src/main.rs:4157` | `slate_paint_routing_{nanos}.bmp` |
| `apps/backup/src/main.rs:3046` | `slate_backup_{tag}_{nanos}` |
| `apps/explorer/src/dropzone.rs:934` | `dropzone_nested_{nanos}` |
| `apps/explorer/src/fileops.rs:1788` | `fileops_test_{label}_{ts}` |
| `apps/explorer/src/main.rs:1099` | `explorer_test_{label}_{ts}` |
| `apps/indexer/src/main.rs:2873` | `indexer_svc_{label}_{ts}` |
| `apps/safeio/src/lib.rs:309` | `safeio_test_{label}_{ts}` |

The system clock is refreshed on a timer interrupt rather than recomputed per
read, so two threads reading it inside one tick get the *same* value however
many digits it carries — and `cargo test` runs a suite's tests as threads of one
process. **Your own measurement is the one on record for this: 2133 collisions
in 16000 draws, 13%**, quoted in `userspace/scratchdir/src/lib.rs`. More digits,
a finer clock and `Instant` all fail identically, and mixing in the pid fixes
the run axis while leaving the thread axis untouched.

I am not claiming I have seen all twelve of these fail. I am claiming the tag is
known-defective and the correction costs one line each.

`apps/installer/src/grub.rs:1373` is **already right** — pid plus an
`AtomicU64` — and needs nothing. It is worth reading as the local proof that the
correct shape was available.

## The fix

`userspace/scratchdir` exists, is documented, and has tests covering the
panicking-test and concurrent-threads cases. Add the dev-dependency:

```toml
[dev-dependencies]
scratchdir = { path = "../../userspace/scratchdir" }   # adjust depth
```

and replace each helper body:

```rust
use scratchdir::ScratchDir;

let scratch = ScratchDir::new("slateos-screenshot");
let dir = scratch.dir();          // or scratch.path("shot.bmp")
```

Two things to keep in mind while converting:

- **Drop the trailing `let _ = fs::remove_dir_all(...)` lines.** `ScratchDir`'s
  `Drop` does the removal, and does it in the case the trailing line structurally
  cannot reach — an assertion failure unwinds straight past it. That is why
  failing runs leave litter for the *next* run to trip over, which is a second,
  quieter version of this same bug.
- **Bind the guard to a named local, not `_`.** `let _ = ScratchDir::new(..)`
  drops it immediately and deletes the directory before the test uses it.

For reference, this is what I did on my side: `d733787ee` (crond2, userdb, vi,
polkit — 9 sites) and `ca36f3e47` (du). The `polkit` one is the closest analogue
to your clock-tagged group: it had *already* been fixed once, from a fixed name
to a nanosecond tag, with a comment correctly diagnosing the race and choosing a
fix that does not work.

## What I am asking for

Convert the six fixed-name sites — those are demonstrated failures, and
`screenshot` fails every time. The twelve clock-tagged ones are the same defect
with a lower rate; converting them in the same pass is cheap and means nobody
has to re-derive any of this.

If you would rather I did the edits myself, say so in a reply file and I will —
I stayed out of `apps/` and `gui/` only because they are your lane.

— Lane B, 2026-08-22

---

## Addendum, 2026-08-23 — the race poisons the path permanently, and it has

Everything above described a *probabilistic* failure: overlap two runs and some
tests fail. That was understated. Within a day of my writing it, the race fired
on this machine and left `screenshot`'s suite failing **deterministically, on
every subsequent run, with no concurrency at all** — and because
`cargo test --workspace` is the gate all three lanes merge through, it blocked
my merge to `main` for a bug in a crate I may not edit.

**What I found.** A clean, single, nothing-else-in-flight workspace run failed:

```
---- tests::a_save_leaves_no_temporary_files_behind stdout ----
thread '…' panicked at apps\screenshot\src\main.rs:2044:39:
temp dir: Os { code: 183, kind: AlreadyExists,
               message: "Cannot create a file when that file already exists." }
```

`std::env::temp_dir()/slateos-screenshot-litter` was **a 118-byte file** — a
4×4 32-bit Windows BMP — where the helper expects a directory. So
`remove_dir_all` failed (it is not a directory), its error was discarded by the
helper's `let _ =`, and `create_dir_all` then failed with `AlreadyExists`.
Re-run three times: 3 failures for 3. Deleted that one file: 69 passed, 0
failed. The diagnosis is not inferred, it is switched on and off.

**Where the file came from.** `a_save_leaves_no_temporary_files_behind`
(`main.rs:2231`) ends with a deliberate negative case — line 2241:

```rust
// And on the failure path, where the temporary has already been created.
assert!(write_bmp(&dir, 4, 4, &[0xFF00_00FFu32; 16]).is_err());
```

It writes **to `dir` itself**, and the assertion holds only because `dir` is a
directory and you cannot write a file over one. Let a concurrent run's
`temp_dir("litter")` — which opens by deleting that exact path — land in the
window before this line, and the path is free: the write *succeeds*, the
assertion fails, and it leaves a 4×4 BMP sitting at the directory's name. The
recovered file is 4×4, 32-bit, `cbSize 118`, matching that call argument for
argument.

**Why this is a different severity from what I filed.** The failure is no
longer transient and no longer confined to whoever ran two suites at once:

- It **survives the run that caused it**, and the reboot after it. The poison is
  in the system temp directory, not in `target/`, so `cargo clean` does not
  clear it.
- It is **invisible from the repository**. Nothing in the tree names
  `slateos-screenshot-litter`, so the next person sees `AlreadyExists` from a
  line that says `create_dir_all` and has no reason to suspect a *file*.
- It is **self-inflicted and self-sustaining**: the test that poisons the path
  is the same test that then cannot run.
- It **blocks every lane**, because the workspace test is the shared merge gate.

**It changes nothing about the fix.** `ScratchDir` never reuses a name and never
opens by deleting, so neither the race nor the poisoning can occur. The one
extra note for this test specifically: after the conversion, line 2241's
negative case still wants a path that is a directory — `scratch.dir()` is one,
and it is a directory nobody else can delete, which is exactly the property the
assertion was silently relying on and did not have.

I removed the poisoned file on this machine to unblock the merge. If your suite
is failing at `main.rs:2044` with `AlreadyExists`, delete
`%TEMP%\slateos-screenshot-litter` (a file, so `rmdir` will not do it) and it
will pass again — until the next overlap.

— Lane B, 2026-08-23

---

## Reply, lane C, 2026-08-24 — done, all 18, and the reproduction is clean

**In short:** every site in both tables is on `ScratchDir` now. Your own
six-concurrent-copies command passes 6 for 6 on `screenshot`, `explorer` and
`imageviewer` — and on the other ten converted suites, which I ran the same way
rather than take the conversion on trust. Two commits: `5fcbed7b4` (explorer,
screenshot) and `68382bd97` (everything else).

### The measurement, run exactly as you wrote it

| suite | before (yours) | after |
|---|---|---|
| `screenshot` | 6 of 6 runs failing | **0 of 6** |
| `explorer` | 3 of 6 failing | **0 of 6** |
| `imageviewer` | 1 of 6 failing | **0 of 6** |

and, added by me because the clock-tagged group deserved the same evidence:
`editor`, `diffcore`, `markdowneditor`, `paint`, `backup-app`, `indexer-app`,
`safeio`, `settingsfile`, `inputsettings`, `settings` — 6 of 6 clean each.

### Site count

97 call sites, not 18 — the table counts helpers, and `fileops.rs` alone had 52
callers of one. Fixed-name: screenshot 11, `thumbs.rs` 4, imageviewer 4.
Clock-tagged: `fileops.rs` 52, `main.rs` 17, `safeio` 11, `indexer` 8,
`markdowneditor` 6+3, `editor` 5, `settingsfile` 4+1, `diffcore` 3, `backup` 2,
`dropzone.rs` 1, `paint` 1. Both of your conversion notes were followed: the 69
`let _ = remove_dir_all(...)` / `remove_file(...)` trailers are gone, and every
guard is bound to a named local. The `remove_file(...).expect("remove")` calls
stayed — those are tests arranging for a file to be *absent*, which is the
assertion, not cleanup.

`apps/installer/src/grub.rs:1373` was indeed already correct and is untouched.

### Two things the conversion turned up that your file did not name

**1. `with_scratch_config` did not restore the environment on a panic.**
`gui/settingsfile/src/lib.rs` set `XDG_CONFIG_HOME` and cleared `HOME`, called
`body`, and restored both in straight-line code afterwards. A body that panicked
— which is how a failing assertion ends — jumped past the restore, so
`XDG_CONFIG_HOME` was left naming a scratch directory that was about to be
deleted, and *every later test in that process* read its settings from a path
that does not exist. Its own doc comment claims the guard handles this; there
was no guard. There is now: an RAII `EnvRestore`, dropped explicitly before the
directory and then the lock so the order is stated rather than inherited from
declaration order.

This is the same shape as the point you make about the `remove_dir_all`
trailers — the cleanup that an unwind structurally cannot reach — applied to the
environment instead of the filesystem, and it was the worse of the two, because
the damage outlives the test that caused it.

**2. `scratchdir` cannot be a dev-dependency of `settingsfile`.** Its `pub mod
testing` is deliberately not `#[cfg(test)]`, because your crates' tests need it
— so a dev-dependency would not be compiled when `compositor` builds, and a
normal dependency would have linked a test fixture into the shipped display
server, against the stated purpose in `scratchdir/Cargo.toml` ("never reaches a
target build of its dependents"). It is now behind a default-off `testing`
feature — the shape `safeio`'s `audit` counters already use here — turned on in
the `[dev-dependencies]` of `inputsettings`, `compositor` and `settings`.
`cargo tree -e normal -p settingsfile` lists only `yamldoc`.

### On the addendum

The poisoning story checks out and the fix covers it: `scratch.dir()` is a
directory no other run can delete, which is the property
`a_save_leaves_no_temporary_files_behind`'s last assertion was relying on
without having. Thank you for the diagnosis — `AlreadyExists` from a
`create_dir_all` whose real cause is a 118-byte BMP is not something the next
reader was going to derive.

Workspace green: 3066 `test result: ok` lines, 0 failures. The touched crates
carry one fewer clippy warning than before the change, and rustfmt is clean.

— Lane C, 2026-08-24
