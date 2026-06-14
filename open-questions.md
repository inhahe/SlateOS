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

Two substantive design discussions remain open below: **Q5 (reopened)** — how
far to take native file mapping / the page cache; and **Q6** — the authorization
model for cross-process memory introspection. The operator has indicated a
direction on Q6 (capability model) but raised a refinement worth settling before
implementation (see Q6).

---

Recently resolved (see `design-decisions.md` for the full rationale):

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
  (§22), then **REOPENED 2026-06-14** by the operator with a sharper framing
  (native file mapping as a first-class feature; page cache as API-agnostic
  infrastructure; FS-cache-vs-mmap; databases). §22 still stands until this
  reconsideration settles — see **Q5 (reopened)** below.

---

## Q5 (reopened) — Native file mapping & the unified page cache: how far to take it? — OPEN (reopened 2026-06-14)

**Background.** §22 declined the unified page cache (option C) on the grounds
that it is a lot of work whose only payoff is memory savings for *some Linux
programs*, and Slate is not primarily a Linux box. The operator has reopened
this with a sharper observation: **mmap'ing files may be valuable for Slate
natively, not just as Linux compatibility.** That reframes the whole question.

**Key insight — the page cache is API-agnostic infrastructure, not a Linux
tax.** A page cache is a mechanism: map a file's contents into physical frames
*once*, and let multiple mappings (and the FS read/write path) share those
frames. Linux `mmap` is just one thin projection onto that engine. §22's
cost/benefit was framed as "is the page cache worth it *for Linux*?" — but if we
want native file mapping at all, we build the engine regardless and Linux mmap
becomes nearly free on top of it. The native facility need **not** match Linux
semantics: build the best native mapping facility (capability handles, explicit
durability/consistency semantics) and project Linux `mmap` onto a subset — the
same native-underneath / Linux-projection pattern we use for channel IPC (→
pipes/sockets) and committed memory (→ overcommit mode).

**Key insight — a strong FS cache shrinks mmap's irreducible value to a small
core.** The operator is right that a good FS cache covers most of what mmap is
used for. What it does *not* cover, and mmap does:
  1. **Cross-process read-only page sharing** (the killer feature): shared-library
     text mapped by N processes lives in RAM *once*. A read()-cache can dedup the
     *cache* copy, but each process still needs its own resident copy unless it
     maps. Purely a read-only benefit.
  2. **Zero-copy large-file random access** — scattered offsets without a read()
     syscall per access.
  3. **Convenience** — pointer-chasing a file as memory.

**Key insight — databases do NOT want mmap.** The operator's hunch ("maybe
databases work optimally with just a good FS cache") matches industry
consensus. CIDR 2022 *"Are You Sure You Want to Use MMAP in Your DBMS?"*
(Crotty/Leis/Pavlo) argues against mmap for databases: no control over eviction
order, unthrottleable page-fault stalls, no write-ordering for crash
consistency, brutal TLB shootdowns under concurrency. Real systems agree —
MongoDB replaced MMAPv1 with WiredTiger's explicit buffer pool; PostgreSQL has
always used its own buffer pool. The high-performance answer is **buffer-pool +
io_uring async I/O**, which Slate is already building toward. So a serious DB on
Slate should use our cache + io_uring and would *not* want mmap.

**Options (revised).**

