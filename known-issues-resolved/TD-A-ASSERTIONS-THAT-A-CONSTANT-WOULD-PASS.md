## TD-A-ASSERTIONS-THAT-A-CONSTANT-WOULD-PASS — the weak-proxy sweep (lane A, 2026-08-23)

**Status:** all four known instances FIXED. Shape 3 (a test that skips itself
and reports success) is now **CLOSED as a class** — swept tree-wide and held
shut by a build gate; see the closing note at the end of this entry. Shapes 1,
2 and 4 remain open as a class: their instances are fixed, but nothing stops a
new one, because no mechanical check for them exists yet.

A self-test assertion is only worth its line if there is a plausible way for
the code under test to be wrong and still fail it. Four shapes recur here
and none of them clear that bar:

1. **`assert!(timestamp > 0)`.** Passes for a non-zero constant, for a value
   stamped once at construction and never updated again, and for a reading
   taken from a different clock. It fails only if the field is left at
   exactly zero — which is the one wrong value a `hpet::elapsed_ns()` reading
   is least likely to be. The fix is to bracket: read the clock either side
   of the operation and require the stored value to fall inside the window.
   - `kernel/src/fs/certmgr.rs` — `last_renewal_ns` (self-test 5). Fixed in
     `33c40109a`; also now checks `not_before_ns`, stamped from the same
     reading, and that `not_after_ns` follows it.
   - `kernel/src/fs/sysrq.rs` — `last_triggered_ns` (self-test 3). Fixed in
     the same commit.

2. **A field written and never read.** `svcstart`'s `crash_history` collected
   a ten-entry ring of crash timestamps on every crash report and no code
   path — not `CrashInfo`, not `/proc/svcstart`, not `svcstart crashes`, not
   a test — ever read it back. Write-only state cannot be wrong, so nothing
   can notice when it stops being written. Fixed in `d4dff8c2a` by carrying
   it on `CrashInfo`, rendering it, and asserting it in self-tests 4 and 5.

3. **A test that skips itself and reports success.** `kernel/src/fs/index.rs`
   printed `"[index]   (skipped VFS tests: /tmp not mounted)"` mid-run and
   then `"[index] Self-test passed"` at the end. The last line is the one a
   reader believes, so the suite went green having tested nothing. It was
   dormant rather than harmless — `/tmp` *is* mounted today (see
   `build/serial-batch32.txt` line 2190, `/tmp -> memfs`), so the branch was
   not taken.

   Fixed 2026-08-23. Two changes, and the second is the larger one:

   - **The skip is now carried to the summary.** `self_test` collects the
     names of sections it could not run and, if that list is non-empty,
     prints `"Self-test passed with N section(s) SKIPPED"` followed by one
     `SKIP:` line each — the shape `report_pathz_skips` in
     `scripts/boot-test.sh` already uses. Test 5's zero-entry rebuild joined
     the list for the same reason: its bookkeeping assertions ran, but the
     thing the test is *for* — that a walk finds files and files them — did
     not happen, and "rebuild OK (0 entries)" read as a pass.
   - **The precondition is now asked, not inferred.** The gate was
     `if Vfs::write_file(..).is_ok()`, which classifies *every* failure as
     "/tmp is not mounted": a permission gate wrongly denying the write, a
     full filesystem, a memfs bug. It would have skipped the very test that
     caught them. It now consults the mount table — a fact — and once /tmp is
     known mounted, a failing write is a reported failure with the error in
     it, not a skip.

   The general lesson for this class: a self-skipping test has *two* defects,
   and fixing only the reporting leaves the worse one. The condition that
   decides to skip must be a statement about the environment, never a
   swallowed error from the code under test.

4. **A sentinel that two different states share, guarded by an `if` with no
   `else`.** `svcstart`'s `boot_start_ns`/`boot_end_ns` were bare `u64`s
   initialised to `0` and compared only behind
   `if st.boot_end_ns > st.boot_start_ns` at both display sites
   (`svcstart.rs`, `kshell.rs:45849`). "Never booted", "boot still running"
   and "boot died before its end stamp" all rendered identically: as the
   absence of a line. Nothing asserted either field, so they could have
   stopped being written entirely and no view and no test would have said so.

   Fixed 2026-08-23. Both fields became `Option<u64>` — the encoding
   `StartNode::started_at_ns` in the same file already used, and for the same
   stated reason ("never started" and "started at uptime 0" are different
   answers). A `BootDuration` enum (`NotStarted` / `InProgress` /
   `Complete`) classifies the pair, `StartupStats::boot_duration()` returns
   it, and `BootDuration::label()` renders *every* variant, so
   `/proc/svcstart` and `svcstart stats` now always carry a `Boot time:`
   line. `boot_services` also clears the end stamp on entry, so a second run
   that dies partway can no longer pair its fresh start against the previous
   run's end.

   Self-test 11 asserts all three states, and asserts the completed one by
   **bracketing**: it reads the clock either side of `boot_services()` and
   requires both stamps to land inside that window. `stamp > 0` would have
   passed for a constant, for a value written once at init, and for a reading
   from another clock; only the window shows the stamps came from that call.

