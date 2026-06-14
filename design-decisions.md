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
Linux syscall ABI for Linux binaries running on SlateOS. Linux's ABI is a
moving target across kernel versions; we need a single, defensible answer to
"which Linux are we?" so that feature detection, version gates, and
sibling-syscall consistency are coherent rather than ad hoc.

**Decision:**
- **Baseline floor: Linux 6.6.** We implement the 6.6 syscall ABI as the
  guaranteed floor. `uname(2)` reports `sysname = "Linux"` and
  `release = "6.6.0-slateos"`.
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
  triple, so `"6.6.0-slateos"` reads as the 6.6.0 baseline.

**Retained post-6.6 features (fully implemented):**
- `F_DUPFD_QUERY` (6.10).
- futex2 family: `futex_wake`/`futex_wait` (6.7), `futex_waitv` (5.16).

**Documented gap (sibling-consistency example):**
- `futex2_requeue` returns `ENOSYS` — glibc falls back to the legacy
  `futex(FUTEX_CMP_REQUEUE)` path, so the gap is safe and honest.

**Alternatives considered:**
- *Pin to a single exact version with no extras* — rejected: needlessly drops
  cheap, fully-implemented post-6.6 conveniences that real binaries probe for.
- *Report "SlateOS"/"0.1.0-slateos" from uname* — rejected: breaks glibc's version
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
- **Native processes have no auxv, by design.** SlateOS native processes do
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
- This is the core Linuxulator-style isolation rule for SlateOS: Linux/SysV
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

---

## 8. coreutils — standalone per-tool crates are canonical (retire the multi-call bundle)

**Date:** 2026-06-12

**Decided by:** Operator (Claude recommended option (a) — standalone per-tool
crates + a shared library — in `coreutils-canonical-answer.md`; the operator
agreed and chose it).

**Context:**
There was duplication between the `coreutils` crate's bundled binaries
(`coreutils/src/bin/{tr,dd,chown,df,…}`, a busybox-style multi-call binary that
dispatches on `argv[0]`) and the standalone per-tool workspace crates
(`userspace/{tr,dd,chown,df,…}`, one crate → one binary). Both implement the
same tools and were drifting. We needed a single canonical set for the OS image.
The operator asked for an analysis weighed purely on **design quality**, with
implementation effort and disk/network footprint explicitly excluded.

**Decision:**
- **Standalone per-tool crates (`userspace/<tool>/`) are canonical.** One tool =
  one crate = one binary = one identity.
- **Extract shared logic into a `coreutils-common` library crate** that every
  standalone tool depends on (arg parsing, usage/error formatting, exit-code
  conventions, I/O helpers) — code reuse without a multi-call binary.
- **Retire the `coreutils/src/bin/*` multi-call bundle.** Its useful content is
  the shared logic, which moves into the library crate; the multi-call *binary*
  role does not ship.
- Repoint the OS image build / kernel-embedding wiring (currently targeting
  `coreutils/target`) at the standalone crates.

**Rationale:**
The deciding factor is **capability-based least privilege**, a core
non-negotiable principle of SlateOS ("capability-based security from day one, no
ambient authority"). A multi-call binary has **one on-disk identity** for every
tool, so the kernel must grant it the **union** of every bundled tool's
capabilities — `cat` would carry the same authority as `ifconfig`. That is
exactly the ambient authority the OS exists to abolish. Per-tool binaries get
per-tool capability grants (`ping` gets raw-socket, `cat` gets nothing).
Supporting axes all agree: smaller per-tool TCB / dependency closure, smaller
fault blast radius at the artifact level, natural granularity for the
content-addressed package store + generations (a one-tool fix doesn't
invalidate the whole bundle's hash), and a legible security/process UI (distinct
names + distinct capability sets). The bundle's only non-size advantage — shared
code — is fully recovered by a shared **library** crate rather than a shared
**binary**, which is the `uutils/coreutils` structure.

**Alternatives considered:**
- **(b) `coreutils` multi-call bundle is canonical** — rejected: a single binary
  cannot express per-tool least privilege (the decisive point), gives every tool
  the largest possible TCB and capability set, and is a single coarse unit for
  the package store. Its on-disk-size win is exactly the concern excluded from
  the decision.
- **(c) Keep both, one generated from the other** — rejected: not a design
  position; it is the drift-generating status quo.

**Where it lives:**
- `coreutils/src/bin/*` (bundled binaries — to be retired; shared logic migrates
  to a new `coreutils-common` library crate).
- `userspace/<tool>/` (standalone crates — canonical; to depend on
  `coreutils-common`).
- The OS image build / kernel-embedding wiring that currently targets
  `coreutils/target` (to be repointed at the standalone crates).
- `coreutils-canonical-answer.md` (the full analysis).
- `todo.txt`: the "DUPLICATION between the coreutils crate's bins …" judgment
  call (2026-05-31) and the "USE STD" audit note.

**How to reverse:**
- Reintroducing a multi-call binary would require giving up per-tool capability
  grants (or building per-`argv[0]` identity into the kernel's capability check,
  which effectively reinvents separate binaries). Only revisit if the capability
  model itself changes.

---

## 9. Next major initiative ordering — terminal/dev toolchain before GUI; CPython then fastpy (fastpy depends on CPython)

**Date:** 2026-06-13 (corrected same day — see CPython-dependency note)

**Decided by:** Operator (Claude surveyed the roadmap, found bounded work
exhausted, and put the strategic ordering to the operator as `open-questions.md`
Q3 with options A–E and a recommendation of "bash first"; the operator chose a
different ordering — toolchain before bash, terminal/dev before GUI, and Python
via fastpy. The operator subsequently corrected a factual error in Claude's
write-up: fastpy is **not** an alternative to CPython but **depends on** it, so
the ordering is CPython *then* fastpy, not "fastpy instead of CPython").

**Context:**
An autonomous-loop survey (2026-06-13) confirmed every readily-actionable
roadmap surface was already mature (procfs/`/proc/sys`, sysfs, sysctlfs, the
full Linux syscall table, the POSIX layer, the container runtime, the ALSA
shim, the DRM/KMS shim). The only remaining roadmap work is large, multi-day
*ports*, each a costly and hard-to-reverse commitment with no obviously-correct
ordering — so the direction was put to the operator rather than picked
autonomously. The candidates were: (A) bash, (B) GCC/CMake/Make toolchain +
CPython, (C) GPU drivers → Mesa → Vulkan/OpenGL, (D) WINE, (E) Chromium.

**Decision:**
- **Terminal / developer environment comes before the GUI stack.** Build out a
  usable command-line dev environment first; defer the GPU/Mesa/compositor app
  vision (options C/D/E) until that's in place.
- **Port the GCC/CMake/Make toolchain (roadmap task 5031) before bash (task
  1491).** The toolchain is the prioritized next initiative.
- **Port CPython (task 5033) *first*, then integrate fastpy (tasks 24 + 5034) on
  top of it.** fastpy is the preferred *fast* execution path for SlateOS userspace
  Python (it AOT-compiles Python to native code and is many times faster than
  CPython, and is maintained to be CPython-3.14-compatible). **But fastpy is not
  a standalone replacement for CPython — it depends on the CPython runtime/DLL as
  a bridge** for a set of operations it does not implement natively, most notably
  **importing binary/compiled Python extension modules** (the C-API extension
  ecosystem). So CPython must be ported *before* fastpy can run, and CPython
  stays resident as fastpy's bridge — it is a **prerequisite and a runtime
  dependency**, not an alternative we skip. **Status check:** neither is ported
  yet — task 5033 (CPython) is `[ ]`, and tasks 24 & 5034 (fastpy) are `[ ]`
  (unstarted) in `roadmap.md`.

**Rationale:**
- A working dev toolchain is the foundation for self-hosting and for building
  everything downstream; it rides the already-mature POSIX layer and has **no
  GPU dependency**, making it the least-blocked big initiative. Doing it before
  the GUI is the intuitive ordering (you build the tools before the storefront).
- fastpy gives CPython-3.14 compatibility at much higher performance, and the
  project's own guidance already prefers "Python via fastpy" for userspace
  components (CLAUDE.md). But because fastpy bridges to the CPython runtime/DLL
  for binary-extension imports and other unimplemented operations, CPython is a
  hard prerequisite — porting CPython is not optional work we can defer in favor
  of fastpy; it is step one, with fastpy layered on top as the fast path.

**Honest nuance recorded at decision time (toolchain ↔ shell bootstrap
co-dependency):** the operator's reasoning was "porting bash will be easier once
the toolchain exists." The dependency is *mostly* the other way around —
GCC/Make are built and driven *by* a shell (`configure` scripts, recipe command
lines invoke `/bin/sh`). In practice neither strictly blocks the other here
because SlateOS already has a kernel shell (`kshell`) and a coreutils set, and the
toolchain itself is **cross-built on the dev host**, not self-hosted on SlateOS
initially — so we don't need bash-on-SlateOS to *produce* the toolchain binaries.
The conclusion (toolchain first) stands; the ordering is fine because the
host-side cross-build sidesteps the circular dependency. A full `make` driving
`configure` scripts *on SlateOS* will eventually want a real `/bin/sh`, at which
point bash (or a smaller POSIX sh) becomes the natural follow-on.

**Alternatives considered:**
- **(A) bash first** — Claude's original recommendation (least-blocked, highly
  decomposable, high leverage). Not chosen: the operator preferred the toolchain
  first; the bootstrap nuance above shows bash-first isn't *required* for the
  toolchain, so toolchain-first is a valid ordering.
