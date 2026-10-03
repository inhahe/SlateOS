## B-A-FCOMPRESS-ROUND-TRIP-TESTS-DEMANDED-A-COMPRESSION-RATIO-THAT-IS-IMPOSSIBLE (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** Five tests compressed a ~110-byte string and asserted the result
came back compressed. It cannot: the compressed-file format spends 27 bytes on
headers and checksums before a single byte of data, and the module correctly
refuses to "compress" anything it would make bigger. The tests were demanding
the bug.

**Found by** wiring `fs::fcompress::self_test` into the boot path
(`B-A-FORTY-ONE-SELF-TESTS-HAD-NEVER-RUN`). Boot cycle 4 panicked at
`fcompress.rs:598`, "should have compressed".

**Mechanism.** `compress_for_write` returns `None` when the compressed form is
not smaller — that is the incompressible-skip path, and it is correct.
`crate::fs::lz4::compress` emits the LZ4 **frame** format: 4 magic + 11
descriptor + 4 block header + 4 end mark + 4 content checksum = 27 fixed bytes.
A 113-byte mostly-English payload with three 10-byte runs cannot save that much,
so the block is stored verbatim and the frame comes out ~140 bytes. `None` is
arithmetic, not a bug. Gzip (18 fixed bytes) and zstd tests had the same shape.

Notably the overhead *was* understood when this was written —
`test_incompressible_skip` carries the comment "Small enough that LZ4 overhead
makes compressed >= original" — the round-trip tests simply landed on the wrong
side of the same threshold, and never ran to reveal it.

**Fix.** A shared `compressible_sample()` returning 4 KiB of repeating text, used
by the lz4/gzip/zstd round-trip tests, `test_rule_matching` and `test_stats`.
That amortises any of the three headers and lets the tests assert what they exist
to assert: that data survives compress → decompress unchanged *and* that it
actually got smaller (a new assertion — `Some` is a promise that the stored form
is smaller, and nothing checked it).

`test_rule_matching` was changed for a second reason: it is about prefix and
extension matching, but with a small payload its `is_some()` was also a bet on
the gzip ratio, so a ratio regression would have failed a matching test.

`test_incompressible_skip` accepted *either* answer, noting "The important thing
is it doesn't panic" — so the skip path had no coverage at all. It now asserts
`None` deterministically (32 distinct bytes cannot shrink, and 27 bytes of frame
go on top) and asserts the skip counter incremented. That matters beyond
tidiness: the skip path is where `note_skipped` lives, and the two inlined copies
it replaced each took `STATE` twice in one statement and would have deadlocked
the moment they ran. A test that cannot fail never ran them.

**Severity.** Test-only; no production defect. But five of the eight tests in the
module were unrunnable, including the only coverage of a path that contained a
fatal deadlock.

### [RESOLVED 2026-08-22] TD-A-REAP-WINDOW-BETWEEN-SET-CURRENT-AND-SWITCH. `set_current_task` retires a dying task from the reaper's exclusion set while it is still running on its own kernel stack — SMP-only, latent 2026-08-22

**RESOLVED** the same day it was logged, with exactly the fix this entry
specified: a cache-padded per-CPU `PREV_TASK_IDS`, stored between
`set_current_task` and `switch_context` at **both** switch sites (the main
`schedule_inner` path and the idle fallback — the latter matters more, not
less, since it is reached from `task_exit` and so its outgoing task is
frequently already `Dead`), cleared by the incoming task, and unioned into
`reap_dead_tasks`'s exclusion set. Design rationale, the alternatives, and the
memory-ordering argument for why the two coverage windows *overlap* rather than
abut are in `design-decisions.md` §276.

**One thing the plan in this entry did not mention, and it is load-bearing.**
"The first thing the incoming task does after the switch returns is clear it"
covers a *resumed* task only. A task running for the **first** time never
returns from `switch_context` — it arrives at `task_entry_trampoline` instead —
so the trampoline needed its own `call sched_finish_task_switch`, placed before
`call rbx` (safe: it clobbers only caller-saved registers, and the trampoline's
two live values `rbx`/`r12` are callee-saved). Without it the predecessor stays
pinned until the next context switch on that CPU, which on an idle CPU is
never, turning the intended bounded delay into a permanent leak of one stack
per CPU.

