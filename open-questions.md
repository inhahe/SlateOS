# Open Questions — Operator Decision Queue

Decisions that genuinely need the human operator: architectural forks,
user-visible policies, and tradeoffs with no obviously-correct answer that
Claude has **deferred** rather than resolved autonomously.

This file is distinct from:

- **`design-decisions.md`** — decisions already *made* (each marked with who
  decided it). When the operator answers a question here, move it there as a
  `Decided by: Operator` entry and delete it from this file.
- **`known-issues.md`** — bugs and accumulated technical debt.
- **`todo.txt`** — the working scratchpad / judgment-call log.

Format for each entry:

- **Question** — the decision to be made.
- **Options** — each with its pros and cons.
- **Claude's recommendation** — if there is a defensible default (and what
  Claude is doing in the meantime).
- **Where it bites** — files/symbols affected, so the resolution can be applied.
- **Status** — `OPEN` until the operator decides.

---

## Q25 — Next large initiative: which giant external port to prioritize now that the self-hosting C toolchain + POSIX layer are comprehensive?

**Status:** OPEN (logged 2026-07-16). This is a prioritization decision, not a
blocker on *all* progress — Claude continues de-risking work in the meantime (see
"in the meantime" below). It is surfaced because the roadmap's entire remaining
unchecked frontier is now giant external ports, and picking among them has
historically been the operator's call (Q3 §… / Q12 §36 / Q15 §40).

**Context.** The on-target C toolchain is now proven end-to-end: `tcc` compiles C
on-target, glibc dynamic linking + `ld.so` load + ring-3 execution all work, and
the Path-Z self-test suite (Parts 1–58) validates a comprehensive set of C/GNU
constructs (types, structs, unions, bitfields, float/long-double, VLAs, function
pointers, computed goto, TLS, setjmp/longjmp, varargs, signals, ctors/dtors,
inline asm, C11 atomics via libtcc1, GNU statement expressions + `__typeof__`,
aggregate init, separate compilation, project headers). The POSIX layer (§2.5) is
extraordinarily complete. So the prerequisites that historically gated the big
ports are largely in place — the remaining work is the ports themselves.

**Question.** Which of the roadmap's remaining giant initiatives should be next?

**Options.**

- **A. bash + coreutils (interactive shell userland).** *Pros:* smallest gap of
  the remaining items — builds directly on the just-proven tcc/glibc/`ld.so`/
  ring-3 path; unblocks a real self-hosting dev environment and essentially all
  user-facing/CLI work; each coreutils tool is individually small so progress is
  incremental and continuously shippable. *Cons:* bash itself (~150k LOC C) is
  non-trivial; needs job control, terminal `tcsetattr`/pgrp semantics, and a
  robust `fork`/`exec`/`waitpid` (a couple of the fork/exec paths are still WATCH
  bugs — B-FORKEXEC-BOOT-HANG, B-PTHREAD-TEARDOWN-PF).
- **B. Mesa / GPU userspace (3D).** *Pros:* unlocks hardware-accelerated
  compositor + real GPU apps. *Cons:* explicitly deferred by Q18 until a virgl
  test environment exists; large; blocked on that prerequisite.
- **C. Chromium (~35M LOC C++).** *Pros:* the single highest-impact app (browser +
  the "system web app" framework, VS Code, Electron apps). *Cons:* enormous;
  needs GPU, audio, networking, and POSIX all *fully* mature; by far the biggest
  single undertaking.
- **D. WINE (Windows app compatibility).** *Pros:* runs unmodified Windows apps.
  *Cons:* large; needs mature graphics + extensive PE-loader / Win32-syscall work;
  higher risk, less incremental.
- **E. Additional filesystems (Btrfs / F2FS / NTFS).** *Pros:* self-contained
  kernel/fs ports with clear boundaries; moderate size; no userspace-graphics
  dependency. *Cons:* lower immediate user-visible payoff than a shell/browser.
- **F. fastpy build-system integration.** *Pros:* infrastructure (not an external
  port) — unblocks writing OS userspace tools in Python-via-fastpy, which
  `CLAUDE.md` explicitly encourages (package manager, settings UI, file indexer,
  installer, etc.); force-multiplier for all subsequent userspace work. *Cons:*
  depends on fastpy maturity; a build-system initiative in its own right.

**Claude's recommendation.** **A (bash + coreutils) first, then F (fastpy
integration).** A is the smallest, highest-leverage next step and builds directly
on what was just proven; a working shell + coreutils is the natural foundation for
everything else and is continuously shippable one tool at a time. F then unlocks
the Python userspace lane for the many small system tools. B/C/D are larger and
either gated (B on Q18/virgl) or dependent on more maturity (C/D on graphics+audio).

