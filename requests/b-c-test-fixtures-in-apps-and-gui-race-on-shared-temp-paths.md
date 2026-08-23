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