Also worth recording: the clear re-reads the CPU index rather than using the
`cpu` local already in `schedule_inner`'s frame. A resumed task can come back
on a different CPU than it blocked on, in which case that local is the *old*
CPU's index — clearing it would leak this CPU's pin and drop a live pin on the
other one at the same time.

**Regression test:** `sched::test_prev_task_pins_outgoing_stack`, with two
independently-failing halves because the race itself is unprovokable under a
uniprocessor boot test (a soak would pass forever and prove nothing): a `Dead`
task named in `PREV_TASK_IDS` must survive a reap *and* be reaped once released
— without the second leg the test also passes against a reaper that never reaps
anything — and a brand-new task must observe a cleared slot, which is the only
thing that would fail if the trampoline's one asm line were deleted. Verified
in boot cycle 11: `Pinned Dead task survives reap: OK`, `Released Dead task is
reaped: OK`, `First-run task clears the pin: OK`, `Scheduler self-test PASSED`.

**What it was.** `reap_dead_tasks` (`sched/mod.rs:4667`) frees a `Dead` task's
kernel stack, and it correctly refuses to reap any task that is *current* on
any CPU — it builds `active_ids` from `CURRENT_TASK_IDS` across all online
CPUs, with a comment explaining that reaping a task another CPU is running is
a use-after-free. That exclusion is right, but it stops covering the task one
step too early.

In `schedule_inner` the sequence is:

```
set_current_task(cpu, next_id);   // <-- the outgoing task leaves active_ids HERE
… write_cr3 …
… wrmsr FS_BASE / GS_BASE …
switch_context(old_ctx_ptr, new_ctx_ptr);   // <-- and only HERE does it leave its stack
```

Every instruction between those two lines executes **on the outgoing task's
kernel stack**, and `switch_context` itself pushes the outgoing register set
onto it. But from `set_current_task` onward the outgoing task is no longer in
any CPU's `CURRENT_TASK_IDS` slot, so a concurrent `reap_dead_tasks` on
another CPU sees a `Dead` task that nobody is running and frees the stack out
from under it.

**Why it is latent today.** The boot test runs uniprocessor (`cpu0` only, no
APs brought up), so no second CPU exists to run the reaper concurrently. Every
current `reap_dead_tasks` caller is a self-test or a bench harness on the boot
CPU. The window is real but unreachable until SMP is exercised, which is
exactly the kind of bug that surfaces the first time someone turns APs on and
then gets blamed on the AP bring-up.

**Same class as `B-FORKEXEC-BOOT-HANG`** (fixed 2026-08-22): a resource is
published as reclaimable at a point that precedes its genuine last use. There
it was the process PML4 versus the dying thread's CR3; here it is the kernel
stack versus the outgoing task's `switch_context`. Both come from treating a
bookkeeping update as if it were the moment the hardware stopped depending on
the object.

**Proper fix** — Linux's `finish_task_switch` / `put_task_struct` shape: hand
the outgoing task off to the *incoming* one rather than declaring it free.
Concretely, add a per-CPU `PREV_TASK_ID` slot; `schedule_inner` writes the
outgoing id into it just before `switch_context`, and the first thing the
incoming task does after the switch returns is clear it. `reap_dead_tasks`
then excludes the union of `CURRENT_TASK_IDS` and `PREV_TASK_ID` across all
CPUs. That closes the window structurally — there is no instant at which a
task is off both lists while a CPU is still on its stack — instead of relying
on the reaper being called from the right places.

**Do not "fix" it by having the reaper skip recently-dead tasks on a timer.**
That converts a correctness bug into a probabilistic one and makes the failure
rarer and harder to attribute, which is strictly worse.

### [RESOLVED 2026-08-22] TD-A-LOCKDEP-VIOLATION-REPORT-NAMES-NO-ADDRESS. `Vfs::unmount` and four other sites took a per-mount filesystem lock while holding the global VFS lock, inverting the order the overlay depends on — found 2026-08-22 (reported for weeks as unreadable `lock "?"` warnings)