- **(C/D/E) GPU/Mesa, WINE, Chromium** — deferred: these are the GUI/app long
  pole, the most hardware-dependent, and (D)/(E) are gated behind the GPU/Mesa
  work. The operator explicitly wants terminal/dev before GUI.
- **"fastpy instead of CPython" (skip the CPython port)** — Claude's original
  write-up framed it this way; **corrected and rejected** by the operator:
  fastpy depends on the CPython runtime/DLL (binary-extension imports etc.), so
  CPython can't be skipped. The relationship is CPython-then-fastpy, with CPython
  remaining resident as the bridge.

**Where it lives:**
- `roadmap.md`: task 5031 (gcc/cmake/make/pkg-config — next), task 5033 (CPython
  — prerequisite for fastpy, port first), tasks 24 & 5034 (fastpy integration &
  compiler — layered on CPython), task 1491 (bash — follow-on after toolchain).
- New top-level work; entry points emerge as the toolchain port begins
  (build wiring under the workspace + `pkg/`/`userspace/` as needed).

**How to reverse:**
- Re-prioritize by reordering the roadmap tasks. The CPython→fastpy dependency is
  not a preference but a technical fact (fastpy's bridge), so it can't be
  reordered unless fastpy gains a native binary-extension loader that removes the
  CPython dependency. If GUI work becomes more urgent than dev tooling, start
  option (C) instead — but per this decision, terminal/dev leads.

---

## 10. set_mempolicy_home_node / NUMA mempolicy on UMA — keep the UMA no-op returning 0 (option A)

**Date:** 2026-06-13

**Decided by:** Operator (this was `open-questions.md` Q1; Claude recommended
option A and laid out the UMA/NUMA/VMA tradeoff; the operator chose A).

**Context:**
SlateOS is a single-node **UMA** system (all CPUs reach all RAM at equal latency —
the desktop hardware we target). Linux's NUMA mempolicy family
(`mbind`/`set_mempolicy`/`set_mempolicy_home_node`) lets a program request that
specific regions of its address space be backed by specific NUMA *nodes*. On UMA
there is exactly one node, so any such policy is functionally a no-op. The
question was what `set_mempolicy_home_node` should return on a valid non-empty
range when we keep `mbind`/`set_mempolicy` as no-ops:
- **(A)** return 0 (success) *(current)*,
- **(B)** return `-ENOENT` (Linux's literal answer for a default-policy range),
- **(C)** implement real per-VMA mempolicy storage so the errno can be
  discriminated faithfully (per-VMA policy objects, `mbind_range`, `mpol_dup` on
  fork — substantial machinery for zero functional effect on UMA).

**Decision — option A: keep the UMA no-op and return 0.**
`set_mempolicy_home_node` on a valid non-empty range returns 0;
`mbind`/`set_mempolicy` continue to accept-and-drop the policy. No per-VMA
policy storage is built.

**Rationale:**
- **Negligible stakes on UMA.** Only programs that call `set_mempolicy_home_node`
  (a NUMA-tuning syscall, Linux 5.17+) are affected — server software tuned for
  multi-socket boxes plus `numactl`/`libnuma`. That's **<0.1% of programs and
  ~0% of desktop programs**; native SlateOS programs are unaffected entirely (NUMA
  mempolicy is a Linux-ABI construct).
- **A maximizes Linux-app compatibility.** The common real sequence is
  `mbind(MPOL_BIND)` then `set_mempolicy_home_node`; returning 0 keeps that path
  succeeding, which is what glibc/libnuma expect. Option B would report failure
  for a sequence Linux accepts (triggering "kernel lacks home-node" warnings or
  degraded fallback paths). Neither A nor B can crash a program or stop it
  starting — the difference is at most a warning log on B.
- **C is real, fragile code for no benefit.** Per-VMA policy means every VMA
  split/merge (`mmap`/`munmap`/`mprotect`/`madvise`/`mremap`) and `fork` must
  carry/dup the policy — meaningful complexity whose entire payoff is faithful
  errnos on syscalls almost nothing calls, with zero effect on what any program
  computes or how fast it runs (one node).

**Alternatives considered:**
- **(B) return `-ENOENT`** — rejected: "more literal" only for a case that has no
  practical consequence on UMA, and it breaks the common post-`mbind` success
  path.
- **(C) per-VMA mempolicy storage** — rejected for now: substantial, bug-prone
  machinery for zero UMA benefit. **The correct trigger to revisit is SlateOS ever
  targeting real multi-node (multi-socket) hardware** — at which point C should
  be implemented *properly* (real page placement, not just errno cosmetics), and
  the faithful errnos come for free.

**Where it lives:**
- `kernel/src/syscall/linux.rs`: `sys_set_mempolicy_home_node`, `sys_mbind`,
  `sys_set_mempolicy`, `sys_get_mempolicy` (the empty-mask/default-policy
  answers).
- `known-issues.md` TD7 (the UMA no-op tech-debt note).

**How to reverse:**
- If a multi-node target appears: implement per-VMA mempolicy + node-aware
  allocation, then make `set_mempolicy_home_node` walk real per-VMA policies and
  return `-ENOENT`/`-EOPNOTSUPP`/0 per Linux. Until then, A stands.

---

## 11. /proc/sys/vm/overcommit_memory & the SlateOS memory-commit policy — build Option 5 (both strategies, configurable) now

**Date:** 2026-06-13 (revised same day — see "Revision" below)

**Decided by:** Operator (this was `open-questions.md` Q2; the operator chose
Option 5 — "build both strategies, make them configurable" — with the priority
"maximize the number of programs that run without crashing; log noise is
acceptable." Options 4 and 5 were the operator's own proposals. Initially the
operator accepted a two-phase "C now, Option 5 later" plan; the operator then
asked to **do Option 5 now if there's no good reason to defer** — and a code
survey found most of the mechanism already exists, so the kernel core is being
built now. See "Revision").

**Update (2026-06-13, later) — split the system-wide knob per ABI.**
**Decided by:** Operator (operator asked "shouldn't we have two system-wide
policy selectors, one for native and one for linux, because linux tends to
expect overcommit?"; Claude agreed and implemented).
The original design had *one* system-wide knob (`mm.lazy_default`) that only
governed the **native** ABI, while the Linux ABI was hardcoded lazy — so an
admin could tune native's default but not Linux's, which is backwards (Linux is
exactly where overcommit-vs-strict is most likely to matter). Fixed by giving
each ABI its own system-wide selector:
- **Native** → `mm.lazy_default` (sysctl id 1), default committed (Desktop).
- **Linux** → new `mm.linux_lazy_default` (sysctl id 8), default 1 =
  lazy/overcommit on all workload profiles. Surfaced to userspace under the
  canonical Linux name `/proc/sys/vm/overcommit_memory`, which now *mirrors the
  live sysctl* (lazy → `0` heuristic-overcommit, committed → `2` never-overcommit)
  instead of being a hardcoded `0`.
`MmapCommitPolicy::linux_lazy` now takes the system-wide value (like
`native_lazy`): `Inherit` follows `mm.linux_lazy_default`, `ForceLazy`/
`ForceCommitted` override per-program. The workload presets carry
`linux_lazy_default = 1` uniformly (Linux apps expect overcommit regardless of
profile; flipping it manually drops profile detection, which is correct). Commit
*"mm: split system-wide commit policy per ABI (native vs Linux)"*. The Settings
front-end (§5.6) therefore exposes **two** system-wide selectors.

**Status (2026-06-13) — all three now-doable kernel items have landed.**
The unblocked kernel work below (items 1–3) is implemented and boot-tested:
- **(2) Linux mmap defaults to lazy/overcommit** + **(3) `/proc/sys/vm/overcommit_memory`
  exposed** (reading `0`, honest now that the Linux path passes `MAP_LAZY`) — commit
  *"mm: Linux mmap defaults to lazy/overcommit + expose vm/overcommit_memory"*.
  The Linux `mmap` path now also translates `PROT_WRITE`/`PROT_EXEC` into
  `MAP_WRITE`/`MAP_EXEC` (a latent read-only-anon bug fixed in passing).
- **(1) Per-program policy** — `pcb::MmapCommitPolicy` {Inherit, ForceCommitted,
  ForceLazy} stored on the PCB, inherited across fork, consulted by *both* `mmap`
  paths via pure `native_lazy`/`linux_lazy` helpers; kernel API
  `pcb::get/set_mmap_commit_policy`; covered by `pcb` self-test. Commit
  *"mm: per-program memory-commit policy override (Option 5 kernel core)"*.
Still following their dependencies (unchanged): the Settings → Advanced GUI
front-end and the capability-gated *writes* to `/proc/sys/vm/*` (`admin.memory_policy`).
The advisory `OvercommitMode` enum in `mmtune.rs` remains unwired — the live
mechanism is `MAP_LAZY` + `PARAM_MM_LAZY_DEFAULT` + the per-program policy; a
future cleanup could retire `OvercommitMode` or fold it into this path.

**Revision (2026-06-13) — do Option 5 now; only the GUI front-end and
capability-gated writes follow their dependencies.**
A survey of the actual code (prompted by the operator asking whether Option 5
could just be done now) found the mechanism is **~80% already built**, so there
is no good reason to defer the kernel core:
- **Both strategies already exist.** Native `mmap`
  (`kernel/src/syscall/handlers.rs::sys_mmap`) supports eager-commit (default)
  *and* demand-paged (`MAP_LAZY`); demand paging is fully implemented
  (`kernel/src/mm/fault.rs`, `VmaKind::Anonymous`).
- **A system-wide toggle already exists.** `sysctl PARAM_MM_LAZY_DEFAULT`
  (`mm.lazy_default`, default 0 = committed on Desktop) flips the system default;
  the per-workload profile presets already set it (Desktop/Dev/Gaming = committed,
  Server = lazy).
- **The advisory `OvercommitMode` enum** in `kernel/src/fs/mmtune.rs` is a second,
  unwired surface for the same concept (no consumer in the commit path).
- **What's genuinely missing (the now-doable, unblocked kernel work):**
  1. **Per-program policy** — today the choice is system-wide only; add a
     per-process override (PCB field consulted by both `mmap` paths).
  2. **Linux programs don't default to lazy/overcommit.** The Linux `mmap`
     (`kernel/src/syscall/linux.rs::sys_mmap`, ~line 4825) routes through the
     native handler with flags=0, inheriting the *committed* desktop default —
     with a now-stale comment claiming it's "demand-allocated." The operator's
     "Linux default = overcommit" is **not actually implemented**; this is a
     latent compat gap (Linux's idiom is large sparse mmaps that expect lazy
     backing). Fix: Linux `mmap` should default to lazy unless a per-program
     policy says otherwise. *(Partly forward-looking: per decision #4 there is no
     Linux ELF loader yet, so no real Linux program runs today — which is why
     this hasn't bitten. Fixing it now makes the path correct for when the loader
     lands.)*
  3. **Expose `/proc/sys/vm/overcommit_memory`** reading the active mode honestly
     (committed ↔ report `2`; lazy ↔ report `0`), plus `overcommit_ratio`/
     `overcommit_kbytes` for completeness.
- **What still follows its dependency (not arbitrary deferral):**
  - **Settings → Advanced GUI** — depends on the GUI/Settings app, which per
    decision #9 comes *after* the terminal/dev phase. Build it when the GUI
    exists; until then the policy is set via sysctl/config.
  - **Capability-gated *writes* to `/proc/sys/vm/*`** (`admin.memory_policy`
    enforcement) — depends on the capability framework (largely unbuilt). Until
    then `/proc/sys` stays read-only and the policy is set via the kernel sysctl
    mechanism.
- **Design nuance noted (not blocking):** SlateOS "committed" currently means
  *eager-populate* (allocate+map all frames at `mmap`), which satisfies "no
  silent overcommit" trivially but costs up-front faulting/RAM for pages never
  touched. Linux's `overcommit_memory=2` instead does *commit accounting* (reserve
  charge against RAM+swap, still demand-page). Eager-populate is the current,
  design-compliant behavior; a future refinement could switch "committed" to
  accounting-style reservation for the same guarantee at lower cost. Out of scope
  for the initial Option 5 build.

**Context:**
`design.txt`/CLAUDE.md mandate "Committed memory by default, **lazy allocation
opt-in**. No silent overcommit." Linux exposes `/proc/sys/vm/overcommit_memory`
(0 = heuristic overcommit [Linux default], 1 = always overcommit, 2 = strict
commit accounting). SlateOS currently hardcodes strict "committed by default, no
overcommit" (`mm/oom.rs`) and our `/proc/sys` is read-only with the `vm/`
subtree omitted. The question was whether to expose the file and at what value.
Options considered: (A) expose `= 2` (honest strict value, but its biggest risk
is that overcommit-expecting apps — Go/JVM/Electron/some WINE paths — may scale
back arenas, warn, or in a few cases refuse to start), (B) expose `= 0` (a lie —
we don't actually overcommit; an app trusting it could over-allocate and hit
commit failures), (C) keep `vm/` omitted (a *missing* sysctl almost never stops
a program — well-behaved code falls back to its built-in default; effect is at
most a line of log noise), (4) per-program user-configurable value with
OS-surfaced diagnosis, (5) implement **both** commit strategies and make the
choice configurable system-wide *and* per-program for both Linux and native
programs.

**Decision — build Option 5's kernel core now (see Revision above for why it's
mostly already built); GUI front-end and capability-gated writes follow their
dependencies.** Until the `/proc/sys/vm/overcommit_memory` surface lands, the
`vm/` subtree stays omitted (the original option C), which is harmless. The full
Option 5 scope:
  - Implement both **strict-commit** and **lazy/overcommit** allocation in the
    kernel (today only strict exists; the `OvercommitMode` enum in
    `kernel/src/fs/mmtune.rs` is advisory-only and **not wired into the commit
    path**).
  - Expose the choice **system-wide and per-program**, for both Linux and native
    programs, under **Settings → Advanced** with warnings.
  - **Default for Linux programs: `overcommit_memory = 0` (overcommit)** for
    maximum drop-in compatibility (operator's call); **native programs default
    to strict-commit** per "committed by default."
  - Option 4 (per-program override + OS diagnosis UX) is folded in as the **UX
    half of Option 5**, not a competing option.
  - Once 5 lands, `/proc/sys/vm/overcommit_memory` simply **reports the active
    mode honestly** (no longer a fabrication), retiring the original A/B/C
    dilemma.
- **Writes to `/proc/sys/vm/*` are gated on the privilege Linux calls
  CAP_SYS_ADMIN.** A Linux program may *write* the sysctl to request a policy
  change if it holds that privilege — but see the capability decision below for
  how that maps onto SlateOS's native model (we do **not** import CAP_SYS_ADMIN as
  a native capability).

**CAP_SYS_ADMIN / capability mapping (operator asked: add it to the native
capability list, or does it map to an existing capability?):**
- **Do NOT add `CAP_SYS_ADMIN` to the native capability list in
  `roadmap-detailed.md`.** CAP_SYS_ADMIN is Linux's notorious "junk drawer" —
  one coarse token gating ~1000+ unrelated operations. Importing it as a native
  capability would reintroduce exactly the **ambient authority** SlateOS exists to
  abolish ("capability-based security from day one, no ambient authority"), and
  it contradicts the project's deliberately **fine-grained** capability model
  (`fs.*`, `admin.*`, `resource.*`, `hook.*`, each a distinct risk level).
- **CAP_SYS_ADMIN is a Linux-ABI construct that lives only in the Linux compat
  layer.** When a Linux program performs an operation Linux gates on
  CAP_SYS_ADMIN, the compat layer maps **that specific operation** to the
  fine-grained *native* capability that actually governs it — it never grants a
  blanket "admin" power.
- **For the overcommit-write operation specifically, no existing native
  capability is an exact fit.** `resource.ram` is a *per-process RAM limit*, not
  a *system-wide VM-policy* control; `admin.*` today covers *user* administration
  (`admin.user`/`admin.user_caps`/`admin.cross_user`). Changing the **system-wide
  memory-commit policy** is a distinct, elevated risk that warrants its **own
  fine-grained native capability** — to be added when Option 5 is built (working
  name `admin.memory_policy`, i.e. "change system-wide memory/VM commit policy").
  A tracking entry is added to `roadmap-detailed.md` now.
  - Note the privilege split this enables (better than Linux's all-or-nothing):
    changing the **system-wide** policy needs `admin.memory_policy`; a user
    changing **their own program's** per-program override via Settings is a
    normal user/Settings action, **not** an elevated syscall — so per-program
    tuning doesn't require an admin capability at all.

**Rationale:**
- C now is the safest immediate answer for the stated priority and requires no
  new code (the `vm/` subtree is already omitted).
- Option 5 is *design-faithful*: the spec already sanctions both strategies with
  lazy as an explicit opt-in. It maximizes compatibility (overcommit-expecting
  Linux apps get what they want) without lying (the user opted in; nothing is
  silent), and keeps native code strict per "committed by default."
- The capability stance preserves least-privilege: a fine-grained
  `admin.memory_policy` is far safer than honoring a Linux blanket CAP_SYS_ADMIN,
  and the Linux-cap→native-cap mapping is the general pattern for the whole
  compat layer.

**Alternatives considered:**
- **(A) expose `= 2`** — rejected for now: real refuse-to-start / arena-shrink
  risk for overcommit-expecting apps, against the "max programs run" priority.
- **(B) expose `= 0`** — rejected: a fabrication (we don't overcommit), against
  the "never fabricate in procfs" rule and the design.
- **Add CAP_SYS_ADMIN as a native capability** — rejected: ambient-authority
  junk drawer; contradicts the fine-grained capability model.

**Where it lives:**
- `kernel/src/fs/procfs.rs`: `SYS_FILES`/`SYS_DIRS` (currently no `vm/`; Option 5
  adds `vm/overcommit_memory` reporting the active mode), `gen_sys`.
- `kernel/src/fs/mmtune.rs`: `OvercommitMode` (exists, advisory-only — Option 5
  wires it into the commit path).
- `kernel/src/mm/` commit/allocation path + `mm/oom.rs` (must learn to honor the
  mode), per-program policy storage (PCB / Linux-ABI PCB state).
- `kernel/src/syscall/linux.rs`: the Linux-ABI write path + CAP_SYS_ADMIN→native
  capability mapping (when sysctl writes are implemented).
- `roadmap-detailed.md`: new `admin.memory_policy` capability (tracking entry),
  and the Option-5 "both commit strategies, configurable" feature.
- Settings app: Advanced section (system-wide + per-program overcommit, warnings).

**How to reverse:**
- Immediate: exposing `vm/overcommit_memory` early (still read-only) is a small
  `procfs.rs` change if a specific app needs to *read* the value before Option 5
  lands; pick the honest current value (strict) per decision #1's "never
  fabricate" rule.
- End-state: if Option 5 proves not worth the complexity, fall back to a single
  honest read-only value reflecting the hardcoded strict policy. The capability
  decision (no native CAP_SYS_ADMIN) is independent and should not be reversed.

---

## 12. Toolchain on Slate OS — run prebuilt Linux binaries on the compat layer (Path Z), native-first kept inviolate

**Date:** 2026-06-13

**Decided by:** Operator (this was `open-questions.md` Q4; Claude initially framed
it as "clang-bootstrap vs build-a-gcc-cross-compiler," the operator's feedback
redirected it to the real fork — *how the toolchain runs on the OS* — and the
operator chose Z: run prebuilt Linux toolchain binaries on the Linux-ABI layer
now, native-port selectively later. The operator also green-lit installing
whatever tooling is needed.)

**Context:**
§9 set the toolchain (gcc/cmake/make/pkg-config, then CPython→fastpy) as the next
initiative. The first real fork is *how those programs run on the OS*:
- **Path X** — run **prebuilt Linux** gcc/make/cmake on the kernel's Linux-ABI
  layer (the Linux ELF loader + `ld.so` loading + Linux syscall table that
  already exist); drop a distro's binaries + glibc/`ld.so` on the image and
  harden the compat layer until they run. Matches the roadmap's "gcc … *(via
  POSIX layer)*" wording. No host C cross-compiler needed; least work to a usable
  toolchain; directly hardens the Linux-ABI layer (reused by every future Linux
  app); **this is the Linux compatibility we need anyway.**
- **Path Y** — native SlateOS/Slate port: gcc cross-compiler targeting
  `x86_64-slateos` + a native C library. Purity path (native syscalls, capability
  security) but enormous per-program effort for gcc/CPython, and it does **not**
  advance Linux compat.
- **Path Z** — hybrid: X now, Y selectively later for components where
  capability-native behavior matters.

**Decision — Path Z, starting with X.** Run prebuilt Linux toolchain binaries on
the Linux-ABI layer to get a working dev environment and harden Linux
compatibility; native-port only where it specifically pays off, later. Install
clang (and any other needed tooling) — clang targets both `x86_64-linux-*` (to
compile real Linux C programs that stress/harden the compat layer) and
`x86_64-slateos` (native C, if/when Y components are pursued).

**Bounding principle (operator-reaffirmed) — native-first is inviolate; the
compat layer must not leak into the native architecture.** Slate OS is a
**native-first OS with a deliberately-scoped Linux compat bridge**, not a
"Linux-compatibility OS." This decision does **not** soften the §4 rule. In
particular:
- We do **not** shape native primitives (IPC, scheduling, startup, the absence
  of signals) around Linux to make translation cheaper. Native primitives are
  designed on their own merits (channel+capability IPC; priority round-robin
  per-CPU scheduling; `SYS_PROCESS_GET_ARGS` startup with no SysV stack; no Unix
  signals — process control is IPC, hardware faults are SEH-style exceptions).
- The compat layer is **fast as a downstream consequence** of a well-designed
  native kernel, never as a design goal that bends native primitives.
- The one choice that helps compat — **ext4** — was made on native merits
  (`design.txt`: "ext4 first, don't write a custom filesystem"); its Linux-native
  semantics are a coincidental benefit and are *why* we avoid WSL1's
  NTFS-semantics performance disaster without any leak.
- Test for any new construct: *would it exist if Linux had never existed?* If no,
  it stays in the compat layer (like signals — see `kernel/src/proc/signal.rs` —
  and the auxv, §4) and never touches native launch/syscalls/startup.

**Honest scope / known walls (not promising 100% Linux parity):**
- The toolchain (CLI: gcc/make/cmake/bash/python/git) sits in the zone
  syscall-translation layers handle well (cf. FreeBSD Linuxulator, illumos LX
  zones). This is low-risk.
- Genuinely hard / may-never-fully-support categories: GPU-accelerated GUI
  (solved later at the **Wayland-protocol + Mesa-port** boundary, *not* by
  matching Linux's kernel graphics uAPI), containers (cgroups/namespaces/
  overlayfs/netlink/seccomp), systemd, exotic networking (AF_PACKET/netlink/eBPF),
  ptrace fidelity, io_uring corners, FUSE.
- Escape hatch for the hard cases: a WSL2-style **real Linux kernel in a VM**
  remains available later — but it needs a hypervisor (KVM-equivalent) Slate
  doesn't have yet, so it's a separate large project. Compat-layer and VM are not
  mutually exclusive (Windows ships both).

**Why this is defensible despite Microsoft's WSL1→WSL2 pivot:** WSL1 translated
to a hostile pre-existing kernel (NT/NTFS) and chased seamless full-Linux parity;
its perf death was the fs-semantics mismatch. Slate is co-designed, uses ext4,
and **bounds the promise** (native-first; compat covers a chosen software set).
The completeness treadmill only bites a project that promises 100% — we don't.

**Alternatives considered:**
- **Path Y (native port first)** — rejected for now: doesn't advance Linux
  compat, enormous for gcc/CPython, and CLAUDE.md already prefers **fastpy** for
  native OS userspace, shrinking the need to native-port big C apps.
- **Pure clang-bootstrap of a native C runtime first** (Claude's original Q4
  "A/C") — rejected: it's native-ABI work mislabeled as the fast path; it does
  not deliver the Linux compatibility the operator wants, and the toolchain
  goal is better served by running real Linux binaries.

**Where it lives:**
- Path X work: `kernel/src/syscall/linux.rs`, the Linux ELF loader, procfs/sysfs
  (Linux-ABI hardening); disk-image work to stage a glibc/`ld.so` + prebuilt
  toolchain runtime; clang (host) for compiling Linux test programs.
- roadmap.md task 5031 (gcc/cmake/make/pkg-config "via POSIX layer").
- Native-first/no-leak constraint: design-decisions.md §4, `design.txt`,
  `kernel/src/proc/signal.rs` module doc.

**How to reverse:**
- If running prebuilt Linux gcc proves to hit an unfixable Linux-ABI wall,
  fall back to Path Y for that component (native cross-build), or stage the
  WSL2-style Linux-VM escape hatch (after building a hypervisor). The
  native-first/no-leak principle is **not** reversible — it is settled policy.

---

## 13. Two roadmap files — roadmap.md is the live source of truth; roadmap-detailed.md is an annotated design reference

**Date:** 2026-06-13

**Decided by:** Claude (operator-approved scope) — the operator delegated the
call ("you're the developer, so I'll make it your call") and suggested the
annotation convention (flag parts done/blocked/blocked-by in
`roadmap-detailed.md` without deleting information). Claude made the specific
policy.

**Context:**
The repo has two roadmap files that had drifted apart:
- `roadmap.md` — 846 commits, continuously updated with task-completion status
  (procfs `/proc/sys`, DRM shim, ALSA, ld.so/dynamic-linker all recorded here).
- `roadmap-detailed.md` — its own header calls it "the fine-grained companion to
  `roadmap.md`. Every actionable feature from `design.txt` … as a checkbox item."
  Only 41 commits; recent work is largely absent (e.g. ld.so/dynamic-linker: 0
  mentions). 1207 items, of which only 156 were marked done — i.e. its status
  flags lag reality badly.

The operator initially believed `roadmap-detailed.md` was "the final say" and
`roadmap.md` might be old news; investigation showed the opposite for *status*
(roadmap.md is the maintained one). The naming misleads: "detailed" = finer
feature enumeration from `design.txt`, not more current.

**Decision:**
- **`roadmap.md` is the single source of truth for live progress/status.** It is
  the file to consult and update when starting/finishing a task.
- **`roadmap-detailed.md` stays the design reference** — the exhaustive
  design.txt-derived feature enumeration. It is **annotated in place** with
  concise status flags (`[x]` done, `[-]` in progress, `[~]`/blocked + a short
  "blocked by …" note) **without deleting any information**, so a reader of the
  design reference can see at a glance what is built. Annotation is **incremental
  and verification-based** — items are flagged done only when verified (cross-
  referenced against `roadmap.md` or the code), never fabricated. A full one-shot
  reconciliation of all 1044 unchecked items is deliberately NOT attempted (too
  large, too error-prone); the gap closes as items are touched.

**Rationale:**
- Avoids dual-maintenance churn and the risk of the two files contradicting each
  other on status, while preserving the genuine value of the detailed file (a
  complete, design-anchored feature inventory the high-level roadmap lacks).
- The operator explicitly wanted at-a-glance status in the design reference, met
  by inline flags rather than by promoting it to the authority.

**Alternatives considered:**
- *Promote `roadmap-detailed.md` to source of truth / deprecate `roadmap.md` in
  CLAUDE.md.* Rejected: roadmap.md is the actually-maintained file; deprecating it
  would discard the live status history. (Also CLAUDE.md is operator-owned; the
  operator's permission to edit it was conditional on roadmap.md being "old news,"
  which proved false, so CLAUDE.md was left untouched.)
- *Keep both fully synchronized.* Rejected: 1207 vs 846-commit drift shows the
  cost is real and the payoff low; the files serve different purposes.
- *Delete `roadmap-detailed.md`.* Rejected: the operator wants it kept as a
  no-information-lost design reference.

**Where it lives:**
- `roadmap.md` (live status), `roadmap-detailed.md` (annotated design reference).

**How to reverse:**
- If maintaining annotations in the detailed file proves not worth it, stop
  annotating and treat `roadmap-detailed.md` as a frozen design snapshot; or, if
  the detailed file becomes the working file, migrate status tracking there and
  note it in CLAUDE.md (operator's call, since CLAUDE.md is operator-owned).

## 14. Doc roles — todo.txt is the AI's scratch file (open TODOs + deferred items only); issues→known-issues.md, decisions→design-decisions.md, open questions→open-questions.md

**Date:** 2026-06-13

**Decided by:** Operator (Claude proposed the "stop todo.txt being a catch-all"
structure and recommended option B; operator chose B and added the refinements
below). The operator also delegated ownership of `todo.txt` to the AI.

**Context:**
`todo.txt` had grown to ~53,000 lines — a `#`-prefixed engineering journal that
duplicated three other sources:
- `roadmap.md` — its ~55 DONE/VERIFIED blocks restate status the roadmap already
  tracks with checkboxes.
- git commit messages — each `DONE: ABI batch NNN` block is essentially the
  commit body re-pasted.
- `known-issues.md` — its ~109 BUG/DIVERGENCE/LIMITATION blocks are exactly what
  `known-issues.md` exists for.
There was also a doc contradiction: `roadmap-detailed.md` said "todo.txt is the
operator's personal file — AI does not write to it," while CLAUDE.md instructs
the AI to write to it (and it is full of AI entries).

Considered: (A) leave as-is (redundant but works); (B) adopt the clean structure
going forward with no mass rewrite; (C) B plus a one-time migration/prune.
Merging the journal into `roadmap.md` was rejected outright — `roadmap.md`'s
value is being a concise, scannable status index; growing it defeats that.

**Decision (option B + operator refinements):**
- **`todo.txt` is the AI's working scratch file** (ownership delegated to the AI;
  `roadmap-detailed.md` updated to agree with CLAUDE.md, resolving the
  contradiction).
- **Going-forward routing:**
  - bugs / divergences / limitations / tech-debt → `known-issues.md`
  - resolved judgment calls & design decisions → `design-decisions.md`
  - judgment calls awaiting operator input → `open-questions.md`
  - completed work → the git commit + the `roadmap.md` checkbox (NOT restated in
    `todo.txt`)
  - `todo.txt` keeps ONLY genuine open TODOs and deferred-with-rationale items.
- **Judgment calls do NOT live in `todo.txt`** (operator's correction): they have
  dedicated homes — `design-decisions.md` (resolved) and `open-questions.md`
  (pending). This supersedes CLAUDE.md's older "document judgment calls in
  todo.txt under a `## Judgment Calls` heading" wording.
- **Legacy migration of the existing issue blocks → `known-issues.md`:** done
  carefully, a chunk at a time, deleting from `todo.txt` as moved. A full
  snapshot was taken first (`todo.backup-2026-06-13.txt`, git-ignored) so the
  move is reversible. (Operator offered either keep-duplicates or
  delete-as-moved-with-backup; chose delete-as-moved-with-backup to actually
  reduce the sprawl rather than add more.)

**Why not the alternatives:**
- *(A) leave as-is:* the redundancy keeps growing and bugs become hard to find
  (three places to look); the operator wants issues consolidated.
- *(C) full bulk reformat now:* `known-issues.md` uses curated formatting
  (`### W1`/`### TD14`, `**Where/What/Why/Proper fix**`), so a faithful migration
  is per-entry human-judgment reformatting, not a mechanical dump — best done
  incrementally to avoid mangling either file.

**Where it lives:**
- `todo.txt` (new scope header at top), `roadmap-detailed.md` (ownership note),
  `known-issues.md` (issue destination), `design-decisions.md`/`open-questions.md`
  (decision/question destinations).

**CLAUDE.md note (operator-owned, NOT edited here):** several CLAUDE.md lines
still describe `todo.txt` as the destination for bugs/limitations (Bug Tracking
section) and judgment calls (`## Judgment Calls`), and for "genuinely stuck"
notes. Those reflect the older catch-all model and are superseded by this
decision. Per the rule that only the operator edits CLAUDE.md, they were left
unchanged — the operator may wish to align them (point bugs→known-issues.md,
judgment calls→design-decisions.md/open-questions.md).

**How to reverse:**
- Restore `todo.backup-2026-06-13.txt` to recover any migrated block verbatim;
  the per-block moves are also in git history. Revert this section and the
  `todo.txt`/`roadmap-detailed.md` headers to return to the catch-all model.

## 15. avahi `autoipd` MAX_CONFLICTS — fail on the (MAX_CONFLICTS+1)th conflict, matching RFC 3927

**Date:** 2026-05-31

**Decided by:** Claude (autonomous) — a small implementation choice made while
bringing `userspace/avahi` (autoipd) up; easily reversible.

**Context:**
RFC 3927 §2.2.1 (IPv4 Link-Local) says a host that experiences more than
`MAX_CONFLICTS` (10) address conflicts should rate-limit / give up. The avahi
`autoipd` conflict counter previously failed when `count >= MAX_CONFLICTS`,
i.e. it tolerated only 9 retries and gave up on the 10th. The unit test encodes
the opposite intent — 10 retries tolerated, fail on the 11th — which matches the
RFC's "exceeds MAX_CONFLICTS" wording.

**Decision:** changed production to `count > MAX_CONFLICTS` to align with both the
test and the RFC, rather than relaxing the test to match the stricter code.

**Why not the alternative:** the stricter "give up at the 10th conflict" reading
is defensible (one fewer probe), but it contradicts the literal RFC wording and
the test's encoded intent; aligning to the RFC is the lower-surprise choice.

**Where it lives:** `userspace/avahi` — `AutoIpd::step`, the `Conflict` arm (one
comparison). Reverse by flipping `>` back to `>=` if the operator prefers the
strict reading.

## 16. nushell port — build with the msvc nightly toolchain; leave upstream warnings unpatched

**Date:** 2026-06-03

**Decided by:** Claude (autonomous) — a build-toolchain workaround and a
don't-touch-upstream call made while bringing up the nushell port; both
reversible.

**Context:**
The project's default toolchain is `1.93.1-x86_64-pc-windows-gnu` (per
`rust-toolchain.toml`). The nushell port hit an upstream gnu-toolchain build
issue. Separately, building nushell surfaces a handful of upstream warnings not
introduced by our port: an unused `PipelineMetadata` import in
`nu-cli/.../history_.rs`, an unused `path::Path` import in `nu/src/command.rs`, a
dead-code `ListPath` variant in the nu binary, and a future-incompat warning on
`proc-macro-error2 v2.0.1` (a dep of a dep).

**Decision:**
- **Build nushell specifically with `rustup run nightly-x86_64-pc-windows-msvc`**
  until the upstream gnu issue is resolved or the broader project moves to msvc.
  The project pin is NOT changed; this affects only how nushell is built, no
  other workspace crate.
- **Do not patch the upstream nushell warnings** — they are warnings only, not
  introduced by our port, and not worth carrying local edits to upstream code.

**Why not the alternatives:** upgrading the project-wide toolchain pin to msvc
would affect every crate and is a larger decision than one port warrants;
patching upstream warnings creates a maintenance burden against future nushell
updates for no functional gain.

**Where it lives:** the nushell port build invocation (msvc nightly); the warnings
live in upstream `nu-cli`/`nu` sources. Reverse the toolchain workaround when the
gnu issue is fixed or the project moves to msvc.

## 17. TD22 Phase 1 — build demand-paged file-backed `MAP_PRIVATE` (option B) autonomously, ahead of the operator's option-C decision

**Date:** 2026-06-14

**Decided by:** Claude (autonomous) — this overrides the prior recommendation in
`open-questions.md` Q5, which had said "do B now, C later, but don't *build* B
until the operator settles C." I re-evaluated that rework risk, judged it low,
and proceeded. Reversible; the operator may overrule.

**Context:**
File-backed `mmap(2)` (`linux_file_mmap` in `kernel/src/syscall/linux.rs`) used
an **eager private-copy** model: at map time every 16 KiB frame was allocated,
the file bytes `read_at`-copied in, and the frame mapped via a `VmaKind::Fixed`
VMA. This wastes memory/latency on large or sparse maps (known-issues.md TD22
gap 1). The proper end-state is a unified page cache + writable `MAP_SHARED`
writeback (TD22 gap 2 / Q5 option C), which is a foundational, multi-subsystem
fork still deferred to the operator (needs a stable VFS file-identity, a
double-cache-vs-unify call against `fs/cache.rs`, etc.).

Q5's earlier recommendation was to hold off on even the `MAP_PRIVATE`
demand-paging half (option B) until C was settled, on the theory that C might
rework B.

**Decision:**
Build option B now: a `MAP_PRIVATE`, non-`MAP_FIXED` mmap of a regular file
registers a `VmaKind::FileBacked { handle, file_offset }` VMA and allocates **no
frames up front**. The page-fault handler resolves a fault by allocating a
zeroed frame, `read_at`-ing one page from the backing handle (tail past EOF
stays zero = Linux page zero-fill), and mapping it. Private writes copy-on-fault
to a per-process frame, never reaching the file. memfd-backed maps, read-only
`MAP_SHARED`, and `MAP_FIXED` overlays (ld.so segment loader) keep the eager
`VmaKind::Fixed` path. Writable `MAP_SHARED` still returns `ENOSYS` (gap 2,
unchanged).

The FileBacked VMA owns an independent reference on the open-file description
(via `fs::handle::dup_shared`, decoupled from the fd), with a full refcount
lifecycle: dup at mmap, per-VMA dup on fork, release on munmap / `MAP_FIXED`
split (net retain/release in `remove_vma_range`) / execve
(`reset_vmas_for_exec`) / process exit. All handle ops are deferred until the
`PROCESS_TABLE` lock is dropped, honoring the PROCESS_TABLE→OPEN_FILES lock
order. A pre-existing exec-VMA-staleness bug (execve tore down the address space
but never cleared the per-process `vmas` list) was fixed in passing via
`reset_vmas_for_exec`.

**Why this was safe to do without the operator (the rework-risk re-evaluation):**
- B's fault-path *shape* — a `VmaKind::FileBacked` VMA lazily populated by the
  fault handler — is exactly what C needs too. C only changes the *source* of
  the page (page cache vs direct `read_at`) and the private/shared policy.
- The only piece C might discard is the small, localized handle-refcount
  lifecycle (~60 lines, isolated in `pcb.rs`). Rewriting it under C is cheap.
- B is independently correct, not a temporary hack: `MAP_PRIVATE` may
  legitimately not observe later file writes, so demand-reading at fault time is
  *more* faithful than the eager snapshot.
- B is a strict improvement meanwhile (no eager whole-span copy) and fully
  reversible.

**Why not the alternatives:** leaving the eager copy (Q5 option A) keeps wasting
memory on big maps; waiting to build B until C is settled (the prior Q5 stance)
delays a correct, reversible improvement for a rework risk that on inspection is
small.

**How to reverse:** drop the `VmaKind::FileBacked` arm and re-point
`linux_file_mmap`'s `MAP_PRIVATE` path at the eager copy loop; the handle
lifecycle wiring in `pcb.rs` then becomes dead and can be deleted.

**Where it lives:** `kernel/src/mm/vma.rs` (`VmaKind::FileBacked`),
`kernel/src/syscall/linux.rs` (`linux_file_mmap`, `self_test_file_mmap`),
`kernel/src/proc/pcb.rs` (handle lifecycle: `add_vma`, `remove_vma`,
`remove_vma_range`, `reset_vmas_for_exec`, `try_resolve_fault`, `fork_create`,
`destroy`, `vma_release_backing`), `kernel/src/proc/spawn.rs` (exec reset),
`kernel/src/fs/procfs.rs` (maps label). Tracked as **known-issues.md TD22**
(now PARTIAL); option C remains **open-questions.md Q5**.

## 18. TD8 membarrier per-mm registration — inherit `membarrier_state` across fork, do NOT reset on exec (yet)

**Date:** 2026-06-14

**Decided by:** Claude (autonomous). Reversible; the operator may overrule. Two
small judgment calls inside the TD8 fix (gating `membarrier(2)` expedited issues
on prior registration), neither of which has an obviously-correct answer given
Linux-source ambiguity and our missing exec-reset hook.

**Context:**
TD8 added a per-mm `Process::membarrier_state` READY bitmask so an unregistered
`PRIVATE_EXPEDITED*` issue returns `-EPERM` (matching Linux). Two lifecycle
questions arose:

1. **Fork inheritance.** Linux's `dup_mm` does `memcpy(mm, oldmm)` and `mm_init`
   does not clear `membarrier_state`, so a forked child inherits the parent's
   registrations. But intuition ("registration is per-mm, a new mm starts
   fresh") points the other way, and the kernel source is subtle.
2. **Exec reset.** Linux's `membarrier_exec_mmap` resets `membarrier_state` to 0
   on `execve`. We have no exec-time PCB-reset hook today (the same gap already
   documented for `linux_pdeathsig`, `linux_dumpable`, `linux_keepcaps`).

**Decision:**
1. **Inherit across fork** (child copies the parent's `membarrier_state`),
   matching the `dup_mm` memcpy. Rationale: it is what Linux actually does, and
   it is the *more permissive* choice — a child that relied on the parent's
   registration won't get a surprise `-EPERM`. Easy to flip to "reset to 0" if
   this proves wrong (one line in `fork_create`).
2. **Do NOT reset on exec yet** — consistent with the existing codebase, which
   already does not reset `pdeathsig`/`dumpable`/`keepcaps` on exec for the same
   reason (no shared exec-time PCB-reset block exists). Chasing a dedicated exec
   hook for one field would be a larger, separate change touching several fields
   at once. Documented as a residual in `known-issues.md` TD8 and `todo.txt`;
   the `todo.txt` entry says to add `membarrier_state = 0` when that shared
   exec-reset block is finally built.

**Alternatives considered:**
- *Reset membarrier_state to 0 on fork* — diverges from Linux's memcpy and is
  stricter (could EPERM a child that Linux permits). Rejected.
- *Build the exec-reset hook now, just for membarrier* — correct end-state but
  premature and narrow; the proper fix resets several fields together. Deferred.
- *Keep TD8 unresolved (per-task or no state)* — rejected: a per-task map would
  wrongly reject a cross-thread issue Linux accepts (threads share one mm); the
  per-mm field is the right home and is now testable via the pure
  `membarrier_decide` helper + direct `pcb` exercise without a userspace harness.

**Where:** `kernel/src/proc/pcb.rs` (`Process::membarrier_state`, fresh/fork
init, `membarrier_state`/`membarrier_register` accessors),
`kernel/src/syscall/linux.rs` (`membarrier_decide`,
`membarrier_registrations_mask`, `sys_membarrier`, self-test
`self_test_membarrier_registration`). Tracked as **known-issues.md TD8**
(now RESOLVED) with the exec-reset residual in `todo.txt`.

**How to reverse:** to reset on fork, set `membarrier_state: 0` in the
`fork_create` child literal instead of inheriting `parent.membarrier_state`. To
drop the whole feature, revert `sys_membarrier` to the unconditional
`fence`/`0` arms and remove the `Process` field + accessors.
