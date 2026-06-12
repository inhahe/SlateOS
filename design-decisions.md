# Design Decisions Log

This file records **deliberate design decisions** made during development,
each with enough context to reconsider it later. It is distinct from the
broad spec (`design.txt`) and the original rationale notes
(`design desicions.txt`, `other design decisions.txt`): this file is a
running, dated log of decisions taken while implementing, especially ones
where a reasonable alternative exists and the operator might want to revisit.

Format for each entry:

- **Decided by** — who made the **final call**, *not* who first proposed the
  idea. Use `Operator` whenever the decision was put to the operator and the
  operator chose — **regardless of who suggested the chosen option.** Claude
  having proposed (or argued against) the option that was picked never moves the
  attribution to Claude; it stays `Operator`. Use `Claude (autonomous)` only when
  Claude resolved it without putting it to the operator (`Claude
  (operator-approved scope)` when the operator pre-approved the direction but
  Claude made the specific call). A parenthetical may record the collaboration —
  who proposed the option and whether Claude agreed — e.g. `Operator (Claude
  proposed this option)` or `Operator (Claude recommended otherwise; operator
  overruled)` — but that note never changes the attribution. An **Operator**
  decision is settled policy and should not be silently revisited; a **Claude**
  one is Claude's to revisit and the operator may want to overrule it.
- **Context** — what problem forced a choice.
- **Decision** — what was chosen.
- **Rationale** — why.
- **Alternatives considered** — and why they were rejected.
- **Where it lives** — files/symbols, so the decision can be located and reversed.
- **How to reverse** — what changing the decision would entail.

---

## 1. Linux ABI version to target — baseline 6.6, "baseline + honored extras"

**Date:** 2026-06-06 (policy) / 2026-06-10 (uname surface resolved)

**Decided by:** Operator (Claude proposed the 6.6 baseline floor and surfaced
the forward-compat question; the operator resolved the forward-compat policy —
option (ii) "baseline + honored extras" — on 2026-06-10, per `todo.txt`).

**Context:**
The Linux compatibility layer (`kernel/src/syscall/linux.rs`) translates the
Linux syscall ABI for Linux binaries running on OuRoS. Linux's ABI is a
moving target across kernel versions; we need a single, defensible answer to
"which Linux are we?" so that feature detection, version gates, and
sibling-syscall consistency are coherent rather than ad hoc.

**Decision:**
- **Baseline floor: Linux 6.6.** We implement the 6.6 syscall ABI as the
  guaranteed floor. `uname(2)` reports `sysname = "Linux"` and
  `release = "6.6.0-ouros"`.
- **Policy: "baseline + honored extras."**
  1. Everything in the 6.6 ABI is the floor.
  2. **Never accept-without-honoring:** if we accept a flag/syscall, we must
     actually implement its semantics. We never silently ignore a flag we
     advertised support for.
  3. Post-6.6 features are kept **only if fully implemented**; otherwise the
     syscall returns `ENOSYS`/`EINVAL` honestly so glibc/musl can fall back.
  4. **Sibling-consistency ("Frankenkernel" trap):** within a feature family,
     don't implement some members and silently no-op others. Either the whole
     family behaves consistently or the unimplemented members return a clear
     error that the caller's fallback path expects.
- **ABI page size = 4096.** Userspace sees `sysconf(_SC_PAGESIZE) == 4096`
  even though native kernel frames are 16 KiB. Any byte→page count reported
  across the Linux ABI boundary (`mmap`/`mprotect`/`msync`/`mremap`,
  `/proc/<pid>/statm`) uses 4096, never the native `FRAME_SIZE` (16384).

**Rationale:**
- 6.6 is an LTS kernel — stable, widely targeted, well-documented ABI.
- `sysname/release` are **Linux-ABI-only surfaces**: in our architecture,
  native code uses native APIs, so the *only* callers of `uname(2)` are Linux
  binaries that expect Linux values. Reporting `"Linux"` / `"6.6.x"` is the
  faithful answer for that personality, not a lie about what the OS is.
