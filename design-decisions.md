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

## Numbering and file order

Sections are numbered by lane, and **the file is ordered by section number,
not by date.** Insert your entry among its numeric neighbours; do not append
at end-of-file unless your number happens to be the highest.

| Band | Owner | Region of this file |
|---|---|---|
| §1–§127 | single-agent history | the head — never renumber these |
| §200–§299 | **lane A** | contiguous, after the history |
| §300–§399 | **lane B** | contiguous, after A's band |
| §400–§499 | **lane C** | contiguous, to end of file |

The numeric *order* is what makes the bands physically disjoint, and that —
not the numbering by itself — is what makes this file merge cleanly between
three lanes: each lane's insertion point is a different line offset, so git
never has to compare two lanes' text. Chronological order silently defeats
it, because then all three lanes insert at the same place. That is not
hypothetical: on 2026-08-16 lane A's §203 and lane C's §435/§436 were all
appended at EOF and conflicted, with both lanes following the rule as
written. The file was sorted into numeric order in the same commit that
fixed the rule.

Entries may be **edited in place inside your own band** — this file is
lane-partitioned, not append-only. See §437 and `roadmap.md` →
"Three-Agent Parallel Execution" rule 3.

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
**Re-confirmed by the operator 2026-06-14** ("go with your recommendation") when
the standing Q1 confirm was put to them — option A stands.

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
built now. See "Revision"). **Re-confirmed by the operator 2026-06-14** when the
standing Q2 confirm was put to them: keep the shipped per-ABI commit-policy
defaults — **native strict/committed, Linux lazy/overcommit**, both configurable.
The operator deferred to Claude on whether strict is the better native default
("if you think strict is better for our OS, then i'll go with that"); strict is
kept for native because a desktop OS benefits from honest, immediate allocation
failures over deferred OOM-kill surprises, while Linux keeps overcommit because
Linux programs assume it.

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

## 19. User mmap allocator — split the window into a VMA-tracked general region and a disjoint device region

**Date:** 2026-06-14
**Decided by:** Claude (autonomous)

**Problem:**
The old `mmap_alloc_vaddr` was a single process-global `static NEXT_VADDR:
AtomicU64` monotonic bump counter handing out user mmap addresses. Three
defects: (1) it never reused a `munmap`'d address → a map/unmap-heavy process
eventually exhausts the window (permanent OOM); (2) one counter was shared
across *all* processes; (3) it never consulted the per-process VMA list, so a
returned address could overlap a `MAP_FIXED` overlay, the ld.so base, or a PIE
segment. This also blocked TD9 (ASLR), which needs a real region allocator.

**Decision:**
Replace the bump counter with a **per-process VMA-aware gap allocator**, and
split the user mmap window `0x0060_…0000 .. 0x0070_…0000` into two **disjoint**
sub-regions:

- **General region** (`USER_MMAP_BASE..USER_MMAP_END`, the low 15/16ths):
  served by `mm::vma::find_gap` (bottom-up first-fit over the sorted VMA list)
  via `pcb::reserve_unmapped_area`, fronted by
  `handlers::alloc_user_mmap_reserve`. Every mapping placed here registers a
  VMA, so freed gaps are *reused* and a returned address can never overlap an
  existing mapping. Used by anonymous mmap (committed + lazy) and file-backed
  mmap.
- **Device region** (`DEVICE_MMAP_BASE..DEVICE_MMAP_END`, the top 1/16th):
  served by the old bump allocator, now repurposed/bounded to this window.
  Used by DRM dumb-buffer mmap and MMIO mmap — mappings that map device frames
  **without** registering a VMA, so the gap finder cannot see them.

Find+insert is done atomically under one `PROCESS_TABLE` lock
(`reserve_unmapped_area` = `find_gap` + `Vma` insert), closing the SMP
find-then-add TOCTOU race (two concurrent same-process mmaps could otherwise
pick the same gap → spurious `ENOMEM` on the second `add_vma`).

**Alternatives considered:**
- *Migrate everything to the gap finder (one region).* Rejected: DRM/MMIO maps
  register no VMA, so they're invisible to `find_gap` and would collide with
  gap-finder allocations. Making them register VMAs would perturb the DRM
  frame/refcount/fork lifecycle (TD11) — a much larger, riskier change. The
  disjoint device window sidesteps this entirely.
- *Keep the bump allocator but make it per-process.* Fixes the cross-process
  bug but still leaks freed VAs and still ignores the VMA list (overlap risk).
  Rejected.
- *Separate find-gap call then `add_vma` (two lock acquisitions).* Simpler but
  reopens the SMP TOCTOU race the old atomic counter didn't have. Rejected in
  favour of the single-lock `reserve_unmapped_area` (a find-only
  `find_unmapped_area` helper was written and then removed to keep the racy
  pattern from being reintroduced).

**Tradeoff accepted:** the device region still uses a no-reuse bump allocator,
so a DRM/MMIO map/unmap-heavy process could exhaust the top 1/16th of the
window. Accepted because device buffers are few and long-lived; reuse there is
tracked as minor debt alongside the broader DRM mmap work (TD11).

**Where:** `kernel/src/mm/vma.rs` (`find_gap` + self-tests),
`kernel/src/proc/pcb.rs` (`reserve_unmapped_area`),
`kernel/src/syscall/handlers.rs` (window constants,
`alloc_user_mmap_reserve`, repurposed `mmap_alloc_vaddr`, `sys_mmap`
`reserved`-flag plumbing), `kernel/src/syscall/linux.rs`
(`linux_file_mmap` fixed/non-fixed atomic-reserve restructure +
`linux_file_mmap_fill` helper). Unblocks **known-issues.md TD9** (the
allocator dependency now exists; only the randomisation policy remains).

**How to reverse:** restore a single monotonic `AtomicU64` in
`mmap_alloc_vaddr` spanning the whole window and route the anon/file paths
back through a find-only helper + `add_vma`; drop `reserve_unmapped_area`
and the `reserved` flag. (Doing so re-introduces all three original defects
and the SMP race, so this is not advised.)

## 20. Interpreter + PIE-executable ASLR — 28 bits of entropy each, always-on when the CSPRNG is seeded (no personality opt-out yet)

**Date:** 2026-06-14 (interpreter); 2026-06-14 (PIE base)
**Decided by:** Claude (autonomous) — reversible; the operator may overrule. This
fully resolves known-issues.md TD9, whose documented "proper fix" was always to
randomise the load bases; the only genuine choices were *how much entropy* and
*whether to honour an opt-out*. Both load bases (ld.so interpreter and the PIE
main executable) are now randomised under the same policy.

**Problem:**
`load_interpreter` (`kernel/src/proc/spawn.rs`) loaded ld.so at the fixed
`LINUX_INTERP_BASE = 0x7000_0000_0000` every exec, removing the ASLR defence.
With the VMA-aware mmap allocator now in place (decision #19), the remaining
work was purely the randomisation policy. Two sub-decisions had real tradeoffs.

**Decision:**
1. **Entropy = 28 bits, in 16 KiB-page units** (`INTERP_ASLR_BITS = 28`). The
   per-exec base is `LINUX_INTERP_BASE + next_bounded(2^28) * FRAME_SIZE`
   (saturating), via the pure `apply_aslr_base` helper. 28 mirrors Linux
   x86_64's default `mmap_rnd_bits` (28) — i.e. the same *number of equally
   likely bases* (2^28), which is the security-relevant metric, even though our
   16 KiB pages make the byte-range (4 TiB) differ from Linux's (1 TiB at 4 KiB
   pages). The 4 TiB window's top (`≈0x73FF_FFFF_C000`) stays far below
   `USER_STACK_GUARD`, so collisions with the stack/executable/brk/mmap-window
   are impossible (the interpreter is the window's sole occupant). A
   `spawn::self_test` assertion guards this clearance invariant against future
   bit-count changes.
2. **Always-on when seeded; fixed-base fallback before the CSPRNG is seeded.**
   No `personality(ADDR_NO_RANDOMIZE)` / `setarch -R` opt-out yet (our
   `sys_personality` accepts but does not honour bits — see todo.txt). ASLR is
   a pure hardening win and every modern OS defaults it on, so always-on is the
   right default; a per-process opt-out can be wired through personality later
   if a debugger needs deterministic addresses.

**Alternatives considered:**
- *Match Linux's byte-range (1 TiB) by using ~26 bits.* Rejected: entropy (bit
  count), not byte span, is the ASLR security metric; matching Linux's 28-bit
  entropy is the principled choice, and our window has ample room for it.
- *Fold the interpreter into the general mmap region and let the gap allocator
  place it.* Rejected for now: the interpreter window (`0x7000_…`) is disjoint
  from the mmap window (`0x0060_…`) by design (TD9 note), and randomising
  within its own dedicated, collision-free window is simpler and lower-risk
  than threading interpreter placement through the general allocator.
- *Match Linux's byte-range for the PIE base.* Same rejection as the
  interpreter: entropy is the metric. The PIE window reuses the 28-bit policy.

**PIE-executable base (second half of TD9):**
The PIE main-executable base previously loaded at the fixed
`LINUX_PIE_BASE = 0x5555_5555_4000` (Linux's `ELF_ET_DYN_BASE`). `exec_load_bias`
is computed once per spawn/exec and threaded through `load_segments_with_bias`,
the biased entry point, and the AT_ENTRY/AT_PHDR auxv, so a single helper
suffices: `choose_exec_load_bias(is_pie)` returns `0` for ET_EXEC, and for PIE
returns `apply_aslr_base(LINUX_PIE_BASE, next_bounded(2^28))` when the CSPRNG is
seeded (fixed `LINUX_PIE_BASE` fallback otherwise) — the *same* `apply_aslr_base`
helper and 28-bit entropy (`PIE_ASLR_BITS = 28`) as the interpreter. The 4 TiB
PIE window sits far above the mmap window (`0x0060_…`) and far below the
interpreter window (`0x7000_…`), leaving ≥1 TiB of headroom below the
interpreter floor (asserted by `test_pie_aslr_window` in `spawn::self_test`). As of
2026-06-14 the brk heap is real (see entry #21 below): a PIE image's heap grows
from its page-aligned image end up to a ceiling of `LINUX_INTERP_BASE` (the
interpreter window floor), i.e. into this window's headroom, so the "brk grows
above the PIE image" concern is now handled by the `brk_ceiling` bound plus the
grow-path VMA-overlap guard.

**How to reverse:** set the bases back to the `LINUX_INTERP_BASE` /
`LINUX_PIE_BASE` constants in `load_interpreter` / `choose_exec_load_bias` (drop
the `is_initialized()`/`apply_aslr_base` blocks) and remove
`test_apply_aslr_base` / `test_pie_aslr_window`. To instead make it opt-out-able,
gate the randomisation on a per-process "no randomise" flag fed from
`personality(ADDR_NO_RANDOMIZE)`.

## 21. Linux `brk`/`sbrk` heap — image-dependent ceiling, committed RLIMIT_AS charge, no `arch_randomize_brk` gap yet

**Date:** 2026-06-14
**Decided by:** Claude (autonomous) — reversible; the operator may overrule. The
core task (replace the `sys_brk` no-op stub with a real heap) had no genuine
fork — the stub was a latent ring-3 SIGSEGV (it claimed a grow succeeded while
mapping nothing, so glibc's malloc brk fast path would fault on first heap
write). Three sub-decisions had real tradeoffs.

**Problem:**
`sys_brk` (`kernel/src/syscall/linux.rs`) echoed the requested break and mapped
no memory. A real heap needs a heap floor/break in the PCB, a demand-paged VMA,
a growth ceiling that can't collide with other regions, and a resource-accounting
policy.

**Decision:**
1. **Image-dependent ceiling (`brk_ceiling`).** A low-loaded ET_EXEC heap
   (`brk_start < USER_MMAP_BASE`) is capped at `USER_MMAP_BASE` (the mmap window
   floor); a high-loaded PIE heap (`brk_start >= USER_MMAP_BASE`, since
   `LINUX_PIE_BASE ≈ 93 TiB` sits above the 384 GiB mmap window) is capped at
   `LINUX_INTERP_BASE` (the interpreter window floor). This is a coarse but
   always-safe bound — the heap can never grow into the mmap region, the
   interpreter, or the stack — backed by a per-grow `linux_vma_overlap_bytes`
   check as a second guard. RLIMIT_DATA bounds the heap far below this in
   practice.
2. **Committed RLIMIT_AS charge for the full grown virtual span up-front**, even
   though frames are demand-paged. This matches the project's "committed memory
   by default, no silent overcommit" design principle (CLAUDE.md / design.txt):
   a successful `brk` grow reserves the address space against RLIMIT_AS
   immediately; shrink refunds it. The alternative (charge per faulted frame)
   would be overcommit and is rejected by the design spec.
3. **`arch_randomize_brk` gap — 13 bits of entropy (added 2026-06-14).** The
   heap floor is the page-aligned image end shifted up by a random gap, mirroring
   Linux x86_64's `arch_randomize_brk` (`randomize_page(mm->brk, 0x02000000)` =
   8192 = 2^13 distinct positions at 4 KiB pages). Per the entropy-is-the-metric
   principle of decision #20, we match Linux's **13 bits** rather than its 32 MiB
   byte span; at our 16 KiB pages that is a 128 MiB max gap. Implemented as
   `spawn::choose_brk_start(image_end)` reusing the same pure `apply_aslr_base`
   helper as the load bases, always-on when the CSPRNG is seeded with an
   `image_end`-no-gap fallback before seeding (and `image_end == 0` "no heap"
   preserved exactly). The gap is dwarfed by the smallest heap window (a
   low-loaded ET_EXEC has hundreds of GiB up to `USER_MMAP_BASE`), so it never
   meaningfully reduces brk growth room or pushes the floor across `brk_ceiling`.
   Covered by `spawn::self_test`'s `test_brk_aslr_gap` (alignment + in-window over
   ET_EXEC/test/PIE bases) and exercised end-to-end by the ring-3
   `self_test_linux_brk` (which grows/writes/reads against the randomized floor).

**Alternatives considered:**
- *A single fixed ceiling for all images.* Rejected: ET_EXEC and PIE images sit
  on opposite sides of the mmap window, so one constant can't bound both without
  either forbidding ET_EXEC heap growth or letting a PIE heap grow into the mmap
  window. The image-dependent split is the minimal correct rule.
- *Per-faulted-frame RLIMIT_AS accounting (lazy charge).* Rejected: that is
  overcommit, which the design spec forbids. Up-front committed charging is the
  principled choice here.
- *Match Linux's 32 MiB byte span for the brk gap (→ 11 bits at 16 KiB pages).*
  Rejected for the same reason as the load bases: entropy (position count), not
  byte span, is the ASLR metric, so 13 bits is the principled match.

**Tests:** `syscall::linux::self_test_brk_logic` (pure: `brk_round_up`
boundary/overflow, `brk_ceiling` ET_EXEC/PIE/ordering) and the ring-3
`proc::spawn::self_test_linux_brk` (real Linux-ABI process queries its break,
grows 32 KiB, writes a sentinel into the *second* heap frame, reads it back,
exits with it — proving `set_brk_region` at load, the grow path, and
demand-paging of multiple new heap frames).

**How to reverse:** the heap is opt-in per process via `brk_start` — setting it
to 0 (as native images do) makes `sys_brk` a permanent "cannot extend" that
returns the unchanged break, so reverting to stub-like behaviour is a one-line
change at the `set_brk_region` call sites. To change the accounting policy, swap
the `linux_as_charge(added)` call for a per-fault charge in the `VmaKind::Brk`
fault resolver. To disable the randomisation gap, replace the `choose_brk_start`
calls with the bare `image_end` (and drop `BRK_ASLR_BITS`/`choose_brk_start`/
`test_brk_aslr_gap`); to make it opt-out-able, gate it on the same per-process
"no randomise" flag as the load-base ASLR.

---

## 22. File-backed `mmap` — stop at demand-paged `MAP_PRIVATE` (option B); decline the unified page cache (option C); writable `MAP_SHARED` stays `ENOSYS`

> **SUPERSEDED IN PART by §23 (2026-06-14).** The operator reopened Q5 and
> chose to adopt **C-lite** (a unified *read-only* page cache) when a concrete
> consumer appears — see §23. What still stands from §22: full option C and
> **writable `MAP_SHARED` writeback remain declined** (`ENOSYS` indefinitely),
> and option B stays shipped meanwhile. Only the blanket "decline the unified
> page cache" is narrowed: the read-only unified cache is now planned (deferred),
> the writable-shared half is not.

**Date:** 2026-06-14

**Decided by:** Operator (this was `open-questions.md` Q5; Claude built option B
autonomously — see §17 — and laid out the option-C fork; the operator declined
C). The operator's words: *"I guess A is the right option, since C is so hairy
and the only advantage appears to be saving memory for some linux programs and
our OS isn't supposed to be primarily a Linux system anyway and doesn't have
full Linux support."* The operator's intent is to **not build the big page-cache
fork (C)**; Claude kept the already-shipped demand-paged `MAP_PRIVATE` (B) rather
than reverting literally to eager-copy (A) — see "Interpretation" below.

**Context:**
File-backed `mmap(2)` had three candidate end-states (full detail in §17 and the
retired Q5 entry):
- **A.** eager private-copy (the pre-2026-06-14 status quo): allocate + `read_at`
  every frame at map time.
- **B.** demand-paged `MAP_PRIVATE` via `VmaKind::FileBacked` (shipped
  autonomously on 2026-06-14, §17): no frames up front, one page read per fault,
  copy-on-fault for private writes. Writable `MAP_SHARED` returns `ENOSYS`.
- **C.** a unified, file-identity-keyed page cache with demand paging +
  dirty-tracking + `msync`/`munmap` writeback — the real Linux model, enabling
  true cross-process shared writable maps and frame dedup, but a large
  multi-subsystem effort requiring a new stable VFS file-identity (`FileMeta.ino`
  is 0 for memfs/FAT today) and a double-cache-vs-unify call against the block
  buffer cache (`fs/cache.rs`).

**Decision — stop at B; do not build C.**
The unified page cache (C) and writable-`MAP_SHARED` writeback (gap 2 of
known-issues.md TD22) will **not** be built. Writable `MAP_SHARED` of a regular
file continues to return `ENOSYS` indefinitely. The shipped demand-paged
`MAP_PRIVATE` path (B) stays as the file-backed `mmap` implementation.

**Rationale (operator's):**
- C is a large, hard-to-reverse, multi-subsystem effort whose principal payoff is
  memory savings for *some* Linux programs plus writable shared file maps.
- Slate OS is **native-first** and does not aim for full Linux support, so paying
  C's complexity to chase complete `mmap` fidelity for Linux binaries is not
  worth it. `ENOSYS` for writable `MAP_SHARED` is an acceptable permanent answer
  for the Path Z target.

**Interpretation — why B is kept rather than a literal revert to A.** The
operator labelled the choice "A," but the reasoning was entirely about avoiding
**C's** hairiness, not about disliking B; the one advantage the operator named —
"saving memory for some programs" — is precisely what B *already delivers* for
`MAP_PRIVATE`, at low complexity, and B is already shipped, tested, correct, and
reversible. Reverting to literal eager-copy A would *discard* that memory saving
and replace working code with a strictly worse model — the opposite of the
operator's stated value ("saving memory"). So B (no C) is the faithful execution
of the operator's intent: no big page-cache project, keep the cheap demand-paging
win. **If the operator actually wants eager-copy A restored, it is a documented
one-spot revert** (drop the `FileBacked` arm, re-point `linux_file_mmap`'s
`MAP_PRIVATE` path at the eager loop — see §17 "How to reverse").

**Consequences:**
- **known-issues.md TD22** moves from PARTIAL to **CLOSED (won't-fix for gap 2)**:
  gap 1 (lazy population) is done via B; gap 2 (writable `MAP_SHARED` +
  cross-process coherence) is declined by operator decision.
- No stable VFS file-identity, no page cache, and no `fs/cache.rs` unify/double
  decision are needed.

**How to reverse:** if a concrete consumer ever needs writable `MAP_SHARED` or
cross-process file-map coherence, reopen the C fork (the three sub-questions in
the retired Q5 entry — gap-2 worth it?, double-cache vs unify?, stable
file-identity? — are the starting point). B's `VmaKind::FileBacked` fault-path
shape is already the right foundation for C; C would only change the *source* of
each page (page cache vs direct `read_at`) and add the shared/dirty policy.

**Where it lives:** same surface as §17 — `kernel/src/mm/vma.rs`
(`VmaKind::FileBacked`), `kernel/src/syscall/linux.rs` (`linux_file_mmap`),
`kernel/src/proc/pcb.rs` (handle lifecycle), `kernel/src/fs/procfs.rs` (maps
label). The declined C surface would additionally have touched
`kernel/src/fs/vfs.rs` (file-identity) and `kernel/src/fs/cache.rs`.

## 23. File-backed `mmap` (reopened) — adopt **C-lite** (a unified *read-only* page cache) when a concrete consumer appears; writable `MAP_SHARED` writeback stays declined

**Date:** 2026-06-14

**Decided by:** Operator (this reopened `open-questions.md` Q5; Claude proposed
the **C-lite** middle option — read-only cross-process page sharing without the
writable-`MAP_SHARED` writeback machinery — and the operator chose it). The
operator's words: *"Q5: yes, we'll go with C-lite, but if you don't want to
implement it now, document it wherever at what time we should implement it
later."* This **narrows §22**: §22's blanket "decline the unified page cache" no
longer holds — the *read-only* half is now planned (deferred). Everything else in
§22 stands: full option C and **writable `MAP_SHARED` writeback remain declined
indefinitely** (`ENOSYS`), and the shipped demand-paged `MAP_PRIVATE` path
(option B, §17) stays as-is meanwhile.

**What "C-lite" is.** A unified *read-only* page cache: pages of a file are
cached once and shared (read-only) across every process that maps or reads them,
giving two wins —
1. **Shared-library / read-only text dedup:** N processes mapping the same
   `libc`/`.text` share one set of physical frames instead of N copies.
2. **De-double-caching:** a file's pages live in one cache rather than being held
   both by the block buffer cache (`fs/cache.rs`) and per-mapping copies.

**What C-lite deliberately OMITS** (and why it's "lite"): the writable
`MAP_SHARED` path — dirty-page tracking, `msync`/writeback ordering, and
cross-process write coherence. That is the hard, hard-to-reverse half and it
stays declined (writable `MAP_SHARED` of a regular file keeps returning
`ENOSYS`, exactly as in §22). C-lite is read-only, so it needs no dirty/writeback
policy at all.

**Decision — implement later, not now.** Per the operator, C-lite is *adopted in
principle* but **not to be built immediately**. It is deferred until a concrete
consumer needs it.

**Trigger to implement (the "at what time" the operator asked for):** build
C-lite when the **first real consumer of cross-process read-only page sharing
appears** — in practice the **dynamic linker wanting shared-library `.text`
dedup** (multiple processes mapping the same `.so`). That is the moment the
memory-saving payoff becomes concrete rather than hypothetical. A secondary
trigger is any measured double-caching cost once the block buffer cache and
file-backed mappings are both heavily exercised.

**Precursor work that must land first.** C-lite needs **stable VFS file
identity** — a page cache is keyed by (file-identity, offset), and today
`FileMeta.ino` is `0` for memfs and FAT, so two mappings of "the same file"
cannot be recognised as such. Implementing stable inode/file-identity in
`fs/vfs.rs` is a prerequisite and should be scheduled as the first sub-task when
the trigger fires.

**Rationale (both sides):**
- *For deferring:* no consumer exists yet (the dynamic linker doesn't dedup
  shared text today), the precursor (stable file-identity) is itself a
  multi-file change, and building a cache with no client risks designing to the
  wrong access pattern. Slate OS is native-first, so the urgency is low.
- *For adopting (vs §22's full decline):* the read-only half is the *cheap,
  reversible, high-value* slice — it captures the one advantage the operator
  cared about ("saving memory for some programs") for the common shared-library
  case, without taking on the writable-shared hairiness that §22 rightly
  declined. C-lite is the Pareto-optimal point between B and full C.

**Consequences:**
- **known-issues.md TD22** reverts from "CLOSED (won't-fix gap 2)" to: **gap 1
  done (option B); gap-2 read-only sharing PLANNED (deferred, see §23); gap-2
  writable `MAP_SHARED` writeback won't-fix (`ENOSYS`).**
- A deferred-with-rationale entry is recorded in `todo.txt` with the trigger
  condition above.

**Where it will live:** `kernel/src/fs/vfs.rs` (stable file-identity precursor),
a new/extended page cache (likely unifying or fronting `kernel/src/fs/cache.rs`),
`kernel/src/mm/vma.rs` (`VmaKind::FileBacked` fault path sources pages from the
cache), `kernel/src/syscall/linux.rs` (`linux_file_mmap`). B's `FileBacked`
fault-path shape is already the right foundation — C-lite only changes the
*source* of each page (shared cache frame vs per-mapping `read_at`) and marks
shared frames read-only/refcounted.

## 24. Cross-process memory introspection — keep channel/shared-memory IPC for *consensual* sharing; add a **debug-capability-gated** `process_vm_readv`/`writev` for *unilateral* introspection

**Date:** 2026-06-14

**Decided by:** Operator (this was `open-questions.md` Q6). The operator's words:
*"Q6: Yes, keep the existing IPC and add a debug-capability-gated ability to read
all of another process' memory."*

**The two-mechanism split (operator-confirmed):**
1. **Consensual** cross-process memory sharing → the **existing channel +
   shared-memory IPC** path, unchanged. Both parties opt in; no special right is
   needed because the owner of the memory chooses to share it.
2. **Unilateral** introspection (one process reading/writing another's memory
   *without the target's cooperation*, à la `process_vm_readv`/`writev` and, in
   future, `ptrace`) → gated by a **debug capability the caller holds over the
   specific target process**, never derived from ambient PID/uid authority.

**What was implemented this turn:**
- **`Rights::DEBUG`** (bit 17) added in `kernel/src/cap/rights.rs` — the
  unilateral-introspection authority, carried on a
  `ResourceType::Process` capability whose `resource_id` is the target PID.
  Delegation stays AND-mask (a holder can only pass on a subset), so debug
  authority can only flow parent→child or from a privileged debugger broker —
  never be conjured from PID/uid.
- **`process_vm_impl`** (`kernel/src/syscall/linux.rs`): the cross-address-space
  arm — previously a hard `ESRCH` rejection — now checks
  `pcb::has_capability_for(caller, Process, target_owner, Rights::DEBUG)`. No
  cap → **`EPERM`** (mirrors Linux `ptrace_may_access` denial); target gone /
  no PML4 → **`ESRCH`**. With the cap, the copy loop routes the remote side
  through `mm::user::copy_from_user_as` (read / `readv`) or
  `copy_to_user_as` (write / `writev`), preserving Linux's best-effort
  partial-copy contract.
- **`DEBUG` gates both read and write.** A debug capability is total
  introspection authority — real debuggers poke memory as well as read it — so a
  single right covers `readv` and `writev` rather than splitting them.
- Self-test `self_test_process_vm_cross_as` (registered in `main.rs`) covers the
  gate predicate (false with no cap / read-only cap / wrong pid; true after
  `DEBUG` granted) and the remote read/write transfer mechanism end-to-end via
  HHDM verification.

**Why a capability and not a PID/uid check.** Slate OS is capability-based with
no ambient authority (CLAUDE.md architectural rule). "Same uid may ptrace" is
exactly the ambient-authority model the design forbids. Routing unilateral
introspection through an explicit, delegable, AND-mask-narrowable `DEBUG` right
on a specific `Process` capability is the native-correct expression of "X may
debug Y."

**Deferred follow-up:** `ptrace` itself (breakpoints, single-step,
register access, signal-delivery interception) still returns `EPERM`/`ENOSYS`;
when it is built it will gate on the same `Process`+`DEBUG` capability. Logged in
`todo.txt`.

**Where it lives:** `kernel/src/cap/rights.rs` (`Rights::DEBUG`),
`kernel/src/syscall/linux.rs` (`process_vm_impl`, `self_test_process_vm_cross_as`,
`sys_process_vm_readv` doc), `kernel/src/main.rs` (self-test registration),
`kernel/src/mm/user.rs` (`copy_from_user_as`/`copy_to_user_as`, the purpose-built
cross-AS primitives).

---

## 25. Path Z libc + rootfs — go straight to **glibc** on an **ext4** rootfs (no musl stepping-stone)

**Date:** 2026-06-14

**Decided by:** Claude (operator-delegated). The operator left the call to Claude
(Q8) with an explicit standing preference: *"I prefer doing all the hard work
upfront over doing extra labor just to make to reach a milestone quicker by doing
scaffolding … whichever one you think will be more efficient in the long run."*
Claude had previously *recommended* the musl-first staged approach (option C in
`open-questions.md` Q8); on reconsideration against the operator's preference and
the current state of the loader, Claude reversed that recommendation and chose
glibc-direct. This is Claude's to revisit; the operator may overrule.

**Context:**
Path Z (run *prebuilt* Linux toolchain binaries on the Linux-ABI layer — Q4,
operator-prioritized) is fully built and proven for **static** binaries
end-to-end (`proc::spawn::self_test_linux_file_mmap`, `self_test_linux_brk` spawn
real ring-3 Linux-ABI processes). The one documented blocker for **dynamic**
execution (`roadmap.md` line 5089) is that there is no real libc + `ld.so` on a
real on-disk filesystem yet. Choosing the libc steers a large amount of
subsequent ABI-compat work and is costly to reverse, so it was deferred to the
operator (Q8) and delegated back to Claude.

**Decision:**
- **libc: glibc.** Bring up the dynamic-execution path directly against glibc
  (`ld-linux-x86-64.so.2` + `libc.so.6` + friends) — the libc the prioritized
  prebuilt distro toolchain (GCC/binutils/CMake/Make, Q3) is actually linked
  against. No intermediate musl bring-up.
- **rootfs: ext4.** Populate a real ext4 image with the libc tree, per the
  design's *"ext4 first"* rule, replacing the current FAT-only test image as the
  vehicle for the Linux-ABI root. (FAT image stays for the FAT driver self-test.)

**Rationale:**
- The operator's standing preference is to do the hard work upfront and avoid
  throwaway scaffolding. musl-static-first is precisely "extra labor to reach a
  milestone quicker via scaffolding": it would require building/sourcing a musl
  rootfs and debugging musl-specific `ld-musl`/ABI quirks that are then discarded
  for the real glibc target.
- The de-risking value that originally motivated the musl-first recommendation
  has largely been *spent*: the static-load path is already validated end-to-end,
  so the only incremental thing musl-*dynamic* would prove cheaply is the
  dynamic-linker machinery in isolation. That benefit is real but modest, and it
  does not transfer to glibc (glibc still needs its own ABI-surface debugging).
- The shared infrastructure (ELF dynamic loading, relocation processing, TLS
  setup, `ld.so` invocation) must be built regardless and is the bulk of the
  work; building it directly against the real target avoids a duplicated rootfs
  setup with no proportional payoff.
- Net: glibc-direct matches the operator's preference and, given the static path
  is already proven, is at worst a wash on long-run efficiency while saving a
  full second rootfs/ABI bring-up.

**Alternatives considered:**
- **musl static-first, then glibc (Claude's original Q8 recommendation, option
  C).** Cheapest path to *a* real compiled binary and isolates dynamic-linker
  bugs from glibc's large ABI surface — but duplicates rootfs setup, adds
  throwaway musl-specific debugging, and its de-risk value is small now that the
  static path is proven. Rejected per operator preference + small marginal value.
- **musl only.** Rejected: most prebuilt distro toolchain binaries (the actual
  Path-Z target) are glibc-linked, so musl proves the loader but never runs the
  prioritized binaries.

**Risk accepted:** glibc is a much larger first-light bring-up (TLS edge cases,
`__libc_start_main`, vDSO, NSS, locale, many more syscalls/`ioctl`s) hit all at
once with no musl intermediate. If glibc cold-bring-up proves to be a long,
hard-to-bisect debug cycle, a *minimal* musl-static smoke test remains available
as a fallback diagnostic to isolate dynamic-linker bugs from glibc-ABI gaps —
this decision does not preclude that, it only declines to make musl the planned
first milestone.

**Where it lives:** `scripts/create-disk.py` (rootfs build — currently FAT test
image only; needs an ext4 image populated with the glibc tree),
`kernel/src/proc/spawn.rs` (`load_interpreter`, the `ld.so` entry path),
`kernel/src/elf.rs` (`interp_path`/`load_segments_with_bias`),
`kernel/src/syscall/linux.rs` (further ABI gaps glibc will exercise), and the
ext4 mount/root path. `roadmap.md` line 5089.

**How to reverse:** the libc/rootfs choice is localized to the rootfs builder and
the on-disk libc tree; switching to musl would mean swapping the `ld-musl`/libc
files into the rootfs and chasing musl-specific ABI quirks. The loader plumbing
itself is libc-agnostic, so reversal cost is dominated by rootfs rebuild +
re-validation, not kernel code.

---

## 26. Kernel-stack-vs-IRQ overflow (B-DF1 / Q7) — per-CPU IRQ stack with manual nesting-aware switch (option A)

**Date:** 2026-06-15

**Decided by:** Operator (this was `open-questions.md` Q7; the operator chose
"option A". Claude had recommended option A as the proper production-grade fix —
"Q7: option A"). This is settled policy, not Claude's to silently revisit.

**Context:**
Hardware IRQs (vectors 32–56, plus the 251/252/255 APIC IPIs) were configured
with IDT IST index 0, meaning the CPU does **not** switch stacks on entry — the
interrupt frame is pushed onto whatever stack the interrupted code was using.
Heavy in-kernel code (gzip/deflate, `format!`-driven JSON/HTML in the in-kernel
HTTP dashboard, crypto) running on a near-full 64 KiB kernel **task** stack could
push the next timer/mouse IRQ frame into the guard page → unrecoverable double
fault (B-DF1). The 16 KiB gzip stack array was fixed earlier, but the underlying
"an IRQ frame overflows a near-full task stack" problem was systemic.

**Decision (option A):**
- **Dedicated per-CPU IRQ stack**, guard-page-backed (allocated from the kstack
  allocator so an IRQ-stack overflow still faults cleanly on a guard page).
  Installed per CPU before that CPU's first `sti` (`idt::init_irq_stack` from
  `kernel_main` for the BSP and `ap_entry` for APs).
- **Manual (software) stack switch in the IRQ entry path**, not hardware IST.
  `irq_common_dispatch` switches RSP to the IRQ stack only on the **outermost**
  IRQ (detected by the current RSP *not* already lying in the IRQ-stack range);
  a nested IRQ keeps growing down the same IRQ stack. This is the key reason for
  *not* using hardware IST, which unconditionally resets RSP to the IST top on
  every interrupt and would clobber an outer handler's frame when the timer
  re-enables interrupts mid-handler for preemption.
- **Deferred preemption.** The context switch a preemption performs must record
  the **task** stack's RSP as the resume point, never the transient IRQ-stack
  RSP. So the timer ISR no longer calls `preempt()` inline; it sets a per-CPU
  `NEED_RESCHED` flag (`request_preempt`), and the outermost IRQ frame services
  it via `do_deferred_preempt()` *after* RSP is back on the task stack.

**Recursion fix (exposed by the restructuring, not a separate option):**
The deferred `do_deferred_preempt → preempt → schedule_inner` runs on the task
stack with interrupts enabled (the timer ISR `sti`s so the outgoing task is
saved with IF=1). A nested timer tick during `schedule_inner` has RSP on the
task stack — outside the IRQ-stack range — so it was misclassified as a fresh
*outermost* IRQ and re-entered the preempt path, recursing one ~2 KiB frame at a
time until the task stack overflowed its guard page (#DF at `schedule_inner+0x11`).
**Fix:** `do_deferred_preempt` disables interrupts (`cli`) across the involuntary
switch and re-enables (`sti`) immediately after `preempt()` returns. The outgoing
task is saved with IF=0 but is *always* resumed at that very `sti` (and the IRQ
stub's `iretq` restores IF=1 from the saved frame regardless), so interrupts are
never permanently lost; voluntary yields (which never take this path) still run
and save with IF=1, preserving the per-task RFLAGS invariant.

**Rationale (vs. the rejected options):**
- **B (just bump the task stack, 64→128 KiB):** rejected as a band-aid — an IRQ
  can still overflow a sufficiently deep stack, and it costs committed memory per
  task. (A 128 KiB *debug-only* bump was in fact tried as a stop-gap and
  *disproved the capacity hypothesis*: the overflow filled the **entire** stack
  at both 64 KiB and 128 KiB, which is what localized the real cause to the
  unbounded preempt recursion above. The bump was reverted.)
- **C (move heavy code to userspace):** correct microkernel direction long-term
  but large effort and doesn't help legitimately-deep in-kernel paths.
- **D (release-build the boot tests):** sidesteps the symptom without fixing the
  bug; diverges test build from the debug workflow.
- **A** bounds interrupt stack use independently of task-stack depth and fixes
  the whole class of bug (Linux's IRQ-stack model), at the cost of a careful
  change to the hottest, most safety-critical path — which is why it needed the
  operator's explicit go-ahead.

**Validation:** `http_gzip_8KiB` (the bench that previously double-faulted at the
gzip→dashboard transition on a near-full task stack) now runs to completion under
QEMU with the IRQ stack + deferred-preempt + recursion fix in place.

**Where it lives:** `kernel/src/idt.rs` (`init_irq_stack`, `run_on_irq_stack`,
`irq_common_dispatch`, the single vector-passing IRQ stub macro, `IRQ_STACK_TOP`/
`IRQ_STACK_BOTTOM`), `kernel/src/apic.rs` (`handle_timer_irq` → `request_preempt`
instead of inline `preempt`), `kernel/src/sched/mod.rs` (`NEED_RESCHED`,
`request_preempt`, `do_deferred_preempt` with the `cli`/`sti` recursion guard),
`kernel/src/main.rs` + `kernel/src/smp.rs` (`init_irq_stack` before each CPU's
`sti`), `kernel/src/mm/kstack.rs` (`STACK_FRAMES` now derived from
`task::TASK_STACK_SIZE`), `kernel/src/sched/task.rs` (`TASK_STACK_SIZE` back to a
single 64 KiB value).

**How to reverse:** drop the manual switch in `irq_common_dispatch` (run handlers
directly on the task stack) and revert `handle_timer_irq` to call `preempt()`
inline with the old idle/softirq guards; the deferred-preempt flag and the
`cli`/`sti` guard would go with it. Reversal is mechanical but reintroduces
B-DF1.

## 27. Deferred preemption must not block on the scheduler lock — skip-and-re-arm vs IRQ-off SCHED

**Date:** 2026-06-15

**Decided by:** Claude (autonomous). Correctness fix on the scheduler hot path,
discovered while driving the deferred benchmark suite to `BENCH_OK` after the Q7
landing (§26). Not a user-visible policy; mine to revisit if a better approach
appears.

**Context:**
With Q7's deferred preemption (§26), the only place an *involuntary* context
switch is initiated is `sched::do_deferred_preempt` → `preempt()` →
`schedule_inner()`, which takes `SCHED.lock()` (a plain `spin::Mutex`, **no**
interrupt masking). If a timer tick lands while the running task is *itself*
holding `SCHED`, the deferred preempt re-enters `SCHED.lock()` on the same CPU
and spins forever — the interrupted frame can never release the lock. The `cli`
added in §26 makes the hang unrecoverable. `bench_dashboard_api_status` is the
reliable reproducer: `api_status()` → `task_list()` holds `SCHED` across a heap
`Vec` collect over all tasks, run 1000× in a tight loop, so a tick almost
certainly lands inside a hold. The same hazard is a *latent* (tiny-window) risk
for every voluntary `SCHED` holder (`yield_now`, `block_current`), which also run
`schedule_inner` with interrupts enabled.

**Options considered:**
- **(A) Make `SCHED` an IRQ-safe lock** (acquire with interrupts disabled, like
  Linux's `rq->lock`). Most thorough: a timer can't fire mid-hold at all. But
  it is a sweeping change to ~40 lock sites and the context-switch path, forces
  every `SCHED` critical section (incl. `task_list`'s heap collect) to run with
  interrupts off (interrupt-latency cost), and *still* leaves the tiny
  SCHED-released-but-mid-`switch_context` window unprotected unless the lock is
  also held *through* the switch (a much larger restructuring). High risk on the
  safety-critical path for a benchmark-exposed bug.
- **(B) Per-CPU `preempt_count`** (Linux model): bracket every `SCHED` section
  with `preempt_disable`/`enable`, preempt only at count 0. Correct and general
  but the most invasive (touches all 40 sites; easy to miss one).
- **(C, chosen) Skip-and-re-arm in `do_deferred_preempt`.** Before preempting,
  check `SCHED.is_locked()`; if held, re-arm `NEED_RESCHED` and return, deferring
  the switch to the next tick. SCHED holds are short, so the preemption simply
  lands on a later tick where the task isn't holding the lock.

**Why C:**
- It fixes the **entire** "involuntary preempt while the interrupted context
  holds SCHED" deadlock class at the **single** point where involuntary
  preemption is initiated — including the latent voluntary-yield window — without
  touching any of the 40 `SCHED.lock()` call sites or the hot switch path.
- It is **consistent with an established pattern in this codebase**:
  `unthrottle_expired()` already uses `SCHED.try_lock()` and bails "because this
  runs in the timer ISR context." `do_deferred_preempt` services a flag the timer
  ISR set, so the same try/skip discipline is the natural fit.
- Preemption is inherently **best-effort/deferrable**: missing one tick's
  preemption because the scheduler lock is momentarily busy costs at most ~10 ms
  of extra runtime for the current task and is retried immediately on the next
  tick. There is no fairness or correctness loss.
- Imprecision is benign: `spin::Mutex::is_locked()` can't tell "held by this
  CPU's interrupted task" from "transiently held by another CPU." We
  conservatively skip in both cases. A cross-CPU false skip is just one deferred
  preemption — never a deadlock, since the other CPU *will* release the lock.

**Risks / tradeoffs:**
- Under sustained pathological `SCHED` contention a CPU could defer preemption
  for several ticks. In practice `SCHED` sections are short by design (§26's
  "single lock acquisition for the switch"); the bench's 1000-iter `task_list`
  loop still made full forward progress and the task was preempted normally
  between holds.
- This does **not** convert `SCHED` to IRQ-off, so interrupt-latency behavior is
  unchanged (a plus here). If a future need arises to hold `SCHED` across longer
  work, revisit option A/B.

**Validation:** with the guard in place the full `--bench` suite runs to
completion — `dashboard_api_status/health/metrics`, `isr_latency`, the scorecard,
and the `BENCH_OK` marker all appear ("Boot test PASSED"). Before the guard, the
suite hard-hung the moment it entered `bench_dashboard_api_status`.

**Where it lives:** `kernel/src/sched/mod.rs` `do_deferred_preempt` (the
`SCHED.is_locked()` skip-and-re-arm guard, ahead of the `cli`/`preempt`/`sti`
sequence).

**How to reverse:** delete the `if SCHED.is_locked() { … return; }` guard. This
reintroduces the deadlock for any involuntary preempt that lands while the task
holds `SCHED` (e.g. the dashboard benches), so reversal should only accompany a
move to option A or B.

---

## 28. 16 KiB logical frames vs. 4 KiB-ABI glibc binaries — keep the 16 KiB frame as the alloc/RSS/reclaim unit, add 4 KiB-sub-frame permission granularity for mmap/mprotect/ELF load

**Date:** 2026-06-15

**Decided by:** Claude (operator-approved scope). The operator settled the
*destination* — run prebuilt dynamically-linked **glibc** binaries on an **ext4**
rootfs, "Path Z," with no musl stepping-stone (see §25). This entry records the
specific *mechanism* I chose autonomously to get there; it's mine to revisit, but
the goal it serves is operator policy.

**Context:**
Slate uses a **16 KiB logical page/frame** (`FRAME_SIZE = 16384`,
`HW_PAGES_PER_FRAME = 4`) as the design-mandated base page (CLAUDE.md
"Architectural Rules"). But standard x86-64 Linux/glibc binaries are linked with
`max-page-size = 0x1000` (**4 KiB**). Consequently `ld.so`'s `_dl_map_segments`:
- maps adjacent ELF segments with *different* permissions (R-- rodata immediately
  followed by RW- data) on **4 KiB** boundaries that fall *inside* one 16 KiB
  frame, and
- issues `MAP_FIXED` overlays and `mprotect` calls (notably the RELRO step
  `mprotect(…, 0x4000, PROT_READ)`) at **4 KiB** alignment that is *not* 16 KiB
  aligned.

A frame-granular memory subsystem cannot represent "the first 4 KiB of this frame
is read-only, the next 4 KiB is read-write," nor honor a 4 KiB-aligned
`mprotect`/`MAP_FIXED` — which is exactly what broke real-glibc execution
(bss zero-fill overlay → "cannot map zero-fill pages"; RELRO → "cannot apply
additional memory protection"). The hardware already uses 4 KiB PTEs (our 16 KiB
frame = 4 contiguous 4 KiB PTEs), so the capability exists at the HW level; the
question was how to expose it without abandoning the 16 KiB design.

**Options considered:**
- **(A) Switch the OS base page to 4 KiB.** Trivially compatible with stock
  Linux binaries, but violates a core, deliberate architectural decision (16 KiB
  pages chosen for fewer TLB entries / smaller page tables / better large-working-
  set behavior) and would require rebuilding the *entire* memory subsystem around
  4 KiB. Rejected: throws away a foundational design choice to accommodate one
  compatibility path.
- **(B) Relink/patch every Linux binary to 16 KiB max-page-size.** Defeats the
  whole point of Path Z (running *unmodified, prebuilt* distro binaries) and is
  impossible for closed-source blobs. Rejected.
- **(C, chosen) Keep 16 KiB as the allocation/RSS/rmap/reclaim unit; add 4 KiB
  sub-frame *permission and file-backing* granularity** on the demand-fault and
  mmap/mprotect paths only. One physical 16 KiB frame still backs all 4 subpages
  and is still the unit that the allocator, RSS accounting, reverse-mapping, and
  reclaim operate on; but each of its 4 hardware PTEs may carry independent
  permissions and (for file maps) independent backing.

**Why C:**
- It preserves the 16 KiB architecture everywhere it matters for performance
  (allocation, accounting, reclaim, the common single-VMA fast path is
  byte-for-byte unchanged) while exposing exactly the 4 KiB granularity the
  hardware already has and stock binaries require.
- The added cost is paid **only** on the slow paths that actually need it: a
  fault on a frame straddled by >1 VMA, a 4 KiB-granular `mmap(MAP_FIXED)`, or a
  4 KiB-granular `mprotect`. A fault on a frame covered by a single VMA takes the
  original fast path.
- It is the minimal change that makes unmodified glibc work — no other
  subsystem's invariants change.

**Mechanism (where it lives):**
1. **Per-subpage demand faulting** — `pcb::resolve_subpaged_fault` (routed to from
   `pcb::try_resolve_fault` when a faulting frame is straddled by more than one
   VMA): allocates/zeroes one 16 KiB frame, then for each 4 KiB subpage installs a
   PTE with that subpage's covering-VMA permissions and file backing via
   `page_table::map_4k_if_absent`. RSS/rmap/reclaim still key on the 16 KiB base.
2. **4 KiB page-table primitives** — `page_table::change_flags_4k` (flip one leaf
   PTE), `map_4k_if_absent` / `unmap_4k`, `is_hw_page_aligned`, `HW_PAGE_SIZE` /
   `HW_PAGES_PER_FRAME`.
3. **4 KiB-granular anonymous `MAP_FIXED`** — `linux_anon_mmap_fixed` + the
   `sys_mmap` fixed-dispatch path (4 KiB align/round, net RLIMIT_AS charge,
   `unmap_user_range` + `remove_vma_range` of the replaced range, Anonymous VMA).
4. **4 KiB-granular `mprotect`** — `mprotect_validate_args` / `sys_mprotect` gate
   and step on `HW_PAGE_SIZE` and flip individual PTEs via `change_flags_4k`.
5. **Per-subpage ELF segment loading** — `proc/elf.rs` two-pass loader: pass 1
   computes the 16 KiB-frame-aligned span over all biased PT_LOAD; pass 2 maps
   each 16 KiB frame with the **union** of its overlapping segments' permissions
   (preserving W^X), copies the overlapping file bytes, and maps via
   `map_frame_subpages`.

**Validation:** `proc::spawn::self_test_linux_real_glibc` drives a real prebuilt
dynamically-linked glibc `/bin/hello` through the complete ring-3 startup —
`ld.so` maps `libc.so.6`, relocates, sets up TLS, `__libc_start_main → main →
exit_group(42)` — and the boot test reports
`REAL glibc dynamic execution … __libc_start_main → main → exit(42)): OK`
(three BOOT_OK cycles).

**Known limitation (tracked, known-issues.md TD27):** `mprotect` updates PTE
permissions but not the underlying `Vma.flags`, so a page reclaimed under memory
pressure and re-faulted is rebuilt from the *VMA's* (pre-mprotect) permissions —
e.g. a RELRO'd page would come back writable. Benign today (no reclaim path
targets RELRO pages, no swap), becomes live with anonymous swap/general reclaim;
proper fix is per-subpage VMA splitting on `mprotect`.

**How to reverse:** the sub-frame paths are additive — the fast paths and 16 KiB
primitives are untouched — so reverting means dropping the `resolve_subpaged_fault`
routing, `change_flags_4k`/`linux_anon_mmap_fixed`, the 4 KiB `mprotect` stepping,
and the per-subpage ELF loader. That would re-break unmodified glibc, so reversal
should only accompany a different compatibility strategy (A or B).

## 29. Linux signal delivery — byte-exact `rt_sigframe` for `AbiMode::Linux` processes, native SEH-style trampoline for native processes

**Date:** 2026-06-15

**Decided by:** Claude (operator-approved scope). The operator settled the
*destination* — run prebuilt dynamically-linked glibc binaries (Path Z, §25). This
entry records the *mechanism* I chose autonomously for the signal-delivery slice of
that goal; it's mine to revisit, but the goal it serves is operator policy. It also
operates strictly *within* design-decision #4 (the native OS does **not** use Unix
signals for process control — it uses language-level/SEH-style exceptions and IPC),
which remains untouched.

**Context:**
glibc programs install signal handlers via `rt_sigaction` and expect the kernel, on
delivery, to build a Linux `struct rt_sigframe` on the user stack and enter the
handler with the Linux register convention (`rdi=signo, rsi=&siginfo,
rdx=&ucontext`), then to resume via the `rt_sigreturn` syscall reached through the
handler's `sa_restorer` (glibc `__restore_rt`). Slate's *native* signal path instead
delivers a single SEH-style `SignalContext` via a kernel trampoline — a deliberately
different model per design-decision #4. Real glibc binaries previously got the native
`SignalContext` written where they expected a Linux `rt_sigframe` (garbage
siginfo/ucontext) and crashed on return (no `sa_restorer` wired).

**Decision:**
Branch signal delivery on the process's ABI mode. `deliver_pending_signal`
(`handlers.rs`) routes `AbiMode::Linux` processes into `deliver_linux_signal`, which
runs a per-signal-disposition loop and, for caught signals, calls
`linux::build_linux_rt_frame` to lay down a **byte-exact** Linux `rt_sigframe` (256B
`sigcontext_64` + 304B `ucontext` + 128B `siginfo`, in
`kernel/src/proc/linux_sigframe.rs`) using Linux's exact `align_sigframe` arithmetic.
`linux_rt_sigreturn` restores the saved context from the user `ucontext`, with
attacker-controlled RFLAGS sanitized (whitelist `0x0024_0DD5`, force IF + reserved
bit, drop IOPL/NT/VM). Native processes keep the SEH-style trampoline unchanged.
Per-signal disposition uses a per-process `LinuxSigaction` table (not the single
native trampoline pointer), honouring `SA_NODEFER`/`SA_RESETHAND`/`sa_mask`.

**Alternatives considered:**
- **One unified signal frame for both ABIs.** Rejected: it would force the native OS
  onto the Unix `rt_sigframe`/`rt_sigreturn` model, directly contradicting
  design-decision #4. The native exception model is intentionally *not* Unix signals.
- **Translate Linux `rt_sigaction` into the native trampoline and reuse the native
  delivery path.** Rejected: glibc reads/writes the `ucontext` and relies on the exact
  `siginfo` layout and on `sa_restorer`; only a byte-exact Linux frame satisfies
  unmodified glibc. A lossy translation would be a band-aid that breaks on any program
  that inspects `ucontext`/`siginfo`.

**Trade-offs / why this is a real decision:**
The cost is two parallel signal-delivery code paths (native trampoline + Linux
rt_sigframe) keyed on `AbiMode`, which is more surface area than a single unified
path. The benefit is that each ABI gets exactly the contract its programs expect, and
design-decision #4 (native ≠ Unix signals) is preserved. The split mirrors the
existing per-ABI splits already in the tree (SysV stack builder, auxv, brk ceiling),
so it fits the established Path-Z architecture rather than introducing a new pattern.

**Known limitation (tracked):** delivered `siginfo` is stamped `SI_USER`/0/0 because
the pending-signal bitmap doesn't track sender identity (known-issues.md TD29). The
`SI_KERNEL`/`SI_TKILL` constants are reserved for the future sender-faithful path.

**How to reverse:** the Linux path is additive and gated on `AbiMode::Linux` — the
native trampoline is untouched — so reverting means dropping the `deliver_linux_signal`
branch, `build_linux_rt_frame`/`linux_disposition`, and the `linux_rt_sigreturn`
rewrite. That would re-break unmodified glibc signal handlers, so reversal should only
accompany a different Linux-compat signal strategy.

## 30. memfs hard links — leave unsupported (spec-correct EPERM); test `link(2)` on ext4 instead of refactoring memfs to an inode table

**Date:** 2026-06-16

**Decided by:** Claude (autonomous) — reversible; the operator may overrule. Made
while wiring the `link`/`linkat` syscalls (Path Z Part 28) and discovering the
boot root FS (memfs) cannot represent a hard link.

**Context:**
The Linux-ABI `link`/`linkat` syscalls were stale `EROFS` stubs. Wiring them to
`Vfs::link` (the proper syscall-layer fix) exposed that the in-memory root FS
(`/`, `/tmp`) cannot create hard links: memfs is a tree of by-value `MemFsNode`s
where each directory's `BTreeMap<String, MemFsNode>` *owns* its children and a
regular file stores its bytes inline (`MemFsNodeKind::File(Vec<u8>)`). Two
directory entries therefore cannot reference one shared inode — exactly what a
hard link requires (shared data, shared metadata, shared `nlink`). The default
`Filesystem::link` returns `NotSupported`; ext4 (`fs/ext4/vfs_impl.rs`)
implements real hard links.

**Options considered:**
- **(A) Refactor memfs to an inode-table model** — `MemFs` owns
  `BTreeMap<ino, Inode>` (data + metadata + `nlink`), and file/symlink directory
  entries hold an `ino` instead of the body, so multiple names can share one
  inode; `remove` decrements `nlink` and frees on zero. This is the textbook
  design and would make hard links work on the *actual* running root. But it is
  a sweeping rewrite of a core subsystem touching every memfs operation (read,
  write, truncate, metadata, lstat, remove, rename, the directory walk) and the
  many memfs self-tests, with no current consumer demanding it.
- **(A′) Share only file *bodies* via `Rc`/`Arc`** — keep the tree, wrap file
  data (and metadata) in a refcounted cell. Rejected: `Rc` is not `Send`/`Sync`,
  which would poison the global `Mutex<Vfs>` static; `Arc<Mutex<…>>` nests locks
  inside the already-held VFS lock. More complexity than the clean inode table
  for no extra benefit.
- **(B, chosen) Leave memfs returning "unsupported" and test on ext4.** memfs
  reporting no hard-link support is **spec-correct**: Linux `link(2)` returns
  `EPERM` for filesystems that don't support hard links. The `link`/`linkat`
  syscall wiring is complete and correct; it returns the FS's real answer
  (works on ext4, declines on memfs). The Part 28 regression test exercises the
  success path on the ext4 mount at `/mnt`.

**Reasoning:** The roadmap item is *syscall* fidelity (stop being a blanket
`EROFS` stub), which option B fully achieves. ext4 is the design's real root FS
(`ext4 first`), and it supports hard links — so the practically important case
already works. memfs is the diskless/early/`/tmp` fallback; hard links there are
not a real workload requirement today. Doing the large, risky inode-table
refactor speculatively would violate "don't restructure a core subsystem without
a concrete need," and the deferral is cleanly reversible.

**Known limitation (tracked):** hard links are unsupported on memfs-backed paths
(`/`, `/tmp`) — see known-issues.md B-SYM1. The proper fix (inode-table memfs) is
recorded there and here for when a consumer needs it.

**How to reverse:** implement `Filesystem::link` for memfs via the option-A
inode-table refactor; the syscall wiring needs no change (it already delegates to
`Vfs::link`). The Part 28 test could then also run against `/tmp`.

## 31. `access(2)` family semantics — grant F_OK/R_OK/X_OK for any existing file under the no-DAC capability model (consistent with `execve` ignoring x-bits)

**Date:** 2026-06-16

**Decided by:** Claude (autonomous) — reversible; the operator may overrule. Made
while wiring the `access`/`faccessat`/`faccessat2` syscalls (Path Z Part 34) to
get unmodified GNU `make` running, after `strace` showed make calls
`access(shell, X_OK)` before spawning a recipe and bails on failure.

**Context:**
`sys_access`/`sys_faccessat`/`sys_faccessat2` were stale stubs returning
`ENOENT` unconditionally (a "we have no backing filesystem" skeleton from before
the VFS was writable). With a real VFS this is simply wrong: every existence and
accessibility probe failed. GNU make issues `access("/bin/sh", X_OK)` *before*
spawning its recipe shell and, on failure, reports `"/bin/sh: No such file or
directory"` + `Error 127` without spawning — so the stub blocked the entire
toolchain initiative.

The open question once the path is resolved through the VFS: what does `X_OK`
(and `W_OK`) *mean* in an OS whose security model is capability-based, not Unix
DAC?

**Options:**
- **A — Faithful Unix mode-bit check.** Read the file's mode bits and grant
  X_OK only if an execute bit is set, W_OK only if a write bit is set, etc.
  *Con:* our memfs/FAT report `permissions == 0` (no Unix bits), so a faithful
  X_OK on a staged binary would *fail* even though `execve` of that same binary
  *succeeds* (execve ignores on-disk x-bits here). That inconsistency is worse
  than no check: `access(X_OK)==fail` then `execve==ok` breaks make's own logic.
- **B (chosen) — No-DAC: existence implies F_OK/R_OK/X_OK.** Resolve the path;
  if it exists, grant read/execute. This is *consistent* with what a subsequent
  `execve`/`open` actually does in this OS (authority comes from capabilities,
  not file mode bits). `W_OK` is granted unless the backing FS is known
  read-only (then `EROFS`, as on Linux).

**Reasoning:** Authority in this OS is conferred by capabilities, not by file
owner/group/other permission bits — and `execve` already ignores the on-disk
execute bits. The *only* self-consistent answer for `access(X_OK)` is therefore
"grantable iff the file exists," matching the `execve` that the caller is about
to perform. Option A would make `access` and `execve` disagree, which is exactly
the failure mode that breaks real programs. The check still resolves the path
through the VFS, so a *missing* file correctly returns `ENOENT`.

**Known limitation (tracked):** `W_OK` does not yet consult per-mount read-only
state (we don't track it at this layer), so writes are always granted — see
known-issues.md. A read-only mount should return `EROFS` for `W_OK`.

**How to reverse:** if a real per-user/per-mode policy is ever needed, gate
`X_OK`/`W_OK` on the actual mode bits (and a future read-only-mount flag) in
`access_path_common`; the path-resolution plumbing stays unchanged.

## 32. Real `PROT_NONE` — represent "no access" as the absence of the `USER_ACCESSIBLE` page-table flag (overload the existing flag), not a new VMA field

**Date:** 2026-06-17

**Decided by:** Claude (autonomous) — clearly-correct, low-controversy mirror of
the x86-64 hardware mechanism; reversible. NOTE: this **diverges** from the lean
in the task scouting note (`todo.txt` NEXT STEPS #3), which suggested adding a
*dedicated* VMA field/flag for the access mask "rather than overloading
PageFlags." On full inspection the overload is the cleaner design (reasoning
below), so I took it.

**Context:**
Before this change, `mmap(PROT_NONE)` and `mprotect(..., PROT_NONE)` were
approximated as "read-only + no-execute": the VMA still carried
`PRESENT | USER_ACCESSIBLE | NO_EXECUTE`, so a *read* of the region demand-paged
a zero frame instead of faulting. That is wrong for the two things `PROT_NONE` is
actually used for — guard pages and reserved trap regions (notably glibc's
thread-stack guard, and `mmap(PROT_NONE)`-then-`mprotect(RW)` reservation
patterns). With full per-process VMA tracking now in place, the fault resolver
can distinguish "mapped `PROT_NONE`" from "never-mapped hole," so real
`PROT_NONE` is implementable.

`PROT_NONE` has to be enforced at **two layers**, because a region can be either
already-faulted-in (present PTEs) or still lazy (no PTE yet):
1. **Present pages** — hardware will only fault a ring-3 access if the PTE lacks
   the U/S (`USER_ACCESSIBLE`) bit. So `mprotect(PROT_NONE)` on present pages
   *must* clear `USER_ACCESSIBLE` on the PTE regardless of how the VMA records
   the protection — there is no way around touching the page-table bit.
2. **Lazy pages** — the fault resolver consults the covering VMA to decide
   whether to populate the page or fault. The VMA must record "no access."

**Options for the VMA-layer marker (layer 2):**
- **A (chosen) — Overload `USER_ACCESSIBLE`:** a `PROT_NONE` VMA carries flags
  *without* `USER_ACCESSIBLE` (e.g. `PRESENT | NO_EXECUTE`). The resolver treats
  "user fault on a VMA whose flags lack `USER_ACCESSIBLE`" as unresolvable →
  `KernelError::PageFault` → SEH-style access violation.
- **B — Dedicated `prot_none: bool` (or a full R/W/X access mask) on `Vma`:** an
  explicit second field, separate from `PageFlags`.

**Reasoning (why A):**
- **Single source of truth.** Layer 1 *forces* us to use the U bit on present
  PTEs anyway. Option A makes the VMA use the *same* bit, so the lazy-page marker
  and the present-page enforcement are one representation. Option B introduces a
  second marker that must be kept in sync with the PTE U bit — a classic
  divergence bug waiting to happen.
- **It is the literal hardware semantic, not a hack.** "Userspace cannot access
  this page" *is* the U/S bit. `PROT_NONE` ⇔ no user access ⇔ `!USER_ACCESSIBLE`.
- **Zero construction-site churn.** `Vma` has 26 struct-literal construction
  sites; option B would touch all of them. Option A touches only the handful of
  places that actually *care* about access (the mmap flag build, the resolver
  gate, `mprotect`, and `/proc/<pid>/maps` perm rendering).
- **CoW stays correct for free.** `cow::resolve_cow_fault` derives the copied
  PTE's flags from the existing PTE (`sibling.flags | WRITABLE`, minus `COW`);
  since it never *adds* `USER_ACCESSIBLE`, a CoW of a `PROT_NONE` page stays
  inaccessible — a write to a forked-then-`PROT_NONE`'d page cannot escape.
- **`mprotect` round-trip restores access.** Present `PROT_NONE` pages keep
  `PRESENT` (only U is cleared), so the physical frame and its contents survive;
  `mprotect` back to `PROT_READ|WRITE` re-adds `USER_ACCESSIBLE` and the data is
  intact — no frame leak, no re-zero.

**The one subtlety this forces:** `pcb::protect_vma_range` previously took only
`(want_write, want_exec)`, but `PROT_NONE` and `PROT_READ` are *both*
`want_write=false, want_exec=false` — indistinguishable. So `protect_vma_range`
and the `mprotect` PTE pass gained a `want_access` parameter (true unless
`prot == PROT_NONE`); when false they clear `USER_ACCESSIBLE`, when true they
set it. This is unavoidable under *either* option (the VMA marker has to be told
which of the two zero-prot cases it is).

**Scope kept to user space.** Only `pcb::try_resolve_fault` (the per-process
resolver) gained the `!USER_ACCESSIBLE ⇒ fault` gate. The kernel global-address-
space resolver (`mm::vma::AddressSpace::resolve_fault`) is deliberately **not**
gated: kernel pages legitimately lack `USER_ACCESSIBLE`, and there is no
`PROT_NONE` concept there.

**How to reverse:** if a richer access model is ever needed (e.g. separate
read-vs-execute-only distinctions the U bit can't express, or pkeys), add the
explicit access mask to `Vma` then; the resolver gate and the `mprotect`
`want_access` plumbing are the only call sites that would change.

## 33. Bare-ELF ABI auto-classification (Q9) — Hybrid (option D): default unmarked bare ELF → Linux, note-walk as a positive Linux signal, stamp native binaries with an explicit SlateOS marker

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q9; the operator chose
option **D**, which Claude recommended). The operator's words: *"Q9: Let's do
with D."*

**The decision.** Resolve the bare-static-ELF ambiguity (a `SYSV` static binary
carrying only generic GNU-toolchain artifacts is genuinely indistinguishable
between "Linux binary" and "SlateOS-native binary built with a GNU/LLVM
toolchain") with the **hybrid** approach:
1. **Flip the default for unmarked bare ELFs to Linux ABI.** Any ELF with no
   positive native marker is treated as Linux — every real-world Linux static
   binary (`tcc -nostdlib -static`, static musl, hand-rolled asm) "just works".
2. **Add `NT_GNU_ABI_TAG` note-walking** as an additional *positive* Linux signal,
   on top of the existing `EI_OSABI == ELFOSABI_GNU` / Linux `PT_INTERP` /
   `PT_GNU_PROPERTY` markers.
3. **Stamp SlateOS-native binaries with an explicit marker** — a SlateOS
   `EI_OSABI` value in the architecture range 64–255 and/or a `.note.slateos`
   `PT_NOTE`. Native is the side we fully control and can always mark; Linux is
   the open-world default.
4. **Keep `spawn_process_with_abi(elf, options, AbiMode)`** as the override for
   callers that already know the ABI.

**Rationale (both sides).** *For D:* native binaries are produced exclusively by
our own toolchain, so marking them is cheap and unambiguous; Linux binaries
arrive from the outside world unmarked, so the default should be the side we
can't mark — makes "a Makefile builds a tool with tcc then `exec`s it" work
transparently (central to the Path-Z toolchain goal). *Against / cost:* a
user-visible policy flip; the native toolchain must emit the marker, and existing
bare native test ELFs (`build_test_elf`) need it added, or a truly unmarked
native binary would be mis-run as Linux.

**Where it bites.** `kernel/src/proc/elf.rs::detect_linux_abi` (flip default + add
`NT_GNU_ABI_TAG` note-walk + recognise the native marker);
`kernel/src/proc/spawn.rs::spawn_process_inner` and the `exec` path around
`new_abi_mode`; `build_test_elf` and the native toolchain (emit the marker).
**Sequencing:** decided but not the immediate priority — Q12 selected the page
cache (§36) as the next initiative; Q9 is unblocked and can land when the
native-binary marker is wired into the toolchain.

## 34. Fullscreen-capture video codec (Q10) — hardware encode via the GPU driver long-term (option C); defer the software-codec port near-term (option D); no stub encoder meanwhile

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q10; the operator deferred
to Claude's recommendation). The operator's words: *"Q10: I'll go with your
recommendation."* Claude's recommendation was **C long-term, D near-term**.

**The decision.** The proper home for the remote-desktop fullscreen capture
fallback (roadmap §4.5 — DMA-BUF/buffer-backed game/video surfaces with raw
pixels, not vector `RenderCommand`s) is **hardware video encode via the GPU
driver's encode engine**, which is hard-blocked on a GPU driver with an encode
engine (AMDGPU/i915, roadmap §4.x) that does not exist yet. So:
- **Near-term: defer the whole fallback** rather than build a software encoder
  hardware encode would later obsolete.
- **If** a software fallback is ever wanted before GPU encode lands, prefer
  **AV1 via `rav1e`** (royalty-free + Rust-native), not H.264/x264
  (patent/GPL friction).
- **No stub encoder meanwhile** (band-aid); the draw-command stream already
  covers the flat-shaded-desktop case.

**Rationale (both sides).** *For C/D:* avoids a soon-obsolete software-codec port;
matches real streaming architecture; keeps the royalty-free posture. *Against:*
fullscreen game/video remoting stays unsupported until GPU encode exists —
acceptable because the capture substrate is codec-agnostic (only the encoder
backend is blocked) and the desktop case already streams.

**Where it bites.** `gui/compositor` (fullscreen pixel capture + frame pacing + an
`Encoder` trait) and a future encoder crate; IPC extends
`CompositorRequest`/`CompositorResponse` alongside `StreamStart`/`StreamCapture`/
`StreamStop`. No code now — records the deferral + codec choice for when GPU
encode lands.

## 35. Zero-copy page-flipping for large channel messages (Q11) — explicit opt-in `MSG_ZEROCOPY`-style flag + caller-provided page-aligned landing region (option B); copy path stays the default

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q11; the operator chose
option **B**, which Claude recommended). The operator's words: *"Q11: Yeah, I
like B."*

**The decision.** Implement "zero-copy page flipping for large messages" as an
**explicit, opt-in** mechanism, not transparent or threshold-automatic:
- A `MSG_ZEROCOPY`-style **send flag**; without it, `send` keeps copy semantics
  (the zero-risk default — nothing existing changes).
- The **receiver pre-registers a page-aligned landing region**; on a zero-copy
  send the kernel moves (page-flips) the sender's pages into it. Move semantics
  (sender loses the pages) are explicit and opt-in — no silent `send` ownership
  change.
- Matches the `io_uring`/`vmsplice` model; 16 KiB page granularity and the
  sub-page-tail length field are visible only to opt-in callers.

**Rationale (both sides).** *For B:* keeps the correct copy path as default,
avoids silently changing `send` ownership at a size threshold (option C's
footgun), explicit/predictable. *Against:* more API surface; only helps adopters.
Accepted because the alternative changes user-visible ownership semantics.

**Compiler involvement (operator's follow-up — "should our compilers auto-choose
the flag, or is that up to the programmer?").** *Decision:* **keep it
programmer-/library-controlled; the compiler does not auto-insert the flag.** It
belongs in the IPC **runtime/library wrapper**, not `fastpy`/`rustc`/the C
compiler, for three reasons:
1. **It is a runtime decision on runtime values** — whether to page-flip depends
   on the runtime message length, buffer page-alignment, and whether the sender
   still needs the pages, none of which the compiler reliably knows statically
   (message size is usually dynamic).
2. **It changes semantics, not just performance** — zero-copy *moves* the
   sender's pages; a compiler silently changing ownership/aliasing would violate
   the language memory model (the same transparent-threshold footgun B avoids).
   Optimizations must be semantics-preserving; this isn't.
3. **The right ergonomic home is the channel library** — the send wrapper can
   offer an *auto-threshold helper* (`if len >= N && region.is_page_aligned() {
   send_zerocopy() } else { send_copy() }`) so most callers get "it just works"
   without the compiler, while a caller who needs the pages after send simply
   doesn't use that helper. For `fastpy`, the high-level channel binding exposes
   both an explicit zero-copy hint and the library-level auto-threshold default;
   the AOT compiler emits ordinary calls into that library and does not reason
   about page flipping itself.

*Net:* document a **library-level auto-threshold helper** as the ergonomic path;
do **not** add compiler analysis. (Recorded at the operator's request as part of
the Q11 resolution.)

**Where it bites.** `kernel/src/ipc/channel.rs` (`Message`, `send`/`recv`,
`MAX_MESSAGE_SIZE`), a new MM page-transfer mechanism (`kernel/src/mm`), the
Linux/native syscall glue marshalling channel messages, and the userspace channel
library (the auto-threshold helper). Benchmark exists:
`kernel/src/bench.rs::bench_ipc_channel_large` /
`bench/baselines.toml [ipc_channel_roundtrip_64k]` (~343 µs/64 KiB today,
copy-bound). **Sequencing:** decided but not the immediate priority — Q12 chose
the page cache (§36); Q11 is unblocked and can be built afterward.

## 36. Next large initiative (Q12) — build the operator-pre-approved C-lite read-only page cache now (lifts the §23 "not now")

**Date:** 2026-06-24

**Decided by:** Operator (this was `open-questions.md` Q12; the operator chose
option **E**). The operator's words: *"Q12: I guess let's go with E."*

**The decision.** With the bounded in-context work verified exhausted, the
operator selected the **C-lite unified read-only page cache** (§23 / Q5) as the
next large initiative. **This lifts the §23 "implement later, not now" hold** —
the trigger is now considered fired (the shared-library `.text` dedup payoff plus
the precursor being met), so the work is cleared to start. Scope is exactly §23's
C-lite: cache a file's pages once and share them **read-only** across every
process that maps/reads them (shared-library `.text` dedup + de-double-caching vs
the block buffer cache). **Writable `MAP_SHARED` writeback stays declined
(`ENOSYS`)** per §22/§23 — out of scope.

**Implementation plan (sub-tasks, in order).**
1. **Precursor — stable VFS file identity.** The cache is keyed by
   `(file-identity, offset)`. Verified 2026-06-24 that `FileMeta.ino` is now
   populated (ext4 real inode, FAT first-cluster, memfs `alloc_memfs_ino()`), so
   the precursor is substantially met; confirm every backend yields a stable
   non-zero identity and define the cache key around it.
2. **Read-only page cache structure.** A frame store keyed by
   `(file-identity, page-offset)` → refcounted physical frame, host-testable in
   isolation (insert/lookup/refcount/evict), zero boot-risk before any fault-path
   wiring. Likely unifies or fronts `kernel/src/fs/cache.rs`.
3. **Fault-path integration.** `VmaKind::FileBacked` faults source pages from the
   cache (shared read-only frame, refcount++) instead of a per-mapping `read_at`
   copy; mark shared frames read-only/refcounted; a private write CoW-copies out
   of the shared frame (existing CoW path derives flags correctly).
4. **Lifecycle.** Refcount on map/unmap/exit; eviction policy; coherence with the
   block buffer cache so a file's pages live in one place.

**Status (2026-06-30).** Sub-tasks 1–4 (the correctness slice) are **done**:
1. file identity (`FileId{fs_id,ino}` + `Vfs::file_identity`, commit 80cbbaa54);
2. the read-only `mm::page_cache` store (commits b18e45bfa, ad78a2b5c, model §37);
3. fault-path integration — whole-frame, frame-aligned `MAP_PRIVATE` FileBacked
   faults source shared read-only frames from the cache and CoW-copy on a private
   write; boot exercised it ~2158× with cross-process `FileId` sharing observed;
4. coherence — `invalidate_identity` wired into `write_at`/`write_file`/`truncate`/
   `remove`/replacing-rename (closes stale-data + inode-reuse, B-PAGECACHE-COHERENCE).
Shared-cache-page reclaim under memory pressure is also **done** (commit
f6003260c): `mm::page_cache::shrink(PressureLevel)` evicts idle cache frames
(refcount ≤ 1, no live mapper) proportional to pressure and is registered with
`mm::pressure` by `init()`; it fired under real critical pressure during boot
(freed 49 then 5 frames) with a clean BOOT_OK. Cache frames remain unregistered
with the swap clock/rmap by design (clean file pages reclaimed via the shrinker,
not swap — see `resolve_file_cached`).
**Remaining (performance, not correctness):** de-double-cache the page cache
against the block buffer cache (`fs/cache.rs`) so a page lives in one place.

**Rationale.** The §23-recorded Pareto-optimal slice: the cheap, reversible,
high-value read-only half that captures the memory-saving win for the common
shared-library case without the writable-shared hairiness §22/§23 declined.
Starting with the host-testable cache structure (sub-task 2) before the
boot-critical fault-path wiring (sub-task 3) keeps boot-risk out of early
increments.

**Where it lives:** `kernel/src/fs/vfs.rs` (identity precursor),
`kernel/src/fs/cache.rs` (unify/front the cache) or a new page-cache module,
`kernel/src/mm/` (`VmaKind::FileBacked` fault path), `kernel/src/syscall/linux.rs`
(`linux_file_mmap`).

## 37. C-lite page-cache refcount model — unify on the frame allocator's per-frame refcount (not a cache-owned mapper count)

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). The operator approved building
the C-lite cache (§36 / Q12=E); *how* the cache's frame lifetime integrates with
process teardown is an internal implementation detail with no user-visible
effect, so Claude resolved it. Recorded here because it is a genuine fork with
tradeoffs on both sides.

**The decision.** A cached page's lifetime is governed by the **frame
allocator's existing per-frame refcount** (`mm::frame::refcount` /
`unsafe ref_inc` / `free_frame`, the same mechanism CoW already uses), **not** by
a separate mapper-count inside the page cache. Concretely:

- The page cache holds **exactly one** frame reference per resident entry (the
  entry's presence in the map *is* that reference).
- When the `VmaKind::FileBacked` fault path maps a cached frame into a process,
  it bumps the frame refcount via `ref_inc` and maps the frame **read-only with
  the COW bit** (so a private write copies out of the shared frame via the
  existing CoW handler; writable `MAP_SHARED` stays `ENOSYS` per §23).
- Process unmap / exit frees mapped frames through the **standard `free_frame`
  teardown path with no changes** — it decrements the shared frame's refcount and
  only returns the frame to the allocator when the count hits zero.
- Eviction drops the cache's single reference via `free_frame`. To preserve
  dedup, eviction prefers entries whose frame refcount is exactly 1 (no live
  mappers); a still-mapped page can be evicted from the *index* but its frame
  survives for the mappers.
- "Is this page actively mapped?" is answered by `frame::refcount(frame) > 1`,
  not by a cache field.

**Alternatives considered.**

- **Cache-owned mapper refcount (rejected).** Have the cache count mappers
  itself (the sub-task-2 prototype's `refcount`/`release` API). *Con:* process
  teardown walks page tables and calls `free_frame` on every present user frame;
  a cache frame would then be decremented by the frame allocator while the cache
  *also* believed it owned the reference — a double-free / use-after-free unless
  the boot-critical teardown path is taught to special-case cache frames (skip
  `free_frame`, call `page_cache::release` instead). That is invasive and
  error-prone exactly where bugs are most dangerous. *Pro:* the cache could
  answer "actively mapped?" without taking the allocator lock.
- **Unify on the frame refcount (chosen).** *Pro:* reuses proven CoW + teardown
  machinery; **zero** changes to the exit/unmap free path; no double-bookkeeping;
  a private write already copies correctly because a shared file page always has
  refcount ≥ 2 (cache + mapper). *Con:* the sub-task-2 cache prototype's
  per-mapper `refcount`/`release`/`CachedPage` API is the wrong abstraction and
  was revised — the cache no longer tracks per-mapper references.

**Consequence.** The sub-task-2 `mm::page_cache` module (commit b18e45bfa) is
revised: `get_or_fill` returns the shared frame and the cache holds one
reference; per-mapper reference counting moves to the frame refcount; eviction
prefers unmapped frames. Only the **whole-frame** `FileBacked` fast path uses the
cache; the sub-page-straddling path (glibc's 4 KiB-packed segments, where one
16 KiB frame backs multiple VMAs at different file offsets) stays on the private
per-mapping read path — a single `(FileId, page_index)` key cannot describe a
frame shared across mismatched file offsets.

**Where it bites:** `kernel/src/mm/page_cache.rs` (refcount model + eviction),
`kernel/src/proc/pcb.rs` (`try_resolve_fault` whole-frame `FileBacked` path),
`kernel/src/mm/frame.rs` (`ref_inc`/`free_frame`, unchanged), the CoW handler in
`kernel/src/mm/cow.rs` (unchanged — already copies on write for refcount > 1).

## 38. De-double-cache file data (Q13) — page-cache-primary (option A): the page cache is the single cache for regular-file data; the buffer cache caches only filesystem metadata

**Date:** 2026-06-30

**Decided by:** Operator (this was `open-questions.md` Q13; the operator chose
option **A**). The operator's words: *"Q13: A."* Claude recommended A as the
correct long-term end-state. (Q12=§36's one remaining performance item.)

**The decision.** File *data* I/O is cached in exactly **one** place: the
**page cache** (`mm::page_cache`, 16 KiB pages). The block buffer cache
(`fs/cache.rs`, 512 B sectors) is demoted to caching only filesystem
**metadata** — superblock, block/inode bitmaps, inode tables, directory blocks,
journal — never regular-file data pages. Regular-file `read(2)`/`write(2)` **and**
mmap all source/sink their data through the page cache, which unifies `read(2)`
and mmap coherence for free (one shared frame, no separate invalidation needed
for the read path). Today (status quo before this change) a mmap'd file page is
cached as 32 sectors in the buffer cache *and* as one 16 KiB page in the page
cache — this change removes that double-caching.

**Alternatives considered (from Q13).**
- **(B) Read-through + drop-behind (rejected).** Keep the buffer cache as the
  device cache but mark the sectors the page-cache fill consumed as immediately
  evictable / bypass the buffer cache for whole-page file reads. *Pro:* small,
  localized, no FS-path refactor. *Con:* doesn't truly unify — a concurrent
  `read(2)` re-populates the buffer cache; read/mmap coherence still leans on the
  §36 invalidation hooks rather than a genuinely shared frame. A stepping-stone
  that A subsumes, so going straight to A avoids throwaway work.
- **(C) Leave as-is (rejected).** Accept the double-caching. *Pro:* zero risk.
  *Con:* memory wasted on hot mmap'd files; not the §36 end-state.

**Rationale.** Option A is the canonical, proven (Linux-like) design: truly one
copy of a file's data, and `read(2)`/mmap coherence falls out of the shared
frame for free. The cost is a real FS-data-path refactor (route metadata vs.
data correctly per filesystem) — the largest blast radius of the three — but it
is the correct end-state and the operator picked it directly, so there is no
reason to build B first as a throwaway.

**Where it bites.** `kernel/src/mm/page_cache.rs` (`get_or_fill` fill path),
`kernel/src/fs/cache.rs` (buffer cache — restrict to metadata),
`kernel/src/fs/handle.rs` / `kernel/src/fs/vfs.rs` (`read_at`/`write_at`
routing through the page cache), and the ext4/FAT data read/write paths under
`kernel/src/fs/` and `fs/` (route data through the page cache, metadata through
the buffer cache).

### Implementation sub-design (2026-06-30)

**Decided by:** Claude (operator-approved scope — the operator chose option A;
these are the implementation-level sub-decisions made while building it). All
reversible.

The refactor landed in four increments, all preserving the **per-block
read/write cache-path symmetry** invariant: for any one physical block, reads
and writes use the *same* cache path, or a read-after-write serves stale bytes.

1. **Two buffer-cache-bypassing sector primitives** (`fs/cache.rs`):
   `read_sector_uncached` (serves a *dirty* buffer-cache hit if present —
   that's legitimate metadata pending writeback — else drops a clean hit and
   reads straight from `blkdev`) and `write_sector_uncached` (writes straight
   to `blkdev`, then `invalidate_range` drops any buffer-cache alias). Plus
   ext4 `BlockReader` data methods (`read_data_block`/`write_data_block`/
   `invalidate_block`) and `read_block_classed`/`write_block_classed`
   dispatchers taking an `is_file_data: bool`.

2. **Block-reuse coherence** (`fs/ext4/balloc.rs`): `free_block` now calls
   `reader.invalidate_block` on the freed LBAs. Directory/extent-tree blocks
   are allocated from the same data-region pool; when a freed metadata block is
   later reused as file data (written via the bypass path), a stale *dirty*
   metadata buffer-cache entry would otherwise win. This mirrors Linux's
   `clean_bdev_aliases`.

3. **Data-vs-metadata classification by inode mode** (`fs/ext4/driver.rs`):
   `inode_holds_file_data(inode)` = `(i_mode & S_IFMT) == S_IFREG`. The shared
   leaf read/write helpers (`read_file_data`, `write_file_data`,
   `write_to_existing_blocks`, the extent/indirect leaf readers) are used by
   **both** directories and regular files, so the data/metadata split is keyed
   on the inode mode threaded through as `is_file_data`, *not* on the function.
   A blanket switch would have read directories (written via the buffer cache)
   back through the bypass path → stale directory reads. Extent-tree *internal*
   nodes, htree directory blocks, xattr blocks, bitmaps and inode tables stay on
   the buffer cache (metadata).

4. **`read(2)`/`read_file` routed through the page cache** (`fs/vfs.rs`):
   `Vfs::read_at` and `Vfs::read_file` now serve **stable-identity regular
   files** (`ino != 0`: ext4, memfs) from `mm::page_cache` via a new
   `page_cache::read_through` (splits an arbitrary `[offset,len)` into covering
   16 KiB pages, fills misses from the FS *data* path, copies out, drops each
   caller ref). Non-regular files and no-stable-identity filesystems (FAT,
   ISO9660, pseudo-fs — they keep their own caching) fall back to the
   per-filesystem read unchanged. This is what restores caching for `read(2)`
   after increment 3 removed regular-file data from the buffer cache — and it
   unifies `read(2)`/`mmap` on one shared frame.

   - **Reentrancy fix.** The `mmap` fault fill (`proc/pcb.rs resolve_file_cached`)
     previously filled via `handle::read_at`, which now routes through
     `get_or_fill` → it would recurse on the very page being filled. New
     `Vfs::read_at_uncached` / `handle::read_at_uncached` read straight from the
     FS data path (bypassing *both* caches); the mmap fill and `read_through`'s
     fill closure use them. No lock nesting: the page-cache lock is always
     dropped before a fill closure takes the VFS lock (order is VFS→drop,
     cache→drop, VFS-fill→drop — never simultaneous).

**Known minor inefficiency (logged):** memfs/tmpfs files now hold their data
both in memfs's own store *and* (when read/mmap'd) in the page cache — Linux's
tmpfs *is* the page cache, so this is a double-store for tmpfs specifically. It
is coherent (writes invalidate) and was already true for `mmap` of memfs before
this change; not worth special-casing now.

## 39. Connect the two cgroup subsystems (Q14) — cgroupfs as the frontend, `kernel/src/cgroup.rs` as the enforcement engine (option A)

**Date:** 2026-06-30

**Decided by:** Operator (this was `open-questions.md` Q14; the operator chose
option **A**). The operator's words: *"Q14: A."* Claude recommended A.

**Background.** The OS had two independent cgroup implementations that did not
talk to each other: `kernel/src/cgroup.rs` (the in-kernel resource controller —
the real *enforcement* hooks: the frame allocator charges a task's cgroup on
every `alloc_frame`/`alloc_frame_zeroed` via the per-frame `FRAME_CGROUP` owner
array, plus `io_charge` and PID accounting, reading the current task's group via
`sched::current_task_cgroup()` → `Task::cgroup_id`), and `fs::cgroupfs` (the
user-facing cgroup-v2 filesystem — 5 controllers, hierarchical groups,
`memory.max`, PID limits, per-group process assignment, but **no enforcement**).
Net effect before this change: neither system actually constrains a real
process's memory (cgroupfs limits cosmetic; the kernel controller dormant —
D-CGROUP-TASK-UNASSIGNED).

**The decision.** Wire the two ends into **one pipe**: `fs::cgroupfs` is the
cgroup-v2 **frontend**, `kernel/src/cgroup.rs` is the **enforcement engine**.
Concretely:
- `cgroupfs` controller writes flow through to the kernel controller:
  `memory.max` → `cgroup::set_mem_limit`, and `cgroup.procs` assignment sets the
  target task's `cgroup_id`.
- `fork`/`clone`/`spawn` **inherit** the parent's `cgroup_id` (universal cgroup
  semantics).
- The two group-ID spaces (cgroupfs groups vs. `cgroup.rs` `CgroupId`, capped at
  256) are reconciled, and the 5 controllers mapped through.

**Alternatives considered (from Q14).**
- **(B) Collapse onto one (rejected).** Delete/absorb one implementation. *Pro:*
  eliminates duplication entirely. *Con:* biggest blast radius; risks regressing
  whichever subsystem's self-tests; `cgroup.rs` is on the allocator hot path so
  its per-frame `u8` owner array must be preserved regardless — so the "collapse"
  saving is smaller than it looks.
- **(C) Containers drive `cgroup.rs` directly, leave cgroupfs standalone
  (rejected).** *Pro:* smallest change to make container memory limits real.
  *Con:* leaves two permanently-parallel ways to express "a cgroup" — confusing
  long-term.

**Rationale.** The frame-allocator charging in `cgroup.rs` is the correct,
hot-path-proven enforcement engine, and cgroup-v2 (`cgroupfs`) is the right
user-facing model — they should be two ends of *one* pipe, not two pipes. A also
keeps both subsystems in their current roles (lowest regression risk to the
existing self-tests) while finally making limits real.

**Where it bites.** `kernel/src/cgroup.rs` (`set_mem_limit`, `mem_charge`,
`current_task_cgroup`), `kernel/src/fs/cgroupfs.rs` (controller writes, process
assignment), `kernel/src/sched/task.rs` (`cgroup_id` field + 3 constructors,
all defaulting to `ROOT_CGROUP`), `kernel/src/sched/mod.rs` (a lock-taking
`set_task_cgroup` setter), `kernel/src/container.rs` (`Container::cgroup_id`),
and the task-creation paths in `kernel/src/proc/{fork,thread,thread_clone,spawn}.rs`
(cgroup inheritance).

## 40. Next focus after Q13/Q14 (Q15) — execute Q13 + Q14 (option A), then a large initiative; C (GPU accel) or D (Docker port) in operator-indifferent order

**Date:** 2026-06-30

**Decided by:** Operator (this was `open-questions.md` Q15; the operator chose
option **A**, then C-or-D). The operator's words: *"Q15: A, then do C or D. I'm
not sure which is better to do first between C and D. I guess it doesn't matter
because it all has to be done anyway and nobody can use the OS yet."* Claude
recommended A as the immediate next step (and had recommended B as the next large
initiative; the operator instead directed C or D).

**The decision.** Immediate next step: **(A)** — execute the now-resolved Q13
(page-cache-primary, §38) and Q14 (connect the cgroup subsystems, §39). After
that, proceed to a large initiative: either **(C) GPU acceleration** or **(D)
Docker / container-runtime port**, in whichever order — the operator is
explicitly indifferent ("it all has to be done anyway"). **This is the explicit
operator go-ahead the standing rule required for the Docker/container-runtime
port (a giant external port).** Option (B) TCP/IP→userspace, which Claude had
recommended as the next *large* initiative, was not selected as the immediate
follow-on; it remains valid future work but C and D come first.

**Alternatives considered (from Q15).** (B) TCP/IP stack → userspace (Claude's
recommended next large initiative — internal, stack already feature-complete, on
the microkernel roadmap); the operator chose C/D instead. C and D are the two
selected; the operator left their relative order open.

**Where it bites.** (A) §38 (Q13) + §39 (Q14). (C) `gui/gpu/`,
`gui/compositor/`. (D) `kernel/src/container.rs`, `pkg/`, plus a large external
dependency — and Q14's cgroup-enforcement gap was a stated prerequisite, now
being closed by §39.

## 41. Container runtime increment 1 — `container::run()` orchestration and the PID-vs-task-id binding split

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). The operator pre-approved the
Docker/container-runtime port as a whole in §40 ("the explicit operator go-ahead
the standing rule required"); this entry records the specific implementation
choices Claude made within that scope for the first increment.

**Context.** §40 chose initiative D (container runtime). The container subsystem
(`kernel/src/container.rs`) already had the full create/start/stop/delete state
machine plus all four namespaces, a cgroup, and veth networking — but nothing
actually *launched a process inside a container*. `start()` only flipped the
state flag; `add_process()` bound a pre-existing (synthetic, in tests) PID.
Increment 1's job: a real `docker run`-equivalent that spawns an init process,
binds it to the container's cgroup (so Q14/§39 billing applies), and transitions
to Running.

**The decision.**
1. **Add `container::run(id, elf_data, options) -> pid`** that orchestrates
   spawn → bind → Running atomically: it validates the container is `Created`,
   calls `proc::spawn::spawn_process` (the process is enqueued but does not
   execute until the scheduler picks it, so the binding is guaranteed in place
   before its first instruction), binds it, records the init PID, and flips to
   Running. On any post-spawn failure it tears the process down
   (`kill_process_threads` + `pcb::destroy`) so a failed run leaks nothing.
2. **Split the process-id from the task-id in the binding path.** A spawned
   process's global PID and its initial thread's scheduler *task id* are
   independent allocations (observed in the self-test: pid=215, task=179). The
   scheduler-level resources — cgroup billing (`set_task_cgroup`) and network
   namespace (`set_task_net_ns`) — are keyed on the **task id**; the PID-namespace
   mapping and the container's tracked-process list are keyed on the **PID**.
   The old `add_process(id, global_pid)` conflated them, which silently no-ops
   the cgroup/net-ns assignment whenever PID ≠ task id (the cgroup `set` fails to
   find a task with that id). Fixed by adding `add_process_task(id, pid, task_id)`
   / `remove_process_task(id, pid, task_id)` as the real entry points;
   `add_process`/`remove_process` are now thin wrappers passing `pid` as both
   (correct only for the current-task case, e.g. the existing net-ns self-test).
   Threads the process spawns later inherit the cgroup automatically
   (`sched::spawn` copies the creator's `cgroup_id`), so binding the initial
   thread suffices.

**Alternatives considered.**
- *Have `run()` reuse `add_process(id, pid)` unchanged.* Rejected: it would bill
  nothing to the cgroup (PID ≠ task id), defeating the entire point of building on
  Q14. The conflation was a latent bug regardless of `run()`.
- *Make `add_process` take both ids and update its one existing caller.* Would
  also work, but keeping the single-id wrapper preserves the ergonomic
  "bind the current task" call site (`add_process(id, current_task_id())`) used by
  the net-ns-propagation self-test, where PID==task by construction.
- *Spawn-into-namespaces (clone-style) vs. spawn-then-bind (setns-style).* This
  increment uses spawn-then-bind because the process does not run until after the
  bind completes, so the result is observably equivalent for a single init
  process. The genuine clone-vs-setns fork (relevant once a container must
  *enter* an existing namespace mid-life, and for mount-namespace/rootfs/
  pivot_root) is deferred to a later increment; it is an implementation choice,
  not an operator policy fork, so it will be resolved autonomously and recorded
  here when reached.

**Deferred to later increments (not in increment 1).** Mount-namespace field +
rootfs / `pivot_root` on the `Container` struct (it currently has no mount_ns
field); OCI image pull/unpack + overlayfs; a userspace `docker run` CLI. The
cgroup `nr_tasks` accounting asymmetry surfaced while writing the self-test
cleanup is logged in `known-issues.md` (it is pre-existing Q14 behavior, not
introduced here).

**Where it bites.** `kernel/src/container.rs` (`run`, `add_process_task`,
`remove_process_task`, the `init_pid` field, self-test 17). Relies on §39 (Q14)
for the cgroup billing that `run` exercises end-to-end.

## 42. Container runtime increments 3–4 — per-process filesystem root (chroot) and `oci run` launching the jailed entrypoint

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). Same §40 pre-approval of the
container-runtime port; this records the implementation choices for the rootfs/
jail and the `oci run` launch path that §41 explicitly deferred.

**Context.** After §41, a container could launch an init process, but that
process resolved every path against the **host** filesystem — `/bin/sh` was the
host's `/bin/sh`, not the container image's. Real container isolation needs the
init process jailed to the container's rootfs. The host already had two relevant
mechanisms: the per-process `ipc::namespace` Bind/Hide path-translation hook
(consulted first in `Vfs::resolve_follow`), and an `fs::overlay` module accessed
by ID (not VFS-mounted). `sys_chroot` was an EPERM-only Linux gate ladder — no
real per-task VFS root existed.

**The decision.**
1. **Implement chroot as a dedicated per-process *root*, not as a Bind rule.**
   A Bind rule `{ "/" → "/containers/x/rootfs" }` would re-anchor paths but
   **cannot clamp `..`**: a guest path `/../etc` would normalize (after the
   prefix is applied) to a host path *above* the rootfs — a jail escape. So
   `ipc::namespace` gains a `PROCESS_ROOT` map with `set_root`/`clear_root`/
   `get_root`. `resolve_path_for` applies Bind/Hide first (guest path space),
   then re-anchors under the root via `apply_root`, which **normalizes within
   the jail with `..` clamped at the root** (`normalize_jailed`: popping an
   empty stack stays at root, exactly like Linux chroot) before prefixing. This
   makes escape structurally impossible rather than relying on a later check.
2. **Key the jail on the global PID, not the task id.** VFS resolution looks the
   root up via `current_task_id() → owner_process()`, i.e. the PID; child threads
   share the process, so they inherit the jail for free. (Contrast §41's
   scheduler resources, which are keyed on the *task id*. The container binding
   path now sets *both* correctly: cgroup/net-ns by task, jail by PID.)
3. **`Container` gains a `root_path` field + `set_root_path` (Created-only).**
   `add_process_task` reads it and calls `set_root(pid, root)`;
   `remove_process_task` calls `clear_root(pid)`. `run()` therefore launches the
   init already jailed. Changing the root of a running container is rejected
   (it would not retroactively re-jail live processes).
4. **`oci run` launches the entrypoint jailed to the extracted rootfs.** It sets
   `root_path` to the extracted `lower` tree, reads `command[0]` from the host
   path inside that tree, and `container::run`s it with the image's argv+env.
   The manual `container start` stub is now only a fallback (no entrypoint /
   unreadable binary / spawn failure), preserving `container exec` usability.

**Alternatives considered.**
- *Bind-rule chroot* — rejected for the `..`-escape reason above; the clamp is
  the whole point.
- *A per-task root in the scheduler `Task` struct* (mirroring Linux `fs_struct`)
  — viable, but path resolution already routes through `ipc::namespace` per
  *process*, and threads should share one jail, so the PID-keyed map in the
  existing module is the lower-friction home and avoids a second resolution hook.
- *VFS-mount the overlay so the jail routes through copy-on-write* — deferred.
  The overlay is ID-addressed today; mounting it into the path tree is a larger
  change. For now the jail points at the extracted `lower` dir, so image writes
  land there directly (documented limitation in `known-issues.md`).
- *Relative-path jailing* — `apply_root` only jails absolute paths; relative
  paths are left for the (not-yet-jailed) per-process cwd layer. Documented as a
  limitation rather than silently half-jailing.

**Deferred (still open).** Overlay-backed CoW rootfs (VFS-mounted upper);
per-process cwd jailing; a mount-namespace field + `pivot_root` semantics on
`Container`; dynamic-linker/interpreter presence checks for the entrypoint; a
userspace `docker`/`podman` CLI (Python/fastpy per CLAUDE.md).

**Where it bites.** `kernel/src/ipc/namespace.rs` (`PROCESS_ROOT`, `set_root`/
`clear_root`/`get_root`, `apply_root`/`normalize_jailed`, `resolve_path_for`,
`detach`, self-test "Process filesystem root (chroot)");
`kernel/src/container.rs` (`root_path`, `set_root_path`, `add_process_task`/
`remove_process_task` jail wiring, self-test 18); `kernel/src/kshell.rs`
(`container rootfs` subcommand, `Rootfs:` in `container info`, the `oci run`
launch path).

## 43. VFS dispatch holds a *per-mount* lock, not the global VFS lock — enabling stacked filesystems (overlay) and removing the I/O-under-global-lock anti-pattern

**Date:** 2026-06-30

**Decided by:** Claude (autonomous). This was a structural fix forced by
implementing the §42-deferred overlay-backed CoW rootfs (container increment 5):
VFS-mounting the overlay deadlocked, and the proper fix is a foundational change
to VFS lock granularity that benefits the whole filesystem layer. No operator
fork — there is one correct design (don't hold a global lock across filesystem
I/O), and CLAUDE.md mandates the proper fix over a hack.

**Context.** The overlay engine (`fs::overlay`) reads/writes its lower and upper
layers through ordinary VFS paths. To give containers real copy-on-write rootfs,
increment 5 wraps a live overlay in an `OverlayFs` adapter implementing the
`FileSystem` trait and mounts it into the path tree. But the VFS held its single
global `Mutex<VfsInner>` **across every filesystem method call** (e.g.
`read_file_routed` did `let vfs = VFS.lock(); … return mp.fs.read_file(relative)`
*inside* the locked scope). When `Vfs::read_file("/mnt/ovl/x")` called
`OverlayFs::read_file`, that re-entered `Vfs::read_file("<lower>/x")` to fetch the
backing bytes → re-acquire the same non-reentrant spinlock → **hard deadlock**
(observed: boot hung immediately after mounting the overlay). Holding a global
lock across I/O is also independently an anti-pattern CLAUDE.md calls out (it
serializes *all* filesystem I/O system-wide on one mutex).

**The decision.** Change `MountPoint.fs` from `Box<dyn FileSystem>` to
`Arc<Mutex<Box<dyn FileSystem>>>` — i.e. give **each mount its own lock**. A new
`resolve_mount(path)` helper takes the global `VFS` lock only long enough to do
the longest-prefix mount-table lookup, clones the `Arc`, copies the stable
`fs_id`/`MountOptions`/relative path, and **drops the global lock**. Every one of
the ~50 dispatch sites then locks the returned *per-mount* handle to run the
actual operation. Because a stacked filesystem's lower layers live on *different*
mounts (different `Arc`s, different locks), an overlay method can freely re-enter
the VFS: it briefly re-takes the global lock to resolve the lower mount, then
locks that mount's *own* lock — never the overlay's — so there is no reentrancy
on any single lock.

Consequences/details:
- **Atomicity is now per-mount, not global.** Operations that needed two steps
  to be atomic w.r.t. one filesystem (RENAME_NOREPLACE's exists-check + rename;
  the cache-identity capture + remove/rename/truncate) now hold that mount's lock
  across both steps — same guarantee, scoped to the mount. Cross-mount checks
  (same-mount rename, hard-link, `RENAME_EXCHANGE`) now compare handles with
  `Arc::ptr_eq` instead of comparing mount-path strings.
- **Iteration sites** (`sync`, `mount_info`, `trim_device`, `debug_stats`,
  `mounts`/`mounts_full`) snapshot the `Arc` handles (or clone the matching one)
  under a brief global lock, then call the filesystem lock-free — so even a
  stacked filesystem's `statvfs`/`sync` cannot deadlock during a full-VFS scan.
- **Page-cache fill** (`read_at_routed`/`read_file_routed`) drops the per-mount
  guard *before* calling `page_cache::read_through`; the fill closure
  (`fill_file_page`) re-resolves and locks freshly, so the cache lock and the
  per-mount lock never nest and a file's own lock isn't held across its fill.
- `cache_identity` was re-signatured to take the already-locked
  `&mut Box<dyn FileSystem>` + `fs_id` (it used to take `&mut MountPoint`).
- `mount`/`mount_with_options` wrap the incoming `Box` in `Arc::new(Mutex::new(…))`;
  callers are unchanged (still pass a `Box<dyn FileSystem>`).

**Why `Arc<Mutex<Box<dyn>>>` and not alternatives.**
- *Convert all `FileSystem` methods to `&self` + interior mutability* — would let
  a bare `Arc<dyn FileSystem>` work, but is a far larger, riskier change touching
  every filesystem impl, and the per-fs `Mutex` we need anyway preserves the
  "one operation at a time per filesystem" assumption every impl was written
  against.
- *A reentrant/recursive global lock* — rejected: reentrant locks hide bugs, our
  spinlock has no stable thread identity to key on, and it would not fix the
  I/O-under-one-global-lock serialization problem.
- *Make the overlay bypass the VFS for its layers* — impossible in general: a
  layer path can span arbitrary mounts and needs full VFS resolution.

**Result.** Overlay self-test 13 ("VFS mount adapter — CoW routing") passes:
reading through the mounted overlay returns the merged (lower) view, writing
copies up into the upper layer, and the lower layer is never mutated — all via
ordinary `Vfs::read_file`/`write_file` on the mount path. Full kernel boots
clean (BOOT_OK); no new clippy warnings (baseline unchanged). This unblocks the
§42-deferred overlay-backed CoW container rootfs (next: mount an `OverlayFs` at
each container's rootfs and point the chroot jail at it).

**Where it bites.** `kernel/src/fs/vfs.rs` (`MountPoint.fs` type, `resolve_mount`
helper, `cache_identity` signature, and ~50 dispatch sites converted from
global-lock-held to per-mount-lock); `kernel/src/fs/overlay.rs` (`OverlayFs`
adapter + self-test 13). Tech-debt TD32's "VFS-mount overlay" half is now
unblocked (the lock barrier that made it deadlock is gone).

## 44. fd-backed VFS ops resolve the path *once* at open() — `*_resolved` worker split (open-fd semantics, double-jail fix)

**Date:** 2026-06-30

**Decided by:** Claude (autonomous). Forced by a correctness bug (fd-backed
file I/O was fundamentally broken for chroot-jailed/container processes); there
is one correct design (an open fd holds a resolved reference), so no operator
fork. CLAUDE.md mandates the proper fix over a band-aid.

**Context — the double-jail bug.** `namespace::apply_root` is intentionally
**non-idempotent**: it blindly prefixes the jail root onto a path, *assuming the
input is a guest (pre-jail) path*. Every path-based `Vfs::*` method begins with
`resolve_follow` → `namespace::resolve_path` → `apply_root`, so a guest path is
jailed exactly once on the way in. But `handle::open()` stored the
*already-resolved host path* in the file handle (`file.path = Vfs::resolve_path(path)`),
and every subsequent fd op (`Vfs::read_at(&file.path)`, `write_at`, `truncate`,
`metadata`, `readdir_at`, `file_identity`, `flock`/`funlock`/`lock_query`) called
`resolve_follow` **again** on that host path → `apply_root` prefixed the jail root
a *second* time → the op hit a path that doesn't exist (`/jail/jail/…`). For a
jailed process even `open()` itself failed, because its internal probes
(`stat`/`truncate`/`write_file`) re-jailed. Non-jailed processes escaped notice
only because `resolve_follow` is idempotent on already-resolved *non-jailed*
paths (apply_root is a no-op when there's no jail). Increment 6's CoW overlay
mount would have exposed this the instant a container opened a file.

**The decision.** Split every path-based `Vfs` method into two functions:
- a thin **wrapper** (`X`) — `let p = Self::resolve_follow(path)?; Self::X_resolved(&p, …)`;
- a **worker** (`X_resolved`) — operates on an already-resolved host path and
  does **no** namespace translation / symlink re-follow.

Handle-backed ops call the `*_resolved` worker directly with the path captured at
`open()`. This encodes correct **open-fd semantics** (Unix): an open file
description is bound to the file it resolved to at open time and is immune to
later chroot, rename, or symlink changes to the path. Split methods: `read_at`,
`read_file`, `stat`, `write_file`, `write_at`, `truncate`, `metadata`,
`read_at_uncached`, `readdir_at`, `file_identity`, `flock`, `funlock`,
`lock_query`. Native path-based syscalls (e.g. `sys_fs_flock`, which takes a raw
user guest path) keep calling the resolving wrapper; only callers holding a
*resolved* path (handle ops, `handle_path()`-derived syscalls) use `*_resolved`.

**Why not option B (store the guest path on the handle, re-resolve each op).**
Re-resolving per op would also fix the double-jail, but it *regresses* fd
stability: a handle would re-follow symlinks and re-resolve renamed/relinked
path components on every read/write, so an fd could silently start pointing at a
different file after a rename or symlink swap — the opposite of Unix open-fd
semantics. Resolving once at open() is both correct and avoids repeating the
(non-trivial) resolution cost on every I/O.

**Regression guard.** `namespace::test_process_root` (run at boot via
`main.rs`) now asserts the non-idempotency directly: resolving an already-jailed
path a second time must produce the double-jailed result. If a future refactor
makes handle ops re-resolve, this boot self-test fails loudly. The existing
`fs::handle::self_test` (open→read→seek→write→read-back→fstat→truncate) provides
end-to-end coverage that the wrapper/worker split itself didn't regress.

**Result.** Build clean, clippy warning count unchanged (17754 before and after
the split — zero net-new), boot-test green. fd-backed file I/O now works for
jailed/container processes.

**Where it bites.** `kernel/src/fs/vfs.rs` (13 method splits + `*_resolved`
workers); `kernel/src/fs/handle.rs` (open/read/write/pread/pwrite/read_dir_at/
metadata/truncate/file_identity/funlock call sites → `*_resolved`);
`kernel/src/syscall/linux.rs` (`sys_flock` → `flock_resolved`/`funlock_resolved`);
`kernel/src/ipc/namespace.rs` (non-idempotency regression assertion). Resolves
the increment-7 double-jail half of TD32; part (b) cwd jailing still open.

## 45. Per-process cwd and `*at` dirfd base paths are stored as *guest* paths, not resolved host paths (chroot relative-path containment)

**Date:** 2026-06-30

**Decided by:** Claude (autonomous). Closes the relative-path-containment half of
TD32 part (b); the guest-path representation is the obviously-correct choice
(consistency with how `chdir` and the canonicalize-then-jail pipeline already
work), not an operator fork.

**Context.** Relative paths are canonicalized against the per-process cwd in the
*syscall* layer (`open_common`, `resolve_at_path`) → an absolute path → then the
VFS jails it via `apply_root`. So a relative path is contained **iff the cwd it
joins against is a guest path** (jailed exactly once on the way out). `chdir`
already stored a guest cwd. But three sites stored/used the *resolved host* path:
- `fchdir` stored `handle_path(fd)` (the resolved host path) as cwd → `getcwd`
  leaked the jail's host location, and a later relative path joined the host cwd
  and was jailed a *second* time (double-jail → nonexistent path).
- `sys_openat(realdirfd, rel)` built `host_dir + "/" + rel` and re-opened it
  (re-jailed), and its directory type-check `Vfs::stat(&host_dir)` re-jailed too
  (→ ENOENT for *every* relative `*at` from a jailed process).
- `resolve_at_path` (shared resolver for fstatat/unlinkat/fchownat/…) had the
  identical defect.

These didn't bite the common container launch (image entrypoints + libs use
absolute paths), but any container process using `fchdir`/relative `*at` would
break, and `getcwd` leaked the host jail path.

**The decision.** Represent **all** stored/derived directory bases as *guest*
paths, so the single canonicalize-then-`apply_root` pipeline jails them exactly
once. Concretely:
- Added `namespace::unjail_path_for(pid, host) → guest` — the exact inverse of
  `apply_root`: strip the process's jail-root prefix (no-op for an unjailed
  process; `host == root` → `/`; out-of-jail host returned unchanged
  defensively).
- `fchdir` converts `handle_path` (host) back to guest with `unjail_path_for`
  before `set_cwd`.
- New shared helper `dirfd_to_guest_dir(dirfd)` resolves a real dirfd to its
  *guest* directory path, doing the directory-type check with `stat_resolved`
  (the §44 worker — no re-jail). Both `sys_openat` and `resolve_at_path` use it,
  replacing their duplicated host-path-prepend logic.

**Why guest paths, not "store the host cwd and skip re-jailing"** (the
alternative the original TD32 note sketched). If cwd were stored as a host path,
the canonicalizer would produce a host absolute path, but it cannot distinguish
that from a genuine *guest* absolute path the user passed (`open("/etc/x")`),
which **must** be jailed. One uniform rule — "everything entering the VFS is a
guest path, jailed once" — is only possible if cwd is a guest path. This also
keeps `chdir` and `fchdir` representations consistent (both guest), so
`get_cwd`/`getcwd` always returns a guest path.

**Why not store the guest path on the open handle** (which would make
`unjail_path_for` unnecessary). That is the fully general solution and the only
way to also reverse namespace Bind/Hide remapping, but it enlarges every
`OpenFile` and the open path for a case that does not occur: the container
runtime isolates with the chroot jail alone and never layers Bind rules on a
jailed process, so stripping the chroot prefix is exact. The limitation (a
Bind-rules-*and*-chroot process that `fchdir`s would get the post-Bind guest
path) is documented on `unjail_path_for`; revisit only if such combos arise.

**Regression guard.** `namespace::test_process_root` (boot self-test) now asserts
the round trip: `unjail_path_for(pid, resolve_path_for(pid, g)) ==` the
normalized guest path, the unjailed no-op, and the out-of-jail passthrough.

**Result.** Build clean; warning counts for every touched file unchanged vs the
prior commit (linux.rs 2341, vfs.rs 69, namespace.rs 8 — zero net-new); boot-test
green. Closes TD32 part (b); TD32's remaining scope is the larger
mount-namespace/`pivot_root` feature deferred in §42.

**Where it bites.** `kernel/src/ipc/namespace.rs` (`unjail_path_for` +
round-trip assertions); `kernel/src/syscall/linux.rs` (`dirfd_to_guest_dir`
helper, `sys_fchdir`, `sys_openat`, `resolve_at_path`).

## 46. Container runtime increment 9 — volume (bind) mounts layered on the chroot jail

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). Same §40 pre-approval of the
container-runtime port; this records the implementation choices for the volume /
bind-mount mechanism, which is the first concrete piece of the broader
"mount-namespace / `pivot_root`" work that §42/§45 deferred as TD32's remaining
scope. It is an implementation choice (how mounts compose with the chroot jail),
not an operator policy fork, so it is resolved autonomously and recorded here.

**Context.** After increments 6–8 a container's init process is jailed to a
copy-on-write overlay rootfs and every absolute/relative/`*at` path is contained.
But a real container runtime must also let a container *share a host directory* —
Docker's `-v /host/dir:/data`. The existing chroot (`apply_root`) re-anchors
**every** guest path under the single rootfs prefix, with no way for a subtree to
point somewhere else. The pre-existing `ipc::namespace` Bind/Hide rules (step 1
of `resolve_path_for`) operate purely *within* the guest path space — a Bind
rewrites a guest path to another guest path that step 2 then prefixes with the
rootfs — so they cannot express "this guest subtree lives at an arbitrary host
location outside the rootfs." A new mechanism was needed.

**The decision.**
1. **Per-process volume table** (`PROCESS_MOUNTS: BTreeMap<pid, Vec<VolumeMount>>`
   in `namespace.rs`), each entry mapping a normalized guest prefix → an absolute
   host target. Keyed on the **global PID**, exactly like `PROCESS_ROOT`, so
   child threads inherit it and PID-reuse safety is handled by clearing it in
   `detach()` (alongside the chroot).
2. **Resolution composes volumes *over* the chroot, not as a step-1 Bind.** A new
   `apply_root_with_volumes()` runs in step 2 *after* `normalize_jailed` clamps
   `..` against the guest root `/`. The longest-matching volume prefix wins and
   the path is re-anchored under that volume's host target; otherwise the path
   falls through to the normal rootfs prefix. Putting volume matching **after**
   `..`-normalization is the security-critical choice: a guest cannot use
   `/data/../../etc` to climb out of a volume into the host, because the path is
   collapsed to `/etc` (clamped at the guest root) *before* any volume is
   considered, so it simply resolves under the rootfs. The empty-volume-list fast
   path keeps the common (no-volume) jailed process on the original `apply_root`.
3. **Reverse mapping** (`unjail_path_for`, used by `fchdir`/`*at`) also reverses
   volumes — a host path inside a volume target maps back to the volume's guest
   prefix (longest host-target match) — so `getcwd` inside a volume reports the
   guest path and a subsequent relative op is jailed exactly once (no double-jail,
   consistent with §45). Checked before the rootfs strip because a volume's
   contents live outside the rootfs subtree.
4. **Container plumbing.** `Container` gains a `volumes: Vec<(guest, host)>`
   field; `add_volume_mount(id, host_target, guest_prefix)` (Docker `-v` order,
   Created-state-only) records them; `add_process_task` installs each via
   `namespace::add_volume` after `set_root`; `remove_process_task`/`delete` clear
   them. Last-writer-wins on a repeated guest prefix (mirrors Docker re-mount).

**Alternatives considered.**
- *Implement volumes as step-1 Bind rules.* Rejected: step-1 rewrites stay in
  guest space and are then rootfs-prefixed, so they can't escape the jail to an
  arbitrary host path — exactly the thing a volume must do. They also wouldn't
  get `..`-clamped the same way, opening an escape.
- *A full mount-tree (longest-prefix mount table that subsumes the rootfs as the
  `/` mount).* The cleaner long-term model and the eventual `pivot_root` target,
  but a wholesale replacement of the just-stabilized chroot path (increments 7–8)
  carries real regression risk for no immediate functional gain. Volumes-over-
  chroot is additive, leaves the hardened chroot untouched, and delivers the
  user-visible `-v` feature now. The mount-tree refactor remains TD32's deferred
  scope.
- *Resolve volumes before `..`-clamping.* Rejected outright — it would let
  `/data/../../etc` escape a volume into the host. Normalization must come first.

**Limitations / deferred.** Volumes apply only to a *jailed* process (a volume on
an unjailed process is ignored — volumes are a container concept). No read-only
volume flag yet (Docker `-v ...:ro`); no `tmpfs`/named-volume types — these are
straightforward follow-ups on the same table. The `unjail` reverse mapping is
ambiguous only if a volume target is nested *inside* the rootfs subtree (it
prefers the volume), which does not occur for normal host-dir volumes.

**Where it bites.** `kernel/src/ipc/namespace.rs` (`PROCESS_MOUNTS`,
`add_volume`/`clear_mounts`/`volume_count`, `apply_root_with_volumes`,
`longest_volume_match`, `unjail_path_for` volume reversal, `detach` cleanup,
`test_volume_mounts`); `kernel/src/container.rs` (`volumes` field,
`add_volume_mount`, `add_process_task`/`remove_process_task`/`delete` wiring,
self-test 19).

## 47. Container auto-restart (`--restart`) and auto-remove (`--rm`) run through the kernel workqueue as a deferred reaper, driven off the init-exit hook

**Date:** 2026-06-30

**Decided by:** Claude (operator-approved scope). Same §40 pre-approval of the
container-runtime port; this records the implementation choices for Docker's
`--restart`/`--rm` lifecycle automation (increments 48–54). These are
implementation choices (where the respawn/delete work runs, and how the Docker
policy state machine is encoded), not an operator policy fork, so they are
resolved autonomously and recorded here.

**Context.** A real container runtime must react to a container's init process
*exiting*: `--restart` policies relaunch it, and `--rm` deletes the container.
The only place the kernel learns an init has died is `notify_init_exit(pid,
code)`, which is called from the **generic process-exit (zombie-transition)
path** — a context that holds scheduler state and cannot safely allocate a new
address space, read the VFS, or tear down an overlay. So the reaction cannot run
inline there.

**The decision.**
1. **Deferred reaper on the kernel workqueue.** `notify_init_exit` only updates
   container *state* under the table lock (→`Stopped`, records the exit code),
   decides whether a restart/remove is due, then `workqueue::submit`s a callback
   (`do_container_restart` / `do_container_autoremove`) that runs in the
   `kworker` task context where spawning, VFS reads, and overlay teardown are
   safe. This mirrors the existing `sched::supervisor` task-restart precedent. A
   full queue drops the action with a logged warning rather than blocking or
   spawning on the exit path.
2. **Docker restart-policy semantics encoded as a pure decision function**
   (`should_auto_restart(policy, exit_code, user_stopped, restart_count)`): `no`
   never restarts; `always`/`unless-stopped` restart on any exit;
   `on-failure[:N]` restarts only on non-zero exit, capped at N (0 = unlimited).
   `unless-stopped` is identical to `always` in our single-session model (there
   is no daemon restart to replay a "don't auto-start on boot" distinction).
   Being pure, it is exhaustively unit-tested without spawning anything.
3. **`user_stopped` gate distinguishes a graceful stop from a kill.** A user
   `stop()` sets `user_stopped=true`, which suppresses *every* restart policy
   (Docker: a `docker stop` is intentional and must not fight the user). A
   `kill()` does **not** set it — Docker still honours the restart policy after a
   kill. The flag is cleared on every (re)launch.
4. **`restart_count` is incremented in `notify_init_exit` (when it schedules an
   auto-restart) and reset to 0 only on a *manual* `start`/`restart`** — never on
   the internal `run_path`/auto-restart path. This makes an `on-failure:N` series
   actually terminate after N attempts instead of looping forever, while a human
   intervention re-arms the budget.
5. **Restart tears down a running container stop-before-kill.** `relaunch_recorded`
   calls `stop(id)` (leaves `Running`) *before* `kill(id)`, so when the old
   init's death reaches `notify_init_exit` the container is no longer `Running`
   and cannot trigger a spurious nested restart. This closes a self-restart race
   the naive kill-first order would open.
6. **`--rm` yields to `--restart`.** In `notify_init_exit` the auto-remove branch
   is an `else` of the restart branch: a container that is going to restart is
   never removed. Deletion is deferred identically because it touches the
   VFS/overlay; the container is already `Stopped` by reaper time, so `delete()`
   (which refuses a `Running` container) succeeds.

**Alternatives considered.**
- *React inline in `notify_init_exit` / from a softirq.* Rejected: the exit path
  and softirqs run in restricted contexts that cannot spawn a process or touch
  the VFS. The workqueue is the established "defer to full task context" channel.
- *A dedicated container-reaper kernel thread polling for dead inits.* Rejected as
  redundant — the workqueue already provides the task-context execution and a
  wakeup; a bespoke thread would duplicate it and add a polling loop.
- *Set `user_stopped` on `kill()` too (treat kill as a user stop).* Rejected —
  it contradicts Docker, where `docker kill` still triggers the restart policy.
- *Reset `restart_count` whenever the recorded command is replayed.* Rejected —
  it would make `on-failure:N` loop forever, defeating the cap.

**Update (increment 57): exponential restart back-off implemented.**
Auto-restart no longer fires immediately: `notify_init_exit` now schedules the
restart through an hrtimer with an exponential crash-loop back-off
(`restart_backoff_ns`: 100 ms, 200 ms, 400 ms, … doubling per consecutive
attempt, capped at 30 s), so an `always`-policy container that crashes instantly
can't spin the CPU in a tight respawn loop. The timer fires in ISR context and
hands the actual relaunch to the kworker via a trampoline
(`restart_backoff_fire` → `workqueue::submit(do_container_restart)`) — spawning
inline on the timer/exit path is unsafe. The back-off is derived from the
(already-incremented) `restart_count`; it is *not* reset after a period of
successful running (Docker resets after ~10 s up), because `restart_count`
doubles as the `on-failure:N` cap and resetting it would defeat the cap. In
practice the monotonic back-off is strictly safer (a flaky container backs off
more, never less).

**Update (increment 56): lifecycle event log.** A bounded (256-entry) ring
records create/start/die/stop/kill/pause/unpause/restart/destroy events
(`record_event`/`events_snapshot`, surfaced by `container events`). `record_event`
is lock-local (event-log lock only, never the container table), so it is safe
from within `with_table` closures and the process-exit path.

**Limitations / deferred.** Restart back-off is *not* reset after a successful
run window (see the increment-57 update above for why — it shares the
`on-failure:N` counter). `unless-stopped` collapses to `always` because there is
no persistent daemon to replay boot-time start decisions. `container ls -n/-l`
order by a monotonic per-table creation sequence (`created_seq`), added because
slot ids are reused and so are not creation order.

**Where it bites.** `kernel/src/container.rs` (`RestartPolicy` +
`parse_restart_policy`/`should_auto_restart`; `Container`/`ContainerConfig`/
`ContainerInfo` gain `restart_policy`/`restart_count`/`user_stopped`/
`auto_remove`/`created_seq`; `ContainerTable::next_seq`; `notify_init_exit`
rewrite; `do_container_restart`/`do_container_autoremove` workqueue callbacks;
`relaunch_recorded` stop-before-kill; `set_restart_policy`; self-tests
19u/19v/19w); `kernel/src/kshell.rs` (`container create restart=`/`rm`,
`update --restart`, `ls -a`/`-n`/`-l` + newest-first ordering, `inspect --json`).

## 48. Container named volumes and user-defined networks are runtime-owned registries; networks add IPAM but not (yet) a shared L2 bridge

**Date:** 2026-07-01
**Decided by:** Claude (autonomous) — within the operator-approved Docker/container-runtime port (open-questions Q15).

Two Docker-parity subsystems landed as sibling in-memory registries alongside
the container table (increments 59–61):

**Named volumes (`docker volume`, increment 59).** `kernel/src/volume.rs` is a
registry of runtime-owned backing directories under
`/var/lib/slate/volumes/<name>`, created on demand and bind-mounted into
containers via `-v NAME:/guest`. The source form is distinguished exactly as
Docker does — a leading `/` means a host bind mount, a bare name means a named
volume — so `-v` handles both with one flag. The registry is in-memory (like
the container table), but a volume's *data* lives on the ext4 rootfs and
survives until `remove`d, so create+populate+run behaves as expected within a
boot. Backing dirs are flat (`ROOT/<name>`), not Docker's `ROOT/<name>/_data`,
because our runtime owns the layout and there is no metadata sidecar to
separate from the data.

**User-defined networks with IPAM (`docker network`, increments 60–61).**
`kernel/src/cnetwork.rs` is a registry of named IPv4 subnets with address
management: `allocate` scans `[network+1, broadcast)` skipping the gateway and
taken addresses, `release`/`release_container` return leases to the pool.
`oci run --network NAME` reserves an unowned address *before* the container is
created (the interface must be configured from the container config, which is
built pre-create), then binds the lease to the container id after create via
`set_allocation_owner`; a failed create releases the reservation, and
`container::delete` calls `release_container(id)` so leases never leak. Default
subnets carve from `172.20.0.0/16` upward (clear of Docker's `172.17` default
bridge, inside the `172.16/12` private block).

**The tradeoff — IPAM without L2 bridging.** The named-network feature
deliberately delivers naming + conflict-free IPAM but *not* a shared layer-2
broadcast domain: each container keeps its existing per-netns veth-to-host link
(host/external connectivity via NAT), so two containers on the same named
network cannot yet address each other directly. This was a real fork:

- *Alternative A (chosen): ship IPAM now, defer L2 bridging.* Pros: the
  immediately valuable, fully-testable capability (removes the footgun of
  hand-picking a non-colliding `--net IP`) lands in two clean increments;
  `inspect` reports only what is real, so nothing over-promises. Cons: "same
  network" is not yet a connectivity guarantee, which could surprise a user who
  expects Docker's inter-container DNS/reachability.
- *Alternative B: build the shared bridge first, ship networks only when peers
  can talk.* Pros: matches Docker's connectivity semantics on day one. Cons:
  needs bridge↔veth port registration and frame plumbing between
  `net::veth::poll_all` and the `net::bridge` FDB — a substantially larger,
  riskier change — to deliver *any* of the (independently useful) naming/IPAM
  value.

Chose A because IPAM is useful standalone and the honest `inspect` output
prevents the surprise from becoming a silent correctness bug. The L2-bridge
follow-up is tracked as `known-issues.md` D-CNET-L2BRIDGE with a full design.

**Where it bites.** `kernel/src/volume.rs` (new); `kernel/src/cnetwork.rs`
(new); `kernel/src/main.rs` (`mod` + boot self-tests); `kernel/src/kshell.rs`
(`container volume`/`container network` subcommands + `docker` passthrough; the
`-v` source-form split; `oci run --network` reservation/bind/release wiring);
`kernel/src/container.rs` (`delete` → `cnetwork::release_container`).

## 49. `container diff` is overlay-only (Docker semantics), and the container records its `OverlayId` rather than re-deriving it from the overlay name

**Date:** 2026-07-01
**Decided by:** Claude (autonomous) — resolves open-questions Q16, within the
operator-approved Docker/container-runtime port (Q15). Q16's OPEN entry
recommended this option (A); no operator input was solicited because it is the
only *proper* (non-band-aid) implementation and stays inside the approved scope.

Docker's `docker diff <ctr>` lists filesystem changes of a container relative to
its image: `A`dded / `C`hanged entries live in the writable upper layer, `D`eleted
entries are whiteouts. This is *defined* only for an overlay rootfs. Our runtime
has two rootfs kinds — overlay-backed (`oci run`, real lower/upper/whiteouts) and
plain bind-rootfs (`container create` + `rootfs <dir>`, a chroot to a host dir
with no base to diff against).

**The decision (Q16 option A).** Implement `diff` only for overlay-backed
containers; plain bind-rootfs returns `InvalidArgument` ("no overlay rootfs").
`container::diff(id)` resolves the container's overlay, walks the upper via an
**iterative work-stack** (`Vfs::readdir`, bounded kernel stack — not recursion),
classifies each entry with `overlay::which_layer` (`Both`→Changed, `Upper`→Added),
appends `overlay::whiteouts` as Deleted, and returns the list sorted by path,
each formatted `"/{rel}"`. Rejected: option B (point-in-time baseline captured at
first `start()`) because it is not Docker's semantics and puts a full rootfs walk
+ per-container manifest on the start hot path; option C (both) because two
meanings of "diff" under one command is confusing.

**Sub-decision — store the `OverlayId` on the container, don't re-derive it.**
`diff` needs to recover the overlay from a container id. Overlays are created as
`oci-{image_name}`, so it *could* be looked up by reconstructing that name — but
that breaks under rename and couples the container to the overlay's naming
convention. Instead `Container` gained an `overlay_id: Option<OverlayId>` field,
set on the `oci run` path via `set_overlay_id` (Created-state-only, mirroring
`set_rootfs_mount`). Robust identifier > reconstructed name (matches the
CLAUDE.md "store stable identifiers, not derived references" rule).

**Blocking `container wait` (increment 62, same series).** Independently, the
old `container wait` busy-polled `wait_status` in a `yield_now()` loop — the
CLAUDE.md-forbidden busy-wait. Replaced with event-driven `container::wait(id)`:
register `set_wait_task(init_pid, task_id)`, re-check terminal state (lost-wakeup
guard), then `block_current()`; the init process's `remove_thread` exit hook wakes
the task. Returns `WaitOutcome::{Exited(code), Removed}`. This is the same
join mechanism `sys_wait4` uses and the proven basis for a future real
`container exec` (Q17).

**Where it bites.** `kernel/src/container.rs` (`overlay_id` field +
`set_overlay_id` + `WaitOutcome`/`wait` + `DiffEntry`/`DiffKind`/`diff`, boot
self-tests 19k2/19k3); `kernel/src/fs/overlay.rs` (`upper_path`, `whiteouts`
accessors); `kernel/src/kshell.rs` (`container diff` arm, `container wait` rewrite,
`oci run` → `set_overlay_id`, `docker` passthrough + help/usage).

## 50. `docker build` writes OCI images natively (`oci::write_image`/`build_image`); base-image layers are carried forward as verbatim blobs, not re-tarred

**Date:** 2026-07-01
**Decided by:** Claude (autonomous) — within the operator-approved Docker/
container-runtime port (Q15). Ungated: the image writer + every non-`RUN`
Dockerfile instruction need no exec, so no operator fork was required; only
`RUN` remains gated on Q17.

`docker build` is the last big Docker-port capstone. It needed two things
`oci.rs` lacked: an OCI image **writer** (previously it could only *read*
images) and a **Dockerfile interpreter**.

**Writer (`write_image`).** Authors a standard OCI layout under a dest dir:
per layer, build an uncompressed tar → `diff_id` = sha256(tar) → gzip → blob
(digest = sha256(gzip)); then config, manifest, `index.json`, `oci-layout`.
Round-trips byte-identically through the existing `load_image`. Factored a
shared `finish_image` tail (config+manifest+index assembly from already-written
layer descriptors) so both the plain writer and the builder feed it.

**Builder (`build_image`).** Interprets a Dockerfile into an `ImageSpec` +
layers, then calls the writer. Supports every instruction **except `RUN`**:
FROM (`scratch` or a local OCI image dir), COPY/ADD (file + recursive directory
sources with Docker dest semantics), ENV (both forms + quoted values),
CMD/ENTRYPOINT (JSON exec + shell form → `/bin/sh -c`), WORKDIR (absolute +
relative-append), USER, EXPOSE (default `tcp`), LABEL, ARG, plus
`${VAR}`/`$VAR`/`${VAR:-default}` expansion and `\`-continuation/comment
handling.

**Key tradeoff — base-image layer carry-forward.** `FROM <local-oci-dir>`
inherits the base image's config *and* its layers. Two ways to carry the
layers: (a) **copy the base layer blob files verbatim** into the new image and
reuse the base's descriptors + `diff_id`s, or (b) extract each base layer and
re-tar/re-gzip it into a fresh blob. Chose **(a)**: it is byte-exact
(identical digests, so content-addressed dedup still works), avoids a
decompress→recompress round-trip that could perturb bytes, and is far cheaper.
The cost is that `build_image` must special-case "carried" layers (their
`diff_id`s come from the base config, not recomputed) — handled by seeding
`layer_descs`/`diff_ids` with the base's before appending freshly-built COPY
layers. `finish_image` then treats the concatenation uniformly.

**Other calls.** (1) `RUN` is rejected with a precise
`BuildError::RunUnsupported { line }` (it needs the Q17-gated in-container
exec), not silently dropped — an honest failure beats a wrong image. (2)
Unsupported instructions (VOLUME/HEALTHCHECK/STOPSIGNAL/SHELL/ONBUILD) are
likewise rejected with a clear message rather than ignored, for the same
reason; MAINTAINER maps to the conventional `maintainer` label. (3) The
Dockerfile is parsed as UTF-8 (a Dockerfile is text, and our VFS already models
directory-entry names as `String`), so COPY paths ride the same `&str` path
surface as the rest of `oci.rs`. (4) `BuildError` is a distinct type from
`KernelError` so the shell prints a Docker-style diagnostic.

**Where it bites.** `kernel/src/oci.rs` (`ImageSpec`/`BuildLayer`/`LayerFile`,
`write_image`/`finish_image`/`create_layout_skeleton`, `build_image` +
`BuildError` + Dockerfile helpers, self-tests 11–12); `kernel/src/kshell.rs`
(`oci build` arm + `docker build` shim delegate + help/usage). Follow-up: `RUN`
support arrives with Q17's `container exec`.

## 51. Named image store — a single shared OCI layout at `/var/lib/images` keyed by `ref.name` annotations, with blob GC on `rmi`

**Date:** 2026-07-01
**Decided by:** Claude (operator-approved scope) — within the operator-approved
Docker/container-runtime port (Q15). No operator fork: this is the
obviously-correct Docker-parity default, and the on-disk internals are
reversible.

Until now SlateOS had no image *store* keyed by name: `oci run`/`FROM`/`docker
images` all operated on an on-disk OCI layout **directory path**. That works but
diverges from Docker, where images are referenced by `name:tag`. The store adds
that name→image mapping.

**Design.** A single OCI image layout lives at `/var/lib/images`. Its
`index.json` holds one manifest descriptor **per tag**, each carrying an
`org.opencontainers.image.ref.name` annotation — the real OCI multi-image
pattern (the same layout a registry pull populates). All tags share one
content-addressed `blobs/sha256/` pool, so identical layers across images are
stored once.

**Operations (`oci.rs`).** `store_tag_from_dir(dir, ref)` imports a built image
directory into the store (copies its blobs, adds/replaces the tag);
`store_add_tag(src, dst)` re-tags an existing ref with no blob recopy (`docker
tag`); `store_resolve(ref)` → manifest digest; `store_list()` → rows for `docker
images`; `store_remove(ref)` drops a tag and **garbage-collects** every blob no
longer reachable from a surviving manifest (walk each remaining manifest → keep
its manifest+config+layer hexes → delete the rest). `normalize_ref` defaults a
bare name to `:latest` and leaves `@digest` refs untouched.

**Key tradeoff — shared layout + GC vs. per-image directories.** Alternative
(b): keep every image in its own directory and make the "store" just a
name→directory map. Chose the **shared single-layout** approach: it is what
Docker/registries actually do, gives free cross-image layer dedup, and keeps a
single `oci-layout`/`index.json` to reason about. The cost is that deletion is
no longer "rm -rf a directory" — it must reference-count blobs across all
remaining tags (the GC pass). That GC is the one piece of real complexity, and
it is covered by self-test 20 (two tags sharing blobs: removing the first GCs
nothing; removing the last GCs everything).

**Where it bites.** `kernel/src/oci.rs` (`STORE_DIR`, `StoredImage`/`StoreEntry`,
`normalize_ref`, `store_read_index`/`store_write_index`, `copy_all_blobs`,
`store_tag_from_dir`/`store_add_tag`/`store_resolve`/`store_list`/`store_remove`,
`collect_manifest_blob_hexes`, self-test 20); `kernel/src/kshell.rs` (`oci
tag`/`images`/`rmi` arms + `docker` shim routes for `images`/`tag`/`rmi`).

**Follow-up (done, same day).** Store references are now resolvable everywhere
an image is named, via `resolve_image_source(arg)` — which treats `arg` as an
on-disk OCI layout directory if it has an `oci-layout` marker, else looks it up
in the store (`store_resolve` → `load_manifest_by_digest(STORE_DIR, digest)`,
returning `STORE_DIR` as the blob-source since all store images share its blob
pool). Wired into `FROM name:tag` (base inheritance), `oci`/`docker run`,
`oci inspect|layers|history`, and `oci build -t name:tag` (auto-import the built
image into the store). A dedicated `load_manifest_by_digest` was needed because
the store is a *multi-manifest* layout — `load_image`'s host-platform manifest
selection would be ambiguous across tags. Covered by self-test 21.

**Follow-up 2 — store-aware `save`/`load` (done, same day).** `oci save
name:tag` exports *one* image (not the whole shared store) into a standalone
single-manifest layout via `store_export_ref` (copies only that manifest's
config + layer blobs and writes a one-entry `index.json` preserving the
`ref.name` annotation), then tars it; `oci load` extracts a tar and calls
`store_import_dir`, which copies the blobs into the shared pool and re-adds each
`ref.name`-annotated manifest as a store tag — matching Docker, where `load`
repopulates the local image store. `load`'s dest-dir is now optional (temp dir +
store import when omitted). The index (de)serialisers were generalised to a
`dir` parameter (`serialize_index`/`write_index_at`/`read_index_at`) so the same
code writes the store index and per-export indices. Covered by self-test 22
(build → tag → export → wipe store → import → resolve + extract original bytes).

**Follow-up 3 — `commit`: author an image from a container's changes (done,
same day).** `docker commit <container> [repo:tag]` produces a *new image* from
a running container's filesystem changes. This is distinct from the existing
native `container commit`, which *clones a container* (snapshots one container's
rootfs into a second independent container). Both semantics are legitimate and
useful, so rather than repurpose the shipped `container commit`, the image-
production path got its own verb and the two are kept separate:

- **`oci commit <container-id> <dest-dir> [name:tag]`** and **`docker commit
  <container-id> <name:tag>`** → image production (`oci::commit_image` →
  `container::commit_image`). Captures the container's overlay **upper** layer
  (added/changed files, walked iteratively via VFS `readdir`/`metadata`/
  `read_file`) plus its **whiteouts** (deletions, emitted as OCI `.wh.<base>`
  empty-file markers) as **one new layer** stacked on top of the base image the
  container was created from. The base image's config (Env/Cmd/Entrypoint/
  WORKDIR/USER/… and `onbuild`) and existing layers are carried forward verbatim
  (blobs copied by digest, descriptors + diff_ids reused), and a
  `#(nop) COMMIT` `history[]` entry is appended. Written as a standalone OCI
  layout at `dest_dir`; `docker commit` additionally stages that layout in a
  temp dir and imports it into the store under the given `name:tag`, then
  discards the temp dir (Docker's `commit` leaves no dir artifact).
- **`container commit <src-id> <new-name> <rootfs-dir>`** → unchanged
  (container clone).

To recover the base image at commit time, the container now records the image
it was created from: `ContainerConfig::image_source` (an OCI-layout dir path or
a `name:tag` store reference) is stamped at `oci run` time and stored on the
`Container`; `container::commit_image` reads it back and resolves it via
`oci::resolve_image_source` (dir-or-reference). A container created from a bind
rootfs (no image) or with no overlay is rejected with `InvalidArgument` — there
is no base to extend / no writable layer to capture. Covered by self-test 23
(build base with Cmd/Env → synthesise an overlay upper + a whiteout →
`commit_image` → assert base-layer carried + exactly one commit layer +
Cmd/Env preserved + COMMIT history entry + the commit layer's tar holds the
added files and the `.wh.` marker).

**Decided by:** Claude (operator-approved scope — the Docker/container-runtime
port was green-lit by Q15). The `docker commit`→image-production vs. native
`container commit`→clone split is a Docker-parity choice within that scope, not
a genuine fork; both behaviours are retained under distinct verbs so nothing is
lost. `RUN`/`HEALTHCHECK` (in-container rootfs exec) remain gated on Q17.

## 52. The root netns default gateway stays owned by the interface config; the route table holds only non-default routes, and `resolve_next_hop` consults the table first then falls back to the interface gateway

**Date:** 2026-07-02
**Decided by:** Claude (autonomous) — completes TD18 follow-up (b) (route-table
write syscalls). Clearly-correct default with no operator fork needed: it adds
a capability to an existing subsystem without changing established semantics.

**Context.** The kernel already had a full per-namespace routing table
(`netns::add_route`/`remove_route`/`route_lookup` with longest-prefix-match,
`routes`), but two things were missing for the *root* namespace: (1) no syscall
exposed it to userspace, so `ip route add`/`route add` for non-default routes
hard-errored; and (2) `net::ipv4::resolve_next_hop`'s root branch ignored the
table entirely, using only `interface::info().gateway`. The new
`SYS_NET_ROUTE_ADD`/`_DEL`/`_LIST` (857/858/859) expose the table, and the root
branch now consults `route_lookup(ROOT_NS, dst)` before the interface fallback.

**The decision.** There are two plausible homes for the *default* route
(`0.0.0.0/0`):

- **(A, chosen)** Keep the default gateway in the interface config
  (`SYS_NET_IF_CONFIG` GATEWAY field). The route table holds only *specific*
  (non-default) routes. `resolve_next_hop` tries the table first (specific
  routes win by longest-prefix-match), and if nothing matches falls back to the
  interface gateway for the implicit default + connected delivery. `ip route add
  default via X` / `route add default gw X` continue to write the interface
  gateway (already wired in follow-up (a)); only non-default routes touch the
  table.
- **(B, rejected)** Make the route table the single source of truth, with
  `default via X` inserting a `0.0.0.0/0` table entry and the interface
  `gateway` field becoming a derived cache (or removed).

**Why A.** (1) No migration/reconciliation: the default-gateway semantics from
follow-up (a) and every existing `resolve_next_hop` path are unchanged, so this
is purely additive and backward-compatible — an empty table behaves exactly as
before. (2) The display tools already synthesize the default route from
`SYS_NET_IF_INFO` separately from listed routes, so keeping the two sources
distinct matches what userspace already renders. (3) It avoids two writers
racing on the same `0.0.0.0/0` slot. **Cost of A:** the default route is not a
row in the route table, so a naive `route -n` merge must union the interface
default with the table (the tools already do this). **Cost of B:** a larger,
riskier refactor touching `configure()`, `resolve_next_hop`, and every place
that reads `info().gateway`, for a mostly-cosmetic unification. Revisit B only
if we later need multiple default routes or per-route metrics on the default.

## 53. Firewall write syscalls (860–864) mirror the kernel Rule model exactly; the `fw` tool's richer on-disk format skips (with a warning) any rule the kernel cannot represent rather than pushing a broader rule

**Date:** 2026-07-02
**Decided by:** Claude (autonomous) — completes TD18 follow-up (b) (firewall
write syscalls). Additive capability on an existing subsystem; no operator fork.

**Context.** The kernel already had a full per-namespace packet-filtering
firewall (`net::firewall`: `Rule { active, direction, action, protocol, src_ip,
src_prefix, dst_port, priority, match_count }`, global + per-ns tables, packet
path via `check_inbound_ns`/`check_outbound_ns`, reads served by
`/proc/net/firewall`). No syscall exposed the *write* path, so `fw enable`,
`fw allow/deny`, `fw policy`, `fw delete`, `fw reset` could only edit the local
`/etc/fw.rules` file and never touched the running kernel — the old `fw_ioctl`
stub returned `ENOSYS`. The new `SYS_NET_FW_ENABLE`/`_SET_POLICY`/`_ADD_RULE`/
`_DEL_RULE`/`_FLUSH` (860–864, all root-gated, operating on the caller's netns
with root ns == the global firewall) close that gap.

**The decision — ABI shape.** `ADD_RULE` takes a fixed 12-byte binary record
(`[direction, action, protocol, src_prefix, dst_port:u16le, priority:u16le,
src_ip:4]`) rather than a text line. Binary avoids a parser in the kernel
syscall path (the kernel has no reason to reparse the human format), keeps the
decode branch-simple (destructure the array by value — no indexing), and mirrors
the `Rule` fields 1:1. `ENABLE`/`SET_POLICY`/`DEL_RULE` are scalar-only; `FLUSH`
takes no args. Reads stay on `/proc/net/firewall` (no read syscall), matching
the route-syscall precedent (§52) where listing has both a syscall and procfs
but control is the syscall's job.

**The decision — model mismatch handling.** The `fw` tool's on-disk rule format
is richer than the kernel model: it carries `src_port` and `dst_ip` dimensions
the kernel `Rule` has no field for. Two options:

- **(A, chosen)** When a rule constrains `src_port` or `dst_ip`, the tool
  **skips** pushing it to the kernel and prints a warning; the rule is still
  saved to `/etc/fw.rules` (so no user data is lost and a future richer kernel
  model could honour it). `to_kernel_record` returns `None` for such rules.
- **(B, rejected)** Drop the unrepresentable dimension and push the rule anyway
  (e.g. ignore `dst_ip`, matching all destinations).

**Why A.** Silently widening a rule (B) is a security footgun: an operator who
wrote "allow from 10.0.0.5 to 10.0.0.10:80" would get "allow from 10.0.0.5 to
*:80" installed in the kernel — strictly more permissive than intended, exactly
the wrong direction for a firewall to err. A explicitly refuses to install a
rule it cannot honour and tells the operator, which is fail-safe. **Cost of A:**
the kernel ruleset can diverge from the file (some file rules aren't installed);
the tool's warning makes this visible, and `fw list` reads kernel state so the
divergence is observable. Revisit if/when the kernel `Rule` gains `src_port`/
`dst_ip` fields — then A's skipped rules become representable with no ABI change
on the enable/policy/del/flush syscalls (only the ADD record grows).

**Positional delete correctness.** Because unrepresentable rules are never
pushed, the kernel index of a rule ≠ its position in the tool's list. `fw
delete N` computes the kernel index as the count of *representable* rules before
position N and only issues `DEL_RULE` if the target rule was itself pushed —
avoiding an off-by-one that would delete the wrong kernel rule.

---

## 54. Next-big-initiative prioritization (Q22) — root-cause the ring-3 spawn/reap SMP timing race first (option D)

**Date:** 2026-07-02
**Decided by:** Operator (Claude recommended A + D-when-reachable; operator chose D)

**Context.** With the editor merge-on-external-change request complete and a full
cross-phase roadmap survey showing the project extraordinarily mature, the only
substantial remaining work fell into two buckets: giant external ports (dev
toolchain, Chromium, GPU/Mesa, extra filesystems) and deferred-risky internal
kernel work (the ring-3 spawn/exec/reap SMP timing race + TD31 symmetric cgroup
accounting + TD32 mount-tree remainder). Q22 asked the operator which to green-
light next: **A** dev toolchain (gcc/CPython/fastpy), **B** Chromium/web stack,
**C** GPU drivers + Mesa, **D** root-cause the spawn/reap SMP race, **E** extra
filesystems / container mount-tree.

**Decision.** The operator chose **D**, with the stated rationale "I like all
bugs to be solved asap." This authorizes fully working the ring-3 spawn/exec/reap
SMP timing race in a supervised session — the one class of kernel work CLAUDE.md
otherwise warns not to destabilize unsupervised. The operator being reachable
satisfies the prior condition (sanity-check boot stability across several runs).

**Why D over the recommended A.** Claude recommended A (dev toolchain) as the
highest-leverage *port* with D done when the operator is reachable. The operator
prioritizes bug elimination over new capability: fixing the spawn/reap race
unblocks TD31 (symmetric cgroup nr_tasks accounting) and the deferred fork/wait
E2E self-test, and directly improves boot stability — clearing the deferred-risky
kernel-bug bucket before taking on a large port. Both are defensible; the operator
owns the product/prioritization call (which is exactly why it was reserved).

**Consequence / plan.** Establish a green boot-test baseline, study the spawn/
exec/reap + kill/reap paths and the prior TD31 patch, assess whether the
B-PREEMPT-SPINLOCK fix (2026-07-01, claimed true root cause) already resolves the
residual WATCH flakes (B-DASH-STDIN-FLAKE, B-PTHREAD-YIELDBUDGET), instrument any
remaining race, and re-attempt TD31 boot-testing >=3x for stability. The other
Q22 options (A/B/C/E) remain available for a future steer and are NOT closed by
this decision.

---

## 55. Boot ordering — enable interrupts BEFORE the ring-3 self-test battery (not after)

**Date:** 2026-07-02
**Decided by:** Claude (operator-approved scope — Q22 option D, "root-cause this
hang," authorized working the ring-3 spawn/reap path; this is the resulting fix).

**Context.** `kernel_main` (`main.rs`) historically deferred `cpu::sti()` until
Step 21, *after* the entire ring-3 integration self-test battery (dozens of real
Linux-ABI processes: glibc/dash/gcc/make, which fork, CoW-clone, exec, and
demand-page file-backed mappings). So the whole battery ran with **IF=0**. The
battery is driven cooperatively by `sched::yield_now()` loops, which work without
a timer, so it *functioned* — but it monopolized the BSP with interrupts disabled
for many seconds. That is the "long operation under IRQs-disabled" anti-pattern:
no timer ticks means no preemption, blind timer-driven watchdogs, and a starved
hard-lockup-watchdog kick. In debug builds (heap poisoning) the battery's
O(n)-over-large-data work is seconds-long, so jitter occasionally crossed the
~9.8 s watchdog / harness-timeout threshold → the intermittent "BSP-dead
total-silence hang" (known-issues.md B-PTHREAD-YIELDBUDGET). Two independent
seconds-long IF=0 offenders were found (SHA-256 auto-versioning; page-fault file
reads + poison_free), proving per-offender fixes were band-aid accumulation.

**Decision.** Move the interrupt enable (`idt::init_irq_stack(0)` + `cpu::sti()` +
APIC-timer verification) to the init/test seam — after all deterministic
kernel/subsystem init and in-kernel self-tests, immediately before the first
ring-3 spawn self-test. The battery now runs with interrupts on and preemption
live, exactly as userspace runs in steady state.

**Alternatives considered.**
- *Keep sti late; fix each IF=0 offender individually (cap SHA-256 size, skip
  poisoning during staging, etc.).* Rejected: band-aid accumulation; new
  offenders keep appearing in the same window; doesn't address the anti-pattern.
- *Don't arm the hard-lockup watchdog during the IF=0 battery.* Rejected: hides
  the symptom (still slow, still non-preemptive, a real deadlock would still go
  silent) rather than fixing the root; the watchdog false-fire is a *correct*
  signal that the window is structurally wrong.
- *Move sti even earlier (right after IOAPIC init, before device/fs init).*
  Deferred: wider blast radius (network/block/fs init would change to IF=1) for
  no additional benefit to the battery; the init/test seam is the natural, minimal
  boundary. Could be revisited if those init steps later prove slow under IF=0.

**Pros.** Eliminates the entire seconds-long-IF=0 class by construction; the
timer-driven liveness/hung-task watchdogs become live during the battery (a
genuine clone/CoW/reap deadlock now yields a task-table dump instead of silence);
boot is ~2× faster (BOOT_OK 91 s vs historical 161–229 s) since ring-3 children
get timer-driven CPU + interrupt-driven I/O completion; the self-tests now run in
a *representative* (preemptive, interrupts-on) environment rather than an
artificial cooperative one.

**Cons / risk.** Enabling preemption during boot self-tests adds real concurrency
that the cooperative-only path masked; a latent spawn/reap/futex race could now
surface at boot. This is accepted deliberately — such races are real bugs that
occur in production (always interrupts-on), so exposing them in testing is
correct, not a regression. Mitigation: validated by a green single boot plus a
20-boot watchdog-armed soak. Easily reversible (a code move) if a specific
ordering assumption is found to require IF=0.

**Where it lives.** `kernel/src/main.rs` Step-21 block (relocated) + the two
tail validations (`sleep_ns`, `softirq`) that must follow interrupt-enable but
need not precede the battery. Commit `c596b2fcc`.

## 56. Page-fault handler re-enables interrupts when the faulting context had them (preemptible #PF)

**Date:** 2026-07-02
**Decided by:** Claude (operator-approved scope — Q22 option D, continuation of
§55's root-cause of the ring-3 hang; this closes the residual IF=0 window).

**Context.** After §55 made the *battery* preemptible, a fresh watchdog-armed
soak still caught one recovered NMI false-fire whose RIP landed in a single page
fault (`resolve_subpaged_fault`). Root cause: `#PF` is dispatched through an
interrupt gate (IDT type 0xE), so `handle_page_fault` ran with **IF=0 for its
whole duration**. One fault can be long — demand-paging a subpaged file frame
reads up to 16 KiB through the VFS, CoW/large copies touch many pages, and debug
heap poisoning makes every alloc/free O(size) per-byte — so a single slow fault
could hold IF=0 past the ~9.8 s hard-lockup threshold even with everything else
preemptible.

**Decision.** In `handle_page_fault`, after capturing CR2, `cpu::sti()` **iff the
faulting context's saved `RFLAGS.IF` was set**. This makes fault resolution
preemptible for faults taken from interruptible contexts (the common case: ring-3
demand paging, and kernel code running with interrupts on), matching Linux's
`do_page_fault` calling `local_irq_enable()` early.

**Alternatives considered.** (a) Widen the ~9.8 s watchdog threshold for
debug+poison builds — rejected: masks the anti-pattern instead of fixing it, and
makes the watchdog less useful. (b) Re-enable interrupts only around the specific
long operation (the VFS read) — rejected: more fragile (must be re-audited as new
long ops appear on the fault path); the Linux-style early enable covers all of
them by construction. (c) Convert the #PF IDT entry to a trap gate — rejected:
that would unconditionally leave IF at its prior value with no way to keep it
disabled for faults from IF=0 contexts, and would not clear the nested-CR2
hazard; the explicit conditional `sti` after capturing CR2 is safer and clearer.

**Pros.** Closes the residual single-fault IF=0 window by construction; timer
tick / preemption / liveness+hard-lockup watchdogs all stay live across even a
long demand-paging or CoW fault; consistent with how these same paths already run
under IF=1 in syscall context.

**Cons / risk.** Page-fault resolution is now genuinely reentrant/preemptible —
a nested fault or a timer preemption can occur mid-resolution. This is safe: CR2
is captured into a local *before* the `sti`, so a nested fault can't clobber the
value we resolve against; and faults from IF=0 contexts (ISR/scheduler/raw-spin
critical sections) keep interrupts disabled via the conditional, so we never
widen interruptibility beyond what the interrupted code already permitted.

**Where it lives.** `kernel/src/idt.rs` `handle_page_fault`, immediately after
the CR2 read.

## 57. Only the outermost timer IRQ handler re-enables interrupts (bounded IRQ-stack nesting)

**Date:** 2026-07-03
**Decided by:** Claude (operator-approved scope — Q22 option D, "root-cause the
ring-3 spawn/reap hang"; this is the *actual* root cause and its fix).

**Context.** The intermittent (~5 %/boot) ring-3 self-test wedge tracked under
B-PTHREAD-YIELDBUDGET was finally caught with a real kernel backtrace (the
first-NMI one-shot dump added to `idt.rs::handle_nmi` this session). It is **not**
a livelock/reap/futex race and **not** SMP (QEMU boots 1 CPU): it is a **kernel
IRQ-stack overflow**. `handle_timer_irq` re-enables interrupts *while executing on
the fixed 16 KiB per-CPU IRQ stack* — inside `softirq::process_pending` (its
internal `STI`) and via an explicit pre-preempt `sti`. The softirq `IN_SOFTIRQ`
guard bounds softirq *work* but not the raw interrupt re-enable, so when a handler
outlasts the ~10 ms tick period (trivial in the poison-debug build, where each
file-page read does a `relatime → clock_monotonic → tsc_freq` clock call and heap
ops are `O(size)`), the next timer nests on the same IRQ stack, re-enables again,
and recurses until the guard page faults (`0xffffc10000028000`) → fatal `#PF`.

**Decision.** Only the **outermost** timer handler re-enables interrupts. Using the
per-CPU hardirq depth already maintained by `cputime` (new accessor
`cputime::irq_depth()`), `handle_timer_irq` computes `nested = irq_depth() > 1`
after `enter_irq()`; when nested it skips `process_pending` and the explicit `sti`,
running its whole body with IF=0. Since vector 32 is an **interrupt gate** (IF
auto-cleared on entry) and the nested handler never sets IF, no further timer can
fire before it returns — hard-capping nesting at **depth 2**.

**Alternatives considered.** (a) Grow / guard-expand the IRQ stack — rejected:
merely raises the depth at which it overflows; unbounded nesting is still
unbounded. (b) Widen the tick period / disable the per-tick liveness check —
rejected: masks the anti-pattern (holding/looping in IRQ context too long) rather
than bounding it, and slow handlers can exceed *any* fixed period under the poison
heap. (c) A dedicated re-entrancy latch just for the timer — rejected: `cputime`
already tracks exactly the hardirq depth we need; a second counter would be
redundant state to keep in sync. (d) Never re-enable interrupts in the timer
handler at all — rejected: the outermost handler legitimately needs IF=1 for
softirq processing (device IRQs must not be blocked during the softirq scan) and
for the deferred-preempt path to save a preempted task with IF=1.

**Pros.** Bounds worst-case IRQ-stack depth to 2 by construction, independent of
per-handler cost or timer frequency (incl. hrtimer tick-shortening); no new state
(reuses `cputime.irq_depth`); softirq bits from a nested tick are simply drained by
the outer frame's own loop — identical to the existing `IN_SOFTIRQ` short-circuit
but without ever toggling IF.

**Cons / risk.** A nested tick does slightly less work: it skips softirq processing
(deferred one tick to the outer/next handler — already the designed spillover
behavior via `MAX_SOFTIRQ_LOOPS`) and does not itself request the outer preempt
re-enable (harmless: nested IRQs never run `do_deferred_preempt`; the outermost
frame owns preemption). Net effect is strictly *less* work in an already-nested
context, which is the intent.

**Where it lives.** `kernel/src/apic.rs` `handle_timer_irq` (the `nested` guard on
`process_pending` and on the pre-preempt `sti`); `kernel/src/cputime.rs`
`irq_depth()` accessor.

---

## 58. `container exec` semantics (Q17) — keep the netns-debug facade AND add real rootfs-binary exec under a distinct verb (option B)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended B).

**Context.** Our shipped `container exec <id> <builtin>` switches into the
container's **network namespace** and runs a **kshell builtin** there — a
network-debugging facade, not Docker's `docker exec` (which spawns a **new
program from the container's own rootfs** inside the running container's
namespaces + cgroup). The netns-debug facility is genuinely useful and would be
*lost* if `exec` were simply replaced. `docker build`'s `RUN`/`HEALTHCHECK`
instructions need the *real* rootfs exec.

**Decision.** Build **both, under distinct verbs.** `container exec` keeps its
netns-debug meaning; add a new verb (`container run-in <id> <path> [args…]`, and
accept `container exec --rootfs` as an alias) that spawns the rootfs binary in
the container's namespaces + cgroup and joins on its exit code (reusing the
proven `set_wait_task`→`block_current` join used by `container::wait`). The
`docker exec` delegate maps to the real rootfs path. `docker build`'s
`RUN`/`HEALTHCHECK` consume the real exec.

**Alternatives.** (A) Replace the facade with real exec — rejected: deletes the
netns-debug facility. (C) Keep facade only — rejected: leaves a real Docker gap
and blocks `RUN`/`HEALTHCHECK`.

**Where it lives.** `kernel/src/kshell.rs` (`container exec` arm + new `run-in`
arm + `docker` delegate map), `kernel/src/container.rs` (new
`exec(id, argv) -> KernelResult<i32>`), `kernel/src/oci.rs` (`build_image`
`RUN`/`HEALTHCHECK`). Supersedes known-issues D-CONTAINER-EXEC-WAIT.

---

## 59. GPU acceleration scope (Q18) — build the kernel-side virtio-gpu render-ioctl dispatch now with honest "no-3D" reporting; defer the Mesa port (option B)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended C; operator chose B).

**Context.** Q15 green-lit the GPU-accel initiative; the 2D foundation is done.
Real 3D is gated on a virgl-capable test environment (our headless TCG CI exposes
plain virtio-gpu with **no** `VIRTIO_GPU_F_VIRGL`) and the large external Mesa
port.

**Decision.** Build the kernel-side virtio-gpu render-ioctl dispatch now with
**honest "unsupported" reporting**: `GETPARAM` reports `3D_FEATURES=0`,
`GET_CAPS` returns no capsets, every 3D-requiring ioctl returns the correct
errno; verified by a ring-3 self-test that opens `renderD128` and issues the
ioctls. The Mesa port stays deferred until a virgl test environment exists.

**Alternatives.** (A) Invest in virgl env + Mesa now — deferred. (C) Stop at the
foundation — operator chose to land the ioctl ABI now.

**Where it lives.** `kernel/src/syscall/linux.rs` `drm_card_ioctl` (new
`DRM_COMMAND_BASE`-range arm routing to `drm::virtgpu_uapi`), plus a ring-3
`renderD128` ioctl self-test.

---

## 60. Container network model (Q19) — generalise to multi-network membership (Docker parity, option B)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended B).

**Context.** Docker containers can join **multiple** user-defined networks, each
with its own interface + address + embedded-DNS scope. Our model assumed **one**
veth pair per container. `container network connect/disconnect` needs a model
decision.

**Decision.** Generalise the data model to **N interfaces per container.**
`Container` holds a list of `(netns-iface, veth_pair, network_name, ip)`
memberships; `network connect` allocates a new veth into the running container's
netns, configures it, attaches it to that network's bridge, and registers DNS
names; `network disconnect` tears one membership down. `inspect`/`ps` become
per-network. Its own dedicated increment (a real refactor).

**Alternative.** (A) Single-network minimal — rejected: diverges from Docker.

**Where it lives.** `kernel/src/container.rs` (`Container.veth_pair` → membership
list; runtime `attach_network`/`detach_network`), `kernel/src/cnetwork.rs`
(runtime connect), `kernel/src/kshell.rs` (`container network
connect|disconnect` arms + `docker` delegate).

---

## 61. Hard-lockup (BSP-dead) detector (Q20) — build the `i6300esb` watchdog + inject-nmi detector (option A), opt-in behind the boot-test flag

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended A).

**Context.** The BSP-dead variant of `B-PTHREAD-YIELDBUDGET` (BSP wedged with
IF=0, total serial silence) is uncatchable by any IF-gated software watchdog;
only an NMI can interrupt it. Under our TCG single-CPU boot test the one workable
NMI source is QEMU's `i6300esb` PCI watchdog with `-action watchdog=inject-nmi`.
The harness half (opt-in `boot-test.sh --hard-lockup-watchdog` flag) landed
2026-07-01.

**Decision.** Build the detector (option A), keeping it **opt-in** behind the
existing flag so the shared boot harness is untouched by default. Kernel half: an
`i6300esb` driver (BAR map + periodic kick), a dedicated NMI IST stack, and
`handle_nmi` → `sched::dump_task_table`, armed over the boot ring-3 window
(mirroring `sched::liveness_arm/disarm`). A diagnostic, not a fix.

**Alternatives.** (B) Attack root cause without a catcher — §57 already
root-caused/fixed the observed overflow variant; the detector remains valuable
for residual BSP-dead repro. (C) Defer — operator chose to build it.

**Where it lives.** `scripts/boot-test.sh` (flag landed), the `i6300esb` driver
lives in `kernel/src/hardlockup.rs` (BAR map + periodic kick, ~4915 ms/stage),
`kernel/src/idt.rs` (`handle_nmi`, `isr_nmi` on IST2 → `hardlockup::classify_nmi`
→ `sched::dump_task_table`), `kernel/src/gdt.rs` (dedicated NMI IST2 stack),
`kernel/src/main.rs` (`hardlockup::init/arm/disarm` over the boot ring-3 window),
`kernel/src/sched/mod.rs` (`hardlockup::kick()` from `timer_tick`).

**Validated 2026-07-14.** A `boot-test.sh --hard-lockup-watchdog` run exercised
the detector end-to-end: it armed over the ring-3 window, detected ~9.7 s of real
BSP kick-starvation, delivered an NMI on the dedicated IST2 stack, and dumped a
usable rbp-chain backtrace + task table. The starvation was *not* a deadlock — it
was a per-page-fault `serial_println!` storm saturating the serial port and
delaying the timer-driven kick; that separate bug (`B-FAULT-SERIALSTORM`, routed
to `klog!(Trace, …)`) was found *because* the watchdog fired. Net: the detector
works as designed and immediately earned its keep as a diagnostic.

---

## 62. `nft`/`iptables` compat tooling (Q21) — keep as an explicit parser/pretty-printer only; fix the docs; steer users to `fw` (option C)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended C).

**Context.** `userspace/nft` (also `iptables`/`ip6tables` via `argv[0]`) is
stateless: each invocation builds a fresh `Ruleset`, applies one command, prints,
and discards it — it never persists or touches the kernel, despite a module doc
claiming persistence. The native `fw` tool now fully configures the kernel
firewall (§53). The kernel firewall model is far narrower than nftables (no NAT,
no sets/maps, one src IP/prefix + one dst port, input/output only), so a faithful
`nft` is impossible and a lossy one risks silently under-applying a user's policy.

**Decision.** Keep `nft`/`iptables` as an **explicit parser/pretty-printer
only.** Correct the module doc to state it does not persist or apply; print a
clear "not applied — use `fw` to configure the kernel firewall" notice on
mutating commands; treat `fw` as the one true firewall front-end. Full/minimal
wiring (A/B) is deferred until a concrete need appears.

**Alternatives.** (A) Full-ish wiring, (B) minimal wiring — both deferred:
large, lossy, misleading against the narrow kernel model.

**Where it lives.** `userspace/nft/src/main.rs` (`run_nft`/`run_iptables` module
doc + mutating-command notice). Related: known-issues TD18 residual.

---

## 63. Move the TCP/IP stack to userspace — migrate the *service* first, keep a thin kernel NIC shim (Path B), not full userspace drivers yet (Path A)

**Date:** 2026-07-14
**Decided by:** Claude (operator-approved scope). The operator selected the
"move the TCP/IP stack to a userspace service" initiative from the roadmap fork;
the specific migration *strategy* (Path B vs Path A) is Claude's call and is
recorded here as reviewable/reversible.

**Context.** `design.txt` is explicit: "don't put networking in the kernel." The
whole protocol stack (`kernel/src/net/*.rs`, ~50 files: Ethernet/ARP/IPv4/IPv6/
ICMP(v6)/UDP/TCP/DHCP(v6)/DNS/… plus app-protocol helpers) currently runs
kernel-resident, polling the NIC driver directly. `kernel/src/net/mod.rs` names
this a prototype "to be migrated to userspace once the driver framework supports
device access from user processes." The NIC boundary is already clean: drivers
(`virtio/net.rs`, `e1000.rs`) expose `send(&[u8])` / `recv() -> Option<Vec<u8>>`,
wrapped by `net::send_frame_ns` / `recv_frame`.

**The fork.**
- **Path A — full userspace driver.** Move the NIC *driver itself* to userspace,
  granting MMIO/DMA/IRQ via capabilities + IOMMU sandboxing. Purest microkernel
  end-state and the design's ultimate goal.
- **Path B — userspace *service*, thin kernel NIC shim.** Keep a minimal kernel
  NIC driver exposing only capability-gated raw-frame TX/RX + interface query;
  move the *entire protocol stack* into a userspace `netstack` daemon; socket
  syscalls become IPC to that daemon.

**Decision: Path B first.** Rationale:
- The roadmap item is literally "Move to userspace **service**," and `design.txt`
  treats IOMMU-sandboxed userspace *drivers* as a separate, *optional* speed
  feature ("for when the 5–15% speedup matters"), gated on IOMMU being present/
  enabled. Driver-to-userspace is its own later roadmap track, not a prerequisite
  for de-kernelizing the protocol stack.
- Almost all of `kernel/src/net/` is privilege-free protocol logic (parsers,
  state machines) that can move into a userspace crate largely as-is — the big
  win (kernel attack surface, "restart the network service") is captured by
  Path B alone.
- Path B is incrementally testable and reversible: the kernel-resident stack
  keeps working throughout; the daemon is built alongside and cut over only when
  it reaches parity. Path A requires userspace MMIO/DMA/IRQ/IOMMU plumbing before
  a single packet flows — far higher risk for the same protocol-stack payoff.

**Performance note (net is in the perf-critical table).** Path B adds one IPC
hop app↔netstack and one raw-frame syscall netstack↔kernel per batch. Mitigate
with io_uring-style batched raw-frame TX/RX (submit/complete many frames per
syscall) and shared-memory ring buffers for the socket data path, matching the
design's batching guidance. Measure against the current in-kernel numbers before
cutover; do not regress the perf targets.

**Phased plan** (tracked in `net-userspace-migration.md`):
1. Kernel raw-frame boundary: capability-gated `sys_net_raw_*` (open/tx/rx) +
   interface query. Additive; existing stack untouched.
2. `netstack` userspace crate skeleton: open raw iface, poll loop, prove ARP +
   ICMP echo end-to-end.
3. Port protocol layers into the daemon (IPv4/IPv6, UDP, TCP, DHCP, DNS, …),
   reusing the kernel modules' logic.
4. Socket syscalls → IPC to `netstack` (shared-mem data path).
5. Cut over; delete the kernel-resident stack; keep only the thin NIC shim.

**Alternatives.** Path A now (rejected: higher risk, no extra protocol-stack
payoff, driver-userspace is a separate optional track). Leave in kernel
(rejected: violates the design's core microkernel tenet).

**Where it lives.** New kernel raw-frame syscalls (`kernel/src/syscall/`,
`kernel/src/net/mod.rs` shim), new `netstack/` userspace crate, socket-syscall
IPC bridge. Migration plan + status in `net-userspace-migration.md`.

## 64. netstack Phase 4 — Service-Registry channel transport, and bounded-self-test validation because the raw-NIC claim is exclusive

**Date:** 2026-07-14
**Decided by:** Claude (operator-approved scope). Sub-implementation call under
§63's Path B; reviewable/reversible.

**Context.** Phase 4 turns socket syscalls (`SYS_DNS_RESOLVE`, `SYS_TCP_*`,
`SYS_UDP_*`) into IPC to the `netstack` daemon. Two questions had to be settled
before writing code: (a) *what transport* carries the app↔daemon request/reply,
and (b) *how to validate it* given the rest of the system.

**Transport decision: the kernel Service Registry (`kernel/src/ipc/service.rs`).**
It already provides exactly the primitives Phase 4 needs: `register(name)` for the
daemon to publish `net.stack`, `connect(name)` for the kernel-side syscall handler
to obtain a client channel endpoint, and `accept`/`try_accept` for the daemon to
take the server endpoint. Requests/replies ride `channel::Message` byte payloads
over that channel (`channel::send`/`recv`). It is kernel-mediated, name-based, and
namespace-isolated — no new IPC mechanism required. Rejected alternatives: a bespoke
syscall pair dedicated to netstack (redundant with the registry, more ABI surface);
raw shared memory only (needed later for the *bulk data* path, but overkill for the
one-shot request/reply control path — start with channel messages, add a shared-mem
data ring for TCP/UDP streaming as a later increment).

**Validation constraint (the important finding): the raw-NIC claim is exclusive.**
The Phase-1 `sys_net_raw_open` gives its owner an *exclusive* claim, and `net::poll()`
deliberately skips draining the physical NIC while a raw owner holds it (so the
kernel stack and a raw owner never both consume frames). Consequence: a *persistent*
netstack daemon that holds the NIC open forever would starve the kernel-resident
stack's RX. Since the kernel stack must stay live until Phase 5 cutover, Phase 4
**cannot** run an always-on daemon that owns the NIC and simultaneously forward
syscalls to it while the kernel stack still serves other traffic — they would fight
over the NIC.

**Decision: validate Phase 4 with bounded self-tests (Phase-2 style), defer
persistent cutover to Phase 5.** Each Phase 4 increment (starting with
`SYS_DNS_RESOLVE`) is proven by a self-test that: spins up the daemon, has it claim
the NIC, register its service, service exactly one request/reply, then release the
NIC — a bounded window, not a permanent takeover. The syscall-forwarding wiring is
built and exercised, but the kernel keeps its own stack as the live path until
Phase 5 flips every socket syscall to the daemon at once and deletes the kernel
stack (at which point the exclusive claim becomes correct, not a conflict).

**Where it lives.** `services/netstack/src/main.rs` (service registration +
request loop), kernel socket-syscall handlers (`kernel/src/syscall/handlers.rs`,
`connect`/`send`/`recv` to `net.stack`), a bounded self-test in the boot self-test
path. Schema + status tracked in `net-userspace-migration.md` Phase 4.

## 65. netstack bulk data path — io_uring-style SPSC shared-memory ring (fixed 32B SQE / 16B CQE, cache-line-separated indices, separate data area)

**Date:** 2026-07-14
**Decided by:** Claude (autonomous). Sub-implementation call under §63/§64;
reviewable/reversible — the ABI is versioned (`RING_VERSION`) so it can be
revised before anything depends on it long-term.

**Context.** §64 settled the *control* path (one-shot request/reply over a
Service-Registry channel) but explicitly deferred the *bulk* path: streaming
`send`/`recv` on a TCP/UDP socket cannot ride per-call `channel::Message` copies
without blowing the < 2 µs IPC round-trip and the throughput targets (a per-byte
kernel↔daemon copy per stream chunk is exactly the anti-pattern CLAUDE.md's perf
section calls out — "IPC channels should move pages, not copy contents"). Phase 4
needs a zero-copy transport for socket data before the daemon can host persistent
per-connection state machines.

**Decision: an io_uring-style pair of SPSC rings in one shared-memory region.**
Modeled directly on Linux io_uring (the reference the perf table cites for
submission cost). One `SYS_SHM_CREATE` region holds: a header, a **submission
queue** (SQ — kernel produces, daemon consumes: connect/send/recv/close), a
**completion queue** (CQ — daemon produces, kernel consumes: result + echoed
`user_data`), and a **separate bulk data area**. SQE/CQE carry only a
`(data_off, data_len)` window into the data area, so message bytes are never
copied across the channel — the kernel writes send-data straight into shared
memory and the daemon reads it in place (and vice-versa for recv).

**Sub-choices and why:**
- *Fixed 32-byte SQE / 16-byte CQE* (not variable-length). Fixed stride makes
  `slot = index & (entries-1)` a single mask, keeps entries cache-friendly, and
  avoids a length-parsing step on the hot path. 32B holds op + conn_id +
  (data_off,data_len) + user_data + an 8-byte `aux` (endpoint pack for connect);
  16B holds user_data + result + flags. Chosen over io_uring's larger 64B SQE
  because we don't carry its full opcode surface.
- *Free-running u32 indices, power-of-two entry counts.* Wrapping monotonic
  indices give unambiguous empty (`head==tail`) / full (`tail-head==entries`)
  without a wasted slot, and the mask replaces a modulo.
- *Four indices on separate cache lines* (SQ head/tail, CQ head/tail — header is
  5 cache lines, `HEADER_LEN=320`). Producer and consumer touch different lines,
  so no false sharing on the hottest words. Straight from the per-CPU /
  cache-line-alignment guidance in CLAUDE.md's perf patterns.
- *Pure, mapping-agnostic module.* `netipc/src/ring.rs` defines only the byte
  layout, entry (de)serialization, and index arithmetic — no atomics, no
  mapping. That keeps the shared crate `no_std`, dependency-free, and
  `#![forbid(unsafe_code)]`. Both sides link the one module, so the ring ABI
  can't drift — same rationale as §64's shared schema.
- *Shared atomic driver in a separate `netring` crate (not duplicated at each
  integration site).* The acquire/release atomic index accesses and the
  `push`/`pop`/`write_data`/`read_data` bounds logic are the one genuinely
  subtle, `unsafe`, easy-to-get-wrong part of the ring. Writing them **once** in
  a `no_std` `netring` crate (which depends on `netipc` for the ABI) — rather
  than hand-rolling the Acquire/Release dance separately in the kernel forwarder
  and again in the daemon — means the memory-ordering correctness is written,
  reviewed, and *host-tested* exactly once, then linked verbatim into both
  sides. This directly answers the CLAUDE.md unsafe-policy rule ("isolate
  `unsafe`, wrap it in a safe abstraction as close to the operation as
  possible"): after `Ring::init`/`Ring::attach`, every hot-path method is safe
  and bounds-checked against the length-validated geometry. `netipc` stays
  `#![forbid(unsafe_code)]`; `netring` is the single audited home for the ring's
  `unsafe`. The alternative — open-coding the atomics at both integration sites —
  was rejected because two independent copies of Acquire/Release logic is exactly
  where a subtle ordering bug (visible only under concurrent cross-address-space
  contention, i.e. nearly untestable in situ) would hide.

**Deferred sub-choice (flagged, not yet decided): recv/notification blocking.**
When a `recv` SQE has no data yet, *how* the waiter is parked and woken — futex
on a shared word, an eventfd-style handle, or a channel-signal — is left open;
the ring itself is notification-neutral (a consumer can poll it). This will be
settled when the kernel/daemon integration lands, and is a candidate for an
`open-questions.md` entry if it turns out to have a real tradeoff (polling vs.
futex latency vs. CPU burn). Logged here so the ring ABI isn't mistaken for
having answered it.

**Rejected alternatives:** (a) a single bidirectional ring — conflates the two
producers and needs locking; SPSC pairs are lock-free. (b) Copying stream data
inside `channel::Message` — the thing this whole decision exists to avoid.
(c) Variable-length SQEs — parsing cost + harder slot math for no real gain at
our opcode count.

**Where it lives.** `netipc/src/ring.rs` (ABI + 10 host tests) and the
`netring` crate (`netring/src/lib.rs` — the atomic driver + 9 host tests,
including an end-to-end kernel-init → daemon-attach echo through the ring). Both
are workspace-`exclude`d (built for `x86_64-unknown-none` as deps of the kernel
and daemon; host-testable with an explicit `--target`). Wiring the ring into the
kernel forwarder and the daemon (SHM region + `Ring::init`/`Ring::attach` +
a ring-echo control op) comes in following Phase 4 increments; tracked in
`net-userspace-migration.md`. Streaming-only limitations still noted under
`known-issues.md` `D-NETSTACK-TCP-MINIMAL` until the ring is wired end-to-end.

## 66. netstack Phase 5 cutover — phased deletion (Q22a→C) + staged cutover behind a default-off switch (Q22b→ii)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended both chosen options; operator
approved both).

**Context.** §63 (Path B) settled that the TCP/IP stack moves to the userspace
`netstack` daemon behind the thin capability-gated kernel NIC shim. Phases 1–4
(NIC boundary, daemon skeleton, shared `netproto` parsers, IPC + zero-copy ring
socket ops) plus the Phase-5 daemon prerequisites (persistent ring session §5.2,
shared RX demux §5.3 — both boot-validated) are done. Phase 5's final cutover —
forwarding the POSIX/Linux socket syscalls to the daemon and **deleting the
~60 K-line `kernel/src/net/`** — had two forks with no obviously-correct answer
and irreversible cost, raised as Q22 in `open-questions.md`:

- **Q22a (deletion scope):** `kernel/src/net/` is ~48 modules. Only the L2–L4
  core (`ethernet, arp, ipv4/ipv6, icmp/icmpv6, tcp, udp, dns, dhcp, frag,
  interface, ndisc`) is what the daemon replaces; the rest are app-level protocol
  servers/clients (ssh, httpd, ftp, smtp, telnet, tftp, ntp, dhcpd, syslog, …)
  that happen to live in-kernel and depend on the in-kernel `tcp`/`udp` APIs.
- **Q22b (cutover mechanism):** given §64's *exclusive* NIC claim, a persistent
  daemon and the still-live kernel stack cannot both reach the uplink — there is
  no true concurrent dual-stack.

**Decision.**
- **Q22a → Option C (phased deletion).** Delete the L2–L4 core first (once the
  daemon proves parity and the forwarders are wired); re-home each app-protocol
  module to userspace in its own dedicated follow-up task, deleting it from
  `kernel/src/net/` as it lands. No single big-bang removal of app features.
- **Q22b → (ii) staged cutover.** Land a persistent daemon + a socket-forwarding
  path behind a **boot/config switch that defaults OFF**, keeping the in-kernel
  stack as the compiled fallback and the NIC owner. Prove parity in QEMU with the
  switch ON, **flip the default to the daemon**, then (only then) delete the
  L2–L4 core. The switch selects *which stack owns the NIC at boot* — not a
  concurrent dual-stack (which §64 forbids).

**Rationale.** Every step stays buildable and boot-testable, and no step is a
giant irreversible leap. The staged switch means the daemon path can be exercised
end-to-end in QEMU while the known-good kernel stack remains one boot-flag away,
so a regression is a flag flip, not a revert of a 60 K-line deletion. Phased
deletion avoids a large temporary feature regression (ssh/http/ftp/… servers
vanishing at once) and gives each app protocol a real userspace re-home rather
than a silent drop. Cost: more increments and a longer calendar span, plus a
transitional period where the kernel still hosts app protocols over a
daemon-provided socket API (added coupling) — accepted as the price of always
being able to build, boot, and bisect.

**Alternatives considered.**
- *Q22a Option A (L2–L4 only, keep app modules in-kernel as-is):* rejected —
  those modules call the in-kernel `tcp`/`udp` APIs being deleted, so they can't
  actually stay unchanged; not cleanly separable without rewiring them onto the
  daemon socket API anyway.
- *Q22a Option B (delete everything at once):* rejected — large, irreversible,
  temporary regression of every app protocol.
- *Q22b (i) big-bang (flip persistence + forwarding + deletion in one commit):*
  rejected — a huge, effectively untestable step; a regression would require
  reverting the deletion.

**Where it lives.**
- Daemon: `services/netstack/src/main.rs` (persistent serve loop, `RingSession`,
  `RingConns`, `TcpConn`, NIC-claim lifecycle in `main`).
- Kernel NIC shim: `kernel/src/net/raw.rs`, `SYS_NET_RAW_*`.
- Socket forwarders: `kernel/src/syscall/linux.rs`
  (`sys_socket`/`sys_connect`/`sys_sendto`/`sys_recvfrom`/`sys_bind`/`sys_listen`/
  `sys_accept`/…), which today dispatch into `kernel/src/net/{tcp,udp,…}` and
  will gain a switch-gated branch that forwards to `net.stack` instead.
- Persistent-spawn path: how init/the service manager launches the daemon at boot
  (today it is spawned only by the bounded kernel self-test in
  `kernel/src/proc/spawn.rs`).
- Deletion target (final step, phased): `kernel/src/net/` L2–L4 core, then each
  app module.

**How to reverse.** While the switch defaults OFF, reverting is a no-op (the
kernel stack is still the default). After the default flips but before deletion,
reverting is flipping the default back. After deletion, the L2–L4 core would have
to be restored from git history — which is precisely why deletion is the *last*
step, gated on proven QEMU parity.

**Tracking.** Increment plan in `net-userspace-migration.md`; roadmap line under
Phase 2 "Move to userspace — Path B".

## 67. ALSA `snd_pcm_status` ABI target — time64 (64-bit `time_t`), not the legacy 32-bit-timespec variant

**Date:** 2026-07-15
**Decided by:** Claude (autonomous). Sub-implementation call under the ALSA
compatibility-shim roadmap item; low-risk and reversible (a translator-layer
struct/ioctl-number choice, no persistent state), so resolved directly rather
than raised to the operator.

**Context.** The ALSA PCM `STATUS`/`STATUS_EXT` ioctls return `struct
snd_pcm_status`, which — unlike `SYNC_PTR`, whose pages sit in 64-byte unions —
embeds bare `struct timespec`s directly. Its `sizeof` therefore depends on the
`time_t` width, and because the ioctl request number is `_IOR/_IOWR('A', nr,
sizeof(struct))`, the *request number itself* differs between the legacy
32-bit-`time_t` layout and the modern 64-bit (`time64`) layout. The upstream
kernel maintains two distinct structs/numbers for exactly this reason (proven
by the mainline `reserved[]` size expression going negative under a 16-byte
timespec — it cannot be a single struct). So implementing STATUS is not "pin
the timespec layout" but "decide which alsa-lib variant our userspace targets."
This ambiguity is why STATUS was deferred (known-issues TD10) while SYNC_PTR
(ABI-independent) shipped.

**Decision.** Target **time64**: 64-bit `time_t`, 16-byte `struct timespec`,
`sizeof(snd_pcm_status) == 152`, giving `STATUS = 0x8098_4120` and `STATUS_EXT
= 0xC098_4124`. Implemented in `kernel/src/audio_alsa.rs` (`SndPcmStatus` +
size/ioctl-encoding self-test) and `kernel/src/syscall/linux.rs`
(`alsa_pcm_ioctl_status`).

**Alternatives considered.**
- *Legacy 32-bit-timespec variant.* Pro: matches ancient 32-bit alsa-lib
  builds. Con: Y2038-unsafe; not what any modern 64-bit distro's alsa-lib is
  compiled against; a dead-end for a brand-new OS.
- *Implement both request numbers.* Pro: maximal compatibility. Con: doubles
  the surface for a convenience overlay; the 32-bit path is pure legacy we have
  no reason to carry. Can be added later if a real 32-bit client ever appears —
  the numbers are distinct, so adding a second arm is non-breaking.

**Reasoning.** SlateOS is a new x86_64-only OS with a 64-bit `time_t`
throughout; every other timespec-bearing syscall/ABI here is already 64-bit.
A modern 64-bit ALSA-lib (what an unmodified Linux audio client links) uses the
time64 layout, so this is the variant that actually makes `snd_pcm_status(3)` /
`snd_pcm_delay(3)` work for the shim's stated goal (unmodified Linux audio
clients). There is no realistic scenario where a 32-bit-`time_t` alsa-lib runs
on this OS. The choice is effectively forced; recorded here only because the
*existence* of the fork was non-obvious and previously blocked the work.

**Tracking.** known-issues TD10 (RESOLVED); roadmap Phase 5 ALSA shim item.

## 68. UDP `SOCK_DGRAM` over the ring — convey the per-datagram source address as an in-band header prefix, not by widening the CQE

**Date:** 2026-07-15
**Decided by:** Claude (autonomous). Sub-implementation call under §63/§65
(the netstack userspace migration and its io_uring-style ring). Reversible: the
ring ABI is versioned (`RING_VERSION`) and no UDP client depends on it yet — the
daemon/kernel UDP layers are still to be built on top of this ABI.

**Context.** The stream socket path is done; the next daemon-backed socket
feature is connectionless UDP (`SOCK_DGRAM`): `bind` a local port, then `sendto`
arbitrary destinations and `recvfrom` arbitrary senders. Stream ops never need a
per-op peer address (the connection *is* the peer), but every UDP datagram
carries its own source (on recv) and destination (on send). Destination is easy
— it fits the existing 48-bit `[ip:4][port_be:2]` `Sqe::aux` endpoint packing
(same as `OP_CONNECT`). The hard part is the **recv** direction: the 16-byte
`Cqe` (echoed `user_data` + `i32` result + `u32` flags) has no room for a
source address, so the daemon needs another channel to report *who* a received
datagram came from.

**Decision: prepend a fixed 24-byte source-address header to the recv data
window.** `OP_UDP_RECV` has the daemon write, at the front of the SQE's data
window, a `UDP_ADDR_HDR_LEN` (24-byte) header — `[family:2][port_be:2][ip:16]
[reserved:4]` (`Sqe::pack_udp_addr`/`unpack_udp_addr`) — immediately followed by
the datagram payload. The CQE `result` reports the *payload* length only (the
header is not counted). New opcodes `OP_UDP_BIND`/`OP_UDP_SEND`/`OP_UDP_RECV`
(0x0C–0x0E) and sentinels `ERR_ADDR_IN_USE`/`ERR_MSG_SIZE`.

**Alternatives considered.**
- *Widen the CQE to 32 bytes with an address field.* Pro: semantically cleaner
  (the address rides the completion, not the data buffer); the payload window is
  "just payload". Con: the CQE layout is **shared by every opcode** and its
  serialization (`Cqe::to_bytes`/`from_bytes`) is on the hot path for the stream
  sockets too — widening it perturbs the whole ring for a UDP-only need, and 16
  bytes still can't hold a 16-byte IPv6 address + port + family anyway (would
  need 32B, doubling CQ memory for all ops). Larger blast radius, reworks proven
  stream code.
- *A side channel / second ring for addresses.* Pro: keeps both the CQE and the
  payload window pure. Con: a whole extra SPSC structure and index dance per
  datagram; far more moving parts than a fixed prefix; more to get wrong.
- *IPv4-only 8-byte prefix now, extend later for IPv6.* Pro: smallest header.
  Con: a second, incompatible header layout when IPv6 datagram sockets land — a
  gratuitous ABI fork. The 24-byte header already carries a full IPv6 address, so
  one layout serves both families forever.

**Reasoning.** The in-band prefix keeps the CQE and the entire stream-socket ring
path **byte-for-byte unchanged** (zero regression risk to the working TCP
sockets), needs no new ring structures, and is trivially forward-compatible with
IPv6 (the 16-byte `ip` slot holds a v4 address left-packed or a full v6 address,
selected by `family`). The only cost is that the UDP recv payload window is
offset by a fixed 24 bytes — a one-line arithmetic detail on both sides, and the
same in-band-metadata pattern Linux itself uses for ancillary data. The chosen
`result = payload-length` convention (header excluded) means callers size and
copy exactly the datagram bytes, matching `recvfrom` semantics.

**Tracking.** known-issues D-NETSOCK-SYNC (UDP `SOCK_DGRAM` listed as a remaining
gap); roadmap netstack Phase 5. This commit lands the ring ABI + helpers +
unit tests; the daemon UDP socket table, kernel `UdpConn` client, and the
`sys_socket(SOCK_DGRAM)`/`sendto`/`recvfrom` fd wiring build on it in follow-ups.

---

## 69. Next large initiatives (Q25) — order the remaining giant ports: **A(Oils+coreutils) → F(fastpy) → B(Mesa/GPU) → C(Chromium) → D(WINE) → E(filesystems)**

**Date:** 2026-07-18

**Decided by:** Operator (Claude recommended "A first, then F"; the operator
adopted that and fixed the full ordering of the remaining initiatives).

**Context.** With the self-hosting C toolchain (tcc on-target, glibc + `ld.so`
dynamic linking, ring-3 execution, the Path-Z self-test suite) and the POSIX
layer both comprehensive, the roadmap's entire remaining unchecked frontier is
"giant external ports." Picking the order among them has historically been the
operator's call (open-questions Q25).

**Decision.**
- **Do the interactive-shell userland first.** The item labeled "bash" in Q25
  option A is **not bash** — the shell we port is **Oils (OSH)**, the
  bash-compatible *superset* already on the roadmap ("Port Oils (bash-compatible,
  replaces bash for POSIX compatibility)", `roadmap-detailed.md` §2.7 Shells,
  ~line 861). OSH runs existing bash scripts (superset) and is the POSIX/bash
  compatibility shell; Nushell remains the default *interactive* shell. So Q25-A
  = **Oils + coreutils**, not a bash port.
- **Fixed order for the remaining giant initiatives** (so this need not be
  re-asked later):
  1. **A — Oils (OSH) + coreutils** (interactive shell userland).
  2. **F — fastpy build-system integration** (unblocks writing OS userspace
     tools in Python-via-fastpy: package manager, settings UI, file indexer,
     installer, etc.).
  3. **B — Mesa / GPU userspace** (3D; still gated by Q18 on a virgl test
     environment — see that item).
  4. **C — Chromium** (browser + "system web app"/Electron framework).
  5. **D — WINE** (Windows app compatibility).
  6. **E — Additional filesystems** (Btrfs / F2FS / NTFS).

**Rationale.** A is the smallest, highest-leverage next step and builds directly
on the just-proven tcc/glibc/`ld.so`/ring-3 path; a working shell + coreutils is
the natural foundation for everything else and is continuously shippable one tool
at a time. F then unlocks the Python userspace lane (a force-multiplier for the
many small system tools `CLAUDE.md` says to write in fastpy). B/C/D/E are larger
and either gated (B on Q18/virgl) or dependent on more maturity (C/D on
graphics+audio); E is self-contained and lowest immediate payoff, so it sorts
last.

**Alternatives considered.** Leading with B/C/D/E instead of A/F — rejected:
they are larger, some are gated, and none give the incremental
shell-plus-coreutils foundation that unblocks the most subsequent work. Doing F
before A — rejected: fastpy integration is valuable but the shell/coreutils
userland is the more universal unblocker and the smaller gap from what's proven.

**Where it lives.** `roadmap.md` (line ~1494 bash/Oils; line ~24 fastpy; lines
~5117–5119 filesystems; line ~5032 Chromium; line ~5114 WINE);
`roadmap-detailed.md` §2.7. The practical gates for A are the fork/exec WATCH
bugs in `known-issues.md` (B-FORKEXEC-BOOT-HANG, B-PTHREAD-TEARDOWN-PF).

**How to reverse.** Re-open Q25 and re-sequence; the ordering is guidance for
task-selection, not a code commitment, so reversing costs nothing but a new
decision.

---

## 70. Raw `spin::Mutex` holder-preemption (Q24) — **proactive kernel-wide audit/conversion (option B)**, not reactive-only

**Date:** 2026-07-18

**Decided by:** Operator (Claude recommended **A**, reactive, with **C** as an
escalation; the operator **overruled** and chose **B** — "Let's not have
technical debt and do it the right way").

**Context.** The kernel had (at decision time) four confirmed single-CPU
deadlocks on raw `spin::Mutex` locks across two sub-variants — *holder-preemption*
(heap, `container::TABLE`) and *interrupt-reentrancy* (`sysctl::REGISTRY`,
completion-timer→`SCHED`). A raw `spin::Mutex` neither disables preemption on
acquire (so a holder can be preempted mid-section and a second task spins forever
on one CPU) nor is IRQ-safe by construction. The preempt-aware
`crate::sync::Mutex` prevents the holder-preemption class, but ~476 kernel files
import raw `spin::` locks. Claude had been fixing each caught instance reactively.

**Decision.** Do the **proactive audit/conversion (option B)** rather than
continuing reactive-only. Eliminate the whole deadlock class deliberately instead
of waiting for the soak to surface each latent instance. This is explicitly a
"no technical debt, do it right" call by the operator.

**Rationale (operator).** Two deadlock sub-variants and four instances already
found means the latent-instance tail is real; leaving it to chance (reactive-A)
is accepting known technical debt. A deliberate audit removes the class and can
add lockdep/owner-tracking where it pays.

**Execution guidance (to keep B safe — it "can't be a blind sed").**
- **Not a mechanical `use spin::Mutex` → `crate::sync::Mutex` sweep.** Some locks
  are deliberately raw and must stay raw + manual preempt discipline (e.g. the
  global heap lock — lockdep can't allocate under it). Triage each lock.
- Prefer a **preempt-aware, non-lockdep spinlock** (the `PreemptSpinMutex` idea
  from option C: `preempt_disable/enable` around the raw spin, no registry) for
  hot **leaf** locks where lockdep would be pure overhead; reserve
  `crate::sync::Mutex` (full lockdep + owner tracking) for **contended, non-leaf**
  locks where ordering bugs are plausible and the registration cost is
  affordable.
- Keep IRQ-context acquirers on `try_lock`/`without_interrupts` (the
  interrupt-reentrancy surface — timer hard-IRQ, softirq→`SCHED`, `#PF` — was
  already audited clean; don't regress it).
- Do it **incrementally and validated** — convert in reviewable batches, keep
  `scripts/wedge-soak.sh` green between batches, and expect a flood of
  newly-surfaced lock-ordering reports from lockdep to triage as locks are
  registered.

**Alternatives considered.** **A (reactive)** — rejected by the operator as
leaving known latent debt. **C (middle path, convert only contended non-leaf
locks)** — folded into B as the *execution technique* (add `PreemptSpinMutex`,
choose per-lock) rather than the whole scope.

**Where it lives.** `kernel/src/sync.rs` (`Mutex`; add `PreemptSpinMutex`); every
`use spin::Mutex` site (~476 files); already-fixed anchors `kernel/src/mm/heap.rs`,
`kernel/src/container.rs`, `sysctl` (B-SYSCTL-IRQ-DEADLOCK),
completion-timer→SCHED (B-COMPLETION-TIMER-IRQ-DEADLOCK). Detector:
`scripts/wedge-soak.sh`. Track the audit as a roadmap task.

**How to reverse.** Stop the sweep and fall back to reactive-A; already-converted
locks stay converted (no harm). Reversing is cheap since each conversion is
independently sound.

**Execution status / triage outcome (2026-07-18).** The sweep converted, in
reviewable per-subsystem batches (each boot-tested green before commit):
- **`PreemptSpinMutex`** (preempt-disabling, no lockdep) for hot/cold *leaf*
  locks held briefly in process/thread context: most of `fs/` (procfs stat/config
  stores), `ipc/` leaves (channel, completion, epoll, eventfd, inotify, memfd,
  pipe, semaphore, service_limits, shm, signalfd, stream_socket, timerfd,
  alsa_pcm), `mm/` service locks (mempool, page_cache, rmap, vmalloc),
  `proc/{exception,thread_clone}`, `cap/file_tags`, and driver/service leaves
  (blkdev, cnetwork, drvmon, initproc, ksyms, logpersist, netns, pidns, reslimit,
  scfilter, sockact, svcstart, syshealth, termsession, userns, volume,
  drm/{card_fd,dumb_mmap,mod,hotplug}, power, devhotplug, devpower, udriver,
  vmguest, acpi/mod, bench, eventlog, kshell, syscall/linux).
- **`crate::sync::Mutex`** (full lockdep + owner tracking + preempt-disable) for
  contended non-leaf/nested locks: core-FS contended locks, `ipc/{futex,io_ring,
  namespace,service}`, all of `net/` (28 files, uniform), `cap/groups`
  (GROUPS→NEXT_ID nesting), `kevent`.
- **Deliberately kept RAW** (holder-preemption does not apply — the lock is only
  taken with interrupts already off, or in panic/scheduler-core context where a
  preempt-aware wrapper is wrong or circular): `kernel/src/sync.rs` itself (the
  backing store — never convert); the scheduler core (`sched/{mod,priority_rr,
  waitqueue,kchannel}` — circular with `preempt_disable`); IRQ/panic-context
  primitives `console`, `klog`, `tty` (keyboard IRQ input), `rng`
  (`add_interrupt_entropy` runs in ISR), `sysctl` (reached from an ISR),
  `serial` (`lock_irqsave`), `hrtimer`/`workqueue` (acquired under
  `without_interrupts`), `proc/{itimer,signal}` (all sites under
  `without_interrupts`), and the hardware device drivers whose ISRs take their
  locks (`e1000`, `hda`, `xhci`, `virtio/{blk,net}`, `iommu_remap`). These
  acquire on `try_lock`/`without_interrupts` or run with IRQs disabled, so a
  timer preemption of the holder cannot occur.

One pre-existing **flaky self-test** surfaced (not a conversion bug): the
container port-forward Test-20 (`container.rs`) spawned an instantly-exiting
init and asserted the host-port NAT forwards were still live, but a container's
forwards are flushed by `notify_init_exit` the moment its init exits — a race the
new preemption timing made observable. Fixed by snapshotting the forwards inside
a `preempt_disable`/`enable` window straddling `run()` (the single-CPU boot test
then cannot schedule the init to flush in between).

---

## 71. Daemon-backed AF_INET **server** sockets (Q23) — **shared refcounted session (option A)**, and a standing "don't gold-plate interim netstack work" guideline

**Date:** 2026-07-18

**Decided by:** Operator (Claude recommended **A**; operator chose **A** and
added a guideline about interim/stop-gap work — see below).

**Context.** In the userspace netstack daemon, a session == one SHM ring; `OP_ACCEPT`
installs the newly-established connection into the *listener's own* session on the
*same* ring, so a listening socket and all its accepted connections physically
share one ring. Linux instead gives every accepted fd a fully independent socket.
This fork gates the final AF_INET/AF_INET6 server socket-fd wiring
(`sys_bind`/`sys_listen`/`sys_accept4`), which in turn is the last gate on
flipping `net.userspace` on by default.

**Decision.** **Option A — shared, refcounted session, no daemon-ABI change.**
The listening `SocketInner` owns the session; each accepted socket is a new fd
holding an `Arc` on the same session with its own conn_id. Per-connection `close`
sends `OP_CLOSE` for that conn_id; the session's `OP_STOP` fires only when the
last reference (listener or any accepted socket) drops — giving Linux-correct
*lifetime* semantics (closing the listener no longer kills already-accepted
connections). The known limitation — all connections under one listener funnel
through one ring/lock, so a *blocking* op on one accepted conn can stall others
until its deadline — is accepted as temporary (a non-issue for the
`accept`+`poll`+non-blocking-I/O server pattern).

**Rationale.** The whole per-op synchronous socket path is explicitly a stepping
stone to the async, always-on socket server, which will replace the ring-per-op
model wholesale. Paying for option B's daemon-ABI complexity (accept-into-a-fresh-
ring, migrating `TcpConn` between sessions) now, only to rework it at the async
cutover, is poor value. A fixes the correctness-critical *lifetime* semantics with
zero protocol change.

**Operator guideline recorded with this decision (applies beyond Q23).** The
operator questioned doing *any* stop-gap netstack work that the async migration
will replace, and picked A specifically because it is the **minimal** interim
step. Standing guidance going forward: **do not gold-plate interim/throwaway
netstack infrastructure.** For the server-socket path, that means A only — do not
invest in per-connection ring independence (option B) or other elaboration before
the async socket server; if genuine per-connection concurrency is ever needed
before that cutover, revisit. (Note: the *client* socket path already built —
connect/recv/send/poll, IPv6 — is interim-but-*used* real functionality, not
throwaway; the async migration replaces the ring-per-op transport mechanism, not
the syscall-level behavior. The part most at risk of rework, and therefore kept
minimal, is exactly this server-socket layer.)

**Alternatives considered.** **B (accept-into-a-fresh-ring, daemon-ABI change)** —
true per-connection independence/concurrency, but a costlier-to-reverse protocol
commitment that the async cutover would largely redo; rejected as poor value for
an interim layer. Deferring server sockets entirely until the async migration —
considered (the operator floated it) but A is cheap enough and unblocks the
`net.userspace` default-flip for server programs now.

**Where it lives.** `kernel/src/net/socket.rs` (`SockState`, `SocketInner`,
`SOCKET_TABLE`; a shared `Arc<Mutex<Session>>`), `kernel/src/net/netstack_client.rs`
(a `Session` abstraction hosting multiple conn_ids), `kernel/src/syscall/linux.rs`
(`sys_bind`/`sys_listen`/`sys_accept4` routing). Tracking: known-issues
D-NETSOCK-SYNC; `net-userspace-migration.md`; the 5.7 default-flip.

**How to reverse.** Switch to B by extending the accept ABI (SQE carries a ring
handle; daemon `OP_RING_TCP`-attaches it and migrates connection state between
session tables) — or skip straight to the async socket server, which supersedes
the whole question.

## 72. Oils (OSH) port strategy (Q25-A) — **Rust reimplementation of the OSH language in-tree**, not a C++ `oils-for-unix` cross-compile

> ## ⛔ SUPERSEDED IN PART — 2026-08-14, by **§305**. Read §305 before doing any osh parity work.
>
> **Do not read this entry's rationale as current, and do not treat its "How to
> reverse" clause as live — it already fired, on 2026-07-22, and went unchecked
> for 25 days.** The operator settled the resulting question (Q41) on
> 2026-08-14 with the **hybrid**: osh remains the shell, the cross-compiled GNU
> bash ships beside it as the escape hatch and future on-device differential
> oracle, and **osh's bash-fidelity scope is frozen** behind a written stopping
> criterion. §305 carries that criterion and is binding on all `TD-OILS-*` work
> and on `userspace/oils/tests/corpus/`.
>
> What survives here: the *choice* of a Rust reimplementation, which stands.
> What does not: the "no C/C++ → `x86_64-slateos` cross-toolchain" premise
> (false since 2026-07-22 — bash now boots and runs on SlateOS) and the
> open-ended byte-for-byte parity goal that grew out of it.

**Decided by:** Claude (operator-approved scope) — the operator committed to "port
Oils (OSH), a bash-*superset* shell (NOT bash itself)" as the first large
initiative (§69, Q25→A). *How* to port it (faithful C++ cross-compile vs. Rust
reimplementation) is the sub-decision recorded here. Flagged to the operator as
open-question **Q26** because it is large and costly-to-reverse; proceeding on the
prerequisite-forced default while the operator is away.

**Decision.** Build `userspace/oils` as a **real Rust reimplementation** of the
OSH language (a bash/POSIX superset shell that actually forks/execs external
programs on SlateOS), matching the pattern already used for **coreutils** (85
real Rust tools) and the existing 1194-line `userspace/coreutils/src/bin/sh.rs`
minimal POSIX shell. **Not** a cross-compile of upstream Oils' C++
(`oils-for-unix`) tarball.

**Why (the decisive prerequisite fact).** There is **no C/C++ → `x86_64-slateos`
cross-toolchain in this repo** — verified: no crate/build.rs/script references a
C++ cross-compile to slateos, and every "port" to date is either a Rust
reimplementation (coreutils) or a Rust personality binary (the in-tree
`userspace/nushell` is a *stub* that simulates output; the real `nu.exe` was only
verified building against the **Windows host** target, never slateos).
Cross-compiling `oils-for-unix` would first require standing up an entire C++
cross-toolchain **and** a slateos libc/CRT sufficient for Oils' POSIX use — a
separate, massive, unlisted prerequisite initiative. A Rust reimplementation is
the only path that yields a **running** shell on the OS now, and it is the honest
match to the operator's intent (Q24 was spent specifically de-risking the
fork/exec teardown deadlock so this shell can fork/exec for real — a stub would
not exercise that at all).

**Alternatives considered.**
- **C++ `oils-for-unix` cross-compile (faithful port).** Pro: bit-for-bit OSH
  semantics, no reimplementation risk. Con: blocked on a non-existent C++/slateos
  toolchain + libc — not buildable today; would deliver nothing runnable for a
  long time. Rejected as prerequisite-blocked.
- **Extend the existing coreutils `sh.rs` in place.** Pro: least new code. Con:
  that binary is deliberately a *minimal POSIX sh*; growing it to a bash-superset
  OSH would bloat the coreutils crate and blur the "one crate = one deliverable"
  layout. A dedicated `userspace/oils` crate keeps the OSH shell reviewable and
  independently buildable/testable, and lets `sh.rs` stay a small POSIX baseline.
- **Rust stub personality (like the checked-in nushell).** Rejected — a shell
  that only prints simulated output is not a "port," does not run programs, and
  wastes the Q24 fork/exec de-risking.

**How to reverse.** If a C++/slateos toolchain is later built (e.g. as part of the
Mesa/Chromium/WINE initiatives, which need C/C++ anyway), the faithful
`oils-for-unix` cross-compile can replace `userspace/oils` — the crate is an
isolated userspace binary with no other code depending on its internals, so the
swap is local. Until then the Rust OSH shell is the deliverable.

**Where it lives.** `userspace/oils/` (new crate; auto-registered via the
`userspace/*` workspace glob). Roadmap: §2.7 "Port Oils (OSH)" (roadmap.md:1494).
Tracking: open-questions.md Q26.

> **⚠ Audit note — 2026-08-12: the prerequisite fact above is STALE.** The
> "decisive prerequisite fact" — no C/C++ → `x86_64-slateos` cross-toolchain —
> was true on 2026-07-18 when this was written, and **stopped being true on
> 2026-07-21/22**, when the `x86_64-slateos` C cross-target and `zig cc` landed
> (fastpy initiative F) together with `toolchain/sysroot/lib/libc.a`. The "How to
> reverse" clause above therefore fired within four days and was never
> re-examined; ~1,100 of the 1,181 `userspace/oils` commits postdate it. Note
> that bash is **C**, not the C++ this decision was actually arguing against, so
> it needs strictly less. This does *not* retroactively invalidate the decision
> (it was correctly reasoned on the facts of the day), and one real blocker
> remains — `posix/src/signal.rs:572` has no kernel suspend mechanism, so bash's
> job control cannot work yet — but the rationale must no longer be read as
> current. Raised by the operator; now **open-questions.md Q41**.
>
> **Follow-up, same day — measured, not argued.** The operator authorised a
> spike (`scripts/bash-spike/`). **GNU bash 5.2 now boots and runs on SlateOS**:
> cross-compiled with `zig cc`, linked statically against
> `toolchain/sysroot/lib/libc.a` with **zero undefined symbols and no shims**,
> and exercised by `self_test_bash_on_slateos_libc` in
> `kernel/src/proc/spawn.rs` on a script using arrays, `${v,,}`, `$(( ** ))`
> and brace expansion — constructs dash lacks, so no `/bin/sh` fallback can
> explain the result. Closing the gap took exactly three small additions to
> `posix/src`: `killpg`, `eaccess`/`euidaccess`, `__fpurge`. The prerequisite
> objection is therefore not merely stale but comprehensively so:
> **feasibility is settled and is no longer an input to Q41**, which is now
> purely a scope/ownership question — keep osh, switch to bash, or keep both
> and use bash as a differential oracle running on SlateOS itself.
>
> **Answered 2026-08-14 — the third option, and osh's scope is now frozen.
> See §305.**

## 73. YSH port strategy — **defer YSH; obtain it by cross-compiling genuine Oils once a C++/slateos toolchain exists, NOT by hand-porting or auto-translating**

**Date:** 2026-07-19
**Decided by:** Operator (Claude recommended this option; operator agreed).

**Context.** §72 covers **OSH** (the bash-compatible half of Oils), reimplemented
in Rust as `userspace/oils` and now very mature (~26k lines, 480 passing tests,
byte-for-byte vs. bash across extensive probing). Oils is **two languages in one
binary**: OSH *and* **YSH** (formerly "Oil") — the genuinely new, typed shell
language (real `Int/Float/Str/List/Dict/Obj` values, an expression sublanguage,
`var/const/setvar`, `proc`/`func`, closures, J8/JSON, eggex, structured error
handling). YSH is **not built at all**. The operator asked whether the full YSH
language should also be ported, and by what mechanism.

**Key technical facts that drove the decision.** Oils' source of truth is a
statically-typed subset of **Python** ("mycpp"); the shipping `oils-for-unix`
binary is **machine-generated C++** (from that Python) riding Oils' own
garbage-collected runtime. There is **no realistic automated path** to turn
either form into good Rust: Python→Rust transpilers (`py2many`, etc.) are
toy-grade; `c2rust` is C-only (negligible C++ support) and, even if it worked,
would emit an unmaintainable unsafe blob modeling Oils' GC. Rust *refactoring*
libraries (`syn`/`quote`, rust-analyzer-as-lib, `comby`, `cargo fix`) only
rewrite Rust we already have — they do not port another language *in*.

**Decision.** Do **not** hand-port or auto-translate YSH into Rust. Instead:
1. **Now** — keep hardening the Rust **OSH** shell (§72); it is the high-value
   bash-superset and nearly complete.
2. **Later** — once a **C++/slateos cross-toolchain** exists (a prerequisite the
   Mesa/GPU, Chromium, and WINE initiatives all need anyway) plus enough
   SlateOS POSIX/libc surface, obtain YSH by **cross-compiling genuine upstream
   Oils C++** — which yields faithful **OSH *and* YSH at once**, no
   reimplementation. Track YSH as **blocked-on-C++-toolchain**, not
   blocked-on-effort.

**Deferred sub-decision (revisit when the toolchain lands).** Once real Oils can
cross-compile, choose between: (a) keep the lightweight Rust OSH as the default
shell and ship genuine Oils as an *installable package* for YSH users; or
(b) retire the Rust OSH in favor of upstream Oils entirely. Not settled now.

**Alternatives considered.**
- **Hand-reimplement YSH in Rust** (mirroring the OSH approach). Pro: runs on
  SlateOS today with no new toolchain; consistent with §72. Con: YSH is a whole
  second language (typed value system + expression parser + `proc`/`func` +
  eggex + J8 + YSH builtins) — on the order of the entire OSH effort again — and
  it would perpetually chase upstream YSH, which is still evolving. Rejected as
  the *primary* plan: once a C++ toolchain exists anyway, a faithful cross-compile
  gets both languages for far less work and with exact semantics. (Left available
  as a fallback if the C++ toolchain never materializes and YSH becomes urgent.)
- **Automated source translation** (Python→Rust or generated-C++→Rust). Rejected:
  no production-grade tooling exists; the GC-runtime-generated C++ is
  especially hostile to `c2rust`. This corrects an earlier assumption that the
  C++ toolchain would unlock an *automated* YSH port — it unlocks a faithful
  *cross-compile*, not a translation.

**How to reverse.** Symmetric with §72: the strategy is a sequencing/prerequisite
call, not a code commitment. If YSH becomes urgent before the C++ toolchain
lands, fall back to a Rust reimplementation; the `userspace/oils` crate is
isolated so either a YSH-in-Rust module or a swap to genuine Oils is a local
change.

## 74. osh error diagnostics — adopt bash's `<name>: line N:` prefix, but keep osh's own `$0` name (not bash's `environment` pseudo-name) and a uniform syntax-error form

**Date:** 2026-07-19
**Decided by:** Operator authorized the overall feature (the operator directed
"Continue porting a bash-compatible shell from oils" / "port all of it" and
recorded the pro-`line N:` argument in `todo2.txt`, lifting the prior gate on
TD-OILS-ERRLINE); **Claude (autonomous)** made the implementation sub-calls
below. See known-issues.md TD-OILS-ERRLINE for the full shipped writeup.

**Context.** bash prefixes non-interactive runtime diagnostics with
`<$0>: line <N>: `. osh previously emitted only `osh: <msg>` (no line, and it
hard-coded `osh:` even for scripts). Adopting the prefix is a real
debugging-usability win. Byte-matching bash is impossible regardless, because
osh's `$0` is `osh`, not `bash` — so this is about format fidelity for SlateOS's
own shell, not literal equality.

**Sub-decisions (Claude autonomous tradeoffs).**
- **Function-scope source name.** Inside a `-c`-defined function, bash reports the
  magic source name `environment` (`environment: line N:`). osh keeps its own
  `$0`-based name (`osh: line N:`) instead. *Pro:* consistent, meaningful name;
  osh's name differs from bash anyway so mirroring the magic string buys nothing.
  *Con:* one more surface where the literal text diverges from bash. Chosen: the
  meaningful name. (Function-relative *line numbers* DO match bash.)
- **Syntax/parse errors.** bash inserts an extra `-c:` for `-c` parse errors
  (`bash: -c: line N: syntax error…`). osh uses the uniform
  `<name>: line N: syntax error…` form (no `-c:` insert). *Pro:* one code path,
  no special-casing of the invocation channel; the name differs anyway. *Con:*
  the `-c:` token is absent. Minor; chosen for simplicity.
- **`line N:` gated to non-interactive mode.** Matches bash (interactive bash
  omits the line number). osh's REPL therefore stays `osh: <msg>`.
- **`eprintln!` → `errln`.** Converted all error sites off `eprintln!` (which
  bypassed osh's stderr-redirect stack) onto `errln`/`emit_stderr`, so a
  diagnostic under `cmd 2>file` now goes to the file — bash parity, and a latent
  bug fix independent of the prefix itself.
- **Pure `builtin_usage()` lines stay unprefixed.** bash prints
  `<builtin>: usage: …` with no shell-name/line prefix; osh matches that exactly
  for the getopts/trap/unalias usage messages (excluded from the prefix helper).

**How to reverse.** Error-message formatting is trivially reversible: the prefix
is produced in one place (`Shell::err_prefix()`), so the format can be changed or
reverted centrally without touching the ~140 call sites again.

**Where it lives.** Strategy note only (no new code). Related: §72 (OSH),
§69 (giant-port ordering — Mesa/Chromium/WINE supply the C++ toolchain
prerequisite). Roadmap: YSH tracked as blocked-on-C++-toolchain under the Oils
line.

## 75. osh arithmetic error tokens — single consistent "offending-position-to-end" rule, accepting documented bash yacc-artifact divergences

**Date:** 2026-07-19
**Decided by:** Claude (operator-approved scope) — the operator authorized the
overall bash-compatibility feature (§74's directive "Continue porting a
bash-compatible shell from oils" / "port all of it"); Claude made the specific
error-token rule call below. See known-issues.md TD-OILS-ARITH-ERRFMT.

**Context.** Extending §74's diagnostic-format work to arithmetic errors, osh
now matches bash's full line `<name>: line N: [<builtin>: ]<expr>: <body> (error
token is "<tok>")`. The `<body>` taxonomy and `<expr>:` prefix are
unambiguous, but the `<tok>` ("error token") is not: **bash's own error-token
choice is internally inconsistent.** For division/modulo bash reports the whole
RHS *source* text; for exponent it reports its lexer's last-consumed *token*;
for a nested array subscript it reports a yacc-reduction fragment; at the
recursion limit it reports the innermost value. Byte-matching all of these at
once would require reproducing bash's exact yacc/lexer state, bug-for-bug.

**Decision.** Adopt one consistent rule for every raise site: the error token
is *the de-quoted source text from the offending position to end of input*
(operator position for operand-expected, RHS-operand start for div/mod/exp,
current position for trailing input, etc.). This matches bash byte-for-byte on
the common cases (25/27 probed) and is predictable/explainable. The residual
edge divergences are documented rather than special-cased:

- `$((2**-1))` — bash token `1`, osh `-1` (bash's exponent uses last-lexed token).
- `$((a[9/0]))` — bash echoes `9/0`, osh `a[9/0]` (yacc reduction artifact).
- recursion limit — bash echoes innermost value, osh the top-level expr.

**Rationale.** *Pro:* one code path, self-consistent behavior, no per-operator
lexer-state emulation; matches bash where it matters. *Con:* three rare
edge-case tokens differ from bash's literal output. Given bash is itself
inconsistent on these, chasing bug-for-bug parity is negative-value.

**Alternatives considered.** (a) Per-operator special-casing to mirror bash's
exact token in every case — rejected: high complexity, emulates bash bugs, brittle.
(b) Omit the error token entirely — rejected: loses real debugging value and the
`(error token is "…")` suffix is the most useful part for locating the fault.

**Where it lives.** `userspace/oils/src/arith.rs` — `ArithError { msg, token }`,
`AParser::rest_from`/`last_op_start`/`last_atom_start`, `Expr::Bin`'s RHS-source
4th field; `userspace/oils/src/interp.rs` — `emit_arith_error`, `eval_arith_cmd`,
`arith_cmd` (bash `this_command_name` model). Tests:
`arith.rs::error_bodies_and_tokens_match_bash`,
`interp.rs::arith_error_matches_bash_format`.

**How to reverse.** Token selection is localized to the `with_token(...)` call
sites in `arith.rs`; the rule can be changed per-site or the token dropped
centrally by making `Display` ignore it.

## 76. osh recursion limits — honour `FUNCNEST` exactly, plus a 64 MiB interpreter stack so legitimate deep recursion matches bash instead of aborting at ~300 frames

**Date:** 2026-07-19
**Decided by:** Claude (autonomous) — surfaced by a probe (`FUNCNEST=5;
f(){ f; }; f`) showing osh crashing with a native stack overflow where bash
prints a graceful error. Operator may revisit the stack size.

**Context.** osh is a tree-walking interpreter: each nested shell-function
call (and each compound command) recurses natively through
`exec_items → exec_and_or → exec_pipeline → call_function → exec_program → …`.
Those frames are large, so on the ~1 MiB default main-thread stack osh
overflowed and aborted (`thread 'main' has overflowed its stack`, exit 127)
after only ~200–400 nested calls — far short of the ~2000–4000 bash tolerates
before it segfaults. Two separate problems: (a) no `FUNCNEST` support at all,
so a user's explicit recursion guard was silently ignored; (b) even without
`FUNCNEST`, legitimate deep recursion (recursive shell algorithms) crashed
much sooner than bash.

**Decision.** (1) Implement `FUNCNEST`: when set to a positive integer N, refuse
the (N+1)th nested call with bash's exact diagnostic and a fatal
`jump_to_top_level(DISCARD)` (`Flow::Discard` — aborts the rest of the current
top-level command, bypassing `&&`/`||`/`;`, bounded by the nearest subshell,
then resumes at the next parse unit). 0 / empty / non-numeric ⇒ unlimited.
(2) Run the interpreter on a dedicated thread with a **64 MiB** reserved stack
(`INTERP_STACK_SIZE` in `main.rs`), which raises the crash threshold to
~6700+ levels (debug) — comfortably past bash — while `FUNCNEST` provides the
graceful ceiling when the user wants one.

**Rationale.** *Pro:* `FUNCNEST` is exact bash behaviour (byte-matched for
same-line abort, `||` non-catch, subshell containment, multi-line resume);
the big stack removes an easy DoS/robustness footgun where a shell aborts
uncleanly on a few hundred levels of legitimate recursion. The 64 MiB is
*reserved* virtual address space grown on demand via guard pages — not
eagerly committed — so it is cheap on the host and on SlateOS (whose
"committed by default" policy governs heap/mmap, not thread-stack reservation).
*Con:* osh still eventually aborts on truly-unbounded recursion (like bash's
segfault), just later; and 64 MiB is a magic number — too small for a
pathological workload, wasteful if a future SlateOS std eagerly commits stacks.

**Alternatives considered.** (a) A low default internal recursion cap that
converts overflow into a graceful error even without `FUNCNEST` — rejected:
any fixed cap either breaks legitimate deep recursion (too low) or still
crashes (too high), and it would diverge from bash, which has no such default.
(b) Leave the default 1 MiB stack and only add `FUNCNEST` — rejected: crashing
at ~300 legitimate levels is a real robustness defect for an OS shell.
(c) Convert the tree-walk to an explicit heap-allocated work stack (no native
recursion) — the truly principled fix, but a large rewrite; deferred.

**Where it lives.** `userspace/oils/src/interp.rs` — `funcnest_limit()`, the
guard at the top of `call_function` returning `Flow::Discard`;
`userspace/oils/src/main.rs` — `INTERP_STACK_SIZE`, the large-stack thread in
`main()`. Test: `interp.rs::funcnest_caps_recursion_like_bash`.

**How to reverse.** Drop the guard in `call_function` (and `funcnest_limit`) to
remove `FUNCNEST`; change or remove `INTERP_STACK_SIZE` / the thread wrapper in
`main()` to alter the stack. If (c) is ever done, both become unnecessary.

## 77. Oils (OSH) port strategy confirmed (Q26) — **finish the Rust reimplementation (A) now; keep A as a permanent user option even if a faithful C++ `oils-for-unix` port (B) lands later**

**Date:** 2026-07-21
**Decided by:** Operator (Claude proposed/recommended finishing A; operator
confirmed and added the long-term B-as-well framing) — resolves open-question
**Q26**, which §72 had deferred to the operator as a large, costly-to-reverse
call.

**Context.** §72 committed to building `userspace/oils` as a Rust
reimplementation of the OSH language because there is no C/C++ → `x86_64-slateos`
cross-toolchain in-tree, so a faithful `oils-for-unix` C++ cross-compile (option
B) is prerequisite-blocked. Q26 asked the operator to ratify that or re-order.

**Decision.** Finish the **Rust reimplementation (A)** — which had already reached
high maturity — and ship it. Keep A as a **permanent user-selectable option**
even after a genuine C++ `oils-for-unix` port (B) eventually becomes possible: the
project may adopt B later for bit-for-bit fidelity, but users will still be able
to choose the in-tree Rust `osh`. B is therefore an *additive future option*, not
a replacement that retires A.

**Rationale.** *Pro:* A is the only path that runs on SlateOS today; it's already
nearly done, so finishing it delivers a working bash-superset shell now, and
retaining it as an option hedges against B's fidelity/porting risk and gives
users a lightweight native-Rust shell that needs no C++ toolchain. *Con:*
maintaining two OSH implementations long-term (A and an eventual B) is ongoing
cost; A will never be bit-for-bit upstream OSH on obscure corners.

**Operator's exact words.** "Since I think you're already mostly done with option
A, go ahead and finish it if you're not already finished, but we may eventually
want to go with B, but we'll still have a as option for the user."

**Where it lives.** `userspace/oils/`. Supersedes the "open" status of Q26 in
open-questions.md (now removed). §72 records the original A-vs-B rationale.

## 78. `osh` advertises itself as bash via `$BASH_VERSION`/`$BASH_VERSINFO` (Q27) — **option A (advertise), made a per-user toggle defaulting on**, mirroring upstream Oils' `bash_compat`

**Date:** 2026-07-21
**Decided by:** Operator (Claude recommended A and proposed the toggle; operator
chose A and asked that it be a per-user option defaulting to A) — resolves
open-question **Q27**.

**Factual finding that informed the call (operator asked "what does the original
osh itself do?").** Upstream Oils' `osh` **does advertise as bash by default.**
`core/shell.py` sets `BASH_VERSION='5.3'` and `BASH_VERSINFO=(5 3 0 0 release
unknown)`, gated on a `bash_compat` flag that defaults **on for `osh`** and off
for `ysh` (Oils' honest non-bash language). So option A matches upstream `osh`
exactly, and making it a toggle mirrors upstream's own `bash_compat` switch.

**Decision.** `osh` sets both `BASH_VERSION` and `BASH_VERSINFO` by default
(option A), controlled by the per-user env toggle **`OSH_BASH_COMPAT`** (default
**on**; `0`/`off`/`false`/`no` disable it, whereupon both variables are left
unset so bash-detecting scripts see a non-bash shell — like `ysh`). We keep the
reported level at **`5.2.0(1)-release`** / `BASH_VERSINFO=(5 2 0 1 release
x86_64-slateos)` — deliberately **5.2, not upstream's 5.3** — because osh targets
bash-5.2 semantics and must never claim a 5.3-only feature it doesn't implement.

**Rationale.** *Pro:* the dominant real-world use of `$BASH_VERSION` is the "is
this bash? then run the bash branch" gate that a bash-superset shell *wants* to
satisfy; matches upstream `osh`; the toggle lets a user who wants honesty opt out.
*Con:* advertising bash is a deliberate half-truth — a script may then assume a
specific bash behaviour osh implements slightly differently; the toggle is
env-based (process-global) so it's a startup/login-time choice, not per-invocation.

**Operator's exact words.** "I lean A, but what does the original osh itself do?
… Perhaps Q27 should be a user option, too, that defaults to A?"

**Where it lives.** `userspace/oils/src/interp.rs` — `bash_compat_enabled()`
(reads `OSH_BASH_COMPAT`), gating the `BASH_VERSION`/`BASH_VERSINFO` seeds in
`seed_shell_vars`; `BASH_VERSION` const at interp.rs:108. Supersedes Q27 in
open-questions.md (now removed).

## 79. `osh` identity vars `$EUID`/`$UID` (Q28) — **default root (`0`/`0`) [option A], made per-user configurable** via `OSH_UID`/`OSH_EUID`

**Date:** 2026-07-21
**Decided by:** Operator (Claude recommended A = root default; operator accepted
the recommendation and added that it be the *default* with a per-user override) —
resolves open-question **Q28** (the `$HOSTNAME` half was already resolved
autonomously 2026-07-20).

**Context.** bash always defines readonly-integer `$EUID`/`$UID`; scripts lean on
them constantly (the canonical `[ "$EUID" -ne 0 ]` root check). osh left them
unset, so those comparisons errored on an empty operand. SlateOS has no
`getuid`-equivalent wired into the host or target build yet, so osh can't read a
real credential — the *reported* identity is a policy choice.

**Decision.** Seed `$UID` and `$EUID` as **real readonly-integer shell vars**
(`declare -ir`, matching bash's own attributes), defaulting to **root
(`0`/`0`)** — the shell genuinely *is* the all-powerful system during pre-privilege
bring-up, so root-gated scripts should take their root path. The reported identity
is **per-user configurable** via the `OSH_UID` / `OSH_EUID` env toggles (resolved
once in the free fn `reported_identity`; `OSH_EUID` defaults to `OSH_UID` if
unset), so a login/session layer can inject a real per-user identity. Seeded
*before* `import_environment` so an inherited `UID=` in the environment neither
overrides nor becomes exported (matching bash: UID/EUID are non-exported shell
vars). The `\$` prompt escape now keys `#`-vs-`$` on `$EUID == 0`.

**Faithfulness note (why real vars, not the dynamic `PPID` model).** osh's other
readonly-integer specials (`PPID`/`BASHPID`) are computed dynamically in
`param_value` and are *not* actually readonly-enforced (`PPID=5` is silently
accepted) nor listed in bulk `declare -i`/`declare -p`. EUID/UID instead use real
`self.vars` entries + `self.readonly` + `self.integer_attr`, which makes them
correctly readonly-enforced and correctly present in `declare -i`/`declare -p`/
`set`/`${!prefix*}` listings — strictly more bash-faithful. The remaining
dynamic specials are logged in known-issues (TD-OILS-IDVARS) as a latent
inconsistency to migrate later.

**Rationale.** *Pro:* fixes the extremely common `$EUID`/`$UID` idioms; root
default matches current single-user bring-up reality; the per-user override models
the eventual "shell runs in a user session" norm and lets a future login layer set
a real identity without code change. *Con:* a "are you root? then it's safe" script
proceeds where a real multi-user system wouldn't — masks the absent privilege
model; the override is env-based (process-global), a startup/login choice.

**Operator's exact words.** "I'll go with your recommendation, though I suggest
making that the default and actually giving the user the option of what to report,
per user."

**Where it lives.** `userspace/oils/src/interp.rs` — `reported_identity()` (reads
`OSH_UID`/`OSH_EUID`), the UID/EUID seed in `seed_shell_vars`, the `\$` prompt
escape (~4658). Test: `special_var_identity_uid_euid`. Supersedes Q28 in
open-questions.md (now removed); known-issues TD-OILS-IDVARS updated.

## 80. fastpy → SlateOS integration (initiative F) target strategy (Q29) — **pure-mode native compile first (A); add the CPython bridge later as a superset (B)**

**Date:** 2026-07-21
**Decided by:** Operator (Claude recommended A-first-then-B; operator confirmed
"A at first but eventually B") — resolves open-question **Q29**, unblocking the
*start* of initiative F.

**Context.** fastpy is an AOT Python→LLVM-IR→native compiler that today targets the
**host** only and links a C runtime plus an embedded-CPython bridge (for programs
using unsupported stdlib). Making it emit **SlateOS** binaries needs: (1) an
`x86_64-slateos` LLVM target/data-layout (modest), (2) the C runtime ported to
SlateOS syscalls/libc (gated on the Phase 2.5 POSIX layer), and (3) a decision on
the CPython bridge — the crux Q29 asked.

**Decision.** Begin with **pure-mode only (A)**: on the SlateOS target, compile
only programs fastpy supports natively and **disable the CPython fallback**. Add
the CPython bridge later (**B**) as an *enhancement/superset* once CPython is
ported to SlateOS (a large, later effort) — B becomes a strict superset of A, not
a competing design. Sequencing: mature the POSIX layer enough to host the C
runtime → add the `x86_64-slateos` fastpy target + port the runtime in pure mode →
pick one real OS component (e.g. the package manager) as the first
fastpy-compiled SlateOS binary.

**Rationale.** *Pro:* A is the only path that both starts soon *and* delivers the
roadmap's stated goal (native components that run *on* SlateOS); no CPython port
needed up front; matches how OS components would actually be written (plain typed
Python). *Con:* until fastpy grows native support for a given stdlib module, a
component using it won't compile on SlateOS — "any valid Python is valid fastpy"
doesn't hold *on-target* until B lands; commits the project to a prerequisite
chain (POSIX layer → runtime port → first component).

**Operator's exact words.** "A at first but eventually B."

**Where it lives.** fastpy `compiler/toolchain.py` (target triple / data layout,
currently host-only via `llvm.Target.from_default_triple()`; `link_executable`
unconditionally links libpython — the bridge to gate off for the slateos target),
fastpy `runtime/*.c` (syscall/libc surface to port), SlateOS `posix/` (Phase 2.5
libc coverage the runtime links against); roadmap.md Phase 0 (the F task).
Supersedes Q29 in open-questions.md (now removed).

## 81. C cross-toolchain for fastpy's SlateOS runtime (Q30) — **option A (a clang cross-toolchain to musl), realized via `zig cc`**

**Date:** 2026-07-21
**Decided by:** Operator (operator said "do A"; Claude proposed and implemented
`zig cc` as the concrete mechanism for A) — resolves open-question **Q30**,
delivering the runtime-port + full-program-link increments of initiative F.

**Context.** With §80's pure-mode-first strategy chosen and the codegen +
link halves already built and tested, the one remaining prerequisite for a
*runnable* SlateOS binary was compiling fastpy's C runtime (`runtime/*.c`) to
`x86_64-slateos` (musl) ELF objects. That needs a C compiler emitting
`x86_64-unknown-linux-musl` ELF **and** musl C headers to compile against. The
dev host had neither: no `clang` on PATH, MSVC `cl.exe` emits COFF/MSVC-ABI, and
the OS sysroot (`toolchain/sysroot/lib`) ships only Rust-built archives with no C
headers. Q30 offered: **A** install a clang cross-toolchain + vendor musl
headers; **B** reimplement the runtime as fastpy-generated IR; **C** wait for the
CPython bridge. Claude recommended A.

**Decision.** **A**, realized with **`zig cc --target=x86_64-linux-musl`**. `zig
cc` is a self-contained clang plus bundled musl headers and musl libc in one
portable download (zig 0.16.0 unpacked at `D:\utils\zig-x86_64-windows-0.16.0\`,
~97 MB, no installer, not system-wide). This *is* option A (a clang cross-
toolchain to musl) but its packaging sidesteps **both** of A's listed cons: no
heavyweight system-wide LLVM install, and no separately vendored/maintained musl
header set (zig ships a consistent musl). rust-lld (already used for the link
half, same LLVM family) links the result.

**Rationale.** *Pro:* directly unblocks the runtime port with the proper,
robust musl cross-compile path; portable and reproducible; avoids the large,
error-prone rework of option B (reimplementing GC/bigint/threading as generated
IR) and the stranding of option C. *Con:* introduces a zig dependency for the
SlateOS runtime build (mitigated: located via `$FASTPY_ZIG`/PATH/portable-dir
fallback, and only needed for the SlateOS target — the host build is unchanged);
zig's bundled musl must stay ABI-compatible with the OS `posix` crate's libc
shape (it does — both are standard x86-64 System V / musl-shaped, and a
successful static non-PIE link proves symbol resolution).

**Operator's exact words.** "okay, do A."

**Outcome (implemented same day).** fastpy `compiler/toolchain.py` gained
`_find_zig_cc()`, `_find_slateos_sysroot_lib()`, `_compile_shared_runtime_slateos()`,
and `ensure_slateos_runtime_built()`; `link_executable(target=SLATEOS_TARGET)` now
builds the six pure-mode TUs (`runtime`, `objects`, `threading`, `gc`, `bigint`,
and a new **`bridge_stub.c`** substituting for the CPython bridge) and links
program + runtime + sysroot `libc.a` via rust-lld. Pure mode is selected with
`-DFPY_PURE_MODE` (compiles out the JIT symbol table). A real fastpy program
(lists/iteration/print) links to a **~2.9 MB SlateOS ET_EXEC ELF with zero
undefined symbols**. Tests in fastpy `tests/test_cross_target.py` (skip without
zig/rust-lld/sysroot). **Crt startup wired (same day):** the link entry is
`_start` (the real ELF entry), so rust-lld pulls the crt0 from `libc.a` — the
`posix` crate is built for `x86_64-unknown-none`, so its `#[cfg(target_os =
"none")]` crt (`_start` → `__libc_start_main` → retrieve argv/envp via kernel
syscall, init environ/signals, run ELF constructors → `main` → `exit`) is
present. The ELF's entry is nonzero `_start`, so startup/args/clean-exit are all
in place. Remaining: compile one real OS component and boot it under the kernel
to confirm on-target behavior — the "first real component" milestone.

**Where it lives.** fastpy `compiler/toolchain.py`, `runtime/bridge_stub.c` (new),
`runtime/{runtime,objects,objects.h,threading.h}.c/.h` (pure-mode guard + latent
MSVC-ism fixes for clang), `tests/test_cross_target.py`; OS `toolchain/sysroot/lib`
(`libc.a` linked against). Supersedes Q30 in open-questions.md (now removed).

## 82. SlateOS native-ABI main-thread ELF TLS setup (Q31) — **option A (the posix crt sets up TLS in userspace via `__ehdr_start`), plus a native `SYS_SET_FS_BASE`**

**Date:** 2026-07-21
**Decided by:** Operator (operator said "I'll go with A"; Claude recommended A) —
resolves open-question **Q31**, unblocking initiative F's "first real component"
(on-target execution) milestone.

**Context.** Booting a fastpy-built SlateOS binary surfaced a native-ABI gap: the
ELF carries a `PT_TLS` segment because fastpy's C runtime uses compiler
thread-locals (`runtime/threading.h`: `FPY_THREAD_LOCAL __thread`), which the
compiler lowers to `%fs:offset` (x86-64 TLS variant II) and which therefore
require an `%fs` base (thread pointer) before first use. But SlateOS gives a
*native* static binary none: the kernel resets `fs_base` to 0 on exec expecting
userspace to set it up (the Linux/libc model), yet — unlike a Linux crt that
reads the aux vector and calls `arch_prctl(ARCH_SET_FS)` — the posix crt fetches
argv/fds via native syscalls (no aux vector) and never sets up TLS. So the first
`__thread` access faults. This is *additive*, not a reversal of the posix crate's
deliberate "tid-keyed pthread TSD instead of FS/GS TLS" choice (that's *library*
TSD via `pthread_setspecific`; compiler `__thread` is a distinct mechanism that
any real C/C++ on SlateOS needs). Q31 offered **A** (posix crt sets up TLS in
userspace, finding `PT_TLS` via the linker-defined `__ehdr_start` — no aux vector
needed — and setting `fs_base` via the existing kernel primitive), **C** (the
kernel ELF loader sets it up during spawn), or **B** (fastpy drops `__thread` on
SlateOS — a correctness trap for multithreaded programs; recommended against).
Claude recommended A.

**Decision.** **A.** `__libc_start_main` (in `posix/src/crt.rs`), before running
constructors or `main`, locates the program's `PT_TLS` program header via the
linker-defined `__ehdr_start` symbol (present in static non-PIE links), allocates
a variant-II TLS block + a TCB whose first word self-points, copies the `p_filesz`
init image and zeroes the `.tbss` remainder, then sets the thread pointer. Setting
`fs_base` uses a **new native syscall `SYS_SET_FS_BASE`** that calls the kernel's
existing `set_current_task_fs_base` (`kernel/src/sched/mod.rs`) — the native
counterpart of the Linux-ABI `arch_prctl(ARCH_SET_FS)`, so native binaries need no
Linux-table dispatch. The TCB reserves enough space to cover the stack-protector
canary slot (`%fs:0x28`) in case the runtime is built with `-fstack-protector`.

**Rationale.** *Pro:* keeps TLS setup in userspace, matching the kernel's existing
"reset to 0, userspace sets it up" design; keeps the microkernel loader minimal;
consistent with the native crt already doing its own startup (args/fds/environ/
signals/constructors); generalizes to child-thread TLS (`pthread_create`) in the
same layer later. *Con:* implements variant-II TLS layout in the posix crate and
adds one native syscall; child-thread TLS is deferred (documented) until a fastpy
program actually spawns threads.

**Operator's exact words.** "I'll go with A."

**Where it lives.** `posix/src/crt.rs` (`setup_main_thread_tls()`, called at the
top of `__libc_start_main`), `posix/src/syscall.rs` (`SYS_SET_FS_BASE` number +
wrapper), the kernel native syscall dispatch + `kernel/src/sched/mod.rs::
set_current_task_fs_base`; then rebuild the sysroot `libc.a`, rebuild the fastpy
binary, embed + spawn (`services/fastpy-hello`, self-test
`kernel/src/proc/spawn.rs::self_test_fastpy_slateos_tls`), and boot-test.
Supersedes Q31 in open-questions.md (now removed).

**Outcome (2026-07-21) — DONE and validated on-target.** The fastpy binary
`services/fastpy-hello` boots under the kernel and runs to `exit(0)` at ring 3;
the self-test asserts `Zombie` + exit 0 and the full boot-test passes
(BOOT_OK, no self-test failures). Serial: *"fastpy-on-SlateOS TLS … set up
main-thread ELF TLS via SYS_SET_FS_BASE and ran to exit(0): OK"*.

**Latent bug fixed en route.** The first attempts faulted (exit -8): the crt's
TLS `mmap` came back `PermissionDenied` (-400). Root cause was **not** the TLS
logic but a syscall-number skew — `posix/src/syscall.rs` numbered the native
memory syscalls `SYS_MMAP=30`, `SYS_MUNMAP=31`, `SYS_MPROTECT=32`, which alias
the kernel's *capability-gated* IRQ syscalls (`SYS_IRQ_REGISTER=30`,
`SYS_IRQ_WAIT=31`, `SYS_IRQ_RELEASE=32`), while the kernel's real native numbers
are `SYS_MMAP=20`/`SYS_MUNMAP=21`. So a native `mmap()` was rejected by the
capability check *before* `sys_mmap` ran (hence no handler-level trace). Fixed by
correcting posix to 20/21/22 and reserving `SYS_MPROTECT=22` in
`kernel/src/syscall/number.rs`. Note `SYS_MPROTECT=22` has no native handler yet
(native `mprotect()` → `ENOTSUP`, safe); the Linux-ABI mprotect is unaffected.
Tracked in known-issues.md (BUG-NATIVE-MMAP-NUM resolved; TD-NATIVE-MPROTECT
open).

**Follow-up (2026-07-30) — the deferred child-thread half landed; see §91.**
The "generalizes to child-thread TLS later" promise in the rationale above is
now fulfilled: the layout code moved out of `crt.rs` into a shared
`posix/src/tls.rs` that both `__libc_start_main` and `pthread_create` use, so
the `setup_main_thread_tls()` named above no longer exists (it is
`tls::setup_main_thread()`).

---

## 83. fastpy `os.setuid`/`os.setgid` — thin kernel mutation primitive + userspace-enforced POSIX cap policy (initiative F)

**Decided by:** Claude (autonomous)

**Context.** Until this increment, posix `setuid()`/`setgid()` (and the
`setre*`/`setres*` family) were permission-checking *stubs*: they ran the
CAP_SETUID/CAP_SETGID + identity check, then **returned success without
changing any credential state**. So an unprivileged program that "dropped
privilege" actually kept it — `os.getuid()` after `os.setuid()` still read the
old id, and the kernel's `ProcessCredentials` were untouched. Making the
setuid/setgid family real forces a choice about *where the authority lives*:
should the kernel decide who may change to which uid, or should userspace?

**Decision.** Keep the permission **policy in userspace** (the existing
POSIX-capability model in `posix::sys_capability`: a change is allowed iff the
target equals the current id, or the caller holds CAP_SETUID/CAP_SETGID), and
add a **thin kernel mutation primitive**, `SYS_PROCESS_SET_CREDENTIALS` (530),
that simply writes the *calling* process's `ProcessCredentials` (arg0=uid,
arg1=gid; sentinel `0xFFFF_FFFF` = leave that field unchanged). The kernel
enforces only "you may mutate your own process"; it does **not** re-derive or
second-guess the uid policy. posix `setuid`/`seteuid`/`setgid`/`setegid`/
`setreuid`/`setregid`/`setresuid`/`setresgid` now perform the cap/identity
check and then call `unistd::set_real_credentials(uid, gid)` → the syscall.

**Rationale.** POSIX capabilities in SlateOS are **userspace-only** — they live
in `posix::sys_capability`'s `CAP_EFF_LO/HI` atomics with *no kernel backing*
(the kernel's Fuchsia-style `CapTable` is a separate, handle-based system the
kernel cannot correlate with POSIX cap bits). So the kernel **cannot** enforce
a "only root may setuid to an arbitrary uid" rule anyway — it has no authority
to check. Given that, the consistent design is the one the kernel already uses
for every other cap-gated operation: **trust the userspace cap check** and give
the kernel a minimal, mechanism-only syscall. This also keeps host and target
behavior aligned and preserves the large existing Phase 192–195 posix test
suite, which encodes exactly this CAP_SETUID/CAP_SETGID model.

**Alternatives considered.**
- *Kernel-authoritative root/non-root rule* (kernel checks: uid 0 may set any
  id, others may only set their own): rejected. It would (a) break the Phase
  192–195 tests that codify the CAP-based model, (b) diverge host vs target,
  and (c) create a *false* sense of kernel enforcement while the real authority
  (the handle-based CapTable) is unrelated to the credential uid — the kernel
  check would be theater, not security.
- *Leave the stubs as-is*: rejected — a privilege-drop primitive that silently
  no-ops is a latent security footgun.

**Known limitation (tracked in known-issues.md).** Because policy is in
userspace, a process that issues the raw `SYS_PROCESS_SET_CREDENTIALS` syscall
directly (bypassing the posix wrapper's cap check) can set its own uid to 0.
This is **inert today**: no kernel authority is derived from the credential
uid (the design defers uid-based access control to "once a login service
exists"), and the real authority is the unforgeable handle-based CapTable, so a
forged credential uid grants nothing. When credential-uid-based authority is
introduced, this syscall must move its policy check into the kernel (or gate
the syscall behind a capability).

**Where it lives.** `kernel/src/syscall/number.rs` (const + doc),
`kernel/src/syscall/handlers.rs::sys_process_set_credentials`,
`kernel/src/syscall/dispatch.rs`, `posix/src/syscall.rs` (const mirror),
`posix/src/unistd.rs` (`set_real_credentials`, `uid_change_permitted`,
`gid_change_permitted`, and the rewritten setuid/setgid family). fastpy
lowering: `compiler/codegen.py` (5 sites), `runtime/objects.c`,
`runtime/runtime.c`. Self-test: `kernel/src/proc/spawn.rs::
self_test_fastpy_slateos_setuid`; tool: `services/fastpy-setuid/`.

**How to reverse.** To move to kernel-authoritative policy, add the uid rule to
`sys_process_set_credentials` (check the caller's current uid before applying),
drop the userspace cap short-circuit, and update the Phase 192–195 tests.

## 84. fastpy `os.nice`/`os.getpriority`/`os.setpriority` — make nice a *real* scheduler attribute via a thin kernel mutation primitive (initiative F)

**Decided by:** Claude (autonomous)

**Context.** Until this increment, posix `nice()`/`getpriority()`/`setpriority()`
stored the nice value in a **process-local userspace static** (`NICE_VALUE`)
that had **zero** effect on kernel scheduling. A program could "renice" itself,
read the new value back, and believe it had changed its priority while the
scheduler ignored it entirely. Separately, the *Linux-ABI* path
(`sys_setpriority`/`sched_setattr`) wrote a `linux_nice` field into the PCB via
`pcb::set_nice` that *also* had no scheduling effect. So there were **two**
disconnected, inert nice stores and neither moved a task in the run queue.
Making nice real forces two choices: (a) how nice maps to our 0..31 priority
band, and (b) where the CAP_SYS_NICE policy lives.

**Decision.** Add a **thin kernel mutation primitive**,
`SYS_PROCESS_GET_NICE` (531) / `SYS_PROCESS_SET_NICE` (532), and a single
authoritative kernel entry point `proc::thread::set_process_nice(pid, nice)`
that (1) writes the PCB `linux_nice` field **and** (2) re-prioritises every task
the process owns via `sched::set_priority(tid, nice_to_priority(nice))`. Both
the native syscall path *and* the Linux-ABI `sys_setpriority`/`sched_setattr`
fair-nice branch now funnel through `set_process_nice`, so nice is one real
scheduling attribute regardless of which ABI sets it. The CAP_SYS_NICE **policy
stays in userspace** (posix `resource.rs`), exactly as decision #83 keeps
setuid/setgid policy in userspace; the kernel enforces only "you may renice your
own process." The nice↔priority map is
`priority = round((nice+20)·31/39)` (nice −20→0 highest, 0→16 default, 19→31
lowest). The syscall ABI carries nice **biased by +20** (range 0..39) so the
value is always non-negative and can never collide with the negative
`SyscallResult` error sentinels; userspace subtracts 20.

**Rationale.** Same reasoning as #83: POSIX capabilities are userspace-only, so
the kernel cannot meaningfully enforce the CAP_SYS_NICE raise-policy — trusting
the userspace wrapper and giving the kernel a mechanism-only syscall is the
consistent design. Unifying the two previously-disconnected nice stores behind
one `set_process_nice` avoids band-aid accumulation (two inert stores that both
claim to be "the" nice). The +20 bias is the minimal ABI change that keeps a
plain register return unambiguous against error codes.

**Alternatives considered.**
- *Leave the userspace-static stub*: rejected — a priority primitive that
  silently no-ops is a latent footgun (a service that lowers its priority to
  yield CPU would keep hogging it).
- *Kernel-authoritative CAP_SYS_NICE*: rejected for the same reasons as #83 —
  the kernel has no POSIX-cap authority to check, so the check would be theater
  and would diverge host vs target.
- *Raw (unbiased) nice over the syscall register*: rejected — a −20..19 range
  overlaps the negative error-sentinel space of `SyscallResult`; the +20 bias
  sidesteps it with no information loss.

**Where it lives.** `kernel/src/syscall/number.rs` (consts + docs),
`kernel/src/proc/thread.rs` (`nice_to_priority`, `set_process_nice`),
`kernel/src/syscall/handlers.rs` (`sys_process_get_nice`/`sys_process_set_nice`),
`kernel/src/syscall/dispatch.rs`, `kernel/src/sched/mod.rs`
(`get_base_priority`), `kernel/src/syscall/linux.rs` (route
`sys_setpriority`/`sched_setattr` through `set_process_nice`),
`kernel/src/proc/pcb.rs` (doc updates), `posix/src/syscall.rs` (const mirror),
`posix/src/resource.rs` (`kernel_get_nice`/`kernel_set_nice` + rewritten
nice/getpriority/setpriority). fastpy lowering: `compiler/codegen.py`,
`runtime/objects.c`, `runtime/runtime.c`. Self-test:
`kernel/src/proc/spawn.rs::self_test_fastpy_slateos_nice`; tool:
`services/fastpy-nice/` (uses a *negative* nice — a CAP_SYS_NICE-gated priority
*raise*, exercising that path — and then **sleeps** rather than busy-spins after
writing its output: having raised its priority above the polling harness, a spin
would starve the harness and livelock the boot, whereas a blocked task still
keeps its scheduler slot and readable base priority).

**How to reverse.** To move to kernel-authoritative nice policy, add the
CAP_SYS_NICE raise-rule to `sys_process_set_nice` and drop the userspace
short-circuit. To change the priority band mapping, edit
`thread::nice_to_priority` (and the self-test's expected priority).

## 85. fastpy `os.umask` + the file-creation-mode ABI gap — thread the caller's create mode through a *new backward-compatible syscall number* rather than overloading the existing one (initiative F)

**Decided by:** Claude (autonomous)

**Context.** `os.umask` was untestable-as-real because of a deeper gap: the
native VFS create path **ignored the caller's mode entirely**. `SYS_FS_OPEN`
(610) and `SYS_FS_MKDIR` (604) took no mode argument, so every newly-created
file was stamped a hardcoded `0o644` and every new directory `0o755`,
regardless of what the caller requested or what the umask was. A program could
`umask(0o022)` then `open(path, O_CREAT, 0o777)` and still get `0o644` on disk —
the umask (and the mode) had **zero observable effect**. Making umask real
therefore first requires closing the create-mode ABI gap. Two sub-decisions
fall out: (a) **how** to thread the mode through without breaking already-built
binaries, and (b) **where** umask masking is computed.

**Decision.**
1. **New syscall numbers, not an overload.** Add `SYS_FS_OPEN_MODE` (659,
   arg3 = create mode) and `SYS_FS_MKDIR_MODE` (660, arg2 = mode) as *distinct*
   numbers. The old `SYS_FS_OPEN`/`SYS_FS_MKDIR` keep their exact prior
   default-mode behavior. The kernel treats an arg mode of `0` as "unspecified →
   historical default" (`0o644` file / `0o755` dir), so a non-`O_CREAT` open that
   passes 0 never creates a `0o000` file.
2. **umask lives in userspace.** The posix create wrappers compute
   `mode & ~umask` (`apply_umask`) and pass the **final on-disk permission bits**
   to the kernel, which stamps them verbatim. `UMASK_VALUE` is a process-local
   static in the posix libc (default `0o022`); the kernel has no umask concept.
   This matches decisions #83 (setuid) and #84 (nice): the kernel is a thin
   mutation primitive, POSIX policy is userspace.

**Rationale.** Overloading arg3 of the *existing* `SYS_FS_OPEN` (my first
attempt) is a real regression: `syscall3` only loads rdi/rsi/rdx and `syscall2`
only rdi/rsi, but the kernel populates `SyscallArgs` from **all** arg registers
uniformly. Every already-built embedded ELF (there are ~48 fastpy self-test
binaries plus the whole userspace) calls the old `open`/`mkdir` **without**
setting the mode register — so an overload would make them pass whatever garbage
was left in that register as the file mode, corrupting permissions on file
creation *during boot*. A new number is the only backward-compatible way: stale
binaries keep hitting the old default-mode handler, and only freshly-rebuilt
posix wrappers use the mode-carrying number. Keeping umask masking in userspace
keeps the kernel free of per-process mask state and keeps the on-disk-bits
contract identical whether a file is created via the native ABI or any future
one.

**Alternatives considered.**
- *Overload arg3/arg2 of the existing syscalls*: rejected — breaks every
  pre-built binary (garbage mode register → corrupted create permissions at
  boot), as caught before committing.
- *Kernel-side umask state* (a per-process mask the kernel subtracts): rejected —
  adds per-process kernel state for a pure-POSIX-policy concept, and would
  duplicate the masking logic the userspace wrapper already needs for the
  host-test build. Thin primitive + userspace policy is consistent with #83/#84.
- *Mode 0 means literally 0o000*: rejected — a non-`O_CREAT` open legitimately
  passes 0 (no create), so 0 must mean "use the default", not "create a
  no-permission file". (Documented limitation: a mask that computes to exactly
  `0` falls back to `0o644`; vanishingly rare and harmless.)

**Where it lives.** `kernel/src/syscall/number.rs` (consts + rationale docs),
`kernel/src/fs/handle.rs` (`DEFAULT_CREATE_MODE`, `open_with_mode`),
`kernel/src/fs/vfs.rs` (`DEFAULT_DIR_MODE`, `mkdir_mode`),
`kernel/src/syscall/handlers.rs` (`sys_fs_open_mode`/`sys_fs_mkdir_mode`),
`kernel/src/syscall/dispatch.rs`, `posix/src/syscall.rs` (const mirror),
`posix/src/file.rs` (`apply_umask` + rewired `open`/`mkdir` wrappers). fastpy
lowering: `compiler/codegen.py`, `runtime/objects.c`, `runtime/runtime.c`.
Self-test: `kernel/src/proc/spawn.rs::self_test_fastpy_slateos_umask`; tool:
`services/fastpy-umask/` (umask(0o077)→prior 18, umask(0o022)→prior 63, then
`os.open(mode 0o777)` under mask 0o022 must yield `0o755` on disk — distinct
from both the `0o644` default and the `0o777` request, so neither a
"umask-ignored" nor a "mode-ignored" bug can false-pass).

**How to reverse.** To move umask into the kernel, add a per-process mask to the
PCB and subtract it in `sys_fs_open_mode`/`sys_fs_mkdir_mode`, then drop
`apply_umask` from the posix wrappers. To retire the old numbers once every
binary is rebuilt, point `SYS_FS_OPEN`/`SYS_FS_MKDIR` at the mode handlers with a
default mode and delete 659/660.


## §86 — Build KASAN-style shadow memory to root-cause B-KNULLJUMP

**Date:** 2026-07-23
**Decided by:** Operator (Claude recommended A; operator concurred — "A")

**Decision.** Invest a focused effort in building **KASAN-style heap-corruption
detection** (option A of Q32) rather than keeping B-KNULLJUMP on WATCH (C) or
relying on a fragile hardware-watchpoint hunt (B). Build a **1/8-scale shadow
memory** region that marks every kernel-heap byte as addressable or poisoned,
with instrumented alloc/free and — on the suspect Path-Z spawn/teardown paths —
checked stores, so the corruption is caught **at the corruptor's write** instead
of at the victim's much-later read.

**Why.** B-KNULLJUMP is an intermittent (~1-in-120) kernel memory corruption at
Path-Z process spawn/teardown, on WATCH for many sessions. On 2026-07-22 it was
finally *symbolized* (see `known-issues.md`): the victim is a scheduler
`BTreeMap` node (`SchedState.tasks`) whose link pointer is zeroed — a *live-node*
wild write that the existing slab poison/redzone (`mm/poison.rs`, `mm/heap.rs`)
cannot catch (it is neither a UAF of a poisoned slot nor an adjacent-redzone
overflow). Shadow memory catches this whole class durably and layout-
independently, and pays off for all future memory bugs.

**Alternatives considered.**
- *B — hardware-watchpoint hunt on the reliable reproducer*: rejected as primary
  — the corrupted address isn't known a priori, varies with layout, and the
  reproducer is fragile (dissolves on the next kernel edit). May still be used
  opportunistically once shadow memory narrows the writer.
- *C — keep on WATCH, continue features*: rejected — leaves a real kernel
  corruption (can halt boot) unfixed and wastes the current concrete symbols.

**Tradeoffs (why this needed the operator).** Sizable new `mm` subsystem; memory
overhead (1/8 of the heap); perf cost on the alloc/store hot path — so it **must
be debug-gated** to protect the <200 ns heap-alloc target. std `BTreeMap`'s
internal stores may not be instrumentable without compiler support, so a targeted
store-check shim on the suspect paths may be needed.

**Where it lives.** `kernel/src/mm/` (new shadow module + hooks in `heap.rs`,
extending `poison.rs`), `kernel/src/mm/frame.rs` (buddy `FreeNode`), the Path-Z
teardown path in `kernel/src/proc/`, and `kernel/src/sched/mod.rs`
(`SchedState.tasks` — the observed victim). Tracked as B-KNULLJUMP in
`known-issues.md`.

## §87 — Reduce embedded-ELF kernel bloat before promoting fastpy coreutils to /bin

**Date:** 2026-07-23
**Decided by:** Operator (Claude recommended A; operator leaned B — "I lean towards B")

**Decision.** For the next phase of initiative F, do **option B — reduce the
embedded-ELF kernel bloat (TD-KERNEL-EMBED-BLOAT) first** — before **option A**
(promoting fastpy-compiled coreutils to real `/bin` commands driven by the
shell). The ~48 fastpy self-test ELFs are currently `include_bytes!`'d into the
kernel's `.rodata` (~3.5 MiB each; kernel image ~202 MB). Move them (and future
fastpy binaries) onto the **rootfs disk** and load-from-disk.

**Why.** The initiative-F `os.*` self-test line has *saturated* (identity,
scheduling nice, umask+create-mode, metadata, content I/O, namespace, query,
timekeeping, sleep, pipes, the whole pkg suite) — further self-tests are low-value
churn. Both A and B are the real "native executables for OS components" payoff.
The operator leaned B because it is real, growing tech debt (slow builds/boot from
a 202 MB image) and is *prerequisite-ish* for a `/bin` that lives on disk anyway —
so doing B first avoids building the `/bin` install pipeline twice.

**Alternatives considered.**
- *A — promote fastpy coreutils to real `/bin` commands (Claude's recommendation,
  highest end-value)*: deferred, not rejected — it is the actual roadmap goal and
  the natural follow-on once `/bin` lives on disk (B). Its verification fork
  (non-interactive boot-test; osh command-resolution surface) remains to be
  settled when A is taken up.
- *C — declare the self-test phase done and move to an unrelated roadmap area*:
  rejected — leaves the "native executables for OS components" payoff unrealized.

**Tradeoffs / open sub-questions (to resolve while implementing B).** Boot-ordering:
the self-tests currently run *before* the rootfs (Path-Z glibc `rootfs.ext4`, vdb)
is guaranteed mounted, so either the self-tests must move after mount, or a small
early-mount / dedicated self-test partition is needed. Also a mechanism sub-choice
(plain disk files vs a compressed archive vs a dedicated fastpy-bin image). These
are implementation decisions for B, not re-openings of the A/B/C fork.

**Where it lives.** `kernel/src/proc/spawn.rs` (the `include_bytes!` self-test
harness — every `self_test_fastpy_slateos_*`), the boot-image assembly, the rootfs
build, and `scripts/boot-test.sh` (non-interactive verification). Tracked as
TD-KERNEL-EMBED-BLOAT in `known-issues.md`.

**Sub-design resolved (2026-07-23) — implementing B.** *Decided by:* Claude
(operator-approved scope). The two open sub-questions flagged above are now
settled from reading the code:
- *Boot-ordering: no reordering needed.* The rootfs (`rootfs.ext4`, vdb) is
  mounted at `/mnt` in `kmain` at ~line 1180 (`fs::ext4::mount(ext4_dev,"/mnt")`),
  which runs **long before** the Path-Z self-test block (~line 2320+) where the
  fastpy tests live. So the fixtures are already on a mounted fs by the time the
  tests run — no early-mount and no dedicated self-test partition required.
- *Mechanism: plain disk files on the existing rootfs, not a compressed archive
  or a separate image.* Stage the 49 distinct `services/fastpy-*/*.elf` (~3.3 MiB
  each) flat into `/tests/<name>.elf` on `rootfs.ext4` at rootfs-build time
  (`scripts/create-ext4-rootfs.sh`, bump `IMG_SIZE` 48M→256M). Load each at
  runtime with `load_test_elf(name) -> Option<Vec<u8>>`, a thin wrapper over the
  existing `crate::fs::Vfs::read_file("/mnt/tests/<name>.elf")` primitive already
  used throughout the Path-Z tests; `None` (file/disk absent) makes the affected
  self-test cleanly *self-skip* rather than fail, so a lean production build (no
  fixture disk) still boots green.
- *Why plain files over an archive/dedicated image:* zero new machinery (reuses
  the mounted ext4 + `Vfs::read_file`), the fixtures are individually inspectable,
  and it mirrors exactly how the glibc Path-Z fixtures are already staged. A
  compressed archive would need an in-kernel decompressor on the test path; a
  dedicated fastpy-bin image would add a second disk + mount for no benefit at
  this stage. When A (promote to real `/bin`) is later taken up, the staging
  simply retargets `/bin` and the self-tests spawn the installed binaries — the
  loader indirection introduced here is the seam that makes that trivial.
- *Scope correction:* only the **61 fastpy sites in `spawn.rs`** are the 164 MiB
  bloat. The 3 `main.rs` + 6 `container.rs` `include_bytes!` sites are tiny
  hand-written no_std services (`init`/`hello`/`ticker`, tens of KB) and stay
  embedded.

**Implemented & verified (2026-07-23).** Done exactly as designed above. 54
`static FASTPY_*_ELF` decls in `spawn.rs` converted to `load_test_elf()` disk
loads (self-skip on absence); all 49 fastpy `*.elf` staged into `/tests` by
`create-ext4-rootfs.sh` (image 48M→256M). **Debug kernel binary 361.7 MiB →
181.8 MiB (−180 MiB / ~50 %).** All 55 fastpy ring-3 self-tests pass loading
from `/mnt/tests`, green boot. One surprise: the sparse fastpy ELFs were the
*first* sparse files ever read through the ext4 extent path, which exposed a
latent extent-reader bug (holes collapsed → data shifted) — fixed as
BUG-EXT4-SPARSE-READ (`kernel/src/fs/ext4/driver.rs`, `block_copy_placement`).
The loader indirection (`load_test_elf`) is now the seam for the follow-on
option A (promote fastpy coreutils to real `/bin`).

**Follow-on — first /bin promotion (2026-07-23).** Option A started: `fastpy-cat`
is the first fastpy binary promoted from a `/tests` self-test fixture to a real
installed command. `create-ext4-rootfs.sh` now maps a curated
`PROMOTED[fastpy-cat]=cat` set — the binary is installed at `/bin/cat` (and
*not* also under `/tests`, so no ~3.5 MiB duplication). The kernel gained a
`resolve_command(name, PATH)` helper (`spawn.rs`) that searches `COMMAND_PATH`
(`["/mnt/bin"]`, where the rootfs `/bin` is mounted) — the resolve+load step a
shell/`init` performs before `exec`. `self_test_fastpy_slateos_cat` now resolves
`cat` **by command name** through that PATH and spawns it with `argv[0]="cat"`,
so the test exercises the real installed-command execution path rather than a
fixture load. Additive and reversible: no existing Rust coreutil is touched, and
the promotion is a per-command opt-in via the `PROMOTED` map. The file-reading
coreutils `wc`, `head`, and `tail` were promoted next the same way (their
self-tests now `resolve_command(...)` and spawn `argv[0]="wc"|"head"|"tail"`), so
`/bin` holds four fastpy commands and `/tests` dropped to 45 fixtures. A second
batch — `grep`, `sort`, `uniq`, `ls` — followed the same way, bringing `/bin` to
eight promoted fastpy commands and `/tests` to 41 fixtures. A third batch — the
filesystem-mutating coreutils `rm`, `mv`, `mkdir`, `rmdir`, `chmod`, `chown` —
was promoted the same way, bringing `/bin` to 14 promoted fastpy commands and
`/tests` to 35 fixtures. (The remaining `fastpy-*` self-tests are OS-API probes,
not user-facing commands, so they stay under `/tests`.) Whether these minimal
utilities should ever *replace* the mature Rust coreutils in a shipping `/bin` is
deferred to the operator as `open-questions.md` Q35 (recommendation: keep the
promotions additive; never swap a Rust coreutil silently).

## §88 — Boot-window liveness watchdog: derive the deadline from the harness timeout, and gate the progress detectors on serial silence

**Date:** 2026-07-27
**Decided by:** Claude (autonomous)

**Context.** The boot-window liveness watchdog
(`kernel/src/sched/mod.rs`) had three false-fire modes and reported a hang on
every healthy boot (known-issues BUG-LIVENESS-DEADLINE-FALSE-FIRE). Two design
choices in the fix have genuine tradeoffs.

### Decision 1 — the wall-clock boot deadline is derived from `boot-test.sh`'s `--timeout`, not hardcoded

The deadline must sit above the slowest healthy armed-to-`BOOT_OK` duration and
below the harness's kill. Those are two moving numbers, and the kernel-side
constant had no way to learn about either.

* **Alternative A — re-tune the constant** (raise 200 s → ~400 s). Zero moving
  parts, no cmdline plumbing, works on real hardware unchanged. But it is a
  treadmill: `scripts/boot-test.sh` documents in its own `TIMEOUT` comment that
  the Path-Z self-test battery keeps growing, so the constant will drift out of
  sync again — as it already did once, silently, for weeks.
* **Alternative B — derive it (chosen).** The harness passes
  `sched.boot_deadline_ms=$((TIMEOUT * 1000))` on the Limine cmdline;
  `liveness_arm()` subtracts a 45 s dump margin and the monotonic arm timestamp
  to convert the QEMU-relative budget into the armed-relative units the backstop
  measures in. Cost: a kernel↔harness coupling and a new cmdline key, plus a
  fallback path (`LIVENESS_BOOT_DEADLINE_DEFAULT_NS`, 900 s) for boots with no
  such cmdline. Benefit: the invariant "dump before the harness gives up, never
  before a boot the harness would have tolerated" becomes structural rather than
  a tuned guess, and raising `--timeout` moves both in lockstep.

Chosen B. The coupling is real but it is the *correct* coupling — the two
numbers are answering the same question, and a constant that has already gone
stale once will go stale again.

### Decision 2 — both progress detectors require serial silence

`USEFUL_WORK_TICKS` (ticks preempting ring-3 code or a CPU with a queued task)
is not a valid "boot is advancing" signal: `kmain` and a *starting* ring-3
process both run long stretches of kernel-side work that the counter cannot see.

* **Alternative A — fix the tick charging** so kernel-side boot work counts
  (e.g. a per-CPU "parked in the idle loop" flag replacing the
  `local_has_real_work` proxy). Most principled, but there are several
  independent idle paths (`idle::idle_once`, the scheduler's HLT fallback loop,
  the AP idle loop in `smp.rs`) plus healthy busy-waits (`keyboard`, `virtio`),
  so "is this CPU idle?" has no single choke point today and every missed site
  is a new false positive.
* **Alternative B — gate on serial output (chosen).** `serial::_print` is a
  single choke point; one relaxed increment there is free next to the ~87 µs/char
  the UART costs. This kernel narrates its boot continuously, so a silent
  interval means execution really stopped and a chatty one means it did not —
  and it is exactly the criterion `boot-test.sh --stall-secs` already applies
  from the outside, so the inside and outside views now agree.

Chosen B. **Cost accepted:** a livelock that keeps printing escapes the two 15 s
detectors. That is tolerable because the wall-clock boot deadline catches every
hang mode by construction, and (per decision 1) that deadline is now
trustworthy. Alternative A remains the better long-term answer if a single
authoritative per-CPU idle flag ever exists; the two are complementary, not
exclusive.

## §89 — `jobs -x` substitutes a job's *pid*, where bash substitutes its process group

**Date:** 2026-07-27
**Decided by:** Claude (autonomous)

**Decision.** osh's `jobs -x cmd …` replaces each whole-word job spec among the
operands with the job's **process id**. Reference bash replaces it with the
job's **process group id** (`job->pgrp`).

**Why they differ.** bash's substitution is a pgid because bash puts each job in
its own process group when job control is on — so the pgid *is* the job, and
`jobs -x kill -9 %1` reaches every process in the pipeline. osh has no terminal
job control and no process groups at all (TD-OILS13); a job is one process (or
one in-process thread), so there is no group to name.

* **Alternative A — answer with the pid (chosen).** `%1` names something that
  really is the job, so `jobs -x kill %1` does what it says. Cost: on a
  multi-stage background pipeline it names the stage osh records as the job,
  not the whole pipeline — which is the same limitation osh's `kill %1` already
  has, so `-x` adds no new gap.
* **Alternative B — mirror bash literally by answering with the shell's own
  process id** (bash without job control puts every child in the shell's group,
  so that is the number it prints). This would match the reference shell's
  output byte-for-byte under the differential harness, at the price of making
  the feature useless and actively dangerous: `jobs -x kill %1` would signal the
  shell itself.

Chosen A. Fidelity to what the operand *means* beats fidelity to a number that
reference bash only produces because it is not doing job control in the
harness. The differential corpus (`tests/corpus/jobs-execute.sh`) therefore
covers everything except the substituted value — which words are replaced at
all, the diagnostics, the option rules and the exit status — and the value
itself is pinned by a unit test instead.

## §90 — A background job's signal disposition is modelled on bash's *intent*, not on what the reference shell happens to print

**Date:** 2026-07-27
**Decided by:** Claude (autonomous)

**Decision.** osh gives a job started with `&` the dispositions bash's
`setup_async_signals` gives an asynchronous child: `SIGINT` and `SIGQUIT` are
ignored (job control is off, so an interrupt meant for the foreground must not
reach into the background), and every other signal keeps its default — so
`CHLD`, `URG`, `WINCH` and `CONT` are no-ops too, and only a signal that would
really terminate kills the job. Before this, osh killed a job for *any* nonzero
signal. A consequence worth stating: **no background job can ever be listed as
`Interrupt` or `Quit`.**

**Why this needed deciding.** The reference bash appears to contradict this. Run
directly, `sleep 1 & kill -INT %1` reports `Interrupt` — and the corpus case
`jobs-listing.sh` had been asserting exactly that. Two hypotheses were
investigated and *both* were disproven by probing:

* *"Cygwin drops `SIG_IGN` for `SIGINT` across `exec`."* Disproven directly:
  `(trap "" INT; exec sleep 1) & kill -INT %1` leaves the job running, so the
  ignore does survive `exec` on this host.
* *"The job's leader decides — a wrapped job (`( )`, `{ }`, a pipeline) has a
  shell in front of it that ignores the signal, a bare command does not."* This
  fitted a 16-case shape×signal matrix exactly — and was still wrong. Re-running
  the same file showed bash answering `Interrupt` for the `( )` shape on one run
  and `Running` on the next.

The actual explanation is a **race**: bash establishes an asynchronous child's
dispositions *after* forking it, so a signal delivered immediately can land
while the child still holds the disposition it was forked with. Given 0.05 s to
settle, the reference shell is unambiguous — 30 trials: `INT` 10/10 running,
`QUIT` 10/10 running, `TERM` 10/10 terminated.

* **Alternative A — implement the settled behaviour (chosen).** Costs the
  corpus its only `Interrupt` row, since that state is now unreachable.
* **Alternative B — keep matching what the reference prints when signalled
  immediately.** Would have kept the row, at the price of encoding a race as if
  it were a rule — and the row would have been a latent flake all along, since
  bash decides it by timing.

Chosen A. **The general lesson, which cost three wrong models here: when the
reference shell is used as an oracle, a disagreement that is *reproducible* is
not thereby *deterministic*.** Before modelling any behaviour that involves a
freshly forked child, re-run the probe several times and give the child time to
settle; a single confident-looking answer can be one side of a race. Every case
in `tests/corpus/kill-dispositions.sh` therefore sleeps before signalling, and
says in a comment why.

---

## §91 — Child-thread ELF TLS: one combined stack+TLS mapping, initialised by the *creating* thread

**Date:** 2026-07-30
**Decided by:** Claude (autonomous)

**Context.** §82 gave the *main* thread a variant-II TLS block but explicitly
deferred child threads, so `pthread_create`'d threads started with `fs_base ==
0`: in a C program the very first instruction of the start routine's prologue
(a stack-protector canary load from `%fs:0x28`) faulted. Fixing it forces two
choices — **where the child's TLS block is allocated**, and **who initialises
and installs it**.

**Decision.** **One mapping, initialised by the parent.** `pthread_create`
mmaps `DEFAULT_THREAD_STACK_SIZE + PT_TLS-reserve` bytes as a single anonymous
region, puts the thread's stack at the bottom and its TLS block + TCB at the
top (the musl layout), copies the `.tdata` init image and writes the TCB
self-pointer *before* the child exists, and passes the resulting thread
pointer to the child as a third word pushed on its stack. The child's only
TLS-related action is `SYS_SET_FS_BASE(tp)`, as the literal first thing
`__pthread_thread_start` does. `ThreadSlot` grew a `map_size` field so the
existing join/detach reclaim protocol unmaps the whole region.

**Alternatives considered.**

- *Separate TLS mapping owned by the child.* Rejected: it creates a second
  lifetime that must be freed in exactly the same three places the stack is,
  and — worse — there is no safe ordering for a self-unmapping thread. If the
  child frees its TLS before `SYS_THREAD_EXIT`, any instruction between the
  two syscalls that touches `%fs` (e.g. a canary check in the epilogue of the
  very function doing the unmapping) faults; if it frees it after, it can't.
- *Child allocates its own TLS.* Rejected: a child that discovers it has no
  memory for TLS has nobody to report to — `pthread_create` has already
  returned success — and cannot safely run the start routine either. Doing it
  in the parent is what makes `EAGAIN` reportable.
- *Kernel-allocated TLS (a `SYS_THREAD_CREATE` flag, `CLONE_SETTLS`-style).*
  Rejected: it would put ELF layout knowledge (`PT_TLS`, variant II, the
  `.tdata` image) inside the microkernel, contradicting §82's "kernel resets
  `fs_base` to 0, userspace sets it up" split, for no gain — the primitive
  already exists.

**Rationale.** *Pro:* exactly one owner and one `munmap`; no new syscall; no
window in which a thread has freed its TLS but still runs `%fs`-touching code;
`pthread_create` can report allocation failure properly; the parent does the
work once, so the child's own startup is a single syscall. *Con:* the stack and
TLS can no longer be sized or reclaimed independently, and `pthread_getattr_np`
must report the *usable stack* (`stack_size`) while reclaim uses the *whole
region* (`map_size`) — two sizes that must not be confused (they are separate
`ThreadSlot` fields for exactly this reason).

**Latent bug this uncovered.** Both the old `crt.rs` code and the first cut of
the shared module computed the block offset as `round_up(p_memsz, max(p_align,
16))`. The linker assigns `__thread` offsets relative to `TP -
round_up(p_memsz, p_align)` using the segment's **own** `p_align`, so any
binary with `p_align < 16` had its whole TLS block placed at the wrong distance
below TP — the init image was written to one address and every access read
another. It had never shown because fastpy binaries happen to have `p_align ==
0x10`; the new C fixture has `p_align == 4` and caught it immediately (exit
21 = "the child's `.tdata` copy is missing the initialiser image"). The
16-byte TCB alignment is now a separate `TlsImage::tp_align()`, and because the
block start may consequently be as weakly aligned as `p_align`,
`pthread_create` rounds the child's `stack_top` down to 16 to preserve the SysV
entry-RSP alignment.

**Where it lives.** `posix/src/tls.rs` (new: `TlsImage`, `image()`,
`init_block()`, `install()`, `setup_main_thread()`), `posix/src/crt.rs` (now
calls `tls::setup_main_thread()`; its own copy deleted), `posix/src/pthread.rs`
(`pthread_create`, `_pthread_trampoline`, `__pthread_thread_start`,
`ThreadSlot::map_size`, the three reclaim sites). Validated on-target by
`services/ctest-tls-thread/` (plain C, `-fstack-protector-all`) +
`kernel/src/proc/spawn.rs::self_test_ctls_thread`, staged into `/tests` by a
new `services/ctest-*` loop in `scripts/create-ext4-rootfs.sh`. Resolves
D-NATIVE-CHILD-THREAD-TLS in known-issues.md; reading the reclaim protocol to
decide where the TLS mapping should be freed also exposed
D-PTHREAD-SLOT-PUBLISH-RACE (logged, not fixed here).

**How to reverse.** To go back to independent stack and TLS mappings, split
`map_size` off `pthread_create`'s single `mmap` and free the TLS region in the
*joiner*/detach paths only (never in the exiting thread) — the ordering hazard
above is why that is the only safe split. To move TLS setup into the kernel,
add a `tls` argument to `SYS_THREAD_CREATE` and have `spawn_user` write
`Task::fs_base`; `tls::image()` would then have to be reimplemented against the
kernel's ELF loader.

## §92 — Per-thread libc storage lives in a fixed slot above the thread pointer, not in `#[thread_local]` or a malloc'd block

**Date:** 2026-07-30
**Decided by:** Claude (autonomous)

**Context.** A whole family of POSIX functions returns a pointer to storage the
standard says is *per-thread*: `errno` (via `__errno_location`), `gmtime`/
`localtime`'s `struct tm`, `asctime`/`ctime`'s text buffer, `inet_ntoa`'s
buffer, and the four netdb result structs (`gethostbyname`, `gethostbyaddr`,
`getservby*`, `getprotoby*`), plus the resolver's `h_errno`. Ours were 16
process-wide `static mut`s. That is
a genuine product thread-safety gap on the OS target, and on the host it made
`cargo test -p posix` flaky under the parallel harness (TD-POSIX-TEST-PARALLEL).
Fixing it means picking *where per-thread libc storage lives* — a decision that
binds the whole libc, not just these functions, since everything added later
(`strtok`'s cursor, locale state, …) will go in the same place.

**Decision.** One `#[repr(C)] struct PerThread` (`posix/src/perthread.rs`)
holding every such field, parked at a **fixed offset above the thread
pointer**: `TP + TCB_SIZE`. `TlsImage::reserve()` grew by
`perthread::BLOCK_SIZE`, so the block rides inside the mapping the thread
already owns — the same single mapping §91 established for child threads.
`perthread::current()` is one `mov {}, fs:[0]` plus a constant add. `errno` is
deliberately the **first** field, so `current().cast::<i32>()` *is* the errno
pointer and `__errno_location` — on the hot path of every failing syscall —
compiles to the same two instructions with no field arithmetic.

**Alternatives considered.**

- *`#[thread_local]` statics.* The obvious answer, and rejected only for a
  toolchain reason: the attribute is nightly-only (E0658), and the sysroot
  `libc.a` builds on **stable** rustc targeting `x86_64-unknown-none`. Moving
  the sysroot to nightly for one attribute would split the toolchain and put
  every future sysroot build at the mercy of nightly churn. Worth revisiting if
  the attribute stabilises — it is strictly nicer, since the linker would then
  place and size the storage instead of us.
- *A pointer slot in the TCB pointing at a lazily-`malloc`'d block.* Rejected:
  it introduces an allocation on a path that must not fail or re-enter the
  allocator. `strerror` returning `NULL` because the per-thread block couldn't
  be allocated is not a behaviour any caller handles, and calling `malloc` from
  inside `__errno_location` risks re-entering an allocator that is itself
  mid-`errno`-write. It would also need a free-on-exit hook in all three thread
  reclaim paths.
- *A fixed-size array indexed by thread id.* Rejected outright: caps the thread
  count, and the index lookup is a shared-cache-line read on the hottest path
  in the libc.

**Rationale.** *Pro:* no allocation (hence no failure path and no allocator
re-entrancy), no teardown (`pthread_join`/`pthread_detach` already unmap the
whole thread region), and zero-initialisation for free — the mapping is fresh
anonymous memory and every field's correct initial state happens to be all-zero.
Lookup is two instructions. *Con:* the block is sized at compile time and paid
for by every thread whether or not it ever calls `gmtime`; and "all-zero is a
valid `PerThread`" is now a load-bearing invariant that isn't visible at the
field declarations. The latter is guarded by a test that constructs the struct
via `core::mem::zeroed()`, which makes rustc's `invalid_value` lint a
*compile-time* tripwire if anyone adds a field (a `NonNull`, a reference, an
enum without a zero variant) whose zero bit pattern is invalid. The former is
bounded by a test asserting `BLOCK_SIZE <= 2048`.

**The no-thread-pointer case.** Reading `%fs:0` faults while `fs_base` is still
0, which is the state of any bare-metal `services/` binary that skips the crt.
`tls.rs` therefore keeps a process-global `TP_INSTALLED: AtomicBool` (Release on
a successful `SYS_SET_FS_BASE`, Acquire in `thread_pointer()`), and
`current()` hands back a shared `static mut FALLBACK` when it is clear. A single
*process*-global flag is sound for a *per-thread* question only because the
ordering is fixed: `__libc_start_main` installs the main thread's TP before
anything else runs, and `pthread_create` is unreachable before that, so no
thread can ever observe the flag set without having installed its own TP.
(`asm!` deliberately omits `pure` so the read can't be CSE'd across an
`install()` call.)

**Where it lives.** `posix/src/perthread.rs` (new), `posix/src/tls.rs`
(`reserve()`, `TP_INSTALLED`, `thread_pointer()`), `posix/src/errno.rs`
(`set_errno`/`get_errno`/`__errno_location` rewritten; the `#[cfg(test)]`
`AtomicI32` split deleted), `posix/src/time.rs` (`TM_RESULT`, `ASCTIME_BUF`
deleted), `posix/src/socket.rs` (13 statics replaced by `HostentBuf`/
`ServentBuf`/`ProtoentBuf`, which also deduplicated ~35 lines of copy-pasted
pointer-web assembly between `gethostbyname` and `gethostbyaddr`; and `h_errno`,
whose exported *data* symbol was deleted outright — see BUG-POSIX-H-ERRNO).
Host build
uses a `thread_local!` behind the identical `current()` API. Validated by 10
consecutive clean parallel `cargo test -p posix` runs (20013 tests each) and,
on target, by extending `services/ctest-tls-thread` to assert that each pthread
gets its own `__errno_location()`, distinct from the parent's and starting at 0.
Resolves TD-POSIX-TEST-PARALLEL.

**How to reverse.** If `#[thread_local]` stabilises for our sysroot toolchain,
delete `perthread.rs`, declare each field as its own `#[thread_local] static
mut`, and shrink `TlsImage::reserve()` back (the linker will have folded the
storage into `PT_TLS` itself). Callers change only in that
`(*perthread::current()).x` becomes `X`; `__errno_location` would return
`&raw mut ERRNO`.

## §93 — osh byte strings: `Vec<u8>` + the `bstr` crate, not a hand-rolled `ShellStr`

**Date:** 2026-07-30
**Decided by:** Claude (autonomous)

**The problem.** osh models every shell string as a Rust `String`, so it cannot
carry a byte a POSIX filesystem permits but UTF-8 forbids. Our own filesystem
allows every byte except `/` and NUL, so `aÿb` is a legal filename that the
shell cannot name — and the failure mode is not a diagnostic, it is a *wrong
file*: `from_utf8_lossy` turns the byte into U+FFFD and the shell then opens,
`stat`s or deletes something else. This is CLAUDE.md rule 7 violated at the
root of the userspace. Fixing it means changing osh's string type
(TD-OILS-BYTE-STRINGS).

**The decision.** The shell string type becomes plain `Vec<u8>` / `&[u8]`, with
the `bstr` crate (`default-features = false`, `features = ["std"]`) supplying
the str-like methods over it, plus a small in-tree `bytes.rs` for the handful
of operations `bstr` deliberately does not have.

**Alternative considered: a hand-rolled `ShellStr(Vec<u8>)` newtype.** *Pro:* no
external dependency in a core OS binary; we control the whole API surface and
can name methods after shell semantics rather than after `str`. *Con:* it is
several hundred lines of substring search, splitting, trimming, case folding
and UTF-8 chunk iteration that must be written, tested and maintained — and
`memchr`-quality substring search is genuinely hard to get both correct and
fast. Reimplementing it would be the "correct-but-naive code" the design spec
warns about, in a hot path (every `${v#pat}`, every field split).

**Alternative considered: keep `String` and smuggle bytes through a private
encoding** (surrogate escapes as WTF-8, or a private-use-area mapping, as
Python's `surrogateescape` does). *Pro:* a very small diff — the type never
changes. *Con:* it is not total (a value can already legitimately contain the
escape characters, so encode/decode is ambiguous), every syscall boundary needs
an encode/decode pass that is easy to forget, and `${#v}` / `${v:o:l}` would
silently count the escapes. This is precisely the kind of quick fix CLAUDE.md
forbids: it makes the bug rarer instead of absent.

**Why `bstr` wins.** Its real dependency tree is one crate deep — `oils -> bstr
-> memchr` — with no proc macros and no `syn`; it builds clean for
`x86_64-slateos` (verified); it is the de-facto standard for this exact problem
in Rust (ripgrep, gitoxide); and its API is deliberately `str`-shaped, so the
conversion of 60k lines of osh is close to mechanical rather than a redesign.

**What stays a `String`.** Variable names, function names, option names,
`set -o` flag names, format specifiers, and anything else the shell grammar
already restricts to a portable-character-set identifier. Those cannot be
non-UTF-8 without the *parser* having accepted a non-UTF-8 name, and keeping
them as `String` preserves `HashMap<String, _>` lookups with `&str` keys. What
becomes bytes is **values, literal word text, captured command output,
positional parameters, environment strings, paths, and every diagnostic that
interpolates one of those**.

**What `bytes.rs` has to add.** Three things `bstr` cannot give us without its
heavyweight `unicode` feature (which pulls `regex-automata`):

* Unicode case mapping for `${v^^}` / `${v,,}` / `${v@U}` — `bstr`'s
  `to_lowercase`/`to_uppercase` are ASCII-only without that feature, so we walk
  `utf8_chunks()` and case-map the valid runs while passing invalid bytes
  through unchanged.
* bash's character *counting* rule for `${#v}` and `${v:off:len}`: an invalid
  byte counts as exactly one character. `bstr`'s `char_indices()` already
  yields `(i, i+1, U+FFFD)` for one, which is the rule, so this is a thin
  wrapper rather than a reimplementation.
* `bfmt!`, a concatenating byte-string builder, because `format!` has no
  byte-string counterpart and `Display for BStr` is *lossy* — using it to build
  a shell value would reintroduce the very corruption being fixed. A macro that
  concatenates rather than parses a format string keeps that door shut by
  construction: there is no `{}` that can silently accept a `String`-formatted
  byte string. (The external `format-bytes` crate was rejected: it is a proc
  macro built on the deprecated `proc-macro-hack`, and buys only syntax.)

**How to reverse.** The change is a type change, not an architectural one: if
`bstr` ever became a problem, `bytes.rs` is the seam — vendor the dozen methods
osh actually uses into it and drop the dependency, with no change at the call
sites.

## §94 — osh `$SRANDOM`: read `/dev/urandom` per expansion, with a documented non-cryptographic fallback

**Date:** 2026-07-31
**Decided by:** Claude (autonomous)

**The problem.** bash 5.1 added `$SRANDOM`: 32 bits drawn from the *system
entropy source* rather than from bash's own `$RANDOM` LCG, with no seed (so
assignments to it are documented as having no effect). Scripts reach for it
precisely when `$RANDOM` is too weak — temp-file names, tokens, salts. osh had
no `$SRANDOM` at all, so `$SRANDOM` expanded to the empty string.

Implementing it forces a choice about what to do on a build where there is no
entropy device — notably the *host* (Windows) build of osh, which is how the
shell is developed and tested.

**Options.**

* **A — entropy device only; expand to empty (or fail) without one.** Never
  hands out a weak number. But it makes `$SRANDOM` silently useless on the host
  build, and "empty" is the worst possible failure for `salt=$SRANDOM`: under
  `set -u` it aborts, and without it the script proceeds with an empty salt.
* **B — pull in the `getrandom` crate.** Real per-OS backends (BCrypt on
  Windows, the `getrandom` syscall on Linux). But it has no backend for the
  custom `x86_64-slateos` target, so it would need the custom-backend hook
  wired up anyway — a dependency plus a shim, to serve a path that on the real
  target is just `/dev/urandom`.
* **C (chosen) — `/dev/urandom` per expansion, falling back to a SplitMix64
  stream seeded from clock+pid.** On SlateOS the device is the kernel CSPRNG
  behind `getrandom`, which is exactly bash's source; the fallback only ever
  runs where there is no device.

**Why C.** It is what bash itself does — `variables.c` falls back to bash's own
generator when no entropy source is available — and it is what the rest of this
userspace already does (`mktemp`, `passwd`, `useradm` all read `/dev/urandom`
with a PRNG fallback). Adding a dependency (B) to serve a case the target does
not have is cost without benefit, and A trades a weak number for an *empty* one,
which is not safer.

**What keeps the fallback honest.** It is a separate stream from `$RANDOM`, so
`RANDOM=1` cannot make `$SRANDOM` reproducible; it is seeded from clock+pid, so
two shells started in the same microsecond diverge; a subshell perturbs its
state *and* advances the parent's, so neither a subshell and its parent nor two
subshells started in the same instant repeat each other's numbers; and the
device is probed once — after the first failure the flag is cleared, so a host
build does not pay a failed `open` per expansion. The fallback's
non-cryptographic status is stated at the function, not buried.

**How to reverse.** `Shell::next_srandom` is the only reader; swapping in a
different source (a crate, a syscall wrapper, a kernel-provided handle) is a
change to that one function and to `read_entropy_u32`.

## §95 — osh `compgen -A hostname`: invalidate the kept list by *value*, not by an assignment hook

**Date:** 2026-07-31
**Decided by:** Claude (autonomous)

**The problem.** bash's `hostname` completion action offers the names in
`$HOSTFILE` (or `/etc/hosts` when it is unset), and it *keeps* the list rather
than re-reading the file per completion. The list's lifetime is observable, and
the two ways it changes are not symmetric:

* naming a different `HOSTFILE` **adds** that file's names to the ones already
  offered, and
* *unsetting* `HOSTFILE` throws the list away, so the next file named starts
  from nothing.

Both were measured against bash 5.2 and both are in bash's manual. But bash
re-reads whenever `HOSTFILE` is **assigned**, not when its value changes — so
`HOSTFILE=f; compgen -A hostname; HOSTFILE=f; compgen -A hostname` offers every
name in `f` twice. That third behaviour is not in the manual; it falls out of
bash hanging the invalidation off `sv_hostfile`, which its assignment path calls
unconditionally.

**Options.**

* **A — a general variable-assignment hook**, bash's
  `stupidly_hack_special_variables`. Reproduces all three behaviours exactly.
  But osh has no such hook today, and adding one means finding *every* path that
  can write a variable — `HOSTFILE=x cmd` prefixes, `read`, `declare`,
  `printf -v`, `local` going out of scope, environment import — and firing a
  side effect from each, in order to emit duplicate completions.
* **B (chosen) — remember the `HOSTFILE` value the list was read from**
  (`hostname_source: Option<Option<Str>>`, the outer `None` meaning "nothing
  read yet", the inner one "`HOSTFILE` unset"), and re-read only when the value
  differs — plus a single hook in `Shell::unbind_var` for the clearing half.
* **C — re-read the file on every completion, no list at all.** Simplest, but
  wrong for both of the *documented* behaviours: the additive one would be lost
  outright, and a `HOSTFILE` naming an unreadable file would stop answering
  where bash keeps answering from the list it already has.

**Why B.** It matches the manual exactly — "the next time hostname completion is
attempted **after the value is changed**" — and it matches bash byte-for-byte on
everything except the redundant-assignment duplicate, which is the one part that
is an implementation artefact rather than a described behaviour. The unset half
*is* behaviour, so it gets a real hook, but `unbind_var` is a single, obvious
choke point (it is exactly where bash's `unbind_variable` reaches
`clear_hostname_list`) rather than a new cross-cutting mechanism. This follows
the same line already drawn for bash's doubled circular-nameref warning
(TD-OILS-NAMEREF-WARNING-COUNT) and its self-inconsistent post-`wait` job table:
copy the behaviour, not the internals that produce it.

**What is given up.** Duplicate candidates after a redundant assignment, logged
as TD-OILS-COMPGEN-HOSTFILE-REASSIGN and deliberately kept out of the corpus.

**How to reverse.** If osh ever grows a general assignment hook for another
reason, `Shell::compgen_hostnames` becomes the trivial "if not initialised,
read" that bash has, and the value comparison drops out. The cache lives in two
fields and one function.

---

## §96 — `/etc/services` stays the IANA database; SlateOS's own service config moves to `/etc/startup.conf` and `/etc/service.d/`

**Date:** 2026-07-31
**Decided by:** Claude (autonomous)

**The problem.** Three subsystems had independently claimed the path
`/etc/services` (BUG-ETC-SERVICES-THREE-WAY-COLLISION in known-issues.md):
`getent`/`posix` as the classic IANA port/protocol *file*, `init`/`kernel` as a
*file* holding our line-per-ELF startup list, and the `service` CLI as a
*directory* of YAML unit definitions. Two of the three had to move. Which two
is a filesystem-layout choice, and layout is user-visible and awkward to change
later, so it is worth recording rather than just patching.

**What was decided.**

* `/etc/services` is the IANA port/protocol database, and nothing else.
* Init's startup list becomes `/etc/startup.conf`.
* The service manager's unit definitions become `/etc/service.d/<name>.service`,
  with enable-symlinks in `/etc/service.d/enabled/<name>`. Runtime state stays
  where it already was, `/run/services/<name>/`.

**Why this direction.** `/etc/services` is not a name SlateOS gets to pick. It
is specified — POSIX's `_PATH_SERVICES`, `getservbyname`/`getservbyport`, and
`getent services` all resolve it, and `posix/src/paths.rs` already hard-codes
it because the C API contract says so. Any program ported to SlateOS that calls
`getservbyname` will read that path whether or not we agree. So the standard
consumer is the one that *cannot* move, and the two SlateOS-specific ones are
the ones that can. Choosing the other way would mean either shipping a
non-conforming libc or teaching the porting layer to redirect a documented path,
both of which cost far more than a rename.

**The alternative considered:** put the IANA database somewhere else (say
`/etc/inet/services`) and keep `/etc/services` for the service manager, on the
grounds that a *service manager* is what a desktop user is more likely to mean
by "services", and the port database is a legacy table almost nobody edits.
Rejected: the port database's whole value is that ported software finds it
without being told, and "almost nobody edits it" is an argument about *editing*,
not about *reading* — it is read on every `getservbyname`. A path we invent for
it would simply be wrong for every ported program.

**Why two new names rather than one.** Init's list and the `service` CLI's units
describe overlapping things, and it is tempting to merge them into one directory.
They are not merged here, because they are genuinely different artifacts today:
init is `no_std`, parses a flat line format into a fixed 2 KiB buffer, and runs
before any YAML parser exists; the CLI reads YAML from a full `std` userspace.
Unifying them is a real refactor of the service manager, not a rename, and doing
it under cover of a bug fix would have hidden it. `design.txt` line 653 in fact
anticipates two mechanisms ("our service manager is one way, and then we'll have
some simple list of apps … to load"), so two paths is not obviously wrong even
long-term.

**What is given up.** Nothing existed on disk to migrate — the kernel writes the
bootstrap file fresh on every boot and no image ships a populated
`/etc/services` — so there is no compatibility shim and no upgrade path to
maintain. If that stops being true (once SlateOS has persistent installs), a
rename like this one will need one.

**How to reverse.** Three string constants and their comments:
`services/init/src/main.rs` + `kernel/src/main.rs` for `/etc/startup.conf`, and
`userspace/service/src/main.rs` for `/etc/service.d`. If the service manager is
later unified, both should collapse into whichever path the unified manager
picks — but not back onto `/etc/services`.

## §97 — osh gives a coproc a command of grace before disposing of it

**Date:** 2026-08-01 (revised the same day — see "Revision" at the end)
**Decided by:** Claude (autonomous)

bash disposes of a coproc when it reaps the body: both parent-side descriptors
are closed and `NAME`/`NAME_PID` are unset. That is now osh's behaviour too
(`Shell::dispose_reaped_coproc`, hung off the job sweep in `poll_jobs`) — with
one deliberate deviation: neither the command that started a coproc nor the
command in which the shell first notices the body is over may dispose of it,
however finished it already looks.

**The problem.** bash's coproc body is a forked child. A fork costs orders of
magnitude more than the builtin the parent runs next, so the child cannot be
reaped before the parent has moved past the `coproc` statement — bash's window
is not guaranteed by anything in the code, but in practice it is hundreds of
fast builtins wide. osh's body is a thread on the same machine, and a body like
`{ exit 7; }` routinely finishes before the shell reaches the next command
boundary. Without a grace period, the commonest idiom of all —

```sh
coproc C { … }
echo feed >&"${C[1]}"
```

— works or does not work depending on how the two were scheduled. Measured: the
same script alternated between `waitC=7` and `wait: `': not a pid or valid job
spec` across runs, and two `interp.rs` unit tests failed intermittently for the
same reason.

**Alternatives considered.**

- *No grace (dispose at the first sweep that finds the body reaped).* The purest
  reading of bash's rule, and what was implemented first. Rejected: it makes
  osh's observable behaviour depend on thread scheduling, which is not a
  property a shell should have. Every test touching a coproc becomes flaky, and
  worse, so does every *script*.
- *Require N consecutive sweeps to find it reaped.* No N is right: the number of
  sweeps between one command and the next is not fixed (see the revision note
  below), so any N buys a probability rather than a guarantee, and a larger N
  also delays disposal for a long-running body whose end bash notices at once.
- *A wall-clock minimum lifetime.* Would model the fork cost most literally, but
  it makes behaviour depend on a timer, which is worse than depending on a
  scheduler, and it is untestable.
- *Spawn the body as a real process.* Would remove the divergence at the root,
  but osh's whole job model is threads (`JobBody::Thread`); changing that is a
  different, much larger decision, and on the SlateOS target processes are not
  cheaper anyway.

**The cost.** A script that puts *two* command boundaries between an
instantaneous body and a use of `NAME` still diverges from bash, which would
usually keep it alive there too. That is accepted: such a script is relying on
an unspecified race in bash as well, and the corpus and unit tests are written
to keep bodies alive with a trailing `read` rather than to bet on the window.

**How to reverse.** Delete `TrackedCoproc::born_item`,
`TrackedCoproc::seen_dead_item`, `Shell::item_seq` and the guards at the top of
`Shell::dispose_reaped_coproc`. The corpus cases
`coproc-is-a-job.sh`, `coproc-read-end-can-be-duped.sh` and
`coproc-is-disposed-of-when-it-is-reaped.sh` already avoid the window, so they
would keep passing; the `coproc_*` unit tests in `interp.rs` likewise.

**Revision — the grace is counted in commands, not in sweeps.** As first
implemented the grace was "the next reap sweep is a no-op" (a `TrackedCoproc::fresh`
flag). That was not enough, and the corpus caught it: `coproc-is-a-job.sh` failed
under the load of a full corpus run with an empty `$C_PID`, because **two**
sweeps happen between one top-level command and the next — `cleanup_dead_jobs`
at the input-unit boundary and `notify_signalled_jobs` at the top of the item —
so the second one could still dispose of a body that had already finished. One
sweep of grace made the race rare instead of removing it, which is the worst of
both: it passed in isolation and failed under load, and it made the timing of
disposal depend on how many sweeps a given construct happened to perform.

The grace is now anchored to a counter of *items* (`Shell::item_seq`, bumped at
the top of each item **after** that item's sweep, so the sweep still belongs to
the item just finished), and two fields on `TrackedCoproc`:

- `born_item` — the counter's value when the `coproc` ran. The item that started
  a coproc never disposes of it.
- `seen_dead_item` — the counter's value at the first sweep that found the body
  over. Disposal waits for a *strictly later* item than that.

The second field is the one that matters, and the first attempt at this revision
lacked it. A grace tied only to birth still lost, because a body is usually
released by the command *after* the `coproc` rather than by the `coproc` itself:

```sh
coproc D { read -r x; }
echo d >&"${D[1]}"     # the body reads, and ends, during this command
wait "$D_PID"          # …and must still be able to name it here
```

Here the body dies during the *second* item, so a birth-anchored grace has
already expired and the sweep at the top of the third can dispose before
`$D_PID` is expanded. That is exactly how `coproc-second-warns-about-the-first.sh`
failed. Requiring the death to be *noticed* in one item and acted upon in a
later one gives every such pair a whole command in which the body may end. bash
gets the same effect for free: between the write that releases a forked body and
the next command there is not enough time for it to exit *and* be reaped.

Both rules are arguments from the code rather than probabilities — no sweep
count, no timer. Verified with ten concurrent runs of `coproc-is-a-job.sh` and
`coproc-second-warns-about-the-first.sh` under four spinning CPU hogs, plus the
full corpus.

## §98 — a background job's death is not news for the first 20 ms

**Date:** 2026-08-01
**Decided by:** Claude (autonomous)

An operand-less `wait` has to sort the jobs into two piles: the ones it *waited
for*, which it has therefore reported, and the ones it merely found already over,
which it has not. The distinction is observable — bash purges the first pile and
spares the `$!` job from the second, so a later `jobs` announces it — and it
turns entirely on whether the shell had been *told* of a job's death before the
`wait` was reached.

In bash that is a race the shell always wins, because hearing about a background
job costs a fork, an exit and a `SIGCHLD`. In osh a background job is a thread
that can be over before the `&` has finished being executed. So osh holds the
news back: `Job::born_at` records when the job started, and `poll_jobs` refuses
to set `exit_seen` until `JOB_EXIT_NOTICE_GRACE` (20 ms) has passed. The reap
itself is not delayed — `jobs` and `wait` report the status the instant it
exists; what waits is only the shell's claim to have *heard*.

**Why time, when §97 rejected a timer for the coproc.** Because here the thing
being modelled is itself a wall-clock quantity, and that was measured rather than
assumed. On the reference bash, with `true &` followed by various filler before a
`wait`:

| filler between the `&` and the `wait` | job survives the `wait`? |
|---|---|
| nothing | no — the `wait` waited for it |
| one to eight `:` | no |
| `for ((i=0;i<20;i++)); do :; done` | no |
| `for ((i=0;i<200000;i++)); do :; done` (~0.7 s, no external command) | **yes** |
| `sleep 0.05` | **yes** |
| `sleep 0.4` | **yes** |

No count of commands separates rows 3 and 4 — they are the same command — and no
"did an external process run" rule does either, since row 4 spawns nothing. Only
elapsed time does. §97's rejection of a timer stands for the coproc, where a
deterministic rule expressed in items *does* fit every measurement; it does not
generalise to a case where the measurements rule such a thing out.

**Choosing 20 ms.** The bounds above put bash's own boundary on this host
somewhere between "a twenty-iteration builtin loop" (tens of microseconds) and
`sleep 0.05`. 20 ms is an order of magnitude above any plausible run of builtins
and comfortably below the shortest delay bash was seen to notice within. It is a
model of what a fork costs, not a measured constant, and it is host-dependent in
bash too.

**Alternatives considered.**

- *No grace.* What was there before, and it made `tests/corpus/jobs-wait.sh`
  fail once in a full corpus run and never in sixteen targeted attempts — the
  worst kind of failure. See known-issues TD-OILS-WAIT-NOARGS-LEAVES-A-JOB-FOR-WAIT-N.
- *A grace counted in items, as §97 uses for the coproc.* Tried first; it is
  ruled out by rows 2–4 of the table above, which need opposite answers from
  equal or smaller command counts.
- *Only the `drain_jobs` snapshot* (record what was known before the `wait`'s own
  catch-up poll, and count everything else as waited for). Necessary but not
  sufficient: the sweep at the top of the `wait`'s own item already runs before
  `drain_jobs` and would have learned of the death there. Both changes are in.
- *Spawn background jobs as real processes.* Removes the divergence at the root,
  and is the same much larger decision §97 declined.

**The cost.** A script that backgrounds an instantaneous job and then does 20 ms
of *builtin* work before an operand-less `wait` will have the job purged where
bash — whose fork may well have been reaped by then — would spare it. That is a
narrower window than the one it replaces, and it is a region where bash's own
answer is a race.

**How to reverse.** Delete `Job::born_at`, `JOB_EXIT_NOTICE_GRACE` and the guard
in `Shell::poll_jobs`, and drop the `known` snapshot at the top of
`Shell::drain_jobs`. `tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`
pins the behaviour; the `settle_job` helper in `interp.rs`'s tests sleeps past the
grace and would no longer need to.

## §99 — osh carries readline's compiled-in keymaps as a generated table, not a runtime library

**Date:** 2026-08-02
**Decided by:** Claude (autonomous)

osh has no line editor and never will have readline. Yet `bind`'s listings are
not optional decoration: a *non-interactive* bash answers every one of them,
prefixing only a `bind: warning: line editing not enabled` on stderr, and that
is exactly and permanently osh's condition — so the whole builtin is
corpus-testable, and "print nothing" is a visible wrong answer rather than an
honest silence. Answering `-p`, `-P`, `-v`, `-V` and `-q NAME` requires
readline's *default keymaps*: 174 function names, five keymaps totalling 928
bindings, and 46 variables.

**The decision.** Embed them as `const` tables in
`userspace/oils/src/bind_tables.rs`, captured from a reference bash by
`scripts/gen-oils-bind-tables.py`.

**Why generated rather than transcribed.** 928 `(key sequence, function)` pairs
transcribed by hand would be wrong somewhere and there would be no way to tell
where. A script makes the provenance a command instead of a claim: rerun it
against any bash and the diff is the answer. It also *checks* what a
transcription would assume — that `emacs-standard` really is `emacs` and that
`vi`, `vi-move` and `vi-command` really are one map — by capturing all of them
and refusing to proceed if they differ.

**`INPUTRC=/dev/null` is load-bearing, and was the trap.** A plain `bind -p` on
this host gives 493 lines, not 488, and `bind -s` gives 10, not 0, because
`/etc/inputrc` is loaded. A naive capture would have baked one machine's
configuration into osh as though it were readline's compiled-in default. The
generator exports it, the module doc records the two numbers, and the corpus
case exports it too — otherwise bash reads `/etc/inputrc` while osh does not and
the two diverge for a reason that has nothing to do with osh.

**Why its own module.** `interp.rs` is already large enough that adding ~1200
lines of table to it ran rustc out of memory under `--test`
(`STATUS_STACK_BUFFER_OVERRUN`). That is recorded in known-issues; the split is
the mitigation.

**Alternatives considered.**

- *Link real readline.* Correct by construction and would bring `-f` inputrc
  parsing and mutation for free. Rejected: it is a C dependency on a shell that
  is meant to build for a `no_std`-adjacent target, for a feature whose only
  consumer is a listing. The tables are 40 KB of `const`; the library is not.
- *Reimplement readline's initialisation.* The tables are what
  `rl_initialize()` builds from static C arrays; transcribing the *arrays*
  rather than their output is the same data with more code between it and the
  answer.
- *Print nothing and document the gap.* What osh did for a day. It is a wrong
  answer that the corpus can see, and the exclusion list was growing.

**The cost, and the part since closed.** As first written the tables were
`const`, so `-u`, `-r` and `-x` — which in bash mutate readline's live tables
even with no line editor — were accepted, reported exactly as bash reports them,
and then forgotten. That was closed on 2026-08-03: `bind_keys::Maps` holds an
owned, mutable copy on `Shell`, seeded from these constants on first use, and
`bind -f` and the startup `$INPUTRC` read fold an inputrc into it. See
known-issues TD-OILS-NO-BIND-BUILTIN. The constants remain the *seed*, which is
the whole reason they must be captured under `INPUTRC=/dev/null`.

**How to reverse.** Delete `bind_tables.rs` and the generator, and cut the
`list_p`/`list_pp`/`list_v`/`list_vv` blocks and the `-q`-known path out of
`builtin_bind`. `bind_listings_come_from_readlines_tables` and
`tests/corpus/a-bind-warns-then-works-in-phases.sh` pin the behaviour.

## §100 — the shell is compiled for speed, against the workspace's size-optimised userspace default

**Date:** 2026-08-02
**Decided by:** Claude (autonomous)

The workspace builds all userspace at `opt-level = "s"`, and that default has a
real reason behind it: sixty coreutils binaries are embedded in the kernel image
(`requests/coreutils_needs_kernel_embedding.md` puts the total at ~45 MiB), so a
megabyte saved is saved sixty times over.

osh does not fit that shape. It is one binary, and it is the *interpreter* — the
thing standing between every line of every script and the work that line asks
for. Its size is paid once at image build; its speed is paid once per loop
iteration, forever.

**The decision.** `[profile.release.package.oils]` overrides the default with
`opt-level = 3` and `codegen-units = 1`, alongside the override the kernel
already has.

**The measurement.** Twelve constructs, each a 40k–300k-iteration loop, best of
three, against bash 5.2.37 on the same host:

| construct | `-Os` | `-O3` | speedup | bash | `-O3`/bash |
|---|---|---|---|---|---|
| `for ((i=0;i<N;i++)); do :; done` | 2356 ms | 1939 ms | 1.22x | 1187 ms | 1.63x |
| `while ((i<N)); do ((i++)); done` | 1739 ms | 1271 ms | 1.37x | 997 ms | 1.27x |
| `while [ $i -lt N ]; do i=$((i+1)); done` | 4257 ms | 3496 ms | 1.22x | 2475 ms | 1.41x |
| `for (( )); do s=$((s+i)); done` | 2793 ms | 2138 ms | 1.31x | 1795 ms | 1.19x |
| `s+=x` | 3277 ms | 2981 ms | 1.10x | 2180 ms | 1.37x |
| `x=${v#h}` | 1806 ms | 1381 ms | 1.31x | 1304 ms | 1.06x |
| `[[ abc123 =~ [0-9]+ ]]` | 1121 ms | 859 ms | 1.31x | 924 ms | 0.93x |
| function call | 2930 ms | 2292 ms | 1.28x | 1750 ms | 1.31x |
| associative-array fill | 405 ms | 322 ms | 1.26x | 262 ms | 1.23x |
| indexed-array fill | 929 ms | 698 ms | 1.33x | 568 ms | 1.23x |
| `case` | 1332 ms | 1035 ms | 1.29x | 999 ms | 1.04x |
| `printf` | 1646 ms | 1365 ms | 1.21x | 746 ms | 1.83x |

Nothing regressed; the geometric mean is ~1.27x. That is a bigger win than any
single algorithmic fix found in the same hunt, and it moves osh from roughly
1.5–1.8x bash to roughly 1.0–1.4x.

**`codegen-units = 1` is free twice over.** It is not measurably faster than 16
units, but it produces a *smaller* binary — 3.22 MB against 3.68 MB — because
one unit lets LLVM share an inlined body instead of duplicating it per unit. So
taking it costs nothing and gives back half the size `-O3` spent.

**The cost.** +884 KB against `-Os` (2.33 MB -> 3.22 MB) and about 20 s more
build time for the crate. Set against ~45 MiB of embedded coreutils, ~0.9 MB for
the shell is under 2% of the image for the component that decides how fast
everything else *feels*.

**Alternatives considered.**

- *Leave it at `-Os` and hunt the 1.5x in the interpreter instead.* That hunt
  had already run its course: two genuinely pathological cases were found and
  fixed (unclosed-bracket globbing, the linear-scan associative array) and the
  remainder was a flat constant factor across every construct — which is exactly
  the signature of a codegen-level cost, not an algorithmic one. Ablating
  `$BASH_COMMAND`'s per-command `put_var` — the most-suspected remaining waste —
  moved nothing (0–8%, and negative on one case).
- *Raise the whole userspace default to `-O3`.* Sixty binaries times ~40% is
  most of a megabyte each, and almost none of them are hot. The per-package
  override is the precise instrument, and the kernel already establishes the
  pattern.
- *Also enable LTO.* Cargo will not take `lto` in a per-package profile — it is
  workspace-wide only — so this cannot be aimed at the shell alone without
  slowing every other crate's build. Left alone.

**How to reverse.** Delete the `[profile.release.package.oils]` section. Nothing
in the source depends on it.

## §101 — osh's `%q`/`@Q` treats every byte above 127 as printable, rather than modelling a C library's `isprint` table

**Date:** 2026-08-03
**Decided by:** Claude (autonomous)

**Context.** bash's `printf %q` (and `${v@Q}`) picks between two renderings by
asking `ansic_shouldquote`, which walks the value and returns true as soon as a
character is not `isprint` in the current locale. A true answer switches the
whole value to the `$'…'` ANSI-C form; a false one uses backslash quoting.
`isprint` is a *locale* question, and the answers differ sharply:

* glibc in a UTF-8 locale: a byte sequence that does not decode is not
  printable, so a lone `\x80` gives `$'a\200b'`, while valid UTF-8 like `é☃`
  passes through raw.
* the reference bash this project diffs against (Git-for-Windows / Cygwin, and
  identically under `LC_ALL=C`): a single-byte Latin-1-ish table, so
  `0x80`–`0x9F` and `0xAD` are non-printable and every other high byte is
  printable — which makes `é☃` (whose UTF-8 holds `0x98` and `0x83`) come out
  as `$'M-CM-)M-b\230\203'`, half raw and half octal.

osh has no locale machinery at all: it is byte-oriented from the lexer up, and
paths, variables and pipe data are `Vec<u8>` by policy (CLAUDE.md's "paths and
OS-boundary data are bytes").

**Decision.** `printf_quote`/`shell_quote` treat exactly the ASCII controls
(`0x00`–`0x1F` and `0x7F`) as non-printable, and every byte from `0x80` up as
printable and passed through untouched.

> **Narrowed 2026-08-07 by §104.** The rejected alternative below —
> "ANSI-C-quote anything that is not valid UTF-8" — was rejected on the grounds
> that it "commits the shell to UTF-8 as *the* encoding". §104 has since made
> exactly that commitment, on the operator's decision, so the objection no
> longer holds and **that half is now implemented**: a byte that decodes to no
> character forces the `$'…'` form, as does a non-ASCII Unicode control
> (`U+0080`–`U+009F`, which the old byte-level test could not see). The
> byte-string policy is not contradicted — `$'a\377b'` re-reads as the same
> bytes, so nothing the user typed is mangled or lost; only its *rendering*
> changes.
>
> What this entry still decides, unchanged, is the **rest** of a libc's
> `isprint` table: format characters (`U+00AD`, `U+200B`, `U+FEFF`), private use
> and unassigned code points stay printable to osh even though the reference
> bash quotes them. The reason is stronger than it was in August: `U+2028`/
> `U+2029` go the *other* way (newlib prints them, glibc does not), so there is
> no single table to copy, and `Cn` (unassigned) would mean carrying a full
> Unicode assignment table that drifts with each release.
>
> The four surfaces that ask the question — `printf %q`, `${v@Q}`, a
> `declare -p` value, and a `declare -p` associative key — now share one
> predicate, `needs_ansi_c_quote`, so a future category model has exactly one
> place to land. The measured table is in `known-issues.md` under
> `TD-OILS-PRINTF-Q-HIGH-BYTES`.

**Rationale.** It is the only rule a shell with no locale can state honestly. It
agrees with glibc-in-UTF-8 for all *valid* UTF-8 — the case that actually occurs
— and it never mangles a byte the user typed. Encoding the reference host's
Latin-1 `isprint` table would bake a Cygwin implementation detail into an OS
that will never run Cygwin, and would still be wrong for the glibc case.

**Alternatives considered.**

* *Match the reference bash byte-for-byte* (a 256-entry table with `0x80`–`0x9F`
  and `0xAD` non-printable). Would make the differential corpus green over the
  whole byte range, but it is a host artifact, not shell semantics, and would
  read as inexplicable to anyone maintaining the code.
* *ANSI-C-quote anything that is not valid UTF-8.* Principled, and matches
  glibc-in-UTF-8 exactly. Rejected because it commits the shell to UTF-8 as
  *the* encoding, which contradicts the byte-string policy — a Latin-1 filename
  is not an error, and quoting it as octal would make `%q` output that no longer
  round-trips visually.

**Consequence for the corpus.** The differential corpus cannot assert `%q` over
bytes above 127; `tests/corpus/printf-converts-its-width-and-quotes-by-a-deny-list.sh`
says so in its header and stays inside ASCII. The deviation is logged in
`known-issues.md` under `TD-OILS-PRINTF-Q-HIGH-BYTES`.

**Where it lives.** `userspace/oils/src/interp.rs` — `printf_quote`,
`shell_quote`.

**How to reverse.** Replace the `is_ascii_control` test in both quoters with a
predicate that takes the desired `isprint` model, and extend the corpus case.

---

## §102 — osh's last-resort recursion ceiling is a measured stack budget, not a nesting count

**Date:** 2026-08-03

**Decided by:** Claude (autonomous)

**Context.** osh's evaluator is a recursive tree-walker, so shell-level
recursion is native recursion. With `FUNCNEST` unset — the default — nothing
bounded it, and `f() { f; }; f` ran until the thread's stack was exhausted. A
Rust stack overflow is an immediate `abort()`: no `trap`, no `EXIT` handler, no
flushed output, and nothing an embedder could survive. The reference bash is no
better here (it segfaults, status 139), so there was no parity answer to copy —
the ceiling and its wording are ours to choose.

**Decision.** Guard on *measured stack consumption*. `Shell::new` records the
stack address it was built at; the binary, which is the only party that knows
how big the thread's stack is, calls `Shell::set_stack_budget` with three
quarters of it; `Shell::exec_command` refuses to descend once the current frame
is that far below the origin, reporting `maximum nesting level exceeded (out of
stack)` with status 1 and letting the recursion unwind. The budget defaults to
`None` — unguarded — so an embedder that never states its stack size keeps the
old behaviour. `FUNCNEST` is untouched and remains the lower, explicit,
bash-compatible ceiling.

**Rationale.** A shell level costs a different number of Rust frames depending
on how it was reached — a function call, an `eval`, a nested compound command —
so no single depth number is right for all of them: it would be too low for the
cheap path and too high (i.e. still an overflow) for the expensive one. The
measured budget is correct for every path by construction, scales on its own
with a release build or a bigger stack, and needs no re-tuning when the
evaluator's frames change size. Placing it in `exec_command` covers every nested
construct at one point, so no re-entrant path can be forgotten.

**Alternatives considered.**
- *A fixed depth counter.* Simpler and cheaper, but wrong for the reason above,
  and it would need re-calibrating whenever a frame grew.
- *A default `FUNCNEST`.* Would change a documented, bash-compatible variable's
  meaning, and would not catch `eval` or compound-command recursion at all.
- *Growing the stack further.* Only moves the cliff; the process still aborts.
- *Unwinding to the top level (bash's `jump_to_top_level(DISCARD)`, which is
  what `FUNCNEST` does).* Rejected because the guard is a resource limit rather
  than a user-stated one: failing the single command keeps `&&`/`||` and the
  enclosing script working, which is the more conservative behaviour for
  something the script never asked for.

**Where it lives.** `userspace/oils/src/interp.rs` — `stack_mark`,
`Shell::stack_base`/`stack_budget`, `Shell::set_stack_budget`,
`Shell::stack_exhausted`, the guard at the top of `Shell::exec_command`;
`userspace/oils/src/main.rs` — `INTERP_STACK_SIZE`, `FALLBACK_STACK_SIZE`,
`stack_budget`, `run`.

**How to reverse.** Stop calling `set_stack_budget` in `main.rs`; the guard is
inert without it. To change the ceiling instead, adjust `stack_budget`'s
fraction (or `INTERP_STACK_SIZE`).

## §103 — `$PATH` is split on the shell's separator, and on the host's too where the host wrote the value

**Date:** 2026-08-04

**Decided by:** Claude (autonomous) — this was Q36 in `open-questions.md`,
raised for the operator on 2026-08-03. It is being taken autonomously because
the recommended option turned out to be small, local and easy to reverse, and
because leaving it open was blocking every corpus case that needs a
multi-entry `$PATH`. The operator may overrule.

**Context.** osh split `$PATH` with `std::env::split_paths`, which on the
Windows development host splits on `;`. Every shell script in the world writes
`PATH=/a:/b`, so osh read that as one directory literally named `/a:/b` —
which never exists. A multi-entry `$PATH` built by a script silently found
nothing, and that reached command lookup, `hash`, `type`, `command -v`,
`compgen -c` and `.`/`source` alike. No corpus case could use a `$PATH` list.

The separator could not be changed on its own, because the separator and the
path *syntax* are one decision. `:` is unambiguous only if a path can never
contain one — but osh keeps native Windows paths with drive letters
(`C:/Users/...`), deliberately, since that is what `$PWD`, `hash` and `type`
show. Splitting those on `:` alone would cut every entry after its drive
letter.

**Decision.** Q36's **option B**: split at the `$PATH` boundary only, with a
drive-letter escape. `std::env::split_paths` is replaced by two free functions
in `interp.rs`:

* `split_search_path` splits on `:` — the shell's separator, everywhere, and
  the whole rule on the SlateOS target. On Windows it *additionally* splits on
  `;`, because the value the shell inherits there is the host's own and is
  written that way.
* `drive_colon_at` is the single exception: a `:` one byte after the start of
  an entry, preceded by a single ASCII letter **and followed by `/` or `\`**,
  belongs to a drive letter and is not a split point.

**Rationale.** It buys the behaviour that matters — a script's own
`PATH=bin:$PATH` works, and multi-entry lookup order becomes corpus-testable —
for one localised function, without committing the project to an msys
convention that SlateOS will never use. Requiring a *separator after* the
colon is what keeps the heuristic honest: `PATH=x:y` still splits into two
one-letter directory names, which is far likelier in a shell script than a
drive-relative `x:y`, and it matches how Windows itself reads the two.

Writing the splitter by hand also fixed two smaller `split_paths` mismatches
that had nothing to do with the separator: `split_paths` yields *nothing* for
an empty string where the shell wants one empty entry (which means the current
directory), and it strips double quotes around an entry, which bash does not.

**Alternatives considered.**
- *A — msys-style drive mapping everywhere* (`/c/Users/...` as the shell's
  canonical spelling). The only option that also aligns `$PWD`, `pwd`,
  `BASH_SOURCE`, glob results and `cd` with the reference bash on this host.
  Rejected as the largest change by far — it touches every place a host path
  enters or leaves the shell — and because it bakes an msys convention into a
  shell whose real target is SlateOS, where paths are already `/`-rooted and
  the problem does not exist. It would be the right answer only if osh were
  meant to be a first-class Windows shell, which it is not.
- *C — accept the divergence as dev-host scaffolding*, documenting it and
  letting it come right for free on SlateOS. Zero risk and zero work, but it
  leaves a real bash-parity gap in `$PATH` lookup order permanently untested
  for as long as the dev host is Windows — which is the whole foreseeable
  future of this work.

**Where it lives.** `userspace/oils/src/interp.rs` — `split_search_path`,
`drive_colon_at`, `Shell::search_dirs`; unit test
`the_search_path_separator_is_the_shell_s_own`.

**How to reverse.** `split_search_path` is the only caller-visible seam: make
its `b';' => cfg!(windows)` arm `false` (or restore `std::env::split_paths` in
`search_dirs`). Nothing else in the shell depends on the two-separator rule.

## §104 — osh is UTF-8-only; the corpus harness compares against a UTF-8 bash

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended this option; operator accepted and
asked that the rejected scope stay documented in `known-issues.md`). This was
Q38 "Should osh be locale-aware, or UTF-8-only?" in `open-questions.md`.

**Context.** bash decides *per locale* whether a string is a sequence of bytes
or of characters: every multibyte site sits behind `HANDLE_MULTIBYTE` and calls
`mbrlen`/`mbstate`, so `${#s}` on `a…b` is 5 under `LC_ALL=C` and 3 under
`LC_ALL=C.UTF-8`. osh has no such switch — it always does UTF-8 character
semantics. `scripts/osh-bash-diff.py` pinned `LC_ALL=C` for both shells, so on
any multibyte input osh was being compared against a bash doing byte semantics,
a baseline osh was never built for. No corpus case had exercised it until one
happened to put a `…` inside a `printf '%-46s'` label.

**Decision.** osh's string layer is UTF-8, full stop, and the harness is moved
to a UTF-8 locale so that the reference bash agrees. `LC_ALL` is not modelled as
an observable switch over character semantics.

**Rationale.** The OS this shell ships in is UTF-8 throughout; there is no
non-UTF-8 locale on the SlateOS target for the switch to serve. Making osh
locale-aware would thread a locale notion through `bytes.rs` — today free
functions with no state — and through every character-counting site (`${#v}`,
`${v:off:len}`, `${v^^}`/`${v,,}`, `%q`, `\u`/`\U`, `select`'s display width,
plausibly globbing and `[[ =~ ]]`), for an axis nothing in the OS exercises.

**What this gives up, kept on the record.** A real bash under `LC_ALL=C` is now
not reproducible by osh at all, so that axis of bash's behaviour goes untested.
Scripts that set `LC_ALL=C` for speed or determinism — a common idiom — get
different answers from osh than from bash. The sharpest example is `%q` on a
byte that is no character: bash's `ansic_shouldquote` defers to
`ansic_wshouldquote`, which quotes when `mbstowcs` fails, so under UTF-8
`a\xffb` prints `$'a\377b'` and under C it prints the raw bytes. There is no
edit to osh that is right under both.

**Alternatives considered.**
- *B — make osh locale-aware, as bash is.* Actually matches bash, which is the
  project's stated goal, and would let the corpus test both axes. Rejected on
  cost-to-value: it is the whole string layer, and the C locale is the *easy*
  half — a non-UTF-8 multibyte locale would be far worse, so the honest scope is
  "C vs UTF-8", not "all locales". The scope stays written down in
  `known-issues.md` under
  `TD-OILS-THE-CORPUS-HARNESS-RUNS-THE-REFERENCE-BASH-IN-THE-C-LOCALE` so that a
  future change of mind starts from a survey and not from scratch.

**Where it lives.** `scripts/osh-bash-diff.py` (the environment it pins);
`userspace/oils/src/bytes.rs` (`char_count`, `char_slice`, `char_at`) and its
callers.

**How to reverse.** Re-pin the harness to `LC_ALL=C` and work the survey in the
`known-issues.md` entry. Nothing in osh encodes the choice; the decision is
which reference behaviour the corpus is measured against.

## §105 — bash's own defects are outside osh's parity target

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended this option). This was Q37 "How far
should osh's bash parity go when the behavior being matched is an upstream bash
*defect*?" in `open-questions.md`.

**Context.** osh is driven toward byte-exact bash 5.2.37 parity, and until this
question every divergence found had turned out to be *designed* bash behaviour
once its source was read. `declare -n q='n[1]'; declare q` — a valueless,
flagless declaration through a reference to an array element — is not: bash
binds a **null value** into `n[1]` via
`bind_variable(q, NULL, ASS_FORCE)` → `assign_array_element("n[1]", NULL, …)` →
`array_insert(a, 1, NULL)`, a NULL that was never checked for. Every reader of
`n` then stops at the null (`${#n[@]}` is 0, `${!n[@]}` is empty) while the
elements are all still there and reappear on the next store. It ignores
`readonly`, and it turns a scalar base into an *empty* array.

**Decision.** Divergences whose bash side is an unchecked defect rather than a
behaviour are waived, marked in the corpus with the reasoning, and recorded in
`known-issues.md`. They are not reproduced.

**Rationale.** The parity target is worth a great deal, but not the core value
model. Reproducing this one means making the array element type nullable
(`Option<Str>`) and teaching every reader — listing, `${!a[@]}`, `${#a[@]}`,
`${a[@]}`, `${a[i]-D}`, arithmetic reads, `unset`, iteration — a "stop at the
first null" rule, purely to chase a state bash cannot explain and may fix
upstream, at which point the change becomes dead weight to unwind.

**Alternatives considered.**
- *B — reproduce it* in the value model. Byte-exact parity with no exceptions,
  which is the stated goal. Rejected as a large invasive change to
  `Shell::arrays` / `Shell::assoc`, threaded through most of `interp.rs`, for a
  defect.
- *C — reproduce only the observable surface*, with a "poisoned" flag on the
  variable that makes readers report it empty until the next store. Rejected as
  exactly the band-aid CLAUDE.md forbids: the flag is a fiction that does not
  survive the next edge case — bash's `n[5]=z` recovery already needs a rule of
  its own.

**The cost, stated plainly.** This sets a precedent that requires judging "bug
vs. design" case by case. The bar: a divergence is waivable only when the bash
side has been traced to its source and found to be an unchecked error path with
nothing in the manual or the comments suggesting intent. Anything short of that
is designed behaviour and gets matched.

**Where it lives.** `known-issues.md`,
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`;
`userspace/oils/src/interp.rs` — `Shell::declare_ref_bind_read`.

## §106 — Defender process exclusions are the answer to the spawn-latency spike

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended this option). This was Q38 "Add
antivirus exclusions so the osh corpus sweep is runnable again?" in
`open-questions.md`.

**Context.** Process creation on the development host costs **~360–390 ms per
spawn** through the MSYS runtime — roughly 20× normal, stable across
back-to-back measurements over four days (`bash -c 'for i in $(seq 1 100); do
/usr/bin/true; done'` takes 36–41 s; re-measured 2026-08-07 at 36.1 s). That
makes `scripts/osh-bash-diff.py` unusable as a gate: cases that should take a
second take 13–53 s against a 20 s budget, and the *reference* shell is what
times out. A sweep on 2026-08-07 returned 41 failures, every one of them a
never-finished case rather than a real diff.

**Decision.** Add Windows Defender **process** exclusions for `bash.exe` and
`osh.exe`, scoped as narrowly as they will go, rather than blanket path
exclusions over the Git and `target\` trees — that keeps ordinary file scanning
intact.

**Action still outstanding — this needs the operator.** Defender's exclusion
list cannot be read or written without elevation, and the automation runs
unelevated (`Add-MpPreference` returns "You don't have enough permissions").
The exact command, to run once from an **Administrator** PowerShell:

```powershell
Add-MpPreference -ExclusionProcess 'bash.exe','osh.exe'
```

Until that is run the sweep stays a soft gate: timeouts are discriminated from
real regressions by timing the case under bash alone.

**Alternatives considered.**
- *B — diagnose further before excluding anything.* The cause is not proven:
  Defender was equally on during a green 444-case sweep on 2026-08-06 at 05:15,
  so something changed. Rejected because it costs operator time and the sweep
  stays unusable meanwhile; the exclusion is cheap to undo if it turns out not
  to be the cause.
- *C — live with it,* relying on the unit suite plus targeted single-case runs.
  This is what was happening meanwhile. Rejected as the standing answer: the
  unit suite does not compare against real bash at all, and single-case runs
  cannot catch a regression in a case you did not think to run.

**Where it lives.** `scripts/osh-bash-diff.py` (`CASE_TIMEOUT = 20` and the
`# TIMEOUT: N` per-case override). Note the fix is *not* raising `CASE_TIMEOUT`:
that would make every genuine hang cost minutes instead of seconds.

**How to reverse.** `Remove-MpPreference -ExclusionProcess 'bash.exe','osh.exe'`
from an elevated shell.

## §107 — B-KNULLJUMP escalates to a compiler-instrumented KASAN kernel build

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended sequencing A first and then
escalating; the operator agreed the lighter path is exhausted and chose B).
This was Q34 in `open-questions.md`.

**Context.** B-KNULLJUMP is an intermittent (~1-in-120) wild **store** into a
live scheduler BTree node. The Q32→A decision built the two lighter corruption
detectors — a lazily-mapped KASAN shadow (`kernel/src/mm/kasan.rs`) and a slab
free-quarantine (`kernel/src/mm/quarantine.rs`), both boot-green and
self-tested. Neither catches this failure mode *passively*: they only see a
write that lands in a parked or poisoned granule, and B-KNULLJUMP stomps a live
node. A 100-iteration armed hunt (`soak-20260723-190300`) came back
100/100 PASSED with `corruptions=0` — **inconclusive, not exonerating**, since
a clean 100-run is ~43% likely even with the bug fully present.

**Decision.** Wire up full compiler-instrumented KASAN
(`-Zsanitizer=kernel-address`), which auto-instruments every load and store and
flags the faulting instruction directly. Probing confirms the target supports
it (`x86_64-unknown-none` → `supported-sanitizers: ['kcfi', 'kernel-address']`).

**Rationale.** It is the only remaining tool that sees the store. The lighter
path was built, hardened and run at scale without localizing the bug, so
continuing it is the "verification loop that yields no edits" CLAUDE.md warns
against.

**What this commits to — a genuine build fork.** Whole-kernel instrumentation
is a large perf hit, so this lands as a **separate debug profile**, not as the
shipping build. It needs: whole-kernel-VA shadow backing rather than heap-only
(Linux uses a shared zero shadow page for untracked regions), in-kernel
`__asan_*`/`__kasan_*` runtime callbacks, a fixed compile-time shadow offset
matching our layout, and `#[no_sanitize]` plus careful ordering on every
early-boot and shadow-setup path. The main risk is destabilizing boot if the
shadow is not fully ready before the first instrumented code runs — which is
why the shadow-setup path itself must be uninstrumented.

**Alternatives considered.**
- *A — keep working the lighter tools.* Cheap, low-risk, already built, and it
  was the right first move. Rejected as the standing path because it has now
  been run to exhaustion; its structural blind spot (only parked/poisoned
  granules are visible) is exactly where this bug lives.

**Where it lives.** `.cargo/config.toml` rustflags
(`-Zsanitizer=kernel-address`, `-Cllvm-args=-asan-mapping-offset/scale`), a new
`__asan_*` runtime module, whole-VA shadow setup in early boot (`main.rs` mm
init), and the existing `kernel/src/mm/kasan.rs` / `quarantine.rs` /
`heap.rs` hooks.

**How to reverse.** The instrumentation is a build profile; dropping the
rustflags returns the kernel to the current build. The `__asan_*` runtime and
the whole-VA shadow are additive modules that go unused without it.

## §108 — fastpy stays additive for now, with a stated trajectory to becoming a real implementation

**Date:** 2026-08-07

**Decided by:** Operator (Claude recommended "A for now"; the operator accepted
it and set the direction beyond it). This was Q35 in `open-questions.md`.

**Context.** The fastpy `/bin`-promotion (§87 follow-on) installs `cat`, `wc`,
`head`, `tail` at `/bin/<cmd>` in the **test** rootfs. These are minimal
proof-of-pipeline implementations — `cat` is ~5 lines of Python — while SlateOS
already ships 85 mature Rust coreutils (roadmap §2.7). At some point one
shipping `/bin` has to decide which `cat` is *the* `cat`.

**Decision — three parts.**

1. **For now: additive only.** fastpy commands keep being promoted into the
   *test* rootfs `/bin` to exercise the pipeline. No Rust coreutil is touched,
   shadowed or retired. A silent swap is a user-visible policy change and is not
   Claude's to make.
2. **The trajectory is toward fastpy being a real implementation**, per command,
   gated on two bars: a real parity test suite for that command, and a
   performance bar — fastpy's version must be **faster, equal, or not
   significantly slower** than the Rust one. Whether the user gets the fastpy
   version is then an **opt-in switch**, not a silent substitution.
3. **fastpy's scope is not coreutils.** The operator's original intent was any
   non-CPU-intensive OS function — driving a file explorer window, a settings
   dialog — and, because fastpy compiles to native code, CPU-intensive ones too.
   The coreutils promotion is a *pipeline test*, not the point. Roadmap items
   that reach for fastpy should be picked with that in mind rather than treating
   `/bin` as the target surface.

**Still open, deliberately.** Which way the *shipping default* points — whether
a stock install prefers fastpy implementations where they exist, or prefers the
canonical ones and makes fastpy the opt-in — is not settled. It is carried
forward in `open-questions.md` as its own narrower question, to be answered when
there is at least one fastpy utility that has actually cleared both bars, since
answering it earlier would be answering it without evidence.

**Alternatives considered.**
- *B — swap per command as each reaches parity, retiring the Rust one.* This is
  the trajectory, but not the current state: adopting it now would throw away
  the maturity and measured performance of the Rust tools before any fastpy
  utility has a parity suite to justify it.
- *C — coexist under distinct names* (`/bin/pycat`). Rejected: it clutters
  `/bin` and leaves no answer to "which is canonical", which is the actual
  question.

**Where it lives.** `scripts/create-ext4-rootfs.sh` (the `PROMOTED` map —
currently the *test* rootfs `/bin`), `kernel/src/proc/spawn.rs`
(`resolve_command` / `COMMAND_PATH`), the `services/fastpy-*` sources, and
whatever eventually assembles the production rootfs `/bin`.

**How to reverse.** Part 1 is the status quo and needs no unwinding. Parts 2–3
are direction, not code; the first per-command swap is where the decision
becomes concrete, and it is gated on the two bars above.

## §109 — `kill(-pgid)` gets one implementation shared by both ABIs, and `CAP_KILL` now covers it

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Two calls made while fixing
`TD-POSIX-PROCESS-GROUPS-ARE-FAKE-FOR-NATIVE-ABI-PROGRAMS`; both are mine to
revisit and the operator may overrule either.

**Context.** `AbiMode` is per-process — a program is Native or Linux, never
both — and the kernel's process groups were reachable only through the Linux
syscall shim. Fixing that meant giving the native ABI its own way in, and the
obvious shape (a native handler that reimplements what `kill_process_group`
does) would have produced two copies of the group-signal ordering rules.

**Decision 1 — the fanout moves *down*, not sideways.**
`handlers::signal_send_to_group` is the single implementation, written against
`KernelError`; `sys_signal_send_with_info` routes every `arg0 <= 0` to it, and
`linux.rs::kill_process_group` loses its ~68-line body to become a three-line
adapter that translates the result through `linux_from_native`.

*For:* the ordering rules that make group signalling match Linux's
`kill_something_info` — resolve membership first, **ESRCH before EINVAL**,
`sig == 0` as an existence probe, best-effort fanout that succeeds if any
member accepted — are subtle, individually invisible when wrong, and now exist
in exactly one place. A native-ABI shell and a glibc shell provably observe
identical semantics, which is the property the bug was a violation of.

*Against:* the native layer now owns a shape borrowed from Linux, including
`ESRCH`-before-`EINVAL`, which is a Linux compatibility choice rather than
something our design spec asked for. A future native-only refinement has to be
made without breaking the Linux caller. Accepted because the alternative —
letting the two ABIs drift — is the exact failure this whole change exists to
repair, and because the ordering is defensible on its own terms: "which group?"
is a more fundamental question than "which signal?".

*Alternative considered:* keep `kill_process_group` where it is and have the
native handler call *it*. Rejected because it inverts the layering — the Linux
shim is a translation layer over the native ABI everywhere else, and making
native code depend on it would be the only exception.

**Decision 2 — `CAP_KILL` now gates the group forms of `kill`.**

Previously `posix::kill` applied `CAP_KILL` only to `KillTarget::Other`, and
the group arm was exempt.

*For:* the exemption was not a considered policy. It existed because the group
forms returned `ENOSYS` before reaching any gate, so the gate was unreachable
dead weight rather than a deliberate carve-out. Now that a group send really
does reach other processes, leaving it exempt would make
`killpg(g, SIGKILL)` a way to do exactly what `kill(pid, SIGKILL)` is denied —
a privilege escalation with a one-line exploit. Linux's own
`check_kill_permission` is applied per target, group or not.

*Against:* it is a behaviour change to an existing API, and it broke two posix
tests that asserted the old exemption (they were rewritten to assert `EPERM`).
A caller that held no `CAP_KILL` and relied on `killpg` "working" would now
fail — but no such caller can exist, because before this change `killpg` could
not work for anyone.

Note the gate is a *userspace* layer: the kernel independently applies its own
per-target authority check (parent/self/`Process` capability with DELETE) to
each member of the fanout, so a forged libc cannot use this route either.

**Where it lives.** `kernel/src/syscall/handlers.rs`
(`signal_send_to_group`, `sys_signal_send_with_info`),
`kernel/src/syscall/linux.rs` (`kill_process_group`, `sys_kill`),
`posix/src/signal.rs` (`kill`, the `KillTarget::ProcessGroup` arm).

**How to reverse.** Decision 1: copy the core back into `linux.rs` — but then
the two ABIs need a shared test asserting they agree, or the divergence
returns. Decision 2: delete the `has_capability(CAP_KILL)` check from the
`ProcessGroup` arm and restore the two posix tests, which are named for what
they assert and so will read as a deliberate exemption rather than an accident.

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


## §111 — A self-stop gets its own syscall rather than reusing `SYS_SIGNAL_SEND`

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** The `Stop` default action (SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU) was
the last default action `posix` could not carry out. Its code said *"We have no
kernel suspend mechanism; report ENOSYS"* — and that had been false for weeks:
`TaskState::Suspended`, `JobControlEvent`, `stop_process_for_signal` and
`continue_process` all existed and were driven by the *cross-process* signal
path. What was missing was only a way for a process to ask for its own stop.
That stale comment is the same expired-rationale defect as Q41's §72 blocker
and the `nnp_guard()` doc; it is recorded here because the fix is one line of
code and the *finding* is the valuable part.

**The obvious implementation is wrong.** `kill(getpid(), SIGTSTP)` looks like
it should work, and for SIGSTOP it nearly does. But `classify_post_info` tests
SIGSTOP and SIGCONT explicitly *before* it tests `has_trampoline`, so the three
**catchable** stop signals fall through to the trampoline branch — and a native
process always has a trampoline registered, from `init_signals()`. A self-sent
SIGTSTP would therefore be marked pending for handler delivery, the trampoline
would run `dispatch_self_signal`, and that is the function that just resolved
it to `SIG_DFL` and asked for the stop. The result is an infinite delivery
loop, not a stop. Reordering `classify_post_info` to test the stop signals
first was rejected: that check is what makes a *cross-process* SIGTSTP
catchable, which is required — a shell must be able to trap Ctrl-Z.

**Decision.** Add `SYS_SIGNAL_STOP_SELF` (1062), taking only a signal number,
validated to 19..=22, which calls `stop_process_for_signal(pid, sig, Some(current))`
directly. Two properties follow that a send-based version could not have:

- **The wait status names the right signal.** The parent's `WSTOPSIG` reports
  the signal that actually stopped the child, so a shell's Ctrl-Z shows
  SIGTSTP rather than being flattened to SIGSTOP.
- **It grants no authority.** There is no pid argument and no capability check,
  because self-only is a property of the *signature*, not of a runtime test.
  Cross-process stops stay on `SYS_SIGNAL_SEND` behind `CAP_KILL`. Adding a pid
  argument here — the tempting generalisation — would turn it into a cheaper,
  un-capability-checked route to exactly that authority, so it is deliberately
  absent.

*Against:* it is one more syscall number for something POSIX expresses with
`kill()`, and it splits "stop" across two entry points. *For:* the alternative
is not "reuse `kill`" but "reuse `kill` **and** reorder the classifier so that
cross-process SIGTSTP stops being catchable" — that is a worse trade, and the
loop it would create is silent rather than diagnosable.

**Testing note.** The kernel self-test covers only the rejection path. A valid
stop signal suspends every thread of the caller and returns only on SIGCONT, so
issuing one from the boot self-test task would park the boot thread with nobody
left to resume it. The argument gate runs before any process lookup, so the rejection
cases short-circuit safely. The accept path needs a second process to observe
the stop and send the SIGCONT, so it belongs in a ring-3 test.

---

## §112 — Job-control wait status gets a new syscall number, not new arguments on `SYS_PROCESS_WAIT`

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** §111 gave a process a way to *stop* itself. That is only half a
job-control implementation: a shell also has to *observe* the stop from the
parent side. Our native `waitpid` accepted `WUNTRACED`/`WCONTINUED` and then
dropped them — it called `SYS_PROCESS_WAIT` (501), which reports only a death.
So the question was how to get an options word and a wait *status* to and from
the kernel.

**Decision.** A new syscall, `SYS_PROCESS_WAIT_STATUS` (1063), taking
`(pid, options, status_out)`. `SYS_PROCESS_WAIT` (501) is left exactly as it
was.

**Why not extend 501 — two independent reasons, either one sufficient.**

1. *Its return channel is full.* 501 returns the child's exit code in `rax`.
   Every 64-bit value is a legitimate exit code, so there is no bit pattern
   left to mean "stopped" or "continued". Adding a status out-pointer would
   mean 501's callers must now read a second output they were never told
   about.
2. *Its argument registers already hold garbage.* This is the decisive one, and
   it was already written down: 501's own specific-pid path refuses to use
   `arg1` because "callers using a 1-arg syscall wrapper leave a stale
   (possibly valid) pointer in that register". An options word read from such a
   register would enable random option bits; a status pointer read from one
   would be **written through** — a wild store into a caller that did nothing
   wrong. A new syscall number is the only way to add arguments to a syscall
   whose existing callers do not set them.

**Against.** It is one more number in the table, and two syscalls now answer
"wait for a child", which is a maintenance hazard if they drift: they read the
same kernel records, so they must not disagree about which children a selector
names, which transition is reported first, or what wstatus bits describe it.

**How that hazard is contained** (per §109, "move the fanout down, not
sideways"): rather than duplicating `sys_wait4`'s 185-line blocking loop, that
body moved *down* into `handlers::wait_for_child_event`, and the two wstatus
encoders moved down into `pcb` as `JobControlEvent::to_wstatus` and
`ExitInfo::to_wstatus`. Both ABIs are now thin adapters over one implementation,
so they cannot drift — drift would require someone to deliberately re-fork the
shared core. `sys_wait4` shrank from 184 lines to 32 in the process.

**Alternative considered: a `waitid`-style syscall taking a `siginfo_t`.** That
is what Linux would suggest, and it generalises better (`P_PID`/`P_PGID`/`P_ALL`
selectors, `WNOWAIT`). Rejected for now because it needs a userspace struct
layout the kernel must write field-by-field, which is a larger trust boundary
than a single `i32`, and because our `waitid` already synthesises its result
from the same records in userspace. If `waitid` ever needs to be authoritative,
it should be built on `wait_for_child_event` too rather than beside it.

**Testing.** `dispatch::test_dispatch_wait_status_reports_job_control` drives
nine fixture cases with synthetic `record_jc_stopped`/`record_jc_continued`
records — all with `WNOHANG`, because the boot self-test task must not park —
covering option validation, invisibility without `WUNTRACED`, single-consumption
of each report, that a stop does not reap, and the zombie→`ECHILD` sequence.
The blocking path and the two-process case belong to the ring-3 fixture
`services/ctest-jobctl`, which is the only layer that can observe them.

## §113 — The controlling terminal is session state in the kernel, and `tty::FOREGROUND_PGID` had to die for it

**Date:** 2026-08-12
**Decided by:** Claude (autonomous)

### Context

`tcsetpgrp`/`tcgetpgrp` and `ioctl(TIOCSPGRP/TIOCGPGRP)` are how a
job-control shell hands the terminal to a job and takes it back. Before
this change the "foreground process group" existed in three unrelated
places:

1. `posix`'s `FG_PGRP` — a per-process userspace static. A shell that
   foregrounded a job wrote its own copy; the job read its own, different
   copy. Neither could see the other.
2. `kernel/src/tty.rs`'s `static FOREGROUND_PGID: AtomicU64` — the value
   that `^C`/`^\` actually signalled. Only the Linux shim's `TIOCSPGRP`
   ever wrote it.
3. Nothing at all for the native ABI.

So a glibc program and a slateos-libc program disagreed about which job
owned the terminal, and *both* could disagree with the group that would
actually receive `^C`. The roadmap had recorded (1) as "local rather than
wrong — no other process can observe or contradict it"; that rationale
was false the moment (2) existed.

### Decision

One table in `pcb`, keyed by session ID:
`static CTTY_FG_PGRP: Mutex<BTreeMap<ProcessId, ProcessId>>` (sid → fg
pgid). Everything else becomes a derived read of it:

- Native ABI: `SYS_TTY_GET_PGRP`/`SET_PGRP`/`ACQUIRE_CTTY`/`RELEASE_CTTY`
  (537-540).
- Linux shim: `TIOCGPGRP`/`TIOCSPGRP` call the same `pcb` functions;
  `TIOCSCTTY`/`TIOCNOTTY` are implemented rather than silently accepted.
- `tty::foreground_pgid()` returns `pcb::ctty_console_fg_pgrp()` and owns
  no state of its own.
- `posix`'s `FG_PGRP` is deleted; on the host it is replaced by a
  *thread*-local test double, per §110.

Locking order: `CTTY_FG_PGRP` is never taken while `PROCESS_TABLE` is
held. Every accessor reads what it needs from the process table, drops
that lock, then takes the ctty lock.

### Alternatives considered

**Keyed by session vs. a single global `Option<(sid, pgid)>`.** We have
exactly one console, so a single `Option` would be sufficient today and
simpler. Rejected: the map costs nothing, and the *shape* of the state is
the thing being got right — "the foreground group belongs to a session"
is the invariant that the old code violated, and a global `Option` states
"the foreground group belongs to the machine", which is the same category
error one step further out. The one place the single-console assumption
does leak is `ctty_acquire`, which treats "the map is non-empty" as "the
console is taken"; that is documented at the call site and is the only
line that needs revisiting when a second terminal exists.

**Reusing `TIOCSPGRP`'s existing `tty.rs` atomic and having libc call
into it.** Rejected: it has no notion of a session, so it cannot answer
`ENOTTY` (a process in a session with no terminal), cannot reject a pgid
from another session, and cannot be cleaned up when a session exits. It
was the wrong shape, not merely the wrong location.

**New syscall numbers (537-540) vs. multiplexing an existing one.**
Consistent with §112: a distinct operation gets a distinct number rather
than a mode argument on a neighbour. Four numbers because the get/set
pair and the acquire/release pair genuinely do four things.

**Leaving `TIOCSCTTY`/`TIOCNOTTY` as silent no-ops.** They were
previously accepted and ignored, which was harmless only while
`tcgetpgrp` could never fail. Once `ENOTTY` became reachable, a program
that did `setsid(); ioctl(fd, TIOCSCTTY, 0); tcsetpgrp(...)` would have
been told its `TIOCSCTTY` succeeded and then denied the terminal. Silent
acceptance is a lie that only pays while nothing checks.

**`ctty_acquire` resetting the foreground group when called twice.**
Rejected. Some programs call `ioctl(TIOCSCTTY)` defensively at startup;
if a redundant call reset the foreground group to the caller's own, a
shell would yank the terminal back from a job it had just foregrounded.
A repeat acquire by the session that already owns the terminal succeeds
and changes nothing.

### Consequences

- The group that receives `^C` and the group userspace believes is
  foreground can no longer diverge, because they are the same read.
- `setsid()` drops the caller's terminal (which is why the daemon idiom
  is `setsid()` and not `TIOCNOTTY` — the latter hangs up the caller's
  own group).
- `TIOCNOTTY` by the session leader does SIGHUP-then-SIGCONT across the
  foreground group, matching Linux's `disassociate_ctty(on_exit=0)`. The
  hangup lives in `handlers::hangup_released_ctty` and is shared by both
  ABIs so it cannot drift.
- `SIGTTIN`/`SIGTTOU` enforcement is *not* included: the predicate it will
  use (`pcb::ctty_is_background`) is here, but nothing calls it yet. The
  enforcement points are the console read path, the console write path
  (gated on `TOSTOP`), and `tcsetpgrp`/`tcsetattr`. See `todo.txt`.
- **`Ctrl-Z`, by contrast, started working the moment this landed**, and an
  earlier draft of this entry wrongly listed it as deferred alongside
  `SIGTTIN`/`SIGTTOU`. `tty::feed` has always turned `VSUSP` into
  `LineStep::Signal(20)` under `ISIG`, exactly the way it turns `VINTR`
  into `SIGINT`, and `linux.rs`'s `deliver_console_signal` has always sent
  that to every member of `pids_in_group(tty::foreground_pgid())`. What was
  missing was not the generation or the delivery but the *target*: before
  this change `foreground_pgid()` read `tty.rs`'s own atomic, which only
  the Linux shim's `TIOCSPGRP` ever wrote, so a `^Z` was delivered to
  whatever group last happened to be written there. Now that it is a
  derived read of the session's table, `^Z` reaches the job the shell
  actually foregrounded. This is the clearest illustration of why the
  three-copies bug was worth fixing: the job-control *mechanism* was
  complete and correct, and was simply aimed at the wrong group.

---

## §114 — The native ABI reaches the real terminal: `termios` and console reads become kernel state

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** §113 unified the foreground process group and observed, in
passing, that the terminal's *other* state was still split. Following that
thread found the same bug shape twice more, both on the native-ABI side:

- **`tcgetattr` answered from a constant.** `posix/src/ioctl.rs`'s
  `handle_tcgets` returned `default_termios()` — cooked mode with echo —
  unconditionally, no matter what the terminal was actually doing.
- **`tcsetattr` was a silent no-op.** `handle_tcsets` accepted the call and
  discarded it. Its comment explained why: *"our console has no configurable
  line discipline"*. That was true when written. It stopped being true when
  `kernel/src/tty.rs` gained `ICANON`/`ISIG`/`ECHO`/`NOFLSH`/`VMIN`/`VTIME`,
  and the comment outlived the fact.
- **`read()` on a console fd bypassed the line discipline entirely.**
  `posix/src/file.rs` issued `SYS_CONSOLE_READ_CHAR`, which reads one raw
  byte straight from the keyboard driver. So a native-ABI program got no
  line editing, no `VEOF`, no `VMIN`/`VTIME`, and — the serious one — no
  `ISIG`: `^C` arrived as byte 0x03 instead of generating `SIGINT`.

Meanwhile the Linux shim had all three for real (`tty::get_termios`,
`tty::set_termios`, and a `dispatch_console_read` driven by
`tty::console_read`). Two programs on the same console, differing only in
ABI, therefore saw two different terminals — and the native one saw a
terminal that could not be configured and could not generate a signal.

**Decision.** Add three native syscalls (541–543) and make libc use them:

- `SYS_TTY_GET_TERMIOS` / `SYS_TTY_SET_TERMIOS` — read and write the same
  `tty::get_termios()`/`tty::set_termios()` the line discipline consults.
- `SYS_TTY_READ` — a console read *through* the line discipline, replacing
  `SYS_CONSOLE_READ_CHAR` as libc's `read()` path for `HandleKind::Console`.

The read implementation is shared, not duplicated:
`handlers::tty_read_into_user` returns a `TtyReadOutcome`, and both the
native `sys_tty_read` and the Linux `dispatch_console_read` encode it. They
differ only in error representation (a `KernelError` versus a negative Linux
errno), which is the one thing the enum leaves to the caller. Terminal-signal
generation likewise moved to the shared `handlers::deliver_console_signal`.
This is the same anti-drift move as §113's `hangup_released_ctty`.

**Alternatives considered.**

- **Make `SYS_CONSOLE_READ_CHAR` itself go through the line discipline.**
  Rejected: it is a *raw single-keystroke* primitive with real callers who
  want exactly that (and returning up to `count` bytes from a syscall
  documented to return one would break them). Its doc comment now says so
  explicitly instead, and points at 543.
- **Give libc its own line discipline in userspace.** Rejected outright —
  it is precisely the fourth-copy mistake §113 was about. The signal
  generation has to happen somewhere that can `kill_pgrp`, which is the
  kernel.
- **Multiplex termios onto an existing syscall rather than spend two
  numbers.** Rejected for the same reason as §113: a multiplexed `ioctl`
  with a command word is exactly the ABI shape this kernel avoids, and
  535–599 is not a scarce range.
- **Reject unknown `c_lflag` bits in `SYS_TTY_SET_TERMIOS`.** Rejected:
  `termios` has no invalid encodings, and every program builds its argument
  by reading the current settings and OR-ing, so rejecting a superset would
  break the normal usage pattern. The Linux shim's `TCSETS` already installs
  what it is given.

**Consequences.**

- Raw mode is real for native-ABI programs: `tcsetattr` changes what the
  next `read()` does, and `tcgetattr` reports it back.
- `^C`/`^\`/`^Z` at a native-ABI program now generate signals for the
  session's foreground group instead of being delivered as data bytes. With
  §113 aiming them at the right group, terminal job control now works from
  *either* ABI.
- **`sys_tty_read` is the first native syscall to emit a restart sentinel.**
  This needed no new *machinery*: `entry.rs` applies
  `resolve_syscall_restart` to both ABIs' dispatch results, so a `^C` meant
  for another process transparently restarts the read, and a `^C` that runs
  a native handler becomes `EINTR` (native handlers have no `SA_RESTART`, so
  restarting would hide the interruption from a program that asked to see
  it). The stale "no native syscall emits a sentinel today" comment in
  `deliver_pending_signal` was corrected. But it did expose two latent
  *encoding* bugs, both unreachable until a native syscall could be
  interrupted:
  - **The native delivery path was folding sentinels into Linux's `EINTR`.**
    `deliver_pending_signal`'s native branch called what was then
    `leaked_sentinel_to_eintr`, which substitutes Linux's `EINTR` = 4, i.e.
    a return value of `-4`. A native process reads its return value as a
    `KernelError` code, and `-4` there is `WouldBlock`/`EAGAIN`. An
    interrupted read would have reported "try again" — the one errno a
    caller most needs to distinguish from "interrupted". Fixed to
    `KernelError::Interrupted` (`-8`), and the helper renamed
    `leaked_sentinel_to_linux_eintr` so the ABI assumption is in the name
    rather than in a comment. The `restart` module stays inside `linux.rs`:
    everything in it *except* that one function is ABI-neutral, so moving
    the module wholesale would be as wrong as leaving it unlabelled.
  - **`posix`'s `errno::native` table was missing `INTERRUPTED` (-8) and
    `DEADLOCK` (-7).** Both fell through to `_ => EIO`, so even after the
    kernel returned the right code libc would have told the program the
    disk had failed. Added, with a test that pins the numeric collision
    (`-EINTR` is bit-identical to `native::WOULD_BLOCK`) so a future
    refactor that merges the two encoding paths fails loudly.
- **Verification had to reach ring 3 to mean anything.** A set/get round
  trip cannot prove the two structs agree — the kernel stores and returns
  whatever it is handed, so *mutually wrong* marshalling round-trips
  perfectly. The proof is `ctest-ctty` checks 90–116, which read
  kernel-authored values (Linux's `INIT_C_CC`, `B38400`) at *musl-computed*
  indices, then set raw mode and confirm the untouched fields did not
  shift. Host `cargo test` sees only the marshalling (the syscall arms are
  `#[cfg(target_os = "none")]`) and the kernel self-test calls the handlers
  directly, never through libc; only ring 3 joins the two halves. A first
  boot run appeared to pass while actually executing the *previous* fixture
  ELF — the full chain (sysroot → fixture build → rootfs image → boot) has
  to be re-run or the test silently proves nothing.
- **`default_termios()` is now `#[cfg(not(target_os = "none"))]`.** It is
  host-test scaffolding, and compiling it out of the bare-metal build is
  what guarantees no target-side path can quietly start answering from a
  constant again — which is the exact failure §114 exists to remove.
- The two wire formats stay distinct on purpose: musl's user `struct
  termios` is 60 bytes (`NCCS` 32, plus `c_ispeed`/`c_ospeed`) and the
  kernel's is 36 (`NCCS` 19, baud carried in `c_cflag`'s `CBAUD` bits).
  `posix` marshals between them exactly as glibc and musl do, so a program's
  struct never has to match the kernel's. Both sizes are asserted in tests.
- What is still *not* here: `SIGTTIN`/`SIGTTOU` enforcement (unchanged from
  §113 — the predicate `pcb::ctty_is_background` exists, the call sites do
  not; **closed by §115**), and there is still no TTY *device* layer (no
  `/dev/tty`, no PTYs). A line discipline is not a tty driver; this closes
  the ABI gap, not the device gap.

---

## §115 — Terminal-access job control: one pure decision, two ABIs, and the disposition the kernel cannot see

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** §113 and §114 both closed with the same sentence: the foreground
process group is real, the terminal's modes are real, but nothing *enforces*
the distinction. A background process could read the keystrokes meant for the
foreground job, and could reconfigure or seize the terminal out from under it.
`pcb::ctty_is_background` had existed since §113 and had no callers. This is
the last of the three pieces `todo.txt` recorded as deferred (the other two —
Linux's `TIOCSCTTY` steal argument and per-process `TIOCNOTTY` — are blocked
on a credential model and on a second terminal existing, respectively).

Linux's `__tty_check_change()` (`drivers/tty/tty_io.c`) is the model, and it
has two asymmetries that are easy to get backwards:

```c
if (tty_pgrp && pgrp != tty_pgrp) {
    if (is_ignored(sig))            { if (sig == SIGTTIN) ret = -EIO; }
    else if (is_current_pgrp_orphaned())  ret = -EIO;
    else { kill_pgrp(pgrp, sig, 1); ret = -ERESTARTSYS; }
}
```

First, the signal goes to the **caller's own** group, not the terminal's
foreground group. Second, when the signal is ignored or blocked, a *read*
fails `EIO` while a *write* is let through — because the point of refusing the
read is to keep terminal input away from a background job, and there is no
equivalent harm in output.

**Decision 1 — the policy is a pure function; only a thin wrapper has
effects.** `handlers::tty_job_control_decide(pid, sig) -> TtyAccessDecision`
(`Allow` / `Signal(sig)` / `Fail(err)`) contains all of the logic;
`tty_job_control_check(sig)` resolves the caller, calls it, fans the signal
out over `pcb::pids_in_group`, and returns the restart sentinel.

The split is not aesthetic. The effectful form asks about *the calling
process*, and the kernel self-test task owns no process at all — `caller_pid()`
returns `None` — so a self-test can never put itself in the background of a
terminal in order to be checked. Every branch of this policy would have been
unreachable from `cargo test` and from the kernel's own self-tests, leaving
only the ring-3 fixture, which cannot construct an orphaned group on demand.
With the decision extracted, `dispatch.rs::test_dispatch_tty_job_control`
builds shell + job + orphan states directly out of `pcb` and asserts all four
cases.

**Decision 2 — the blocked-signal check is a liveness requirement, not a
nicety.** It is tempting to read the `is_ignored` arm as an errno refinement.
It is not: a *blocked* `SIGTTIN` stays pending and undeliverable, so posting it
and returning `ERESTARTSYS` would restart the read, re-check, post again — an
unkillable spin inside the kernel. The kernel owns the blocked mask for both
ABIs, so this half is always exact and is checked first.

**Decision 3 — the kernel deliberately cannot see a native-ABI `SIG_IGN`, and
that is documented rather than approximated.** `signal_ignored_or_blocked`
answers from three sources: the blocked mask (kernel-owned, exact); "no
trampoline registered", in which case the kernel's own default-action table
*is* the disposition (exact); and an explicit `SIG_IGN`, which lives in
`linux.rs`'s private `linux_sigaction_table` and is therefore visible only for
`AbiMode::Linux`. This is not an oversight but the standing architecture:
`proc/signal.rs` states that userspace owns the native per-signal disposition
table, which is exactly why `SYS_SIGNAL_STOP_SELF` exists — userspace *reports*
an already-resolved disposition rather than asking the kernel to re-derive one.

The alternative — inventing a native `sigaction` table in the kernel now, purely
so this one predicate could consult it — would have duplicated state that
userspace already owns, which is the bug shape §113 and §114 were spent
removing. The consequence is recorded instead: a job-control shell on our
native libc must **block** `SIGTTOU` around `tcsetpgrp` rather than ignore it
(bash ignores it). `todo.txt` item 1a states the proper fix and its trigger,
and the ring-3 fixture demonstrates the blocking form so the limitation is
executable documentation rather than a comment.

**Decision 4 — de-duplicate the enforcement points instead of adding the
policy twice.** The Linux shim's `TCSETS`/`TCSETSW`/`TCSETSF` and `TIOCSPGRP`
arms had their own copies of the `copy_from_user` + `tty::set_termios` and
`ctty_set_fg_pgrp` bodies. Adding the gate to both copies would have been two
places to forget it. They now delegate to shared
`handlers::tty_set_termios_from_user` / `tty_set_pgrp_checked` returning a
`TtyCtlOutcome`, the same shape `dispatch_console_read` already used for reads
in §114.

Note the `TOSTOP` asymmetry, which mirrors `n_tty`: the write gate is
conditional on the new `lflag::TOSTOP` (off by default, as on Linux), while the
read gate and the terminal-control gate (`tcsetattr`, `tcsetpgrp`) always
apply. `TOSTOP` governs background *output* only.

**Two bugs this uncovered.**

- **`linux_from_native` was flattening restart sentinels to `EINVAL`.** It maps
  any negative it cannot decode as a `KernelError`, and `restart_result`'s
  `-512` is exactly that. Latent until `sys_console_write` grew a `SIGTTOU`
  gate, since `dispatch_write` routes console writes through it. The guard
  added there is load-bearing, not defensive.
- **`posix`'s `ctty_errno` mapped `IoError` and `Interrupted` to `ENOTTY`.**
  Its fallback arm is `ENOTTY` on the reasoning that it is the conservative
  reading — true for an unknown failure, actively wrong for these two, which
  job control had just made reachable. `ENOTTY` tells a shell "you are not
  interactive, skip job control", i.e. the exact opposite lesson to draw from
  having just been stopped for touching the terminal. Both now map explicitly.
  This is the same class of defect §114 found in `errno::native` (missing
  `INTERRUPTED`/`DEADLOCK`) — an error table that was complete for the set of
  errors reachable when it was written.

**Consequences.**

- `pcb`'s orphan predicate was split. The existing function conflated "is
  orphaned" with "has a stopped member" (the `SIGHUP`+`SIGCONT` trigger);
  job control needs the plain form. Both now derive from one locked pass
  (`pgrp_orphan_state`), so the two callers cannot see the group in two
  different states.
- `proc/signal.rs`'s private `signal_bit` became public and absorbed a second,
  freshly-written copy of the same `sig - 1` shift, plus an open-coded
  `1 << ((sig - 1) & 63)` in `emit_linux_rt_frame`. That `& 63` was a real
  latent bug: an out-of-range signal number would have blocked some *other*
  signal for the duration of a handler, where `signal_bit` yields `None`.
- A native process with **no** trampoline gets full POSIX behaviour on the
  restart path — `entry.rs` runs `deliver_pending_signal` (which performs the
  stop inline and returns after `SIGCONT`) before `resolve_syscall_restart`
  rewinds `RIP`. A native process **with** a trampoline sees `EINTR` instead,
  because our native ABI has no `SA_RESTART` for a handler to request.
- The ring-3 fixture gained checks 72–86, which run in the one window where it
  is genuinely background — after handing the terminal to its child's group.
  It cannot be stopped there: the kernel spawns it with parent 0, so its group
  is orphaned and it gets `EIO`. That made the fixture's old check 73 (`EPERM`
  for foregrounding a reaped child's empty group) unreachable, which is
  correct — job control fires first — so the check was kept and moved behind a
  `sigprocmask(SIG_BLOCK, SIGTTOU)`, which is what a real shell must do anyway.

---

## §116 — A recorded mtime is only evidence once it has outlived its own granularity

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Where:** `apps/diffcore/src/lib.rs` — `FileSync::{record, touch, stat, changed}`,
`FileSync::mtime_racy`, `MTIME_SETTLE`. Consumers: `Document::disk_changed` in
`apps/editor/src/main.rs` and `apps/markdowneditor/src/main.rs`.

### The problem

`FileSync` is the external-edit detector both text editors embed. It records a
file's content (the merge ancestor) and its mtime, and `changed()` used the
mtime as a fast path: same mtime as recorded ⇒ `Unchanged`, skip the read.

That inference is false. Filesystem timestamps are coarse — FAT to 2 s, ext3 to
1 s, NTFS nominally to 100 ns but stamped from a system clock that advances in
~15.6 ms steps — so a write landing in the same tick as the recording carries
the *same* mtime. The editor then never raises the external-change prompt, and
the next save overwrites the external edit with no merge and no warning. This
is silent data loss, not a missed notification, and it was found by diffcore's
own test suite failing intermittently on a fast machine.

### The decision: take the raciness verdict at record time, git's way

`record`/`touch` funnel through a private `stat()` that captures the mtime and,
**at that moment**, decides whether it can ever be evidence:
`now - mtime < MTIME_SETTLE` sets `mtime_racy`, and `changed()` refuses the fast
path while it is set. This is git's "racily clean" rule (`read-cache.c`) applied
to a single file instead of an index.

**Alternatives rejected:**

- *Compare `SystemTime::now()` inside `changed()` instead of storing a flag.*
  Unsound, and the failure is the same silent one. The aliasing write happened
  **inside** the granularity window, which has already closed by the time anyone
  asks; the mtime it left behind is indistinguishable from the honest one
  forever. Raciness is a property of the *recording*, so it must be decided
  there.
- *Delete the fast path.* Sound, but it throws away a real optimisation: a file
  nobody has touched should not be re-read on every check.
- *Pick `MTIME_SETTLE` from the actual filesystem's granularity.* No portable
  way to ask, and the asymmetry makes guessing safe in one direction only — too
  large costs a content read, too small loses an edit. So the constant is sized
  for the coarsest filesystem we could plausibly be asked to watch (2 s), not
  the finest.

### The accepted cost: the flag is sticky

`mtime_racy` is cleared only by the next `record`/`touch`, never by the passage
of time — because ageing genuinely does not help: if the aliasing write already
happened, the file's mtime *equals* the recorded one permanently. So a file
recorded right after a save is content-compared on every check until the next
save.

Git accepts exactly this, smudging a racily-clean entry back to trusted only
when it rewrites the index. The equivalent refinement here — clear the flag from
`changed()` once a content comparison came back identical *and* the mtime has
outlived `MTIME_SETTLE` (at which point no future write can alias it) — is
sound, but forces `changed()` to take `&mut self`, which locks read-only UI code
out of asking. Deferred: `check_external_change` has no production caller yet,
so the poll rate that would justify the API cost is not merely unmeasured, it is
zero. Recorded in `known-issues.md` under the fixed entry.

### Consequences

- Any future mtime-based caching in this tree (a file indexer, a build-stamp
  check, a package-manager freshness test) inherits the same trap. The rule to
  carry over is that a timestamp is evidence of *absence* of change only once it
  is older than the clock's own granularity; before that it is evidence of
  nothing.
- The regression test deliberately contains no `sleep`. A sleep would separate
  the two writes into different ticks and hide the exact adjacency under test.

---

## §117 — Userspace randomness comes from the kernel, and fails closed

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Where:** `posix/src/random.rs` (new), `kernel/src/syscall/number.rs`
(`SYS_GETRANDOM` = 90), `kernel/src/syscall/handlers.rs::sys_getrandom`,
`posix/src/unistd.rs::{fill_random, getrandom, getentropy}`,
`posix/src/crt.rs::ensure_at_random_initialized`,
`posix/src/perthread.rs::PerThread::random`, `posix/src/process.rs::fork`.

### The problem

`getrandom()`/`getentropy()` were backed by a 64-bit LCG seeded from one
`RDRAND` draw, falling back to the monotonic clock. See
`known-issues.md` `TD-POSIX-GETRANDOM-WAS-AN-LCG-SEEDED-FROM-ONE-RDRAND-DRAW`
for why each of those three properties is independently fatal. The kernel
already ran a real ChaCha20 CSPRNG (`kernel/src/rng.rs`) with four entropy
sources; nothing exposed it.

Three decisions had to be made, and none of them is forced.

### 1. `SYS_GETRANDOM` is not capability-gated

Every other resource-touching syscall in this kernel goes through
`require_cap_type`. Randomness does not, for three reasons: there is no
resource another process can be deprived of (the CSPRNG is stateless from the
caller's point of view), there is nothing to authorise reading (the output is
by construction independent of anything the caller could otherwise not see),
and refusing it would push callers back onto a homebrew PRNG — which is the bug
we just fixed, re-created one library at a time. Linux, the BSDs and Fuchsia
(`zx_cprng_draw`) all reach the same conclusion. The one genuine cost is CPU
time, which the scheduler already bounds.

**Against:** it is a real exception to "no ambient authority", and a sandbox
that wants to make a process fully deterministic (for record/replay debugging)
now cannot. If that use case appears, the right answer is a per-process
*determinism* mode that seeds the pool from a fixture, not a capability.

### 2. No entropy source means failure, not best-effort bytes

If neither the kernel CSPRNG nor a hardware RNG answers, `getrandom`/
`getentropy` return `-1`/`EIO`, and `arc4random` and `AT_RANDOM` — which have
no error channel — abort the process.

**For:** this is precisely the bug being fixed. The old code's contract was
"always succeed", so a missing entropy source degraded silently into
predictable output that callers spent as if it were secret. A caller handed
`EIO` can fail closed; a caller handed predictable bytes cannot do anything at
all. OpenBSD takes the same position (its `arc4random` raises `SIGKILL`).

**Against:** a program that only wanted a temp-file suffix now dies where it
used to muddle through, and `getrandom` gains a failure mode Linux does not
have (Linux's pool is always seedable, so there is no ABI precedent for `EIO`
here — `getentropy`'s specified `EIO` is the closest thing, which is why both
calls report it). Accepted: on the OS target this can only fire if the kernel
is broken, and a broken kernel should not be papered over.

`GRND_INSECURE` deliberately does **not** override this. That flag means "do
not block waiting for the pool to initialise", not "make something up".

### 3. `arc4random` gets a userspace pool; `getrandom` does not

`arc4random` is expected to be cheap enough to call in a loop, so a syscall per
call would defeat it: it runs a ChaCha20 stream in userspace, seeded from the
kernel. `getrandom` goes straight to the kernel every time, matching Linux —
a caller asking for key material *by name* should not be served from a
userspace copy that outlives the call in this process's address space.

The pool lives in `PerThread`, so it needs no lock and two threads can never be
handed the same bytes; and it uses **fast key erasure** (256 bytes of keystream
per refill, the first 32 of which immediately replace the key and are never
emitted). The 12.5% throughput cost buys forward secrecy: an attacker who reads
the pool later cannot roll it backwards to bytes the process already used.

**The fork hazard.** A forked child inherits a byte-identical pool and would
replay the parent's stream, so a parent and child that each generate a "random"
session key generate the *same* one. `fork()`'s child branch bumps a
process-wide generation counter that invalidates every thread's pool.

*Alternative rejected:* comparing a cached pid on each call (LibreSSL's
approach). That is a syscall per `arc4random()`, which is the cost we built the
pool to avoid. *Also rejected:* `MADV_WIPEONFORK` on the pool page (glibc's and
OpenBSD's approach) — we have no such madvise flag, and adding one to serve a
single caller is a worse trade than a relaxed atomic load.

### Consequences

- `posix::random::secure_bytes` is now the single source of random bytes in
  libc. Anything that needs randomness — TLS, `mkstemp`, ASLR seeds, hash-table
  seeds — should call it rather than growing its own.
- `fill_random` returns `bool` and is `unsafe`; every caller must handle
  failure. There is deliberately no infallible variant.
- The host build (`cargo test` against the Windows triple) reaches the pool via
  RDSEED/RDRAND, because the raw `SYSCALL` instruction is gated off there. That
  path is cryptographically sound, not a stub — which matters, because it is
  the path all the unit tests exercise.
- `CPUID` is now probed before `RDRAND`/`RDSEED` is issued. The old code issued
  `rdrand` unconditionally, which is `#UD` on a pre-2012 CPU.

## §118 — The KASAN pre-shadow window is raw assembly, and the invariant is checked by a build gate rather than by review

**Date:** 2026-08-12

**Decided by:** Claude (operator-approved scope). §107 was the operator's call
to build the instrumented kernel; every decision below is a specific call made
while making it actually boot, and is mine to revisit.

**Context — the failure that forced this.** Between the kernel entry point and
the moment `mm::kasan::early_init` finishes installing the zero shadow there is
no shadow to read *and* no IDT to catch the resulting page fault. One
instrumented memory access in that window is a triple fault: QEMU resets, the
kernel prints **nothing at all**, and `boot-test.sh` reports the same "no
BOOT_OK" it would report for any other boot failure. Two of these were hit for
real before the gate below existed, each costing a `-d int,cpu_reset` run and a
symbol lookup to localize.

**The discovery that makes this non-obvious.** A module-level
`#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]` does **not**
establish that the module's code is uninstrumented. `sanitize` is a per-function
LLVM attribute, so it covers functions that are *ours*. Generic `core`
functions monomorphise into the kernel crate's codegen units, are emitted
out-of-line at `-O0`, and carry the default (instrumented) attribute — and they
dereference the pointers we hand them, so the shadow probe lands in `core`'s
frame where our exemption has no reach. The two real faults:

- `serial::init` takes a spinlock → `core::sync::atomic::atomic_compare_exchange_weak::<u8>`
  probes the shadow of `serial::SERIAL`.
- `for i in 0..512` hands `&mut Range<usize>` to
  `core::iter::range::RangeIteratorImpl::spec_next`, which probes the shadow of
  *this* frame's stack slot. (`-asan-stack=0` suppresses instrumentation of a
  function's own allocas — not of a pointer parameter that happens to point at
  a caller's alloca.)

Neither is visible by reading the source of the exempt module.

**Decision 1 — the window issues its own loads and stores via `asm!`.**
`mm::kasan::raw_load_u64` / `raw_store_u64` / `raw_shr_u64` are the audited
primitives; `limine::LimineRequest::<HhdmResponse>::offset_raw` and
`boot::hhdm_offset_early` are built on them, and the idempotency flag is a plain
`static mut EARLY_SHADOW_DONE: u64` read with `raw_load_u64` rather than an
`AtomicU64` (whose `load` is a `core` generic, hence instrumented).

*For:* inline assembly is opaque to LLVM, so it cannot be instrumented,
elided or reordered — the last property is what the previous `read_volatile`
was there for anyway. *Against:* it is more code than `*ptr`, and the safety
argument moves from the type system into `// SAFETY:` comments. Accepted
because there is no attribute that reaches into a monomorphised `core` generic;
keeping the access in our own frame is the only mechanism available.

**Decision 2 — debug arithmetic checks count as instrumented code.** In a debug
build `+` branches to `core::panicking::panic_const_add_overflow` and `>>` to
`panic_const_shr_overflow`, both instrumented panic machinery reachable from
the window. `wrapping_shr` is *not* a fix: it forwards to
`core::num::<u64>::unchecked_shr`, whose `ub_checks` precondition check is
itself a `core` generic that monomorphises in with instrumentation. So the
window uses `wrapping_add`/`saturating_add` for arithmetic and `raw_shr_u64`
(an `asm!` `shr`) for shifts, and `while` loops with plain counters instead of
`for`-over-`Range`.

**Decision 3 — `early_init` splits into two named phases.** `install_zero_shadow`
runs with no shadow and returns a `ShadowRoots`; `publish_shadow_roots` runs
after the TLB flush and does the ordinary atomic stores. `early_init` is a
two-statement wrapper.

*For:* the phase boundary was previously a comment in the middle of one
function, which no tool can reference. A call-graph walk cannot express "the
first half of this function", so the *code* draws the line instead, and the
gate below can name `install_zero_shadow` as a root. *Against:* it is a split
made for the benefit of tooling rather than for the code's own structure.
Accepted: the boundary is real regardless, and naming it also stopped the
gate's false positive on the post-shadow atomic stores.

**Decision 4 — the invariant is a build gate, not a review item.**
`scripts/kasan-check-preshadow.py` disassembles the built kernel, walks the
call graph breadth-first from the pre-shadow roots, and fails on any reachable
`__asan*` call or any indirect call it cannot prove exempt. `kasan-build.sh`
runs it immediately after the build. A `check_entry_order` pass separately
verifies that `kernel_main` still calls nothing but the allowed set before
`serial::init`, so inserting a new call ahead of the window's end is *reported*
rather than silently escaping the walk.

*For:* the whole point of the section above is that source review cannot
establish this property, and the failure mode gives you no evidence to work
from. Failing here costs a message; failing at boot costs a debugging session.
*Against:* it is a disassembly-based check, so it is coupled to symbol mangling
(`ROOT_SUBSTRINGS` carries v0 length prefixes such as `19install_zero_shadow`)
and needs updating when those functions are renamed. Mitigated by the root-not-
found and entry-order checks, which turn a stale list into a hard failure
instead of a silent pass.

**Where it lives.** `kernel/src/mm/kasan.rs` (the raw primitives, the phase
split, `early_translate`), `kernel/src/limine.rs` (`offset_raw`),
`kernel/src/boot.rs` (`hhdm_offset_early`), `kernel/src/main.rs` (the shadow
install is now statement zero of `kernel_main`, before `serial::init`),
`scripts/kasan-check-preshadow.py`, `scripts/kasan-build.sh`.

**How to reverse.** All of it is inert in the ordinary build: the raw
primitives compile to the same loads and stores, the phase split is a
refactor, and the checker exits 2 ("not an instrumented build") when handed a
kernel with no `__asan` symbols.

## §119 — KASAN checks are outlined, because inline checks cannot survive a kernel that dereferences user pointers

**Date:** 2026-08-12

**Decided by:** Claude (operator-approved scope). A follow-on to §118, made
while getting the instrumented kernel past its first user-mode fault; mine to
revisit.

**Context.** With the pre-shadow window fixed (§118) the instrumented kernel
booted 1298 serial lines — and then died on an unrecoverable kernel #GP inside
`core::ptr::write::<u64>`, reached from `idt::try_dispatch_user_exception`
writing the SEH `ExceptionContext` onto the faulting thread's **user** stack.
The faulting instruction was `cmpb $0x0, (%rax)`: LLVM's inline shadow compare.

**The mechanism.** Inline instrumentation computes
`shadow = (addr >> 3) + 0xDFFFE00000000000` and dereferences it
*unconditionally*, before anything has had a chance to decide whether the
address is one we shadow. For a kernel address that is fine. For a user address
it is not: `shadow(0x40_0000_0000) = 0xDFFF_E008_0000_0000`, whose bits 63:48
are `0xDFFF` while bit 47 is 1 — not sign-extended, therefore **non-canonical**,
therefore #GP. There is no offset that fixes this; a single-add mapping cannot
cover both halves of a 48-bit split address space, which is why Linux's KASAN
covers only kernel addresses too.

Linux gets away with inline checks because kernel code there *never*
dereferences a user pointer directly — every access goes through hand-written
`uaccess` asm that the compiler does not instrument. Our kernel does dereference
user pointers directly in several places by design (the SEH context above,
`mm::user`'s copy helpers, …), so the same guarantee does not hold, and each
such site is a kernel panic with a backtrace that points at the sanitizer rather
than at the bug being hunted.

**Decision.** Build with `-asan-instrumentation-with-call-threshold=0`, which
makes LLVM emit a **call** to `mm::kasan_rt::__asan_load8_noabort` & co. for
every checked access instead of the inline compare. The entry points already
existed (they were defined defensively for LLVM's 7000-access threshold); they
call `kasan::shadow_allows`, which runs `shadow_of` *first* and returns "no
shadow, therefore allowed" for any address outside the backed window — user
addresses included. The bad dereference is now unreachable by construction.

**For.** It is one rule that covers every site, present and future, instead of
an open-ended hunt for raw user derefs where each miss costs a full boot cycle
(~10 minutes) to find. It also makes the shadow lookup a single place where
policy can be added — the pre-shadow "is the shadow even installed yet"
question, address-space filtering, future quarantine integration — rather than
something baked into thousands of inline sequences. Linux offers exactly this
mode (`CONFIG_KASAN_OUTLINE`) for essentially the same reasons.

**Against.** A call per memory access is substantially slower than a
compare-and-branch, and the instrumented kernel was already `-O0` with a check
on every load and store. Accepted for the *correctness* goal: this is a debug
profile whose entire purpose is localizing one bug, and a boot that finishes
slowly beats a boot that panics in the sanitizer.

**Measured cost, and the consequence nobody costed up front.** The slowdown was
guessed at "roughly doubled" when this was written; measuring it gave something
far worse. A plain debug boot reaches `BOOT_OK` in **~283–318 s**
(`soak-20260723-190300`, 100/100 iterations). The instrumented debug boot ran
975 s to reach the point a plain boot reaches at line 4016 of 23532 — 17 % of
the log — with 48 of the 66 ring-3 spawn tests and 178 MB of the 217 MB of test
ELF still ahead of it. Both extrapolations (line rate, and remaining-ELF ratio)
land between **5500 s and 8500 s**, i.e. a **~20× slowdown**, not 2×. The first
attempt was launched with a 5400 s timeout on the strength of the 2× guess and
had to be abandoned at 17 % rather than yield a truncated answer.

For proving the instrumented kernel *boots*, ~2 h per boot is merely tedious.
For the thing this profile was built for it is disqualifying: B-KNULLJUMP fires
at roughly 1 boot in 120, so a soak that has an even chance of catching it needs
~80 boots — **over a week of wall-clock** at this rate, against ~7 h for the
plain-build soak that has been run repeatedly. So the cost is only "accepted"
for the single validating boot. Making the *hunt* viable needs a separate
decision (an optimized instrumented build being the obvious candidate, since
most of the 20× is `-O0` codegen rather than the sanitizer, but a release kernel
has never been booted here and optimization perturbs exactly the timing a rare
race depends on) — see `open-questions.md` Q43.

**The obligation it creates.** Every checked access now *calls* the runtime, so
nothing on the path from a check entry point down through `shadow_allows` may
perform an instrumented access of its own — it would call the check again,
unbounded, and appear as a stack overflow with no explanation. This is the same
invariant as §118's pre-shadow window with different roots, and it is enforced
the same way: `scripts/kasan-check-preshadow.py` now runs a second walk from the
`__asan_load*`/`__asan_store*` entry points. It caught a real violation on the
first run — `shadow_allows` calling `Option::<u8>::is_some`, a `core` generic
monomorphised into the kernel *with* instrumentation — which is why `byte_bad`
now returns a `u8` sentinel (`0` = addressable) rather than an `Option<u8>`, and
why `get_shadow` loads the mapped-frame bitmap and the shadow byte with `asm!`
and indexes with shifts and masks instead of `/` and `%`.

The *report* path is deliberately exempt from that rule and the walk stops
there: it formats and backtraces, far too much code to keep raw, and it does not
need to be. A report calls instrumented code, whose checks call `shadow_allows`,
which is clean and returns — one level of nesting, not a regress.

**Where it lives.** `scripts/kasan-build.sh` (the flag and its rationale),
`kernel/src/mm/kasan_rt.rs` (the check entry points are now the primary ones),
`kernel/src/mm/kasan.rs` (`get_shadow`, `byte_bad`, `shadow_allows`,
`raw_load_u8`, `raw_shl_u64`), `scripts/kasan-check-preshadow.py` (the
`RUNTIME_ROOT_PREFIXES` walk).

**How to reverse.** Drop the one flag. The check entry points stay defined and
unused, exactly as they were before, and the raw-`asm!` shadow lookup is
semantically identical either way — so reversing costs only the reappearance of
the #GP class this fixed.

## §120 — Deliberate access to poisoned memory goes through `mm::rawmem`, and its build gate judges accessors rather than reachability

**Date:** 2026-08-12

**Decided by:** Claude (operator-approved scope). The third and last of the
§107 escalation's "make the instrumented kernel actually boot" decisions;
mine to revisit.

**Context — the same hazard from a third root.** §118 and §119 recorded that a
module-level `#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]`
cannot exempt a generic `core` function, because the monomorphisation lands in
*this* crate carrying the default (instrumented) attribute. Both were found in
places where instrumentation is fatal for structural reasons: before the shadow
exists (triple fault), and underneath the check itself (unbounded recursion).

The third place is different in kind and was found the expensive way. A whole
class of our code exists to read and write bytes that KASAN has *deliberately*
marked inaccessible — `mm::heap`'s free-magic and redzone checks, `mm::poison`'s
fills and verifies, `mm::quarantine`'s parked slots. For them the access **is**
the detector, so a report on it is not a finding. Every one of those modules
carried the opt-out, and every one of them did its actual byte touching through
`core::ptr::{read_volatile, write_volatile, write_bytes}` — so the exemption was
cosmetic on precisely the operations it existed for. The first instrumented boot
to reach the self-tests died of it after ~2.7 hours, with
`report <- __asan_load1_noabort <- core::ptr::read_volatile::<u8> <-
mm::heap::check_redzone` (`known-issues.md` →
`B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS`).

**Decision 1 — one module of `asm!` accessors, not hand-rolled loops in each
exempt module.** `mm::rawmem` provides `read_u8`, `write_u8` and `fill_u8`
(`rep stosb`), all inline assembly, and every deliberate poisoned-memory touch
in `heap.rs`/`poison.rs`/`quarantine.rs` routes through them.

*For:* LLVM's AddressSanitizer pass does not instrument the memory operands of
inline asm, so the property holds regardless of whether the helper is inlined,
whose function body the access ends up in, or whether that caller is exempt —
it is enforced by the mechanism rather than by everyone remembering a rule.
*Against:* it is more code than `*ptr`, and it moves the safety argument out of
the type system into `// SAFETY:` comments. The considered alternative was to
hand-roll byte loops inside the exempt modules so no `core` generic is called.
Rejected: it works today and breaks silently the first time someone reaches for
a `core::ptr` helper inside an "exempt" module, which is a natural thing to do
and produces no warning. Volatility is preserved by omitting `nomem`/`readonly`,
since the call sites depended on it (a poison fill must not be dead-store
eliminated, a verify read must not be constant-folded).

**Decision 2 — the accessors get a boot self-test, not a `#[cfg(test)]` one.**
`rawmem::self_test` runs from `kernel_main` *before* the poison, KASAN and
quarantine self-tests.

*For:* the kernel binary cannot be built for the host harness at all
(`cargo test -p kernel --bin kernel` fails with a duplicate `panic_impl` lang
item), so this matches the existing `mm::poison::self_test` convention. Ordering
is the substantive part: these three helpers are the foundation the poison,
redzone and quarantine checks all stand on, so if a hand-written `asm!` store
had the wrong operand size or direction, every downstream "OK" would be
meaningless. *Against:* it costs boot time in every build, instrumented or not.
Accepted — it is three buffers and a `rep stosb`.

**Decision 3 — the gate's third walk judges *which* function is instrumented,
not whether it is reachable.** `scripts/kasan-check-preshadow.py` grew a third
root set, but deliberately **not** the violation rule the other two use.

Walks 1 and 2 flag every instrumented function they can reach, and that is
right for them: before the shadow exists, or underneath the check, an
instrumented access is fatal no matter what memory it touches. Applying the
same rule here produced 500-odd "violations" on its first real run, every one
of them an `AtomicUsize::fetch_add` on a stats counter, a `Range::next` on a
loop variable, or a `SpinMutexGuard` deref on the serial port — ordinary live
kernel objects, where instrumentation is correct and even desirable, since it
is how a bug in the memory debugger itself would surface.

So `walk_poison` judges exactly two things: a root's own body (an `__asan_*`
call there means the module's opt-out is not in force) and the raw-pointer
accessors it reaches, which is the §118/§119 hazard itself. 500 hits became 2,
and both of those turned out to be real information about the walk rather than
about the kernel (see below).

Getting the accessor list right needed one non-obvious addition. `core::ptr` and
`core::intrinsics` look like they cover the ground, but `core::intrinsics` emits
only `rotate_left`/`rotate_right` in this binary — pure register operations —
because `write_bytes` and `copy_nonoverlapping` are lowered by LLVM straight to
`memset`/`memcpy` calls rather than to callable monomorphisations. A bulk fill
aimed at poisoned memory therefore appears in the call graph as a plain
`call memset`, which is the single most likely way to write this bug and which
both patterns would have missed. The three `mem*` builtins are matched by exact
name.

One consequence is worth stating rather than leaving to be rediscovered: after
the `rawmem` conversion, *zero* accessors of any kind are reachable from the
poison roots, because the byte touching is all inline `asm!`, which emits no
call. The accessor branch therefore judges an empty set today and passes because
there is nothing to judge — the intended state, but indistinguishable by exit
code from a check that has quietly stopped working. The gate's OK line reports
the accessor count next to the reachable count so the two cannot be confused.
The real work every run is done by the root-body branch (all thirteen roots
verified uninstrumented); the accessor branch is a tripwire for reintroduction.

*For:* a gate that reports 500 non-problems does not get 500 exemptions, it gets
rubber-stamped as noise, and the one real signal goes with it. Precision here is
not tidiness, it is whether the check survives contact with a reader.
*Against:* it can miss a poisoned-memory access made some way other than through
a raw-pointer accessor, and it can over-report a `read_volatile` aimed at MMIO.
The over-report direction is cheap (a `rawmem` call or a documented exception);
the under-report direction is bounded by the fact that a plain `*ptr` in an
exempt module *is* covered by the module attribute — the generics are the entire
residual gap, and they are what is checked. `core::slice` is deliberately out of
scope: slice helpers are used on ordinary memory throughout these modules, so
including them would reintroduce the false positives above, and poisoned memory
is reached here through raw pointers regardless.

**Decision 4 — a cut that matches nothing is a hard failure.** The walks stop
at cold reporting machinery (the serial printer, the panic path, the KASAN
reporter), which is reached only *after* a violation has been found and is far
too much code to keep clean. Those cut lists are now validated against the
binary, exactly as the root lists already were.

*For:* this is not hypothetical. `'6serial6_print'` was one underscore short of
the real symbol — v0 escapes the leading `_` of `_print`, encoding it as
`6__print` — so the cut matched nothing, and a cut that matches nothing does not
stop the walk. It ran on through the printer into the APIC MMIO accessors and
reported `read_volatile` there. Both of the two surviving "violations" above had
that single cause. A dead root fails safe (the walk covers less than you think
and says so); a dead cut fails *unsafe*, by burying the real signal in noise
from code the check never meant to cover. *Against:* it couples the script even
harder to symbol mangling. Accepted for the same reason §118 accepted it: a
stale list becomes a hard failure instead of a silent pass.

**Where it lives.** `kernel/src/mm/rawmem.rs` (new), the converted call sites in
`kernel/src/mm/{heap,poison,quarantine}.rs`, `kernel/src/main.rs` (the self-test
call, ahead of `mm::poison::self_test`), `scripts/kasan-check-preshadow.py`
(`POISON_ROOT_SUBSTRINGS`, `POISON_ACCESSOR_SUBSTRINGS`, `walk_poison`).

**How to reverse.** Inert in the ordinary build: the `asm!` accessors compile to
the same loads and stores a `read_volatile` would, and the gate exits 2 ("not an
instrumented build") when handed a kernel with no `__asan` symbols.

---

## §121 — DF is cleared at the gate and again at the `rep` instruction, and a re-entrant print garbles rather than blocks

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Two small tradeoffs settled while fixing
`B-NO-CLD-ON-INTERRUPT-ENTRY`; mine to revisit. The bug itself had no decision
in it — a missing `cld` on interrupt entry is simply wrong, and there is no
case for the other side.

**Context.** No IDT stub cleared the direction flag. An IDT gate clears TF, NT,
RF and VM but leaves DF exactly as the interrupted context had it (Intel SDM
Vol. 3A §6.12.1), so ring 3 could hand the kernel DF = 1 with a one-byte `std`
and every `rep`-string operation — including every compiler-emitted
`memset`/`memcpy` — would then write *before* its intended destination instead
of at it. The SYSCALL half was already covered by `IA32_FMASK` bit 10. Details,
evidence and the B-KNULLJUMP hypothesis are in `known-issues.md`.

Three questions did have two defensible answers.

**Decision 1 — belt *and* braces: `rawmem::fill_u8` clears DF itself, even
though entry now guarantees it.**

*For:* the helper stops depending on an invariant established somewhere else
entirely. `fill_u8` is a memory-*debugging* primitive: it runs in the exact
situations where other invariants are already suspect, and a backwards fill
would corrupt the bytes *before* a redzone — which is to say, it would
manufacture exactly the kind of corruption the surrounding subsystem exists to
detect, and blame it on someone else. Clearing DF can only ever repair state,
never damage it, because DF = 0 is what all compiled code requires anyway; so
the redundancy carries no risk of its own. The cost is one instruction on a path
that is not hot.

*Against:* it is genuinely redundant now, and redundancy invites the reader to
wonder which of the two mechanisms is the real one — the failure mode §118 kept
hitting, where a defence that looks sufficient is not. Mitigated by saying so
explicitly at both sites: the `idt.rs` comment names `rawmem::fill_u8` as a
dependant, and the `fill_u8` comment says entry already clears DF and that this
is deliberate belt-and-braces.

*Consequence:* `options(preserves_flags)` had to come off `fill_u8`'s `asm!`
block, since it now writes DF. Negligible — the compiler just reloads flags it
almost never has live across a bulk fill.

**Decision 2 — a re-entrant print garbles the output rather than dropping the
line or blocking.**

The console lock cannot be taken twice by one CPU, and `cli` does not help
because exceptions ignore it. Three options:

- **Block (the old behaviour).** Deadlock. Not really an option; it is the bug.
- **Drop the nested line.** *For:* output stays clean and parseable, and the
  boot-log greps in `boot-test.sh` keep working unchanged. *Against:* it drops
  precisely the highest-value line in the log. Nested prints are not routine —
  a print inside a print means a fault fired during a fault report, which is the
  most interesting event in the entire boot.
- **Chosen: write the nested line through the lock-free emergency port,
  interleaved.** *For:* nothing is lost. The wedge that motivated this
  (`B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT`) is a case where
  the operator needed one token — the faulting RIP — and the failure mode
  deleted it. *Against:* a line can now be cut in half by another line, so any
  future log parser must tolerate that. Accepted: interleaving happens only when
  something has already gone wrong, so it degrades the pretty case never and the
  broken case from "silence" to "readable but ugly".

**Decision 3 — claim the per-CPU re-entrancy flag before taking the lock, not
after.**

*For:* setting it after acquiring leaves a window — one instruction, but real —
where the CPU holds the lock and a nested exception would not recognise that,
and would block forever. The whole point is to have no such window. *Against:*
during the interval where this CPU is merely *waiting* for the lock, the flag
claims it is "inside" when it is not. That turns out to be the correct reading
anyway: the flag's meaning is "this CPU is somewhere inside `_print`", which is
exactly the condition under which a nested call must not block. And it is
per-CPU, so no other CPU's view is affected.

**Where it lives.** `kernel/src/idt.rs` (the `cld` in all three `global_asm!`
stub macros, the "`cld` on entry" comment, `df_on_entry_self_test`, and the
`BP_ENTRY_DF` observation in `handle_breakpoint`), `kernel/src/mm/rawmem.rs`
(`fill_u8`), `kernel/src/serial.rs` (`_print`, `IN_PRINT`,
`reentrancy_self_test`), `kernel/src/main.rs` (both self-test calls).

**How to reverse.** Each piece is independent. Removing `fill_u8`'s `cld`
(decision 1) leaves the entry `cld` load-bearing and is safe as long as it stays
— restore `preserves_flags` if so. Decisions 2 and 3 are confined to `_print`;
reverting to the blocking form restores the deadlock, so it should only be done
alongside some other escape. The entry `cld` itself should not be reverted.

---

## §122 — SYSCALL masks the full Linux RFLAGS set, but SMAP stays off behind a self-verifying gate rather than shipping an unconditional `clac`

**Date:** 2026-08-12
**Decided by:** Claude (autonomous)

**Context.** Fixing `B-NO-CLD-ON-INTERRUPT-ENTRY` (§121) established that RFLAGS
the kernel inherits from ring 3 at an entry gate is attacker-controlled state.
Auditing the rest of that class turned up `EFLAGS.AC`, which is the SMAP
override and is settable from ring 3 with an unprivileged `popfq`. An IDT gate
does not clear it (measured, not assumed — see
`B-AC-INHERITED-AT-KERNEL-ENTRY`). Three sub-decisions followed.

### 1. Widen `IA32_FMASK` to Linux's full set, not just the bits with a known bug

`FMASK` was `TF | IF | DF`. Adding `AC` alone would have closed the specific
hole found. Instead it now masks what Linux's `MSR_SYSCALL_MASK` masks: AC, NT,
IOPL, RF, ID and the arithmetic flags as well (`0x257fd5`).

*For the narrow fix:* only change what you can justify; every extra masked bit
is a behaviour change on the syscall hot path, and masking arithmetic flags in
particular fixes no known bug.

*For the wide fix (chosen):* the cost is exactly zero — FMASK is applied by the
CPU as part of the instruction, so masking fifteen bits is no slower than
masking three. And the audit that found AC found it only because someone went
looking; NT (a stray `NT` makes a later `iret` attempt a task-switch return) and
IOPL are the same shape of latent problem. Enumerating "which inherited flags are
harmless" is a standing obligation to re-audit on every future change, whereas
masking everything not needed is a one-time decision that stays correct. Linux
reached the same conclusion after its own history of entry-flag bugs, which is
decent evidence about where this ends up.

### 2. Do not put an unconditional `clac` in the ISR stubs

The symmetric fix to §121's `cld` would be a `clac` beside it. Rejected: `clac`
#UDs when CPUID.SMAP is absent, so this would fault at the first interrupt on
any pre-Haswell / pre-Zen CPU — turning a latent hardening gap into a total boot
failure on older hardware. Linux alternatives-patches `ASM_CLAC` to a NOP on such
CPUs; we have no patching framework, so the honest position is that this fix is
*blocked on infrastructure*, not merely unwritten. The alternatives are recorded
in `known-issues.md` rather than half-implemented.

A `pushfq`/`and`/`popfq` sequence would work everywhere with no patching, and was
rejected on cost: ~20+ cycles versus `clac`'s ~2, on every single interrupt. That
is a real, permanent tax on the hottest path in the kernel to fix a bug that is
currently latent (SMAP is off). If a cheap alternatives framework proves
impractical, this becomes the fallback.

### 3. Gate CR4.SMAP on a constant, and make the constant self-verifying

The failure mode being defended against is specific: someone finishes the
STAC/CLAC instrumentation, enables SMAP, and gets a protection that reports
itself ACTIVE while enforcing nothing, because AC is still inherited. Nothing
crashes and no test goes red — the §118 failure mode exactly.

*Rejected — a comment.* The existing code already had a comment explaining why
SMAP was deferred, and it named only the STAC/CLAC prerequisite. The AC one had
never been noticed. Comments do not survive being unread.

*Chosen — a gate plus a cross-check.* `smap_enable_blocker()` refuses to set
CR4.SMAP while `ENTRY_PATHS_CLEAR_AC` is false, and
`idt::ac_on_entry_self_test()` measures what an IDT gate actually does with AC
and asserts it against that constant. The cross-check is what makes this more
than bookkeeping: the constant cannot drift from reality in *either* direction.
Flipping it without adding `clac` fails the boot test; adding `clac` without
flipping it also fails, so the mitigation cannot sit unused after being written.

The cost is a self-test that asserts a currently-*broken* property is broken,
which reads oddly. That is deliberate: it asserts agreement between the code's
belief and the hardware, not that AC is clear, so it keeps passing unchanged
through the eventual fix and only fails if the two disagree.

**How to reverse.** Decision 1 is one literal in `syscall::entry::init()`; the
old `TF|IF|DF` value still boots. Decisions 2 and 3 are the same change viewed
from two sides — deleting the gate re-enables SMAP the moment `features.smap` is
true, which is precisely the silent failure, so the gate should outlive the
`clac` work rather than be removed with it.

**Update (2026-08-13).** Both prerequisites are now met and CR4.SMAP is on.
Decision 2's blocker was resolved by §123's alternatives framework (the ISR
stubs carry a NOP that is patched to `clac` only when CPUID reports SMAP), and
the STAC/CLAC instrumentation finished with
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE`. Decision 3 held up
exactly as intended and is kept: the gate now reads `None`, but it stays as the
one documented switch for turning SMAP back off, and the self-test was rewritten
to assert CR4 and `SMAP_ENABLED` agree with `smap_enable_blocker()` in *both*
directions rather than asserting SMAP is deliberately off — the same
"agreement, not a fixed answer" property, now checked from the other side.

Worth recording what the enforcement caught the moment it went live: a raw
`core::ptr::write` of the SEH exception frame onto the user stack in
`idt::try_dispatch_user_exception`, which three separate greps and two careful
readings had missed because it derives its user address from the ring-3
interrupt frame's RSP rather than from a syscall argument — and which, checked
properly, turned out to be an arbitrary kernel write available to any process
(`B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP`). That is the strongest
available argument for decision 3's premise: an instrumentation claim is not
verifiable by inspection, so the gate had to be flipped and tested, not
reasoned about.

---

## §123 — Boot-time code patching writes `.text` through its HHDM alias, not by clearing `CR0.WP`

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

**Context.** `clac`/`stac` raise #UD on a CPU without CPUID.SMAP, so the `clac`
that `B-AC-INHERITED-AT-KERNEL-ENTRY` requires at the top of every ISR stub
cannot simply be assembled in unconditionally. The standard answer — Linux's,
and now ours in `kernel/src/alternatives.rs` — is to reserve a 3-byte NOP at
each site and overwrite it at boot iff CPUID says the feature exists. That
keeps the hot path branch-free, but it means the kernel must write to its own
`.text` exactly once, early.

The first implementation just stored to `entry.site` directly, reasoning that
`.text` is not made read-only until `mm::protect::harden_kernel_sections()`
runs, thousands of lines later in `kmain`. That was wrong, and the boot test
said so immediately:

```
EXCEPTION: Page Fault (#PF) at 0xffffffff81efcb87, address=0xffffffff8110aaad, error=0x3
```

`error=0x3` is present + write: a write to a read-only page. `.text` is
read-only from the *first instruction* — `linker.ld` gives it a `PT_LOAD` with
`FLAGS(R|X)` and Limine honours that. `harden_kernel_sections()` re-asserts
W^X; it does not establish it.

**Decision.** Patch through a *second, writable mapping* of the same physical
pages — the HHDM, which Limine already provides read-write over all of physical
memory. `boot::executable_address()` (a newly-added Limine
executable-address request) gives the kernel's physical load base, so a
`.text` virtual address converts to its writable alias by a single constant
offset, with no page-table walk. The executable mapping is never modified.

**Alternatives considered.**

1. **Clear `CR0.WP` around the patch loop.** The obvious trick, and the one
   most hobby kernels reach for.
   - *For:* three instructions, no dependency on the HHDM or on knowing where
     the kernel was loaded, works before any memory subsystem exists.
   - *Against:* `CR0.WP` is not scoped to the patch — it disables write
     protection for *this entire CPU* for the duration, so any unrelated stray
     write anywhere in that window silently corrupts read-only kernel memory
     instead of faulting. It also creates a genuine W^X hole (briefly, all of
     `.text` is writable *and* executable), which is exactly the property
     `audit_kernel_wx()` exists to prove we never have. Rejected.

2. **Temporarily `change_flags()` the site's page to writable, then restore.**
   - *For:* narrowly scoped to the pages actually being patched.
   - *Against:* still makes a page W+X for a window, still needs a TLB flush
     per site, and — decisively — needs `mm::page_table` initialized, which it
     is not this early (`page_table::init()` is ~100 lines further down
     `kmain` than `alternatives::apply()`). Moving the patcher later would
     mean every alternative stays at its default for that stretch of boot.
     Rejected.

3. **Write through the HHDM alias.** Chosen.
   - *For:* the executable mapping is never writable, not even transiently, so
     W^X holds throughout and the `CR0.WP` blast radius does not exist. Needs
     nothing but two Limine responses, so it works arbitrarily early — the
     patcher is free to run before *any* memory subsystem is up, which is what
     lets it precede `smep_smap::init()`. It is also what Linux's `text_poke`
     does, minus the temporary-mm machinery we do not need with one CPU
     running.
   - *Against:* depends on the bootloader answering two requests, and on the
     kernel image being physically contiguous (Limine guarantees this). Both
     are checked at runtime; failure logs and patches nothing.

**Consequence for the SMAP gate.** Because patching can now fail for reasons
other than "the CPU lacks the feature" (no HHDM, no executable-address
response, a malformed table), `has_run()` is too weak a signal to gate SMAP on:
the patcher could run to completion having installed nothing, and a caller
checking only "did it run?" would enable SMAP with no `clac` anywhere. So
`apply()` also exports `all_supported_sites_patched()`, set only if every site
whose feature is present was actually rewritten, and `smep_smap` gates on that.
This is the same fail-closed principle as §122.

**On x86 cache coherency.** Writing via one mapping and executing via another
needs no explicit flush: caches and the instruction-fetch unit are coherent
over *physical* addresses. The `cpuid` serialization after the patch loop is
still required, but for the separate reason that a stale prefetch of the
rewritten bytes may already be in flight (SDM Vol. 3A §8.1.3).

**How to reverse.** The alias arithmetic is confined to `TextAlias` in
`alternatives.rs`; swapping in the `CR0.WP` approach means replacing
`TextAlias::writable()` with the identity and bracketing the loop. Do not — it
would reintroduce the W^X window this decision exists to avoid.

---

## §124 — Untrusted return state is rejected at the point it is supplied, and the check lives in one shared place

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

**Context.** Three syscalls do not return to their caller — they overwrite the
saved return frame so the SYSRET path resumes elsewhere:
`sys_exception_return_with_frame`, `sys_signal_return_with_frame`, and Linux
`rt_sigreturn`. All three took RIP, RSP and RFLAGS out of a userspace structure
and installed them unchecked (`B-FRAME-REWRITING-RETURNS-INSTALLED-UNSANITISED-USER-STATE`).
Two further paths — `idt::try_dispatch_user_exception` and the signal-delivery
builder — write a frame *to* a user address taken from the saved ring-3 RSP,
which had the same missing check
(`B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP`). Fixing them raised three
questions with real arguments on both sides.

### 1. Reject a bad handler at registration, or let delivery fail like Linux

Linux accepts any `sa_handler` from `rt_sigaction` and only kills the process
when delivery faults (`force_sigsegv`). We reject a handler at or above
`USER_SPACE_END` in `sys_signal_register`, `sys_set_exception_handler` and
`sys_rt_sigaction`.

*For the Linux behaviour:* it is the compatible one, and it needs no check on
the registration path at all — delivery has to handle a bad address anyway
(the page can be unmapped after registration), so the registration check is
strictly redundant for *safety*. Two checks where one suffices is the kind of
duplication that later drifts.

*For rejecting early (chosen):* the two checks are not the same check. Delivery
can only answer "this address did not work"; registration can answer "this
address can never work", and it can say so at the point the caller made the
mistake, with an `EFAULT` the caller can act on. A process that registers a
kernel address is not going to be helped by dying silently thousands of
instructions later. And the delivery-side check remains, so this adds a
diagnostic, not a dependency — deleting the registration check would leave the
system safe, merely less debuggable. Nothing in-tree registers a kernel address,
so the compatibility cost is hypothetical.

### 2. Test `< USER_SPACE_END`, not canonicality

The classic bug here (CVE-2012-0217) is a *non-canonical* RIP reaching `sysretq`,
which loads RIP from RCX while still at CPL 0 — so the #GP lands in ring 0. Our
entry stub makes that worse than the original: it does `mov rsp, gs:[8]` and
`swapgs` before `sysretq`, so the ring-0 fault handler would run on an
attacker-influenced stack with the user's GS base.

*For a canonicality test:* it is the precise statement of the hardware hazard,
and it is what the CVE is about.

*For `< USER_SPACE_END` (chosen):* it is strictly stronger — it rejects every
non-canonical address *and* every canonical kernel one. A canonical kernel RIP
is not a `sysretq` hazard, but installing one is never a legitimate request from
ring 3, and the same predicate then serves the frame-*writing* paths, where a
canonical kernel address is exactly the dangerous case. One predicate that is
right for both directions beats two that each cover half.

### 3. Mask RFLAGS to an allowlist, not a denylist of known-bad bits

`USER_RFLAGS_MASK = 0x0024_0DD5` keeps CF/PF/AF/ZF/SF/TF/DF/OF/AC/ID and drops
everything else; `USER_RFLAGS_FORCED = 0x0000_0202` puts IF and the reserved
bit-1 back. This is the same allowlist-over-denylist call as §122's FMASK
widening, reached for the same reason: the bits that hurt (IOPL=3 gives ring 3
direct I/O-port access; NT corrupts a later `iret`; VM enters a mode nothing
handles; a cleared IF wedges the CPU) were found by enumeration, and enumeration
is a standing obligation to re-audit. An allowlist is a one-time decision.

`linux.rs` already had a local `SIGRETURN_RFLAGS_USER_MASK` doing this correctly
for one of the three paths. It was deleted rather than copied: three private
copies of a security predicate drift, and the one that drifts is the one nobody
is looking at. The constants and both predicates now live in
`kernel/src/syscall/entry.rs`, next to the SYSRET path they protect.

**How to reverse.** The predicates are two small functions in `syscall/entry.rs`;
loosening the policy means widening `USER_RFLAGS_MASK` or relaxing
`user_return_state_ok`, in one place. Do not delete the registration-side checks
and the return-side checks together — the return-side ones are load-bearing.

---

## §125 — The POSIX `TZ` engine is a shared crate, and osh takes its zone from the *exported* `TZ`

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

**Context.** The libc had no timezone support at all: `localtime_r` was
`gmtime_r`, `mktime` was `timegm`, and `tzset`/`timezone`/`daylight`/`tzname`
were stubs. Separately, `userspace/oils` renders broken-down time *itself* —
`printf '%(FORMAT)T'` and the `\d \D{…} \t \T \@ \A` prompt escapes do their own
`epoch.div_euclid(86_400)` and hardcoded `%z`/`%Z` to `+0000`/`UTC` — because
osh never calls `strftime`. Two independent renderers of the same quantity,
both wrong in the same direction.

### 1. One engine in a shared crate, not a `posix` module

The obvious plan was to write the `TZ` parser inside `posix/src/tz.rs` and be
done. That would have fixed `localtime_r` and left osh alone.

*Against the shared crate:* it is a fourth `no_std` leaf crate
(`netproto`, `netipc`, `netring`, now `tzrules`), it must build for
`x86_64-unknown-none` *and* `x86_64-slateos`, and it has to be added to the
workspace `exclude` list with a comment explaining why — real, if small,
structural cost for what is ~900 lines of pure calendar arithmetic.

*For it:* fixing only the libc would have been **worse than fixing neither.**
Today both are UTC, so `date`, a C program's `localtime`, `printf '%(%T)T'` and
`PS1='\t'` at least agree with each other. Fix one and they disagree — and the
shell is precisely where a user would notice and mistrust the answer. A
disagreement about what time it is between the shell and every C program on the
machine is not a bug you can leave open; it is the kind that makes people
distrust the clock and work around it. Two implementations of one rule diverge
the moment either is touched, so the only durable answer is that there is one
implementation. Chosen.

The same argument had already been made twice in this tree, for `netproto`
(kernel + netstack daemon must parse the same Ethernet frame) and `netipc`
(both ends of the control channel must agree on the schema). `tzrules` is the
third instance of the same shape, and its module doc says so.

### 2. osh resolves the zone from `TZ` *only when it is exported*

This looks like a shortcut — reading a `HashSet` membership instead of doing
what a libc does — but it is bash's actual rule, and copying it is what makes
osh match. `variables.c:sv_tz`:

```c
v = find_variable (name);
if (v && exported_p (v)) array_needs_making = 1;
else if (v == 0)         array_needs_making = 1;
if (array_needs_making) { maybe_make_export_env (); tzset (); }
```

bash renders time through `strftime`, which reads the *process* environment,
and the only thing that puts a shell variable into that environment is the
export attribute. So `TZ=EST5 printf '%(%Z)T'` with no `export` genuinely does
print `UTC` in bash. `Shell::shell_tz` reproduces that exactly.

*Against:* it is surprising, and a user who writes `TZ=EST5` and sees UTC will
think the shell is broken. A "helpful" reading of any assigned `TZ` would look
more correct to that user.

*For:* the surprise is bash's, and diverging from it would break scripts that
rely on the distinction; the common paths (an inherited `TZ`, `export TZ=…`,
and an assignment *prefix* like `TZ=EST5 printf …`) all work, so the surprising
case is rare in practice. It also has a pleasant side effect: a unit-test shell
has never imported an environment, so nothing is in `exported` and the whole
suite renders in UTC on every host — the determinism the test corpus needs
falls out of bash's rule instead of being a carve-out from it, which was the
blocker the original bug report named ("any case that does not pin `TZ` is
nondeterministic across machines, so the corpus cannot simply stop pinning").

### 3. An unparseable `TZ` is UTC, silently

`Tz::parse` understands the POSIX grammar only. A zoneinfo name
(`America/New_York`) fails to parse and falls back to UTC, which is what glibc
does when it can find no matching tzfile — but glibc usually *can* find one,
and we have no tzdata at all, so the fallback fires where glibc would have
succeeded.

*Against:* silence. The user selected a zone and got a different one with no
diagnostic. A warning would at least be honest.

*For:* the libc has nowhere to warn to (`tzset` returns `void` and a libc that
prints to stderr on its own is worse than one that is quiet), and osh warning
where bash does not would be a gratuitous divergence. POSIX also specifies UTC
as the fallback for an unusable `TZ`, so this is the conforming answer even if
it is not the helpful one. Recorded instead as
`known-issues.md TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`, with tests in both
crates that assert the current behaviour so the day it changes is loud.

**How to reverse.** Adding a TZif reader to `tzrules` closes §3 without
touching either consumer — `Tz::parse` stays as the tail rule for times past a
file's last transition, which is exactly how TZif v2+ footers work. §2 is one
function (`Shell::shell_tz`). §1 is the one to keep: merging `tzrules` back
into `posix` would re-create the divergence it exists to prevent.

---

## §126 — A path is bytes; the VFS API is a `Path`/`PathBuf` newtype, and the JSON change-journal carries the exact bytes in a companion `_hex` field

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

### The problem

`design.txt` says the filesystem is case-sensitive, uses `/` as separator, and
allows **every** character except `/` and NUL. That makes a path a *byte
string*. The VFS was nevertheless written against `&str`/`String`, which
carries a UTF-8 invariant the filesystem does not have. Everywhere the two met,
the code did `from_utf8_lossy` or `from_utf8(..).unwrap_or("")`, and every one
of those was a latent bug rather than a cosmetic one:

- `cap/file_tags.rs` degraded a non-decodable tagged path to `""`. The lookup
  key always begins with `/`, so it could never match `""` — the tag was
  registered, counted, and **never enforced**. A fail-open access-control bug.
- `ext4/driver.rs` *skipped* directory entries whose names were not UTF-8, so
  such a file was invisible to `readdir` and its parent directory could never
  be `rmdir`ed.
- `fs/index.rs` folded case with `from_utf8_lossy`, collapsing two names that
  differ only outside ASCII into the same search key.

### Decision 1 — a `Path`/`PathBuf` newtype over `[u8]`/`Vec<u8>`

Rather than pass `&[u8]` around, `kernel/src/fs/path.rs` defines
`Path(#[repr(transparent)] [u8])` and `PathBuf(Vec<u8>)` mirroring `std`'s API
surface (`components`, `file_name`, `extension`, `parent`, `join`, `push`,
`starts_with`, `is_absolute`).

*For:* it gives the *path* operations a home, so the component-boundary rule
lives in one place (`Path::starts_with`) instead of being open-coded — the
open-coded form (`p.starts_with(prefix) && p.as_bytes().get(prefix.len()) ==
Some(&b'/')`) **fails open** whenever `prefix` ends in `/`, and had already
done so in `fs::intercept` (a deny rule that permitted everything),
`fs::integrity` and `fs::findex`. It also makes the type system enforce the
distinction between "these bytes are a path" and "these bytes are file
contents".

*Against:* it is a large mechanical change (~600 compile errors at peak) and a
newtype the rest of the kernel has to learn.

Two deliberate omissions, both copied from `std`:

- **No `Display`.** `{}` on a path would silently do a lossy conversion at
  every log site, and a lossily-rendered path can never be fed back into a
  lookup. `Path::display()` must be written out, which makes each lossy
  conversion a visible, reviewable choice. (`Debug` *is* implemented, so
  `assert_eq!` still works.)
- **No `PartialEq<&str>`.** Comparing against a literal must be spelled
  `x.as_path() == Path::new("lit")`, which keeps the byte comparison explicit.

`PathBuf::push` clears the buffer when the pushed piece is absolute, also
matching `std`. That is *exactly* the "an absolute symlink target discards the
parent directory" rule, so the three-way branch that `memfs::resolve_path_str`
and `resolve_write_path` each carried collapsed to a single `push`.

### Decision 2 — `impl AsRef<Path>` on inherent functions, `&Path` on the `FileSystem` trait

Inherent and free functions (`Vfs::open`, `index::add_entry`,
`history::record_version`, …) take `impl AsRef<Path>`; the object-safe
`FileSystem` trait takes `&Path`.

*For:* the `impl AsRef<Path>` form keeps the several thousand existing
`&str`-literal call sites compiling unchanged, which is what made the
conversion tractable at all. `dyn FileSystem` cannot have generic methods, so
the trait has no choice.

*Against:* the split is a wart — two spellings for the same concept, and a
reader has to know which side of the line a given function is on. A uniform
`&Path` everywhere would be cleaner but would require touching every literal.

### Decision 3 — the JSON change journal writes `path` **and** `path_hex`

`fs/journal.rs` persists to `/_JOURNAL` as JSON-lines, mandated by design.txt's
"No binary logs. Text-based (JSON-lines) structured logging." But a JSON string
must be valid Unicode and a path need not be. Each path is therefore written as
up to two fields: `"path"` (always present, the lossy U+FFFD rendering) and
`"path_hex"` (present **only** when the path is not valid UTF-8, lowercase hex
of the exact bytes). Renames use `"from"`/`"from_hex"`. A malformed `_hex`
field rejects the entry rather than falling back to the lossy field, because
the lossy field names a different file.

Alternatives considered:

- **Surrogate escapes (`\udcXX`, PEP 383 / Python's `surrogateescape`).** One
  field, unambiguous (a lone low surrogate cannot arise from valid text), and
  RFC 8259 permits the syntax. *Rejected* because lone surrogates are
  ill-formed Unicode and strict parsers silently repair them — Go's
  `encoding/json` replaces them with U+FFFD, so a Go log consumer would corrupt
  the path with no error.
- **Lossy only.** Simplest, and correct for ~every real path. *Rejected*
  because the journal is a *replay* log for backup tools, not a human log; a
  U+FFFD in it is a path that cannot be restored.
- **Base64 instead of hex.** More conventional and 33% smaller. *Rejected* for
  hex only because the encoder/decoder is a third the size and has no padding
  rules to get wrong, and the field is emitted for a vanishing fraction of
  entries so its size does not matter.

*For hex-companion:* works with every JSON parser, never emits ill-formed
Unicode, self-describing (the presence of `_hex` is the signal that `path` is a
rendering), and costs nothing in the UTF-8 case — a journal of ordinary paths
is byte-identical to what this module produced before, which a self-test now
asserts.

*Against:* two fields for one datum; a naive consumer that reads only `path`
gets a rendering and does not know it. The `_hex` name is at least loud enough
that a consumer reading the format will notice it.

### How to reverse

Decision 1 is the one to keep — it is the whole point. Decision 2's split can
be collapsed to uniform `&Path` later by a mechanical pass over the call sites
once the churn has settled. Decision 3 is confined to `json_push_path` /
`json_extract_path` in `fs/journal.rs`; switching to base64 or to surrogate
escapes is a two-function change plus a format-version bump.

---

## §127 — An unhandled ring-3 fault kills the whole process, and `SYS_THREAD_JOIN` moves its exit value to an out-pointer

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

### The problem

A 40-boot soak caught a `pthread_create`d thread starting at a garbage RIP and
being killed by the page-fault handler (known-issues.md
`B-PTHREAD-CHILD-JUMPS-TO-GARBAGE`). The interesting part was not the crash but
what the program then *printed*:

```
captured: SLATE_GLIBC_PTHREAD_OK counter=30000 joinsum=9
expected: SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10
```

The process survived one of its threads being killed, the survivors finished,
and `main` produced a plausible-looking wrong answer and exited 0. A test that
asserted only "the binary exited cleanly" would have passed. Two mechanisms
conspired: `kill_userspace_task_with_info` killed only the faulting thread, and
`thread::join()` reported `Ok(0)` for a thread that ended without recording an
exit value — which is exactly what a killed thread looks like.

### Decision

Three changes, which only work together:

1. **An unhandled ring-3 fault terminates the process.** `idt.rs`'s
   `kill_userspace_task_with_info` — reached only after *both* the Linux-ABI
   signal path and the native SEH trampoline have declined the exception —
   calls `proc::thread::kill_process_threads(pid)`.
2. **A killed thread is a distinct outcome, not a missing exit value.**
   `THREAD_EXIT_VALUES: BTreeMap<TaskId, i64>` became `THREAD_OUTCOMES:
   BTreeMap<TaskId, ThreadOutcome>` with `ThreadOutcome = Exited(i64) |
   Killed`; `join()` reports `KernelError::Cancelled` for `Killed`.
3. **`SYS_THREAD_JOIN` (512) returns the exit value through an `arg1`
   out-pointer** and returns `0`/`-errno` itself.

### Rationale

(1) matches the default disposition on both systems whose semantics we
implement: a Windows SEH exception nobody handles terminates the process, and
an unhandled `SIGSEGV` on Linux terminates the whole thread group. Killing one
thread is a *third* behaviour that neither ABI's programs are written against,
and it is the one that produces silently wrong output rather than a crash.

(3) is not gold-plating — it is what makes (2) representable. The old ABI
returned the exit value in the result register, so an exit value and an error
code shared one 64-bit channel. `pthread_exit(PTHREAD_CANCELED)` passes
`(void *)-1`, and `KernelError::Cancelled` is `-5`: both are legal exit values
*and* look like errors. With the out-pointer, `posix`'s `pthread_join` maps
`Cancelled` to a successful join returning `PTHREAD_CANCELED` — which is
precisely the value POSIX reserves for "this thread did not finish normally" —
and a `-1` exit value stays a `-1` exit value.

### Alternatives considered

- **Leave the kill scoped to one thread and only fix `join()`.** Rejected: the
  fixture that caught this joins through *glibc's* futex-based join over the
  Linux ABI, never touching `SYS_THREAD_JOIN`, so the kernel-side `join()` fix
  alone would not have changed its wrong answer at all. And a surviving process
  with a hole in its address space is not a state any program is written for.
- **Keep the value-in-rax ABI and reserve a magic sentinel for "killed".**
  Rejected: every sentinel is also a legal exit value. That is the exact
  category of bug being fixed.
- **Return the value in rax *and* write the out-pointer, for compatibility.**
  Rejected: a caller must decide from the return register whether it is holding
  a value or an error, which is the ambiguity itself. Since the only two call
  sites in the tree are in `posix/src/pthread.rs`, there is nothing to be
  compatible with — the sysroot and every C fixture were rebuilt.
- **A new syscall number, leaving 512 alone.** Rejected: 512's old shape is not
  worth preserving, and two join syscalls is a permanent tax to avoid a
  one-afternoon rebuild of prebuilt fixtures.

### Where it lives

- `kernel/src/idt.rs` — `kill_userspace_task_with_info`.
- `kernel/src/proc/thread.rs` — `ThreadOutcome`, `THREAD_OUTCOMES`,
  `record_killed`, `kill_thread`, `take_outcome`, `outcome_to_result`,
  `join`, `kill_process_threads`, self-test 8.
- `kernel/src/syscall/{number.rs,handlers.rs}` — `SYS_THREAD_JOIN`, and
  `terminate_current_process_for_signal`, which tears down sibling threads
  when an unhandled fatal signal kills a process. It called
  `sched::kill_task` directly and so recorded no outcome; it now goes
  through `kill_thread`, because dying to an unhandled fatal signal is an
  involuntary death exactly like an unhandled ring-3 fault. It still runs
  `on_thread_exit` itself when the scheduler refuses the kill (already
  Dead or unknown), since process teardown must not leave a half-reaped
  thread holding the thread→process mapping or a parked joiner.
- `posix/src/pthread.rs` — `pthread_join`, `pthread_detach`,
  `KERNEL_ERR_CANCELLED`, `PTHREAD_CANCELED_VALUE`.
- `kernel/src/kshell.rs` — `cmd_kill` now uses `proc::thread::kill_thread`.

The recurring shape here is worth naming: **`sched::kill_task` is the wrong
call at every site that kills someone else's thread.** It marks the
scheduler task Dead and nothing more — no outcome recorded, no death hook,
so the mapping, IRQ registrations and parked joiners all survive. Three
call sites had independently made that mistake (kshell's `kill`, the
fatal-signal teardown, and the exception path). `kill_thread` exists to be
the one correct entry point; the remaining bare `kill_task` callers are
scheduler/bench self-tests operating on kernel tasks that were never
registered as threads, where there is nothing to record.

### How to reverse

(1) is a one-function change in `idt.rs` and is the piece most likely to be
revisited — a future per-thread "exception isolation" policy (something like
Windows' `SetUnhandledExceptionFilter` opting a process into surviving) would
go there. (2) and (3) should be kept regardless: they close a
silently-wrong-answer channel, and reverting (3) alone would re-open the
negative-exit-value ambiguity.

---

## §200 — The B-KNULLJUMP hunt runs the *uninstrumented* kernel first (E), and escalates to the optimized KASAN build (A) only if that fails to settle it

**Date:** 2026-08-15
**Decided by:** Operator (Claude proposed this option — it was Claude's revised
recommendation after measuring the instrumented boot; the operator adopted it)

**In short:** there is a rare bug — a jump through a null pointer inside the
kernel, `B-KNULLJUMP` — that shows up on roughly **1 boot in 120**. To catch it
in the act we built a special "instrumented" kernel that checks every memory
access, but that kernel turned out to boot **~20× slower**, which would make the
hunt take over a week of machine time. The question was how to make the hunt
affordable. The answer: **first just run the ordinary kernel** many times, now
that it carries a suspected fix, and see whether the bug stops happening. Only
if that is inconclusive do we go back to the slow instrumented kernel, built
with optimizations on to claw back the speed.

**The options, and why E won.** The full option set (A–E, with measurements) is
preserved in `open-questions.md` → Q43's original analysis, which this entry
replaces as the decision of record.

- **E — soak the plain kernel carrying the `B-NO-CLD-ON-INTERRUPT-ENTRY` fix.**
  Cheap (~283–318 s/boot, versus 5500–8500 s instrumented), and it tests the
  thing we actually care about: whether the bug still happens in the kernel we
  ship. Chosen as the first step.
- **A — build the instrumented kernel `--release` and soak that.** Kept as the
  fallback, not discarded.

**"If necessary" has a specific shape, and it is not symmetric.** This is the
half most likely to be misread later:

- **E *catching* a B-KNULLJUMP falsifies the `B-NO-CLD-ON-INTERRUPT-ENTRY`
  hypothesis**, and is precisely the outcome that gives A a well-motivated job.
- **E coming back clean is suggestive, not proof.** It cannot separate "fixed"
  from "got lucky" at a 1-in-120 base rate. A clean E is therefore **a reason to
  stop, not a reason to escalate** — escalating on it would spend a week of
  machine time to re-answer a question E has already answered as well as it can
  be answered.

**A's cheap gate still stands, and is not optional.** **No release kernel has
ever been booted in this project** — every boot test to date is the debug
profile. So before any release soak: build `--release`, run
`scripts/kasan-check-preshadow.py`, and attempt **exactly one** boot (~30 min).
That answers both unknowns — does it boot at all, and what does it actually
cost — before the soak is committed to.

**A clean *release* soak is weaker evidence than a clean debug one.**
Optimization perturbs instruction timing and layout, which is exactly what a
1-in-120 race depends on; the base rate itself is a debug-build measurement and
may not carry over. Any result reported from A must carry this caveat attached
(§119 already records it).

**Two caveats on E's own numbers**, from the 2026-08-13 update: it samples a
SMAP-enabled kernel, which the 1-in-120 base rate was **not** measured on, and
its per-boot wall time is ~355 s rather than the ~283–318 s the ~21 h soak
budget was built from.

**Where it lives:** `scripts/kasan-build.sh`,
`scripts/kasan-check-preshadow.py`, `scripts/boot-test.sh` (the soak driver),
`known-issues.md` → `B-KNULLJUMP` and `B-NO-CLD-ON-INTERRUPT-ENTRY`, and
§107/§118/§119 for how the instrumented profile got here.

**Provenance:** the operator answered in a Lane B session ("q43: e, then a if
necessary"); Lane B relayed it as `requests/b-a-operator-answered-q43.md`
rather than writing into Lane A's §200–299 range itself.

## §201 — Install the GNAT/SPARK toolchain **with `gnatprove`**; clang + lld (and therefore CFI) is deferred, not refused

**Date:** 2026-08-15
**Decided by:** Operator (both halves; on the prover, the operator challenged
Claude's original framing and was right — see below)

**In short:** two unrelated compiler installs were bundled into one question.
**Ada/SPARK** is a second programming language whose toolchain can
mathematically *prove* that driver code has no buffer overflows and no illegal
state transitions; `design.txt` (lines 84–95) wants it for safety-critical
drivers. That one is **approved, including the prover program `gnatprove`**.
**clang + lld** is an alternative C compiler and linker, whose point would be
enabling **CFI** (Control-Flow Integrity — a compiler feature that stops an
attacker redirecting a function call to code of their choosing). That one is
**"not yet"**: deferred with a trigger, not rejected.

**On the prover — why "including gnatprove" is the load-bearing half.** The
original question carried a con reading, in effect, *"if we install a toolchain
without the prover we get FFI plumbing and none of the proof."* The operator
challenged it — *"why wouldn't we install gnatprove?"* — and that challenge was
correct on the facts: `gnatprove` is **freely available on this platform**.
SPARK is open source, AdaCore publishes Windows x86-64 binaries, there is an
Alire crate (`alr with gnatprove`), and `GNAT-FSF-builds` ships FSF builds. No
licence and no cost blocks it. The bullet was a **route warning**, not a veto.
The operator then answered by explicitly naming the prover, which settles it:
**the prover is part of the definition of done.** Ada-without-SPARK is just
another systems language, and we already have a memory-safe one — the feature
is justified in `design.txt` on the *proof* specifically.

**Three consequences that follow directly:**

1. **The install route cannot be MSYS2.** `mingw-w64-x86_64-gcc-ada` ships
   `gnat` and `gprbuild` and **no** `gnatprove`, and MSYS2 has no such package.
   Taking the easy route would buy the entire cost of the feature and none of
   its justification. The route must be **Alire** (`alr toolchain --select`,
   then the `gnatprove` crate) or **AdaCore's own download**.
2. **The prover stack is a further install:** Why3 + Alt-Ergo, optionally Z3 and
   CVC5. `gnatprove` without a solver proves nothing.
3. **GPL is not a problem here.** The toolchain is a tool we *run*, not
   something we link; it does not reach our output.

**Two sub-decisions this does *not* settle — they are Lane A's to make:**

- **Which GNAT distribution.** FSF-via-Alire now looks clearly preferable to
  GNAT Pro precisely because it carries `gnatprove`, but nobody has recorded
  that as a decision.
- **The restricted runtime: ZFP vs light.** A freestanding kernel cannot use
  the full Ada runtime, which wants an OS underneath it. Configuration work
  with real content, not part of the install.

**On clang + lld — "not yet" is a deferral with a trigger.** The install is
small and uncontroversial; what is missing is a *reason*. We use C only for
ported code, and the one piece of C compiled today
(`scripts/create-ext4-rootfs.sh`) is built with gcc — so enabling CFI now would
change Lane B's build for a benefit that only materialises when the large C
ports land, and would pull in LTO (whole-program optimization at link time),
which slows every build it touches. Nothing is blocked by waiting. It moves to
`deferred-questions.md` as **D-Q2**, with the trigger being **the first
substantial C port entering the build**, so it returns when the payoff is real
instead of being quietly dropped.

**Where it lives:** the Ada/SPARK FFI bridge is a Lane A roadmap item, so the
follow-through is Lane A's; `deferred-questions.md` → D-Q2 for the clang half;
`design.txt` lines 84–95 for the original justification.

**Provenance:** the operator answered in a Lane B session, verbatim *"q44: a,
including gratprove."* The `q44` label is a typo for **A-Q1** — it arrived
immediately after the real Q44 answer (`Q44: a.`), and Q44 (the libc capability
mapping) has no option "including gnatprove". Relayed as
`requests/b-a-operator-answered-a-q1.md`.

---

## §202 — When the answer does not fit, `SYS_CAP_QUERY` returns an error and writes nothing, rather than truncating and reporting the size it wanted

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** A program can ask the kernel "list the permissions I hold." It
passes a buffer — a chunk of its own memory for the kernel to write the list
into — and says how many entries fit. Sometimes the list is longer than the
buffer. There are two conventional ways to handle that, and this records which
one this call uses and why: **it fails with a distinct error and writes nothing
at all**, rather than filling the buffer with as much as fits and returning a
number the caller might not check.

**Terms:** *truncation* = writing a partial answer. *`ERANGE`* = the POSIX error
number meaning "result too large for what you gave me" — as opposed to `EINVAL`
("your request was malformed"), which tells a caller to stop rather than retry.
*Probe* = calling with a null buffer purely to learn the required size.

### The two shapes

| Shape | On overflow | Failure mode when the caller is careless |
|---|---|---|
| **A — POSIX `listxattr` style** (used by `SYS_FS_LIST_XATTR`, `handlers.rs:8882`) | Return **success**, with the *required* size as the return value; write nothing | The caller treats a success as "here is your list", reads the untouched buffer, and sees whatever was there before |
| **B — chosen here** | Return `BufferTooSmall` (`-9` → `ERANGE`); write nothing | The caller gets a negative return it must handle; ignoring it cannot be mistaken for data |

### Why B, and why the codebase now has both

The deciding argument is what a *silent* wrong answer means for this particular
call. `SYS_CAP_QUERY` enumerates authority. Lane B's libc projects its result
onto Linux `CAP_*` bits (§312). A truncated list does not read as "an error
happened" — it reads as **"this process does not hold that capability."** So the
failure lands as a *false negative on a permission check*, which is the
direction nobody notices: things quietly do not work, or worse, a security
decision is made on a short list. Under-reporting authority is the same class of
bug as over-reporting it, minus the alarm.

Shape A is not a mistake where it is used — `SYS_FS_LIST_XATTR` implements the
POSIX `listxattr` contract, and that contract is not ours to redesign; callers
are ported code that already expects it. This call has **no legacy contract**,
so it takes the shape where ignoring the failure is impossible.

The convenience A buys — learning the required size from the failed call — is
retained without the hazard: **probe mode**. Passing a null pointer or a zero
capacity returns the count and writes nothing, so "ask how big, then ask for the
data" is two cheap calls rather than one call with two meanings. That also keeps
the probe path off the expensive route entirely: it is answered from
`cap_count()` without ever building the snapshot.

### The cost, stated plainly

A caller racing against its own capability set (one that gains a capability
between the probe and the fetch) gets `ERANGE` and must loop. Shape A would have
handed it the new size in the same call. The loop is two lines and terminates —
capability grants are not adversarially fast — and this is the trade taken.

### Consequence for Lane B

`posix/src/errno.rs` mirrors kernel codes as a **non-exhaustive** constant
table, so adding `-9` cannot break their build; it falls through to `EIO` until
they map it. Until they do, an overflow reports as a generic I/O error rather
than the retryable `ERANGE` — annoying, not wrong. Filed in the reply to
`requests/b-a-cap-enumerating-query-syscall.md`.
## §203 — A benchmark that is not recorded must say so out loud: every measurement window is either scored, tracked, or *declared* a diagnostic

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** The kernel benchmark suite prints ~90 timing measurements per
`--bench` boot, but only some of them get filed into `bench/history.jsonl`, which
is the file the regression comparator reads. The rest are printed to the serial
log and forgotten — no history, so nothing can ever notice them getting slower.
Nobody knew which were which. This records the rule adopted: **every measurement
must be classified**, and the suite now prints a line each boot naming any
measurement that hasn't been.

**Terms:** *score* = file the measurement with a target to grade it against.
*track* = file it with no target, so it is compared to previous boots but never
graded pass/fail. *diagnostic* = a number that is deliberately not filed, because
it means nothing on its own. *window* = one measurement — one call to the
harness's `run()`.

**The decision.** Three destinations, and a measurement must reach exactly one:

| | Filed to history? | Graded? | For |
|---|---|---|---|
| `score(name, r, target)` | yes | yes | a benchmark with a defensible published or measured target |
| `track(name, r)` | yes | no | a benchmark worth comparing run-over-run with nothing to grade against |
| `run_diagnostic(name, …)` | no | no | a number that is only meaningful *relative to another number in the same block* |

The coverage line at the end of the suite reports the three counts plus a fourth
— `unjudged` — and names every unjudged window. `unjudged` is expected to be 0
forever; anything else is a measurement whose author has not said which of the
three it is.

**The tradeoff, which is real in both directions.**

*Against having a `diagnostic` category at all:* it is an opt-out from
regression detection, and opt-outs get used to silence inconvenient reports.
Every window `run_diagnostic` covers is a number that can now degrade forever
unwatched. The maximally-safe design is "record everything" — `track` costs
almost nothing, and the comparator can always be told to ignore an entry later.

*For it:* recording a decomposition stage does not give you regression detection
on it, it gives you *noise*. `sd_ktrace_pair` measures two ktrace calls in
isolation so they can be subtracted from the syscall-dispatch total; its absolute
value is meaningless, it swings with TCG scheduling, and a comparator watching it
would fire on nothing. Worse, the eleven decomposition sub-measurements would
have been ~13% of the record, all of them noisy, which degrades the
signal-to-noise of the whole file. And the alternative to a `diagnostic`
category is not "record them" — it is *the status quo*, where they were
unrecorded **and unmarked**, indistinguishable from a benchmark someone forgot.

That last point is what settles it. The category is not licence to skip work;
it is the mechanism that makes skipped work *visible*. Before it, the coverage
report could only ever say "21 windows unrecorded" and would say it every boot
forever, which is the shape of a check people learn to ignore — the same failure
as an assertion that fires on every healthy boot. With it, the report reads 0 and
any non-zero is real news.

**Guard against the obvious abuse.** `run_diagnostic`'s doc comment states the
rule as a question about the *number*, not about convenience: reach for `track`
whenever the value would mean something on its own next boot, including when it
has no target — "untargeted is not the same as uncomparable, and conflating the
two is the easier mistake." That conflation is exactly what had happened: the
pass that adopted this rule found **eight** genuine benchmarks printed and
discarded, among them `rdtsc_overhead`, which is the instrument every other
benchmark in the suite is measured with.

**Rejected alternative: infer coverage by name.** The first implementation
diffed the measured names against the scorecard names. It is wrong, and not
marginally: five benchmarks record under a different name than they measure
under (`lock_tracked`→`lock_uncontended`, `heap_raw_alloc_free_64`→
`heap_alloc_free_64`, and three more), so a name diff reports five fully-covered
benchmarks as uncovered. Coverage is now keyed by `BenchResult::seq`, an index
into the measurement list, so a window is covered precisely when *that window*
was handed to `record` — which leaves benchmark naming free rather than making
it load-bearing for an unrelated instrument.

**Where the `track`/`diagnostic` line actually falls: a scaling sweep is
tracked, a decomposition is not.** The first boot under the new instrument
forced this distinction to be stated rather than felt. `bench_pick_next_scaling`
measures run-queue pick cost at five depths; `bench_syscall_dispatch_breakdown`
measures six stages of one syscall. Both are "a group of related numbers", and a
rule of thumb about groups would have put them in the same bucket. The rule that
decides them is instead **what each number claims on its own**:

- A *decomposition*'s stages are meaningless apart from their siblings — stage 3
  of a syscall is not an operation anyone performs, and a comparator handed its
  history has nothing to compare it against. Diagnostics.
- A *scaling sweep*'s points are each a complete measurement of the same
  operation under a different load, and the claim being made is **the shape they
  trace**. Tracked. The in-kernel verdict tests only the two endpoints against a
  4x threshold with generous headroom, so a regression that bent the middle of
  the curve — the exact failure the sweep exists to detect — passed it silently.
  Recording each point is what makes the shape diffable across boots.

That fix needed a rename, not just a wiring change, which is the sharper lesson:
all five depths ran under one name, and five history entries under one key is
not a series but four values overwriting each other. A measurement's name is
part of whether it *can* be recorded at all.

**A second failure of the static scan, in the other direction.** The rejected
alternative above records that a name diff produces false *positives*. The audit
script that replaced it produced a false *negative* on this same sweep: it
searched forward from `run()` for a `score`/`track` naming the same binding and
matched a `&result` belonging to a different function sixty lines downstream.
Two independent static approaches, two opposite errors, one runtime instrument
that got it right — which is the argument for keeping the authority at runtime
rather than adding a third scanner.

**The invariant is asserted, not just printed.** `scripts/boot-test.sh` gained
`check_bench_coverage()`. A coverage line nobody greps is prose, and this project
has a precedent for what prose costs: `BUG-LIVENESS-DEADLINE-FALSE-FIRE` required
a clean liveness log from 2026-07-27 and runs that violated it still exited 0.
The one design point worth recording is the **absent** case — under `--bench` the
line is *required*, because a missing coverage line means the instrument stopped
running, and scoring that as "nothing to complain about" would reproduce this
very bug one level up. It was confirmed to fire against the real pre-fix log
before being trusted on a passing one.

**Where it lives:** `kernel/src/bench.rs` — `MEASUREMENTS`, `note_measurement`,
`declare_diagnostic`, `run_diagnostic`, `record`, and the coverage block in
`print_scorecard`; `scripts/boot-test.sh` — `check_bench_coverage`. Written up in
`known-issues.md` under the scorecard-coverage entry.

## §204 — The #UD handler decodes clang's `ud1` sanitizer traps, and names only the reasons that were actually measured

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** When a program does something the compiler was told to check for
— an array read past its end, an integer overflow, a function call redirected
somewhere it should not go — the compiler does not print a message. It plants
a deliberately invalid instruction ("trap") at that spot, and the CPU raises a
fault when execution reaches it. The kernel catches that fault. Until now it
could recognise exactly one such instruction shape, and only when the fault
came from the kernel itself; a trap in an ordinary program was reported as an
anonymous "invalid opcode" with no hint of which check failed. This adds a
decoder that reads the *reason* out of the trap instruction's own bytes and
names it, for programs as well as for the kernel.

### The thing that makes this worth a design entry: it is `ud1`, not `ud2`

Everyone's mental model of a compiler trap is `ud2` (bytes `0F 0B`) — that is
what rustc emits, and what `__builtin_trap()` emits. But clang's
`-fsanitize-trap=<check>` emits **`ud1`** (`0F B9`), because `ud1` takes an
operand and clang uses it to carry *which check failed*:

```
67 0f b9 40 02        ud1    2(%eax), %eax      <- reason 2 = cfi_check_fail
^  ^^^^^ ^^ ^^
|  |     |  +-- disp8 = the failing check's ordinal
|  |     +----- ModRM: mod=01 -> reason is in a disp8; mod=00 -> reason is 0
|  +----------- ud1 opcode
+-------------- 0x67 address-size prefix (the operand is 32-bit %eax)
```

The consequence is a trap in the project's most-repeated failure shape: **the
obvious way to check whether a sanitizer is switched on is to scan the binary
for a trap instruction, and the obvious instruction to scan for is `ud2` —
which is never there.** That scan returns "no traps found" on a *fully
instrumented* binary. It produced exactly that false negative here before being
caught. A check that cannot fire is indistinguishable from a check that passes.

### Decision 1 — decode on the ring-3 path too, before dispatch

The old handler decoded only for `rip >= 0xFFFF_8000_0000_0000`, and the
userspace branch returned immediately with no byte inspection at all. That
inverts the priority: a CFI violation *is* a ring-3 fault — protecting user
programs is the entire point of the feature — so the one path that could not
name it was the one that would see it.

- *Alternative considered:* report only when no handler catches the fault, on
  the grounds that a caught fault is not a crash. **Rejected**: a program that
  installs a handler and swallows its own CFI violation is precisely the case
  where silence is most expensive.
- *Cost accepted:* reading user memory inside a fault handler. This is safe
  here for a specific reason worth stating — the branch is reached only under
  `is_userspace_exception`, i.e. the interrupted CS had RPL 3, so the
  interrupted context was ring-3 code and held no kernel lock on this CPU. The
  helper carries a "do not call this from the ring-0 path" note for that
  reason. Bytes are read **one at a time**, stopping at the first failure, so
  an instruction at the end of the last mapped page still decodes as far as it
  goes instead of failing wholesale.
- *Spam:* nothing is printed unless the bytes decode as a deliberate trap, so
  a program probing for CPU features with a bad opcode stays quiet.

### Decision 2 — name only measured ordinals; print the rest as numbers

clang's reason ordinals are assigned by declaration order in its internal
`SanitizerHandler` list, so inserting one check renumbers every check after
it. There is no ABI promise here at all.

- *Alternative:* transcribe clang's whole table from its header. **Rejected.**
  It would be a table of ~25 names of which we had verified none, in a file
  whose entire job is to tell a human what went wrong. A confidently wrong
  cause attached to a real bug is worse than a bare number.
- *Chosen:* **measure them.** Twenty of the twenty-five were established by
  compiling a one-line function per check with a single
  `-fsanitize=X -fsanitize-trap=X` and byte-scanning the output for the
  resulting `ud1`. Those twenty are named. The five that were not reachable
  from C (Objective-C casts, nullability attributes, implicit-conversion)
  deliberately return `None` and print as `reason N (unrecognised)`.
- *Cost accepted:* a toolchain bump can silently renumber the named twenty.
  Mitigated by the self-test below and by an explicit instruction in the doc
  comment to **re-measure rather than trust the comment**.

### Decision 3 — a boot self-test, not a `#[cfg(test)]` module

The kernel binary cannot be built for the host test harness (duplicate
`panic_impl` lang item), so a `#[cfg(test)]` module in `kernel/src/idt.rs`
would never be compiled, let alone run. Writing one would have been the same
mistake this decoder exists to prevent, one level up. `ud_trap_decode_self_test`
therefore runs at boot alongside the other IDT self-tests, asserting against
byte sequences copied verbatim from real clang output — so a toolchain change
that alters the encoding fails loudly at boot instead of quietly mis-naming
every fault thereafter. It also asserts the **negative** cases (SIB and
RIP-relative addressing forms, truncated instructions, multi-byte nops),
because a decoder that answers "sanitizer trap" for arbitrary bytes would be a
net loss.

### What this does *not* do

It does **not** enable CFI. That is deferred by an operator decision (§201,
`deferred-questions.md` → D-Q2) until the first substantial C port. This is the
half that is useful regardless: it names out-of-bounds, shift, divide-by-zero,
null-dereference and sixteen other trap kinds today, on any C we already
compile, and it means the CFI decision — whenever it is taken — lands on a
kernel that can already say what happened.

**Where it lives:** `kernel/src/idt.rs` — `UdTrap`, `decode_ud_trap`,
`sanitizer_trap_name`, `report_ud_trap`, `read_user_insn_bytes`,
`ud_trap_decode_self_test`, and both branches of `handle_invalid_opcode`;
called from `kernel/src/main.rs` with the other IDT self-tests. The measured
flag set and the three false-negative traps found while verifying it are in
`deferred-questions.md` → D-Q2's 2026-08-16 amendment.

## §300 — A NULL pointer is `EFAULT` only where the kernel would see it; glibc's own pre-checks keep their `EINVAL`

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

### The problem

`posix/` had a blanket rule, applied by an undocumented sweep (recorded only
obliquely in `todo.txt` as "Phase 215"), that any NULL pointer argument to a
libc entry point is reported as `EFAULT`. A follow-up phase then "fixed" the
tests that disagreed with the sweep, cementing the behaviour with green tests.

The rule is right most of the time and wrong in a specific, identifiable class
of cases. It is right whenever the pointer is forwarded to a syscall: Linux
validates user pointers in `copy_from_user`/`copy_to_user`, a NULL page is not
mapped, and the kernel returns `EFAULT`. So `open(NULL, …)`, `stat(NULL, …)`,
`getxattr(NULL, …)` all correctly produce `EFAULT`, and our sweep matched
Linux there.

It is wrong whenever the function is implemented *entirely in userspace* and
glibc rejects the NULL argument with an explicit early return, before the
pointer is ever dereferenced or handed to the kernel. Those functions produce
`EINVAL`, and no syscall is issued at all, so there is no kernel to produce an
`EFAULT`. Three of ours had drifted:

| Function | glibc source | glibc errno | ours (before) |
|---|---|---|---|
| `realpath(NULL, buf)` | `stdlib/canonicalize.c:195` — `if (name == NULL) { __set_errno (EINVAL); return NULL; }`, citing SUSv2 | `EINVAL` | `EFAULT` |
| `canonicalize_file_name(NULL)` | `stdlib/canonicalize.c:460` — `return __realpath (name, NULL);` | `EINVAL` | `EFAULT` |
| `__realpath_chk(NULL, …)` | forwards to `realpath` | `EINVAL` | `EFAULT` |

A fourth candidate, `ptsname_r(fd, NULL, n)`, turned out to be a different bug
and is treated separately below.

### The `ptsname_r` case, and why it is not the same bug

`ptsname_r` was the function that started this: its *doc comment* said
`buf == NULL -> EINVAL` while its body said `EFAULT`, and both `ptsname_r(3)`
and POSIX.1-2024 document `EINVAL` for a NULL `buf`. The obvious conclusion —
that the sweep had overwritten a correct `EINVAL` — was wrong, and it was wrong
in a way that only reading the source could show.

glibc 2.39's `__ptsname_r` (`sysdeps/unix/sysv/linux/ptsname.c`) has **no NULL
check at all**. It opens with `__ioctl (fd, TIOCGPTN, &ptyno)` and returns that
call's errno on failure, touching `buf` only afterwards:

```c
int
__ptsname_r (int fd, char *buf, size_t buflen)
{
  int save_errno = errno;
  unsigned int ptyno;

  if (__ioctl (fd, TIOCGPTN, &ptyno) == 0)
    { …  memcpy (__stpcpy (buf, devpts), p, …);  }
  else
    /* Bad file descriptor, or not a ptmx descriptor.  */
    return errno;
```

So on Linux the *descriptor verdict outranks the buffer*: a bad fd gives
`EBADF` and a live non-PTY fd gives `ENOTTY`, with a NULL `buf` making no
difference to either. `EINVAL` is reachable only on a real ptmx fd — where
glibc does not return it but segfaults in `__stpcpy (buf, devpts)`. (musl
clamps the length to 0 and yields `ERANGE`.) The documented `EINVAL` is a
survival from an older glibc whose check the TIOCGPTN fast path removed.

Our implementation checked `buf` *first*, so it answered a NULL-buf caller with
an argument complaint where Linux answers with the fd's errno — the check being
`EFAULT` rather than `EINVAL` was the lesser half of the divergence. The
ordering is now glibc's: fd checks, then the PTY-master verdict (always `ENOTTY`
here, since `posix_openpt` returns `ENOSYS`), and `buf` is not examined at all.
A comment marks where the NULL check belongs once PTY support lands, and says
to use `EINVAL` there — following the man page and POSIX rather than copying a
segfault.

### Decision

The policy is stated positively rather than as a blanket:

> **`EFAULT` is what the kernel reports for a pointer it cannot access.** Use
> it in `posix/` when the argument would reach a syscall. When a function is
> implemented in userspace and upstream glibc rejects the argument itself with
> an early return, mirror glibc's errno — which for a NULL argument is
> `EINVAL`, an argument-domain error, not a fault. **Read the glibc source
> before deciding which case you are in**; a man page is not sufficient
> evidence, as `ptsname_r` shows.

The three `realpath`-family functions now return `EINVAL`, each with a comment
naming the glibc translation unit and quoting its check, so the next sweep has
something to read before it changes them back. `ptsname_r` keeps no NULL check
and its tests now assert the fd's errno; `test_ptsname_r_null_buf_efault`
became `test_ptsname_r_null_buf_still_reports_the_fd_verdict`, and
`test_ptsname_r_null_buf_beats_bad_fd` became `…_bad_fd_beats_null_buf` — the
test names carry the ordering so it cannot be silently inverted again.

To make this checkable rather than recalled, glibc 2.39's source now sits
beside bash's at `D:\refsrc\glibc-2.39` (shallow clone of the `glibc-2.39` tag
from the `bminor/glibc` GitHub mirror; `sourceware.org` answered 429). Every
citation in this entry was verified against it — including the two that a
from-memory first draft of this entry got wrong.

### Alternatives considered

**Keep the blanket `EFAULT`.** Simpler to state, and uniform: one rule, no
per-function research, no risk of a half-applied sweep. But it is wrong in a
way that is visible to real programs. A portable caller that branches on
`EINVAL` — the errno every `realpath(3)` man page shows for a NULL path, and
the one glibc actually returns — takes the wrong branch on SlateOS. The whole
point of the compatibility layer is that unmodified Linux binaries behave as
they do on Linux; "uniform but divergent" is the failure mode this project
exists to avoid, and it is exactly the "correct-but-naive" pattern CLAUDE.md
warns about.

**Make it configurable / strict-vs-lenient mode.** Rejected as unjustified
machinery: there is a single correct answer per function (whatever glibc
does), so there is nothing for a knob to select.

**Follow the man page rather than the source.** Cheaper, and it is what the
first draft of this change did. It produces exactly the wrong answer for
`ptsname_r`, whose documented `EINVAL` no glibc has returned since the TIOCGPTN
fast path landed. Man pages describe the contract an implementation once had;
the compatibility target is the contract binaries are actually built against.
Where the two differ and neither is dangerous, the source wins; where the
source's behaviour is a crash (as it is here for a real ptmx fd), take the
documented errno instead — and say so at the site.

### Consequences

- The rule now requires *reading glibc* for each NULL check rather than
  applying a regex. That is the cost, and it is the right cost — it is the
  same discipline the rest of the compatibility layer already uses.
- Argument-validation *ordering* is part of the ABI too, not just the errno
  values. `ptsname_r`'s bug was not really the choice of `EFAULT` over
  `EINVAL`; it was checking `buf` before the descriptor, which changes the
  answer for every NULL-buf caller regardless of which errno the check sets.
  The audit below has to look at order, not only at the constant.
- A cheap mechanical form of the audit exists and was run the same day:
  search *glibc* for `if (x == NULL) { __set_errno (E…) … }` and intersect the
  results with our entry points, rather than reading our 289 sites one by one.
  The one trap is that many hits — most of `io/*.c` and `posix/*.c` — are the
  generic `ENOSYS` stubs that no Linux build uses; filter them by looking for
  `stub_warning`/`ENOSYS` in the same file. That sweep found nine further
  divergences (the five sigsetops, `closedir`, and the three `cfset*speed`
  functions), which are listed in the known-issues entry. The `pthread.rs`
  cluster is explicitly *not* answerable this way: glibc has no checks there
  to copy, so what we return in place of its segfault is a decision, not a
  lookup, and it is left for a separate pass.
- A future audit should sweep the remaining `is_null() -> EFAULT` sites in
  `posix/` and classify each as syscall-forwarding (keep `EFAULT`) or
  userspace-pre-check (switch to glibc's errno). The `xattr`, `stat`, and
  `file` NULL-path checks spot-checked during this change are all
  syscall-forwarding and correctly `EFAULT`; the audit is about coverage, not
  a suspicion that more are wrong.
- Tests that encode an errno now carry a one-line citation of the upstream
  source. Without it, a test is only evidence that the code and the test were
  written by the same pass — which is precisely what happened here.

---

## §301 — Slot pools are serialised by one shared spin lock over the whole scan, not by a per-slot atomic, and the lock is scoped exactly like the table

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)
**Lane:** B (POSIX & userland)
**Affects:** `posix/src/perprocess.rs` (the primitive), `posix/src/{dirent,stdio,aio,epoll,mqueue,semaphore,sysv_msg,sysv_sem,sysv_shm}.rs` (the pools)

### The problem

The POSIX layer keeps about fifteen fixed-size slot pools — `DIR` handles,
`FILE` handles, `popen` children, `aiocb` records, epoll/timerfd/inotify
instances, the getdents caches, POSIX message queues and their descriptors,
named semaphores, and the three System V tables. Ten of them claimed a slot
like this:

```rust
for (i, slot) in table.iter_mut().enumerate() {
    if !slot.in_use {       // check ...
        slot.in_use = true; // ... then set, with nothing held
        return Some(i);
    }
}
```

Check-then-set with nothing held is correct in a single-threaded process, and
every one of these modules carried a `// SAFETY: single-threaded posix layer`
comment saying so. That comment stopped being true when this crate grew a
`pthread_create`. Two threads calling `opendir` at the same instant can both
read `in_use == false` for slot 3 and both be handed slot 3, after which each
silently scribbles on the other's directory stream. Nothing in the type system
or the test suite catches it: single-threaded tests pass forever.

The remaining five modules (`mqueue`, `semaphore`, and the three System V
ones) already locked correctly — but each with its own hand-rolled
`AtomicBool` + `lock_acquire`/`lock_release`/`struct Guard`, five near-identical
copies of the same twenty lines.

### Decision 1 — one lock over the whole scan, not a per-slot `compare_exchange`

`PoolLock` is a spin lock; `lock_pool()` returns a `PoolGuard` that releases on
`Drop`; the entire scan-and-claim runs inside one critical section.

The obvious cheaper alternative is a `compare_exchange` on each slot's `in_use`
flag: no lock, no contention between threads claiming *different* slots, and it
makes the simple pools correct. It was rejected because it does not cover the
*compound* pools. `sysv_msg::alloc_queue(key)`, `sysv_sem::alloc_set(key, …)`
and `sysv_shm::alloc_segment(key, …)` first scan for an existing entry with a
matching key and allocate only if there is none. A per-slot atomic claim keeps
two threads from taking the same *slot*, but does nothing to stop them each
creating a *separate* segment for the same key — the lookup and the claim have
to be one indivisible step, and a per-slot CAS cannot express that. The
getdents cache has the same find-or-create shape.

So the choice was between one primitive that covers every pool, or two
primitives where the weaker one silently does not apply to a third of them and
the next person to add a pool has to work out which. Allocation is not a hot
path — once per `opendir`, per `epoll_create`, per `mq_open` — the critical
section is a bounded scan of a small array, and the crate is `no_std` on the
target with no blocking primitive available. Uniformity wins; the contention
cost is not measurable at these call rates.

### Decision 2 — the lock covers *scans*, not the objects in the pool

Only allocate, release, and find-by-key take the lock. Once a caller holds the
`DIR *`, `mqd_t`, or instance index that names its own slot, it reaches that
slot unlocked: `with_instance_mut`, `readdir`, `epoll_ctl` and friends stay
lock-free.

This is a real boundary, not laziness. POSIX already makes concurrent use of a
single `DIR *` or `FILE *` by two threads the caller's problem, so there is no
contract to uphold; and taking a process-wide lock on every `readdir` would
serialise unrelated directory streams for nothing. The lock is on the *pool*,
not on the objects in it — and each unlocked accessor now says so in place of
the old "single-threaded" comment, so the boundary is documented where someone
would otherwise "fix" it.

Two consequences are accepted rather than solved:

- **Releases take the lock too.** Clearing `in_use` outside it is a plain data
  race against a concurrent claim reading it, and could let a slot be reissued
  before the release is visible. Every `*_close` therefore takes the guard even
  though it, too, names its slot by index.
- **The getdents cache can still end up with two slots for one fd.** Its
  `SYS_FS_LIST_DIR` snapshot is far too slow to hold a process-wide lock
  across, so the pool uses claim-then-publish: `claim_getdents_cache()` takes
  the slot with `fd = -1` (so no concurrent `find_getdents_cache` can match it)
  and `publish_getdents_cache(slot, fd)` sets the real fd once the buffer is
  filled. Two threads calling `getdents64` on the *same* fd simultaneously can
  therefore each get their own cache slot, each with its own position. That is
  concurrent use of one fd — already the caller's problem — and it is
  memory-safe, which is what the lock is for.

### Decision 3 — the lock's scope must match its table's scope

The pools are declared two ways, and the lock has to follow:

| the table is | the lock must be | why |
|---|---|---|
| `process_global!` (`epoll`, `mqueue`, `semaphore`, `aio`, `stdio`'s popen store) | `process_global!`, declared in the same block | on the host each test thread owns its own table, so a shared lock serialises threads that cannot collide |
| a plain `static mut` (`dirent`, `stdio`'s `FILE` pool, the three System V tables) | a plain `static` | the table really is shared on both builds, so a per-thread lock would guard nothing |

Getting this backwards is silent — a too-narrow lock compiles, passes every
single-threaded test, and protects nothing. `mqueue` and `semaphore` were both
found with the *other* mismatch: a plain `static` lock over a `process_global!`
table. That direction is safe (over-broad, not under-broad) and correct on the
target, where `process_global!` is itself a `static mut`. It was still fixed,
because a spin lock has no poisoning and no unwinding path: one host test that
panics inside the critical section of a *shared* lock hangs every later test
that touches the pool, including tests that share no data with it. Declaring
the lock inside the `process_global!` block, beside its table, is what keeps
the two from drifting apart later.

The one deliberate exception is `epoll`'s `event_scratch_lock`, which guards a
~8 KiB *buffer* rather than a slot table and is held across the
`SYS_FS_WATCH_READ` syscall. It has to be: the syscall is what fills the
buffer, and the records are parsed straight out of it, so releasing the lock in
between would let another thread refill it mid-parse. It is re-taken per batch
so a long drain does not monopolise the buffer.

### Alternatives considered

**Leave it single-threaded and document the restriction.** The pools predate
`pthread_create`; one could declare that the POSIX layer's allocators are
simply not thread-safe. Rejected: the whole point of this layer is that
unmodified Linux binaries behave as they do on Linux, and on Linux `opendir`
from two threads is ordinary, unremarkable code. A documented restriction that
real programs violate by default is not a restriction, it is a latent bug.

**A `Mutex`-like type with poisoning.** No `std` on the target, and poisoning
implies unwinding, which the kernel-side build does not have. The spin lock's
failure mode under a panic is a hang rather than a poisoned error — which is
exactly why Decision 3 pushes every lock to the narrowest scope its table
allows.

**Keep the five hand-rolled locks.** They worked. But five copies of the same
twenty lines is five places for the next fix to be applied to four of them, and
the copies had already drifted (`AcqRel` vs `Acquire` on the exchange, differing
guard names, and the two scope mismatches above). Collapsing them onto the
shared primitive is what surfaced the mismatches in the first place.

### How the test has teeth

`a_scan_and_claim_under_the_lock_never_hands_out_a_slot_twice` is worth
describing, because its *first* version proved nothing: it passed with
`lock_pool` deliberately removed. The `Mutex<Vec<usize>>` used to collect the
claimed indices was serialising the worker threads all by itself, so they never
actually raced. The version in the tree fixes that three ways — a
`std::sync::Barrier` so all eight threads enter the claim loop together, a
`yield_now()` between the read of `in_use` and the write to widen the window,
and per-thread private `Vec`s merged only after `join`. With the lock removed
it now fails as `left: 20, right: 64`. A concurrency test that has never been
observed to fail is not evidence of anything.

### How to reverse

`PoolLock` is a private crate primitive with no ABI surface; the guard is
acquired at the top of each allocator. Reverting means deleting the
`let _guard = …` lines and the lock statics. The claim/publish split in
`dirent`'s getdents cache is the one structural change that would need undoing
separately.

---

## §302 — A zoneinfo zone is a borrowed view over the file's bytes, and the POSIX rule engine is its *tail*, not its alternative

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)
**Lane:** B (POSIX & userland)
**Affects:** `tzrules/src/tzif.rs` (the reader), `posix/src/tz.rs` (`Zone`, `ZONE_FILE`), `userspace/oils/src/interp.rs` (`ShellZone`, `Zone<'a>`)

### The problem

`tzrules` already understood the POSIX `TZ` grammar. To make
`TZ=America/New_York` work, both the libc and osh had to learn to read TZif
(RFC 8536) binary zoneinfo files. Three things had to be decided along the way,
each with a real trade-off.

### Decision 1 — the reader borrows the file's bytes rather than owning a table

`TzFile<'a>` holds four slices into the caller's buffer (transition times,
transition types, ttinfo records, designations) and decodes an entry on each
lookup. It does not copy the transition table into a struct.

**For:** `tzrules` is `no_std` with no allocator — it is linked into the libc,
where `localtime()` must not allocate. The alternatives are a fixed inline
array, which either wastes kilobytes in every `Tz` or caps the transition count
at some arbitrary number (`America/New_York` has 236 transitions; some zones
have over 1 000), or requiring an allocator, which the libc cannot provide on
this path.

**Against:** it pushes the lifetime problem onto every consumer — the bytes
must outlive the zone. In the libc that meant a `static mut ZONE_FILE` buffer
and `TzFile<'static>`; in osh it meant an owning `ShellZone` holding an
`Rc<[u8]>` with a `view()` that borrows it. Two different answers to the same
question is a smell, but they are genuinely different situations: the libc has
one process-wide zone and no allocator, osh has an allocator and wants a
snapshot per prompt.

**Also against:** decoding per lookup is a few big-endian loads and a binary
search rather than a single indexed read. Measured against the cost of the
`open`/`read` that precedes it, this is noise.

### Decision 2 — the footer rule governs at and after the last transition

A TZif v2+ file ends with a POSIX `TZ` string. `TzFile::lookup` consults the
existing `Tz` engine for any instant at or past the last recorded transition
(and for *every* instant in a file with no transitions at all), and the
transition table only before that.

**For:** this is what the format means. `zic -b slim` — the default in modern
tzdata — stops emitting transitions as soon as the footer describes them, so a
reader that ignored the footer would freeze every zone at its last recorded
entry and report the wrong offset for every future date. Treating the rule
engine as the *tail* of the file engine also means the two paths share one
implementation of DST arithmetic and can never disagree about a future date,
which is the property the whole shared-crate arrangement exists to guarantee.

**Against:** it means a lookup can take either of two quite different code
paths depending on the instant, so a bug in one is invisible from the other.
Mitigated by testing both sides of the boundary in the same fixture.

### Decision 3 — the v2+ footer is mandatory

`parse` returns `None` for a v2+ file with no `\n<rule>\n` footer, even though
the data block before it is complete and self-consistent.

**For:** RFC 8536 §3.3 requires it, and it is the *only* structural check that
catches a file truncated exactly at the end of its data block — every count
still adds up, every index is in range, and the file looks perfectly valid. A
truncated zoneinfo file that silently becomes a zone with a plausible-looking
history is exactly the failure that is hardest to notice.

**Against:** a hand-written or unusual file with an empty footer is refused
where a more permissive reader would accept it. Judged the right trade: `TZ` is
attacker-shaped input (any libc will open the path you give it), so refusing
the ambiguous case is worth more than accepting the odd one.

### Decision 4 — `standard()`/`daylight()` prefer the tail over the history

The summary accessors that back `tzname[]`, `timezone` and `daylight` report
the footer rule's two halves when there is a footer, and fall back to the most
recent matching type in the table only when there is not.

**For:** it matches glibc, and it matches what a user means. Europe/Moscow has
decades of DST transitions on record and abandoned DST in 2011; São Paulo did
the same in 2019. A "does this zone have DST?" that answers from the history
says yes for both, and `daylight = 1` on a machine in Moscow is simply wrong.

**Against:** it makes the accessors disagree with `lookup()` for a historical
instant — `daylight()` says "no DST half" for a zone that was in DST in 1985.
That is the same thing glibc does, and the POSIX globals are documented as
describing the *current* zone, so the disagreement is in the spec rather than
in us.

### Reversibility

Decision 1 is the structural one: undoing it means giving `TzFile` an owned
table and an allocator, which would change every consumer's type. Decisions 2,
3 and 4 are each a handful of lines in `tzrules/src/tzif.rs` (`lookup`,
`footer_body`, `standard`/`daylight`) with tests naming the behaviour they pin,
so each can be revisited on its own.

## §303 — NPTL validates its scalar argument before it touches the pointer, so an out-of-domain scalar outranks a NULL pointer

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)
**Lane:** B

### The problem

§300 established the rule for *which* errno a NULL pointer produces, and its
Consequences section asked for a per-function audit of the ~330 remaining
`is_null() -> EFAULT` sites. `known-issues.md`'s
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT` singled out
`posix/src/pthread.rs` (47 sites) as "the one that still needs a decision
rather than a lookup", because glibc's NPTL contains **no NULL checks at
all** — every one of these functions simply dereferences and faults. There is
no upstream errno to copy, so the audit appeared to be blocked on a judgement
call about what to substitute.

That framing was wrong. §300's own closing instruction says to "check the
*order* of the validations too, not only the constant — that is what
`ptsname_r` actually got wrong", and ordering is exactly what `pthread.rs` had
wrong, nine times over. The question is answerable from the source without any
judgement at all.

### What the source says

NPTL has one consistent shape. Every entry point that takes both an attribute
pointer and a scalar validates the **scalar first**, returns `EINVAL`/`ERANGE`
from that test, and dereferences the pointer only afterwards:

| Function | glibc source | first test |
|---|---|---|
| `pthread_attr_setstacksize` | `nptl/pthread_attr_setstacksize.c` | `int ret = check_stacksize_attr (stacksize); if (ret) return ret;` |
| `pthread_attr_setstack` | `nptl/pthread_attr_setstack.c` | same `check_stacksize_attr` prologue |
| `pthread_attr_setdetachstate` | `nptl/pthread_attr_setdetachstate.c` | `/* Catch invalid values.  */` on `detachstate` |
| `pthread_mutexattr_settype` | `nptl/pthread_mutexattr_settype.c` | `if (kind < PTHREAD_MUTEX_NORMAL \|\| kind > PTHREAD_MUTEX_ERRORCHECK) return EINVAL;` |
| `pthread_condattr_setclock` | `nptl/pthread_condattr_setclock.c` | full `clock_id` validation |
| `pthread_rwlockattr_setpshared` | `nptl/pthread_rwlockattr_setpshared.c` | `futex_supports_pshared (pshared)` |
| `pthread_barrier_init` | `nptl/pthread_barrier_init.c` | `if (count == 0 \|\| count >= BARRIER_IN_THRESHOLD) return EINVAL;` |
| `pthread_getname_np` | `nptl/pthread_getname.c` | `if (len < TASK_COMM_LEN) return ERANGE;` |

So on Linux, a call that is bad in *both* ways — a NULL pointer **and** an
out-of-domain scalar — returns the scalar's errno, and never faults. Our code
checked the pointer first in every one of these, so it returned `EFAULT` where
Linux returns `EINVAL` or `ERANGE`.

The affinity pair is the interesting case, because the two halves order their
checks *oppositely*, and the asymmetry is the kernel's rather than glibc's
(both glibc wrappers are bare `INTERNAL_SYSCALL_CALL`s):

- `sched_getaffinity` (`kernel/sched/core.c:8506-8509`) has two `EINVAL`
  tests — `len < cpumask_size()` and `len & (sizeof (unsigned long) - 1)` —
  and both sit **above** the `copy_to_user` that would fault. Length wins.
- `sched_setaffinity`'s `get_user_cpu_mask` (`kernel/sched/core.c:8429`) has
  **no** size rejection at all; a short `len` merely leaves the top of the
  mask clear. `copy_from_user` runs first, so the NULL pointer wins. (A short
  `len` still reaches `EINVAL` there, but the long way round: the cleared mask
  is empty, and `__sched_setaffinity` rejects an empty CPU set.)

### The decision

1. **Order our checks the way NPTL and the kernel order theirs.** Scalar
   first in the eight table rows above and in `pthread_getaffinity_np`;
   pointer first in `pthread_setaffinity_np` alone.

2. **Keep `EFAULT` for the pointer itself**, when every scalar is valid. glibc
   segfaults there, which is not an errno we can return, so a substitute is
   unavoidable. `EFAULT` is the right substitute because on the two paths where
   the pointer genuinely does reach a syscall — the affinity pair, and
   `pthread_setname_np`'s `prctl` — `EFAULT` is precisely what Linux gives; a
   caller who checks for it is never surprised by the ones that don't syscall.

The alternative to (2) was to fault as glibc does, on the grounds that
returning *any* errno invents behaviour no Linux program can observe. Rejected:
we are a hosted libc whose callers include our own test suite and our own
userland, and a diagnosable errno is worth more than bug-compatibility with a
segfault. The alternative to (1) was to leave the order alone and document the
divergence. Rejected because the order is part of the ABI — a program that
passes a stack size of 8192 and a null attribute expects `EINVAL`, and getting
`EFAULT` sends it down an error path that was written for a different bug.

### Three latent bugs the audit turned up

Reordering the checks was not the whole of it. Reading the glibc source
line-by-line exposed three cases where our *constants* were wrong too:

- **`PTHREAD_STACK_MIN` was hardcoded as `4096`** in `pthread_attr_setstacksize`
  and `pthread_attr_setstack`, while the crate's own
  `linux_pthread_key_types::PTHREAD_STACK_MIN` is `16384` (glibc's
  `check_stacksize_attr`, `sysdeps/nptl/pthreadP.h:704`, on x86-64). We
  accepted three stack sizes glibc rejects, and contradicted ourselves. The
  setters now use the shared constant.

- **`pthread_getname_np` applied `ERANGE` only when the stored name did not
  fit.** glibc compares `len` against `TASK_COMM_LEN` (16) *unconditionally*,
  never against the name's actual length, so `pthread_getname_np(t, buf, 4)`
  is `ERANGE` even for a two-character name — a call we used to accept. With
  the entry test corrected, the second length test becomes dead and is gone;
  glibc has no second test either.

- **`pthread_barrier_init` accepted `count == u32::MAX`.** That was a latent
  hang: our arrival counter is an `AtomicI32`, so a count above `i32::MAX`
  can never be reached and every waiter would block forever. glibc caps
  `count` at `BARRIER_IN_THRESHOLD` (`UINT_MAX / 2`,
  `sysdeps/nptl/internaltypes.h:119`) for its own reset protocol; we adopt the
  same bound for our own reason, and the constant is now named in the code.

And one errno was simply the wrong constant:
**`pthread_rwlockattr_setpshared` returned `ENOTSUP` for any non-`PRIVATE`
value**, conflating "we don't support cross-process rwlocks" with "that isn't
a pshared value". glibc gates on `futex_supports_pshared`
(`sysdeps/nptl/futex-internal.h:102`), which accepts *both* POSIX values and
returns `EINVAL` for anything else. `ENOTSUP` is our own verdict on
`PTHREAD_PROCESS_SHARED` — which glibc accepts and we do not — and must not
swallow out-of-domain values with it.

### Consequences

Nine functions changed behaviour, all in the direction of matching Linux.
Callers that passed a bad scalar *and* a null pointer see a different errno;
callers that passed only one of the two are unaffected, except for the three
constant fixes above, which reject inputs we previously accepted (an 8 KiB
stack, a short `getname` buffer, a `u32::MAX` barrier). Each is a case where
the old acceptance was itself the bug.

Every changed site carries the glibc (or kernel) file name and the actual
check in a comment, at both the code and the test, per §300's rule that "a test
that encodes an errno with no upstream citation is only evidence that the code
and the test were written by the same pass."

The audit's remaining clusters — `file.rs` (28), `spawn.rs` (16), `socket.rs`
(15), `unistd.rs` (13), `xattr.rs` (11) — are unaffected by this entry's
reasoning, which is specific to NPTL's shape. They are ordinary §300 lookups:
does the pointer reach a syscall or not.

## §304 — A timed wait validates its deadline where *its own* upstream validates it, even though they all share one predicate

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)
**Lane:** B

### The problem

None of `pthread_cond_timedwait`, `pthread_mutex_timedlock` or
`sem_timedwait` checked `abstime->tv_nsec` at all. Adding the check is not the
decision; *where* to add it is. glibc calls one shared inline predicate,
`valid_nanoseconds` (`include/time.h:517`), from all three — which makes it
look as though the natural port is one helper called from one place in each
function, namely the top. That port would be wrong in two of the three.

### What the source says

| Function | glibc source | position of the check |
|---|---|---|
| `pthread_cond_timedwait` | `nptl/pthread_cond_wait.c:635` | **first statement**, above the mutex release |
| `sem_timedwait` | `nptl/sem_timedwait.c:28` | **above** `__new_sem_wait_fast` |
| `pthread_mutex_timedlock` | `nptl/pthread_mutex_timedlock.c:221` | **inside the contended branch**, under the comment "We are about to block; check whether the timeout is invalid" |
| `pthread_rwlock_{rd,wr}lock_full64` | `nptl/pthread_rwlock_common.c:292` | **first statement**, with a comment recording that this was *changed* from lazy to eager |

The observable consequence: `pthread_mutex_timedlock` on an **uncontended**
mutex with `tv_nsec = 1e9` returns 0, while `sem_timedwait` on a semaphore
with a **positive count** and the same timespec returns `EINVAL`. Same
predicate, same class of caller error, opposite answers.

POSIX permits both — "the validity of the abstime parameter need not be
checked if the lock can be immediately acquired" — which is precisely why the
implementations diverged and why the rwlock comment exists: glibc took the
allowance for mutexes, declined it for rwlocks and semaphores, and documented
the switch.

The fourth site is a different predicate entirely. `mq_timedsend` and
`mq_timedreceive` are bare syscalls, so the kernel validates: `prepare_timeout`
(`ipc/mqueue.c`) → `timespec64_valid` (`include/linux/time64.h`), which
rejects `tv_sec < 0` as well as an out-of-range `tv_nsec`. glibc's predicate
never inspects `tv_sec`, so for the pthread and semaphore waits a negative
`tv_sec` is a deadline in the past — `ETIMEDOUT`, not `EINVAL`.

### The decision

1. **Port the predicate once** (`time::valid_nanoseconds`, glibc's definition
   verbatim) but **place each call where its own upstream places it** —
   eagerly in `pthread_cond_timedwait` and `sem_timedwait`, lazily in
   `pthread_mutex_timedlock`.

2. **Do not share the predicate with `mqueue`.** `deadline_from_timespec`
   keeps its own `tv_sec < 0` test on top of `valid_nanoseconds`, because it
   is implementing `timespec64_valid`, not `valid_nanoseconds`.

The alternative to (1) was to check eagerly everywhere: one rule, simpler to
state, and arguably *better* — a caller who passes a malformed timespec has a
bug whether or not the lock happened to be free, and the lazy version makes
that bug appear only under contention, i.e. intermittently. Rejected anyway.
The whole point of this layer is that a program built against glibc behaves
the same here, and a real program does depend on the lazy behaviour by
accident: an uncontended `pthread_mutex_timedlock` with a sloppily-computed
deadline is a common pattern that works on Linux, and failing it would be a
regression visible only to us. The eager version is a defensible libc design
and not the one we are implementing. (§303 made the same call for a different
reason: the order of validations is part of the ABI.)

The alternative to (2) was one predicate for everything, taking the union or
the intersection. Rejected: the union breaks `sem_timedwait` with a
past deadline (`EINVAL` instead of `ETIMEDOUT`), the intersection breaks
`mq_timedsend` with `tv_sec = -1` (a very long wait instead of `EINVAL`).
Two rules that differ by one line and cannot be merged.

### Consequences

Anyone adding `pthread_cond_clockwait`, `sem_clockwait` or the
`pthread_rwlock_{timed,clock}{rd,wr}lock` family must look up that function's
own placement rather than copying a neighbour's. The rwlocks are **eager**,
and the clock-taking variants additionally reject an unsupported `clockid`
(`futex_abstimed_supported_clockid`) *before* the nanoseconds check.

The general form of the rule, which outlives this entry: a shared helper
upstream is evidence about the *predicate*, not about the *control flow*.
§303's "port the upstream helper rather than the check" is necessary and not
sufficient.


## §305 — osh ships as the shell, cross-compiled bash ships beside it, and osh's bash-fidelity scope is **frozen**

**Date:** 2026-08-14
**Decided by:** Operator (Claude recommended this option; the operator asked the
question that exposed the problem in the first place — see the history below)

### The question

Should SlateOS's POSIX shell be `userspace/oils` (osh — a 141,899-line Rust
reimplementation, 1,210 commits, 662/662 corpus cases byte-exact against bash
5.2.37), or the genuine GNU bash 5.2 that we can now cross-compile and which
**already boots and runs on this OS**? Tracked as open-questions **Q41**, now
closed by this entry.

### Decision — the hybrid, with a stopping criterion

Three parts, and the third is the one that actually changes day-to-day work:

1. **osh remains the shell.** It works today, it is ours to debug and extend,
   it needs no dynamic linker, and 141k lines of it are already correct. No
   rewrite, no deletion, no migration project.
2. **The cross-compiled bash stays and is a first-class artifact.** It is the
   escape hatch (a script that genuinely needs exact bash can run exact bash)
   and, longer term, the differential oracle *running on SlateOS itself* —
   which is the path to dropping `scripts/osh-bash-diff.py`'s dependency on a
   host Linux/MSYS reference bash. Keep `scripts/bash-spike/` reproducible and
   keep `self_test_bash_on_slateos_libc` green.
3. **osh's fidelity scope is frozen.** Byte-for-byte parity with bash is no
   longer an open-ended goal. The corpus stops growing for its own sake. The
   stopping criterion is written out below, and it is binding.

### The stopping criterion (read this before opening a `TD-OILS-*` entry)

**Fix an osh divergence when at least one of these is true:**

- **Something on SlateOS actually hits it.** A real script, service, init file,
  build step, package recipe, test harness or interactive session that we ship
  or run. "A user might type this" does not qualify; "our own `create-ext4-rootfs.sh`
  types this" does.
- **It is a crash, hang, wrong-exit-status-that-propagates, data-loss or
  security bug.** These are bugs on their own terms, independent of bash.
- **It is a regression** against a corpus case that is already green. The 662
  existing cases stay green; that is a floor, not a ceiling.

**Do not fix — and do not write a corpus case for:**

- **Diagnostic wording, spelling, and the exact substring a message echoes.**
  Two of the last three sessions' fixes were of exactly this kind: which of
  bash's two messages an unterminated `${` takes when its *name* scan runs off
  the text, and the precise `%s` slice in ``bad substitution: no closing "`" in
  …`` when an unmated backquote sits inside a single-quoted run in an array
  subscript. Both are now correct, both were verified byte-exact, and neither
  will ever matter to anything running on this OS.
- **Artifacts of bash being a 40-year-old C program.** The canonical example,
  already in the corpus: `OPTIND=4294967297` wraps to the first argument
  because bash stores it in a C `int`. That is a fact about `int`, not about
  shells.
- **Constructs only reachable by adversarial or nonsense input** whose only
  observable difference is which error text appears.

**When a divergence is real but out of scope**, the answer is now bash: note it
in the `TD-OILS-*` entry as `SCOPE: out of frozen scope (§305)` and move on.
Do not open a fidelity investigation.

### Why this and not the alternatives

- **B — keep osh, close Q41 permanently.** Rejected as stated, because it
  answers the feasibility question that was already settled and leaves the
  *actual* problem — that byte-for-byte fidelity has no stopping criterion —
  exactly where it was. This entry is B plus the missing stopping criterion,
  which is why it supersedes rather than contradicts it.
- **C — switch to cross-compiling bash, retire osh.** Rejected: it discards
  141k lines of working, byte-exact, dependency-free Rust in exchange for
  maintaining a fork of 40-year-old C, and osh's independence from a C
  toolchain has value on an OS whose toolchain story is still moving. Note
  that this rejection is *not* on feasibility grounds — bash demonstrably runs.
- **The hybrid (chosen).** Gets C's benefit (fidelity is available exactly,
  for free, when something actually needs it) without C's cost (no rewrite, no
  fork to maintain), and converts the unbounded chase into a bounded job.

### The history this entry exists to prevent repeating

This is the part a future session must read. **§72 chose the Rust
reimplementation on one decisive fact — "there is no C/C++ → `x86_64-slateos`
cross-toolchain in this repo" — and wrote its own reversal condition. The
condition fired four days later and nobody checked for twenty-five days.**

| Date | Event |
|---|---|
| 2026-07-18 | `userspace/oils` begins; §72 rejects the cross-compile as prerequisite-blocked |
| 2026-07-21 | `x86_64-slateos` C cross-target lands (fastpy, initiative F) |
| 2026-07-22 | `zig cc` wired in; `toolchain/sysroot/lib/libc.a` exists — **§72's premise is now false** |
| 2026-08-12 | The operator asks the question. Spike measures it: **bash 5.2 boots and runs on SlateOS**, 5,349,720-byte static ELF against our own `libc.a`, zero undefined symbols, no shims, three small `posix/src` additions (`killpg`, `eaccess`/`euidaccess`, `__fpurge`) |
| 2026-08-14 | This entry |

Roughly 1,100 of oils' 1,210 commits postdate the moment §72's stated blocker
ceased to exist. The original call was correctly reasoned **on the facts of its
day**; the failure was procedural, not analytical — a decision carrying an
explicit expiry condition was never re-audited, and a large initiative kept
compounding on a premise that had quietly become false.

### The general rule this establishes

**A decision whose rationale rests on a stated prerequisite being absent must
name the condition that reverses it, and that condition must be *checked*, not
merely recorded.** Concretely, for this repo:

- When a `design-decisions.md` entry contains a "How to reverse" / "revisit if
  …" clause, the reversing condition belongs in `todo.txt` as a live item, not
  only in the entry's prose. A clause nobody is scheduled to evaluate is a
  comment, not a control.
- **When you build a capability, grep the design decisions for entries that
  were rejected for lack of it.** The `zig cc` work of 2026-07-21/22 was the
  moment to re-read §72, and the person best placed to notice was whoever
  landed the toolchain. `grep -n "no C/C++\|cross-toolchain\|prerequisite"
  design-decisions.md` would have found it.
- Prefer rationales that rest on *tradeoffs*, which age slowly, over rationales
  that rest on a *missing prerequisite*, which can evaporate overnight.

### Where it bites

`design-decisions.md` §72 (superseded in part — its prerequisite claim and its
"How to reverse" clause are both spent; see the pointer added there),
`open-questions.md` Q41 (closed by this entry), `roadmap.md` §2.7 and the Lane B
backlog summary, `roadmap-detailed.md` §2.7, `todo.txt`, `known-issues.md` (the
whole `TD-OILS-*` family is now scope-gated by the criterion above),
`userspace/oils/` (all of it), `scripts/osh-bash-diff.py` and
`userspace/oils/tests/corpus/` (the corpus no longer grows for its own sake),
`scripts/bash-spike/`, `scripts/create-ext4-rootfs.sh` (stages `/bin/bash`) and
`kernel/src/proc/spawn.rs::self_test_bash_on_slateos_libc`.


## §306 — The shared documents stay per-branch; a fetch/merge cadence is what keeps them current

**Date:** 2026-08-14
**Decided by:** Claude (operator-approved scope — the operator was offered
three options and delegated the choice: "do whichever one you think is best")

**The question.** `roadmap.md`, `known-issues.md`, `design-decisions.md`,
`open-questions.md`, `todo.txt` and `requests/` are ordinary tracked files, so
each lane branch carries **its own copy** of every one of them. Should they
instead live in one place — on `main` only — so that all three lanes read the
same bytes?

**What prompted it.** Two failures on the same day, both mine:

1. `requests/a-b-init-conflates-syscall-error-with-exit-code.md` — Lane A's
   report that `services/init/src/main.rs` treats `process_try_wait`'s negative
   kernel error as a child exit code — sat on `origin/main` **unread for a
   day**. It was never in my worktree, because `lane-b` had never once fetched
   or merged since the split (55 ahead, 72 behind).
2. I then diagnosed the repo's integration state by reading the shared docs in
   `D:\visual studio projects\os`, and concluded that no lane had ever merged
   to `main`. That was **wrong**: the `os` directory is a checkout of `main`
   sitting **67 commits behind `origin/main`**, and Lane A had in fact merged
   (`6d69d308e`). I recommended an architecture change to the operator on the
   strength of it, then had to retract the reasoning before it was acted on.
3. **A third surfaced while writing this entry, and it is the cleanest
   illustration of the three.** The first boot test after the merge failed on
   a missing `limine/BOOTX64.EFI`, and before that on six missing service ELFs
   that `kernel/src/main.rs` `include_bytes!`es. Every one of them is
   provisioned by `scripts/bootstrap-worktree.sh` — a script that exists *for
   exactly this purpose*, that landed on `main` on 2026-08-13 (`0d013beb1`,
   `60dab49d5`), and that `lane-b` had never seen. I was one `git merge` away
   from a provisioned worktree for a day, and instead diagnosed it as a
   sequence of unrelated build failures.

**The decision.** Keep the per-branch copies. Add an explicit **sync cadence**
instead, recorded in `roadmap.md` §5.5 and mirrored into `CLAUDE.md`:

- `git fetch origin && git merge origin/main` at the **start** of every task;
- push the lane and **merge the lane up into `main`** at the **end** of every
  green one;
- `origin/main` is the trunk — the `os` directory is a *view* of it that may
  be arbitrarily stale, and must be pulled before it is read as authoritative;
- **merge, never rebase**, when integrating `origin/main` into a lane (see
  below).

**The alternative, and why not.** Option B was to move the shared docs to
`main` only. It buys freshness by construction and would have prevented failure
(1) outright. It was rejected because:

- **It trades away worktree isolation — the one property the three-lane
  arrangement exists to provide.** Editing a doc that lives only on `main`
  means three agents writing the same file in the same checkout, which is
  precisely the clobbering failure mode the lanes were drawn to prevent. It
  would reintroduce it for exactly the files that are hardest to reconstruct
  from memory.
- **The per-lane conventions demonstrably work.** The merge that closed the
  72-commit gap produced **zero** conflicts in `design-decisions.md` — the
  §200/§300/§400 numbering split doing its job across two lanes appending
  simultaneously — and five elsewhere, every one of them the trivial "both
  lanes appended at the same spot" shape. Total resolution: minutes.
- **The failure was not structural.** The dropbox was not broken; nobody
  emptied it. A problem that regular fetching solves does not justify
  surrendering a safety property.

A narrower slice of B — make `requests/` alone main-only — was also considered
and dropped: a lane cannot write to the `os` worktree (the rule forbidding it
is not negotiable), so filing a request would still require getting a commit
onto `main`, which is the same merge-up step the cadence already mandates. It
would add a mechanism without removing one.

**A correction that came out of this.** `roadmap.md` rule 5 said "**Rebase on
`main`, never merge**". That is unsafe now: the lane branches are *published*
at `origin/lane-<x>`, so rebasing one requires a force-push — which the very
next bullet of the same rule forbids outright, because it destroys the other
lanes' work. The rule contradicted itself, and following its first half is what
would have stranded `lane-b`. It now reads: merge `origin/main` in; rebase
remains fine only for commits never pushed.

**Where it bites:** `roadmap.md` §5 and the new §5.5, `CLAUDE.md` ("Three
Sessions", "Branch Strategy", "When You Start a Task" step 1, "When You Finish
a Task" step 11, and the push bullet under Autonomous Work), and
`requests/b-a-fetch-and-merge-main-every-task.md` /
`requests/b-c-fetch-and-merge-main-every-task.md`, which carry the rule to the
other two lanes — since the only correct way to change their copy of a shared
document is to land it on `main` and let them merge it down.

---

## §307 — A failed port is a bug report against our libc; the default is to fix the libc and re-try, not to reimplement

**Date:** 2026-08-14
**Decided by:** Operator (Claude proposed the framing, the four-way triage and
the two refinements; the operator raised the question — "if upstream pkgconf
doesn't build against our libc, could that be taken as a suggestion to improve
our libc for it and for future apps instead?" — and made the call)

**The question.** `roadmap-detailed.md` now tells you to try cross-compiling an
existing C/C++/Rust program before writing a replacement for it. That rule only
covered the *success* branch. What is the correct response when the port does
**not** build against `toolchain/sysroot/lib/libc.a`?

**The decision.** A failed link is treated as a **defect report against the
libc**, and the default action is to implement the missing surface in `posix/`
and re-try the port. "Fall back to reimplementing the application" is not the
default and now requires a reason from the triage below.

**Why the operator's framing is right, stated as reasons rather than assertion:**

- **A real port is the only honest coverage test a libc has.** You cannot guess
  which of several thousand symbols matter; real software tells you, weighted by
  actual usage rather than by what looked important when the header was written.
- **The fix compounds; a rewrite does not.** A libc function added for one port
  is inherited free by every later port. A reimplementation helps exactly one
  program, and then has to be maintained forever against upstream's bug fixes.
- **The cost asymmetry points the same way.** Implementing a missing libc
  function is minutes to an hour. Reimplementing the application is hours to
  days — and §305 is the measured proof of how far that can run (~25 days on a
  bash reimplementation, against ~a day for cross-compiling GNU bash as-is).
- **Precedent, not theory.** In `scripts/bash-spike/`, `libc.a` defined 2,900
  symbols, bash referenced 2,030, and the first SlateOS link resolved all but
  **three** — `killpg`, `eaccess`/`euidaccess`, `__fpurge`. All three were
  implemented for real in `posix/src` rather than shimmed. One port attempt
  converted into permanent coverage for everything that follows.

**The nuance that makes it a decision rather than a slogan — triage.** Only the
first two categories mean "improve the libc":

1. **Missing standard POSIX/C function** → implement it in `posix/`. Highest
   leverage, and the common case.
2. **Missing non-standard extension** (glibc/BSD-isms) → usually implement, but
   check first whether upstream's `configure` already has a fallback path, in
   which case the gap is imaginary and the fix is to let autoconf find it.
3. **Architectural mismatch → do *not* grow the libc into it.** If the program
   needs something SlateOS deliberately rejects — Unix signals as native process
   control, 4 KiB page assumptions, ambient-authority fds, `/proc` special nodes
   — the Linux Compatibility Boundary governs: `ENOSYS`, or emulation *inside*
   the compat layer, never a hack into native code to satisfy a Linux quirk.
   This is the one case where a failed port genuinely says stop.
4. **Not a libc gap at all** → build-system friction (cross-compile detection,
   sysroot plumbing). Fix the build. The bash spike's own example: `$CC` cannot
   contain spaces because autotools word-splits it, and this repo lives under
   `D:\visual studio projects\`, so the first attempt died with a thoroughly
   misleading "C compiler cannot create executables".

**Two refinements, both learned from that spike:**

- **A kernel gap can masquerade as a libc gap.** `killpg` exists now and still
  returns `ENOSYS`, because process groups do not exist in the kernel — yet the
  symbol had to exist, since bash references it from job-control code and the
  *link* needs it even where job control cannot work. Implement the symbol
  honestly; file the underlying gap against the kernel (cross-lane: a
  `requests/` entry, not an edit outside your lane).
- **Whatever you add must actually work.** "Never accept-without-honoring"
  applies at full force: a stub returning success is *worse* than `ENOSYS`,
  because the port links, appears to work, and fails subtly later. `killpg`
  reporting `ENOSYS` truthfully is the correct shape.

**Where it flips.** The argument is leverage, so it evaporates when there is
none. If one port needs a large, exotic subsystem nothing else will ever use,
that is cost without reuse — weigh it like any other feature rather than
treating "it improves the libc" as automatically decisive.

**The alternative that was rejected**, and why it is tempting: treat a failed
build as evidence that the program is "not portable to SlateOS" and write our
own. It is tempting because it is *unblocking* — reimplementation never fails to
link, so it always feels like progress, and the failure it replaces is concrete
and immediate while the compounding benefit is diffuse and deferred. That is
precisely the bias this entry exists to counter: the cheap-feeling path is the
one that costs 25 days.

**Where it bites:** `roadmap-detailed.md` §"Porting vs. Reimplementing" (the
sub-block "When the port *fails*, that is usually a bug report against our libc
— not a verdict on the port"), the whole of `posix/`, and — immediately —
`userspace/pkgconf/`, whose in-progress Rust rewrite prompted the question and
whose fate now depends on whether upstream pkgconf cross-compiles.


## §308 — A private file stays out of GitHub via a pre-push hook plus an orphan branch, not by being untracked

**Date:** 2026-08-14
**Decided by:** Claude (operator-approved scope) — the operator set the
requirement verbatim ("todo2.txt can be committed to the local git, but i don't
want it on github"); the mechanism below is my call and is mine to revisit.

**The requirement, and why it is not trivially satisfiable.** Git has no
per-file push filter. A pushed branch carries every file its commits contain,
and every earlier version of that file that its history contains. So "tracked
locally but never published" is not a property you can attach to a *file* — it
has to be enforced at the boundary where publication actually happens.

That matters more here than in a normal repo, because three autonomous agents
push freely and often, under a standing instruction to "push often, on your own
volition". Anything tracked on a shared branch reaches GitHub within minutes,
by design. A convention would not survive that.

**Decision — three parts, each covering a gap the others leave:**

1. **`/todo2.txt` stays in `.gitignore` on the shared branches.** This is the
   cheap guard: it stops the file being swept up by a `git add -A`, which is
   how it would realistically get committed by accident.
2. **A `pre-push` hook (`scripts/hooks/pre-push`) refuses any push whose new
   commits add or touch a guarded path.** This is the guard that actually
   implements the operator's requirement, because it holds even when the file
   *is* committed. It is deliberately not a blanket block: it inspects the tip
   tree of each ref being pushed and the commits not yet on any remote-tracking
   branch, so ordinary lane pushes are untouched and only an actual leak is
   stopped. `ALLOW_PRIVATE_PUSH=1` bypasses it for the day the operator changes
   their mind.
3. **Local history lives on the orphan branch `private/todo2`,** appended to by
   `scripts/snapshot-todo2.sh` using plumbing (`hash-object` / `mktree` /
   `commit-tree` / `update-ref`) so no worktree HEAD ever moves. This is the
   part that delivers "can be committed to the local git" for real — the file
   gets genuine version history and diffs — without that history riding on a
   branch anyone pushes.

**Why the file cannot simply be committed on `main` or a lane branch.** This is
the option that first looks right and is in fact the worst one. Those branches
are pushed constantly; with the hook in place, committing `todo2.txt` on `main`
would make *every subsequent push of `main` by any of the three lanes* fail
until someone removed it. The requirement would be met and the project would be
bricked. An orphan branch shares no commits with the project, so it can never be
dragged along by a merge or a push of something else.

**Alternative considered: a separate local-only repository** (a bare repo
outside the tree, driven with `--git-dir`/`--work-tree`). Strictly safer — there
is no remote at all, so no hook to forget. Rejected because it puts the
operator's file under a second VCS with its own path incantations for every
read, and because the failure it prevents (a hook lost to a fresh clone) is
already handled: the hook source is *tracked* at `scripts/hooks/pre-push` and
`scripts/install-hooks.sh` re-arms it in one command. Worth revisiting if the
set of private files ever grows beyond one.

**Alternative considered: rely on `.gitignore` alone** (the status quo before
this entry). Rejected because it answers a different question. Ignoring a file
prevents it being *staged*; it does nothing once the file is tracked — and the
file *was* tracked on all five branches until 2026-08-14, which is exactly how
it reached GitHub in the first place. `.gitignore` guards the input; the hook
guards the output; the operator asked about the output.

**Limits, stated plainly.** The hook stops *future* publication. It does not
remove `todo2.txt` from history already on GitHub — every revision pushed
before 2026-08-14 is still reachable there, and purging it would require
rewriting published history and force-pushing, which this project forbids and
which would break all three lanes. Hooks are also per-clone and not carried by
`clone`/`fetch`; all four worktrees share one `.git`, so one install covers
every lane today, but a fresh clone starts unarmed.

**Where it bites:** `.gitignore`, `scripts/hooks/pre-push`,
`scripts/install-hooks.sh`, `scripts/snapshot-todo2.sh`, the orphan branch
`private/todo2` (never merge it into anything), and
`requests/b-a-todo2-untracked.md` / `requests/b-c-todo2-untracked.md`, which
tell lanes A and C why a push of theirs might be refused.

---

## §309 — Byte-fidelity with bash has an "unless it is a defect" clause; osh does not reproduce the null array element

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this option — open-questions.md Q40 option B)

**The question.** osh is held to byte-fidelity with bash 5.2.37. One measured
bash behaviour, reachable only through a nameref, stores a **null pointer** into
an array element (`n=(a b c); declare -n q='n[1]'; declare q`), after which the
array reads as empty while its elements are demonstrably still present. It looks
like a defect rather than a design: bash cannot describe the resulting state
with any of its own printers, no bash-level operation other than this one can
produce it, and the bind carries `ASS_FORCE` so it also silently defeats
`readonly`.

**Decision: do not reproduce it (option B).** osh keeps `Str` array elements and
the array reads normally. The divergence is waived in the corpus and the full
write-up stays in `known-issues.md` →
`TD-OILS-A-DECLARATION-WITH-NOTHING-TO-DO-BINDS-A-NULL-THROUGH-THE-REFERENCE`,
so the decision is reversible if a real script is ever found that depends on it.

**What this actually settles — the precedent, not the bug.** The narrow
cost/benefit was never close: option A wanted every array reader in `interp.rs`
rewritten around an `Option<Str>` element type, permanently, to preserve a state
no bash-level operation can otherwise produce. The reason it needed the operator
is that it establishes **whether byte-fidelity has an "unless it's a bug" clause
at all**. It now does. Consequences:

- "The measurement wins" is no longer absolute. A measured bash behaviour may be
  waived when it is (i) unreachable except through a construct built to reach
  it, (ii) inconsistent with bash's own observable model, and (iii) expensive to
  reproduce in a way that degrades osh's value model.
- Every future waiver must be argued against those three tests **in
  `known-issues.md`**, not decided silently. A waiver that is not written down is
  a divergence, not a decision.
- This does not loosen §305's frozen fidelity scope. §305 says which behaviours
  are in scope; this says a behaviour in scope may still be waived as a defect.

**Rejected alternative — option C, reproduce only the visible half** via an
out-of-band "poisoned index" marker. It is a second parallel representation of
emptiness, threaded through the same readers as option A, for a less honest
model — most of A's cost without A's one virtue.

**Where it bites:** `userspace/oils/src/interp.rs` (`Shell::declare_ref_bind_read`,
`Shell::arrays`, `Shell::assoc`), the corpus case
`a-declaration-with-nothing-to-do-evaluates-the-subscript-the-reference-carries.sh`
(which covers the evaluated-subscript half osh *does* match and deliberately
stops short of the store), and `known-issues.md` as above.


## §310 — One-shot repo-wide rustfmt, with a `.git-blame-ignore-revs` file alongside

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this option — open-questions.md Q42 option A)

**The question.** `CLAUDE.md` sets the convention as "rustfmt defaults, no manual
formatting overrides", but two crates are not rustfmt-clean: `kernel` (16 911
hunks) and `posix` (389 hunks across 244 of 2 299 files). Because `cargo fmt` is
package-scoped with no file filter, formatting your own change in a drifted
crate rewrites hundreds of files you never touched — one ~150-line edit in
`posix` produced a 1 403-insertion / 1 429-deletion diff across 173 files that
could not afterwards be separated from the real change, costing a
revert-and-redo.

**Decision: option A — reformat the whole repo once, and commit a
`.git-blame-ignore-revs` file naming the reformat commits.** Afterwards
`cargo fmt` is safe, the stated convention becomes true rather than
aspirational, and any future drift is a real diff.

**The cost, stated honestly.** This rewrites `git blame` for ~17 000 hunks of
kernel code. Blame is the primary tool for "why is this line here?" in a
codebase with no human reviewer and a 4 600-commit history, and this is the one
part that cannot be undone. `.git-blame-ignore-revs` mitigates it for anyone who
configures it (`git config blame.ignoreRevsFile .git-blame-ignore-revs`) and for
`git blame --ignore-rev`, but **not** for GitHub's plain blame view or a casual
`git log -S`. The operator accepted that trade: the trap is permanent and recurs
on every edit, the blame churn is one-time.

**Execution constraints that shape how it lands.**

- `cargo fmt --all` does **not** run in this workspace — it dies with
  `The filename or extension is too long. (os error 206)`, the Windows
  command-line limit, hit by the sheer number of workspace members. The
  reformat must iterate crates one at a time.
- It is **two commits in two lanes**, not one. `posix/` is Lane B's and
  `kernel/` is Lane A's; a single cross-lane reformat commit would be exactly
  the clobbering the lane split exists to prevent. Each lane reformats its own
  crate, and both commit hashes go into `.git-blame-ignore-revs`.
- Each reformat commit must contain **nothing but** formatting, so that
  `--ignore-rev` is safe to apply wholesale.

**Rejected alternatives.** **B** (format only files you edited, via
`rustfmt --edition 2024 <file>`) was the working stopgap and has zero history
churn, but leaves the trap armed for anyone reaching for the obvious command and
never shrinks the drift. **C** (reformat `posix` only) clears the crate under
daily work for 1.5% of A's blame cost, but leaves the worst offender armed — and
a half-applied convention is the state that caused the incident.

**Where it bites:** every `.rs` file in `kernel/` and `posix/`, the new
`.git-blame-ignore-revs`, `CLAUDE.md`'s formatting convention (now true), and
`known-issues.md` → `TD-REPO-IS-NOT-RUSTFMT-CLEAN-SO-RUNNING-CARGO-FMT-IS-A-TRAP`.


## §311 — Ship full tzdata, vendored as prebuilt TZif binaries, updated as a `pkg/` package

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this combination — open-questions.md B-Q1, A1 + B1 + C1)

**The situation.** Both the libc and osh resolve `TZ` through real binary
zoneinfo: `tzrules::TzFile` reads TZif v1/v2/v3 (RFC 8536) with no allocator,
`TZDIR` defaults to `/usr/share/zoneinfo`, and an unset `TZ` follows
`/etc/localtime` exactly as glibc does. Every piece was built except the data,
so `TZ=America/New_York` silently answered UTC — the user gets UTC while
believing they selected Eastern. Shipping the data is a packaging decision, not
a coding one, which is why it went to the operator.

**Decision, in three parts:**

- **(a) A1 — full tzdata**, including the `backward` compatibility links
  (`US/Eastern`, `Asia/Calcutta`). ~450 KiB and ~1 800 files in every base
  image. Chosen because ~450 KiB is nothing against being *wrong* about
  `US/Eastern`, and because the entire reason to use TZif rather than invent a
  format is that ported programs expect exactly what everyone else ships.
  A2 (`zic -b slim`, no backward links) saves ~200 KiB and breaks a very common
  spelling **silently, back to UTC** — the same failure mode this work exists to
  end. A3 (minimal at install, rest as a package) leaves a fully-installed-looking
  machine unable to resolve a zone the user did not personally pick.
- **(b) B1 — vendor the prebuilt binaries** from the IANA distribution,
  checked in and version-pinned. Reproducible, no build dependency, ~450 KiB of
  binary per update in git history. B2 (port `zic`) and B3 (write our own TZif
  generator in Rust) were both rejected for the same reason: `zic` is a real
  compiler for the tzdata source grammar, and getting it subtly wrong produces a
  **wrong clock that nobody notices for months**. B3 is the most likely of the
  three to be subtly wrong and would put that risk on our own code.
- **(c) C1 — updated as a `pkg/` package.** tzdata changes several times a year
  at short notice; that cadence is exactly what `pkg/` exists for. C2 (ship with
  the OS image only) ties a timezone fix to a full release. C3 (a dedicated fast
  channel for tzdata alone) is infrastructure to build only once C1 has proven
  too slow in practice — not before.

**The residual risk this accepts.** A user who never runs `pkg update` drifts
into a stale tzdata and therefore a wrong wall clock, with nothing loud to tell
them. If that proves common, C3 is the escalation, and it is additive.

**Cross-lane note.** The reader, the libc paths and osh are Lane B; the `pkg/`
packaging is **Lane C's tree**, so the C1 half lands via a `requests/` entry
rather than directly.

**Where it bites:** `pkg/` (Lane C), `posix/src/tz.rs` (`TZDIR_DEFAULT`,
`LOCALTIME_PATH`, `load_zoneinfo`), `userspace/oils/src/interp.rs`
(`TZDIR_DEFAULT`, `Shell::zoneinfo_dir`), `tzrules/src/tzif.rs` (the reader,
already done), the installer (which must write `/etc/localtime`), and the two
tests that assert the current UTC fallback and **must start failing the day the
data lands** — `test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped`
(libc) and `printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve` (oils).

## §312 — libc's Linux capability words are a conservative projection of the kernel's `(ResourceType, Rights)` handles, not a fiction

**Date:** 2026-08-15
**Decided by:** Operator (Claude recommended this option)
**Question:** `open-questions.md` Q44 (now RESOLVED)
**Answer, verbatim:** *"Q44: a."*

### The decision

**Option A — conservative projection.** Each Linux `CAP_*` bit is derived from a
specific `(ResourceType, Rights)` predicate over the capabilities the process
actually holds, and reports **not held** whenever no rule matches. The default
is *deny*, not *allow*: an unmapped `CAP_*` is false, never true.

Worked examples of the rule shape:

| `CAP_*` | Predicate over held capabilities |
|---|---|
| `CAP_SYS_RAWIO` | any `PortIo` handle with `READ` or `WRITE` |
| `CAP_KILL` | any `Process` handle with `SIGNAL` |
| `CAP_SYS_PTRACE` | any `Process` handle with `DEBUG` |
| `CAP_SYS_NICE` | any `Thread` handle with `IO_REALTIME` |
| `CAP_NET_RAW` | any `NetRaw` handle |
| `CAP_SYS_ADMIN` | **hand-maintained union** — see below |

`CAP_SYS_ADMIN` is the exception and is explicitly *not* derived. It is Linux's
junk drawer, it has no natural preimage in a per-object model, and it accounts
for 20 of libc's 63 gate sites. It gets an explicit, hand-written union of the
predicates that each of those 20 sites actually needs, maintained as a list with
a comment per member. A derived rule for it would either be permanently false
(breaking 20 sites) or so broad that it re-grants everything (which is the bug
being fixed).

### What this replaces

`posix/src/sys_capability.rs` kept Linux's three capability words in libc's own
memory and initialised them from `CAPS_DEFAULT` with **every bit set**. Nothing
ever asked the kernel what the process held, so `capget()` reported the full set
to a process spawned with `capabilities: &[]`, and every libc-side gate passed.

That was safe only by accident: the kernel re-checks every privileged operation
itself, so libc's optimistic answer could never *grant* anything. The failure
mode is the silent one — a port that trusts `capget()` to decide what to attempt,
or to drop privileges, gets a confidently wrong answer with no error anywhere.

### Why A and not the others

- **B (a `ResourceType::PosixCapability` handle granted per `CAP_*` at spawn)**
  was rejected on principle. It is ambient authority wearing a capability
  costume: a handle meaning "may do `CAP_SYS_ADMIN` things", process-wide, tied
  to no object. It reproduces exactly the property `design.txt` rejects while
  looking compliant because it is spelled as a handle. Notably it is the option
  that would have made `CAP_SYS_ADMIN` *easy*, and it was still not worth it.
- **C (keep libc optimistic, document `capget()` as "the ceiling, not the
  grant")** is the honest do-nothing. It breaks no fixture and carries no risk,
  but it leaves the silent-wrong-answer trap open — which is the entire reason
  `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S` was logged.
- **D (make `capget()` fail with `ENOSYS`)** is the most honest answer and the
  worst outcome for a compatibility layer. Linux software calls `capget()`
  informationally and does not expect failure; this trades one silent wrong
  answer for loud breakage across every port.

### Consequences, in the order they have to happen

1. **An enumerating capability query syscall must exist first.** This is not
   optional under any option and is not in dispute. `SYS_CAP_QUERY` (400,
   `kernel/src/syscall/handlers.rs::sys_cap_query`) returns only a *count* of
   the caller's capabilities; its own doc comment says "a future extension will
   support filling a user-space buffer with detailed capability entries", and
   its sole consumer today is `userspace/strace`'s syscall name table. **That
   handler is Lane A's tree** — filed as `requests/b-a-cap-enumerating-query-syscall.md`.
2. **libc seeds its words from that query**, once, at process start, and on
   demand after any operation that could change the set.
3. **The libc gates stay advisory until the fixtures are given real
   capabilities.** This is the staging that keeps the change from being a
   flag-day. Making a gate truthful breaks every fixture that relies on the
   permissive behaviour, and there are known ones:
   `services/ctest-jobctl` (its doc comment says so out loud — "our libc's own
   `CAP_KILL` gate reads the process capability words, which start out with
   every capability held"), `self_test_cctty`, and `self_test_cpgroup`, all
   spawned with `capabilities: &[]`. That is boot-test-visible, so the flip
   from advisory to enforcing lands with QEMU free.

### The cost accepted

`capget()` becomes truthful, which means code that previously sailed through a
libc gate now gets refused by it. That is the point — but it means the flip is a
behavioural change to every fixture's effective privilege, not a no-op refactor,
and it must be sequenced (step 3) rather than switched on with the mapping.

**Where it lives:** `posix/src/sys_capability.rs` (`CAPS_DEFAULT` ~line 251),
the 63 gate sites led by `posix/src/process.rs` (13) and `posix/src/unistd.rs`
(10), `kernel/src/cap/mod.rs` + `kernel/src/cap/rights.rs` (the model being
projected), `kernel/src/syscall/handlers.rs` (`sys_cap_query`), and
`known-issues.md` → `TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S`.

## §313 — `open-questions.md` is written for a reader who does not know the subsystem, and questions that are not yet answerable move to `deferred-questions.md`

**Date:** 2026-08-15
**Decided by:** Operator (operator's own proposal, both halves)

**In short:** the operator said they often cannot understand the entries in
`open-questions.md` — partly terse wording, but mostly terms they do not know.
They also pointed out that one entry (Q39) says in its own text that now is not
the time to decide it, and asked whether it should live somewhere else. Both
observations are about the same failure: the file is supposed to be a list of
things the operator can act on, and it had drifted into being a list of things
Claude found interesting.

### The problem, stated plainly

`open-questions.md` is the **operator's decision queue**. Its only job is to let
the operator make a decision. An entry that is technically flawless but not
understandable has failed at that job completely — it produces no decision, and
worse, it produces *silence*, which is indistinguishable from "not yet read".
Several of today's answers arrived days after the questions were filed for
exactly this reason, and one (`Q44`) came back not as an answer but as a
question about a term used in the question itself ("i forget what 'ambient'
means").

The second failure is padding. Q39's own recommendation section read "None yet,
on purpose… Ask again then." A queue containing items that say *do not act on
this* teaches the reader to skim the queue, which costs attention on the items
that genuinely need it.

### Decision, part 1 — legibility rules for `open-questions.md`

Recorded in `CLAUDE.md` under the `open-questions.md` bullet, and mirrored in
the file's own header:

- **Every entry opens with `In short:` — 2–4 sentences, no jargon at all**:
  what is wrong now, what a user would actually see, what the choice is between.
  If a term of art seems unavoidable there, the paragraph is wrong.
- **Every term of art is glossed in-line on first use, in ≤ 10 words**, even if
  it was glossed in another entry, another file, or last week. The operator
  reads one entry at a time, months apart; nothing carries over.
- **Every option carries a one-line `What changes:`** stated as an observable
  difference ("the clock reads Eastern instead of UTC"), not an implementation,
  so options can be compared without reading the prose.
- **Every entry says what happens if it is never answered** — is today's
  behaviour safe, is anything blocked, does it get worse with time.
- **Entries are capped to what a decision needs.** Detail that only matters
  *after* the answer goes in `known-issues.md` or the `requests/` file. Prefer a
  table to a paragraph, an example to an abstraction.
- The same `In short:` opener applies to `design-decisions.md` entries, so a
  decision can be re-read a year later without reconstructing the context.

**The tension this creates, and how it resolves.** The operator also said they
do not want *a lot more reading*. Glossing terms and adding a summary both add
words, so the rules above are not purely additive — the length cap is part of
the decision, not a footnote to it. The `In short:` paragraph is meant to
*replace* the rambling first section of an entry, and the "cap it to what a
decision needs" rule is what pays for the glosses. An entry that gets longer
overall has applied the rule wrong.

**Alternative rejected: a project glossary.** A central `glossary.md` looks like
the tidy answer — define "ambient authority" once, link it everywhere. It was
rejected because it optimises for the writer, not the reader: it turns one
unfamiliar word into a click, a context switch, and a second document, at the
exact moment the reader is trying to hold a tradeoff in their head. A ten-word
parenthetical costs the writer a repetition and costs the reader nothing. The
repetition is the feature.

### Decision, part 2 — `deferred-questions.md`

A new shared document at the repo root for questions that will need the operator
**eventually** but cannot be answered usefully **today**, because the evidence or
the prerequisite does not exist.

- **Every entry carries a `Trigger:` line** — the concrete event that makes it
  answerable. Without one it is either a real open question or dead; it is never
  deferred. This is the whole mechanism: a deferred question with no trigger is
  just a question that has been hidden.
- Entries are numbered `D-Q<n>` and are append-only, same as the other shared
  docs (`roadmap.md` rule 3, now updated).
- When the trigger fires, the entry moves *back* into `open-questions.md`,
  refreshed with whatever the evidence turned out to be, and is deleted here.

**Moved on creation:** Q39 (which way the shipping default points once a fastpy
utility clears both the parity bar and the performance bar) became **D-Q1**. Its
trigger is the first fastpy utility clearing both bars; nothing does today, so
answering it now would be answering without evidence.

**The risk accepted.** A deferred question is easier to forget than an open one —
that is the point, and it is also the hazard. `deferred-questions.md` has no
process that surfaces it on a schedule; it relies on whoever fires the trigger
noticing the entry. The mitigation is that triggers are written as events in the
work (*"the first fastpy utility clears both bars"*), so the person who causes
the event is the person reading the file's subject matter at that moment. If
that proves optimistic, the escalation is a check in the task-completion
checklist, not a bigger file.

## §314 — A conservative capability projection may gate an *attempt*, never a *refusal*

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — a corollary of §312, which the operator
decided; this resolves how §312's projection may be *consumed*, which §312
itself did not say.

**In short.** §312 made libc's capability words deliberately pessimistic: if we
cannot prove the kernel would allow something, we report "not held". That is the
right way to *answer a question*, but it is the wrong way to *make a decision*.
A dozen places in our libc refuse an operation outright when the capability
reads false — which means they will start refusing things the kernel is
perfectly willing to do, purely because our projection is cautious. The rule
adopted here: libc may use the projection to decide whether it is worth trying
something, but never as its reason for saying no. Where the kernel is the one
who actually decides, libc forwards the call and reports the kernel's answer.

**The decision.**

1. **A capability that reads false is not evidence of a denial.** §312's
   projection under-approximates the kernel's authority by construction —
   deny-by-default, unmapped `CAP_*` false, `CAP_SYS_ADMIN` a hand-written
   union that admits five uncovered sites. An under-approximation used as a
   denial test produces false denials at exactly the rate it is conservative,
   which is to say: by design.
2. **Where a kernel call stands behind the operation, libc does not pre-empt
   it.** `kill` reaches `SYS_SIGNAL_SEND`, `chown` reaches `SYS_FS_SET_OWNER`;
   both kernels evaluate the real predicate and return a real error. libc's job
   there is to forward and translate, not to guess first and guess narrowly.
3. **Where libc is the sole decider, the gate must express Linux's whole
   predicate** — the capability *and* its alternative — or the operation is
   reported as unimplemented. A stub that returns `EPERM` because a projected
   capability is missing has invented an authority failure for something it was
   never going to do; `ENOSYS` is the honest answer and the one that does not
   mislead a port into dropping a feature it could have had.

**Why not the alternative** — keep the pre-emptive gates and teach each one
Linux's full rule. It is the obvious move and it fails on the facts: for the
cross-process cases libc *cannot evaluate the rule*. Linux's `kill` permission
test needs the **target's** credentials, and we have no syscall that exposes
them; `ptrace_may_access` additionally needs the target's dumpable flag. A gate
that cannot evaluate its own predicate is not a gate, it is a guess with an
`EPERM` attached. Where the alternative *is* evaluable — `mlock`'s
`CAP_IPC_LOCK` **or** `RLIMIT_MEMLOCK`, `setuid`'s `target == cur` — teaching
the gate the full rule is exactly right. This decision is about the ones where
it is not evaluable, and it deliberately does **not** license removing a gate
whose alternative we could have checked. Applying it surfaced three such sites
(`nice`, `setpriority`, `sched_setscheduler`'s RT arm, all keyed on
`RLIMIT_NICE`/`RLIMIT_RTPRIO`) that had been *mis*-filed as unevaluable on the
strength of their own comments; they get the full-rule treatment, not this one.
Deciding which arm a site falls in therefore means checking what libc can
actually see, not what the comment above the gate claims.

**What is given up, honestly.** libc loses a defence-in-depth layer: a caller
that would have been stopped early now issues a syscall and is stopped there.
That costs a syscall on a path that was going to fail anyway, and it means a
buggy caller discovers its mistake one layer deeper. Both are acceptable
because the layer being removed was never load-bearing — the kernel re-checks
every privileged operation regardless, which is the same property that makes
§312's optimistic-answer period safe. What is *not* acceptable is the
alternative's cost: a wrong denial is indistinguishable, at the call site, from
a real one.

**Where it bites.** The full site-by-site survey is `known-issues.md` →
`TD-POSIX-CAP-GATES-OMIT-LINUX-S-NON-CAPABILITY-ALTERNATIVE`. This decision
governs its Class A (7 sites, actionable) and the ptrace-family half of Class B
(4 stub sites — `ptrace`, `process_vm_readv`/`writev`, `kcmp` — where the
alternative genuinely is unevaluable and rule 3 applies). All eleven are done.
The remaining three Class-B sites are the `RLIMIT` ones noted above; they are
outside this decision and were fixed by writing the whole predicate (a
`can_nice()` mirroring Linux's `is_nice_reduction || capable`, an RT gate
consulting `RLIMIT_RTPRIO` with `SCHED_DEADLINE` left capability-only, and
`RLIMIT_NICE`/`RLIMIT_RTPRIO` corrected to Linux's `{0, 0}` cold-start values
so the change preserves current behaviour exactly). It is a **prerequisite for
§312 step 3** — flipping the gates truthful before applying it would turn every
one of these into a live regression on the same day. With all fourteen sites
resolved, that prerequisite is met.

## §400 — Every GUI process finds its own UI font, lazily, from a compiled-in fallback list

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

### The problem

`osfont` can now parse a real face, scale it, and rasterize its outlines, but
nothing had ever handed it one: both the toolkit's measurement cache and the
compositor's drawing cache were constructed with `FontCache::new()` and left
holding the built-in 8x16 bitmap face. The outline stack was complete and
unreachable.

Getting a face into those caches raises a question that is not obviously
answerable, because the two caches live in *different processes*. An
application measures its own text — that is how it decides a button is wide
enough for its label — and the compositor draws it. If the two disagree about
which file "the UI font" is, every centred label in the system is off by half
the difference between two fonts' metrics, every right-aligned one by the
whole difference, and neither process looks wrong on its own. It is a bug that
presents as "the UI is subtly crooked" with no component to blame.

### The options

**(a) An explicit startup call.** Each process calls something like
`text::init_fonts()` before it draws. Cheapest, and it makes the cost of the
directory scan visible at the point it is paid.

**(b) Configuration: read the family from a settings file.** The desktop
already has an `ui_font` field in `appearance_settings.rs`, so the machinery
half-exists.

**(c) Lazy automatic installation from a list compiled into the toolkit.**
The first call that touches text builds the index and installs the first
family on `DEFAULT_UI_FAMILIES` that resolves.

### The decision

(c), with (b) deferred until settings actually persist.

(a) was rejected on the specific grounds that it is wrong the *first time an
app forgets*, and the failure is silent: an app that skips the call still
draws — in the bitmap face, against a compositor drawing in the real one. An
API whose misuse is invisible and whose correct use has to be repeated in
every process is the wrong shape for something every process needs. The
scan is not expensive enough to be worth that risk: 558 faces in 0.36 s on
this host in a debug build, once per process, and only for processes that
draw text at all.

(b) is where this should end up, and cannot go there yet:
`AppearanceSettings::save()` copies the pending settings into a `saved` field
and writes nothing to disk. There is therefore no cross-process source of
truth to read — a settings-driven font would be read by the app that changed
it and by nothing else, which is exactly the divergence being avoided.
`text::set_font_family` exists and works; it just has no persistent caller
yet.

The fallback list is ordered `Inter` (the design's intended UI font), then the
default sans of each platform this runs on, ending with `DejaVu Sans` /
`Liberation Sans` / `Arial`, which are installed almost everywhere. The
alternative to finding *a* face is the bitmap font, which is legible but looks
nothing like the system it stands in for, so it is worth walking a list of
eight to avoid.

### What makes the two processes agree

Not the list — a shared *function*. `guitk::text::install_ui_faces(&mut
FontCache)` is public precisely so the compositor can call it instead of
resolving a family by its own rule. Two copies of a fallback list agree until
the first time one of them is edited; one function cannot disagree with
itself. The compositor keeps a separate `FontCache` (it only draws, and would
otherwise take the toolkit's global lock once per glyph run and never measure
anything), but the *contents* of that cache come from the toolkit's own
resolution path.

`FontDb::finish()` sorting its faces by path is the other half of this:
directory iteration order is unspecified, so two processes scanning the same
directories could otherwise index the same family in different orders and pick
different files for it.

### Consequences

- Installation is atomic across regular and bold. A family that loaded one
  weight and not the other would draw a label's bold run in one face and the
  rest in another, which reads as a rendering fault rather than as a missing
  font, so `install_family` loads both before touching the cache.
- Toolkit tests that asserted pixel geometry against the bitmap face's 16 px
  line height had to start pinning their own cell size — a test about
  scrolling arithmetic must not depend on which fonts the host has installed.
- The desktop's `ui_font` setting is still inert. Wiring it up is blocked on
  settings persistence, not on this.

**Where it lives.** `gui/toolkit/src/text.rs` (the cache, the fallback list,
`install_ui_faces`/`install_family`/`set_font_family`),
`gui/toolkit/src/fontdb.rs` (the index and the family→file resolution),
`gui/font/src/select.rs` (the CSS Fonts 4 matching rule),
`gui/compositor/src/main.rs` (`RenderEngine::new`).

## §401 — Kerning reads GPOS in preference to the legacy `kern` table, and reads both

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

### The problem

The layout path advanced the pen by one glyph's own advance width and nothing
else. That is the width the designer drew around the letter *in isolation*;
for a pair like `AV`, `To`, `Yo`, `P.` or `r.` they also specify a correction,
and without it the diagonal of the `V` sits a visible gap away from the `A`.
Text set this way does not look broken so much as amateur, and the error is
systematic — it accumulates along a run, so a long right-aligned string drifts.

The corrections live in one of two places, and which one is not a matter of the
font's age: `GPOS`'s `kern` feature (OpenType, class-based, the table a modern
designer maintains) or the legacy `kern` table (a flat pair list). Supporting
one is much less work than supporting both.

### The options

**(a) Legacy `kern` only.** A few hundred lines: a flat, sorted pair array with
a binary search over it.

**(b) `GPOS` only.** The table modern tooling emits, and the one that is
authoritative where a face has both.

**(c) Both, preferring `GPOS`.**

### The decision: (c), and the reason is measured, not assumed

A sweep of the 556 faces installed on this machine, reading only each file's
table directory, gives:

| carries | faces |
|---|---|
| neither | 126 |
| `GPOS` only | 188 |
| `kern` only | 94 |
| both | 133 |

So (a) leaves 188 faces unkerned and (b) leaves 94 — in both cases a large,
arbitrary slice of the installed fonts silently renders worse than the rest,
with no way for a user to tell why one font looks right and another does not.
Neither table is a legacy concern that can be skipped. `GPOS` wins when a face
has both, because it is the table the designer's tooling generates and the
legacy copy in such a face is a compatibility shim that may be a lossy
flattening of it.

The host sweep is kept as an `#[ignore]`d integration test
(`installed_fonts_kern_the_pairs_that_need_it`), not because the counts must
hold on another machine but because kerning is the one part of the stack whose
correctness cannot be seen in a glyph: a wrong pair value still parses, still
rasterizes, and still has ink. The test therefore also pins five known faces
(Arial, Times, Segoe UI, DejaVu Sans, Verdana) to the assertion that `AV` is
*narrower* than `A` plus `V`, which is an oracle independent of our own parser.

### What is deliberately left out

- **Script and language selection.** Every lookup reachable from any feature
  tagged `kern` is used, whatever script system it hangs under. Correct
  selection needs the itemised script of the run, which the layout path does
  not yet compute; using all of them is wrong only for a face that kerns a pair
  differently per script, which is rare, and the failure is a slightly wrong
  gap rather than a wrong glyph.
- **Contextual and cross-stream kerning**, and the legacy table's format 2.
  Vanishingly rare; the subtables are skipped rather than misread.
- **Device tables** on value records. They tune a value for one specific ppem
  and are a sub-pixel concern.

Each of these is noted in `gui/font/src/kern.rs`'s module documentation so the
next reader does not have to rediscover that the omission was a decision.

### Consequences

- `SystemFont::kern` and `ScaledFont::kern` are *public*, and kerning is
  applied inside `measure`/`draw_text` as well. The public form exists because
  the compositor draws one glyph at a time through its own clip stack and
  cannot call `draw_text`; if it could not ask about a pair it would space runs
  differently from the way the toolkit measured them, which is exactly the
  cross-process divergence §400 exists to prevent.
- `Face::kern` is infallible and returns 0 for a malformed table. A bad kerning
  table means text spaced slightly wrong, which is not worth failing a draw
  over when the alternative is a blank window.
- Parsing is eager at face load (it resolves to a short list of subtable
  offsets) so that "GPOS or the legacy table?" is not re-decided on every pair
  of glyphs drawn.

**Where it lives.** `gui/font/src/kern.rs` (all table parsing),
`gui/font/src/sfnt.rs` (`Face::kern`, `Face::has_kerning`),
`gui/font/src/scaled.rs` and `gui/font/src/system.rs` (application during
measurement and drawing), `gui/compositor/src/main.rs`
(`RenderEngine::draw_text`).

---

## §402 — Text is shaped once into a run; ligatures are `liga` + `rlig` only, in a single pass

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Decision.** Every consumer of a string's layout — measuring, drawing,
hit-testing, truncation — goes through one `ScaledFont::shape` /
`SystemFont::shape` call that returns a `ShapedRun`. On top of that run,
GSUB ligature substitution is enabled for exactly two features, `liga`
(standard ligatures) and `rlig` (required ligatures), read from LookupType 4
in a single left-to-right pass with longest-match-first within a ligature
set.

**Why one run.** The recurring bug class in this code is two loops over the
same string arriving at different answers. It has now happened twice for
real: kerning was added to `measure` but not to `fit`/`char_index_at`, so a
prefix chosen to fit *N* px measured wider than *N* px and a click landed on
one character while the caret drew at another; and `measure` expanded `\t`
to four cells while `draw_text` drew it as the missing-glyph box. Neither
was a hard fault — both are text that is quietly wrong, which is the kind of
defect that survives every test that looks at a single function. A single
`ShapedRun` walked by all four callers makes the divergence unrepresentable
rather than merely discouraged, and it is what made ligatures expressible at
all: the old `SystemFont::glyph(ch)` API was 1:1 char→glyph, so no ligature
could have been returned through it. That API was deleted rather than
documented.

*Cost:* shaping allocates a `Vec` per call where the old loop allocated
nothing, and a widget that measures then draws shapes twice. Accepted:
`shape` touches `cmap`/`hmtx`/GPOS/GSUB only, never the rasterizer or the
glyph cache, and the alternative — a cache keyed by (string, size, weight) —
buys a memcpy-scale saving at the price of an invalidation problem. Revisit
if profiling of a real desktop frame says otherwise.

**Why kerning is charged to the preceding glyph.** A `ShapedGlyph`'s
`advance` includes the kern against the glyph that follows it, so
`sum(advance) == run.width()` and the draw loop is exactly `pen +=
advance` — no separate "look at the next glyph" step that a caller could
forget. The part of the advance that is a kern is kept in `kern_next`
because it has to be *recoverable*: `fit` cuts a prefix that the caller then
draws **alone**, so the kern pulling the last surviving glyph toward a
dropped one must come off. Without that, `fit("AVATAR", 10.0)` keeps the `A`
on the strength of a −0.4 px correction for a `V` that is no longer there,
and the result measures 10.32 px — wider than the budget it was asked to
fit. `fit_end` needs no such correction, which is the same fact seen from
the other side.

**Why only `liga` and `rlig`.**

- `liga` is what a face means by "this is how I am meant to be set": `fi`,
  `fl`, `ffi`. Leaving it off means the `f`'s hood collides with the `i`'s
  dot in every serif face on the desktop — the font ships a fix for its own
  defect and we were declining it.
- `rlig` is not optional in the sense that matters: Arabic lam-alef is a
  *required* ligature, and text that omits it is not a stylistic variant,
  it is wrong.
- `dlig`, `hlig` and `swsh` are discretionary by definition — the spec says
  they are off unless the *document* asks. There is no per-run feature list
  in this stack to ask with, so enabling them would apply a typographic
  choice to every button label on the system.
- `clig` (contextual ligatures) is excluded for a different reason: it needs
  LookupType 6 (chained context) to be honest, and a half-implementation
  that fires context-dependent lookups without their context is worse than
  none.

**Why a single pass.** The spec's model is iterative — a lookup's output can
feed a later lookup. In practice `LigatureSet`s are ordered longest-first by
convention, so trying every record at a position and taking the first match
gets `ffi` right without a second pass. The alternative (loop until
fixpoint) costs a bounded-iteration guard and a re-walk of the run for a
gain no Latin face demonstrates. Documented here so the next session knows
it is a decision, not an oversight: a script that genuinely needs multi-pass
substitution (Indic reordering) needs far more than this anyway.

**What is deliberately left out.** Script and language selection (the
`ScriptList` is not consulted — features are taken from the `FeatureList`
by tag, which is the default-script behaviour every Latin face wants and is
wrong for a face whose `liga` differs by script); GPOS mark attachment;
contextual and chained-context substitution; bidi. Each is a separate
roadmap step, not a gap this one should have closed.

**Measured.** Of 556 faces installed on the development host, 169 carry
ligature lookups this reads and 114 substitute `fi`. Notably `times.ttf` and
`segoeui.ttf` do **not** — the shipped Microsoft core versions carry no
`liga` ligature lookup for it — which is why the host-font test treats its
oracle list as "at least one of these" rather than "all of these".

**Where it lives.** `gui/font/src/shape.rs` (`ShapedRun`, `ShapedGlyph`,
`GlyphKey`, `fit`/`fit_end`/`offset_at`/`x_of`), `gui/font/src/gsub.rs`
(ligature parsing), `gui/font/src/otl.rs` (the walk shared with GPOS),
`gui/font/src/sfnt.rs` (`Face::ligature`, `Face::has_ligatures`),
`gui/font/src/scaled.rs` and `gui/font/src/system.rs` (`shape`, `measure`,
`draw_text`, `glyph_mask`), `gui/toolkit/src/text.rs` (`fit`, `fit_end`,
`char_index_at`), `gui/compositor/src/main.rs` (`RenderEngine::draw_text`).

---

## §403 — Combining marks: attach from GPOS anchors, zero their advance, and decide mark-ness from GDEF ∪ coverage

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** Kerning (§401) and ligatures (§402) are refinements: get them
wrong and text is a fraction of a pixel off, or an `fi` has a visible
collision. Mark attachment is not in that class. A combining mark is drawn
at the pen, and the pen for the mark is the *left edge* of the following
cell — so with no positioning at all, `e` + U+0301 draws the acute in the
gap **before** the `e`, and a second mark draws on top of the first. Every
accented language on the machine renders visibly broken. `hmtx` cannot help:
it says a mark takes no room, never where it goes.

**Decision, in four parts.**

*1. Read MarkBasePos (GPOS type 4, feature `mark`) and MarkMarkPos (type 6,
`mkmk`); do not read MarkLigPos (type 5).* The first two share a
byte-for-byte identical header — coverage for the attachee, coverage for the
mark, a class count, two arrays — so one reader handles both, which is not
a coincidence being exploited but how the spec defines them. MarkLigPos is
left out because it attaches a mark to one *component* of a ligature, which
requires remembering which component each mark belonged to before
substitution collapsed them. A `ShapedRun` glyph knows its cluster, not its
components. A mark on a ligature therefore falls back to MarkBasePos, which
most faces also provide; where they do not it lands unpositioned, which is
wrong in the way the font asked for rather than wrong in a way nobody could
trace back.

*2. Mark-ness is `GDEF` class 3 **or** membership of a mark coverage — the
union, not `GDEF` alone.* This was measured, not chosen: DejaVu Sans Mono
classes `acutecomb` as `GlyphClassDef` 1 (base) while its own `mark` feature
carries an anchor for that very glyph, so a `GDEF`-exclusive reader leaves
every accent in that family unattached. Coverage alone is equally wrong: a
mark the face has no anchor for would read as a base, and the *next* mark
would then stack onto it. The fully correct answer is Unicode general
category `Mn`/`Mc`/`Me`, which is a property of the character and true
whatever the font claims; that needs a category table this crate does not
have. The union covers every face that has anchors to attach with, which is
every face where the answer changes anything.

*3. A mark's advance is zero, whatever `hmtx` says.* Also measured: Segoe UI
gives U+0301 an advance more than half an `e` wide, because the same outline
doubles as the spacing acute U+00B4. Honouring it made `e` + U+0301 measure
25.3 px against the bare `e`'s 16.7 — a gap after every accented letter, and
a caret that lands in it. HarfBuzz zeroes mark advances after positioning
for exactly this reason. Because mark-ness now decides a *width*, not just a
placement, `MarkPositioning` is kept for a face that has `GDEF` classes but
no anchors — it still knows which glyphs are marks, and that alone is worth
having. Hence `Face::has_marks()` rather than `has_mark_positioning()`.

*4. Marks are never kerned.* Real faces set `IgnoreMarks` on their kerning
lookups so that `A` and `V` still kern with an accent between them. This
engine walks a run strictly in order and cannot skip, so it declines
instead: a pair separated by a mark goes unkerned, losing a fraction of a
pixel, where kerning *against* the mark would shove the accent off the
letter it belongs to. (Ligatures likewise may not span a tab — the tab's
width is a layout decision, not a glyph width, and joining across it would
swallow the gap it exists to make.)

**Cost.** `ScaledFont::shape` is now four passes rather than three, each
justified by a data dependency: substitution cannot be decided while
characters are still arriving; kerning applies to the glyphs that *survive*
substitution; and a mark's displacement is measured from its base glyph's
origin, whose distance from the pen is the sum of the advances in between —
which pass three is still adjusting. The fourth pass allocates one `f32` per
glyph to hold the running pen and runs only on faces that know about marks.

**Alternatives rejected.** Decomposing precomposed characters and
recomposing via `ccmp` (needs Unicode normalization data we do not have, and
buys nothing for text that already arrives precomposed); synthesising an
accent placement from the base glyph's bounding box when the font offers no
anchor (invents a number the font never authorised, and it would be wrong in
a way no one could attribute).

**Measured.** Of 556 faces on the development host, 349 can tell a mark from
a letter and 175 place a combining acute on an `e`. The four oracle faces
(Segoe UI, DejaVu Sans, Calibri, Times New Roman) all put the accent's ink
inside the `e`'s advance and above the baseline. Note that the *sign* of the
displacement is not an oracle here, unlike kerning's: Cascadia Code draws
its combining acute already at accent height inside its cell, so its anchors
coincide and the displacement is legitimately `(0, 0)`.

**Where it lives.** `gui/font/src/mark.rs` (`MarkPositioning`, anchor and
`MarkArray`/`BaseArray` reading), `gui/font/src/otl.rs` (the walk shared
with GSUB and kerning), `gui/font/src/sfnt.rs` (`Face::is_mark`,
`mark_on_base`, `mark_on_mark`, `has_marks`), `gui/font/src/shape.rs`
(`ShapedGlyph::offset`), `gui/font/src/scaled.rs` (`shape`'s fourth pass,
`attach_marks`, `glyph_mask`, `draw_text`), `gui/font/src/system.rs`,
`gui/compositor/src/main.rs` (`RenderEngine::draw_text`).

## §404 — Configuration is edited as text, not serialized: a format-preserving YAML document with a line index

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** `design.txt` mandates YAML for configuration, "processed with a
library that preserves comments and formatting", and nothing in the tree could
do the second half — the installer's parser is read-only and lossy, and there
is no YAML crate anywhere in the lock file. Meanwhile
`gui/desktop/src/appearance_settings.rs` had a `save()` that copied one struct
field into another and wrote nothing
(`known-issues.md`, TD-APPEARANCE-SETTINGS-ARE-NEVER-WRITTEN-TO-DISK), and
about twenty other `*_settings.rs` panels have the same shape. Persisting even
one of them required deciding what "preserves comments and formatting" means in
code.

**Decision.** `yamldoc` is not a serializer. A `Document` holds the file's
original lines; a write splices the new value into the one line that holds it,
and every region the edit did not name comes out byte for byte as it went in.
The line classification (`scan()`) is recomputed on demand rather than cached.

Four consequences follow, each chosen deliberately:

1. **Anything not modelled is opaque, not an error.** Anchors, tags, flow
   collections, block scalars and multi-document files are carried through
   untouched but are not indexed, so they cannot be read or written through the
   API. A block scalar's body is explicitly excluded from key indexing, so a
   user's prose containing a colon is never mined for a key.
2. **Parsing is infallible; every getter returns `Option`.** There is nothing
   useful a config reader could do with a "malformed YAML" error: its answer to
   a key it cannot read is the default, which is what `None` says.
3. **Paths are `&[&str]`, never dotted strings.** A dotted path does the wrong
   thing the first time a key holds a font family (`Noto Sans CJK.Bold`) or a
   hostname.
4. **Reading is liberal, writing is conservative.** `get_bool` refuses the
   YAML 1.1 spellings — `NO` is a country, not `false` — but `set_str` quotes
   them anyway, along with hex/octal/underscored numbers. The file is not only
   read back by us: the user's `yq`, their editor's highlighter and the next
   tool to touch it all get a look. The asymmetry is the point.

**Alternatives rejected.**

*Deserialize into a struct, mutate, re-serialize.* The obvious approach, and
the reason `design.txt` calls the requirement out. It is lossy by construction:
it discards every comment, blank line, deliberate ordering and choice of
quoting, and hands the user a machine-flattened version of what they wrote. Do
that once and nobody hand-edits a config again, because their annotations do not
survive the next time they touch a setting in the UI.

*Take a dependency (`serde_yaml`, `yaml-rust`).* Only twelve external crates
exist in the whole lock file, none of them YAML. `serde_yaml` is lossy in
exactly the way above and is unmaintained; a full round-trip implementation
(the `ruamel.yaml` model) is far more machinery than a settings file needs. And
configuration is read where there is no `std` — the service manager and the
package manager both run early — so the crate is `no_std` + `alloc` and
dependency-free, mirroring `tzrules` and `netproto`. A config reader that only
works once `std` is up cannot be used to decide how `std` comes up.

*Cache the line index inside the `Document`.* Rejected on the recurring lesson
of this lane: two loops over the same text coming to different answers is the
bug class that keeps costing the most (§401, §402). The lines are the single
source of truth, and a cached index that can disagree with them makes that
divergence representable. Config files are a few dozen lines and edits happen
when a user clicks Save, so the scan is not on any hot path.

**Cost.** `set_*` is O(lines) per call rather than O(1). A settings panel
writing twenty keys rescans twenty times. Measured against the alternative —
an index that can go stale — this is the right trade at this size, and if a
consumer ever writes thousands of keys the fix is a batch API, not a cache.

**Second decision, same commit: where configuration lives.**
`gui/desktop/src/config.rs` resolves `$XDG_CONFIG_HOME/slateos/<name>.yaml`,
falling back to `$HOME/.config/slateos/`. XDG because the rest of the tree
already assumes an XDG-shaped home (`~/.cache`, `~/.local/share/Trash`) and a
user who set the variable has said plainly where they want their config. With
neither variable set, `store` reports `NotFound` rather than inventing a
location: writing one user's personal preferences into a system-wide path
because `$HOME` was missing is worse than not writing them. Writes go to a
`.new` file beside the target and are renamed over it — rename within a
directory is atomic, so a crash mid-save leaves the whole old file or the whole
new one, never a truncated middle. The temporary is named after the target
rather than randomly, so a crash between write and rename leaves one
identifiable piece of litter the next save overwrites, not an unbounded pile.

**Third: config spellings are not UI labels.** Each settings enum gets a
`yaml_name`/`from_yaml_name` pair (generated by the `yaml_enum!` macro) that is
deliberately separate from `label()`. A label is screen text — "Extra Large
(96px)", "Accent Color" — and changes when the wording is improved or a preset
is retuned. A config spelling is part of the file format: change it and every
existing user's saved choice silently reverts to the default on next start. An
unknown spelling reads as `None` and leaves the field at its default, so a file
written by a newer desktop degrades rather than refusing to load.

**Where it lives.** `yamldoc/src/lib.rs` (the whole crate); the "Configuration
file" section of `gui/desktop/src/appearance_settings.rs`
(`read_from`/`write_into`, `yaml_enum!`, `color_to_hex`/`color_from_hex`);
`gui/desktop/src/config.rs` (`config_dir`, `path_for`, `load`, `store`).

## §405 — The accent is a role with two values, and it is chosen for contrast rather than fidelity to Catppuccin

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** Wiring the saved appearance settings into the shell meant the
desktop finally had to paint a *light* theme — it had only ever had the
hard-coded Catppuccin Mocha one. An accent colour then stops being a single
constant: `AccentColor::Blue` has to mean one thing on a near-black base and
another on a near-white one, because the shell draws the accent as **text**
(the start glyph on the taskbar, the start-menu heading), not only as a fill.

**Decision.** `AccentColor` carries two values per name — `color()` for dark
backgrounds and `color_light()` for light ones — and `effective_accent()`
resolves between them from `theme_mode`. The light values are Catppuccin
Latte's hues **darkened until they clear 4.6:1 on the Latte base**, not Latte's
published values.

**Why the deviation.** Measured against Latte base `#EFF1F5`, the upstream
Latte accents are decorative, not text-safe: yellow 2.31:1, pink 2.34:1,
rosewater 2.34:1, sky 2.47:1, flamingo 2.64:1, peach 2.64:1, sapphire 2.78:1,
lavender 2.81:1, green 2.96:1, teal 3.31:1, maroon 3.48:1 — only blue (4.34, so
also nudged), mauve (4.79) and red (4.80) approach or clear the 4.5:1 that body
text needs. Shipping them unmodified would have meant a light mode whose start
menu heading is illegible for eleven of the fourteen accents. Each value is
therefore its Latte hue with all three channels scaled toward black by the
smallest factor reaching 4.6:1, which holds the hue — these still read as the
colours Catppuccin named. Blue, mauve and red are barely touched. The dark
palette needed no treatment at all: every Mocha accent is 7:1–13:1 on Mocha.

**Alternatives rejected.**

- *Ship Latte as published and accept the contrast.* The honest form of "match
  the upstream palette", and rejected because a heading nobody can read is not
  a style choice. Catppuccin publishes accents for syntax highlighting and
  decoration, where a 2.3:1 accent sits next to a label that carries the
  meaning; here the accent *is* the label.
- *Keep one accent value and never draw the accent as text.* Structurally
  cleaner — one constant per name — but it forbids the two places the shell
  already uses the accent to mean "this is the thing you pressed", and would
  push every accent onto a fill with a contrasting foreground, which is a much
  heavier visual language for a start-menu heading.
- *Derive the light value at runtime by darkening.* Rejected: the same loop
  that produced these constants would then run on every theme change, and a
  colour a user sees would be the output of a search rather than a value anyone
  can look up, diff, or override. The search was run once, offline; its results
  are the table.

**Consequences.** `every_accent_is_readable_as_text_in_both_modes` asserts the
4.5:1 bar for all fourteen accents in both modes, so the palette cannot regress
to the upstream values without a test failing. A **custom** accent is exempt —
`effective_accent()` returns `custom_accent` verbatim in either mode, because
the user named a specific colour rather than a role, and silently darkening it
would override the one choice that was stated in full. That exemption is the
known hole in the contrast guarantee.

**Companion decision — `ThemeMode::System` resolves to dark.** `System` means
"follow the system's light/dark schedule", and nothing in this tree computes
sunrise or watches a time-of-day trigger yet. It answers dark, because dark is
what the shell has always painted and what every other default is tuned
against; answering light would flip the whole desktop for a user who asked only
to be left on automatic. `ThemeMode::is_light()` is the single place that has
to change when a schedule exists.

**Companion decision — the resolved palette is a separate type from the
settings.** Render functions read `DesktopTheme` fields and never consult
`AppearanceSettings`. A renderer that derived colours itself would re-derive
them every frame and, worse, would be free to derive them slightly differently
in each of the dozen places a colour is drawn. `DesktopShell::set_appearance()`
is the single door: it re-derives the theme and stores the settings together,
so `theme` can never be a stale derivation of an older `appearance`. Each
surface also carries its own foreground, because a shared one is only correct
while every surface has a similar brightness — which "accent on the taskbar"
ends the moment it is switched on.

**Where it lives.** The light-accent palette and `AccentColor::color_light`,
`ThemeMode::is_light`, `AppearanceSettings::effective_accent` in
`gui/desktop/src/appearance_settings.rs`; `DesktopTheme::{dark, light,
from_settings}`, `readable_on`, `emphasized`, `taskbar_alpha` and
`DesktopShell::{set_appearance, load_appearance}` in `gui/desktop/src/main.rs`.

## §406 — GSUB is an ordered list of lookups, not a bag of subtables; single substitution and `ccmp` before contextual (GSUB 5/6)

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** §402 read GSUB for one purpose — find LookupType 4 anywhere in
`liga`/`rlig` and join glyphs — so it flattened every matching lookup into one
list of subtable offsets and walked that. That is adequate for ligatures alone
and wrong for anything else, which the next step needed.

**Decision, part one: a lookup is the unit of application.** `otl::Lookup`
keeps a lookup's type together with its own subtable offsets, and
`Substitutions::apply` runs each lookup across the *whole* glyph buffer before
starting the next. Within one lookup, subtables are tried in font order and the
first match wins; a glyph one subtable substituted is not re-offered to the
rest of that lookup.

This is not a refactor for tidiness — it is the only mechanism that makes
`ccmp` reliably precede `liga`. A font lists its lookups in the order it wants
them applied, so what lookup 1 substitutes is what lookup 2 sees. Flattening
subtables into one list destroys that boundary: `ccmp` mapping A→B and a
ligature covering B but not A would find nothing to join in a single flat pass.
Two tests pin the semantics in both directions —
`an_earlier_lookup_feeds_a_later_one` and
`a_later_lookup_does_not_feed_an_earlier_one`.

*Looping to a fixpoint was rejected*, and the second of those tests is why: a
pass that repeated until nothing changed would ligate in the second case too,
which is the font asking for the opposite. It also would not terminate on a
font whose lookups feed each other cyclically. One ordered pass is both correct
and total.

Callers with genuinely one lookup type and no ordering to preserve — pair
kerning, mark attachment — keep the flat list through `feature_subtables`,
which is now a thin wrapper over `feature_lookups`. They were not touched.

**Decision, part two: type 1 and `ccmp` now, types 5/6 later.** The roadmap
bullet named contextual substitution (GSUB 5/6) as the next unblocked step.
That is out of order: 5 and 6 work by invoking *other* lookups by index at a
matched position, so a general "apply lookup N here" mechanism plus the simple
types it dispatches to are a strict prerequisite, not a parallel feature. This
commit therefore builds the ordered lookup list and LookupType 1 (Single
Substitution, both formats) and adds `ccmp` to the default-on feature set;
5/6 become implementable rather than blocked.

**What is deliberately still out, each for its own reason.**

- *Type 2 (Multiple).* One glyph becomes several, so the run grows and two
  glyphs share a cluster — which `shape.rs` currently assumes cannot happen.
  `ShapedRun::x_of` and `fit_end` both give wrong answers under it (traced:
  with clusters `[0,0,1]`, `x_of(0)` returns the first glyph's advance instead
  of zero). That is a change to `shape.rs`'s invariants with its own test
  story, not a change to `gsub.rs`, and is the next commit rather than this
  one.
- *Type 3 (Alternate).* Picks by an alternate index that only a per-run feature
  list can supply, and there is none. It has no default-on caller, so
  implementing it would add an unreachable branch.
- *`locl`.* On by default in every shaper, but *language*-specific. Applying it
  without knowing the run's language hands every reader some other locale's
  letterforms, which is worse than not applying it.

**Consequence, and the known hole.** Widening the read from `liga`-only to
"any applicable substitution" makes the existing no-script-selection limitation
bite for the first time: the feature walk takes every script's features, and
`ccmp` is precisely where a script puts its normalisation rules. On the
development host this is visible — `ebrima.ttf` substitutes the *space* glyph
from an African script's `ccmp`. Logged as
`TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES` and verified against an independent
Python parse of the table, so it is our *selection* that is wrong, not our
*parsing*. Two new host-font tests bound the damage:
`installed_fonts_leave_plain_latin_alone` asserts the *proportion* of faces
that alter Latin prose stays tiny (a parser fault hits fonts in bulk, a genuine
rule hits a handful), and `installed_fonts_leave_a_tab_alone` asserts the tab
survives.

**Decision, part three: a substitution may not reach across a tab.** A tab is
carried through shaping as the space glyph, but it is a layout decision wearing
a glyph's clothes. A GSUB lookup handed a run matches anywhere in it, so the
only way to express a boundary is to not show the lookup across one:
`ScaledFont::substitute_between_tabs` substitutes each stretch separately.
Without it a lookup covering the space replaces the tab (Ebrima again), or a
ligature swallows it and the tab flags stop lining up with the glyphs, after
which every advance past that point is charged to the wrong glyph. The same
mechanism is what style changes and bidi run edges will use: `Face::substitute`
takes a run, and the caller decides what one run is.

**Where it lives.** `otl::{Lookup, feature_lookups, read_lookup,
lookup_indices}` in `gui/font/src/otl.rs`; `gsub::{SubGlyph, Substitutions,
apply_single, single_at, apply_ligature}` in `gui/font/src/gsub.rs`;
`Face::{substitute, has_substitutions}` in `gui/font/src/sfnt.rs`;
`ScaledFont::{shape, substitute_between_tabs}` in `gui/font/src/scaled.rs`.

## §407 — A cluster is a boundary, not an index: `ShapedRun` works in whole clusters, and GSUB LookupType 2 (Multiple Substitution)

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** §406 left LookupType 2 out with a reason and a plan: one glyph
becomes several, so the run *grows* and two glyphs come to share a cluster —
which `shape.rs` assumed could not happen. This is that follow-up. It is two
changes, and the order matters: `shape.rs` first, because implementing type 2
against the old invariant would have produced a run whose own queries lied
about it.

**Decision, part one: the glyph↔character map is many-to-many in *both*
directions, and every `ShapedRun` query works in whole clusters.** A ligature
already gave several characters one cluster; type 2 gives several glyphs one
cluster. What a cluster is, therefore, is not an index — glyph *n* is not
character *n* in either direction — but a *boundary*: a caret, a cut and a hit
test may only land where a cluster starts, because every one of them is a byte
offset into the string and the offsets a string admits are its character
boundaries.

The three queries were each wrong in their own way under the new case, and the
fix is the same shape in each: walk *groups* of equal cluster, not glyphs.

- `x_of` tested the *next glyph's* cluster to decide whether it had passed the
  target, so with clusters `[0,0,1]` it counted the first half of a decomposed
  character and answered `x_of(0)` with that glyph's advance instead of zero.
  It now tests the next *group's* cluster.
- `fit_end` walked backwards a glyph at a time, so a budget could cut between
  the `e` and the acute of a decomposed `é` and return an offset naming a
  character that is not there in the cut. It now steps whole clusters.
- `offset_at` tipped at a glyph's midpoint, so a two-glyph cluster had a tipping
  point in its middle that no string offset corresponds to. It now tips at the
  *cluster's* midpoint.

Three private helpers (`group_end`, `group_start`, `span_width`) carry the
grouping so the rule lives in one place rather than three. `fit` needed no
change and says why in a comment: it already returns the *failing glyph's*
cluster, which is a cluster start by construction.

**Decision, part two: type 2 is implemented; the run may grow.** `ccmp`
decomposes precomposed characters so GPOS mark attachment can place the pieces
— which is the textbook reason the type exists, and is not hypothetical here:
Cambria on the development host decomposes `é` into `e` + acute, and the new
host test `shaped_runs_agree_with_their_strings_about_boundaries` finds exactly
that one grown run in 281 GSUB faces × 6 strings. Every glyph of a sequence
carries the cluster of the glyph it replaced, because they all came from the
same character. Walking resumes *after* the inserted glyphs, so a font that
decomposes A into A-and-something cannot loop.

**An empty `Sequence` is refused, and refused in the subtable rather than the
lookup.** `glyphCount == 0` would delete the glyph; the spec forbids it and
some shapers honour it anyway. Deleting takes the cluster with it, leaving a
character that no caret position corresponds to — worse than a character drawn
as it arrived. The placement is the subtle half: refusing inside `sequence_at`
lets `find_map` try a *later subtable of the same lookup*, whereas the earlier
draft's check in the caller short-circuited the whole lookup. The distinction
was invisible until mutation testing: removing the guard changed nothing,
because the caller's check masked it. Removing the *caller's* check made the
guard load-bearing and the mutation fatal to two named tests.

**`sequence_at` clears the output buffer on entry, not the caller.** Found
while resolving that redundancy, and a genuine bug rather than a tidiness
point: a subtable that reads half a sequence and then finds the table truncated
leaves those glyphs behind, and `find_map` hands the same buffer to the next
subtable — so subtable 1's junk is prepended to subtable 2's answer
(`[44, 30, 31]` where `[30, 31]` was right). The buffer exists to keep ordinary
text from allocating once per position; making it the callee's to clear keeps
that without letting a failed read be seen as part of a successful one.

**Bounds.** `MAX_SEQUENCE = 16`, deliberately set to match `MAX_COMPONENTS` —
they are inverses of each other, and a font that decomposes one glyph into more
than sixteen is malformed rather than ambitious. Exactly the cap is still
allowed: the bound is on absurdity, not on a font sitting at the limit.

**What is still out.** Type 3 (Alternate) and `locl`, both for the reasons in
§406, and types 5/6 (contextual/chaining), which the ordered lookup list now
makes implementable and which `clig`/`calt` need.

**Where it lives.** `gsub::{apply_multiple, sequence_at, MAX_SEQUENCE}` in
`gui/font/src/gsub.rs`; `ShapedRun::{group_end, group_start, span_width, x_of,
fit_end, offset_at}` and the `ShapedGlyph::cluster` doc in
`gui/font/src/shape.rs`; the host tests `installed_fonts_leave_a_tab_alone` and
`shaped_runs_agree_with_their_strings_about_boundaries` in
`gui/font/tests/host_fonts.rs`.

## §408 — A lookup type is a rule about one position; the pass belongs to a shared driver (GSUB 5/6)

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

### Context

GSUB lookup types 5 (contextual) and 6 (chained contextual) do not
substitute anything themselves. They match a pattern and then say "run
lookup 12 at input glyph 0, then lookup 7 at input glyph 2." The lookups
they name are named *by index into the font's LookupList* — not by
feature, not by anything a feature walk would have found. That is exactly
how a font hides a helper lookup that only makes sense inside one
context.

Until now every lookup type in `gsub.rs` owned its own loop over the run:
`apply_single` walked the buffer, `apply_ligature` walked the buffer, and
so on. Types 5/6 cannot be written that way, because what they need is to
apply some *other* lookup at one position.

### Decision

Invert it. Each lookup type became a function that answers one question —
"what, if anything, do you do at position `i`, and how far does that
carry the cursor?" — and a single shared driver (`apply_lookup`) turns
any such rule into a pass over the run. `apply_at` dispatches on the
type.

Consequences that fall out of the shape rather than being decided
separately:

- **Termination is structural.** The driver resumes past what a match
  produced, so a lookup is never offered its own output. A font whose
  ligature is also its own input, or whose Multiple Substitution
  decomposes a glyph to itself, cannot loop. Because this lives in the
  driver, no lookup type can forget it — which is the real argument for
  the refactor, over and above types 5/6 needing it.
- **Recursion is bounded explicitly**, by `MAX_NESTING = 6` carried in
  `Ctx`, since a font may have lookup 5 invoke lookup 6 invoke lookup 5.

### The hard part: nested position bookkeeping

A `SequenceLookupRecord` names a glyph of the input *as it was matched*.
But the lookup it invokes may grow the run (Multiple Substitution) or
shrink it (Ligature), and a later record in the same list still refers to
the original numbering. Tracking a single offset is not enough, because a
ligature does not just shift later glyphs left — it *swallows* some of
them, and a later record must not silently slide onto a glyph the context
never matched.

So positions are a `Vec<Option<usize>>`, one entry per matched input
glyph. Growth shifts later entries right. Shrinkage shifts them left
*and* sets the swallowed ones to `None`, and a record naming a `None`
position is skipped. `None` is "this glyph no longer exists," which is a
different fact from "this glyph moved," and conflating them is the bug
this representation exists to prevent.

### Alternatives considered

- **Whole-run passes only, with types 5/6 re-entering `apply_lookup`.**
  Rejected: a nested lookup must apply at *one* position, not everywhere.
  Re-running a lookup across the whole run would apply it to text the
  context never matched.
- **Re-matching the context after each nested lookup**, instead of
  tracking positions. Simpler to write, but quadratic, and it changes
  meaning: the spec matches the context once and then edits it.
- **Decoding the whole LookupList up front** so nested lookups are a
  table index. Rejected: most of it is never reached by a given run, and
  re-reading a lookup header on invocation is a handful of offsets.

### Format details worth recording

- Format 1 rules name glyph ids, format 2 rules name *classes*. It is one
  rule layout read two ways, which is why they share the `By` enum. This
  bit us in testing: a rule written `&[11]` (a glyph id) in a format-2
  context is a class number, and matched the wrong thing.
- Chained format 2 has **three separate ClassDefs**. A glyph may be one
  class as lookahead and another as input. Reusing one ClassDef for all
  three would be wrong on real fonts.
- Backtrack is stored **closest-glyph-first**: entry 0 is the glyph
  immediately before the input, not the leftmost of the context.
- A null (zero) offset means "absent" everywhere in OpenType. Following
  one lands back on the subtable's own header, where a format number
  would be read as a coverage format and match plausible-looking
  garbage. `sub_offset` refuses null for this reason.

### `clig` and `calt` are on by default

Both are `On by default` in the OpenType spec, and `calt` in particular
is what makes many faces look right rather than merely legible. Turning
them on changed shaping on 292 of the installed faces, so it is a real
behaviour change, not a formality.

It immediately broke `installed_fonts_ligate_fi`, which demanded that a
pair that does not ligate come back as *the same two glyphs*. Cambria
resolves the `f`+`i` collision contextually instead — a `calt` swaps in a
short-hooked `f` and leaves two glyphs. HarfBuzz produces the identical
`[976, 139]`, so our answer was right and the test's assumption was
stale. The test now checks length, clusters and glyph range, and counts
the faces taking the contextual route.

### Where this lives

`gui/font/src/gsub.rs` (`Ctx`, `apply_lookup`, `apply_at`,
`apply_nested`, `context_match`, `chain_match`, `By`, `Nested`);
`otl::{lookup_list, lookup_at}` in `gui/font/src/otl.rs`;
`installed_fonts_ligate_fi` in `gui/font/tests/host_fonts.rs`.

## §409 — Shape every installed face against HarfBuzz, and set limits from what that measures

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

### Context

Our GSUB implementation had 222 unit tests, mutation-tested guards, and
13 host-font tests over 556 real faces. All green. It was also silently
dropping most of the lookups in 61 of the 365 installed faces that have a
`GSUB`.

The unit tests could not catch it because they build their own fonts, and
a font written to test a feature has exactly the lookups that feature
needs. The host tests could not catch it because *returning the input
unchanged is a legal answer*: a face with no ligature for `fi` correctly
gives back two glyphs, so "gave back two glyphs" cannot distinguish a
face that has no ligature from a face whose ligature we failed to reach.
Every assertion we had was consistent with the bug.

### Decision

Use HarfBuzz as an independent oracle: shape a fixed corpus through every
installed face with both implementations and compare glyph ids. This
meant installing `uharfbuzz` on the development host.

Result over 556 faces and 13 strings: **6426 agree, 324 differ**, and the
324 fall into exactly three classes, of which one was ours:

- **288 — `e` + U+0301.** HarfBuzz runs its own Unicode normalizer
  (`hb-ot-shape-normalize`) before GSUB and composes the pair to a single
  precomposed glyph. That is not GSUB and not a fault in our tables; it
  is a genuine missing stage, now the next shaping item on the roadmap.
- **33 — Amiri and FiraCode** losing Latin ligatures and contextual
  alternates entirely. A real bug: see below.
- **3 — Calibri `1/2`.** A one-glyph disagreement in fraction handling,
  logged in `known-issues.md` rather than guessed at.

### The bug it found, and the rule it produced

`otl::MAX_SUBTABLES` was 64, shared across every lookup a face's features
reach, and its doc comment justified that as "real fonts use single
digits; the largest seen on the development host is 4." That number was
per *lookup*. Summed across the lookups our features reach, 61 installed
faces exceed 64 and the worst declares 1874.

Amiri lists its large Arabic feature set before its Latin `liga`, so the
budget was exhausted before the ligature lookup was reached and `office`
came back as six separate glyphs. FiraCode never reached the `calt` that
shortens `f` before `i`.

The cap itself is kept — the cost of a shape is the run length times this
number, so a hostile font must not be able to set it — but the value is
now measured and the measurement is recorded beside it:

| measure | worst face | count |
|---|---|---|
| subtables in one lookup | SansSerifCollection | 675 |
| lookups reached | SansSerifCollection | 256 |
| subtables in total | SansSerifCollection | 1874 |
| runner-up total | JetBrains Mono | 768 |

8192 is a little over four times the worst real face. Exceeding it still
shapes with the lookups found rather than rejecting the font: a slightly
wrong ligature is a better failure than a blank page.

**The general rule this establishes: a limit whose justification is a
glance is a bug waiting to happen, and the failure mode of a limit set
too low is silence.** Any budget, cap or depth in this crate should carry
the measurement that set it, so the next person to touch it knows what it
protects against and what it must clear.

### Alternatives considered

- **Derive the budget from the table's byte length** (a subtable offset
  costs two bytes, so the count is bounded by `len/2`). Attractive
  because it cannot be outgrown, but it makes the worst case scale with
  font size, which is precisely what the cap exists to stop.
- **Reject a face that exceeds the budget.** Rejected: a face that is
  merely large is not hostile, and a missing font is a worse
  user-visible failure than a missing ligature.
- **Commit the HarfBuzz comparison as a test.** Rejected for now: it
  needs Python and `uharfbuzz` on the host, which the Rust test suite
  cannot assume. What is committed instead is
  `installed_fonts_reach_lookups_past_the_subtable_budget`, which names
  specific faces whose answers are known to live deep. The choice of
  faces is the content of that test: JetBrains Mono declares more
  subtables than FiraCode and is deliberately *not* listed, because
  HarfBuzz shapes its `fi` unchanged too — a deep face only makes a
  witness when something deep applies to it.

### Where this lives

`MAX_SUBTABLES` and its measurement table in `gui/font/src/otl.rs`;
`installed_fonts_reach_lookups_past_the_subtable_budget` in
`gui/font/tests/host_fonts.rs`.

## §410 — Normalize to NFC before shaping, as a layer that knows nothing about the font

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

Shaping used to ask `cmap` for exactly the characters the caller typed. So
`"e"` followed by U+0301 COMBINING ACUTE missed the precomposed `é` glyph that
288 of the 556 installed faces carry, and drew a bare `e` with a mark floated
over it by GPOS where every other shaper draws one letter. The fix is a
normalization stage in front of `cmap`. The decisions worth recording are not
*whether* to normalize but how it is split and where it deliberately differs
from HarfBuzz.

### NFC, not NFD, and not NFKC

NFC because fonts are built for it: a face that has `é` at all has it as one
glyph, and its kerning and ligature rules name that glyph. NFD would decompose
into pieces the font's own tables never mention, and would then rely on mark
positioning to reassemble what the font already had. NFKC is simply the wrong
form for rendering — it replaces a superscript with a digit and `ﬁ` with `f`
`i`, which is a change to what the text *says*. The generator skips every
decomposition carrying a `<...>` tag for that reason.

### Two layers: what the text is, then what the face can show

`nfc()` is pure Unicode and never sees a font. `fit_to_face()` then takes a
character back apart when the face cannot draw it. The alternative — one pass
that composes only when the font has the composed glyph — is smaller, faster
and is what HarfBuzz does, and it was rejected.

The reason is that composability depends on context. Whether `a` and `b` join
depends on the marks between them, so a font-dependent compose gives
canonically equivalent spellings different answers. This is observable in
HarfBuzz on an ordinary Windows font: Agency FB has `ç` and `é` but no
combining acute, and HarfBuzz renders `c` + U+0327 + U+0301 as `ç` plus a
missing-glyph box while rendering the identical text spelled U+1E09 as a
single box. Canonical equivalence means those are the same string; a renderer
that draws them differently is wrong, and no amount of care in the caller can
work around it.

The cost is a second pass over the pieces. It is guarded: `fit_to_face`
returns immediately when the face has everything, which is the common case,
and `needs_work` skips `nfc` entirely for text that cannot change — all ASCII,
and nearly all Latin.

**Pro (chosen):** one answer per string, independent of spelling; the Unicode
half is testable against the UCD with no font in the picture; the font half is
testable with a closure and no font file.
**Con:** two passes rather than one, and a rule (below) about partial fits
that a font-dependent compose gets for free.

### Splitting is decided by the base, and the marks ride along

When a face cannot draw a composed character, `fit_to_face` replaces it with
its pieces only if the face can draw what the decomposition chain bottoms out
at. Marks are emitted whether or not the face has them.

Both halves were chosen from measurements against HarfBuzz over all 556
installed faces, and each half fixes a different failure:

* **Requiring the base.** A face with no `가` also has no `ᄀ` and no `ᅡ`.
  Splitting there turns one missing-glyph box into two — one more wrong thing
  on screen, and one more caret stop than the text has characters. 553 of 556
  faces disagreed with us on this before the rule existed.
* **Not requiring the marks.** A face with `ç` but no combining acute should
  still draw the `ç`. The base carries nearly all the meaning; a missing accent
  costs an accent, whereas refusing to split costs the letter too. 263 faces
  disagreed before this half.

### A mark takes its base's cluster

A combining mark is charged to the cluster of the character it attaches to,
not to its own byte offset. This is partly forced — canonical ordering moves
marks around, and clusters must not decrease along a run — and partly a
choice: a caret must not be able to land between a letter and its accent.

**Pro:** the caret moves over `é` as one thing however it is spelled, which is
what a text field wants and what a grapheme-aware editor would have had to
impose anyway.
**Con:** a mark is no longer individually addressable, so an editor that wants
to delete just the accent cannot use cluster boundaries to find it and must
consult the string.

### Deliberate divergences from HarfBuzz, and why they are not bugs

After this change the corpus sweep leaves three classes of disagreement. All
three are cases where HarfBuzz is font-dependent and we are not, and none is
tracked as a defect:

1. **Jamo runs (553 faces).** `ᄀ ᅡ ᆨ` is canonically `각`, and we compose it.
   HarfBuzz's Hangul shaper composes only when the font has the syllable, so on
   a face with no Hangul it emits three boxes where we emit one. One box for
   one character is the honest report of what NFC says the text is.
2. **Singletons (57 faces).** U+212B ANGSTROM SIGN is canonically U+00C5, and
   we fold it. HarfBuzz keeps U+212B when the face has a glyph for it. Ours is
   what the standard says; the two glyphs are the same shape in practice.
3. **Partial fits (27 faces).** The HarfBuzz self-inconsistency described
   above. We give both spellings the same answer, which is the point.

**Code:** `gui/font/src/norm.rs`, `gui/font/tools/gen_norm_tables.py`,
`gui/font/src/norm_tables.rs` (generated); wired in `ScaledFont::shape` in
`gui/font/src/scaled.rs`.

---

## §411 — Choose `GSUB` features by the run's script; decode them once per face

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

An OpenType feature tag is not unique. A face that supports both Arabic and
Latin registers a `liga` for each, filed under different ScriptRecords, and
they mean entirely different things. The walk in `otl.rs` used to start at the
FeatureList and take every feature carrying a wanted tag, so both applied to
every run. That is not a theoretical hazard — it was two live bugs on a stock
Windows host, and the second one had been mis-filed as a font quirk:

* `calibri.ttf` shaped `1/2` as `[1005, 877, 1006]` where every other shaper
  gives `[1005, 876, 1006]`. Lookup 92 is reached only by `calt` and `rclt`
  registered under `arab`; it rewrote the slash of a Latin string.
  (`B-FONT-CALIBRI-SHAPES-A-FRACTION-SLASH-DIFFERENTLY-FROM-HARFBUZZ`.)
* `ebrima.ttf` substituted the *space* glyph in plain English prose from a
  `ccmp` belonging to one of the African scripts it covers.
  (`TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES`.)

### Selection is per run; decoding is per face

The obvious implementation walks the ScriptList at shaping time. It was
measured and rejected: the worst face installed here re-reads 1874 subtable
offsets to answer one string, and `ScaledFont::shape` is on a
per-label-per-frame path. The obvious alternative — decode once per script and
keep a table per script — duplicates every lookup a face shares between
scripts, which on a pan-Unicode face is nearly all of them.

`ByScript` does neither. It resolves every script the face registers up front,
takes the *union* of their lookup indices, sorts and dedups it, and decodes
that list once. Each script then keeps only positions into the shared vector.
Selecting at shaping time is a binary search on a 4-byte tag.

Sorting the union is not incidental. Lookups must be applied in LookupList
order — that ordering is the whole mechanism by which `ccmp` runs before the
ligatures that depend on it — and decoding in sorted index order is what makes
each script's stored positions ascending, which is what makes "iterate the
positions in order" the correct application order rather than a coincidence.

**Pro:** one decode per face, no duplication, O(log n) selection, and the
ordering invariant falls out of the data layout instead of being asserted.
**Con:** a face's lookups are decoded even for scripts the user never types,
so the parse cost is the union rather than the subset. That cost is paid once
at face load and is bounded by `MAX_SUBTABLES`; the shaping path — the one
that runs per label per frame — pays nothing for it.

The subtable budget (`MAX_SUBTABLES`) is shared across the union rather than
per script. A per-script budget was tried first and is wrong twice over: it
multiplies the worst-case work by the script count, which is exactly what the
budget exists to bound, and it makes the answer for a Latin run depend on how
many other writing systems the face happens to cover.

### The fallback chain belongs to the run, not to the font

A run asks for its script, then that script's older OpenType spelling
(`dev2` → `deva`), then `DFLT`, then `dflt`. `DFLT` is the registered tag;
`dflt` is a misspelling real fonts ship and every shaper accepts. Text with no
script of its own — `"123 456"` — starts at `DFLT`, which is what HarfBuzz does
with a `Common` buffer.

The chain is applied when a *run* looks a script up. It is deliberately not
applied when `ByScript::parse` indexes the font, where each script is filed
under its own tag exactly: doing it there would file `DFLT`'s lookups under
every script's name and make the fallback unobservable.

### `GSUB` selects; `GPOS` does not

The asymmetry is deliberate. `GSUB` rewrites glyph *identity*, so a
wrong-script rule corrupts text. `GPOS` only moves glyphs, and every
positioning subtable is gated on glyph coverage, so a face's Arabic `kern`
simply does not cover Latin glyphs. `GPOS` is also reached through
`ScaledFont::kern(left, right)` — a public API that is handed a glyph pair with
no run behind it and therefore has no script to select with. It keeps the
union over all scripts through `feature_subtables`, tracked as
`TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`.

**Pro:** fixes the whole class of bug where it can corrupt text, without
inventing a script for a two-glyph positioning query.
**Con:** the two halves of the same table walk now answer differently, which is
a thing a reader has to be told rather than infer. Hence this section and the
note in `otl.rs`'s module doc.

### Reading `locl` became safe, and was overdue

`locl` is on by default in every shaper. This crate skipped it on the grounds
that it is language-specific and applying it blind would hand a reader some
other locale's letterforms. Under the old script-blind walk that was right. It
is no longer: only DefaultLangSys is read, so what `locl` yields is the face's
*default* localization — precisely what a shaper applies when the caller names
no language.

Skipping it was visible. `SansSerifCollection.ttf` maps `space` through its
Latin `locl`, so every space in every Latin string came out as the wrong glyph
on that face. Adding the tag fixed four of the five strings it disagreed with
HarfBuzz about.

### What the oracle says now

556 faces x 18 strings: 9066 agree, 942 differ, and every differing string is
accounted for:

| Count | String | Why |
|---|---|---|
| 553 | `각` | §410 class 1, jamo composition |
| 255 | `ḉ` | §410 class 3, HarfBuzz's own spelling-dependence |
| 57 | `Å` | §410 class 2, singleton folding |
| 44 | `العربية` | no Arabic joining shaper yet — fixed in §412, where these 44 move to `reversed` (identical glyphs, RTL order) |
| 27 | `été`, `e◌́te◌́`, `c◌̧◌́` | §410 class 3 |
| 5 | `हिन्दी` | no Indic reordering shaper yet |
| 1 | `hello שלום world` | we itemize the string, HarfBuzz guesses one script for it |

The last row is the change working. HarfBuzz's `guess_segment_properties`
assigns one script to the whole buffer, so it applies Latin's `locl` to the
space between the Hebrew and the English. We give that space to the Hebrew run,
which is what UAX #24 says and what any real itemizer does.

**Code:** `gui/font/src/script.rs`, `gui/font/src/script_tables.rs`
(generated by `gui/font/tools/gen_script_tables.py`), `ByScript` in
`gui/font/src/otl.rs`, `Substitutions` in `gui/font/src/gsub.rs`,
`ScaledFont::substitute_runs` in `gui/font/src/scaled.rs`. Checked by
`gui/font/examples/shape_dump.rs` + `gui/font/tools/harfbuzz_sweep.py`.

## §412 — A positional feature is gated by a per-glyph mask, not by its tag; and the mask belongs to the (script, lookup) pair

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

### The problem

A face reaches Arabic's four positional forms through the features `isol`,
`init`, `medi` and `fina`. Those are ordinary GSUB type-1 lookups. Their
coverage, in every real face, is *every Arabic letter the face has* — that is
what makes them useful: give `fina` any letter and it hands back that letter's
final form.

That makes them unlike every feature the shaper had handled before. `liga` is
unconditional: if it matches, it applies, and asking "should this glyph be
eligible for `liga`?" has the answer "yes, always". `fina` is conditional on
something the lookup itself cannot express — whether *this* letter, in *this*
word, happens to be at the end of a join. Selecting `fina` by tag and running
it the way `liga` is run rewrites an entire word into final forms.

So the shaper's model — "a feature is a set of lookups; run them in order" —
is not expressive enough. It needs a second input: which glyphs each lookup is
allowed to touch.

### The decision

Three parts.

**1. A per-glyph mask, intersected with a per-lookup mask.** Each feature tag
in the shaper's list is assigned a bit. A lookup's mask is the union of the
bits of the tags that selected it. A glyph's mask is the seven unconditional
bits, plus at most one positional bit chosen by the joining pass. A lookup is
applied at a position only when `glyph.mask & lookup.mask != 0`.

This is HarfBuzz's design and it was adopted deliberately rather than
reinvented. The alternative considered was a per-glyph *feature set* — carry
the list of tags each glyph is eligible for and test membership. It is more
obvious to read and strictly worse to run: shaping is a nest of two loops over
lookups and positions, so the eligibility test is the innermost operation in
the crate. One `and` against a `u32` is the right cost for it. The mask's
limit is 32 features, and the shaper's list has 11.

**2. The mask belongs to the (script, lookup) pair, not to the lookup.** This
is the part that is easy to get wrong. Faces share lookups between features —
one lookup can be reached by Latin's `liga` and by Arabic's `fina`. If masks
were folded per lookup across the whole font, that lookup's mask would carry
the `liga` bit, and `liga` is always on in every glyph's mask, so the
positional gate would be defeated for every glyph in the run. `ByScript` in
`otl.rs` therefore stores `Vec<([u8; 4], Vec<(u16, u32)>)>` — per script, the
lookups it selects, each with the mask *for that script* — and folding
duplicates happens only within one script's selection.

**3. Alternate substitution resolves to the first alternate.** GSUB type 3
offers a list of candidates and expects the caller to pick one by feature
value. OpenType numbers alternates from 1 and a boolean feature that is on has
value 1, which selects `alternates[0]`. This is not a guess: HarfBuzz reaches
the same answer by packing the feature's value into the glyph mask and
indexing with it, which for an on-by-default feature is exactly 1. Type 3 had
to be implemented at all because Microsoft Uighur and its relatives write
their `init`/`medi`/`fina` as type 3 rather than type 1. A user-facing
alternate picker (`salt`, `ss01`…`ss20`, `aalt`) would need the value to be
plumbed through as data rather than assumed; that is not built.

Note that `AlternateSubstFormat1` and `MultipleSubstFormat1` are byte-for-byte
identical. Only the lookup type distinguishes "this glyph becomes these three
glyphs" from "this glyph becomes the first of these three candidates". The
dispatcher must therefore trust the declared type and never sniff the
subtable; a test (`a_subtable_is_read_as_the_type_its_lookup_declares`) pins
that.

### What was rejected

**A dedicated Arabic pass that rewrites glyph IDs directly**, consulting
`cmap` for each form, bypassing GSUB. This is how a naive shaper does it and
it cannot work: the four forms are not required to be at any predictable
place, faces disagree about which letters have which forms, and a face is
entitled to reach a form through a contextual lookup rather than a simple
substitution. The face's own lookups are the only correct source. The joining
pass's job is to say *which* feature applies where and then get out of the
way.

**Checking the mask at every matched position.** HarfBuzz tests the mask at
each component of a ligature and each glyph of a context's input. We test only
at the applied position. This is a real narrowness and it is filed as
`TD-FONT-CHECKS-FEATURE-MASKS-ONLY-AT-THE-APPLIED-POSITION`; it was left
because the check wants to live in the same skipping iterator that lookup
flags need, and building it twice is worse than building it once.

### The joining rule itself

A character joins to a neighbour only when *both* are willing, and the
neighbour is the nearest one that is not `Transparent` — so a combining mark
between two letters never breaks the join. "Before" and "after" are logical
order, not visual: this runs before any reordering, and reasoning about it in
visual order is how RTL shapers get written backwards. Join-causing (ZWJ) is
excluded from `shapes()` on purpose — it makes its neighbours join but has no
glyph of its own to put a form on.

`joining::forms` early-outs when no character in the run has a joining type
that shapes, so Latin pays one table lookup per character and nothing else.

### What the oracle says now

556 faces x 18 strings, against HarfBuzz:

| | before | after |
|---|---|---|
| agree | 9066 | 9023 |
| reversed (same glyphs, RTL order) | — | 44 |
| differ | 942 | 941 |

All 44 Arabic-capable faces moved from `differ` to `reversed`: the glyphs are
identical and only the order differs, because HarfBuzz reverses an RTL buffer
for the caller and we do not reorder at all
(`TD-FONT-DOES-NOT-REORDER-RIGHT-TO-LEFT-TEXT`). `Amiri-Bold.ttf` gives
`[55, 1700, 1428, 1745, 3113, 2420, 1633]` against HarfBuzz's
`[1633, 2420, 3113, 1745, 1428, 1700, 55]`. The sweep was taught to classify
that case separately for exactly this reason — burying it in `differ` would
have hidden the fact that the shaping agrees perfectly, which for Arabic is
the whole question.

The remaining 941 are the three deliberate normalization divergences of §410
(553 + 255 + 57 + 27), 43 mixed-script Arabic where HarfBuzz's whole-buffer
script guess is *less* correct than our itemizer, 5 Devanagari with no USE
shaper, and 1 Hebrew in SansSerifCollection.

**Code:** `gui/font/src/joining.rs`, `gui/font/src/joining_tables.rs`
(generated by `gui/font/tools/gen_joining_tables.py` from Unicode 16.0.0),
`ByScript::lookup_indices` and `for_script` in `gui/font/src/otl.rs`,
`FEATURES`/`form_mask`/`apply_lookup`/`apply_alternate` in
`gui/font/src/gsub.rs`, `ScaledFont::shape` in `gui/font/src/scaled.rs`.
Checked by `installed_fonts_join_arabic_letters` in
`gui/font/tests/host_fonts.rs` and by `gui/font/tools/harfbuzz_sweep.py`.

## §413 — One skipping iterator for every matcher: skipping is not the same as not matching, and the feature mask gates the input only

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** Every GSUB lookup carries a `lookupFlag` naming glyph classes it
is not allowed to see — `IgnoreBaseGlyphs`, `IgnoreLigatures`, `IgnoreMarks`,
a mark-attachment class, or an explicit `MarkGlyphSet`. We parsed the field
and ignored it. Separately, §412 added a per-glyph feature mask but tested it
only at the position a lookup was applied to. Both are the same question —
"may this matcher consider this glyph?" — asked at every position every
matcher steps to, and `gsub.rs` had eight or so places that stepped with
`i + 1`.

**Decision.** A single `Skipper` (`gui/font/src/skip.rs`), built once per
lookup from the flag, the `markFilteringSet` index, the `GDEF` class
definitions and the lookup's feature mask. It exposes `next` / `prev` /
`at_or_after` / `considers` and the two counted walks `walk_forward` /
`walk_backward`. Every matcher in `gsub.rs` — the driver loop, the ligature
component walk, and the backtrack/input/lookahead walks of types 5 and 6 —
goes through it. This is the same shape as HarfBuzz's
`hb_ot_apply_context_t::skipping_iterator_t`, for the same reason: honouring
the flag in one matcher and not the others produces output that is harder to
debug than uniformly ignoring it.

**Three things the design had to get right.**

*Skipping is not the same as not matching.* A glyph the flag excludes is
**stepped over** and the match continues — that is precisely what lets an
`fi` ligature form across a fatha. A glyph the *feature mask* excludes is a
**non-match** and the walk stops. Conflating them in either direction is a
real bug: treat a skip as a stop and `IgnoreMarks` does nothing; treat a
non-match as a skip and a `fina` ligature forms across a letter that is not
in final form. So `skips()` continues the loop and `eligible()` returns
`None`.

*The mask gates the input only.* `Skipper::context()` returns the same
iterator with an all-ones mask, and the backtrack and lookahead walks use it.
A neighbour is a neighbour whatever feature reached the rule; gating context
on the mask made every chaining `fina` rule fail whenever its lookahead was a
medial letter. HarfBuzz draws the same line — `iter_input` carries
`c->lookup_mask`, `iter_context` does not.

*The mask does not change on recursion; the flag does.* A contextual rule
reached by `fina` is still a `fina` rule when it invokes a helper, so `Ctx`
keeps the mask across the nested call — but the nested lookup's own flag is
what the nested `Skipper` is built from.

**Alternatives rejected.**

- *A check inside each matcher.* Eight copies of the same predicate, which is
  how a shaper ends up honouring the flag in three matchers and not the other
  five. Rejected on the same grounds the entry it replaces was filed:
  partial support is worse than none.
- *Repositioning the skipped marks after a ligature, as HarfBuzz does.* When
  `f` mark `i` becomes `fi`, HarfBuzz moves the mark to sit after the whole
  ligature and rewrites its attachment component. We leave the mark where the
  run had it, which for the two-component case is already after the ligature
  glyph and therefore identical. It diverges only for a mark between
  components three and four of a four-component ligature, which no face in
  the 556-face sweep exercises. Filed rather than guessed at, because getting
  it wrong moves diacritics onto the wrong letter — a visible corruption —
  and the correct behaviour needs GPOS 5 (mark-to-ligature) to be meaningful
  anyway.
- *Honouring the flag in GPOS at the same time.* `kern.rs` and `mark.rs`
  reach their subtables through `otl::feature_subtables`, which returns a
  flat list of subtable offsets and has already thrown the `Lookup` away.
  Threading a `Skipper` there means changing that function's return type and
  both callers, which is a separate change with a separate blast radius. Left
  open in `TD-FONT-IGNORES-GSUB-LOOKUP-FLAGS`.

**What it is checked by.** Twelve unit tests in `skip.rs` covering each flag
bit, a `GDEF` too old to have mark glyph sets, an ineligible glyph stopping a
walk, and a walk that runs out of run. Six end-to-end tests in `gsub.rs` —
built on `gsub_flagged` / `gsub_lookups_flagged` / `gdef`, added because
every other GSUB test builder writes a zero flag and so exercised the skipper
only in its do-nothing configuration — proving that `IgnoreMarks` forms a
ligature across a mark and keeps the mark, that it still stops at a non-mark,
that the flag hides nothing when the face ships no `GDEF`, that a mark
filtering set hides every mark it does *not* name, and that a chaining rule
skips marks in its backtrack. Measured end to end by the sweep: the corpus
gained a fully vowelled Arabic string, which 8 of the 556 host faces shaped
wrongly before this change (949 differ / 36 reversed) and none do after
(941 / 44).

**Where.** `gui/font/src/skip.rs` (new), `Lookup::flag` / `Lookup::filter` and
`read_lookup` in `gui/font/src/otl.rs`, `Substitutions::parse` / `Ctx` /
`apply_lookup` / `apply_ligature` / `Matched` / the `forward`/`backward`
walkers in `gui/font/src/gsub.rs`, the `Substitutions::parse` call in
`gui/font/src/sfnt.rs`.

## §414 — Kerning reads across a mark by being told what stood between the pair, not by becoming run-aware

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** §413 gave GSUB a skipping iterator, but GPOS was left alone.
Kerning was the half that measurably mattered: real faces flag their `kern`
lookups `IgnoreMarks` precisely so that `A` and `V` keep kerning with an
accent between them, and this engine walked the run strictly in order and
could not skip. `scaled.rs` carried a comment apologising for it — pairs
separated by a mark simply went unkerned, so every accented word measured one
kern wider than the face asked for. On this host that is 82 of the 139 faces
that kern `(T,o)`.

The awkwardness is that kerning is not shaped like substitution. `GSUB` walks
a run and can hold an iterator over it; `Face::kern(left, right)` is a
*pair-at-a-time* query with no run behind it, called from the shaping loop
once per adjacent pair. A pair-at-a-time API cannot skip anything, because it
cannot see what it would be skipping.

**The decision.** Give the query a third argument instead of a run:
`kern_across(left, right, between)`, where `between` is the glyphs that stood
between the pair in the caller's run. The caller says what was there; each
lookup decides, from its own `lookupFlag`, whether it may read across it. A
lookup is consulted only if it would have skipped *every* glyph in `between`.

This required `Kerning` to stop flattening its subtables. `otl::feature_subtables`
returns a flat `Vec<usize>` and throws the `Lookup` away with the flag on it;
a new `otl::feature_lookups` returns the lookups whole, and `Kerning` keeps
one `Group { flag, filter, subtables }` per lookup. The flat shape is still
the right one for mark attachment, which asks a coverage-only question, so
both functions stay.

`scaled.rs` tracks the marks between the pair and charges the kern to the
pair's *left* glyph rather than to whatever it pushed last, so the advances
still sum to the run's width when the pair was read across a mark. Mark
attachment already measures its offsets from the accumulated pen, so the
accent lands on the letter regardless of the kern inserted after it.

**Alternatives rejected.**

*Make kerning run-aware, as `GSUB` is.* The honest shape: hand `Kerning` the
whole run and let it hold a `Skipper`. Rejected because `Face::kern` is public
API used by callers who genuinely do have only a pair (the host tests, the
terminal's fixed-cell measurement), and because the shaping loop already
walks the run once — a second walk inside `Kerning` would have to re-derive
the tab and script boundaries the loop already knows about. `between` carries
exactly the information the flag needs and nothing else.

*Skip marks unconditionally, since nearly every `kern` lookup is `IgnoreMarks`
anyway.* This is the tempting shortcut and it is wrong on real faces:
DejaVuSans and Verdana both ship `PairPos` lookups with flag 0, and HarfBuzz
accordingly widens `T`+acute+`o` by their full kern (348 and 220 units). An
engine that always skipped would disagree with both. The flag has to be read.

*Honour the flag in `mark.rs` at the same time.* Deferred, and recorded as
still-open in `known-issues.md`. `scaled.rs` already picks a mark's base by
walking back past marks, which is what `IgnoreMarks` would have said, so the
change would be a no-op today. It becomes real with GPOS 5
(mark-to-ligature), where `IgnoreLigatures` and the mark-attachment class
decide which component a mark lands on.

*Apply the flag to the legacy `kern` table too.* The legacy table predates
lookups and has no flags, so there is nothing to honour; it is parsed as one
group with flag 0, which is also the historically correct reading — engines
that used it kerned strictly adjacent glyphs.

**What it is checked by.** Five unit tests in `kern.rs` (reads across one mark
and across two; declines across a letter, and across a mark-then-letter; an
unflagged lookup declines; a flag with no `GDEF` behind it names an empty set
and so hides nothing; the legacy table kerns only adjacent glyphs), and the
host test `a_mark_between_a_kerning_pair_costs_the_kern_only_if_the_flag_says_so`,
which checks the whole thing at the level a caller sees it — `measure` of
`T`+combining-acute+`o` against HarfBuzz's own answer on five installed faces,
chosen so that three read across and two do not.

**Where.** `feature_lookups` in `gui/font/src/otl.rs`; `Skipper::skips` made
`pub(crate)` in `gui/font/src/skip.rs`; `Kerning` / `Group` / `parse` / `pair`
in `gui/font/src/kern.rs`; `Face::kern_across` in `gui/font/src/sfnt.rs`; the
`kern_left`/`between` shaping loop and `ScaledFont::kern_across` in
`gui/font/src/scaled.rs`.

## §415 — Bidi belongs inside the shaper: glyphs stay in logical order and carry a permutation beside them

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** `gui/font/src/bidi.rs` implements UAX #9 and passes all 91,707
cases of Unicode's `BidiCharacterTest.txt`. Wiring it into `ScaledFont::shape`
raised two questions that the conformance suite says nothing about: *where*
the pass runs, and *what shape* its answer takes in `ShapedRun`.

`known-issues.md` had already proposed an answer to the first —
"the bidi pass belongs above the shaper, next to `script::runs`, and wants to
be written once for the whole toolkit rather than hidden inside the font
crate" — and implementing it showed that answer to be wrong.

**The decision, part one: the levels are resolved inside `shape`.** Five
things depend on the embedding levels, and three of them are shaping
decisions that no layout stage above the shaper could make:

* **Rule L4, mirroring.** `(` in a right-to-left run is drawn with the glyph
  for `)`. That is a *character* substitution, so it has to happen before
  `cmap` is consulted. A caller above the shaper can only mirror by editing
  the string and shaping again.
* **Run splitting.** `script::runs` now splits on level parity as well as on
  script, because a ligature or a kern must not form across a direction
  change. The run list is built inside `shape`.
* **Kern re-charging** (part three below), which needs the glyph vector.
* Reordering itself, and the caret queries — the two a layout stage *could*
  have done.

Three of five is decisive. Resolving levels above the shaper would mean
passing them back down again for the other three, which is the same coupling
with an extra interface.

**The decision, part two: `ShapedRun` stores glyphs in logical order and
carries a `visual: Vec<u32>` permutation.** The obvious alternative is to
store them already reordered. It was rejected because logical order is what
every existing query on the type is built on: `cluster` is documented
non-decreasing along the run, `offset_at` binary-searches on it, `glyphs()` is
zipped against the source text by two callers. Reordering the vector breaks
all of that silently — the code still compiles, the clusters merely stop being
sorted — and the fix would be to re-derive the logical order from the visual
one, which is the same permutation kept in the other direction.

So: `glyphs()` is logical and unchanged, `draw_order()` walks the permutation,
and a renderer uses the latter and needs to know nothing else. The permutation
is left **empty when it would be the identity**, which is the common case, so
`draw_order()` on English text iterates the glyph slice directly with no
indirection and `is_reordered()` is a length check.

**The decision, part three: kerns are re-charged onto the new left glyph.**
The shaping pass charges each pair's kern to the *logically*-first glyph,
which is the left one in left-to-right text and the **right** one after
reversal. Leaving it there puts the gap on the far side of the pair — an
`AV` kern in a Hebrew run would tighten the space to the left of the `A`
rather than between the two. `recharge_kerns` strips every kern, walks the
pairs that are adjacent *in drawing order*, and gives each its kern back on
the left-hand glyph. A pair that became adjacent only because of the reversal
gets nothing, which is correct: those two glyphs were never a kerning pair,
and HarfBuzz does not kern across a direction boundary either.

Mark offsets needed no such treatment, which is worth recording because it
looks like it should. `attach_marks` computes `offset.x = ... + (pen[base] -
pen[mark])`, so the mark lands at `pen[base] + dx` whichever of the two is
drawn first — provided the pens are accumulated in *drawing* order. Making
that loop follow `visual` was the whole change.

**The decision, part four: a fast path that never consults the class table.**
`is_trivially_ltr` rejects a string with one numeric comparison per character:
the lowest right-to-left code point in Unicode is U+0590, so anything below it
cannot be strong-RTL and cannot be an explicit format character. Latin, Greek,
Cyrillic, Han, Kana and Devanagari — that is to say, almost everything — leave
`shape` having paid one scan, with no `resolve` call, no level vector, no
permutation and no re-charging pass.

**How it was checked.** `tools/harfbuzz_sweep.py` compares against HarfBuzz
over every installed face; `examples/shape_dump.rs` prints *both* orders
because HarfBuzz has no bidi in it and answers in whichever one its
`guess_segment_properties` guessed. Across 556 faces × 19 strings the
`reordered` bucket — same glyphs, different order — went from 44 to **0**.

The sweep also learned to compare positions, in font units, which caught
nothing about bidi and three things about everything else (all now filed in
`known-issues.md`). It required one insight to be useful at all: the two
engines charge a kern to different glyphs and *both are right*. HarfBuzz
splits a legacy `kern` value, giving `k >> 1` to the left glyph's advance and
the remainder to the right glyph's advance *and* offset; we put the whole
correction on the left advance, because that is what makes a run's width the
sum of its advances. Arial Rounded Bold shaping `Th` is the worked example:
HarfBuzz says advances 1266, 1224 with the `h` offset -13, we say 1253, 1237
with no offset, and every glyph lands on the same pixel. So the sweep compares
accumulated *ink positions* and the total width, not raw advances.

**Where.** `gui/font/src/scaled.rs` — `shape`, the `byte_levels` and
`recharge_kerns` helpers, and the drawing-order pen loop in `attach_marks`;
`gui/font/src/shape.rs` — `ShapedRun::visual`, `reordered`, `draw_order`,
`is_reordered`; `gui/font/src/script.rs` — `runs`, which now takes levels;
`gui/font/src/bidi.rs` — `visual_order`, `render_levels`, `is_trivially_ltr`;
`gui/font/examples/shape_dump.rs` and `gui/font/tools/harfbuzz_sweep.py`.

## §416 — Place a mark by measurement when the face has no `GPOS` at all, reimplementing HarfBuzz bug-for-bug, and refuse the scripts whose clusters need a shaper we do not have

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** `tools/harfbuzz_sweep.py` compares this crate against HarfBuzz
over every installed face. Its `misplaced` bucket — same glyphs, different
positions — stood at **559** of 10,564 runs, and roughly **489** of those were
one failure: a combining mark drawn at the pen. `c` + U+0327 + U+0301 on a
face with no `GPOS` put both accents in the gap *after* the letter, where they
overprinted each other and the next glyph. 222 of the host's 556 installed
faces have no `GPOS` table whatsoever; they were built when `é` was one
character with one glyph and a bare U+0301 was somebody else's problem.

Three decisions came out of fixing it.

**Part one: the trigger is "no `GPOS` table", not "no `mark` feature".**
The tempting rule is "if the font cannot tell me where this mark goes,
measure". That is wrong, and a face on this machine proves it: Candara has a
`GPOS` with kerning and no `mark` feature at all. HarfBuzz leaves its marks
exactly where they fall, and so must we. The distinction is that a `GPOS`
table is a *statement* — the designer opened the file, wrote down positioning
rules, and did not write one for this mark. Overruling that is second-guessing
a decision that was made. A missing `GPOS` is not a statement; it is the
absence of one, and there is nothing to overrule. `Face::has_positioning`
exists to carry exactly this distinction, separately from the three things
this crate actually reads out of `GPOS` (`kern`, `mark`, `mkmk`).

**Part two: reimplement HarfBuzz's arithmetic exactly, including its
rounding.** `gui/font/src/fallback.rs` is a transcription of
`hb-ot-shape-fallback.cc`: the `upem/16` gap, the truncating integer division
in the centring, the two `if` branches that pull a mark back when the gap and
the computed offset have the same sign, the growth of the base's box after
each mark so a second accent clears the first. It would have been easy to
"improve" any of these. Two reasons not to. The output is *checked* against
HarfBuzz, so an improvement reads as a regression in the sweep and hides real
divergence in the noise. And the numbers are not arbitrary taste — they are
what a decade of complaints about specific fonts settled on, and this crate
has no evidence to overrule them with. The arithmetic is done in font units
as `i32` for the same reason: doing it in floats would round differently in
the last unit and make an exact comparison impossible.

The one deliberate divergence is that we do not implement HarfBuzz's
*modified* combining classes, which permute Hebrew ccc 10–26 and Arabic 27–36
so that marks sort into display order. We do implement the recategorization
those classes feed — transposed to match on real Unicode classes, which is
sound because the permutation is injective within each block — but not the
re-sorting. Two Hebrew points in the wrong canonical order stack in the wrong
order. Filed in `known-issues.md`.

**Part three: the script gate, which is the part that was nearly got wrong.**
The first working version ran the fallback on every script, and the sweep
immediately caught a *new* failure: 33 Devanagari runs, where the virama's
advance was zeroed and the glyph centred on the consonant before it.

The available quick fix was to skip combining class 9 (virama). It would have
made the sweep green. It would also have been a coincidence: HarfBuzz's actual
rule is per-script, `hb_ot_shaper_t::fallback_position`, and it is `false` for
the Indic, Khmer, Myanmar and USE shapers and for Thai/Lao — the whole
Brahmic-and-relatives family, viramas and matras and nuktas alike, not one
combining class out of it. Matching the symptom rather than the rule would
have left the other classes wrong and put a band-aid where the reason belongs.

So `fallback::positions_marks` gates on the run's OpenType script tag, against
the script set of `hb_ot_shaper_categorize` in `hb-ot-shaper.hh` mapped
through this crate's own tag table — 101 tags, binary-searched. The reason it
is the right rule and not merely HarfBuzz's rule: in a Brahmic cluster the
marks are not a stack of accents on one base. A virama is a *spacing* glyph
that suppresses a vowel; a matra may be reordered to before the consonant it
logically follows; which glyph a mark belongs to is the output of a reordering
pass, not "the last non-mark to my left". Measuring a box and centring on it
produces a confident wrong answer. Leaving the mark at its natural advance
produces an obviously unshaped one, which is both more honest and, in
practice, closer to legible.

Getting the gate required hoisting `script::runs` out of the
`if has_substitutions()` branch of `shape`, so it is now computed once and
used by both the substitution pass and the placement pass. That is strictly
better regardless: the two passes were always answering the same question.

**One thing not copied.** HarfBuzz decides the shaper from the tag the *font*
registers its features under, so a Devanagari-glyph face whose `GSUB` says
only `DFLT` gets the default shaper and does get the fallback. We match on
the tag the *character's script* maps to, unconditionally. Following
HarfBuzz here would mean asking the font which scripts it declares before
deciding how to place a mark — a coupling not worth having for the sake of
faces that carry Devanagari glyphs, no `GPOS`, and no Devanagari `GSUB`
script record. Filed in `known-issues.md`.

**What it measured.** Over 556 faces × 19 strings the `misplaced` bucket fell
from **559 to 98**, and the two largest contributors — `c` + cedilla + acute,
and vowelled Arabic — vanished entirely. The 98 that remain are kerning-charge
and `GPOS`-path divergences that predate this work. Unit tests went 320 → 336;
`installed_fonts_without_gpos_still_place_combining_marks` checks the 22 host
faces that have no `GPOS` and do draw a combining acute.

**Where.** `gui/font/src/fallback.rs` (new); `gui/font/src/sfnt.rs` —
`Face::has_positioning`, `Face::glyph_bbox`; `gui/font/src/gsub.rs` —
`SubGlyph::klass`, which rides the attachment class through substitution
because a `cluster` is shared by a base and its marks and a glyph id is
changed by substitution; `gui/font/src/scaled.rs` — `synthesize_marks`, the
`pens` helper factored out of `attach_marks`, and the hoisted `script::runs`;
`gui/font/tests/host_fonts.rs`.

## §417 — One `GPOS` pass over a segmentation shared with `GSUB`; and a `GPOS` kern is not moved when the run reverses

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** `GPOS` has eight lookup types. This crate read three of them, and
read them in two places that each went to the table on their own:
`gui/font/src/kern.rs` picked out the type-2 pair lookups and walked them, and
`gui/font/src/mark.rs` picked out the type-4 and type-6 mark lookups and
walked those. Types 1 (single adjustment) and 3 (cursive attachment) had no
walker, so they were skipped entirely, which is what
`TD-FONT-IGNORES-GPOS-SINGLE-AND-CURSIVE-ADJUSTMENTS` was filed on.

The obvious increment — add a third standalone walker for types 1 and 3 —
is wrong for a reason that is worth writing down, because it is the same
reason §408 gave for `GSUB`. **A `GPOS` table is an ordered list of lookups,
and the order is semantic.** A face routinely tunes one letter's fit with a
type-1 adjustment and then kerns the tuned letter with a type-2 pair; run the
pair lookup first and the pair sees a glyph the font never intended it to see.
Three walkers each sweeping the whole run cannot express that ordering no
matter what order the walkers themselves are called in.

**Decision, part one: one pass.** `gui/font/src/gpos.rs` takes a `Run` — the
glyphs, their nominal advances, which of them are marks, the direction, the
script — and returns one `Adjust` per glyph. Inside, the lookups a run's
features reach are collected once and applied in table order, each through the
same `Skipper` that `GSUB` uses, with types 1, 2, 3, 4 and 6 dispatched from
one place. `kern.rs` keeps only the legacy `kern` table, which the pass
genuinely cannot see; `mark.rs` keeps only its anchor and subtable readers,
which the pass calls.

*Against:* it is a bigger change than adding a walker, and it deletes public
API (`Face::mark_on_base` / `mark_on_mark`) that a test was using to sweep
mark placement across every installed face. *For:* the alternative is
knowingly-wrong output that gets harder to correct with every walker added,
and the deleted API was itself the bug in miniature — it let a caller ask for
mark attachment *out of lookup order*, which is not a question with a correct
answer. The test now sweeps through `ScaledFont::shape`, which is what a real
caller does anyway.

**Decision, part two: the two passes share one segmentation.** `shape` cuts
the string into `Segment`s — on tabs and on script changes — once, before
substitution, and hands the same segments to positioning. The alternative,
re-deriving the cut after substitution, is not merely wasteful: it is
impossible. Once a stretch has ligated there is nothing left in the glyph run
that says where it began, because a ligature is one glyph for several
characters. The cost is that `substitute_runs` now runs even for a face with
no `GSUB`, purely to produce segments. That is a walk over the run with no
lookups to apply, which is cheap, and the alternative is a second
segmentation that must be kept in agreement with the first by hand.

**Decision, part three: `recharge_kerns` is for the legacy `kern` table
only.** This is the subtle one, and it was a live bug for the length of one
sweep. When bidi rule L2 reverses a run, `recharge_kerns` strips every kern
and re-charges it to whichever glyph is now on the *left* of each visually
adjacent pair — because an advance pushes the pen rightwards regardless of
which way the text reads, and a legacy `kern` value is a pure gap with no
direction in it.

A `GPOS` pair is not a pure gap. A font expressing a right-to-left adjustment
writes **XPlacement and XAdvance into the same value record**: the placement
moves the ink to open the gap, the advance keeps the rest of the line where it
was. That idiom is self-correcting under reversal — it has to be, since the
font author knows the run will be reversed — which means an engine that also
moves the kern applies the correction twice. Measured on Amiri-Bold shaping
`العربية`: HarfBuzz charges the pair's +83 to gid 1745 (343 → 426) alongside
its x_offset of 83; we were charging it to gid 3113 (247 → 330) *and* keeping
the x_offset, putting glyph 3 at x=960 where HarfBuzz has 877.

So `shape` gates the call on `Face::kerns_outside_gpos()`. A `GPOS` value
belongs exactly where its record put it; a legacy value belongs to whichever
pair ends up visually adjacent. *Against:* two kerning paths that behave
differently under reordering is a thing to remember. *For:* they are two
different things — one is a font author's directional statement and the other
is a direction-blind number from a table that predates `GPOS` — and treating
them alike is what was wrong.

**Measured.** `tools/harfbuzz_sweep.py` over 556 host faces x 19 strings:
agree 9526 → 9539, misplaced 98 → 85, reordered 0 before and after. The
Arabic string the known-issues entry was filed on went from 14 disagreeing
faces to 1.

**Where.** `gui/font/src/gpos.rs` (new — `Run`, `Adjust`, the pass);
`gui/font/src/scaled.rs` — `Segment`, `substitute_runs`, `position_segments`,
the `recharge_kerns` gate, and the deleted `attach_marks`;
`gui/font/src/kern.rs` — `Kerning::is_legacy`; `gui/font/src/sfnt.rs` —
`Face::position`, `has_gpos_lookups`, `kerns_outside_gpos`;
`gui/font/src/mark.rs`; `gui/font/tests/host_fonts.rs`.

## §418 — Ligature component numbers are carried on the glyph run, not inferred at positioning time

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

**Context.** `GPOS` type 5, mark-to-ligature attachment, is the lookup that
places a mark over one *component* of a ligature. A `LigatureAttach` table
offers one row of anchors per component, and applying it requires knowing which
row — which half of an `ﻻ` the fatha belongs over. Type 4's reader plus a row
index would have been an afternoon's work; the problem is that nothing in our
run answered "which row".

By the time `GPOS` runs, the ligature has already formed. The run holds one
glyph where three stood, and the mark's cluster is the byte offset of the
*first* character behind the ligature — shared with the base and with every
other mark on it, so it cannot tell them apart. The information is destroyed
during substitution and there is no way to recover it afterwards.

**Options.**

1. **Always use the last component.** HarfBuzz's own fallback when it cannot
   identify the component. One line, no bookkeeping, no new state on the glyph.
2. **Re-derive the component from clusters at positioning time.** Count how
   many characters the ligature's cluster spans, count marks, divide.
3. **Record it during substitution**, as HarfBuzz does: a `lig_props` byte per
   glyph written by ligature substitution and read back by `GPOS`.

**Chosen: 3**, ported from HarfBuzz's `ligate_input` / `match_input` /
`Sequence::apply` rather than reinvented.

**Reasoning.**

*For 1.* It is what the format degrades to anyway for uncovered cases, and for
Latin — where a ligature is `ffi` and nobody puts a mark on it — the difference
is unobservable. It would have shipped in twenty minutes.

*Against 1.* It is wrong exactly where the feature exists to be right. Arabic
`لَا` is lam + fatha + alef: the two letters ligate across the fatha, and the
fatha belongs over the *lam* — component 1 of 2. "Last component" puts it over
the alef, every time, on every vowelled Arabic word with a lam-alef in it. That
is not a fractional-pixel error; it is a diacritic on the wrong letter, which
in Arabic changes what the word says. Shipping the fallback as the *only* path
would have meant writing code whose entire purpose is to be correct here and
having it be wrong here.

*Against 2.* It cannot work. A cluster is a byte offset, and a ligature keeps
its first component's — so a two-component and a three-component ligature over
the same text are indistinguishable by cluster. Worse, `ccmp` decompositions
and nested ligatures make the character-to-component mapping non-monotonic, and
a mark can end up *after* the whole match while belonging to a component inside
it. Any inference would be a guess that fails on precisely the fonts that need
type 5.

*For 3, and the real cost.* It is not one field. Getting HarfBuzz's answers
requires its three special cases — a base joining with only marks stays a base
so further marks can still attach to it; a ligature of only marks keeps its old
id so it still belongs to the ligature underneath; the pieces of a
decomposition after the first are worth zero components so a later ligature
counts them as one thing — plus `match_input`'s legality rules that stop a mark
being pulled out of one ligature and joined to a stranger, plus a trailing
re-adjust pass because a component may itself be a ligature whose marks stand
after the match. Each of those exists because a real font broke without it, and
each is cited in the code with the HarfBuzz issue behind it. That is ~120 lines
of transcription where option 1 was one line.

We took it because the alternative was a known-wrong result on a whole script,
and because the standing rule here is that a fix that needs a refactor gets the
refactor. Option 1 survives *as the fallback* — `lig_attachment` uses the last
component when the ids disagree, which is both HarfBuzz's behaviour and the
right answer for the faces that write mark-to-base as a one-component type 5.

**Consequence.** `SubGlyph` is one byte-ish wider (a `Lig` of four small
fields, unpacked from HarfBuzz's bit-packing because keeping the packing would
buy nothing in a struct the caller does not allocate per character, and would
hide the genuinely subtle part — that the component count and the component
index are mutually exclusive). Substitution does bookkeeping work on every
ligature whether or not the face has a type-5 lookup; the cost is a few
arithmetic ops per matched component and no allocation.

**Measured.** Over 556 host faces, 56 of which ship a type-5 lookup, with a
lam-fatha-alef string added to the sweep corpus: `agree` 10055 → 10081,
`misplaced` 125 → 99. Nothing regressed.

**Where.** `gui/font/src/gsub.rs` — `Lig`, `stamp_components`, `renumber`,
`ligation_allowed`, `base_is_hidden`, and the numbering in `apply_multiple`;
`gui/font/src/mark.rs` — `marked`, `lig_attachment`; `gui/font/src/gpos.rs` —
`MARK_LIG_POS` and `attach_to_lig`; `gui/font/tools/harfbuzz_sweep.py` — the
corpus string that can see the difference.

## §419 — Marks are sorted twice: once into canonical order, once into the order they are drawn

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

Unicode's canonical combining classes are an *ordering*, chosen so that two
spellings of the same text normalize to one string. They are not a description
of where a mark sits, and for the fixed-position classes — Hebrew 10–26, Arabic
27–35, Syriac 36, Telugu 84 and 91, Thai 103, Tibetan 129–132 — the order they
impose is close to the opposite of the order the marks are stacked in. A Hebrew
vowel is typed before the shin dot and drawn under the letter while the dot goes
over it. Arabic shadda is typed after a vowel and drawn nearer the letter than
it; Unicode's own normalization FAQ names that case and declines to renumber,
because the classes are a stability guarantee and cannot move.

So a renderer that sorts marks only into canonical order stacks them wrong. Ours
did, and it was 465 of the sweep's 841 `misplaced` cases — one string,
`שָׁלוֹם` typed with the qamats before the shin dot.

**Decision.** Sort twice. `norm::sort_marks` takes the class function as a
parameter; `nfc` calls it with `combining_class` and the shaper's entry point,
`norm::pieces`, calls it a second time with `display_class` — a permutation of
the fixed-position blocks whose *numeric* order is stacking order, bottom to
top. This is HarfBuzz's `_hb_modified_combining_class`, which exists for exactly
this and does exactly this.

**Where the second sort goes, which is the actual decision.** §410 says `nfc()`
is pure Unicode and never sees a font, and that is the promise worth keeping:
it is the function a caller with a *text* question asks, and its answer must be
NFC or it is not an answer. HarfBuzz has no such constraint — its normalizer is
private to the shaper — so it substitutes the modified classes inside
normalization and sorts once. Three options were live:

1. *Sort with the modified classes inside `nfc`,* as HarfBuzz does. Smallest
   change and one pass instead of two. Rejected: `nfc` would return something
   that is not NFC, silently, and the name would be a lie. §410's two-layer
   split exists precisely so that "what the text is" has a home separate from
   "what this face can show".
2. *Sort in the shaper, in `scaled.rs`.* Keeps `norm.rs` entirely about
   Unicode. Rejected: the sort is a Unicode fact — a table of classes and a
   stable insertion sort — and putting it in `scaled.rs` means `scaled.rs`
   grows a second copy of the loop `norm.rs` already has, and a second answer to
   "which characters are starters".
3. **Chosen:** sort in `norm::pieces`, the shaper's entry point, leaving `nfc`
   exactly NFC. `pieces` already promises less than `nfc` does — it is
   *"the characters a face should actually be asked for"*, and has been
   font-dependent since §410 — so display order is the same kind of claim as
   `fit_to_face`'s, made in the same place.

**Pro:** one implementation of the sort, one table, one definition of a
starter; `nfc` keeps its meaning; the fallback's `attach_class` needs no change
at all, because it matches on real classes and the permutation is injective
within each block.
**Con:** `pieces` output is not NFC for five scripts, which a reader could
assume it is. Stated in its doc comment, in `display_class`'s, and here.

**The bijectivity is load-bearing and is tested.** If two classes the canonical
sort keeps apart mapped onto one display class, the second sort would merge them
and their *typed* order would start deciding their stacking order — silently,
and only for the colliding pair. `each_permuted_block_is_a_bijection_onto_itself`
checks it. It also found the one place the claim is narrower than it looks:
Tibetan 132 maps to 131, which is legal only because Unicode assigns no
character class 131. HarfBuzz's table collides there too.

**Measured.** 556 faces x 23 strings: `agree` 10917 → 11223, `misplaced`
841 → 625, `reordered` 32 → 0, `differ` 998 → 940. The pointed-Hebrew string
went 465 → 249. Nothing regressed.

**Where.** `gui/font/src/norm.rs` — `display_class`, `permute`, `sort_marks`
and the second call in `pieces`. `gui/font/src/fallback.rs` — `attach_class`,
whose doc records why matching real classes still selects the right characters.

---

## §420 — The HarfBuzz sweep reports mixed-script strings apart from its verdict

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

`gui/font/tools/harfbuzz_sweep.py` shapes a 23-string corpus with both this
crate and HarfBuzz over every face on the host, and its value is entirely in
the *set* of disagreements staying the one you expect. Two of the 23 strings
mix scripts:

    hello <hebrew> world
    abc <arabic> xyz

The two halves of the sweep cannot be asked the same question about those.
`hb_buffer_guess_segment_properties` takes the first strong character and
picks **one** script and one direction for the whole buffer; both strings
start with Latin, so HarfBuzz shapes the Hebrew and the Arabic as Latin. This
crate itemizes into script runs and shapes each on its own. The resulting
differences are real and are the itemizer's, not the shaper's:

* FrankRuehlCLM-Bold put `vav` 10 units left of HarfBuzz, because our `hebr`
  run reaches a kern that HarfBuzz's `latn` buffer never selects. The Hebrew
  word shaped **on its own** agrees exactly, -10 and all.
* 43 Arabic faces returned isolated forms from HarfBuzz where we returned
  joined ones, for the same reason.

Two options.

**Itemize inside the sweep** — split the string into script runs in Python and
shape each with its own HarfBuzz buffer, so both halves answer the same
question. Correct in principle, and it would turn 49 dead lines into real
comparisons. But `uharfbuzz` exposes no script-property lookup, so it means
reimplementing the itemizer in the oracle: the Unicode script property, the
Common/Inherited absorption rule, and the bidi run splitting. An oracle that
reimplements the thing under test tells you the two implementations agree with
*each other*, and every rule mirrored slightly wrong becomes a false alarm
that costs a diagnosis to dismiss.

**Report them apart** — keep shaping the whole buffer, count the mismatches,
and print them under their own heading instead of mixing them into
`misplaced`/`differ`. Chosen. The cost is that a genuine mixed-script
regression would land in a bucket already known to be non-empty, which is a
real loss. It is outweighed by what the buckets were doing before: 49 of the
report's lines were noise, and each of the two had already cost a full
diagnosis session to trace back to `guess_segment_properties`. With them held
out, `misplaced` fell from 19 to 13 and every remaining entry is one string —
Devanagari, waiting on the Indic shaper — so the next real regression will be
visible the moment it appears.

Revisit if the crate ever exposes its run boundaries to `shape_dump`: the
sweep could then feed *our* itemization to HarfBuzz, which is the honest
version of the first option and costs no reimplementation.

**Where.** `gui/font/tools/harfbuzz_sweep.py` — the `MIXED` set, the branch on
it in `main`, and the `mixed` line in the report.

## §421 — Transcribe HarfBuzz's Indic shaper, not the Universal Shaping Engine it superseded

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

`TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER` proposed "a Universal Shaping
Engine pass for Indic and South-East Asian" as the second of its two shapers.
That is the shape the Unicode spec suggests — USE was written to be the one
shaper every complex script could share, and the Indic scripts are complex
scripts — and it is what a reading of the standards would produce. It is
nevertheless the wrong target here, and the reason is a fact about the oracle
rather than about the spec.

**HarfBuzz does not run USE for Devanagari.** `hb-ot-shaper-indic.cc` and
`hb-ot-shaper-use.cc` are separate modules, and `hb_ot_shaper_categorize`
sends the nine Indic scripts to the first one. The split is not an accident of
history that USE will eventually absorb: the Indic scripts' reordering is
specified against **Uniscribe**, whose behaviour predates USE and whose quirks
Microsoft's fonts were built around — the `dev2`/`deva` tag revision, the
old-spec halant move, the reph position table, Malayalam's zero-context
exception. USE's cluster grammar deliberately does not reproduce them.

So the two options were not "USE or a narrower USE".

**Write USE and point Devanagari at it.** One shaper instead of two, covering
~90 further scripts as a side effect, and the standards-blessed model. But it
would have matched neither oracle: not HarfBuzz, which runs the Indic shaper,
and not the fonts, which were built for Uniscribe. Every disagreement it
produced would have needed a judgement about whether HarfBuzz or the spec was
right, which is exactly the diagnosis cost the sweep exists to avoid. And it
would not have fixed the measured symptom, since the sweep's only complex-script
strings are Arabic and Devanagari.

**Transcribe the Indic shaper.** More total code — USE still has to be written
afterwards for the scripts it covers, and is now filed as
`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE` — and a second syllable grammar to
maintain. In exchange every disagreement is a bug in the transcription and can
be diagnosed by reading one file of HarfBuzz beside one file of ours, which is
the property that took `हिन्दी` from 5 disagreeing faces to 0 and the whole
`misplaced` bucket to 0.

The second is what was done. The parts that are *not* Indic-specific were
written to be reused: `apply_stages` (a lookup set per stage, the shaper's own
features confined to one syllable), the syllable stamping in `setup_syllables`
(a byte per glyph rather than a range, so a `ccmp` ligature cannot invalidate a
boundary), and `Plan`'s once-per-run probing of what the face declares. USE
needs all three and none of them assumes Indic.

**Where.** `gui/font/src/indic.rs`, `indic_machine.rs`, `indic_shape.rs`;
`gui/font/src/gsub.rs` — `apply_stages`; `gui/font/src/sfnt.rs` —
`Face::substitute`, which dispatches on `Script::shaping`.

## §422 — The face chooses the shaper, not the character

**Date:** 2026-08-14
**Decided by:** Claude (autonomous)

Which shaper a run gets looked like a property of the text: Devanagari
characters want the Indic shaper, Arabic characters the joining one. That is
what `fallback::positions_marks` and `Face::substitute` both assumed, and it is
wrong. `hb_ot_shaper_categorize(script, direction, gsub_script)` takes a third
argument, and for most complex scripts the **font** gets a veto: a Devanagari
run in a face that files its `GSUB` features under `DFLT` or `latn` is shaped
by the *default* shaper, which places marks by measurement and zeroes their
advances where the Indic shaper withholds both. `Hack` rendering `हिन्दी` is
that face and that string, and it was the last 13 cases in the sweep's
`misplaced` bucket.

The veto is not uniform, which is the awkward part: Thai, Lao and Khmer reach
their shapers unconditionally, Myanmar treats a third tag (`mymr`, which
predates the Myanmar shaping spec) as a veto alongside `DFLT` and `latn`, and
Arabic inverts the test (`gsub_script != DFLT`). There is no stated principle
behind the asymmetry — the Thai shaper predates the check, Khmer was split out
of Indic after it — so it is transcribed rather than tidied, in
`fallback::ALWAYS_COMPLEX` and the `mymr` arm of `shaped_as_default`.

**Two options for where the answer lives.**

*A `Shaper` enum resolved once per run*, mirroring HarfBuzz's struct: each
variant carrying `fallback_position`, `zero_width_marks` and the substitution
entry point, with every consumer reading fields off it. Structurally the right
shape, and it makes the "three questions, one answer" property unforgeable.
But `COMPLEX_SCRIPTS` and `NO_ZERO_WIDTH_MARKS` are *measured* lists — probed
against real faces, not read off the source — and folding them into an enum
means rederiving both from the shaper table by hand, with 11,834 agreeing runs
riding on getting it right. The rewrite risked more than it bought.

*A predicate beside the lists that already exist.* `shaped_as_default(tags,
gsub)` answers "did the face call this run's shaper off", and the three
consumers take it as a parameter. Less tidy — the shaper is still implicit,
spread across three lists and a predicate — but every existing measurement
survives untouched, and the one new claim is small enough to test directly.

The second was chosen. The discipline that replaces the enum is that
`scaled.rs` resolves the answer **once per run** and feeds all three consumers
from that one binding: whether the Indic shaper runs, whether marks may be
placed by measurement, and whether their advances are zeroed are three fields
of one HarfBuzz struct, and the failure mode of the predicate form is letting
them disagree.

**One correction the implementation forced.** The chosen tag has to be read
from the raw `GSUB` ScriptList, not from `Substitutions`. `Substitutions`
records only the scripts that reached a lookup this crate can apply, and
`ByScript::parse` returns `None` outright when none does — so `Hack`, with 16
`GSUB` lookups and both `DFLT` and `latn` registered, appears to name no script
at all, and routing the question through it left the sweep stuck at
`misplaced 5`. `Face` now holds `gsub_scripts` and `otl::chosen_from` walks
HarfBuzz's `hb_ot_layout_table_select_script` chain over that. `None` is
`HB_TAG_NONE`, which equals neither `DFLT` nor `latn`, so a face with **no**
`GSUB` keeps its complex shaper — which is exactly the kind of face
`NO_ZERO_WIDTH_MARKS` was measured against.

**Known divergence, deliberate.** Arabic's inverted test would demote a Syriac
run in a `latn`-only face to the default shaper and so suppress
`init/medi/fina/isol`. It is not modelled, because both the Arabic and the
default shaper set `fallback_position = true` and `zero_width_marks =
BY_GDEF_LATE` — the divergence is in joining only, and is recorded under
`TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-CHARACTERS-SCRIPT-NOT-THE-FONTS`.

**Where.** `gui/font/src/fallback.rs` — `shaped_as_default`, `ALWAYS_COMPLEX`,
and the `simple` parameter on `positions_marks`/`zeroes_mark_advances`;
`gui/font/src/sfnt.rs` — `gsub_scripts`, `shapes_as_default`,
`gsub_chosen_script`; `gui/font/src/otl.rs` — `chosen_from`;
`gui/font/src/scaled.rs` — the per-run `simple` binding in `shape`.

## §423 — Normalization takes a Hangul policy, so NFC stays NFC and the shaper still sees jamo

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)
**Lane:** C (graphics, apps & net)
**Affects:** `gui/font/src/norm.rs` (`Hangul`, `normalize`, `nfc`, `pieces`,
`split_undrawable`, `needs_work`), `gui/font/src/scaled.rs` (`shape`),
`gui/font/src/hangul.rs`, `gui/font/src/gsub.rs`, `gui/font/src/fallback.rs`

Korean is encoded two ways — 11,172 precomposed syllables, or conjoining
jamo — and the two are canonically equivalent, so Unicode is content either
way. Normalization is not: NFC composes `<L,V,T>` into a syllable, and that
composition destroys precisely the distinction the Hangul shaper reads.
HarfBuzz's Hangul shaper therefore sets
`HB_OT_SHAPE_NORMALIZATION_MODE_NONE` and does its own thing.

We could not simply copy that, because in this crate `pieces`' composition was
the **only** reason Korean rendered at all: the ordinary Korean text font ships
the precomposed syllables and no jamo, so a run of jamo through a
normalization-free path draws three missing-glyph boxes where it used to draw
one correct syllable. Which encoding to *draw* is a question about the face,
not about the text — and that is the observation the decision turns on.

**Three options.**

*Drop Hangul composition from `nfc` outright*, as HarfBuzz's mode name
suggests. Smallest edit, and wrong: `nfc` is also the answer to "what is the
NFC of this string", which is a question about text with a fixed answer.
Bending it to suit one consumer makes a general-purpose function quietly
lie.

*Keep `nfc` and let the shaper undo its work* — compose, then decompose
syllables back to jamo when the face wants them. Preserves every caller, but
it is two passes that cancel, and the round trip is lossy in the cases where
it matters most (a syllable that composed from a jamo sequence the face can
draw is indistinguishable afterwards from one the author typed precomposed).

*Parameterize normalization on a Hangul policy.* A private
`enum Hangul { Normalize, LeaveAlone }` threads through `decompose_once`,
`compose_pair`, `decompose_into` and `compose`; `nfc(text)` passes
`Normalize` and keeps its meaning exactly, while `pieces` — the shaping
entry point — passes `LeaveAlone` and hands jamo to `hangul::preprocess`,
which decides per face whether to compose. Chosen. It costs one enum and
one argument, and it is the only form in which both callers get a true
answer instead of a compromise between them.

Two consequences worth writing down. `nfc` lost its last production caller
in the split — everything in the shaping path now goes through
`normalize(text, LeaveAlone)` — so it carries an explicit
`#[cfg_attr(not(test), allow(dead_code, …))]` rather than being deleted: it
is the text-half of a deliberate split, and deleting it would leave the crate
with no answer to a question it should be able to answer. And `needs_work`
deliberately asks with `Normalize`, the *conservative* direction: it gates a
fast path, so over-reporting work costs time and under-reporting costs
correctness.

The measured result: the HarfBuzz differential sweep went from 892
disagreements to 339 across 556 host faces × 23 strings, with `reordered` and
`misplaced` both staying at 0, and `osfont` from 482 to 501 passing tests
(`hangul.rs`'s own 19 had never run, because an undeclared module compiles
nowhere). The 339 that remain are a different question — font-aware
recomposition of Latin diacritics — filed as `open-questions.md` C-Q1
because it would invert this same layering for a different script.

## §424 — A program may request a default fold depth for its own activity log, as a hint the user overrides

**Date:** 2026-08-14
**Decided by:** Operator (Claude proposed this option and recommended it)

`roadmap-detailed.md` §4.14 gives every process an activity stream whose
`enter`/`exit` and `phase` records form a tree, and specifies that the viewer
folds it: open frames expanded, completed frames folded to a summary line,
failures expanded regardless. That left one question genuinely open — may a
*program* say how its own output should start out folded? An installer would
like to say "start me collapsed to phases."

### The tension

Both sides are real, which is why it was filed as an ambiguity rather than
decided in passing.

**For allowing it.** The program is the only party that knows which of its
nesting levels are semantically meaningful. Depth is a poor proxy for
importance: one program's level 3 is its phase boundary, another's is a
logging helper's inner frame. A viewer choosing purely by depth will collapse
the interesting level of one program and expand the noise of another, and no
global default fixes both.

**Against.** The program is also the party with an incentive to hide its own
noise, and the party most likely to get it wrong — a default fold depth chosen
by a developer watching their own program on their own machine is tuned for
someone who already knows what the levels mean. A hint mechanism is also a
mechanism, and every "the app can influence the system UI" affordance ever
shipped has been abused by somebody.

### What was decided

Allow it, strictly as a default the user's per-app preference overrides. The
constraints are what make this safe, and they are specified rather than left
implied:

* it cannot survive an explicit user setting for that app;
* it cannot hide a frame from expand-all;
* it cannot fold a frame that the failure rules (a frame that failed, or
  contains an error or warning) would expand;
* the viewer indicates when the active fold depth came from the program rather
  than from the user or the global default, so "why is this collapsed" always
  has a visible answer.

The reasoning that settles it: with those constraints, a program that abuses
the hint costs the user **one click, not the data**. The failure mode is
bounded and self-correcting, whereas the failure mode of refusing the hint —
every program's log folding at a depth that is meaningless for that program —
is unbounded and has no recourse at all.

### The alternative that was rejected

Depth-only folding chosen entirely by the viewer, with no program input. It is
simpler and unabusable, and it is what most log viewers do. It was rejected
because it makes the fold depth a property of the *viewer* when the thing being
folded is a property of the *program*, and because §4.14 already establishes
that adoption is the hard part of this feature: a facility that ignores what
instrumented programs tell it gives authors one less reason to instrument.

**Where it bites:** `roadmap-detailed.md` §4.14 → "Grouping and nesting —
subtasks under tasks" (the fold-state bullets) and "Settings" (the per-app
"whether this app's requested default fold depth is honoured" toggle).
Implementation is lane **C**, in the Process Explorer activity pane and the
call-tree view.

---

## §425 — A monospace face is a *scoped render-tree state*, not a field on every text command

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)
**Zone:** gui-toolkit / gui-core / apps

**Context.** The toolkit had no way to ask for a fixed-pitch face at all.
Every `text::measure` call resolved against the one installed UI face, so the
terminal-shaped apps — `apps/tmux` above all — sized their character cell from
`text::digit_advance`, i.e. the advance of `'0'` *in a proportional face*. At
13px that is 7.55px, while `'W'` in the same face measures 13.08px. The grid
was laid out on one width and filled with glyphs of another: characters
overhung their neighbours' cell backgrounds, and the block cursor was drawn
beside the character it was supposed to mark rather than on it. This is a real
rendering bug, not a test artifact, and it cannot be fixed by picking a
different measuring character — a proportional face has no single advance to
pick.

So `osfont` grew a `Family { Ui, Mono }` axis on its cache key, and the
question became how a *caller* names the family it wants, given that the
render tree is the interface between an app and whatever draws it.

**Options.**

1. **A `family` field on `RenderCommand::Text`.** Each string carries its own
   face. Maximally explicit; no state to get out of balance.
2. **Scoped `PushFont { family }` / `PopFont` commands**, following the
   existing `PushClip`/`PopClip` and `PushTranslate`/`PopTranslate` grammar.
   A region of the tree is drawn in a family; commands inside inherit it.

**Decision: (2), scoped.**

**Reasoning.**

- *It matches what the property actually is.* A family is a property of a
  **region** — a terminal pane, a code block, a hex dump — not of each
  individual string inside it. The clip rectangle and the translate are scoped
  for exactly this reason, and a family behaves the same way: it is set once
  on entering a region and restored on leaving.
- *The cost of (1) is paid by everyone and the benefit is collected by almost
  nobody.* The tree contains **4570 `RenderCommand::Text {` construction sites
  across 208 files**. Option (1) puts a new field on every one of them so that
  a handful — one terminal, one editor gutter, one hex view — can set it to
  something other than the default. Even with `..Default::default()` that is
  4570 sites of churn and 4570 places to get it subtly wrong later.
- *Backends that do not implement it degrade correctly.* A renderer that
  ignores `PushFont`/`PopFont` draws everything in the UI face — precisely
  what it did before the commands existed. Option (1) has the same property
  only if the field is `Option`-shaped, which weakens its one advantage.

**Cost we accept.** Scoped state can go unbalanced: a `PushFont` on a path
that returns early leaks the family into whatever is drawn next. The
mitigations are the same ones the clip stack already uses — the compositor
clears its `font_stack` at both ends of `execute()`, so a leak cannot survive
a frame — plus a per-app test. `apps/tmux`'s
`the_grid_is_drawn_in_the_family_it_was_measured_in` walks the command list,
asserts the depth returns to zero, that the scope was opened exactly one deep,
and that cell glyphs were actually drawn *inside* it. That last assertion is
the one that matters: without it the test passes vacuously on a fresh
multiplexer, whose buffer is all spaces and whose cell loop skips them.

**Wire protocol.** `guiremote` encodes the two commands as tags `0x0B`/`0x0C`
with a one-byte `FontFamilyTag`. `PROTOCOL_VERSION` stays at **1**: the change
is purely additive, every frame a v1 encoder could produce still decodes
identically, and the only mismatch — a new encoder against an old decoder —
already fails cleanly with `DecodeError::BadTag` rather than misrendering.

**Fallback semantics.** A family with no installed face falls back to the
built-in 8x16 bitmap face, which is *itself* monospace — never to the other
family's face. This matters because `Mono` is a promise about **metrics**, not
about looks: a caller may treat the text as a grid. Falling back from `Mono` to
a proportional UI face would silently reintroduce the exact bug this entry
exists to fix, so the fallback preserves the promise even when it cannot
preserve the appearance.

**Where it bites:** `gui/font/src/system.rs` (`Family`, the cache key),
`gui/toolkit/src/render.rs` (`FontFamily`, `PushFont`/`PopFont`),
`gui/toolkit/src/text.rs` (`measure_in`, `cell_advance`, `line_height_in`,
`ascent_in`, mono face installation), `gui/remote/src/lib.rs` (tags
`0x0B`/`0x0C`), `gui/compositor/src/main.rs` (`font_stack`), `apps/tmux`.

## §426 — On-disk records store paths percent-encoded from their bytes, behind a format version marker

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)
**Zone:** apps

**Context.** Two lane-C programs keep a record whose whole purpose is to name a
file well enough to reproduce it later: the file explorer's recycle bin
(`meta.txt`, holding the path an entry came from) and the backup program's
manifest (the JSON list of everything in an archive). Both wrote the path as
text — `path.display()` and `to_string_lossy()` respectively — and our paths
are byte strings that allow every byte except `/` and NUL. A name that is not
UTF-8 therefore became U+FFFD *in the record*, and since the record is the only
copy, "restore" recreated the file under a different name while reporting
success. Both programs were self-consistently wrong: they re-read exactly what
they wrote, so nothing downstream could notice.

The fix has to make an arbitrary byte string survive a file that also wants to
stay human-readable and line- or JSON-oriented. Both formats already existed in
the field.

**Options.**

1. **Store the raw bytes.** Write the path verbatim and parse it back. Exact,
   zero encoding logic — but `meta.txt` is line-oriented and a path may contain
   `\n`, and JSON strings are Unicode by definition, so a raw byte string is
   not representable in the manifest at all.
2. **Base64 the path.** Exact and trivially unambiguous, but the record stops
   being readable: the overwhelmingly common case is an ordinary ASCII path,
   and a recycle bin you cannot inspect with a text editor is a real loss when
   the thing you are debugging is "where did my file go".
3. **Percent-encode: escape `%` and every byte outside `0x20..0x7f`.**
   Ordinary paths are unchanged apart from a literal `%`; the record stays
   printable ASCII, single-line, and valid JSON; and the encoding is exact.
4. **Escape only what the container forbids** (newline for `meta.txt`,
   non-UTF-8 for JSON). Minimal diff, but each format gets a different escape
   with different edge cases, and "what the container forbids" is exactly the
   kind of thing that is revisited later and gets it wrong.

**Decision: (3), the same percent-encoding in both formats, each guarded by a
version marker** — line 1 of `meta.txt` is `slate-recycle-v2`, and the manifest
carries `"version": 2`. Absence of the marker means version 1, which is read
with paths verbatim.

**Reasoning.**

- *It is exact where exactness is the point and invisible where it is not.* The
  encoded form of `/home/u/notes.txt` is `/home/u/notes.txt`. Only the rare
  path pays, and it pays with a form that is still readable (`caf%E9.txt`).
- *One escape, two formats.* Both records now have the same failure modes and
  the same tests. Option (4) would have produced two encodings that look alike
  and differ in the corners.
- *The version marker is what makes the change deployable.* Both formats had
  live data. Refusing to read it would strand every existing backup and every
  populated recycle bin; reading it as if it were escaped would silently
  rename any file whose name contains `%20`. The marker is one line and one
  field, and it makes the old data read correctly instead of plausibly.
- *Against (2):* the readability of the common case is worth more than the
  uniformity of the rare one. Against (1): it does not work in JSON, and a
  path with a newline in it is exactly the case the record must survive.

**Consequences.** A decoder must treat a `%` not followed by two hex digits as
a literal `%` rather than dropping it — a hand-edited file must not lose a byte
silently. The lossless part is kept at byte level (`encode_bytes` /
`decode_bytes`) with the `OsStr` conversion as a thin platform layer above it,
because the byte level is the level the file is written at and the only level
that can be asserted on a non-Unix test host.

**Where it bites:** `apps/explorer/src/fileops.rs` (`encode_path`,
`encode_bytes`, `decode_path`, `decode_bytes`, `META_VERSION`, `RecycleBin`),
`apps/backup/src/main.rs` (the same four functions, `MANIFEST_VERSION`,
`FileEntry::to_json`/`from_json`, `relative_path`).


## §427 — Text that does not fit carries an overflow policy on the draw command, and the compositor draws the ellipsis

**Date:** 2026-08-15
**Decided by:** Operator (answering `open-questions.md` Q45 — "q45: a."; Claude
raised the question and recommended A)
**Zone:** gui-core, gui-toolkit, apps

**In short.** When a piece of text is too wide for the space it was given, we
currently just stop drawing it — no "…", no mark of any kind. So a label reading
`Gateway 192.168.1.1 res` looks like a complete sentence rather than a truncated
one, and a reader has no way to tell that anything was cut. The fix is to make
every text-drawing instruction say up front what should happen when the text
does not fit — either cut it silently or end it with "…" — so that the question
can no longer be left unanswered by accident. The cost is that every place in
the codebase that draws text has to be edited to say which it wants.

**Context.** `RenderCommand::Text` carries an optional `max_width`. The
compositor honours it in `draw_text` by walking glyphs and breaking before the
first one that would cross the limit. Nothing is drawn to mark the break. A
caller who wants the cut marked has to call `text::elide` beforehand — which
measures the string to find the cut point, and then the compositor measures it
again while drawing it, answering the same question twice with two
implementations that can disagree.

The result is the failure mode in `known-issues.md` →
`TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED`: well over a hundred single-line labels
across `gui/**` and `apps/**` pass `max_width` without eliding. Most are safe
only because their values are short and app-authored. The ones that bite carry
user or network data — file names, SSIDs, error strings, host names — where a
plausible-looking truncation is indistinguishable from the real value.

**Options.**

1. **`overflow: TextOverflow` (`Clip` | `Ellipsis`) as a field on
   `RenderCommand::Text`; the compositor draws the ellipsis.** *What changes:*
   text cut by `max_width` ends in "…" wherever a caller asks for it, and the
   ellipsis is placed by the party that knows exactly where the glyphs ran out.
   *Cost:* Rust has no per-field default in a struct variant, so this edits
   **every** construction of `Text` in the tree.
2. **A second variant, `RenderCommand::ElidedText`.** *What changes:* the same
   visible outcome, with no edit to existing call sites. *Cost:* every renderer,
   every test and every match on `RenderCommand` splits an arm forever to encode
   one boolean.
3. **A builder — `Text::new(..).ellipsis()`.** *What changes:* the same outcome,
   opt-in. *Cost:* the struct-literal form stays available and stays wrong, so
   the next label someone writes still has the bug.
4. **Sweep `text::elide` across the call sites that need it.** *What changes:*
   today's hundred-odd bad labels get fixed. *Cost:* nothing prevents the
   hundred-and-first, and the double-measurement stays.

**The decision: option 1.** It is the only option that makes the mistake
*unrepresentable* — after it, a `Text` command cannot exist without having
answered "and what if it doesn't fit?". The operator was told the churn was
several hundred sites and chose A anyway, on exactly that ground. (Measured
afterwards: **4517 `RenderCommand::Text {` sites across 208 files**, well above
the estimate. That does not reopen the decision — the decision was about
representability, not diff size — but it does mean the edit is *scripted*, not
made by hand.)

**Execution constraint, and it is load-bearing.** This lands as **its own commit
with nothing else in flight.** A four-thousand-site mechanical diff entangled
with real work cannot be separated afterwards; that is precisely the trap §310
(the repo-wide rustfmt) exists to document, and it cost a revert-and-redo cycle
in `posix` when it happened there.

**A sub-decision left to Claude, recorded here so it can be overruled.** For the
sites that pass `max_width: None`, the choice is vacuous and they get `Clip`.
For the sites that *do* set a `max_width`, the mechanical translation would be
`Clip` — that preserves today's behaviour exactly. It is nonetheless the wrong
default: today's behaviour *is* the reported bug, and a scripted sweep that
faithfully preserves a bug at four thousand sites has done nothing. Those sites
default to **`Ellipsis`**. The consequence is that some labels which currently
fill their box to the last pixel will end in "…" one glyph earlier; that is the
intended change, not a regression. Sites where clipping is genuinely correct —
a progress bar's fill, a decorative rule — are those that should be
individually set back to `Clip` afterwards, because they are the rare case and
can be argued for one at a time.

**Where it lands.** `gui/toolkit/src/render.rs` (`RenderCommand::Text`, the
`RenderTree::text()` helper), `gui/compositor/src/main.rs` (`draw_text` — the
`break` at the limit becomes the place the ellipsis is drawn),
`gui/toolkit/src/text.rs` (`elide` / `elide_start`, which now overlap the
compositor's job and need reconciling rather than deleting — they still serve
callers who need the *string*, not the pixels), and every `max_width: Some(..)`
in `gui/**` and `apps/**`. Closes `known-issues.md` →
`TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED`.

## §428 — Normalization stays font-blind; the font-fitting stage decomposes what the face cannot draw

**Date:** 2026-08-15
**Decided by:** Operator (answering `open-questions.md` C-Q1 — "c-q1: c.";
Claude raised the question and recommended C)
**Zone:** gui-core

**In short.** Some accented letters can be written two ways: as one character
(`ḉ`) or as a plain `c` with two accent marks stacked on it. A font may contain
the pieces but not the single combined character. Today, when that happens, we
draw an empty box — the "missing character" rectangle — where other systems draw
the letter correctly by falling back to the pieces. The question was *which part
of our code should notice*. We chose: the part that already knows what the font
contains, rather than the part that converts text into its canonical spelling.
The visible result is that those letters render correctly; the text-conversion
step keeps knowing nothing about fonts, which is what lets it be tested and
cached on its own.

**Context.** `gui/font/src/norm.rs` is layered on a principle written into its
module doc: **`nfc` answers a question about *text*** — NFC is the Unicode rule
that spells `e` + `´` as the single character `é` — **and never looks at a font;
`fit_to_face` answers a question about the *font*** and does not renormalize.
Composition is a property of the string, so it is decided before any face
(a font file as loaded for rendering) is consulted.

HarfBuzz — the reference text shaper (the library that turns characters into
positioned glyphs), which we run a differential sweep against — does the
opposite. It decomposes to NFD (the fully-separated spelling) and then
*recomposes only where the face has a glyph*, so the same string normalizes
differently in two different fonts.

The question surfaced as the entire residue of that sweep. Fixing
`TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS` took the disagreement count from 892
to 339, and the remaining 339 are **one question asked 339 times**, not a
scatter: `\u1e09` (ḉ, c with cedilla and acute) 255 cases, `\u212b` (Å, the
angstrom sign) 57, `été` 10, and a short tail. Concretely, for `\u1e09` in a
face holding `c`, the cedilla and the acute but no precomposed `ḉ`: we emit one
missing-glyph box, HarfBuzz emits three glyphs that stack into the right-looking
character.

**Options.**

- **A — keep the current layering unchanged.** *What changes:* nothing; we keep
  drawing a box where HarfBuzz draws correct text. *Pro:* each stage has one job
  and one input. *Con:* the user does not care which stage was principled.
- **B — adopt HarfBuzz's font-aware recomposition wholesale.** *What changes:*
  the sweep residue goes to near zero and partial-coverage faces render
  correctly. *Con:* normalization becomes a function of `(text, face)` — no
  longer hoistable out of a loop, no longer cacheable per string, not reasonable
  about without a font in hand; `norm.rs`'s layering claim becomes false.
- **C — a narrow fallback: `nfc` stays pure, but `fit_to_face` decomposes a
  composed character it cannot draw when the pieces *are* drawable.** *What
  changes:* the same visible outcome as B for exactly the failing case, with A's
  layering intact. *Con:* two mechanisms where HarfBuzz has one — we agree with
  it on output while diverging on structure.

**The decision: option C.** The decomposition happens in the stage that already
owns "what can this face draw", and `split_undrawable` already exists with
exactly that shape — which is why C was the recommendation rather than a
compromise between the other two. Expected result: the 339 disagreements move to
`agree` without `nfc` ever taking a face as input.

**The cost accepted, and what to actually test.** Running two mechanisms where
HarfBuzz runs one means we can match its output while diverging on how we got
there, and divergence in structure eventually shows up as divergence in output.
The concrete risk named in the question is **mark reordering after a late
decomposition** — when several accents attach to one letter, their order matters,
and HarfBuzz gets it right by construction because it decomposes before
reordering, whereas we would decompose after. Treat that as the thing to verify
rather than assume: the sweep is the instrument, and any ordering case it
surfaces is this decision's bill coming due, not a surprise.

**Why B is worth keeping written down.** If a future case cannot be fixed inside
`fit_to_face`, B is the argument that has to be beaten, and it should not be
re-litigated from scratch. It was refused for one reason: it makes normalization
depend on the font, and everything we do with normalized text — caching it,
hoisting it out of a render loop, testing it without a font — depends on it not
doing that.

**Where it lands.** `gui/font/src/norm.rs` (`fit_to_face`, `split_undrawable`,
and the module doc's layering paragraph, which now needs a sentence saying the
fallback exists and why it does not violate the principle),
`gui/font/src/scaled.rs::shape` (call order), and
`gui/font/tools/harfbuzz_sweep.py` (the 339 should move to `agree`). Reference:
HarfBuzz `src/hb-ot-shape-normalize.cc`,
`HB_OT_SHAPE_NORMALIZATION_MODE_COMPOSED_DIACRITICS_NO_SHORT_CIRCUIT`.

## §429 — A required field on a shared type is added and filled in one commit, across lane boundaries

**Date:** 2026-08-15
**Decided by:** Claude (autonomous) — lane C

**In short:** The project is worked by three agents who each own a slice of the
tree and are forbidden from editing each other's files. When one of them needs a
change in another's slice, they leave a note in a `requests/` folder and the
other picks it up later. That works when the two halves of the change are
independently valid. It does not work when a shared data structure gains a
field that every user of it *must* fill in: the half that adds the field and the
half that fills it in are each, alone, a codebase that does not compile. There
is no order to do them in. This entry decides that in that specific case the
adding lane fills in the other lanes' sites too, in the same commit, and tells
them afterwards.

### The situation that forced it

§427 added a required `overflow` field to `guitk::render::RenderCommand::Text`.
That type lives in lane C's tree but is *constructed* wherever anything draws
text: 4,517 occurrences across 208 files in lane C's own tree, and 31 more in
`init/login/src/main.rs`, which belongs to lane B. `net/`, `netscan/` and `pkg/`
have none, so that one file is the whole out-of-lane reach. That number had to
be measured before the options below could be weighed at all — the shape of the
fallout is what decides whether this is a request or something else — and it is
what makes option D tractable. Had the reach been thirty files across two lanes,
the answer would be C.

Rust has no per-field default in an enum struct variant. That is not incidental
to §427; it is the whole mechanism the operator chose it for. So:

- Lane C adds the field. `init/login` no longer compiles. Because the boot test
  builds the whole workspace, **`main` is red for every lane** until lane B
  happens to read its dropbox — which, as `roadmap.md` notes from experience,
  can be a day, because `requests/` is a set of files on a branch rather than a
  mailbox, and a request is invisible until the recipient merges.
- Lane B fills the field in first. It cannot: the field does not exist.

Both orderings are red. The two halves are not independently valid, and the
request mechanism can only express changes that are.

### The options

**A. File a request and let the tree stay red in between.** Honest about the
ownership rule and costs nothing to implement. It also means deliberately
pushing a `main` that does not build, for an unbounded period, in a project
whose stated rule is "never merge a red tree to `main` to unblock myself" —
and it blocks the *other* lane too, which had no part in the change.

**B. Add the field with a `Default` so unfilled sites still compile.** Keeps
every commit green and every lane inside its own tree. It also destroys the
point: a defaulted `overflow` means all 4,548 sites silently keep today's
behaviour,
and today's behaviour is the reported bug. The operator considered and rejected
exactly this under §427. Reintroducing it here as a *process* convenience would
be overturning a decision that was not mine to overturn.

**C. Add a second variant / a parallel type, migrate lane by lane, delete the
old one at the end.** Every commit is green and no lane touches another's files.
It is the textbook answer and it is a real option. Against it: it is three
round-trips through the dropbox for a mechanical change, the intermediate state
has two ways to spell the same command (which is its own bug surface), and the
"delete the old one at the end" step is the one that never happens — it depends
on every lane having finished, with nothing failing if it is skipped.

**D. (chosen) The commit that adds a required field to a shared type also fills
in every construction of it, wherever it lives, and notifies the other lane
afterwards via `requests/`.** One green commit, no intermediate dialect, no
dangling cleanup step. The cost is real and is the reason this entry exists:
lane C wrote to lane B's tree, which the ownership rule forbids outright, and
the ownership rule exists to prevent the single most expensive failure in this
arrangement — two agents editing one file and one silently clobbering the other.

### Why D, and what makes it safe

The clobber risk is not a constant; it is a function of whether the other lane
has work in flight in that file. That is measurable, so it was measured before
the decision rather than assumed: `origin/lane-b` was **0 commits ahead of
`origin/main`** — nothing in flight anywhere in lane B — and
`init/login/src/main.rs` had last been touched only by a repo-wide rename. The
risk the rule guards against was, at that moment, nil.

So D is conditional, and the conditions are the decision:

1. **The change must be mechanical.** A rule stated in one line, applied
   uniformly, with no judgement about the other lane's screens. Here:
   `max_width: Some(..)` → `Ellipsis`, `max_width: None` → `Clip`.
2. **The other lane's branch must be at or behind `main` in the affected
   files** — checked with `git log origin/lane-<x> --not origin/main -- <path>`,
   not assumed. If they have work in flight, this is off the table and the
   answer reverts to C.
3. **It must be scripted, and the script committed**, so the other lane can read
   precisely what was done to their file and re-run it to confirm it is a fixed
   point. (`scripts/q45_apply.py`.)
4. **A `requests/<mine>-<theirs>-*.md` must be filed in the same commit**,
   stating what was changed, by what rule, and that they may freely correct any
   site without asking — it is their file and they know what those screens are
   for.

If any of the four fails, C is the fallback, and the extra round-trips are the
price of not writing to someone else's tree.

### What this does not license

It is not a general exemption from the ownership rule. It covers exactly the
case where a change is **atomic by the type system** — the compiler will not
accept either half alone. Anything a lane could plausibly do in two green
commits still goes through `requests/` and waits. In particular, "it would be
faster if I just did it" is not this rule; the trigger is "there is no ordering
of commits that compiles", which is a fact about the change and not a judgement
about the schedule.

### Where it bites

`scripts/q45_apply.py` (the `ROOTS` list includes `init`, with a comment
pointing here), `requests/c-b-render-text-gained-a-required-field.md`, and
`roadmap.md` → "Three-Agent Parallel Execution", whose `requests/` protocol this
entry qualifies. The next required field on a shared type will hit the same wall;
this is the answer for it.

## §430 — A language is a *list* of OpenType tags generated from HarfBuzz, and the first one the font registers wins

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** A font can hold rules that apply to one language and not another
written in the same alphabet — Turkish spells the lowercase of `I` as a dotless
`ı`, Romanian wants a comma under `ș` rather than a cedilla. To reach those
rules the shaper has to turn what the caller knows ("this text is Turkish",
written `tr`) into the four-letter code the font filed them under (`TRK `).
There is no rule that derives one from the other: it is a lookup table of about
eleven hundred entries that Microsoft maintains. Three things were decided
here — where that table comes from, what to do when one language maps to
several codes, and what to store per font.

### The decisions

**1. The table is generated from HarfBuzz's source, not written by hand.**
`gui/font/tools/gen_lang_tables.py` parses HarfBuzz's `hb-ot-tag-table.hh` and
`hb-ot-tag.cc` and emits `gui/font/src/lang_tables.rs` (148 complex rules, 188
two-letter keys, 916 three-letter keys, 162 blocked codes). A registry update
is a regeneration, not an edit.

*Alternative:* transcribe the Microsoft registry by hand, or write the mapping
as code. Both were rejected for the same reason: the crate measures itself
against HarfBuzz with `tools/harfbuzz_sweep.py`, and a table that disagrees
with HarfBuzz's turns every sweep difference into an argument about whose
registry is right instead of a bug report. Taking the data from the shaper we
compare against makes any remaining difference *ours*.

*Cost:* the generator is coupled to the layout of two HarfBuzz source files and
will break when they are restructured. It is written to fail loudly rather than
silently emit a short table — it checks HarfBuzz's own stated run lengths, and
refuses on a key collision between two of the tables — so a break is a stopped
run and not a wrong answer.

**2. One BCP 47 tag maps to up to three OpenType tags, tried in order, and the
first the *font registers* wins.** `ro-MD` is `MOL ` then `ROM `; `ml` is
Malayalam Traditional then Reformed; `ga` is `IRI ` then `IRT `. The cap of
three is HarfBuzz's `HB_OT_MAX_TAGS_PER_LANGUAGE`.

*Alternative — and this is what the first version of the fix did:* keep only
the first tag of each list, on the reasoning that a language has one code and
the rest are historical spellings. That is wrong, and the HarfBuzz sweep proved
it within one run: 66 of this host's 556 faces (`Candara.ttf` among them)
register `('latn', 'ROM ')` and no `MOL ` at all, so Romanian's comma-below
reached Moldavian in HarfBuzz and not in us. The `ro-MD` disagreement bucket
was 345 against plain `ro`'s 279; after the rework it is 279, exactly. The
candidates are not synonyms — they are an ordered search, and a font gets to
answer at whichever spelling it chose.

*Why cap at three rather than keep every candidate:* HarfBuzz truncates there,
and a fourth candidate we honoured and HarfBuzz did not would be a divergence
in the one place the two engines are meant to agree exactly. The cap is also
the widest run the registry actually contains, so today it truncates nothing.

**3. `ByScript` stores a language's lookup selection only when it differs from
its script's default — but stores the language's *tag* either way.** Two thirds
of the 3031 LangSysRecords on this host select exactly what their script's
default does; storing those would be storing a second copy of an answer already
present.

The second half of that sentence is the subtle part, and it is forced by
decision 2. "Which candidate wins" must be decided by what the font
**registers**, never by what happened to be worth storing — otherwise a face
that registers `MOL ` and gives it no rules of its own would fall through to
`ROM ` and apply Romanian's overrides to Moldavian, on the strength of an
optimisation. So `ByScript` carries a second sorted list of every (script,
language) pair the face names, at 8 bytes each (~5 per face here), and consults
that to choose the candidate before looking up what it selects.
`gsub::tests::the_first_candidate_a_face_registers_wins_even_when_it_selects_nothing`
is the regression guard; mutating `selection` to search the stored selections
directly fails exactly that test and nothing else.

### If this is ever revisited

The thing to preserve is the invariant that the mapping is *HarfBuzz's*, not a
reasonable approximation of it. Every one of the three decisions above bends
toward that, and the one time it was bent away from — keeping the head of each
candidate list — cost a wrong answer on 12% of the host's fonts that 521 green
unit tests could not see.

**Where:** `gui/font/src/lang.rs`, `gui/font/src/lang_tables.rs` (generated),
`gui/font/tools/gen_lang_tables.py`, `gui/font/src/otl.rs`
(`select`, `ByScript::parse`, `ByScript::selection`),
`known-issues.md` → `TD-FONT-IGNORES-LANGSYS-OVERRIDES`.

## §431 — When the host cannot falsify a shaping pass, synthesize a font that can

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** Everything the font crate does is checked by shaping the same
string with our code and with HarfBuzz (the reference text-shaping library
every browser uses) over all 556 fonts installed on this machine, and
comparing. That works only while some installed font actually exercises the
code being checked. For the Thai fallback below, *none* does — so the sweep
would have printed "agree" for a pass that never ran, which is worse than no
test at all. The decision: when no installed font can disprove a pass, build
one that can, with fontTools, and check it in.

### The problem, concretely

A Thai font that predates OpenType ships the shifted forms of its tone marks
(a tone mark moves down and left when it has to clear a tall consonant) as
extra glyphs in the *private use area* — the block of codepoints Unicode
reserves for "whatever the font vendor wants" — at U+F700 for Windows and
U+F880 for the Mac. Picking those glyphs is the shaping engine's job, and the
engine only does it when the font contains no Thai layout rules of its own.

Every Thai font Windows ships today *does* contain its own layout rules, which
turns the fallback off. A direct probe confirmed it: of the 556 faces
installed here, **zero** carry a single one of those private-use glyphs. So
the fallback was unfalsifiable against the host collection — the sweep asks
each face for glyphs, both engines answer with the same missing-glyph boxes,
and the report says the two agree.

### The decision

`gui/font/tools/gen_thai_legacy.py` builds three faces with fontTools that do
not exist on this machine and could not: the Thai block, the private-use
block, and deliberately **no `GSUB`/`GPOS`/`GDEF` at all**. That absence is
the whole point — it is what makes both engines take their fallback path over
the same face, so the two answers become comparable.

Three faces, and the third is as load-bearing as the first two:

| face | holds | what it proves |
|---|---|---|
| `ThaiLegacyWin` | Windows private-use forms only | the fallback fires and picks the right form |
| `ThaiLegacyMac` | Mac forms only | the vendor preference is *tested*, not assumed |
| `ThaiNoPua` | Thai glyphs, no private-use forms | the pass runs, finds nothing, and damages nothing |

`ThaiNoPua` is the case every real modern face without Thai layout rules is
in. A fallback that mangled *that* would mangle a lot of real fonts, and no
amount of agreement on the first two faces would have caught it.

Result: 78 of 78 strings agree with HarfBuzz across the three faces. The
corpus (`gui/font/tools/thai-pua-corpus.txt`) names one string per edge of the
two state machines rather than sampling real text, since a synthetic font's
only purpose is to reach edges real text reaches rarely.

*What it cost:* two generated files and a fontTools dependency for the test
tooling (not for the crate). *Alternative rejected:* hand-write unit tests
asserting specific glyph ids. Those assert what I believe HarfBuzz does; the
synthetic face asks HarfBuzz. When the two disagree, only the second one
tells me which of us is wrong — and it did, immediately, by catching that the
pass was gated on the wrong predicate and never fired at all.

**Generalization.** This is now the rule for the three shapers still to come
(Khmer, Myanmar, USE) and for anything else the host cannot exercise: if the
sweep cannot disagree with a pass, the pass is untested, and the fix is a
font that can disagree — not a weaker claim in the commit message.
`harfbuzz_sweep.py` grew a `--corpus FILE` flag so each such oracle brings its
own strings instead of bloating the built-in corpus.

**Where:** `gui/font/tools/gen_thai_legacy.py`,
`gui/font/tools/thai-pua-corpus.txt`, `gui/font/tools/harfbuzz_sweep.py`
(`--fonts`, `--corpus`), `gui/font/src/thai.rs` (`pua_shape`),
`known-issues.md` → `TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`.

## §432 — The Thai SARA AM pass runs between decomposition and the mark sort, not after normalization

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** Thai has one letter, SARA AM, that is drawn as two separate
marks in two different places — a small circle above the consonant and a
stroke after it. Unicode never says so, so we were asking fonts for a single
glyph almost none of them have. Splitting it is straightforward; *when* to
split it is the decision, because the shaper also sorts marks into a standard
order, and doing these two things in the wrong order draws the circle on the
wrong side of a tone mark.

### The two candidate placements

The crate already has a precedent for a script-specific rewrite of the
character sequence: the Korean pass, which runs *after* normalization is
completely finished. Copying that shape would have been the tidy choice.

It is wrong here, and the proof is a trace rather than an argument. Take
`<0E14, 0E4B, 0E38, 0E33>` — a consonant, a tone mark, a below-base vowel, and
SARA AM:

| order | result |
|---|---|
| split first, then sort (HarfBuzz) | `<0E14, 0E38, 0E4B, 0E4D, 0E32>` |
| sort first, then split | `<0E14, 0E38, 0E4D, 0E4B, 0E32>` |

Same five characters, and the circle (`0E4D`) lands on the opposite side of
the tone mark (`0E4B`). Only the first matches HarfBuzz, and the sweep sees
the difference as a different glyph order.

So the pass is a parameter of the normalizer (`SaraAm::Decompose` /
`LeaveAlone`), invoked between decomposition and the sort. It is safe there
because no character decomposes to or from a Thai or Lao one, so the pass
cannot see a half-decomposed sequence and cannot create work for the
decomposer. `norm::nfc` passes `LeaveAlone`, so plain NFC stays exactly NFC —
the pass is shaping, not normalization, and only the shaping entry point asks
for it.

### Why it cannot be a combining-class table instead

The obvious-looking simplification — express the reordering as combining
classes and let the existing sort do it — does not work, and the reason is
worth recording so nobody tries it later. The circle this pass produces has to
move back over above-base marks. A circle the *user typed* (U+0E4D directly)
must not move at all. They are the same character; only their provenance
differs, and a combining class is a property of a character. HarfBuzz has the
same constraint and solves it the same way, by doing the move at the moment
of splitting, while the provenance is still known.

*Cost of the choice:* `norm::normalize` gained a third parameter and a
script-specific call, which is a small dent in its generality. *Measured
benefit:* host sweep agreement 18806 → 21015 and differences 3382 → 1176,
with every Thai and Lao string agreeing on all 556 faces.

**Where:** `gui/font/src/thai.rs` (`preprocess`), `gui/font/src/norm.rs`
(`normalize`, `SaraAm`, `pieces`), `gui/font/src/hangul.rs` (the contrasting
precedent), `known-issues.md` →
`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`.

## §433 — A feature belongs to exactly one stage, enforced where the stages are built rather than where they are applied

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** A font's shaping rules are grouped into named "features"
(`ccmp` composes characters, `blwf` picks the below-the-line form of a letter,
and so on), and the shaper runs them in a fixed sequence of stages. Both the
Indic and the Khmer shaper were listing some features in an early stage and
then *again* in a final catch-all stage, so those features ran twice. Usually
that changes nothing — running a rule on its own output is normally a no-op —
but it is not what HarfBuzz does, and the second application can rewrite a
glyph the first one already produced. The decision is where to fix it: in the
code that applies stages, or in the code that builds them.

### How it was found, and why that matters

Not by reading. The duplication had been in `indic_shape.rs` since that shaper
was written, and every one of the 556 faces installed on this host agreed with
HarfBuzz anyway, because a real font's lookups are overwhelmingly idempotent:
substituting a below-base form for a glyph that is already a below-base form
matches nothing the second time. It took the Khmer probe font of §431 — where
every feature's lookup appends a marker glyph, and so is *deliberately* not
idempotent — to make the second application visible, as a doubled marker.

That is the §431 argument arriving a second time from a different direction:
the host had been reporting agreement about a code path it could not exercise,
and the bug it hid was not in the shaper being written but in one that had
been shipping for weeks.

### The two places it could be fixed

**In `gsub::apply_stages`** — track which feature bits have already run and
mask them out of later stages. This is closest to what HarfBuzz literally
does: `hb_ot_map_builder_t::compile` sorts the collected features by tag,
merges duplicates, and keeps `hb_min` of the two stage numbers.

**In each shaper's stage list** — build the stages so they are disjoint by
construction, by masking the earlier stages out of the final catch-all.

The second was chosen, for two reasons that pull the same way:

* **`apply_stages` has a legitimate reason to run the same lookup twice.**
  Deduplication belongs to *features*, not lookups. Two different features
  routinely point at one lookup, and when they are in different stages that
  lookup genuinely must run twice, with different masks — that is how a
  feature that is on for one syllable and off for the next is expressed at
  all. A dedup living in `apply_stages` would be one refactor away from being
  written against lookups instead of features, and that version would be
  silently wrong in a way no host face would reveal either.
* **A shaper that names a feature twice has a bug in its stage list**, and the
  stage list is where a reader looks to answer "when does `blwf` run?". Fixing
  it downstream leaves the wrong answer written in the place people read.

So `khmer.rs` and `indic_shape.rs` each grew a `stages()` function returning
the stage masks, with the final stage computed as
`ALL_FEATURES & !already_staged & !liga`. Extracting it from `shape` was the
point of the exercise rather than tidiness: a `stages()` that returns a value
can be asserted about, and four tests now pin the invariant — the stages are
pairwise disjoint, their union is every feature except `liga`, the first stage
is exactly the basic set, and the global features fall in the last stage only.
A comment saying the same thing is deletable by anyone who thinks it is
redundant.

*Cost of the choice:* the invariant is restated once per shaper rather than
enforced once centrally, so a fourth shaper can reintroduce it. That is what
the tests are for, and a shaper without the corresponding test is a shaper
whose stage list nobody checked. *Benefit:* `apply_stages` keeps the ability
to run one lookup under two features, which the alternative would have put at
risk, and the Khmer probe went from **1 agreement of 45 to 43** — measured by
putting the duplication back and re-running, not estimated. That figure is the
other half of the point: against a face that can object, a feature applied
twice is not a subtle difference, it is almost every string. Against the 556
faces installed here it was none of them.

**Where:** `gui/font/src/khmer.rs` (`stages`, `shape`, and the two stage
tests), `gui/font/src/indic_shape.rs` (the same four), `gui/font/src/gsub.rs`
(`apply_stages`, deliberately unchanged), `gui/font/tools/gen_khmer_probe.py`
(the oracle that exposed it), `design-decisions.md` §431.

## §434 — What a lookup wants at a position travels *into* the skip walk, and the never-drawn characters get a third answer

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** Some characters are typed to instruct the text engine and are
never meant to appear on screen — the soft hyphen you put in a long word to
say "you may break it here", the zero-width joiner that asks two letters to be
drawn as one. Until now every one of them acted as a wall: the font's rule for
turning `f` and `i` into a single `fi` shape could not see past it, so writing
`f`, joiner, `i` — which is literally a request for that shape — produced the
opposite of what was asked. The fix is to let a rule *step over* such a
character. The decision recorded here is where the "is this the character I
wanted?" test has to live for that to work: inside the walk, not after it.

### The problem the obvious design cannot express

A lookup asks the run "give me the next glyph I may look at". Before this
change that question had two answers: **hidden** (the lookup's own flags say
to pass over marks, or ligatures, or a named set) and **not hidden**. A
never-drawn character needs a third: *step over it unless it is exactly what
the rule asked for at this position.* HarfBuzz calls this `SKIP_MAYBE`, and
resolves it against the rule's criterion — `matcher_t::may_match` — in the
same loop iteration as the skip test.

Try to keep the criterion outside the walk and it cannot be expressed at all:

```rust
let pos = skip.next(glyphs, at)?;        // walk returns a position
if glyphs[pos].gid != want { return None }   // caller judges it
```

When the caller rejects a glyph it has no way to say *carry on* rather than
*fail* — and "carry on" is the correct answer for precisely the ignorable
case, while "fail" is correct for every other. The walk is the only place that
still knows which of the two the position was. A caller that re-entered the
walk from `pos + 1` would get the other error: it would hop over an ordinary
glyph that failed the criterion, and ligate across a letter.

### The decision

`Skipper::scan` — one private loop — takes the criterion as
`Option<impl FnMut(usize) -> bool>` and returns the first position that both
the flags admit and the criterion accepts, stopping at the first admitted
position that is neither ignorable nor accepted. `next`/`prev` pass `None`,
which reproduces HarfBuzz's `MATCH_MAYBE`: no criterion means an ignorable is
always stepped over. `next_matching` passes the ligature component. And
`walk_forward`/`walk_backward` pass the caller's existing `each` closure —
which already returned `Option<()>` and already deferred its side effects
until after the match succeeded, so the criterion was in the API all along and
only needed routing one level deeper.

*Cost accepted:* `each` now has a load-bearing contract — it may be called at
positions that are not part of the final match, so it must record nothing
before it returns `Some`. That is written on `walk_forward`, and every
existing caller in `context.rs` already honoured it, which is the evidence
that the contract fits the callers rather than being imposed on them.

### The three-way table, and why the joiners are not symmetric

`Joiners` carries which kind of lookup is asking. The answers differ per kind
of character:

| | `GSUB` input | `GSUB` context | `GPOS` |
|---|---|---|---|
| ZWNJ | never | if `auto_zwnj` | always |
| ZWJ | if `auto_zwj` | always | always |
| hidden (CGJ, Mongolian FVS, tags) | never | never | always |
| everything else ignorable | always | always | always |

ZWJ is stepped over and ZWNJ is not, because that asymmetry *is* what the two
characters mean: ZWJ asks for the ligature, so the walk looking for one should
reach past it; ZWNJ forbids it, so the same walk must stop dead. Reading the
row the other way makes `f ZWNJ i` ligate, which is exactly what the writer
typed the character to prevent.

`auto_zwnj`/`auto_zwj` go *off* for features whose own subject is the joiners
— the Indic and Khmer basic features, where a joiner selects between a
conjunct and a half-form rather than decorating a ligature. This is
HarfBuzz's `F_MANUAL_JOINERS`, and it had to be modelled rather than
hard-coded per shaper, because one lookup can be reached by two features:
`manual_joiners` is a mask on the plan, and a lookup whose feature mask
intersects it is manual. Merging that way (manual wins) is the direction
HarfBuzz's `hb_ot_map_builder_t::compile` merges it (`auto_zwj &=`). Note that
`locl` and `ccmp` are *not* in the manual set for either shaper — they are
enabled with `F_PER_SYLLABLE` alone — and that is observable: it decides
whether a `ccmp` ligature may form across a ZWJ.

### The divergence from HarfBuzz that is kept

After this, the host sweep's `misplaced` count is 170, and all 170 are corpus
strings containing an ignorable. In every one, the glyphs agree and every
*visible* glyph's position agrees; what differs is the x of the erased,
zero-advance glyph itself.

The cause is where a legacy `kern` table's adjustment is charged. HarfBuzz
puts it on the right-hand glyph, as both an advance and an x-offset; we charge
it to the pair's left glyph. For adjacent glyphs the two are indistinguishable
— same drawn positions, same total width. They separate only when a
zero-advance glyph stands between the pair, which is exactly the erased
ignorable. For `a` CGJ `b` in Arial Rounded, HarfBuzz's erased glyph sits at
the *unkerned* pen, 1203, while `b` is drawn at 1190 — 13 units inside the
following letter's image. Ours sits at 1190, where the next glyph actually
starts.

Ours is kept. The x of an invisible zero-advance glyph is good for one thing
— placing a caret on that character's cluster — and for that, "where the next
glyph is drawn" is the right answer and "inside the next glyph" is not.
Matching HarfBuzz here would also mean adopting its representation of a kern
throughout, which fights `ShapedGlyph`'s own model, where `advance` and
`kern_next` deliberately describe the gap *after* a glyph.

*Cost accepted:* the sweep will report 170 `misplaced` forever, and a future
reader must not take that as a regression. That is what this section and the
`known-issues.md` entry are for; if the number moves, or a string that is not
in the ignorable set appears in that list, something real broke.

**Measured.** Host sweep, 556 faces × 60 strings: `differ` on `f\u200di` from
76 faces to 0, `misplaced` from 331 to 170. Khmer probe: 45/45 before and
after — the Indic-family features read the joiners themselves and had to come
through untouched, and that they did is the check on `manual_joiners`.

**Where:** `gui/font/src/skip.rs` (`Joiners`, `steps_over`, `scan`,
`next_matching`, and the walk pair), `gui/font/src/norm.rs` (`Ignorable`, the
five-way classification), `gui/font/src/gsub.rs` (`Staging`, `Ctx`,
`skipper`, `ligature_matches`), `gui/font/src/scaled.rs` (the kerning
transparency of an erased glyph), `gui/font/src/gpos.rs` and `kern.rs` (the
`POSITIONING` call sites), `gui/font/src/khmer.rs` and `indic_shape.rs`
(`manual_joiners`), `known-issues.md`
(`TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES`).

## §435 — Mark-advance zeroing is a three-valued question, not a boolean, because *when* it happens changes the width

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** A combining mark — an accent, a vowel sign, a dot under a
letter — is drawn on top of the letter before it, so it must not push the next
letter along; its "advance" (the distance the pen moves after drawing it) is
set to zero. We used to treat that as a yes/no property of the script. It is
not: for Myanmar the zeroing has to happen *before* the font's positioning
rules run rather than after, and doing it in the wrong order slides every glyph
after a medial-RA hook 440 units to the left. So the answer is now one of
three — never, before, or after — and Myanmar is the one script that says
"before".

**The alternatives.**

| | *What changes* |
|---|---|
| **Boolean, zero after `GPOS`** (what we had) | Myanmar text in `mmrtext.ttf` draws with everything after a medial RA shifted a third of an em to the left. |
| **Boolean, zero before `GPOS`** | Every other script loses the `GPOS` adjustments that were meant to be discarded — the mark keeps whatever a lookup charged it. |
| **Tri-state `Zeroing { Never, BeforeGpos, AfterGpos }`** (chosen) | Each script gets the order HarfBuzz gives it, and the two call sites (`zeroes_marks_first`, `zeroes_marks`) each ask the half of the question they need. |

**Why it is not a refinement.** The distinction is invisible in a face whose
marks are all zero-width in `hmtx`, which is most of them, and decisive in one
whose are not. `mmrtext.ttf` classes U+103C — the hook drawn under and around
its consonant — as a `GDEF` mark *and* gives it a 440-unit advance, then charges
that 440 back on with a `dist` feature. Zero afterwards and the `dist`
adjustment is thrown away with it; zero first and the lookup's own number
survives, which is what HarfBuzz prints. That is the whole of
`HB_OT_SHAPE_ZERO_WIDTH_MARKS_BY_GDEF_EARLY`, which of HarfBuzz's nine shapers
only Myanmar and USE set.

**Cost accepted.** Two predicates on `ScaledFont` where there was one, and a
tri-state whose third arm currently has exactly one member. That is the right
shape anyway: USE is the other `BY_GDEF_EARLY` shaper, so its tags join `mym2`
in the `BeforeGpos` arm the moment it is written, and a boolean would have had
to be widened then regardless.

**If it is never revisited:** nothing degrades. The arms are transcribed from
HarfBuzz's shaper table, not guessed, and the sweep pins them.

**Where:** `gui/font/src/fallback.rs` (`Zeroing`, `zeroes_mark_advances`,
`NO_ZERO_WIDTH_MARKS`), `gui/font/src/scaled.rs` (`zeroes_marks_first`,
`zeroes_marks`), `gui/font/src/gpos.rs` (`Run::zero_marks_first`),
`known-issues.md` (`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`).

## §436 — The two mark-zeroing routes are modelled separately, and the fallback owns the marks it places

**Date:** 2026-08-15
**Decided by:** Claude (autonomous)

**In short:** There are two entirely separate reasons a combining mark's width
gets set to zero, and we had them merged into one. One reason is "the font, or
failing that the character's Unicode category, says this glyph is a mark". The
other is "we could not find positioning instructions in the font, so we are
measuring the mark onto the letter ourselves, and a mark we place ourselves
must not also take up room". Merging them meant marks lost their width in cases
where they should have kept it, and in `ကို့` the dot below was drawn two
letters to the right of where it belongs.

**What HarfBuzz actually does, and what we did.**

*Route 1* — `zero_mark_widths_by_gdef`. Gated on a per-script flag
(`plan->zero_marks`, off for eleven scripts). Zeroes every glyph whose `GDEF`
class is mark — **or**, only when the face declares no `GDEF` classes at all,
every glyph whose Unicode general category is `Mn`. `hb_synthesize_glyph_classes`
runs behind `if (!hb_ot_layout_has_glyph_classes(...))`; it is an either/or.

*Route 2* — `_hb_ot_shape_fallback_mark_position`. Zeroes only the marks it
actually places (combining class ≠ 0), plus every `Mn` in the cluster when the
base glyph has no ink to measure against.

We had (a) the two `||`-ed together, so a face that classifies its glyphs had
the character's category second-guessing it, and (b) no per-script gate on
route 1 at all.

**The options.**

| | *What changes* |
|---|---|
| **Keep the union, patch the divergences as they surface** | Each newly-measured face needs another special case; the `simsun.ttc` shift stays until someone notices the next one. |
| **Model the two routes separately** (chosen) | A face that classifies its glyphs is believed; a face that does not falls back to categories; and the measuring pass zeroes only what it places. |

**The structural consequence, which is the part worth remembering.** Route 2
cannot be a single pass. HarfBuzz's `position_around_base` computes each mark's
horizontal offset as an accumulation over the advances *after* zeroing, so
`pens[base] - pens[i]` is only equal to it if the zeroing has already happened.
`synthesize_marks` is therefore **two phases**: walk the clusters and zero, then
compute the pens, then place. It is also where the class-zero bug lived — a mark
with combining class 0 is neither placed nor zeroed, so the old single-phase
splitter mistook it for a base and restarted the cluster on it.

**And "owning" the marks.** `SubGlyph::mark` is derived only for runs the
measuring fallback owns — not for runs `GPOS` reaches, and not for the complex
scripts whose shapers decline the fallback outright (Myanmar's does; its marks
are placed by `GPOS` or not at all). `Role::Base`/`Role::Mark(class)` carries
the answer, so a mark in a `GPOS`-owned run is deliberately `Role::Base` here.
That is not a lie about the character; it is the statement that this pass has no
business touching it.

**Known gap, believed unreachable.** Our cluster splitter reads `Mn` where
HarfBuzz reads `Mn|Mc|Me` — see `known-issues.md`. It needs a spacing matra
followed by a non-zero-class mark in one cluster, which canonical ordering does
not produce.

**Measured.** All twelve `shape_dump` probe cases byte-identical to HarfBuzz,
including `simsun.ttc` index 1 (`0;-128;0` → HarfBuzz's `0;-640;0`); Myanmar
sweep 58/58; full sweep back to its recorded `misplaced 170` baseline with
`agree` up by 19 and no new divergence anywhere else.

**If it is never revisited:** nothing degrades; this *is* the revisit.

**Where:** `gui/font/src/scaled.rs` (`Role`, `zeroed_at`, `places_marks`,
`zeroes_marks`, `synthesize_marks`, `hide_ignorables`),
`gui/font/src/fallback.rs` (`positions_marks`, `attach_class`, `place`),
`gui/font/src/norm.rs` (`is_mark`), `gui/font/src/gpos.rs` (`Run::marks`),
`known-issues.md` (`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`).

---

## §437 — The shared documents are lane-*partitioned*, not append-only, because append-only does not prevent the conflict it was adopted to prevent

**Date:** 2026-08-16
**Decided by:** Operator (operator's own challenge to the rule; Claude proposed this replacement)

**In short:** Four documents that all three lanes write — the bug list, the
open-questions queue, this file, and `todo.txt` — carried a rule saying you
may only *add* to them, never change or delete a line. That rule was adopted
to stop three agents' edits colliding when they merge their work together.
It does not do that, and it made the files worse to read: answered questions
piled up at the top of the questions file where they were most in the way,
and a bug that had been fixed could not say so. The rule is replaced with:
each lane owns a *region* of each file and may do anything it likes inside
its own region, including deleting.

**The rule that was there.** "Shared documents are append-only, with
per-lane sections": new entries go at the very end, prefixed with your lane
letter; editing an existing entry is allowed only if its heading carries
your letter.

**Why it does not work.** Append-only was a proxy for the actual goal —
*three lanes writing one file must never produce a merge conflict* — and it
is a proxy that fails in the ordinary case. Git merges by line region. Two
lanes that both append at end-of-file are writing the same region, so
append-only produces the conflict rather than preventing it. This is
measured, not predicted: merging `origin/main` into `lane-c` on 2026-08-16
conflicted in *this file*, because lane A had appended §203 at EOF and lane
C had appended §435 and §436 at EOF. Both lanes had followed the rule
exactly.

The per-lane numbering (A §200–299, B §300–399, C §400–499) was believed to
be what made this file merge cleanly. It is not — or rather, it would be, if
the file were ordered by number. It is ordered *chronologically*: §424 sits
between §308 and §309. Numbering that does not correspond to physical
position partitions nothing. `CLAUDE.md`'s claim that the numbering
"auto-merged across a 72-commit divergence with zero conflicts" describes a
window in which only one lane happened to append.

**What actually prevents conflicts: partitioning.** If each lane writes a
different *region* of the file, two lanes' edits land at different line
offsets and git merges them without inspecting content. Partitioning
subsumes append-only — appending is just the special case where the region
is "the end" — and it is strictly stronger, because it also covers two lanes
appending at once, which append-only does not.

And partitioning permits, at no cost to the goal, everything append-only
forbade: in-place edits, status stamps, restructuring, and **deletion**
within your own region.

**What the old rule cost, concretely.**

| Symptom | Measured 2026-08-16 |
|---|---|
| `open-questions.md` is cluttered | 7 answered questions occupy lines 52–219; the 3 actually-open ones begin at line 220. The file's entire purpose is to be scanned for open questions, and they are last. |
| The rule is not followed anyway | All 7 answered entries are stamped `Status: **RESOLVED**` *in place*, which append-only forbids. |
| The rule contradicted `CLAUDE.md` | `os/CLAUDE.md` says an answered question moves to `design-decisions.md` and is *removed* from `open-questions.md`; `roadmap.md` rule 3 said "Append only." Agents followed the useful one, so the written rule had already lost. |
| The lane-letter carve-out is undecidable | `known-issues.md` has 999 headings; **34** carry a lane letter. For the other ~965, "editing is allowed only if the heading carries your lane letter" answers *no* for every lane — including whoever fixes the bug. |

That last row is the one that matters most. A bug list that structurally
cannot record "this is fixed" degrades into a list of things that *were once*
wrong, which is the one thing a bug list must not be. 518 of those entries
say FIXED — all of them written in violation of the rule, and all of them
correct to have been written.

**The alternative that was rejected: keep append-only and add a separate
resolution log.** Status would live in a second file keyed by entry ID.
This preserves append-only literally, and it was rejected because it doubles
the number of places a reader must look while making neither of them
authoritative — and it still conflicts, since the resolution log is itself
appended to by all three lanes at EOF. It satisfies the letter of the rule
and none of its purpose.

**The one genuinely cross-lane edit, and why it is allowed.** Any lane may
add or update a single `**Status:**` line at a fixed position under any
`known-issues.md` heading, without a request. It is a one-line edit at a
known offset, so a collision is trivial to resolve; the alternative is that
an issue you fixed stays open forever in the file whose job is knowing what
is open; and two lanes stamping the same entry is *information* — two lanes
believed they fixed the same bug — that should be surfaced rather than
designed away.

**If it is never revisited:** the documents stay readable and the merge
behaviour strictly improves, since numeric ordering in this file turns three
EOF-appenders into three disjoint insertion points.

**Where:** `roadmap.md` → "Three-Agent Parallel Execution" rule 3;
`open-questions.md` (`## Resolved` index); `deferred-questions.md`
(`## Closed` index); `known-issues.md` and the new
`known-issues-resolved.md`; this file's numeric ordering.

---

## §438 — The font engine — own the bytes, reject CFF loudly, no hinting, signed-area rasterization

**Date:** 2026-08-13

**Decided by:** Claude (autonomous)

**Context.** Nothing in the tree could open a `.ttf`. The entire UI — desktop,
toolkit, compositor, every app — was capped at one procedurally generated 8x16
bitmap face, which means one size, no anti-aliasing, and no script outside the
handful of ranges that face was written to cover. The `[C]` roadmap item "2D
drawing library: Vello + HarfBuzz" has a GPU-dependent half (blocked on lane
A's DRM/KMS and GPU driver) and a GPU-independent half — reading font files and
turning glyphs into coverage — which is not blocked on anything. That half is
`gui/font/src/{sfnt,raster,scaled}.rs`. Four decisions inside it had real
alternatives.

### 86a. `Face` owns its bytes rather than borrowing them

**Decision.** `Face::parse(data: Vec<u8>)` takes ownership; the table spans are
offsets into the owned buffer, not `&[u8]` slices.

**Rationale.** The natural callers — a font cache, a `ScaledFont`, a per-display
font set — all outlive whatever read the file. A borrowing parser would push a
lifetime parameter into every one of them, and into every struct that holds one,
for no benefit: nobody has a font's bytes already resident and wants to avoid the
copy.

**Alternative considered.** `Face<'a>` borrowing a `&'a [u8]`, as `ttf-parser`
does. Genuinely better for a caller that mmaps a font file and wants zero copies,
and we may want exactly that once there is a real filesystem-backed font
directory. Rejected for now because the lifetime infects the entire GUI stack and
the copy is one memcpy per font per boot.

**How to reverse.** Add a borrowing `FaceRef<'a>` beside `Face` sharing the same
table-offset logic; `Face` becomes a thin owning wrapper over it. The parsing
code is already written against offsets, so this is mechanical.

### 86b. CFF/Type 2 outlines are a hard error, not an empty glyph

**Decision.** A face whose outlines live in `CFF `/`CFF2` rather than `glyf` is
rejected at `parse` time with `SfntError::CffOutlinesUnsupported`.

**Rationale.** The alternative — open the face and return an empty outline for
every glyph — produces a font that *appears* to work and draws nothing. That
failure surfaces far from its cause and reads at the call site as a rasterizer
bug. Failing at `parse` names the actual limitation, and the caller can fall back
to another face or tell the user.

**Cost, measured.** 18 of the 556 fonts installed on the dev host are CFF (~3%),
but that 3% is most Adobe faces and much of what a user installs by hand.
Tracked as `TD-FONT-NO-CFF-OUTLINES` in `known-issues.md` with the shape of the
real fix.

**How to reverse.** Implement the Type 2 charstring interpreter; the error
variant then becomes unreachable and can be deleted.

### 86c. No TrueType hinting interpreter

**Decision.** `fpgm`/`prep`/`glyf` instruction streams are ignored entirely.
Outlines are scaled and filled as the designer drew them.

**Rationale.** Hinting is a full stack-machine bytecode interpreter — a large,
security-sensitive attack surface (it has historically been a rich source of
remote-code-execution bugs in FreeType and in Windows' font driver) executing
attacker-supplied programs, in exchange for sharper stems at small sizes on
low-DPI displays. FreeType's own default has been the unhinted/auto-hinted path
for years, and the displays this OS targets are high-DPI, where the benefit
largely evaporates. Not writing an interpreter is both the safer and the cheaper
choice.

**Alternative considered.** A vertical-only autohinter (snap horizontal stems to
the pixel grid without running font bytecode) — no attacker-controlled execution,
most of the small-size benefit. Worth doing if small text on a 1080p panel looks
soft. Deliberately deferred, not rejected.

**How to reverse.** Additive: an autohinter is a pass over the `Outline` between
`Face::outline` and `rasterize`, so it needs no change to either.

### 86d. Signed-area accumulation rather than a scanline/active-edge rasterizer

**Decision.** One `f32` accumulator per pixel; each segment deposits *signed*
area only into the cells it crosses; a per-row prefix sum recovers coverage;
`abs().min(1.0)` gives non-zero-winding fill. After Raph Levien's `font-rs`.

**Rationale.** It is branch-light, allocation-free after one buffer, trivially
correct for the non-zero winding rule TrueType requires (counter-wound contours
cancel to zero and become holes without any special casing), and it produces
true analytic anti-aliasing rather than a supersampled approximation. A classic
active-edge-table rasterizer needs per-scanline edge lists, sorting, and explicit
winding bookkeeping — more code and more allocation for the same output.

**Divergences from `font-rs`, both because our input is untrusted.** Every
accumulator write goes through a bounds-checked `add()` that *clips* rather than
clamps (folding an off-left edge's area onto column 0 would paint a spurious
vertical bar), and `into_coverage` resets the running sum per row rather than
sweeping linearly (so a clipped row cannot bleed a solid bar into the next).

**How to reverse.** `rasterize()` is a pure function from `&Outline` and a scale
to a `GlyphMask`; swapping the algorithm touches nothing else.

**Where it lives.** `gui/font/src/sfnt.rs` (86a, 86b, 86c),
`gui/font/src/raster.rs` (86d), `gui/font/src/scaled.rs` (the caching layer
above both), `gui/font/tests/host_fonts.rs` (the 556-font sweep that measured
86b).

---

## §439 — A two-part vowel is never recomposed on the drawing path, and is put back together only when the face cannot draw the halves

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Zone:** gui-core

**In short.** In a dozen or so Asian scripts — Sinhala, Bengali, Tamil,
Balinese and others — a single vowel character is *drawn as two separate marks*,
one on the left of its consonant and one on the right. Unicode's normal
"tidy up the spelling" rule (NFC) wants to keep such a vowel as one character.
But the shaper — the code that decides where each mark goes — has to move the
left half in front of the consonant on its own, and it cannot move half a
character. So on the drawing path we now leave those vowels split. The catch:
if the font has a glyph for the whole vowel but not for its halves, splitting it
makes the text render as two empty boxes instead of one correct mark. So a
second, font-aware pass puts the halves back together whenever the face cannot
draw both of them. Visible result: pre-base vowels land on the correct side of
their consonant in those scripts, and nothing regresses on the 555 installed
faces that have no Sinhala in them at all.

**Context — what forced it.** The Universal Shaping Engine sweep
(`gui/font/tools/use-corpus.txt`, 58 strings × 556 installed faces) disagreed
with HarfBuzz on the Sinhala line `\u0D9A\u0DDC` (KA + KOMBUVA HAA AELA-PILLA).
That vowel canonically decomposes to `\u0DD9` + `\u0DCF` — a pre-base half and a
post-base half. `norm::pieces` was calling the same composition step `nfc` uses,
so the two halves were glued back into one character before the shaper ever saw
them, and the pre-base half could not be moved. HarfBuzz has exactly this case
covered by name: `compose_use` and `compose_indic` in
`hb-ot-shaper-use.cc` / `hb-ot-shaper-indic.cc` both begin

```c
/* Avoid recomposing split matras. */
if (HB_UNICODE_GENERAL_CATEGORY_IS_MARK (buffer->unicode->general_category (a)))
  return false;
```

— refuse to compose whenever the *first* half is a combining mark.

**Decision 1 — one switch, not a per-shaper hook.** HarfBuzz hangs that rule off
each shaper's vtable, so Indic and USE each carry their own copy of it. We take
it as a single mode on the normalizer (`SplitVowels::{Rejoin, LeaveApart}`) which
`pieces` always sets to `LeaveApart` and `nfc` always sets to `Rejoin`.

*Why that is safe rather than merely convenient:* the set of characters the rule
can possibly affect was enumerated exhaustively from the generated tables rather
than assumed. Of every canonical two-part decomposition in Unicode 16 whose
first half is `Mn|Mc|Me`, there are **59**; **12** are composition exclusions
that never recompose anyway (U+0344 and the Tibetan family); the remaining
**47 all belong to scripts shaped by the Indic or USE engines** — Bengali,
Oriya, Tamil, Telugu, Kannada, Malayalam, Sinhala, Balinese, Chakma, Grantha,
Tulu-Tigalari, Tirhuta, Siddham, Dives Akuru and Gurung Khema. There is no
character for which a per-shaper hook and a global switch would differ. That is
pinned by `norm::only_indic_and_use_compose_from_a_mark`, which walks the table
and asserts both the predicate *and* the count 47, so a table regeneration that
adds a script outside the two engines fails the build rather than changing
behaviour quietly.

*Alternative considered:* mirror HarfBuzz and give each shaper a `compose`
function pointer. Rejected: it is two copies of one rule, it puts a font-blind
decision behind a font-dependent dispatch, and the measurement above says the
generality buys nothing.

**Decision 2 — the halves go back together when the face cannot draw them,
and that pass runs *after* `fit_to_face`, not before.** Splitting unconditionally
regressed the sweep hard: `differ 555` on the very first run, one per face,
`ours [0,0,0] vs harfbuzz [0,0]`. The reason is that **HarfBuzz's decomposition
is font-aware and ours was not**. Reading `hb-ot-shape-normalize.cc`:
`decompose()` refuses outright when the second half has no glyph, and falls back
to emitting the whole character when the first half has no glyph and does not
decompose further. So on a face with no Sinhala coverage at all HarfBuzz emits
*one* notdef box, and we were emitting two.

`rejoin_split_vowels` is the font half of the rule: walk adjacent pieces and
rejoin a pair when the first is a mark, the two share a cluster offset, and the
face cannot draw **both** halves.

- *Sharing a cluster offset is the provenance test.* Two halves that carry the
  same offset came from one character this crate took apart; two that carry
  different offsets were two characters the author typed. Only the former may be
  put back — undoing our own split is not the same operation as composing the
  user's text, and conflating them would silently rewrite `\u0D9A\u0DD9\u0DCF`
  (three characters, deliberately) into two.
- *"Cannot draw both" rather than "cannot draw either"* is what makes the
  bottom-up rejoin exactly equal to HarfBuzz's top-down recursion. HarfBuzz asks
  the question on the way *down* and keeps the composed form when the recursion
  fails; we ask it on the way *up* and restore the composed form when it fails.
  The three-piece case is where the two visibly coincide: U+0DDD decomposes to
  U+0DDC + U+0DCA and U+0DDC to U+0DD9 + U+0DCF, and on a face missing only
  U+0DCF both directions land on the same two-glyph answer `U+0DDC, U+0DCA`.
  That case is a test.
- *Ordering.* The pass runs after `fit_to_face` because the two would otherwise
  fight: this one puts a vowel back together when the face cannot draw the
  halves, and `fit_to_face` — seeing a character with no glyph and a drawable
  base — would immediately take it apart again. Running it second means
  `fit_to_face` never sees the rejoined character and has nothing to say about it.

*Alternative considered:* make the split itself font-aware, i.e. only split when
the face can draw both halves, and drop the second pass. Rejected because the
split has to happen before Khmer matra splitting and `fit_to_face`, at a point
where each character is looked at in isolation; the "both halves drawable" test
is a property of a *pair* that only exists once the pieces are laid out. Doing
it as a second pass is also the version that can be read against
`hb-ot-shape-normalize.cc` line by line.

**Cost.** `rejoin_split_vowels` returns immediately unless the run contains at
least one combining mark, which is nearly every string in every Latin document,
so it is off the hot path by a single scan.

**Measured.** USE corpus, 58 strings × 556 faces: `agree 32203`, `reordered 0`,
`misplaced 40`, `differ 0`, `mixed 5` — the 40 being the pre-existing
ignorable-caret divergence tracked under
`TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES`, and the 5 being the itemizer rather
than the shaper (two corpus lines are mixed-script and marked `!`). The default
89-string corpus is byte-identical before and after the change
(`agree 48087`, `differ 1178`), as are the Khmer and Myanmar probe sweeps.

**How to reverse.** `SplitVowels::LeaveApart` in `norm::pieces` back to
`Rejoin`, and delete `rejoin_split_vowels`. Nothing else reads either.

**Where it lives.** `gui/font/src/norm.rs` — `SplitVowels`, `compose_pair`,
`normalize`, `rejoin_split_vowels`; `gui/font/tools/use-corpus.txt` — the two
Sinhala lines that measure it.

---

## §440 — Device corrections are folded into the value at read time, against a size the face itself does not have

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** OpenType lets a font say "this accent sits a bit lower when
you're drawing at 11 pixels tall" — a *device table*, a correction stated in
whole pixels that applies only inside a range of sizes it names. This crate
used to skip past those. Reading them raises three questions with real answers
on both sides: where the correction gets added, what a sizeless object should
answer, and what to do about a second, unrelated feature that occupies the same
eight bytes. What follows is what was chosen and why. Nothing here is
user-visible except at small sizes on the five host faces that ship the
tables — where it is the difference between an Arabic fatha sitting on its
letter and sitting inside it.

**The correction is added where the value is read, not carried alongside it.**
`Value::read` and `mark::anchor` return a coordinate with the delta already in
it; nothing downstream knows a device table was involved.

*The alternative* was to return the correction separately — a `Value` plus a
`Device` — and apply it at draw time. That would let one parse serve two sizes,
which matters for a cache keyed on the face rather than on the scaled font.
It was rejected because the seam it creates is exactly the one this crate has
already been bitten by: two readers of one table, which is what
`kern.rs`'s own doc warns about ("a pair that measures at one correction and
draws at another puts the caret in the wrong place"). Positioning is not cached
across sizes here — `Run` is produced by a `ScaledFont`, which *is* a size — so
the flexibility bought nothing and the risk was live.

**A `Face` answers `Ppem::NONE`, and that is an answer rather than a
fallback.** `Face` is the parsed bytes; it has no size, so it has no device
corrections, and every device table reads as zero through it.
`Face::kern_across` keeps its old signature and its old meaning — design units
— and a new `kern_across_at` carries a size for `ScaledFont::kern_across` to
call.

*The alternative* was to make `Face`'s positioning API take a size, forcing
every caller to state one. Rejected: it would make callers that legitimately
want design-unit metrics (layout at nominal size, measurement for a cache key)
invent a pixel size to get them, and an invented size is a silently wrong
correction rather than an absent one. Two entry points, one of which is
explicitly sizeless, says what is true.

**`deltaFormat` is checked before `startSize`/`endSize`, and this is measured,
not stylistic.** The same eight bytes are also used for a `VariationIndex`
(`deltaFormat == 0x8000`), whose first four bytes are indices into a variable
font's `ItemVariationStore` rather than a size range. A reader that
range-checks first reads those indices as a start and end size, and applies a
delta out of the wrong array whenever they happen to bracket the size being
drawn.

*How often that happens here:* of this host's 9,215 `VariationIndex` records,
3,146 would bracket 9 ppem, 3,086 would bracket 12, and 2,832 would bracket 16
(`gui/font/tools/device_survey.py`). The wrong answer is not a slightly wrong
correction; it is an arbitrary one. Variation indices outnumber real device
tables 60 to 1 on this machine, so this is the *common* path through the
reader, not the edge case — see `TD-FONT-DOES-NOT-READ-VARIATION-STORES`.

**Pixels are converted to font units by truncation, not rounding.** A delta is
in pixels and the value it corrects is in font units, so it is multiplied by
`upem / ppem`. HarfBuzz's `Device::get_delta` does `pixels * (int64_t)scale /
ppem`, which truncates toward zero.

*The alternative* — rounding — is arguably more accurate and was rejected
anyway, because the oracle this crate is measured against truncates, and a
reader that is more accurate than its oracle produces a sweep full of
one-unit differences that have to be explained every time someone reads it.
Matching the reference implementation is worth more than a half-unit.

**Where it lives.** `gui/font/src/device.rs` — `Ppem`, `pixel_delta`;
`gui/font/src/gpos.rs` — `Value::read` and `Run::ppem`;
`gui/font/src/mark.rs` — `anchor`; `gui/font/src/sfnt.rs` — `Face::ppem` and
`kern_across_at`; `gui/font/tools/device_survey.py` — the numbers above;
`gui/font/tools/harfbuzz_sweep.py` — `--ppem`, which is the only way to
measure any of it.

---

## §441 — Script runs resolve through `Script_Extensions`, one row per OpenType tag pair, with direction cut in afterwards

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** Some characters are shared between writing systems. The Arabic
zero `٠` is also how Thaana and Yezidi write zero; the Devanagari full stop `।`
is used by twenty-one Indian scripts. Text has to be cut into stretches of one
script before a font can be asked for that script's rules, and a shared
character sitting in the middle of a word must not cut it — ask a Thaana font
for Arabic rules and you get Arabic-specific substitutions applied to Thaana
letters. Unicode records the sharing as `Script_Extensions`, and this change
resolves runs through it. Three sub-decisions had a real case on both sides.

**One table row per OpenType tag pair, not per Unicode script.** A run
boundary is a change of *row index*, so two Unicode scripts that OpenType files
under one tag are one script here.

*The alternative* — a row per Unicode script, comparing tags to decide
sameness — is the more faithful model of Unicode and was rejected because it is
the wrong faithfulness. Hiragana and Katakana are both `kana`, and they are the
only pair that collides, so the entire practical effect of keeping them apart
is that ordinary Japanese gets cut at every change between the two — for a
difference the font cannot express, since it has exactly one `kana` feature
list to offer either way. Rows also make the extension sets smaller, because a
set naming both collapses to one member.

**A character with no `Script` of its own may narrow a run but never end
one.** When the intersection empties, a character that *has* a script (the
Arabic zero, whose `Script` is Arabic) ends the run and starts a new one; one
that does not (a danda, a tatweel, any combining mark) is left in the run
unchanged.

*The alternative* — treat an empty intersection as a boundary regardless — is
what UAX #24's plain statement suggests and is what was implemented first. It
failed a test written from first principles: `"א" + U+0301 COMBINING ACUTE`
came out as two runs, because U+0301's extension set names eight scripts and
Hebrew is not among them. A mark in a run of its own is a mark whose base's
`GPOS` mark-attachment can never reach it, so the accent lands at the origin
instead of over the letter. The rule that fixes it is stateable in one line —
such a character has an *affinity*, not an identity, and an affinity may refine
a decision but must not make one — and it costs nothing, because a character
with no script had no claim to press in the first place.

**Script is resolved over the whole text first; direction boundaries are cut
into the answer afterwards.** Two passes (`by_script`, then `runs`) rather than
one.

*The alternative* — one pass, closing the open script when the direction turns
— is simpler and is what this module did before. It is also measurably wrong.
In `"ހ٠ހ"` (Thaana with an Arabic-Indic digit in it) the digit is bidi class
`AN`, and rule I2 raises it to an even level inside odd-level Thaana, so it is
its own directional run. A one-pass splitter re-derives the script inside that
run, sees one Arabic-Indic digit, and calls it Arabic — which showed up in the
HarfBuzz sweep as a real disagreement on `SansSerifCollection.ttf`, a face that
registers a `locl` under `arab` and none under Thaana. Pango
(`PangoScriptIter`) and Blink (`RunSegmenter`) both resolve script over the
whole text and intersect with bidi afterwards, for this reason.

*The exception to it*, which the sweep also found: a character with **no**
script at all belongs to the directional run it is *drawn* in. The space in
`"hello שלום world"` is scriptless, so under a whole-text pass it extends the
Hebrew run — and the direction cut then strands it in a one-glyph Hebrew run,
losing the kern between it and `w`. It joins what follows it instead. Only
scriptless characters move this way; one that merely shares its scripts with
its neighbours keeps them across the turn, which is what preserves the Thaana
answer above.

**How it is measured.** 556 faces × 95 strings against HarfBuzz: `agree`
51422 → 51423, `differ` 1179 → 1178, `misplaced` 170 and `reordered` 0
unchanged. The corpus gained a section of shared-character strings — the three
cases above plus `あーア` (which must stay one `kana` run) — so a regression
here is caught by the oracle and not only by the unit tests.

**Where it lives.** `gui/font/src/script.rs` — `ScriptSet`, `by_script`,
`runs`, and the module doc; `gui/font/tools/gen_script_tables.py` —
`extension_ranges`, `pool`, `row_for`; `gui/font/src/script_tables.rs` —
generated, `SCRIPT_EXT_RANGES` / `SCRIPT_EXT_POOL` / `WIDEST_EXTENSION`;
`gui/font/tools/harfbuzz_sweep.py` — the corpus section.

---

## §442 — A caret is a position on the screen with an affinity, and the run carries its bidi levels to answer for one

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** When a line mixes English and Hebrew, one place in the *text* can
be two places on the *screen*. Put the cursor between the English and the
Hebrew: it can be drawn at the right end of the English, or at the right end of
the Hebrew word — which is somewhere else entirely, because Hebrew is drawn
starting from the right. Both are correct; which one the user meant depends on
which side they arrived from. This adds that "which side" as a value called an
**affinity**, and makes the two caret queries measure across the screen rather
than through the string.

**The caret carries an affinity rather than being answered with one x.**
`offset_at` (pixel → text position) returns a `Hit { offset, affinity }`, and
`x_of` (text position → pixel) takes an `Affinity`.

*The alternative* — return the one x that the text's own direction implies, and
let the caller live with it — is what almost every naive implementation does,
and it is not so much wrong as unanswerable. At a boundary the two positions
are equally correct and the run has no way to prefer one: the information that
decides it (which direction the user was moving, whether they just typed) lives
in the editor, not in the shaping. Answering anyway makes the caret jump for
half of all boundary crossings and gives the editor nothing to fix it with.
ICU (`UBiDi` leading/trailing), DirectWrite (`isTrailingHit`) and Chromium
(`SelectionAffinity`) all model it explicitly for the same reason. The cost is
one enum on two signatures; on left-to-right text — every run this crate builds
today — the two affinities name the same point, so a caller with no opinion
passes `Downstream` and is not wrong.

**The run stores per-glyph bidi levels, not just the L2 permutation.**
`ShapedRun` gained `levels: Vec<Level>` beside `visual: Vec<u32>`, kept
whenever any level is odd even if the permutation is the identity.

*The alternative* — derive direction from the permutation, since a reversed
stretch is exactly a right-to-left one — is appealingly economical and is
wrong on the smallest case there is. Reversing a **one-glyph** run is the
identity permutation, so a single Hebrew letter between two Latin words looks
unreordered while still being read from its right edge: a caret reading the
permutation alone would put itself at the wrong end of it. The permutation
answers "where is this glyph drawn"; a caret also needs "which end of it does
the reader start at", and only the levels say that. The extra vector is empty
for all left-to-right text, which is the case that has to stay free.

**The old logical-order sum survives under a name that says what it is.**
`width_upto(at, end)` is the previous `x_of` body verbatim.

*The alternative* — delete it, since the caret is now correct — would have
removed the number that truncation and ellipsis placement actually want. Those
callers cut the string and measure the piece; "how wide is the prefix" is a
real question with a monotone answer, and it is only wrong when it is called a
caret. Keeping both, named apart, is also what let the regression assertion in
`tests/host_fonts.rs` be moved rather than deleted: "the caret never moves
backwards as it advances through the string" is true of `width_upto` and is
precisely what a correct visual caret must violate, for the length of every
right-to-left run.

**A cluster is one box.** Both queries treat a cluster — a ligature's several
characters, or a decomposed character's several glyphs — as a single
rectangle whose left edge is the leftmost slot any of its glyphs occupies. That
is sound rather than approximate: rule L2 reverses whole level runs, and a
cluster lies inside one, so its glyphs may be drawn in the other order but are
still drawn side by side.

**Where it lives.** `gui/font/src/shape.rs` — `Affinity`, `Hit`,
`ShapedRun::levels`, `reordered`, `slot_of`, `cluster_box`, `leading_edge`,
`trailing_edge`, `offset_at`, `x_of`, `width_upto`;
`gui/font/src/scaled.rs` — the per-glyph levels are now hoisted out of the
reordering branch and handed to the run; `gui/toolkit/src/text.rs` —
`char_index_at`, which drops the affinity because a character *index* is the
same on both sides of a boundary.

---

## §443 — A base direction is a third argument to one full `shape_with`, and it switches off the left-to-right fast path

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** Which way a line of text runs is usually decided by the text
itself — if it starts with a Hebrew letter it runs right to left. That guess is
wrong whenever the direction is a property of *where the text is* rather than
what it says: an `OK` button in a Hebrew interface, a Hebrew folder name in a
left-to-right path bar. The shaper had no way to be told. It does now, and two
things about how are worth recording.

**One full form with defaults, not a method per knob.** `shape_with(text, lang,
base)`; `shape(text)` is `(text, None, Base::Auto)` and `shape_lang(text,
lang)` is `(text, lang, Base::Auto)`.

*The alternative* — `shape_with(text, base)` beside the existing
`shape_lang(text, lang)`, which is what the issue that asked for this proposed
— reads better at each individual call site and does not survive the second
knob. Language and direction are orthogonal: a Hebrew UI rendering a Turkish
place name needs both, and under the two-method shape there is nowhere to say
so short of a fourth method. Three methods where the third is the full form and
the other two are it with defaults is the arrangement that does not grow when a
fourth parameter arrives.

*The alternative also considered* — an options struct, `shape_with(text,
ShapeOptions { .. })` — is what this becomes if a fourth parameter does arrive.
It was not worth introducing for two fields, and introducing it later is a
mechanical change at three call sites rather than a redesign.

**The left-to-right fast path is gated on the base, not only on the text.**
`byte_levels` returns an empty level vector — skipping the entire bidi
algorithm — for any string `bidi::is_trivially_ltr` accepts. That now requires
`base != Base::Rtl` as well.

*The alternative* — keep the check purely textual, since the text really does
contain no right-to-left character — is wrong in a way worth naming, because
the check reads like a property of the string and is not one. What
`is_trivially_ltr` actually asserts is a property of the *answer*: every level
comes out even, so the permutation is the identity and rule L4 mirrors nothing.
That is true under `Auto` and under `Ltr`, and false under `Rtl`, where the
same plain-Latin string resolves to level 2 inside a level-1 paragraph — the
letters still read left to right, but the neutrals around them take the
paragraph's direction and any brackets mirror. Left ungated, the fast path
would have discarded the base the caller had just supplied, and done it
silently and only for the strings where the caller most needed it: a UI label
is exactly the kind of string that has no strong right-to-left character in it.

**A note on the example this was filed with.** The issue's motivating case was
`"(123)"` in a right-to-left paragraph, "the parentheses come out mirrored the
wrong way". They do not: under `Base::Rtl` rule L4 mirrors both brackets and
rule L2 then swaps their positions, and the two cancel exactly, so the string
renders identically to the `Auto` answer. That is correct — a number inside a
Hebrew sentence reads left to right, brackets and all. The cancellation is
general for a balanced pair around a single embedded run, which is why the
regression test uses the unbalanced `"(a"` instead: `(a` under `Auto`, `a)`
under `Rtl`. Recorded because the wrong example was persuasive enough to sit in
the issue file unchallenged, and a test written from it would have passed
against a `base` argument that did nothing at all.

**How it is measured.** A host-font test over 547 of this machine's faces
asserts the drawn order and the mirrored glyph, taking every glyph id out of a
*whole shaped string* rather than out of a character shaped alone — on
`Amiri-Bold.ttf` a lone `(` is scriptless and shapes to glyph 3, while the `(`
in `"(a"` joins the Latin run and reaches a `latn` lookup that substitutes
6460. An oracle built from isolated characters would have been testing the
itemizer. The test also does *not* assert that the two directions measure the
same width: L4 substitutes a genuinely different glyph, and Amiri-Bold gives
`)` a different advance from `(` — 11.904 px against 11.552. What is asserted
is that each answer is self-consistent, drawn width equal to measured width.

**Where it lives.** `gui/font/src/scaled.rs` — `shape`, `shape_lang`,
`shape_with`, `byte_levels`; `gui/font/src/bidi.rs` — `Base`, unchanged, and
`is_trivially_ltr`, whose doc already said what it asserts;
`gui/font/tests/host_fonts.rs` —
`a_paragraph_direction_can_be_given_and_changes_the_answer`.

---

## §444 — A selection is a set of boxes, not a box

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)

**In short:** When you drag across text to highlight it, the highlight was
drawn as one rectangle stretching from the first selected character to the
last. That is only right when the text runs one way. In a line that mixes
English and Hebrew, characters the user did *not* select can sit physically
between the ones they did, so a single rectangle paints them too — the
highlight claims you selected words you never touched. The fix is that asking
for a selection's shape now returns a *list* of rectangles, one per stretch
that really is contiguous on screen, and every caller loops.

`ShapedRun::selection_rects(from, to, end) -> Vec<(f32, f32)>` replaces the
two-edge form `x_of(to) - x_of(from)` at every call site
(`guitk::text::selection_boxes`, `pathbar`'s edit-mode highlight, `textview`'s
selection and search-match highlights).

**Why a list rather than a rectangle.** A byte range is contiguous in the
string by construction; it is *not* contiguous on the screen. Rule L2 of the
bidi algorithm reverses each right-to-left run in place, so a logical range
that straddles a direction change lands as two separated boxes, and the gap
between them is occupied by characters outside the range. The two-edge form has
no way to express that — it can only report one interval — so it necessarily
over-paints. The over-painting is not cosmetic: a highlight is the UI's answer
to "what will Copy copy?", and a wrong answer there is a wrong answer about the
user's own data. The shaped run already knows the exact answer, because it
knows the visual order; the only thing missing was a return type able to carry
it.

*The alternative* — return the bounding box of the selected glyphs — is the
same bug with more arithmetic behind it, and is worse for being harder to
recognise as wrong.

**Why `Vec` rather than `impl Iterator`.** Every other geometry query in
`ShapedRun` is lazy, so this one is the odd member and the departure is worth
recording. It cannot be lazy: the caller wants the boxes in drawing order
(left to right), the map from logical index to *selected* must therefore be
complete before the first box can be emitted, and the glyph drawn first may be
the last one logically. A lazy adaptor would have to build the same map inside
its first `next()` — the allocation is not avoided, only hidden, and hiding it
would cost the caller the ability to ask how many boxes there are.

**Why the sweep runs in slot space, not pixel space.** Adjacent boxes are
coalesced into one, and adjacency is decided by comparing *slot indices* —
integers — rather than by comparing a box's right edge to the next box's left
edge. Both quantities are sums of the same advances, but summed in different
orders, and float addition is not associative: the comparison would need a
tolerance, and any tolerance is a number that is too small on a long line and
too large on a short one. Integers need none. (`textview`'s cross-*span*
coalescing does still compare pixels with a 0.01 tolerance, because separate
spans are separately shaped and there is no shared slot space to appeal to.)

**A cluster is always painted whole.** Selecting one half of a ligature
highlights all of it. This falls out rather than being enforced: L2 reverses
whole level runs and a cluster lies entirely inside one, so a cluster always
occupies one contiguous screen box whatever the reordering did. Marking the
cluster through `group_end` is what makes the run-accumulation see it as one
stretch.

**Where it lives.** `gui/font/src/shape.rs` — `selection_rects` and its five
tests (including `selection_boxes_are_ordered_disjoint_and_add_up`, which
checks all 28 sub-ranges of a six-character bidi string);
`gui/toolkit/src/text.rs` — `selection_boxes`/`selection_boxes_in`;
`gui/toolkit/src/pathbar.rs`, `gui/toolkit/src/textview.rs` — the callers.

---
