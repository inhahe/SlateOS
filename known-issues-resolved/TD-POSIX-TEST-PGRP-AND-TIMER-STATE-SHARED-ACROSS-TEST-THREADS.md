### TD-POSIX-TEST-PGRP-AND-TIMER-STATE-SHARED-ACROSS-TEST-THREADS. The same parallel-runner race in two more posix statics: the foreground process group and the POSIX timer tables — 2026-08-12 — ✅ FIXED 2026-08-12

**What.** After the capability words were made per-thread, a 60-run hunt of
`cargo test -p posix --target x86_64-pc-windows-gnu` still caught three failing
runs, across three tests in two unrelated modules:

| Test | Symptom |
|---|---|
| `process::tests::test_tcsetpgrp_bad_pgrp_does_not_change_fg_pgrp` (`posix\src\process.rs`) | `left: 42, right: 555` |
| `time::tests::test_timer_settime_bad_it_value_tv_nsec_does_not_overwrite_slot_phase146` (`posix\src\time.rs`) | `left: -1, right: 0` |
| `time::tests::test_timer_gettime_efault_loop_no_state_change_phase148` | same class |

**Root cause.** Identical to the cap race, in three more statics:
`process.rs`'s `FG_PGRP` plus its `host_pg::PGID`\`SID` test double, and
`time.rs`'s `TIMER_TABLE`\`ITIMER_STATE`. Each module has a per-test reset
helper (`reset_pg()`, `reset_timers()`) whose doc comment claimed it gave
"isolation"; with process-global storage the reset instead *reached into every
concurrently running test*. The two shapes of failure follow directly: a test
asserting "a rejected `tcsetpgrp` left the foreground group at 42" read 555
because another test had just legitimately set it; a test asserting "a rejected
`timer_settime` left slot N untouched" read a slot another test had reset or
consumed.

**Fix.** The same cfg'd-storage pattern, applied three times:

* `FG_PGRP` → module `process::fg_pgrp` with `get`\`set`: `static mut` on
  `target_os = "none"`, `thread_local!` `Cell<PidT>` on host. The six repeated
  `unsafe` blocks at the call sites collapsed into the two accessors.
* `host_pg::PGID`\`SID` → `thread_local!` `Cell<PidT>`s behind accessors. That
  module is host-only to begin with, so there is no target arm.
* `TIMER_TABLE`\`ITIMER_STATE` → module `time::timer_store` handing out a raw
  `*mut` to each table (the call sites mutate in place): `static mut` on target,
  `thread_local!` `UnsafeCell` plus a teardown fallback on host — the exact
  shape `perthread::current()` already uses.

Both reset helpers' doc comments were corrected: they are now safe to call
unilaterally *because* the state is per-thread and libtest gives each test its
own thread, which is what makes "isolation" true rather than aspirational.

**Verified.** The three named tests are green: 40 consecutive suite runs with
zero failures among them. `cargo build` and `cargo clippy --all-targets` clean
for both the target and host builds.

**Not verified: the suite as a whole.** Those same 40 runs failed 7 times, on
six *other* tests, in five modules that were not touched here. See
TD-POSIX-TEST-SHARED-STATICS-REMAINING-TIER below — same failure class, more
statics. That entry is now fixed too; read the two together for the full
picture rather than either alone.

**Note for future statics.** Three separate incidents now share one cause: a
`static`\`static mut` in `posix` that a test mutates. The rule this establishes —
any mutable module-level state in `posix` that tests write must be per-thread on
host builds — is recorded in design-decisions.md §110.
