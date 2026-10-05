### TD-POSIX-TEST-SHARED-STATICS-REMAINING-TIER. `cargo test -p posix` was non-deterministic: ~15 more modules kept test-mutated state in process-globals — 2026-08-12 — ✅ FIXED 2026-08-12

**What.** With the capability, process-group and timer statics converted, 40
consecutive runs of `cargo test -p posix --target x86_64-pc-windows-gnu` still
failed **7 times**, each a single test out of 20,128, on six different tests:

| Test | Module |
|---|---|
| `resource::tests::setrlimit_cap_phase179::test_setrlimit_phase179_eperm_does_not_mutate_state` | `resource.rs` |
| `resource::tests::setpriority_cap_phase169::test_setpriority_phase169_workflow_raise_drop_cap_raise_lower` | `resource.rs` |
| `mman::tests::mlock_cap_phase171::test_mlock2_phase171_rlim_zero_no_cap_eperm` | `mman.rs` (reads `resource.rs`'s rlimits) |
| `epoll::tests::test_timerfd_settime_bad_fd` | `epoll.rs` / `fdtable.rs` |
| `stdio::tests::test_popen_register_full_table` | `stdio.rs` |
| `aio::tests::test_aio_fsync_o_dsync_aio_return_zero` | `aio.rs` |

**Why they are all the same bug.** Every one mutates module-level state that
libtest's thread-per-test model shares with every concurrent test.
`test_popen_register_full_table` is the clearest specimen: it calls
`reset_popen_table()`, fills all `MAX_POPEN` slots, and asserts the next
registration fails — so any other popen test that registers or resets
concurrently breaks it, in either direction.

**Scope.** The reliable tell is a per-test reset helper. There are 18, in 17
modules — every one is a latent instance of this bug:

`aio.rs` `reset_aio_table` · `getopt.rs` `reset_getopt_state` (returns a
`MutexGuard` — the lock approach §110 rejects) · `linux_aio_abi.rs`
`reset_aio_state` · `mqueue.rs` `reset_all` · `pthread.rs`
`reset_cancel_state_and_type` · `pwd.rs` `reset_state` · `resource.rs`
`reset_global_state` · `semaphore.rs` `reset_named_sems` · `signal.rs`
`reset_disposition`, `reset_blocked_mask` · `stdio.rs` `reset_popen_table` ·
`sys_fsuid.rs` `reset_creds` · `sys_personality.rs` `reset_personality` ·
`sys_timex.rs` `reset_timex_state` · `unistd.rs` `reset_hostid_for_test`
(plus the three already converted: `process.rs`, `time.rs`, `sys_capability.rs`).

**Fix (applied).** All of them were converted, in observed-failure order.
Partway through, the hand-written cfg'd storage module was replaced by a macro,
`perprocess::process_global!` — by the sixth module the same pattern had been
written out ten times in three variants, which is the band-aid accumulation
CLAUDE.md warns about. With the macro, converting a table is one invocation, and
zero call-site changes where the module already had a `*_ptr()` accessor.

Order and outcome:

| Module | State moved | Note |
|---|---|---|
| `resource`, `mman`, `stdio`, `aio` | rlimits + nice, popen table, aio table | the four observed victims from the 7/40 hunt |
| `fdtable` | fd table, per-fd path table + lengths | the fifth victim, via `epoll::tests::test_timerfd_settime_bad_fd` |
| `epoll` | epoll / timerfd / inotify instance tables, kernel-watch scratch | an fd's `handle` indexes these, so they had to follow `fdtable`'s scope |
| `signal`, `pwd`, `unistd` | dispositions + blocked mask; getpwent/getgrent cursors; CWD, hostname, domain | the modules with **no** protection at all |
| `sys_timex`, `linux_aio_abi`, `mqueue`, `semaphore` | NTP state, aio contexts, queues + descriptors, named-sem pool | already spinlock-protected, so never *unsafe* — but a lock cannot fix "fill all N slots, assert the next fails" |

`fdtable` was flagged here as needing thought before being touched, on the
theory that tests might lean on fds another test opened. They do not — the
suite passed first try after the conversion. Its 1 MiB path table did need a
different shape (a lazily heap-allocated `Box` rather than const-init TLS,
because libtest creates ~20k threads and would have zeroed 1 MiB at each);
see design-decisions.md §110, *The size cutoff*.

**Not converted, deliberately.** `getopt.rs` (`optarg`/`optind`/`opterr`/
`optopt` are exported C ABI globals whose address a caller may take),
`pthread.rs`'s **thread-specific-data table** (it is *indexed by* thread
already — note this covers only the TSD table, not the rest of the module; see
the cancel-state finding below), and `sys_fsuid.rs` (two `AtomicU32`s, already
memory-safe, 6 call sites, and touched by no other module's tests — converting
would trade safe accessors for `unsafe` derefs). Each is documented at the site.

**Two carve-outs were wrong, and a second hunt found both.** After the
16-module conversion a fresh 40-run hunt came back **2 / 40**, one test each:

| Test | Cause |
|---|---|
| `linux_seccomp::…::test_seccomp_phase186_filter_with_nnp_no_cap_reaches_enosys` | `unistd.rs`'s `no_new_privs` bit |
| `pthread::tests::test_setcanceltype_null_oldtype_succeeds` | `pthread.rs`'s cancel state/type |