**In the meantime (not blocked).** Claude is *not* idling on this: it continues
de-risking the likely-next port (A) with bounded, valuable, non-port work —
extending the on-target compile validation from single-construct probes to
realistic multi-function programs (the actual prerequisite for compiling
coreutils/bash), and keeping the toolchain/self-test green. If that de-risking
line is exhausted and every remaining task is confirmed operator-gated, Claude
will stop and let the loop end per the state-(3) rule rather than manufacture
busywork.

**Where it bites.** `userspace/shell/`, `posix/`, `roadmap.md` (line 1494 bash;
line 24 fastpy; lines 5117–5119 filesystems; line 5032 Chromium; line 5114 WINE);
fork/exec WATCH bugs in `known-issues.md` (B-FORKEXEC-BOOT-HANG,
B-PTHREAD-TEARDOWN-PF) are the practical gates for A.

---

## Q24 — Raw `spin::Mutex` holder-preemption: reactive fixes vs. a proactive kernel-wide audit

**Status:** OPEN (logged 2026-07-15). Not blocking any current thread — Claude is
proceeding reactively and this only asks whether to also invest in a proactive
sweep.

**Question.** The kernel has two confirmed single-CPU **holder-preemption
deadlocks** on raw `spin::Mutex` locks — the global heap lock (fixed 2026-07-15)
and `container::TABLE` (fixed 2026-07-15). A raw `spin::Mutex` does not disable
preemption on acquire, so a holder can be involuntarily preempted mid-critical-
section; a second task then spins on the lock forever while the Ready holder
never gets scheduled on a single CPU. The preempt-aware `crate::sync::Mutex`
prevents this (it calls `preempt_disable()` on acquire) — but ~476 kernel files
import raw `spin::` locks. Should we (A) keep fixing these reactively as the hang
soak catches them, or (B) do a proactive audit/conversion?

**Options.**
- **A — Reactive (status quo).** Fix each lock as `scripts/wedge-soak.sh` catches
  it (the backtrace names the exact lock). *Pros:* zero churn on the ~476 files;
  no lockdep-scope explosion; each fix is targeted and validated by the very
  repro that found it; most raw locks are genuinely safe (true leaf locks,
  trivially short sections, or never contended under preemption). *Cons:* latent
  deadlocks remain until a soak happens to hit them under the right timing;
  relies on soak coverage; each new one costs a catch+diagnose+fix cycle.
- **B — Proactive full audit/conversion.** Mechanically convert `use spin::Mutex`
  → `crate::sync::Mutex` kernel-wide (or triage each). *Pros:* eliminates the
  whole class at once; adds lockdep + owner tracking everywhere (would *catch*
  future ordering bugs too). *Cons:* huge, risky one-shot change; drags every
  lock into lockdep — memory per lock, first-acquire registration allocation, and
  a flood of newly-surfaced lock-ordering reports to triage; perf cost of lockdep
  on hot leaf locks; some locks are deliberately raw (heap) and must stay raw +
  manual-preempt, so it can't be a blind sed.
- **C — Middle path.** Add a preempt-aware-but-*not*-lockdep spinlock to
  `crate::sync` (e.g. `PreemptSpinMutex`: just `preempt_disable/enable` around the
  raw spin, no registry), and convert only the *contended, non-leaf* locks to it
  (or to `crate::sync::Mutex` where lockdep is wanted). *Pros:* closes the
  deadlock class on the locks that matter without the lockdep explosion; cheap.
  *Cons:* still requires judgment per lock about which are "contended/non-leaf";
  adds a third lock type to the codebase's vocabulary.

**Claude's recommendation.** **A for now, with C as the escalation** if a third
or fourth instance shows up. Two instances is not yet evidence that the reactive
approach is failing, and the soak is a reliable detector. If the same class keeps
recurring, switch to C (a targeted preempt-aware spinlock for contended non-leaf
locks) rather than the full-blown B. Meanwhile Claude keeps fixing caught
instances properly (holder-side preempt protection), and each is documented in
`known-issues.md`.

**Where it bites.** `kernel/src/sync.rs` (`Mutex`, and a possible new
`PreemptSpinMutex`); every `use spin::Mutex` site (~476 files); the two fixed
so far: `kernel/src/mm/heap.rs`, `kernel/src/container.rs`. Detector:
`scripts/wedge-soak.sh`.