**RESOLVED.** Fixed in `1422972ad` (vfs), after `b215b83c1` (lockdep prints
addresses) made the reports readable and `70794b766` (symbolize picks the ELF
that was actually built) made the addresses resolvable.

Verified by boot cycle 10: `BOOT_OK`, and **all four violations are gone** —
the only `Holding lock` lines left are lockdep's own self-test pairs at
`0xdead000{1,2,3}`. No regressions: the FAIL count is unchanged at 5 (the 2
known lane-B `make` failures and 3 `drm-atomic` negative tests, all
pre-existing), overlay's 13 tests, container's 61 and OCI's 23 all pass, and
the `[vfs] Unmounted <fstype> from '<path>'` lines still name the right
filesystem, which exercises the rewritten two-phase `fs_type` capture.

Worth recording *why* one fix cleared four reports: only the `VFS -> per-mount`
direction was wrong. Removing that single edge broke every cycle in the order
graph, so the overlay's `per-mount -> VFS` / `-> OVERLAYS` edges — which are
the design working as §43 intends — stopped being reported as inversions
because there was no longer a reverse edge for them to invert against. This is
the confirmation that the analysis below was right; had the overlay's edges
been independently wrong, they would still be firing.

**Owner: lane A** (`kernel/src/lockdep.rs`, plus whichever subsystems the
inversions turn out to be in).

**In short.** The lock-order validator is doing its job: on every boot it
prints several "potential deadlock" warnings for real inversions in real
kernel code, not in its own self-test. But the warning identifies each lock by
a short name, and most locks in this tree are created with `sync::Mutex::new`,
whose default name is the single character `?`. So the report reads
"holding lock `?`, acquiring lock `?`" and there is no way to tell which
two locks it means. The bug being reported is invisible behind the report.

**The reports, from boot cycles 7 and 8 on 2026-08-22** (`build/serial-test.txt`;
the `test-A`/`test-B`/`test-C` pairs near line 2109 are lockdep's own self-test
and are excluded here). Each is followed by `But the reverse order was observed
previously.`:

| Held → acquiring | cycle 7 | cycle 8 | Where |
|---|---|---|---|
| `sysctl-reg` (12) → `?` (9) | line 20932 | **absent** | mid Path-Z ring-3 run, amid `[mmap]` traffic |
| `?` (18) → `?` (110) | line 37124 | line 37119 | inside `overlay`'s VFS mount adapter |
| `?` (130) → `?` (55) | line 39224 | line 39229 | late boot |
| `?` (130) → `?` (18) | line 39228 | line 39233 | late boot |

Two facts from that table matter more than the individual rows:

- **Class 18 appears on both sides** — held in row 2, acquired in row 4. These
  are not four isolated pairs but a connected order graph over classes
  {130, 18, 110, 55}, so they should be resolved together, not one at a time.
- **The `sysctl-reg` row is intermittent**: it fired in cycle 7 and not in
  cycle 8, even though the surrounding `[mmap]` lines are byte-identical
  between the two logs. That does *not* mean it fixed itself. lockdep prints
  only the *escalation* — the acquire that completes a cycle whose reverse
  edge was already recorded — so whether it prints depends on which order the
  two acquires happened to occur in earlier in the same boot. An inversion
  that stays silent for a boot is still present in the code. It also means a
  report may be seen once and never again, which is precisely why the report
  has to be self-describing the one time it does fire.

Every one of these is an escalation, so the reverse-order acquire also
happened at some earlier, un-reported point — the report names the second half
of the inversion only.

**Why the names are `?`.** `sync::Mutex::new` (`sync.rs:381`) sets
`name: b"?"`; only `Mutex::named` supplies a real one. That default is
reasonable — naming every lock is a per-site cost — but it means the *report*
must carry something else that identifies the lock, and it does not.