*`no_new_privs`* was left alone because `nnp_guard()`'s comment said the bit
lives *in the kernel*. It does not — it is an `AtomicBool` in `unistd.rs` read
by three modules (`unistd`, `linux_seccomp`, `linux_landlock`), while the guard
sits in `unistd`'s test module and so can only serialise `unistd`'s own tests.
One test sets the bit and expects `ENOSYS`; a sibling clears it and expects
`EACCES`. Converted to per-thread host storage (the target keeps the
`AtomicBool` exactly); the mutex and *both* copies of the `NnpGuard` RAII
wrapper in `linux_seccomp` and `linux_landlock` went with it, 30 lines each.

*Cancel state* was never explicitly carved out — it was missed, because
`pthread.rs` appeared in the do-not-convert list for an unrelated reason (its
TSD table). It turned out not to be a `perprocess` case at all: **POSIX defines
cancellability as a property of the calling thread**, so the sharing was a plain
conformance bug that the target build would hit the moment a program called
`pthread_create`, entirely independent of tests. Converting it host-only would
have left that in place. The values moved into `crate::perthread` — the real
per-thread block on *both* arms — and both POSIX defaults being zero satisfies
that block's all-zero-is-valid invariant (pinned by a `const` assert). Its
snapshot/restore `CancelGuard` and reset helper went too (17 call sites each);
the guard could never have worked, since it restored the values it *observed*
on entry, which a concurrent test may already have changed.

The lesson generalises past the one already recorded: **a carve-out justified by
what a comment claims about state is worth nothing — check where the storage
actually lives**, and then check *why* it is shared. "Per-process state that
tests happen to share" and "state that was never supposed to be process-wide at
all" look identical in a flake report and have different fixes — the first wants
`perprocess`, the second `perthread`.

**A third carve-out was wrong too — 2026-08-13.** `sys_fsuid::tests::
test_phase79_setfsgid_invalid_from_default_zero` failed once during an unrelated
TZ change, then passed on re-run. `sys_fsuid.rs` was the remaining deliberate
carve-out above, justified as *"two `AtomicU32`s, already memory-safe, 6 call
sites, and touched by no other module's tests"*.

That reasoning had a hole: **"no other module's tests" is not the test — libtest
runs a module's own tests concurrently with each other.** `sys_fsuid.rs` had ~30
tests, every one of which called a `reset()`/`reset_creds()` helper and then
mutated the same two atomics, so they raced *each other*. "Already memory-safe"
was true and irrelevant: atomics stop data races, not logical ones, and the
whole failure class here is "test A observes test B's value", which an atomic
faithfully delivers.

The module even documented its own race and called it acceptable:

```rust
let saw = current_fsuid();
// saw could be non-zero if a prior test left state; tolerate
// both, but always restore to 0.
let _ = saw;
```

A test that tolerates two answers because it cannot tell which it will get is
not a weaker test, it is a *disabled* one — and the tolerance is what stopped
anyone noticing the module was racy.

**Fix.** Converted to `perprocess::process_global!`, keeping the values as
`AtomicU32` (the macro changes only *where* storage lives, so the target build
is byte-identical in behaviour). Both reset helpers and all ~30 of their call
sites are gone — per-thread storage is the reset — and the cold-value test now
asserts `0` exactly instead of tolerating anything. 30 consecutive suite runs
clean afterwards.

**Amended rule for carve-outs.** The question is not "do other modules touch
this state?" but **"can two tests that touch it run at the same time?"** — and
with libtest's thread-per-test model the answer is yes whenever a module has
more than one test that writes it. By that rule the only defensible carve-outs
left are the two where converting would change *observable semantics*:
`getopt.rs`'s exported C ABI globals, and `pthread.rs`'s thread-indexed TSD
table. No remaining module qualifies on "nobody else touches it".

**Residual tech debt — resolved 2026-08-12.** Four modules carried test-only
`std::sync::Mutex` serialisation — `sys_timex` (77 references), `linux_aio_abi`
(28), `semaphore` (14), `mqueue` (2) — that was redundant once the state they
guarded became per-thread. They were harmless, but they were a second, now-false
mechanism for a problem solved elsewhere, and their comments claimed a sharing
that no longer existed. All four are now removed, along with the `reset_*()`
helpers they called (`reset_aio_state`, `reset_timex_state`, `reset_named_sems`,
`reset_all`), which per-thread storage makes unnecessary: every test thread
starts from the initialiser. The production spinlocks in those modules
(`lock_aio()`, `TIMEX_LOCK`, `SEM_LOCK`, mqueue's `lock()`) are untouched — they
guard real concurrency on the target and are not a test artifact.

Removing them was also a *test*, and it passed: 20,133 tests green with no lock,
so no state in those four modules was still shared. Had a suite failed without
its lock, that would have identified another module whose "per-process" state
was really per-thread.

**Rejected: run the suite single-threaded.** `--test-threads=1` would fix all 18
at once with no code change and no host/target divergence, and it is not slow —
measured 4.14 s vs 2.2 s for the 20,128 tests. It was rejected because there is
no way to make it *stick* per-crate: Cargo has no per-package test-harness args,
so it would have to be `RUST_TEST_THREADS=1` in `.cargo/config.toml`, which
applies to the whole workspace. CLAUDE.md requires concurrency stress tests for
every shared data structure, and serialising the workspace's test harness is the
wrong default for a kernel repo. A convention ("always pass `--test-threads=1`
for posix") is not enforcement — the next plain `cargo test -p posix` flakes
again. See design-decisions.md §110.

**If it comes back.** A single failing test out of ~20,133 in a `posix` run,
passing in isolation, is this failure class until proven otherwise — look for
module-level mutable state the test writes, and check it against the
carve-out list above before assuming a real regression.