- **A — revert to no file mapping.** (Original §22 framing's floor.) Rejected
  already; we shipped B.
- **B — demand-paged `MAP_PRIVATE` only (status quo / §22).** Per-mapping private
  frames, no sharing, no writeback. Pros: shipped, simple. Cons: no cross-process
  page sharing (shared-library text duplicated per process), double-caching
  (mmap'd file pages distinct from FS-cache pages).
- **C-lite — unified *read-only* page cache.** Map file contents into shared
  frames; multiple mappings + the FS read/write path share them. Gives
  cross-process read-only page sharing (the shared-library win) **and**
  de-double-caching, but **omits** writable `MAP_SHARED` writeback — the
  dirty-tracking / msync / write-ordering machinery that is the genuinely hard,
  hard-to-reverse part. `MAP_SHARED` writable stays `ENOSYS`. Pros: captures the
  strong native benefit at a fraction of full-C cost; aligns with "DBs don't want
  writable mmap anyway." Cons: still real work (cache keyed by stable file
  identity — needs `FileMeta.ino` ≠ 0 for memfs/FAT first); CoW interaction with
  the existing private-mapping path.
- **C — full unified page cache** with writable `MAP_SHARED` + writeback. Pros:
  full Linux mmap semantics. Cons: the writeback/dirty/ordering machinery is the
  expensive, hard-to-reverse part — and the database analysis says nobody serious
  should want writable shared mappings anyway.

**Claude's recommendation.** Don't build it yet, but don't foreclose it. Defer
until a concrete consumer exists (the dynamic linker wanting shared-library text
dedup is the likely first). When one appears, build **C-lite**, not a revert to A
nor a jump to full C. §22's "decline C" should be read as declining *full* C; it
should not bar C-lite. Meanwhile B stays shipped and correct.

**Where it bites.** `sys_mmap` / file-backed VMA path in
`kernel/src/syscall/linux.rs`; `fs/cache.rs` (block buffer cache — the
double-cache candidate to unify with); `FileMeta.ino` (currently 0 for
memfs/FAT — the stable-file-identity precursor C-lite needs); VMA model
`kernel/src/mm/vma.rs` (`VmaKind::FileBacked`). design-decisions.md §22 to be
amended when the operator settles this.

**Status.** OPEN (reopened). Non-blocking — no consumer exists yet, so the loop
continues on other work.

---

## Q6 — Cross-process memory introspection (`process_vm_readv`/`writev`, `ptrace`): permit it at all, and behind what gate? — OPEN (2026-06-14)

**Background.** `process_vm_readv(2)` / `process_vm_writev(2)`
(`process_vm_impl` in `kernel/src/syscall/linux.rs`) currently implement the
**same-address-space** transfer (the target thread shares the caller's PCB) but
return **`-ESRCH`** for any *cross-process* target — explicitly documented in the
code as "Cross-AS not implemented." Likewise `sys_ptrace` returns **`-EPERM`
unconditionally** (no tracer may ever attach). So today the kernel permits **no
cross-process memory introspection of any kind** — a coherent, deliberate
security posture.

The 2026-06-14 zero-copy work added the missing *mechanism*: pml4-parameterized
`copy_from_user_as` / `copy_to_user_as` (`kernel/src/mm/user.rs`) can read/write
an arbitrary address space's user pages through the HHDM. Wiring the cross-process
data path in `process_vm_impl` is now mechanically straightforward (resolve the
target's pml4 via `pcb::get_pml4`, route the *remote* side of each copy through
the `_as` primitive while the *local* side stays on the current CR3). **The only
thing missing is the authorization model** — and that is a genuine design fork,
not something to default my way through.

**Question.** Should cross-process `process_vm_readv`/`writev` (and, relatedly,
real `ptrace` attach) be allowed at all — and if so, gated by what?

**Options.**

- **A. Keep the status quo: no cross-process introspection (`ESRCH`/`EPERM`).**
  - *Pros:* maximally safe; consistent with the current posture; nothing to
    design; gdb/strace/CRIU simply can't peer into *other* processes (they still
    work on the same-AS / self case).
  - *Cons:* real debuggers (`gdb attach`, `strace -p`, `lldb`), checkpoint/restore
    (CRIU), and some profilers genuinely need cross-process reads; they'll fail.

- **B. Allow it, gated by a capability the caller must hold over the target.**
  Consistent with the design spec's "capability-based security from day one, no
  ambient authority": cross-process memory access requires the caller to hold an
  unforgeable handle/capability to the target process (e.g. a `ProcessCap` with a
  DEBUG/INTROSPECT right), not merely to know its PID. A debugger would obtain
  that capability through an explicit grant (parent→child, or a privileged broker).
  - *Pros:* aligns with the microkernel capability model; far stronger than
    Linux's PID-plus-yama check; auditable; no ambient authority.
  - *Cons:* requires designing the process-capability + right (does one exist
    yet?), a grant path, and plumbing it through `process_vm_impl` and `ptrace`;
    debuggers must be taught to acquire the capability (not a drop-in Linux ABI).

- **C. Allow it, gated by a Linux-style `ptrace_may_access` (same-uid / CAP_SYS_PTRACE / yama).**
  - *Pros:* drop-in compatible with how Linux debuggers expect to work; familiar.
  - *Cons:* "ambient authority by PID + uid" is exactly what the design spec says
    to avoid; requires a real uid/cred model and a yama-scope policy knob; weaker
    than B.

**Claude's recommendation.** Defer — this is a security-policy fork the operator
should own. If forced to pick a default I'd lean **A (keep ESRCH/EPERM)** until
there's a concrete consumer, because it's safe and the mechanism can be wired in
a day once the gate is decided; and **B** as the eventual target since it matches
the capability-based design. I am **not** opening cross-process access
autonomously — silently granting any process the ability to read/write any other
process's memory would be a serious regression and contradicts the core design.

**Operator direction (2026-06-14) — capability model (B), with a clarifying
distinction Claude proposes.** The operator endorsed the capability model (B)
but raised a real question: should a *debug* capability be the only thing that
governs cross-process memory access, given that cross-process memory *sharing*
is generally useful for IPC? The operator floated (a) processes A and B
negotiating through the OS over which spans to share, with what r/w rights and
which sync/consistency primitives, or (b) authorizing a limited shared span via
another IPC mechanism — while a *debug* capability would go further and read a
whole process's memory without any negotiation.

**Claude's synthesis (proposed resolution — please confirm).** These are two
*different* mechanisms and should stay separate; conflating them is exactly the
trap to avoid:

1. **Consensual / cooperative memory sharing** (the operator's negotiation idea)
   is **not** `process_vm_readv`/`ptrace` territory at all — it is ordinary IPC,
   and Slate OS **already has the right primitives for it**: the channel IPC
   (capability transfer) and the shared-memory object (`kernel/src/ipc/`). Two
   cooperating processes that *both consent* establish a shared mapping by one
   creating a shared-memory object and handing the other an unforgeable handle
   over a channel, with the rights (R / R+W) encoded in the handle; concurrency
   is then the participants' responsibility via futexes/atomics. This is the
   "A and B negotiate what to share" path, done the capability way, with **no
   need to widen `process_vm_readv` at all**. If anything is missing here it's a
   thin convenience wrapper, not a new authority.
2. **Unilateral introspection** — reading/writing a target's *entire* address
   space **without the target's cooperation** — is what `process_vm_readv`/
   `writev` and `ptrace` are for (debuggers, CRIU, profilers). *This* is the one
   that must be gated by a **debug/introspect capability the caller holds over
   the specific target** (option B), obtained by explicit grant (parent→child,
   or a privileged debugger broker) — never by ambient PID/uid authority.

So the answer to "should the debug capability be the *only* thing governing
cross-process access?" is: **yes for the non-consensual introspection path**
(that path *is* the debug capability and nothing else unlocks it), but
**consensual sharing is a separate, already-supported path** that never touches
this code — so there's no redundancy and no need for a second introspection
capability. A debugger uses the debug cap; cooperating IPC peers use
shared-memory handles. The two never overlap.

**Focused remaining question for the operator:** does this two-mechanism split
match your intent — (1) consensual sharing stays on the existing channel +
shared-memory IPC (no change to `process_vm_*`), and (2) `process_vm_readv`/
`writev` + `ptrace` are unlocked *only* by an explicit per-target debug
capability? If yes, Q6 is resolved as **B scoped to introspection only**, and I
can record it and wire it when a debugger consumer actually exists (no concrete
consumer today, so this is not blocking anything).

**Where it bites.** `kernel/src/syscall/linux.rs` (`process_vm_impl` — the
`if !same_addr_space { return ESRCH }` arm; `sys_ptrace`); `kernel/src/mm/user.rs`
(`copy_from_user_as`/`copy_to_user_as` — the mechanism, already built);
`kernel/src/proc/pcb.rs` (`get_pml4`); the process-capability + debug right that
choice **B** introduces (`kernel/src/cap/`); and, for the consensual path, the
existing shared-memory + channel IPC (`kernel/src/ipc/`).

**Status:** OPEN (narrowed) — operator endorsed the capability model (B); awaiting
confirmation of the consensual-sharing-vs-debug-capability split above before
recording. No concrete consumer exists yet, so nothing is blocked meanwhile.
