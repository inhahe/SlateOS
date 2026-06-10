# Design Decisions Log

This file records **deliberate design decisions** made during development,
each with enough context to reconsider it later. It is distinct from the
broad spec (`design.txt`) and the original rationale notes
(`design desicions.txt`, `other design decisions.txt`): this file is a
running, dated log of decisions taken while implementing, especially ones
where a reasonable alternative exists and the operator might want to revisit.

Format for each entry:

- **Context** — what problem forced a choice.
- **Decision** — what was chosen.
- **Rationale** — why.
- **Alternatives considered** — and why they were rejected.
- **Where it lives** — files/symbols, so the decision can be located and reversed.
- **How to reverse** — what changing the decision would entail.

---

## 1. Linux ABI version to target — baseline 6.6, "baseline + honored extras"

**Date:** 2026-06-06 (policy) / 2026-06-10 (uname surface resolved)

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
