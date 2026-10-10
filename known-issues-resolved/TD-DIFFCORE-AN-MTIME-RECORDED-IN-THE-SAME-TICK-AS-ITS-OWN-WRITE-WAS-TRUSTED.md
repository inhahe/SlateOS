### TD-DIFFCORE-AN-MTIME-RECORDED-IN-THE-SAME-TICK-AS-ITS-OWN-WRITE-WAS-TRUSTED. `FileSync::changed` reported an external edit as `Unchanged`, so the next save would have silently overwritten it — 2026-08-12 — ✅ FIXED 2026-08-12

**Where:** `apps/diffcore/src/lib.rs::FileSync::changed` (the mtime fast path)
and `FileSync::record`/`touch` (which captured the timestamp). Consumers:
`apps/editor/src/main.rs::Document::disk_changed` and the identical method in
`apps/markdowneditor/src/main.rs` — both feed the external-change prompt that
offers a three-way merge.

**How it surfaced.** The full-workspace run after commit `a6e286332` failed
`diffcore`'s own `tests::test_filesync_detects_modify_and_delete` with
"expected Modified, got Unchanged". It reads as a timing flake and is not one:
the test writes a file, records it, rewrites it, and asks. It passed or failed
depending on whether the two writes landed in the same filesystem timestamp
tick — i.e. on machine speed.

**The bug.** `changed()` treated "the file's current mtime equals the one I
recorded" as proof the file was untouched. A filesystem timestamp is coarse
(FAT: 2 s; ext3: 1 s; NTFS is nominally 100 ns but is stamped from a system
clock that ticks in ~15.6 ms steps), so a write landing inside the same tick as
the recording carries the *same* mtime. The pre-filter then short-circuits to
`Unchanged`, the editor never raises the external-change prompt, and the next
save writes the buffer over the external edit with no merge and no warning —
silent data loss, not a missed notification.

**Why "compare `SystemTime::now()` when checking" does not fix it.** Two wrong
fixes were tried first. Re-deciding raciness at check time is unsound: the
aliasing write happened *inside* the granularity window, which has already
closed by the time anyone asks, so no amount of later waiting makes the
timestamp more informative. Deleting the fast path outright is sound but throws
away a real optimisation (an untouched file is not re-read).

**The fix.** Git's "racily clean" rule from `read-cache.c`. `record`/`touch`
now go through a private `stat()` that captures the mtime *and* decides, at
that moment, whether it is trustworthy: `SystemTime::now() - mtime <
MTIME_SETTLE` (2 s, sized for the coarsest granularity we could plausibly be
asked to watch) sets `FileSync::mtime_racy`, and `changed()` refuses the fast
path while it is set. A too-large `MTIME_SETTLE` only costs a content read; a
too-small one loses an edit.

**Known cost, accepted.** The flag is sticky until the next `record`/`touch`,
so a file recorded immediately after a save is content-compared on every check
until the next save. Git accepts the same cost (it only smudges a racily-clean
entry when it rewrites the index). The refinement — clear the flag from
`changed` once a content comparison came back identical *and* the mtime has
outlived `MTIME_SETTLE` — would force `changed` to take `&mut self` and so
would stop read-only UI code from asking. Revisit if a caller is shown to poll
hot; `check_external_change` currently has no production caller at all.

**Regression tests.** `test_filesync_same_tick_rewrite_is_not_missed` (written
with no `sleep` on purpose — a sleep would hide the very adjacency being
tested) and `test_filesync_settled_mtime_is_trusted` for the other half of the
trade.

### [FIXED 2026-10-01 -- the kernel keeps the ignored set for both ABIs; see A-IGNORED-SIGNALS-DID-NOT-SURVIVE-EXEC-OR-SPAWN and design-decisions §1512] TD-KERNEL-NATIVE-ABI-SIG_IGN-IS-INVISIBLE-TO-THE-KERNEL. Terminal-access job control cannot honour a native-ABI process's `SIG_IGN`, so a native shell must *block* `SIGTTOU` where bash *ignores* it — 2026-08-12

**Fixed 2026-10-01.** The native libc now reports its ignored signals (`SYS_SIGNAL_SET_IGNORED`), and `signal_ignored_or_blocked` asks the kernel's set for both ABIs: a native shell may ignore `SIGTTOU` as bash does. The "narrow report this signal as ignored call" below is what was built. The entry is kept as written for its history.

**Where:** `kernel/src/syscall/handlers.rs::signal_ignored_or_blocked`, used by
`tty_job_control_decide` (the `SIGTTIN`/`SIGTTOU` policy added in
`design-decisions.md` §115). The table it cannot reach is
`kernel/src/syscall/linux.rs::linux_sigaction_table` (private, Linux-ABI only).

**The gap.** POSIX says a background terminal access whose signal is *ignored*
behaves as if the signal had been sent and discarded: a read fails `EIO`, a
write proceeds. `signal_ignored_or_blocked` can answer that from three sources,
and only two are exact for both ABIs:

- the **blocked mask** — kernel-owned, exact for native and Linux alike;
- **no trampoline registered**, in which case the kernel's own default-action
  table *is* the disposition — exact;
- an explicit **`SIG_IGN`**, which lives in `linux.rs`'s per-pid sigaction
  table and is therefore visible only when `pcb::get_abi_mode(pid)` is
  `AbiMode::Linux`.

So a native-ABI process that registers a signal trampoline and then sets
`SIGTTOU` to `SIG_IGN` in its own userspace disposition table is indistinguishable,
from the kernel's side, from one that left it at the default. It gets signalled
where a Linux-ABI process would have been let through.

**Why it is not simply a bug.** `kernel/src/proc/signal.rs` states the standing
architecture: the kernel tracks the pending set, the blocked mask and the
trampoline address; **userspace owns the native per-signal disposition table**.
`SYS_SIGNAL_STOP_SELF` (1062) exists precisely because of that asymmetry —
userspace *reports* an already-resolved disposition instead of asking the
kernel to re-derive one. Inventing a kernel-side native sigaction table solely
to serve this predicate would duplicate state userspace already owns, which is
the exact bug shape §113 and §114 were spent removing.

**Reproduce.** A native-ABI program that is in the background of its
controlling terminal, has a trampoline installed, and has set `SIGTTOU` to
`SIG_IGN`: `tcsetpgrp(0, pgrp)` signals it (or returns `EIO` if its group is
orphaned) instead of proceeding.

**Practical consequence.** A job-control shell written against our native libc
must `sigprocmask(SIG_BLOCK, {SIGTTOU})` around `tcsetpgrp` rather than
`signal(SIGTTOU, SIG_IGN)` the way bash does. `services/ctest-ctty/main.c`
checks 81-86 demonstrate the blocking form, so the limitation is executable
documentation rather than a comment.

**Proper fix.** Give the native ABI a kernel-visible disposition for the
*Ignore* case only — either a native `sigaction` syscall recording into a
per-pid table promoted out of `linux.rs` into an ABI-neutral module, or a
narrow "report this signal as ignored" call shaped like `SYS_SIGNAL_STOP_SELF`.
**Trigger:** when a native-ABI job-control shell is written, or sooner if any
native program needs POSIX-exact ignore semantics. Also tracked in `todo.txt`
item 1a of the controlling-terminal section.
