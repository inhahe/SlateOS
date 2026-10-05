### B-DASH-STDIN-FLAKE. `dash script-from-stdin` ring-3 self-test intermittently returns `InternalError` — WATCH 2026-07-01

**Where:** the boot self-test that runs the REAL `dash` shell over a script fed
on fd 0 (`kernel/src/proc/spawn.rs` ring-3 dash integration test; serial marker
"REAL dash shell script-from-stdin …"). Normally logs `… captured 55 bytes ==
expected, EOF→exit 0): OK`.

**Observed:** on one boot (2026-07-01, `BOOT_OK after 181s`) the harness logged
`WARNING: Path-Z real dash shell script-from-stdin self-test failed:
InternalError` (serial line 3589) instead of the OK line, while the *immediately
preceding* boots (identical dash test) passed. Load-dependent — same family as
B-CONTAINER-JAIL-TESTRACE / B-PTHREAD-YIELDBUDGET (intermittent races in the
ring-3 `clone`/`fork`/`exec`/reap + futex machinery). Non-fatal on this run:
BOOT_OK was still reached; only the one sub-test flaked.

**Assessment:** almost certainly the same underlying low-probability
spawn/exec/reap or futex race already tracked for pthread/container tests, not a
dash-specific logic bug. **Proper fix:** shares the root-cause work with the
pthread `clone`+futex deadlock (B-PTHREAD-YIELDBUDGET) — instrument the ring-3
spawn/reap path (lock-order tracer + futex wait/wake ordering) and fix the race;
also make the dash harness distinguish a transient spawn failure from a real
shell error. Logged so the intermittent dash failure isn't forgotten.

**Diagnostic classification DONE (2026-07-01):** all ~34 ring-3 real-binary
self-tests (`proc/spawn.rs`: glibc hello/stdio/full/pthread/signal/fault/
sigqueue/forkexec/pipe/redir/redirin, all 16 real-dash tests, make/cc/hosted-cc/
make+tcc) previously collapsed *both* a **hang** ("did not exit within N yields")
and a genuine **wrong-result** (mismatched output/exit code) into the same
`KernelError::InternalError`, so a captured flake report ("InternalError") could
not be told apart from a real shell logic bug. Now the two are distinct: a
never-reached-Zombie timeout returns `KernelError::TimedOut` (the transient
spawn/reap/futex flake class — B-DASH-STDIN-FLAKE / B-PTHREAD-YIELDBUDGET), while
a completed-but-wrong result keeps `InternalError`; fd-redirect infrastructure
failures now propagate the real fd-install error. So the non-fatal WARNING line
in `main.rs` is self-classifying: `TimedOut` == flake/hang, `InternalError` ==
real logic bug, other == infra. This does not *fix* the underlying race (root-
cause work still pending), but future flakes are now unambiguously attributed.
The B-PREEMPT-SPINLOCK preempt-disable fix (top of file) may also have reduced
this race's incidence; watching future boots for recurrence.

**Boot-data points (post B-PREEMPT-SPINLOCK fix):** 2026-07-01 (BOOT_OK 177s):
dash script-from-stdin passed (`captured 55 bytes == expected, EOF→exit 0: OK`);
no recurrence. No unexpected WARNING/failed lines this boot (the only
`[lockdep] WARNING`s are the lockdep self-test's intentional AB/BA + transitive-
cycle detections, each followed by `OK`). **2026-07-02 (TD31 landed):** 4 further
consecutive green boots (190/182/181/185 s) with zero self-test-failure lines —
the dash script-from-stdin test passed on every one; no `InternalError`/`TimedOut`
recurrence even with the added spawn/reap CGROUP traffic. **2026-07-14 (bad
flake streak under host load): 3 consecutive `--no-build` boots HUNG** (no BOOT_OK
within 480 s) at three *different* spawn/reap points — run 1 mid ring-3 `dash`
dirstat test, run 2 after the `test-restart-ct` container-init spawn (line 9289;
same wedge the TD "symmetric cgroup accounting" entry documents), run 3 at a
glibc-dynamic-exec page-cache fault for pid 165 (before any container code) — then
**run 4 reached BOOT_OK in 136 s clean**. The three hangs were the pre-existing
spawn/force-kill/reap SMP race, *not* a code regression: run 3 wedged before the
container subsystem even ran, and the runs were competing for host CPU with
concurrent cargo builds + overlapping QEMU instances (the race is host-timing
sensitive, so heavy host load raises its incidence). The Q19/§60 multi-network
self-test (`Multi-network membership (attach/detach): OK`) passed on every run
that reached it (runs 2 and 4). Takeaway: **run boot tests one at a time on an
unloaded host** — overlapping QEMUs materially worsen this flake.