**Update (2026-07-15b) — a *suspected* third instance narrows toward the
escalation trigger.** `B-FORKEXEC-BOOT-HANG` (known-issues.md) is a silent,
output-less boot hang that a static re-audit this session pinned to the
**task-exit teardown path** (last serial line `[sched] Task N exiting`, then
`task_exit`→`notify_exit_hooks`→process teardown), *after* ruling out the
waitpid/scheduler-wakeup hypotheses (the harness polls, never blocks; the
`wake`/`block_current` `pending_wake` protocol is sound; the parent already
reached Zombie). A mid-teardown silent freeze is the exact signature of a
holder-preemption spin-deadlock — the same class as the two fixed ones — which
would make it a **suspected third instance**. It is not yet *confirmed* (needs a
repro with `--hard-lockup-watchdog` RIP capture to prove the wedge sits in a
`spin::Mutex::lock` spin vs. an idle-reschedule bug). Per the recommendation
above ("switch to C if a third or fourth instance shows up"), a confirmed third
instance would tip the balance toward **C** (add a `PreemptSpinMutex` and
convert the contended non-leaf locks on the process-exit/reap path:
`PROCESS_TABLE`, the reaper, and the exit-hook locks). Recommend the operator
decide between staying on **A** vs. pre-authorizing the targeted **C** sweep of
the exit/teardown path specifically (a bounded, non-kernel-wide subset) now that
two separate intermittent boot bugs both point at teardown-path contention.

**Update (2026-07-15c) — a confirmed third instance, but a *different sub-variant*
than the one suspected above; reactive approach still holding.** The wedge-soak
caught a live wedge this session and it root-caused to **`sysctl::REGISTRY`**
(commit 0da3324e5, `known-issues.md` B-SYSCTL-IRQ-DEADLOCK), NOT to the
`B-FORKEXEC-BOOT-HANG` teardown path suspected in update 2026-07-15b. It is a
confirmed third instance of the broad "raw `spin::Mutex` deadlock" class, but of
the **interrupt-reentrancy** sub-variant (a lock acquired *blockingly from IRQ/
exception context* — timer `check_starvation` + the `#PF` stack-grow reader —
while a task held it across a slow `serial_println!`), rather than the
**holder-preemption** sub-variant of the first two. Fixed reactively (approach A):
a non-blocking `sysctl::try_get` for the IRQ-context readers + not holding
`REGISTRY` across the log — no new lock type needed. Crucially, I then did the
**bounded proactive audit** that would have been step one of a **C** sweep, but
scoped to the *interrupt-reentrancy* surface only: the timer hard-IRQ path
(`sched::timer_tick`/`check_starvation`, `cgroup::{cpu_charge,cpu_period_reset,
io_period_reset}`, all of `hrtimer`) and the `#PF` handler — **all clean** (each
already uses `try_lock` in IRQ context or `without_interrupts` on every task-side
holder). So for the IRQ-reentrancy variant, the hot paths are audited-clean and
reactive-A is demonstrably sufficient. The **holder-preemption** variant on the
process-exit/teardown path (`B-FORKEXEC-BOOT-HANG`) remains *unconfirmed* — the
soak caught sysctl first, not a teardown wedge — so the case for a **C** sweep of
the teardown locks (`PROCESS_TABLE`/reaper/exit-hooks) is neither strengthened nor
weakened by this catch. Net: no change to the recommendation (stay on **A**;
escalate to **C** only if the *teardown* hypothesis gets a confirmed RIP capture).
Operator input still only needed on whether to pre-authorize that bounded
teardown-path **C** sweep proactively.

