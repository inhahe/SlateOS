## 650. A precondition that is a constant is not a precondition: the six self-tests that skipped on it move past the mount and assert instead

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous), lane A

**In short:** Six of the kernel's boot self-tests began by asking "is the main
disk mounted and writable yet?", found that it was not, printed a polite
"skipping this test" line, and returned. That question always had the same
answer, because those tests run at boot step 11 and step 19 and the disk is
not mounted until step 20 — so those six tests had never actually run, not
once, on any boot the project has ever taken. Nothing looked wrong: the boot
log said the suites passed. The decision was what to do about it. The six
tests now run *after* the mount, and if the disk turns out not to be writable
at that point they fail the boot rather than excusing themselves.

### What went wrong

The skips were **honest**. design-decisions.md §270 says a self-test may skip,
but only on a fact it *looked up* — not on an inference from a failed
operation, because inferring lets the very bug the test exists to catch switch
the test off. All six obeyed that: each called
`crate::fs::selftest::is_mounted_rw("/")`, a mount-table lookup, and each
recorded the reason into a `Skips` summary so the closing PASSED line named
them. `scripts/check-selftest-skips.py` enforces exactly that honesty, and it
passed all six.

What no rule covered is a skip that is honest **and unconditional in
practice** — a predicate that is a *constant* at the point it is evaluated.
`is_mounted_rw("/")` is a genuine runtime question in general; it simply is
not one at boot step 11. The six cases were:

| Subsystem | Case |
|---|---|
| `syscall` (dispatch.rs) | Dispatch FS roundtrip; Native openat2 |
| `syscall/linux` (linux.rs) | mkdir/rmdir/unlink native-VFS round-trip; rename native-VFS round-trip |
| `proc/spawn` (spawn.rs) | Spawn with fd_map; take_initial_fds one-shot |

This was not found by a gate. It was found by reading a boot log while looking
for something else, and noticing that the same four words appeared six times.

### The decision, in three parts

**(1) Move, don't relax.** The alternative that suggests itself is to mount
earlier, or to give the pre-mount suites a scratch filesystem. Both change
boot ordering to suit a test. The fix pattern already existed in-tree three
times over — `ipc::io_ring::self_test_fh`,
`syscall::linux::self_test_rename_noreplace`, `syscall::linux::self_test_file_mmap`
are all second entry points called from `main.rs` after step 20f for precisely
this reason — so the six join them: `syscall::dispatch::self_test_fs`,
`proc::spawn::self_test_fs`, `syscall::linux::self_test_fs`.

**(2) The guard becomes an assertion, and does not remain as a skip.** Each
case still calls `is_mounted_rw("/")`. A `false` now prints FAIL and returns
`InternalError`. The reasoning: *before* the mount, "root is not writable yet"
is a fact about the boot stage; *after* it, the same words describe a defect.
Answering a defect with a SKIP is the mechanism that gave these six their
entire unrun lifetime, so leaving a skip behind — reporting a condition that
can no longer legitimately occur — would reproduce the bug in the fix.

**(3) FATAL, not WARNING.** `main.rs`'s post-mount block is mixed: some calls
there warn and continue. These three halt the boot. The other halves of the
same three suites already run at steps 11 and 19 and halt on failure; a case
does not become less important for having moved down the file. Demoting them
would take the six from *never running* to *running and not mattering*, which
is the same outcome reached by a longer route.

### Two things the move exposed

Neither was the reported bug; both are the same defect at a different scale,
and both are why the entry is worth writing down.

- **The `Skips` recorders had to be deleted, not left idle.** Once no case in
  `dispatch::self_test`, `spawn::self_test` or `linux::self_test` could skip,
  each recorder could only ever report zero. Keeping it is not neutral: a
  reader who sees `Skips::new()` at the top of a suite reasonably infers that
  the suite has cases that may not run. Machinery that can no longer record
  anything is a false signal, so it goes. (`Skips` itself stays — some twenty
  other suites still use it legitimately.)
- **Two swallowed-error sub-gates, one level down, with no log line at all.**
  Inside the two `syscall/linux` bodies,
  `if crate::fs::Vfs::write_file(..).is_ok() { .. }` wrapped three assertions
  in the mkdir case and the entire `RENAME_NOREPLACE`/EEXIST leg in the rename
  case. A failing write dropped those assertions and the section still printed
  `OK`. That is the outer bug exactly — a failure deciding whether to test —
  except that the outer one at least printed `SKIP`. Both are now hard
  failures. This is also a §270 violation in its own right, and it survived
  because §270's checker looks for skips, and these did not print one.

### Alternatives considered

| Option | Why not |
|---|---|
| Leave the skips; they are honest | They are, and that is the problem: honesty is not the property that was missing. A skip that has never once *not* fired is a deletion with a log line. |
| Mount the root earlier so step 11 passes | Reorders boot to suit a test, and would silently change what every other step-11..20 case sees. |
| Give the pre-mount suites a scratch memfs | The cases test the *native* VFS round-trip specifically; a memfs would test a different thing under the same name. |
| Keep a post-mount skip "just in case" | Reproduces the bug: it is a predicate that should now be a constant in the other direction, and if it ever is not, that is exactly the news worth halting on. |
| WARNING instead of FATAL in `main.rs` | Moves the six from never running to running and not mattering. |
| Fix all six, and also write the class gate now | The gate is real work (see below) and gating it on the same commit would keep six known-broken tests unrun for longer. Split deliberately; tracked in `known-issues.md`. |

### What is *not* done

The **class** is still open. Six instances were found by eye, and eye-finding
does not scale to the next one. The intended gate is dynamic rather than
static: `bench/boot-history.jsonl` already records a row per boot, so extend it
to carry the *names* of skips that fired and fail when a named skip has fired
on 100% of the last N ≥ 10 recorded boots. A purely static check was considered
and rejected — deciding "is this function reachable only from a pre-mount call
site?" needs cross-module call-graph resolution over a 100k-line file, which is
exactly the kind of gate that becomes unreliable and then gets ignored. Tracked
in `known-issues.md`
`A-SIX-SELF-TESTS-SKIP-ON-EVERY-BOOT-AND-HAVE-NEVER-ONCE-RUN`.

### How to reverse it

Move the six bodies back into the pre-mount `self_test()` functions, restore
the `is_mounted_rw` guards and the `Skips` recorders, and drop the three calls
from `main.rs`. The boot log goes back to reporting six skips and the suites go
back to passing without testing anything. The only reason to do this is if the
post-mount block itself is found to run before the mount — in which case the
right fix is to move the *call*, not to restore the skip.
