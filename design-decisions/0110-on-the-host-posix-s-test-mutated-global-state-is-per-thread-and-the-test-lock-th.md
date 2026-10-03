## §110 — On the host, posix's test-mutated global state is per-thread, and the test lock that stood in for that is gone

**Date:** 2026-08-12
**Decided by:** Claude (autonomous)

**Decision.** `posix::sys_capability` stores the effective/permitted/inheritable
capability sets in a `store` module with two cfg'd bodies: process-global
`AtomicU32`s on `target_os = "none"`, and a `thread_local!` `Cell<CapWords>` on
host builds. The crate-global `CAP_TEST_LOCK` mutex that previously serialised
every cap-mutating test — and its `CapTestLockGuard`, plus all 138 use sites
across 25 files — was deleted in the same change.

**Why this came up.** A cap-gated test failed roughly one run in three, a
different one each time, passing in isolation. The lock was already there and
was already documented as preventing "~150 spurious failures per run"; what it
could not prevent were the tests that *read* cap state without taking it. See
known-issues.md `TD-POSIX-TEST-CAP-STATE-SHARED-ACROSS-TEST-THREADS`.

**The real fork: per-thread state vs. more disciplined locking.**

*For per-thread state (chosen).* It makes the failure class unrepresentable
instead of merely defended against — no test can observe or disturb another's
caps, so no future test author has to know the rule. It fixed the unguarded
reader tests without editing any of them, which is the strongest evidence the
diagnosis was right. It removes a global mutex from a 20,128-test suite. And it
matches an established precedent in this same crate: `perthread.rs` moved 15
`static mut` libc buffers to per-thread storage for exactly this reason
(TD-POSIX-TEST-PARALLEL, §92), so this is the house pattern rather than a new
one.

*Against.* Host and target now genuinely differ in behaviour, not just in
mechanism — on the target, one thread's `capset` is visible to its siblings,
and on the host it is not. That is a real divergence a test cannot catch: a
future test asserting "thread B sees the cap A dropped" would pass on the
target and fail on host. It is defensible only because it is the *modelled*
semantics that differ from the *testing* substrate, and posix's host build has
no purpose other than being tested. The alternative reading — that capability
sets are per-thread for real (Linux credentials are in fact per-task) — is not
what our target build implements, so I did not use it as justification.

*The rejected alternative* was to keep the lock and add `CapGuard::snapshot()`
to the dozen reader tests. It is a smaller diff and preserves host/target
symmetry. Rejected because it leaves a live hazard behind an unwritten
convention: the next cap-gated "happy path" test added anywhere in the crate
reintroduces the flake, and the failure surfaces as an unrelated-looking errno
mismatch in a different module.

**Why the lock had to go rather than stay as belt-and-braces.** Its doc comment
stated a rationale that was no longer true, and stale rationale is worse than
none — it would have taught the next reader that cap state is shared. It also
serialised a large fraction of the suite for nothing. Deleting it doubled as
the experiment: with per-thread words removed *and* the lock removed, a still-
shared store would have reproduced the ~150 failures its own comment described.
It did not (82 runs).

**Where it lives.** `posix/src/sys_capability.rs` (`CapWords`, `CAPS_DEFAULT`,
`mod store`, `current_caps`/`set_current_caps`); the deleted `_lock` fields in
46 test-only `CapGuard`s across `posix/src/`.

**How to reverse.** Point both `store::load`/`store::store` bodies at the
atomics (delete the `cfg` split) and reinstate `CapTestLockGuard`. The 46
`CapGuard`s would each need their `_lock` field back — which is the cost signal
that the lock was the wrong layer for this.

### Generalised the same day: the rule now covers all test-mutated statics in `posix`

The cap fix left one uncaptured failure. Hunting it turned up three more —
`process::tests::test_tcsetpgrp_bad_pgrp_does_not_change_fg_pgrp` and two
`time::tests` timer-slot tests — with the identical cause in three *other*
statics: `process.rs`'s `FG_PGRP` and its `host_pg::PGID`/`SID` test double, and
`time.rs`'s `TIMER_TABLE`/`ITIMER_STATE`. All were converted to the same cfg'd
per-thread storage (see known-issues.md
`TD-POSIX-TEST-PGRP-AND-TIMER-STATE-SHARED-ACROSS-TEST-THREADS`).

Three independent incidents from one cause makes this a rule rather than three
fixes, so state it: **any mutable module-level state in `posix` that a test
writes must be per-thread on host builds.** The tell is a per-test `reset_*()`
helper — its very existence means tests write shared state, and under libtest's
thread-per-test model such a helper is a race, not the isolation its doc comment
usually claims. Three shapes were used at first, picked by what call
sites needed: a `Cell` behind `get`/`set` accessors (`fg_pgrp`, `host_pg`), a
`Cell` of a copyable struct (`sys_capability::CapWords`), or an `UnsafeCell`
handing out a raw `*mut` when sites mutate in place (`time::timer_store`,
`perthread`).  The third shape won and is now a macro — see *Consolidated into
`perprocess::process_global!`* below.

