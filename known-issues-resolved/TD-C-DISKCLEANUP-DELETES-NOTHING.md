## TD-C-DISKCLEANUP-DELETES-NOTHING — ✅ **FIXED** 2026-08-26

> **Resolved the same day it was written.** Both halves are real: the scanner
> walks the filesystem and measures what it finds, and "Clean" unlinks it. The
> resolution — including the near-miss that shaped the design, which is the
> part worth reading — is the entry
> `C-DISKCLEANUP-A-REAL-DELETER-NEEDS-A-CONFINEMENT-INVARIANT-NOT-A-CONVENTION`
> at the end of this file. The description below is kept as written, because it
> is the accurate record of what the program was.

**In short:** `apps/diskcleanup` is a disk cleaner that never cleans anything.
Pressing "Clean" walks the list of files it plans to remove, counts them, and
returns — it does not call the filesystem at all. Worse, the *scanner* never
asks how big any of those files are, so every category reports "0 B" and the
whole size-driven half of the window is dead: no "View" link ever appears, and
the "Clean" button can never light up. As of 2026-08-26 the program at least
says so out loud — the results screen is titled "Cleanup Preview" and carries a
banner reading "Preview only — no files were deleted" — but the two stubs
underneath it are still stubs.

**Where:** `apps/diskcleanup/src/main.rs`.

### The two halves

**(a) `CleanupExecutor::execute` is byte-identical to `CleanupExecutor::dry_run`.**
Both iterate `plan.items`, `saturating_add` a count and a byte total, and
return. Neither names `std::fs`, the VFS, or any syscall. There is a comment at
the exact line where the deletion belongs:

```rust
// A real implementation would call `std::fs::remove_file` or the
// VFS equivalent here, and push a `(path, message)` onto `errors`
// when it refused.
```

`CleanupResult` already carries an `errors: Vec<(String, String)>` field for
per-file refusals, and it is always empty for the same reason.

**(b) `CleanupScanner`'s per-category methods record directories, not files, and
never measure them.** `scan_temp_files`, `scan_logs`, `scan_package_cache`,
`scan_thumbnails`, `scan_crash_dumps` and the rest each push
`CleanupItem::new(path, category)` — and not one of them chains `.with_size()`.
`CleanupItem::estimated_size_bytes` therefore stays at its `0` default, so
`run_scan`'s per-category totals are all zero, so:

- every row reads `0 B`;
- `can_clean()` is false whatever the user ticks, so "Clean" stays greyed;
- the per-row "View" link, which only appears for a category with bytes in it,
  is unreachable in the shipped program.

This is why the event-wiring tests build their state through a `scanned()`
helper that injects a size rather than going through `run_scan`: a test that
used the real scanner could never reach the size-gated half of the UI, which is
most of it.

### How the program handles it today

Not by a comment. `CleanupExecutor::deletes_for_real()` is a `const fn`
returning `false`, and `CleanupResult` carries a `simulated: bool` set from it.
Every piece of user-facing wording reads that bit:

| `deletes_for_real()` | Results title | Banner | Row labels |
|---|---|---|---|
| `false` (today) | "Cleanup Preview" | yellow, bold, *above* the numbers | "Files that **would be** deleted" |
| `true` (later) | "Cleanup Complete" | none | "Files deleted" |

The banner sits above the totals rather than below them because a disclaimer
under a bold green "Space freed: 1.4 GiB" is a disclaimer nobody reads. The bit
is stored on the `CleanupResult` rather than consulted by the renderer, so the
history log records it too: a row written today cannot later be mistaken for a
real cleanup once the deleter lands.

When the deleter is real, `deletes_for_real()` becomes `true` in one place and
all of the wording follows. That is the whole point of it being a function.

### Why the deleter was not written in the same commit

`DEFAULT_SCAN_ROOTS` is `["/"]` — `CleanupScanner::scan` treats each base as a
*prefix*, joining `"tmp"`, `"var/log"` and the rest onto it, so a base of `"/"`
yields exactly `/tmp` and `/var/log`. The unit tests run on the **development
host**, not under SlateOS. A real recursive deleter driven by the existing test
suite would therefore delete files on the operator's `C:`/`D:`.

So the sequencing was deliberate: wire the window and make the UI honest first
(one commit, no filesystem writes anywhere in it), and make the back end real in
a follow-up whose tests point **only** at a temporary directory they created
themselves.

### The proper fix

1. **Measure during the scan.** Each `scan_*` method stats the files it finds
   and chains `.with_size(bytes)`. For a directory that means summing its
   entries, which also means the scan must be bounded — a cleaner that hangs on
   a pathological tree is worse than one that reports zero.
2. **Delete for real**, one file at a time, pushing `(path, message)` onto
   `result.errors` for every refusal rather than aborting the run. A cleanup
   that stops at the first permission error leaves the disk in a state the user
   cannot reason about.
3. **Flip `deletes_for_real()` to `true`** in the same commit as (2), never
   before it. The wording follows automatically.
4. **Test against a temp directory only.** The deletion tests must construct
   their own tree, pass its root as the scan base, and assert both that the
   files are gone and that a file outside the base survived. No test may ever
   pass `"/"` to a code path that can delete.
5. Keep the `simulated` field afterwards. It stops being always-`true`, not
   meaningless: a future "preview" button would set it, and the history log
   needs to distinguish the two kinds of row forever.

### The lesson

**A stub that reports success in the same words as the real thing is not an
unfinished feature, it is a lie with a schedule.** The program told the user
"Space freed: 1.4 GiB" while touching nothing; the user finds out when the disk
is still full, which is the worst possible moment and the hardest possible thing
to attribute. The fix that costs almost nothing is to make the *wording* a
function of the *behaviour* — one boolean, read by every string — so that the
day the behaviour changes, no one has to remember to change the words.