- The release string MUST satisfy glibc's startup version gate
  (`__libc_start_main` → "FATAL: kernel too old" if leading MAJOR.MINOR is
  below glibc's build-time minimum). glibc parses only the leading integer
  triple, so `"6.6.0-ouros"` reads as the 6.6.0 baseline.

**Retained post-6.6 features (fully implemented):**
- `F_DUPFD_QUERY` (6.10).
- futex2 family: `futex_wake`/`futex_wait` (6.7), `futex_waitv` (5.16).

**Documented gap (sibling-consistency example):**
- `futex2_requeue` returns `ENOSYS` — glibc falls back to the legacy
  `futex(FUTEX_CMP_REQUEUE)` path, so the gap is safe and honest.

**Alternatives considered:**
- *Pin to a single exact version with no extras* — rejected: needlessly drops
  cheap, fully-implemented post-6.6 conveniences that real binaries probe for.
- *Report "OuRoS"/"0.1.0-ouros" from uname* — rejected: breaks glibc's version
  gate and misleads Linux feature-detection that keys off the kernel version.

**Where it lives:**
- `kernel/src/syscall/linux.rs`: `sys_uname` (~line 31523), `const ABI_PAGE_SIZE: u64 = 4096` (~line 4534), byte→page reporting `bytes / 4096` (~line 4453).
- `roadmap-detailed.md` §72 "Version-surface policy".
- Self-test batch 526 pins `sysname == "Linux"` and `release` leading `"6.6"`.

**How to reverse:**
- Bumping the baseline: change the `release` string in `sys_uname` and update
  batch 526's assertion + §72. Audit feature gates that assume 6.6.
- Changing the extras policy: re-audit each retained post-6.6 feature against
  rule (3)/(4) before removing or adding.

---

## 2. argv/envp persistence for /proc — Approach A (persistent PCB snapshots)

**Date:** 2026-06-10

**Decided by:** Claude (autonomous) — an implementation choice among three
mechanisms, resolved while building `/proc/<pid>/cmdline` and `environ`.

**Context:**
`/proc/<pid>/cmdline` and `/proc/<pid>/environ` must report a process's argv
and environment for the whole lifetime of the process. But the PCB's
`initial_argv`/`initial_envp` are **one-shot**: they are drained by
`SYS_PROCESS_GET_ARGS` at child startup and then cleared
(`take_initial_args`). After startup there was no surviving copy to serve to
procfs.

**Decision — Approach A: keep a separate, persistent snapshot in the PCB.**
- Added `pub proc_argv: Vec<Vec<u8>>` and `pub proc_envp: Vec<Vec<u8>>` to
  `Process`.
- Populated in `set_initial_args` by cloning before the one-shot move into
  `initial_argv`/`initial_envp`.
- **Never drained** (distinct from the one-shot fields).
- **Inherited across `fork`** (cloned from parent), matching POSIX semantics
  where a child initially shares the parent's argv/env view.
- Read by procfs via `get_proc_argv(pid)` / `get_proc_envp(pid)`.

**Rationale:**
- Stores the data as **bytes** (`Vec<Vec<u8>>`), honoring the project rule
  that argv/env/paths are bytes, not UTF-8 strings.
- Cheap and simple: a clone at spawn (one-time) buys lifetime availability.
- Keeps the one-shot startup contract untouched, so no risk to the existing
  `SYS_PROCESS_GET_ARGS` fast path.

**Alternatives considered:**
- **Approach B — a "consumed" flag instead of clearing**: keep
  `initial_argv`/`initial_envp` populated but mark them consumed. Rejected:
  conflates two concerns (startup handoff vs. introspection) in one field and
  makes the drain semantics subtler; a future change to the startup path could
  silently break procfs.
- **Approach C — read argv/env back from the process's user stack on demand**:
  Linux-authentic (it reads `mm->arg_start..arg_end`). Rejected for now: needs
  safe cross-address-space reads, must tolerate a process that has overwritten
  its own argv (`setproctitle`), and is materially more code. Approach A's
  snapshot is "argv as captured at spawn," which is the common, predictable
  case. **If we later want `setproctitle` to be reflected, switch to C.**

**Where it lives:**
- `kernel/src/proc/pcb.rs`: fields (~line 308), spawn ctor (~line 949), fork
  destructure/clone/literal (~lines 1193/1238/1300), `set_initial_args`
  population (~line 3392), getters `get_proc_argv`/`get_proc_envp` (~line 3409).
- `kernel/src/fs/procfs.rs`: `gen_pid_cmdline`, `gen_pid_environ`.

**How to reverse:**
- To drop persistence: remove the two fields + getters and revert
  `gen_pid_cmdline` to the name-only form; delete `gen_pid_environ` and its
  `PID_FILES`/dispatch entries.
- To move to Approach C: replace the getters' bodies with user-stack reads and
  remove the snapshot fields once the stack reader is proven.

---

## 3. /proc/<pid> magic symlinks — cwd, root, and exe

**Date:** 2026-06-10 (cwd/root landed) / exe approved same day

**Decided by:** Claude (operator-approved scope) — the implementation design
(real procfs symlinks via the VFS `readlink` path, the `exe_path` PCB field,
fork/exec inheritance rules) was Claude's; the operator approved doing the
`exe` increment specifically.

**Context:**
Linux exposes magic symlinks in `/proc/<pid>/`: `cwd` (current working
directory), `root` (filesystem root), and `exe` (the executable image).
Tools (and some libc paths) read these. The VFS already supports symlink
resolution end-to-end (`lstat` → `EntryType::Symlink` → `readlink`), so procfs
just needs to participate.

**Decision:**
- Implement all three as real procfs symlinks via `FileSystem::readlink`.
  - **`cwd`** → the process's stored cwd (`Process::cwd`, already maintained).
  - **`root`** → always `"/"` — we have no per-process `chroot`/mount
    namespaces yet, so every process shares the global VFS root.
  - **`exe`** → the resolved absolute path of the loaded executable, captured
    at `exec` time (requires a new `exe_path` field on the PCB; see below).
- `readdir` lists them with `EntryType::Symlink`; `stat` reports `Symlink`;
  `read_file` on a link returns `InvalidArgument` (mirrors Linux `read()` →
  `EINVAL` on a symlink opened without `O_PATH`).
- **Bytes→String at the readlink boundary:** the VFS `readlink` API returns
  `String`, but paths are stored as bytes. A non-UTF-8 target surfaces as
  `InvalidArgument` rather than being lossily mangled — silent path corruption
  is never acceptable. (Canonical paths are ASCII/UTF-8 in practice, so this is
  a theoretical edge.)

**`exe` capture (the part that touches the exec path):**
- Add `pub exe_path: Vec<u8>` to `Process` (bytes, not String).
- **Inherited on `fork`** (clone), **overwritten on `exec`** (not inherited
  across exec — exec replaces the image).
- The exec/ELF-load path stores the canonicalised path of the binary into
  `exe_path` before entering userspace.

**Rationale:**
- `cwd`/`root` data already lives in the PCB (or is trivially `/`), so they
  were landed immediately as a low-risk, additive change.
- `exe` is the genuinely Linux-authentic completion; capturing the path at the
  one place that already resolves the binary (the loader) is the correct,
  non-hacky home for it.

**Alternatives considered:**
- *Resolve `self` as a real symlink too* — currently `/proc/self` is a
  transparent directory alias (resolved inline in `classify_path`), not a
  symlink. Left as-is; making it a symlink is cosmetic and out of scope.
- *Skip `exe` indefinitely* — rejected (operator approved the full increment);
  many tools rely on `/proc/self/exe`.

**Where it lives:**
- `kernel/src/fs/procfs.rs`: `PID_LINKS`, `ProcPath::PidLink`, `classify_path`,
  `readdir`/`read_file`/`stat`, `readlink`.
- `kernel/src/proc/pcb.rs`: `Process::cwd` (existing) + `exe_path` (new),
  fork inheritance, exec-time population.
- ELF loader / exec path: `exe_path` capture site.

**How to reverse:**
- Drop a link by removing it from `PID_LINKS` and its `readlink` arm.
- Drop `exe` capture by removing the `exe_path` field and its loader write;
  the link arm then returns `NotFound`.

---

## 4. /proc/<pid>/auxv — do NOT touch the native process-launch path

**Date:** 2026-06-12 (prompted by the operator)

**Decided by:** Operator (Claude proposed the build-auxv-for-all shortcut; the
operator caught that it would leak the Linux/SysV ABI into the native launch
path and set the rule that the auxv is a Linux-ABI-only construct).

**Context:**
Linux exposes `/proc/<pid>/auxv`: the **auxiliary vector**, a list of
`AT_*` key/value pairs (entry point, program-header address, page size,
`AT_RANDOM` seed, etc.) that the kernel writes onto the process's initial
**System V ABI stack** at `execve` time. glibc/musl startup
(`__libc_start_main`) and `getauxval(3)` read it. Implementing a *real*
`auxv` in procfs requires having an auxv to report — which on Linux means
the kernel built a System V initial stack during exec.

While planning this, the tempting shortcut was: "build a SysV auxv during
exec for **all** processes (native + Linux) and stash a copy for procfs."
The operator caught this and asked the right question: *the auxv is a
Linux/POSIX-ABI convention — does building it on the native launch path
leak Linux compatibility into the rest of the OS, which we decided against?*
It does. That shortcut is rejected.

**Decision — the auxv is a Linux-ABI-only construct; the native launch
path is never modified to produce one.**
- **Native processes have no auxv, by design.** OuRoS native processes do
  **not** receive a System V initial stack. They get argv/envp from the
  kernel via `SYS_PROCESS_GET_ARGS`, and `posix/src/crt.rs` synthesizes
  `argc/argv/envp` for `main()`. There is no `AT_*` vector anywhere in the
  native startup contract, and there must not be.
- **`/proc/<pid>/auxv` for a native process is honestly empty** — a single
  `AT_NULL` terminator (the same "honestly-empty-for-native" pattern used by
  `/proc/<pid>/fd` and `/proc/<pid>/fdinfo`, which are populated only for
  Linux-ABI processes that carry kernel-side `KernelFdTable` state).
- **A real, populated auxv appears only for Linux-ABI processes**, built by
  the (not-yet-existing) **Linux compat ELF loader** as part of constructing
  the System V initial stack for a Linux binary. The saved copy lives in
  **Linux-ABI PCB state** (next to `KernelFdTable`), never in fields shared
  with native processes.
- **The native exec path (`kernel/src/proc/spawn.rs::setup_user_stack` and
  friends) is not touched** to fabricate AT_RANDOM bytes, an entry-point
  AT_ENTRY, or any other AT_* value.

**Rationale:**
- This is the core Linuxulator-style isolation rule for OuRoS: Linux/SysV
  ABI constructs stay confined to Linux-ABI processes and the compat
  translation layer; they never bleed into native launch, native syscalls,
  or native startup. The auxv is exactly such a construct.
- Fabricating an auxv for native processes would be inventing data that the
  native ABI does not define and that nothing native consumes — both a
  Linux leak *and* a violation of the "never invent data in procfs" rule.
- It keeps the hot native launch path lean: no SysV stack layout, no AT_*
  marshalling, no extra copies on every spawn.

**Alternatives considered:**
- *Build a SysV auxv during exec for all processes and snapshot it for
  procfs* — **rejected** (this is the shortcut the operator flagged): leaks
  the Linux ABI into the native launch path and adds SysV-stack machinery to
  a path that has none and needs none.
- *Report a partial/fake auxv for native processes (e.g. just AT_PAGESZ /
  AT_RANDOM)* — rejected: fabricated procfs data; native processes
  genuinely have no auxv, so the honest answer is the bare `AT_NULL`.
- *Implement the full auxv now* — rejected/blocked: there is no Linux compat
  ELF loader yet (a Phase 5.1 feature), so there is no real auxv to serve.
  Tracked in `todo.txt`.

**Where it lives:**
- `kernel/src/syscall/linux.rs`: `PR_GET_AUXV` handler (`0x4155_5856`,
  ~line 7206) returns the 16-byte `AT_NULL` terminator; the comment above it
  (~lines 7189–7205) states the no-native-auxv rule.
- `posix/src/crt.rs`: native startup via `SYS_PROCESS_GET_ARGS` (no SysV
  stack, no auxv).
- `posix/src/sys_auxv.rs`, `posix/src/linux_binfmt_elf.rs`: inert scaffolding
  (AT_* constants/types only; no stack builder) awaiting the Linux compat
  loader.
- `todo.txt`: the `/proc/<pid>/auxv` block (architecture-correction note).

**How to reverse (i.e. when the Linux compat ELF loader lands):**
- Add the auxv builder **inside the Linux compat loader only**, as it lays
  out the System V initial stack for a Linux binary; stash the built auxv in
  Linux-ABI PCB state.
- Have procfs serve that saved copy for Linux-ABI processes, and continue to
  serve a bare `AT_NULL` for native processes.
- Do **not**, at any point, add auxv construction to
  `spawn.rs::setup_user_stack` or any other native launch code.

---

## 5. fork() copy-on-write — swap swapped-out parent pages back IN rather than refcount swap slots

**Date:** 2026-05-31 (predates this file; recorded retroactively 2026-06-12)

**Decided by:** Claude (autonomous) — an implementation choice made while
building the CoW fork path.

**Context:**
`fork()` clones the parent address space copy-on-write. A parent page that
has been **evicted to swap** at fork time poses a question: the child must
end up sharing (CoW) the same logical page, but the page currently lives in a
swap slot, not in RAM. Either the swap slot becomes shared between parent and
child (requiring the swap subsystem to refcount slots), or the page is brought
back to RAM before the CoW share happens.

**Decision — bring the page back in first.**
`clone_user_half` (the CoW fork path) detects a PTE holding a swap entry and
calls `swap::swap_in_page(parent_pml4, virt, swap_in_default_flags())` to
fault the page back into RAM before CoW-sharing it. `swap_in_default_flags()`
returns `PRESENT | WRITABLE | USER_ACCESSIBLE | NO_EXECUTE`, mirroring the
page-fault handler's swap-in path (`idt.rs`), which likewise does not track
per-page protection and restores pages as user RW+NX. The page is then
re-registered as reclaimable so it can be evicted again later.

**Rationale:**
- Avoids adding a swap-slot refcount table and the associated free/evict
  bookkeeping (a slot shared by N address spaces can only be released when the
  last sharer drops it — that's a whole refcount lifecycle to get right).
- Keeps swap slots single-owner, which keeps the swap subsystem simple and its
  invariants easy to reason about.

**Cons / cost accepted:**
- A fork of a process with swapped-out pages pays I/O to page them back in,
  even if neither parent nor child ever touches them again. With swap-slot
  sharing, an untouched shared page would never need to come back.
- Transient RAM pressure: the swapped-out working set is materialized at fork.

**Alternatives considered:**
- *Refcount swap slots and share them directly across the CoW boundary* —
  rejected for now: materially more code and a new lifecycle to maintain; the
  swap-in approach is correct and simpler. **If fork-of-large-swapped-process
  becomes a measured hot path, switch to slot refcounting** — `clone_user_half`
  can then share the slot instead of calling `swap_in_page`.

**Where it lives:**
- `kernel/src/mm/cow.rs`: `clone_user_half` swap-entry branch (~line 557),
  `swap_in_default_flags()` (~line 365).
- `kernel/src/mm/swap.rs`: `swap_in_page`, `register_reclaimable`.

**How to reverse:**
- Add a refcount field to the swap-slot table; in `clone_user_half`, bump the
  slot refcount and copy the swap PTE into the child instead of swapping in;
  make swap-slot free refcount-aware; teach the fault handler that a faulting
  swap page may be shared (CoW-on-swap-in).

---

## 6. fork() / dup() file-descriptor inheritance — refcounted shared open-file descriptions

**Date:** 2026-05-31 (fork) / 2026-06-01 (dup fix); recorded retroactively 2026-06-12

**Decided by:** Claude (autonomous) — an implementation choice (and POSIX
correctness fix) made while building fork fd inheritance.

**Context:**
On `fork()`, the child's userspace libc fd table is CoW-copied, so it
references the **same kernel handle ids** as the parent — the kernel cannot
rewrite that userspace table. POSIX also requires that a forked child (and a
`dup()`/`dup2()`/`F_DUPFD` descriptor) **share one open file description**:
same file offset, same status flags. The kernel's `fs::handle` originally did
*not* refcount `OpenFile` — each id was a distinct entry and `handle::dup`
allocated a **new** id with an **independent cursor** (that is `dup()`-of-a-
*new-description* semantics, which is wrong for both fork sharing and POSIX
`dup`).

**Decision — refcount the open-file description and share ids.**
- Added a refcount to `OpenFile` plus `fs::handle::dup_shared(id)` (bump
  refcount, return the **same** id) and a refcount-aware `close` (the
  underlying description is released only when the last referencing fd closes).
- **fork** bumps refcounts on the existing ids rather than allocating new ones,
  matching pipes/sockets/eventfd which already did same-id refcounted dup.
- **dup()/dup2()/F_DUPFD** for `HandleKind::File` no longer call `SYS_FS_DUP`;
  the userspace `posix` crate shares the source fd's kernel handle id at the
  fd-table level via `alloc_fd_with_flags`, exactly like Pipe/Console/socket
  kinds. `close()` gates `SYS_FS_CLOSE` behind `is_handle_referenced()`.
- The old kernel `handle::dup` (independent cursor) is **left unchanged** and
  still used by `spawn.rs` fd inheritance, where a genuinely separate
  description is wanted.

**Rationale:**
- This is the only model that yields correct POSIX shared-offset semantics
  given that the kernel can't rewrite the child's userspace fd table — both
  ends *must* point at one refcounted description.
- Folding File into the same shared-id path the other handle kinds already use
  removes a special case and a latent dup() correctness bug in one stroke.

**Alternatives considered:**
- *Allocate fresh handle ids for the child on fork* — rejected: impossible to
  apply correctly (the kernel can't edit the CoW-copied userspace fd table) and
  semantically wrong (would give the child an independent offset).
- *Keep `handle::dup`'s independent-cursor behavior for dup()/dup2()* —
  rejected: that is a pre-existing POSIX bug (dup'd fds must share the
  description); fixed by routing File dup through fd-table id sharing.

**Where it lives:**
- `kernel/src/fs/handle.rs`: `OpenFile` refcount, `dup_shared`, refcount-aware
  `close`, `is_handle_referenced`.
- `posix/src/file.rs` (dup/dup2), `posix/src/fcntl_ops.rs::dup_fd_from`
  (F_DUPFD): File shares the id via `alloc_fd_with_flags`.
- `kernel/src/proc/fork.rs`: fd inheritance bumps refcounts.
- `posix` `fdtable.rs` module doc: documents the shared-id model.

**How to reverse:**
- Revert dup/dup2/F_DUPFD to `SYS_FS_DUP` + restore independent-cursor
  `handle::dup` for File (reintroduces the POSIX dup bug — not advisable).
- Drop the `OpenFile` refcount and have fork allocate new ids (breaks shared
  offset — not advisable). This decision is effectively load-bearing for POSIX
  correctness; reversal is only sensible if the fd model is redesigned.

---

## 7. waitpid(pid <= 0) — collapse all "any child" cases; single any-child waiter slot; reaped-pid via arg1

**Date:** 2026-05-31 (predates this file; recorded retroactively 2026-06-12)

**Decided by:** Claude (autonomous) — implementation choices made while
building the wait-for-any-child path.

**Context:**
POSIX `waitpid` distinguishes four pid forms: `> 0` (that specific child),
`== 0` (any child in the **caller's process group**), `< -1` (any child in
process group `|pid|`), and `== -1` (any child whatsoever). The first
"wait for any child" implementation had to choose how faithfully to model the
process-group cases when there is **no process-group subsystem yet**, how to
register the any-child waiter, and how to return the reaped pid to userspace
without breaking the existing specific-pid syscall ABI.

**Decision (three sub-decisions):**
- **(a) Collapse all `pid <= 0` to "any child."** With no process groups,
  `pid == 0` and `pid < -1` are treated identically to `pid == -1`. Correct for
  the common case (shells/make use `-1`) and for any single-process-group
  workload.
- **(b) One any-child waiter slot on the parent PCB** (`Process::wait_any_task`),
  unlike the specific-pid waiter which lives on the *child* PCB. If two threads
  of the same process both call `waitpid(-1)` concurrently, the second
  registration clobbers the first; only one thread reliably gets the child-exit
  wake (the other relies on its own `try_reap_any` at block entry or a later
  wake). The clobber is **safe**: `clear_wait_any_task` only clears the slot if
  it still holds the caller's own `TaskId`, so a thread never clears another's
  registration.
- **(c) Reaped pid returned via the `arg1` pointer.** The any-child path writes
  the reaped child's pid as an `i32` to the user `arg1` slot (posix `waitpid`
  passes a real `&mut` via `syscall2`), while `rax` still carries the exit code.
  The kernel writes `arg1` **only** in the any-child branch — specific-pid
  callers (init/services using `syscall1`) leave a stale pointer in `rsi/arg1`,
  so writing it for them would corrupt memory.

**Rationale:**
- Ships a working `wait(-1)` (the form shells and `make` actually use) without
  blocking on a process-group subsystem that isn't needed yet.
- The single-slot waiter matches typical single-threaded-waiter usage and
  avoids a per-process waiter list before any caller needs one.
- The `arg1` ABI extends wait without breaking the established specific-pid
  calling convention.

**Cons / cost accepted:**
- `pid == 0` / `pid < -1` do **not** filter by process group — once process
  groups land, a caller that means "my group only" would over-match. Acceptable
  while there is exactly one group.
- Concurrent multi-thread `waitpid(-1)` in one process is not fully reliable
  (only one waiter is registered at a time).

**Alternatives considered:**
- *Implement process-group filtering now* — rejected: requires a process-group
  subsystem that doesn't exist; premature.
- *A list/set of any-child waiters woken together* — deferred: no current
  caller does concurrent multi-thread `wait(-1)`; the proper fix when one
  appears is to make `wait_any_task` a small `TaskId` list and wake all.
- *Return the reaped pid in `rax` and the exit code elsewhere* — rejected:
  would break the existing specific-pid ABI (`rax` = exit code) that init and
  services already depend on.

**Where it lives:**
- `kernel/src/syscall/handlers.rs`: `sys_process_wait` / `sys_process_try_wait`
  (`pid_arg <= 0` branch ~line 3162+), `write_reaped_pid` (~line 3147),
  `set_wait_any_task` / `clear_wait_any_task` usage (~line 3238+).
- `kernel/src/proc/pcb.rs`: `Process::wait_any_task`, `set_/clear_wait_any_task`.
- `posix/src/process.rs::waitpid`: passes the `&mut` reaped-pid slot.

**How to reverse:**
- **(a)** When process groups land, subdivide the `pid_arg <= 0` branch on the
  exact value (`0` → caller's group, `< -1` → group `|pid|`, `-1` → any).
- **(b)** Replace `wait_any_task` with a `TaskId` list/set and wake all
  registered waiters on child exit.
- **(c)** Only revisit if the wait ABI is redesigned; the split (exit code in
  `rax`, pid in `arg1`) is intentional and back-compatible.