The *Against* argument above applies unchanged and is worth re-reading before
each new conversion: on the target these really are process-wide, and the host
build diverges. It stays acceptable only for state whose host build exists
solely to be tested — which is all of `posix`, but would not be true of, say,
kernel state a host harness is meant to model faithfully.

### The alternative that would have made all of this unnecessary: `--test-threads=1`

Worth writing down because it is the obvious question and the answer is not
"it's too slow". Running libtest single-threaded fixes every instance of this
class at once, with no code change and — crucially — **no host/target
divergence at all**, which is the one real cost of the chosen approach. It was
measured, not assumed: 20,128 posix tests take **4.14 s** single-threaded
against 2.2 s parallel. On performance grounds it wins easily.

It was rejected on *enforcement scope*. Cargo has no per-package test-harness
arguments, so the only way to make it stick is `RUST_TEST_THREADS = "1"` under
`[env]` in `.cargo/config.toml`, which applies to the entire workspace. CLAUDE.md
requires concurrency stress tests for every shared data structure in this repo;
serialising the harness workspace-wide to work around one crate's host-only test
doubles is the wrong default for a kernel project. And a *convention* — "always
pass `--test-threads=1` when testing posix" — is not enforcement: the next plain
`cargo test -p posix` flakes again, which is precisely how this class stayed
invisible for so long.

If Cargo ever grows per-package harness configuration, this decision is worth
re-examining, but the case for reverting is now much weaker than it looked when
this was written — at the time the estimate was "18 cfg'd storage modules" of
bespoke code, and the answer was to not start the remaining ~15. They were all
done, and the cost came out far lower than that: one macro plus a one-line
invocation per table (see below). Single-threading would still remove the
host/target divergence, which is the one genuine cost, so it remains the better
option *in principle*; it is no longer obviously worth the churn of reverting.

### Consolidated into `perprocess::process_global!` — 2026-08-12

**Decided by:** Claude (autonomous)

By the time the conversion reached its sixth module the cfg'd pair of storage
arms had been hand-written ten times, in three variants, each with its own
retelling of the rationale in a doc comment. That is the band-aid accumulation
CLAUDE.md warns about: the eleventh copy becomes a fourth variant, and ten
copies of a rationale drift apart.

`posix/src/perprocess.rs` now states it once, as a `process_global!` macro that
takes an accessor name, a type and a `const` initialiser and expands to a
`static mut` on the target or a `thread_local!` on the host. Converting a table
is one invocation and, where the module already had a `*_ptr()` accessor, zero
call-site changes. The module is named to pair with `perthread.rs`, and its docs
lead with the distinction between the two, which is the thing most likely to be
confused:

| | real scope | why the host build differs |
|---|---|---|
| `perthread` | per-**thread** | it doesn't — the target is per-thread too |
| `perprocess` | per-**process** | libtest puts many "processes" in one process |

**Modules converted** (in observed-failure order, which is how the whole effort
was driven): `sys_capability`, `process`, `time`, `resource`, `mman`, `stdio`,
`aio`, `fdtable`, `epoll`, `signal`, `pwd`, `unistd`, `sys_timex`,
`linux_aio_abi`, `mqueue`, `semaphore`.

**Deliberate carve-outs**, recorded in the module docs so nobody "finishes the
job" by converting them:

* `getopt.rs`'s `optarg`/`optind`/`opterr`/`optopt` are exported C ABI globals
  whose address a caller may legitimately take. Per-thread storage would change
  observable semantics, not just isolate tests.
* `pthread.rs`'s thread-specific-data table is *indexed by* thread. Making its
  storage per-thread would be a category error. (This covers the TSD table
  *only* — it was read as covering the whole module, which is how the cancel
  state discussed below went unexamined.)
* `sys_fsuid.rs`'s two `AtomicU32`s are already memory-safe, have only 6 call
  sites, and — unlike every other candidate — are touched by no other module's
  tests, so the sharing is confined to one small test module. Converting would
  replace safe atomic accessors with `unsafe` pointer derefs. This one is a
  judgement call, not a principle; if it ever shows up in a hunt, convert it.

**A carve-out that was wrong, and why.** `unistd.rs`'s `no_new_privs` bit was
initially left alone with its `nnp_guard()` mutex, on the strength of that
guard's own comment: *"the bit is in the kernel … making the bit per-thread
would be wrong."* The very next flake hunt failed on
`linux_seccomp::tests::…filter_with_nnp_no_cap_reaches_enosys`. The comment was
false: `NO_NEW_PRIVS` is an `AtomicBool` in `unistd.rs`, a posix static like any
other. Worse, it is read by *three* modules (`unistd`, `linux_seccomp`,
`linux_landlock`) while `nnp_guard()` lives in `unistd`'s test module and can
only serialise `unistd`'s own tests — exactly the shape of the original cap-lock
failure this section opens with, repeated verbatim.

The lesson is not about `no_new_privs`. It is that a carve-out justified by
what a comment *claims* about state is worth nothing; check where the storage
actually lives. Both other carve-outs above were re-verified against the code
after this.