**The diagnostic half is now FIXED** (this commit). `dump_held_locks`
(`lockdep.rs:781`) hit this exact wall and solved it by printing
`{name} @ {addr:#x}`; its comment spells out the reasoning ("entries that
cannot be told apart from each other"). `report_violation` and
`report_recursive` were not given the same treatment and now are, via a new
`class_addr` helper that applies `class_name`'s "admit the unknown" readiness
gate on top of the existing `class_id` read. `LockClass::id` *is* the lock
address and was already in hand, so this was a formatting change, not a
plumbing one. With addresses in the log, the locks resolve
to their statics with `python scripts/symbolize.py 0x<addr>` — which already
reports non-text symbols and tags each hit with its kind, and the debug kernel
ELF carries 2,310 `STT_OBJECT` symbols for it to hit. The kernel's *own*
resolver cannot do this: `ksyms::parse_elf_symbols` (`ksyms.rs:372`) skips
every symbol that is not `STT_FUNC`, so a runtime `ksyms::resolve` of a lock
address returns `None`. That is a deliberate size tradeoff, not a bug, but it
is why the resolution step is offline.

**RESOLVED — the locks are now named, and the culprit is identified.** From the
cycle-9 boot (the first with addresses), via `python scripts/symbolize.py`:

| Class | Address | Identity |
|---|---|---|
| 18 | `0xffffffff82782620` | `kernel::fs::vfs::VFS` +0x10 — the global VFS lock |
| 55 | `0xffffffff827fe8b0` | `kernel::fs::overlay::OVERLAYS` +0x10 |
| 110 | `0xffff80007d6e80a0` | heap — a per-mount `MountPoint::fs` lock |
| 130 | `0xffff80007d6ea120` | heap — a per-mount `MountPoint::fs` lock (overlay) |

(110 and 130 are heap addresses, so they do not appear in the ELF; they were
identified from the call sites instead. `MountedFs = Arc<Mutex<Box<dyn
FileSystem>>>`, `vfs.rs:967`.)

**One of the two directions is the design, and the other is a bug.**
`MountPoint::fs`'s own doc comment (`vfs.rs:972`, citing design-decisions §43)
states the invariant: *"the global VFS lock is released the moment the mount
table lookup is done"*, explicitly so that *"stacked filesystems (e.g. the
overlay) can re-enter the VFS to read their backing layers without
deadlocking"*. So:

- **per-mount `fs` lock → VFS / OVERLAYS is the intended order** — it is the
  overlay re-entering the VFS to reach its lower layer, which §43 designs for.
  That is rows 3 and 4 of the report table (class 130 → 55, 130 → 18), fired
  from the container-delete path in the OCI self-test.
- **VFS → per-mount `fs` lock is the violation** — row 2 (class 18 → 110),
  fired from `Vfs::unmount`, which takes `VFS.lock()` at `vfs.rs:1490` and is
  still holding it at `vfs.rs:1519` when it calls
  `vfs.mounts[idx].fs.lock().sync()`. That is the documented invariant broken
  literally, in the function whose own struct documents it.

**Five sites take a per-mount `fs` lock under the VFS lock**, not one —
`vfs.rs` lines 1519 (`sync()` in `unmount`), 1532 (`fs_type()`), 2809 and 2821
(`fs_type()` in mount listings), 3410 (`device_name()` in a device lookup).
Only 1519 has been caught so far because only it runs while another lock order
is live, but every one of them is the inverted order and the same deadlock
against a stacked filesystem re-entering the VFS. The last four call trivial
accessors that cannot themselves re-enter, which is why they have not hung
anything — but they still create the edge, and the risk is real the moment a
second CPU holds that `fs` lock and waits on `VFS`.

**The fix** is to honour §43 at all five: clone the `Arc<Mutex<..>>` (cheap —
it is an `Arc`, and cloning keeps the filesystem alive) under the VFS lock,
drop the VFS guard, then take the `fs` lock. `unmount` additionally has to
re-acquire and re-find its index afterwards, since the mount table can move
while it is unlocked.

Note that
an inversion reported once may be benign in practice (the two orders may be
unreachable concurrently on a uniprocessor boot) — but that judgement needs
the call sites, and "we could not identify it" is not the same finding as "we
looked and it was fine."

**Do not fix this by naming every lock.** Renaming ~800 `Mutex::new` sites to
`Mutex::named` is a large diff that would still leave the report unable to
identify a lock whose name was mistyped or duplicated. The address is unique
by construction and costs one format argument.