**Update (2026-07-15d) — a confirmed fourth instance, again the interrupt-
reentrancy sub-variant; reactive-A still holding.** Root-caused
**B-SCHED-SPAWN-DEADLOCK** (the intermittent tcc-ring-3-spawn boot wedge the soak
kept re-catching) by static audit to a single blocking `SCHED.lock()` reachable
from softirq context: the timer softirq's `ipc::timer::process_timer_expirations`
→ `completion::notify` → `sched::wake()` (commit 67e224938,
`known-issues.md` B-COMPLETION-TIMER-IRQ-DEADLOCK). Same **interrupt-reentrancy**
sub-variant as B-SYSCTL (a lock taken blockingly from softirq while a task holds
it — here `SCHED`, whose holders don't disable interrupts), not the holder-
preemption sub-variant. Fixed reactively (approach A): softirq-safe
`completion::try_notify` (try_lock + `try_wake`, commit-nothing-on-contention) +
retry-next-tick in `process_timer_expirations` — no new lock type. As with the
sysctl fix, I did the **bounded proactive audit** of the *whole* softirq/IRQ →
`SCHED` surface (the `#PF` handler, `do_deferred_preempt`, `ioapic` device-IRQ
wake, every `softirq::handle_timer`/`handle_sched` sub-call, `ktimer`) and found
**this was the only blocking site** — everything else already uses
`try_lock`/`try_wake` or defers to the workqueue. So the interrupt-reentrancy
surface is now audited-clean *and* the one hole is closed.

Tally: four confirmed instances, but **three of the four are the interrupt-
reentrancy sub-variant** (sysctl, and now completion-timer→SCHED), all fixed with
targeted `try_lock` + audit and **no new lock type** — the reactive approach has
handled every interrupt-reentrancy case cleanly. Only the *holder-preemption*
sub-variant (heap, container — the original two) is the one a **C**
(`PreemptSpinMutex`) sweep would target, and no *new* holder-preemption instance
has appeared since. **Recommendation unchanged: stay on A.** The only thing still
worth the operator's input is whether to pre-authorize a bounded **C** sweep of
the process-exit/teardown locks (`PROCESS_TABLE`/reaper/exit-hooks) *if* the
still-unconfirmed `B-FORKEXEC-BOOT-HANG` teardown-wedge hypothesis ever gets a
confirmed RIP capture. Not blocking anything.

---

## Q23 — Session model for daemon-backed AF_INET **server** sockets (accepted-connection independence)

**Status:** OPEN (logged 2026-07-14; **now the sole remaining socket-fd gate for
the 5.7 default-flip** as of 2026-07-14). The daemon+ring listen/accept layer is
done and boot-validated (see `net-userspace-migration.md`, "Listen/accept server
sockets over the daemon"); this question gates the final AF_INET/AF_INET6
**server** socket-fd wiring (`sys_bind`/`sys_listen`/`sys_accept4` +
`net::socket::SockState::Listening`). Every other pre-5.7 gap is now closed:
non-blocking recv/connect/send, honest poll/epoll readiness, and — as of commits
cf1cba879/e99fb694f — **IPv6 connect end-to-end through the socket-fd layer**
(`sys_connect`/`getpeername` on `AF_INET6` → `NetstackConn::connect6`). So this
fork now blocks the netstack-migration thread's completion: with server sockets
unwired, flipping `net.userspace` by default would regress server programs
(`bind`/`listen`/`accept` would hit the stubbed path). Claude has paused the
netstack thread here rather than pick Option A autonomously, because the operator
explicitly logged this fork *and* the 5.7 flip itself is a user-visible,
costly-to-reverse policy that warrants operator sign-off.

**Background.** In the daemon, a session == one SHM ring (one `RingConns` table +
its listeners). `OP_ACCEPT` installs the newly-established connection into the
**listener's own session**, under a new conn_id on the *same* ring. So a listening
socket and every connection it accepts physically share one ring. Linux, by
contrast, gives every accepted fd a fully independent socket whose lifetime is
decoupled from the listener's.

**Question.** How should the socket-fd layer model accepted connections so their
lifetime/independence matches Linux, given the daemon co-locates them with the
listener?

**Options.**

- **A — Shared, refcounted session (no daemon-ABI change).** The listening
  `SocketInner` owns the session; each accepted socket is a new fd that holds an
  `Arc` on the same session and carries its own conn_id. Per-connection `close`
  sends `OP_CLOSE` for that conn_id; the session's `OP_STOP` fires only when the
  last reference (listener or any accepted socket) drops — so closing the listener
  no longer kills already-accepted connections (Linux-correct lifetime).
  - *Pros:* no daemon protocol change; reuses everything already built; smallest
    diff; matches the migration doc's "interim synchronous model, to be replaced by
    the async socket server" framing.
  - *Cons:* all connections under one listener funnel through **one ring guarded by
    one lock** — a *blocking* op on one accepted conn stalls every other conn on the
    same listener until its deadline. (Mitigated in practice: servers that use
    `accept`+`poll`+non-blocking I/O only serialize per round-trip, not per slow
    client. It is real for naively-blocking multi-client servers.)

- **B — Accept-into-a-fresh-ring (daemon-ABI change).** Extend accept so the kernel
  hands the daemon a *new* ring handle and the daemon migrates the established
  `TcpConn` out of the listener's session into a new single-connection session on
  that ring. Each accepted socket then owns its own ring exactly like a client
  socket.
  - *Pros:* true per-connection independence and concurrency (one slow client can't
    stall others); accepted sockets are structurally identical to client sockets.
  - *Cons:* new/extended accept ABI (SQE carries a ring handle; daemon must
    `OP_RING_TCP`-attach it and move connection state between session tables); more
    moving parts and a costlier-to-reverse protocol commitment.

**Claude's recommendation:** **Option A** for the interim. The whole per-op
synchronous socket path is explicitly a stepping stone to the async, always-on
socket server (see `known-issues.md` D-NETSOCK-SYNC and the migration doc), which
will replace the ring-per-op model wholesale — so paying for B's ABI complexity now,
only to rework it at the async cutover, is poor value. A fixes the Linux *lifetime*
semantics (the correctness-critical part) with zero protocol change; the
concurrency limitation is real but documented and temporary, and is a non-issue for
the poll-driven server pattern. If the operator wants genuine per-connection
concurrency before the async server lands, choose B.

**Where it bites:** `kernel/src/net/socket.rs` (`SockState`, `SocketInner`,
`SOCKET_TABLE`; a shared `Arc<Mutex<Session>>` for A vs. a per-socket ring for B),
`kernel/src/net/netstack_client.rs` (a `Session` abstraction hosting multiple
conn_ids vs. the current single-conn `NetstackConn`), `kernel/src/syscall/linux.rs`
(`sys_bind`/`sys_listen`/`sys_accept4` routing), and — for B only —
`services/netstack/src/main.rs` (accept-into-new-ring) + `netipc/src/ring.rs`
(accept SQE ring-handle field).

---

Earlier deferred operator decisions (Q1–Q22) have been
resolved — see the "Recently resolved" list below and `design-decisions.md` for
full rationale. New decisions should be appended above this line as `## Q24 …`.

---

Recently resolved (see `design-decisions.md` for the full rationale):

- Q22 netstack Phase 5 cutover — deletion scope + cutover strategy — resolved
  2026-07-14 (§66): **Q22a → Option C** (phased deletion — L2–L4 core first, app
  protocols re-homed to userspace individually) and **Q22b → (ii) staged**
  (persistent daemon + socket-forwarding behind a default-off boot switch; prove
  parity in QEMU, flip the default, then delete). Claude recommended both; operator
  approved both.

- The coreutils "which set is canonical?" question — resolved 2026-06-12;
  standalone per-tool crates are canonical (§8).
- Q1 `set_mempolicy_home_node` / NUMA mempolicy on UMA — resolved 2026-06-13,
  **operator-confirmed 2026-06-14**; keep the UMA no-op returning 0, option A
  (§10).
- Q2 `/proc/sys/vm/overcommit_memory` & memory-commit policy — resolved
  2026-06-13, **operator-confirmed 2026-06-14** (keep the shipped defaults:
  native strict/committed, Linux lazy/overcommit; both configurable); build the
  both-strategies model (Option 5); map the system-wide overcommit knob to a
  fine-grained native cap (`admin.memory_policy`), not `CAP_SYS_ADMIN` (§11).
- Q3 next major initiative — resolved 2026-06-13; terminal/dev before GUI,
  GCC/CMake/Make toolchain first, CPython then fastpy (§9).
- Q4 toolchain on Slate OS: run-prebuilt-Linux vs native-port — resolved
  2026-06-13; **Path Z** (run prebuilt Linux toolchain binaries on the Linux-ABI
  layer now, native-port selectively later), native-first/no-leak kept
  inviolate, clang green-lit for install (§12).
- Q5 file-backed `mmap` — how far to take the fix — resolved 2026-06-14
  (§22), then **REOPENED 2026-06-14** by the operator, then **RE-RESOLVED
  2026-06-14**: adopt **C-lite** (a unified *read-only* page cache for
  shared-library text dedup + de-double-caching), deferred until a concrete
  consumer appears (the dynamic linker is the likely first; stable VFS
  file-identity is the precursor); writable `MAP_SHARED` writeback stays declined
  / `ENOSYS` (§23). Deferral trigger logged in `todo.txt`.
- Q6 cross-process memory introspection — resolved 2026-06-14: keep
  channel/shared-memory IPC for *consensual* sharing; add a
  **debug-capability-gated** cross-address-space `process_vm_readv`/`writev`
  (`Rights::DEBUG` on a `Process` capability; `EPERM` without it). `ptrace`
  remains a deferred follow-up behind the same gate (§24).
- Q8 Path Z libc + rootfs — resolved 2026-06-14, **operator-delegated to
  Claude**: go straight to **glibc** on an **ext4** rootfs, no musl
  stepping-stone (§25). Claude reversed its own earlier musl-first recommendation
  per the operator's stated preference for hard-work-upfront over throwaway
  scaffolding, given the static-load path is already proven end-to-end.
- Q7 kernel-task-stack-vs-IRQ overflow (B-DF1) — resolved 2026-06-15,
  **operator-chosen option A** (Claude recommended A): per-CPU guard-page IRQ
  stack with a manual nesting-aware switch + deferred preemption, plus the
  `cli`/`sti` recursion guard the restructuring exposed (§26). Validated:
  `http_gzip_8KiB` no longer double-faults at the gzip→dashboard transition.
- Q9 bare-ELF ABI auto-classification — resolved 2026-06-24, **operator-chosen
  option D** (Claude recommended D): default unmarked bare ELF → Linux ABI, add
  `NT_GNU_ABI_TAG` note-walk as a positive Linux signal, stamp native binaries
  with an explicit SlateOS marker; `spawn_process_with_abi` override kept (§33).
- Q10 fullscreen-capture video codec — resolved 2026-06-24, **operator deferred
  to Claude's recommendation**: hardware encode via the GPU driver long-term
  (option C), defer the software-codec port near-term (option D), no stub
  encoder meanwhile; if a software path is ever needed first, AV1/`rav1e` over
  H.264 (§34).
- Q11 zero-copy page-flipping for large channel messages — resolved 2026-06-24,
  **operator-chosen option B** (Claude recommended B): explicit opt-in
  `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region; copy
  path stays the default. Compiler follow-up: keep it programmer/library-
  controlled (library-level auto-threshold helper), the compiler does not
  auto-insert the flag (§35).
- Q12 next large initiative — resolved 2026-06-24, **operator-chosen option E**:
  build the C-lite read-only page cache now; lifts the §23 "not now" hold (§36).
- Q13 de-double-cache file data — resolved 2026-06-30, **operator-chosen option A**
  (Claude recommended A): page-cache-primary — the page cache is the single cache
  for regular-file data, the buffer cache caches only filesystem metadata (§38).
- Q14 connect the two cgroup subsystems — resolved 2026-06-30, **operator-chosen
  option A** (Claude recommended A): cgroupfs as the frontend,
  `kernel/src/cgroup.rs` as the enforcement engine; fork/clone/spawn inherit
  `cgroup_id` (§39).
- Q15 next focus — resolved 2026-06-30, **operator-chosen option A then C/D**:
  execute Q13 + Q14 first, then a large initiative — C (GPU accel) or D (Docker /
  container-runtime port) in operator-indifferent order; this is the explicit
  go-ahead for the Docker port (§40).
- Q16 `container diff` baseline semantics — resolved 2026-07-01, **Claude
  autonomous (operator-approved Docker-port scope)**: implemented **option A**
  (overlay-only diff). See `design-decisions.md` §41.
- Q17 `container exec` semantics — resolved 2026-07-14, **operator-chosen
  option B** (Claude recommended B): keep the netns-debug `container exec` facade
  AND add real rootfs-binary exec under a distinct verb (`container run-in` /
  `exec --rootfs`); the `docker exec` delegate + `docker build` `RUN`/`HEALTHCHECK`
  route to the real path (§58).
- Q18 GPU acceleration scope — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended C): build the kernel-side virtio-gpu render-ioctl dispatch
  now with honest "no-3D" reporting (GETPARAM `3D_FEATURES=0`, no capsets, correct
  errno on 3D ioctls); defer the Mesa port until a virgl test environment exists
  (§59).
- Q19 container network model — resolved 2026-07-14, **operator-chosen option B**
  (Claude recommended B): generalise to N-interface multi-network membership
  (Docker parity) as its own dedicated increment (§60).
- Q20 hard-lockup (BSP-dead) detector — resolved 2026-07-14, **operator-chosen
  option A** (Claude recommended A): build the `i6300esb` watchdog + inject-nmi
  detector, opt-in behind the existing `boot-test.sh --hard-lockup-watchdog` flag
  (§61).
- Q21 `nft`/`iptables` compat tooling — resolved 2026-07-14, **operator-chosen
  option C** (Claude recommended C): keep `nft`/`iptables` as an explicit
  parser/pretty-printer only, fix the docs, steer users to `fw`; defer full/minimal
  kernel wiring (§62).