The conversion keeps the target arm's `AtomicBool` verbatim and changes only the
host storage, which also disposes of the objection the old comment raised — the
product semantics on the target are untouched. The `nnp_guard()` mutex and both
copies of the `NnpGuard` RAII wrapper (in `linux_seccomp` and `linux_landlock`)
were deleted with it: once the bit is per-thread, they guard nothing.

### `perprocess` or `perthread`? The second wrong carve-out asked a different question

**Decided by:** Claude (autonomous)

The same hunt that caught `no_new_privs` also caught
`pthread::tests::test_setcanceltype_null_oldtype_succeeds`, reading a
cancellation type another test thread had just set. It looks like one more
instance of this section's bug, and it is not.

`pthread_setcancelstate`/`setcanceltype` are specified by POSIX in terms of
*"the calling thread"*, and a new thread is required to start `ENABLE` +
`DEFERRED`. So the cancellation state is **genuinely per-thread**, and storing
it in a process-global pair of atomics was a conformance bug that the *target*
build would hit the moment a program called `pthread_create` — no test harness
involved. A `perprocess`-style host-only split would have made the flake go away
while leaving the real defect in place.

It therefore went into [`crate::perthread`] — the block that is per-thread on
**both** arms — not into `process_global!`. The decision rule this establishes,
and which is now written at the top of both modules:

| If the spec says… | Storage | Host build differs? |
|---|---|---|
| "the calling thread" | `perthread` | no — the target is per-thread too |
| "the process" | `perprocess` | yes — a test thread stands in for a process |

Both POSIX defaults are `0`, which is what let the values ride in a block whose
whole contract is that all-zero is the valid initial state; a `const` assert
pins the constants so a later renumbering can't silently break that invariant.
The test-side `CancelGuard` and `reset_cancel_state_and_type()` went with it —
and the guard could never have worked anyway, since it restored the values it
*observed* on entry, which a concurrent test may already have changed.

### The size cutoff: TLS for small tables, a lazy `Box` for large ones

**Decided by:** Claude (autonomous)

A wrinkle that only shows up at this scale. libtest spawns a *fresh thread per
test* — 20k of them for this crate — and the OS allocates and zeroes a thread's
entire static TLS block at every one of those creations. Per-thread storage is
therefore not free in the way a single process's `static mut` is: it is paid
20,000 times.

For the tables here it is unmeasurable — `fdtable`'s 6 KiB table plus `epoll`'s
~80 KiB across four tables moved the suite from 2.2 s to 1.91–2.02 s, i.e. into
the noise, and in fact slightly *down*. But `fdtable`'s per-fd path table is
1 MiB (256 fds × `PATH_MAX`), and 20k × 1 MiB is ~20 GiB of pointless memset on
a suite that runs in two seconds.

So that one table is a lazily heap-allocated `Box` inside its `thread_local!`
instead of a const-initialised value: only the handful of threads that actually
run an `*at`/`fchdir` test pay for it. It is allocated via
`vec![[0u8; N]; M].into_boxed_slice()` specifically because that hits std's
`IsZero` specialisation and becomes one `alloc_zeroed`, rather than
materialising a 1 MiB temporary on a test thread's stack.

*Against splitting the approaches:* two shapes to understand instead of one,
and the boundary is a judgement call rather than a rule the compiler enforces.
*For:* the alternative is either a 10x slower suite or pushing every table onto
the heap, and the latter costs a lazy-init branch on every access to state
that is read on the `open()`/`close()` path. The cutoff is documented in
`process_global!`'s own doc comment ("keep values here to a few KiB; for
anything approaching a megabyte, hand-roll a lazy `thread_local!`") so the next
person meets the rule at the point of use rather than having to find this file.


### The test mutexes were removed, and removing them was the experiment

**Decided by:** Claude (autonomous)

Four modules (`sys_timex`, `linux_aio_abi`, `semaphore`, `mqueue`) wrapped their
tests in a `std::sync::Mutex` plus a `reset_*()` call. After the conversions
those were dead — **per-thread storage is the reset**, because libtest gives
every test its own thread and every thread starts from the initialiser. (This
holds even at `--test-threads=1`; libtest spawns a thread per test at *any*
concurrency, which was verified directly rather than assumed.)

*For leaving them:* they were harmless, and deleting them is pure churn in
green tests. *For removing them, which is what was done:* a second, now-false
mechanism for a problem solved elsewhere is worse than no mechanism, because
its comments assert a sharing that no longer exists and the next reader will
believe them — exactly the failure recorded above, where `nnp_guard()`'s
carefully-written doc comment is what kept the real bug alive for two weeks.
They also never made these tests correct: "fill every slot, then assert the
next open fails" is broken by a concurrent unlink no matter how well each
individual access is serialised.

The decisive argument is that **removing them is itself the experiment.** With
the locks gone, a suite that still fails has state that is genuinely still
shared, and the failure names the module. It passed — 20 133 tests green, and a
40-run hunt clean — so the conversions are confirmed complete rather than
merely masked by leftover serialisation. Keeping the locks would have made that
unfalsifiable.

The production spinlocks in those modules (`lock_aio()`, `TIMEX_LOCK`,
`SEM_LOCK`, mqueue's `lock()`) are untouched; they guard real concurrency on
the target and are not a test artifact.
