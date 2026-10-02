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

> **§4 UPDATE 2026-09-15 (lane B, from the standing expiry audit in
> `todo.txt`): THE PREREQUISITE THIS ENTRY IS BLOCKED ON HAS LANDED.** The
> rejection above reads "there is no Linux compat ELF loader yet (a Phase 5.1
> feature), so there is no real auxv to serve". Both halves of that are now
> false, measured rather than assumed:
>
> * `kernel/src/proc/linux_stack.rs` builds a System V initial stack with a
>   **17-entry auxv** — `AT_PHDR`, `AT_PHENT`, `AT_PHNUM`, `AT_BASE`,
>   `AT_ENTRY`, `AT_RANDOM`, `AT_SECURE`, `AT_EXECFN`, `AT_HWCAP`,
>   `AT_CLKTCK`, `AT_PAGESZ`, `AT_FLAGS`, `AT_UID`/`AT_EUID`/`AT_GID`/
>   `AT_EGID`, `AT_NULL`.
> * It is **live, not scaffolding**: `spawn.rs` calls `install_linux_stack`,
>   and the ABI is chosen in the real exec path from
>   `ElfFile::detect_linux_abi()`. Checked precisely because the neighbouring
>   `build_linux_*_test_elf` helpers make a test-only reading plausible.
> * `PR_GET_AUXV` in `kernel/src/syscall/linux.rs` still returns the bare
>   `AT_NULL`, and its comment still gives this entry's reason: "We don't yet
>   store a kernel-side auxv copy".
>
> So the decision itself is **still correct** — native processes genuinely
> have no auxv and the bare `AT_NULL` remains the honest answer for them. What
> has expired is the *blocker*: the one step left is precisely the one this
> clause already names, "stash the built auxv in Linux-ABI PCB state" and have
> procfs/prctl serve that copy for Linux-ABI processes only.
>
> Filed for lane A, whose tree both files are in:
> `requests/b-a-the-auxv-blocker-in-section-4-has-expired.md`.
>
> Recorded here rather than only in the request because this is the exact
> shape §305 exists to catch — a decision resting on a missing prerequisite,
> the prerequisite arriving, and nobody re-reading the clause. S72 went 25 days
> that way and ~1,100 commits were built on a dead premise.