**How to find more:** grep the self-tests for `> 0)` and for `is_some()` on a
field whose *value* is the thing under test. The pattern to look for is an
assertion whose truth follows from the code compiling rather than from the code
working. (Shape 3 no longer needs a grep — see below.)

---

**Closing note, 2026-08-23 — shape 3 is swept and gated.**

Shape 3 was the only one of the four with a mechanically checkable form, so it
was swept to zero rather than left to grep. `scripts/check-selftest-skips.py`
implements the two rules this entry states — *the precondition must be a fact
the test looked up, and the skip must reach the line a reader believes* — and
`scripts/boot-test.sh` now refuses to build on a finding. The final count is
**802 files, 0 findings**.

The sweep touched 23 files across five commits (`7c28d2395`, `d5b534e1a`,
`ca7e96f59`, `4a38f346c`, `17a043a8a`/`256e11b30`/`8e873946f`/`9a8b1a817`).
Three shared helpers now carry the pattern so it is not re-derived per call
site, all in `kernel/src/fs/selftest.rs`: `Skips` (the ledger, whose `suffix()`
appends ` — N section(s) SKIPPED` to the summary line), `classify()` (the
`Ready`/`Unsupported`/`Failed` split — only `NotSupported`,
`ReadOnlyFilesystem` and `NoSuchDevice` mean "this system cannot"), and
`is_mounted()`/`is_mounted_rw()` (the commonest looked-up fact). The rationale
is written up in `design-decisions.md` §270 (*a self-test may skip, but only on
a fact it looked up*).

Three sites were more than reporting fixes, because removing the skip exposed
what the skip had been hiding: `fs/mime` now fails when `detect()` errors
rather than warning, `fs/zip` fails when deflate does not compress rather than
noting it, and `kernel/src/watchpoint.rs` deleted its skip outright by giving
the test its own 8-byte-aligned `AtomicU64` — turning a precondition the test
had to check into one the type system guarantees. `syscall/linux`'s
`self_test_fallocate_range` had the worst instance found: a second staging step
that returned `Ok(())` on failure and silently abandoned the remaining
sub-tests without printing anything at all.

**Addendum, 2026-08-23 — the checker was too narrow twice over, and both
narrowings hid real instances.**

It originally matched only `.is_ok()`/`.is_err()`, and it only examined
functions whose *name* looked like a self-test. Fixing each of those turned up
sites the first sweep had declared clean:

| Narrowing | What it hid | Found |
|---|---|---|
| Only `.is_ok()`/`.is_err()` | `if let Ok(cg_id) = cgroup::create(..)` and `match alloc_frame() { .. Err(_) => SKIP }` | 142 sites, incl. all 6 cgroup sections of `mm/frame.rs` |
| Only name-matched functions | a suite split across helpers — `io_ring`'s `test_fh_read_write` / `test_fh_positioned_io_leaves_the_cursor_alone`, both called from `self_test`, both skipping on a failed `/tmp` write | 2 sites, seen first in a boot log, not by the checker |

The checker now also matches `if let Ok(..)`/`if let Err(..)` and a `match`
arm whose pattern is a catch-all `Err(_)`/`Err(e)`; an arm naming *specific*
errors stays exempt, since that is the approved form. And it takes the
transitive closure over same-file calls out of every name-matched self-test,
so a helper is checked exactly when a self-test can reach it — which also
keeps `#[cfg(test)]` unit tests out, as nothing calls them. (That closure was
first written pairwise and made the gate take 200 s, long enough that a boot
test looked hung; extracting each body's callee names in one scan brought it
to 22 s.)

**Remaining blind spot: the skip decided from an integer errno.** Both rules
key on a `Result`, so a syscall-level test that reads a negative return value
is invisible:

    // syscall/dispatch.rs, before the fix
    let write_result = dispatch(SYS_FS_WRITE_FILE, &write_args);
    if write_result.value < 0 {
        serial_println!("[syscall]   Dispatch FS roundtrip: SKIPPED (no FS, err={})", ..);
        return Ok(());
    }

That one is fixed (it asks `is_mounted_rw("/")` and now fails when a mounted
`/` rejects the write), but a new one would not be caught. Detecting it
textually is not obviously possible: `if x.value < 0` is ordinary code, and
the thing that makes it a defect is that the value came from the code under
test — which needs dataflow, not a pattern. The practical mitigation is that
this spelling only arises where a test drives the syscall dispatch table
directly, which is a handful of files; they were reviewed by hand.
