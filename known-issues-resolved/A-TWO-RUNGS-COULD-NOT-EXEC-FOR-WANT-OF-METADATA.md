### [A] `A-TWO-RUNGS-COULD-NOT-EXEC-FOR-WANT-OF-METADATA` — `ctest-coreutils-runs` (11) and `ctest-python-repl` (8) were spawned unable to stat -- 2026-09-24
**Status:** FIXED 2026-09-24 (lane A, `kernel/src/proc/spawn.rs`) — the exec now succeeds, as the addendum below shows, and `ctest-coreutils-runs` stays red on two link-level faults filed for lanes B and D.

**In short:** two tests start a program that then starts another program. The
second start failed every time, and weeks of investigation looked for the
reason in the disk image, the C library and the kernel's program loader. The
reason was in the tests themselves: they launched their program without the
permission needed to look up a file's size — the first thing the C library
does when asked to run a file.

**The mechanism.** `posix/src/spawn.rs::load_elf` sizes its buffer with
`SYS_FS_STAT` before it reads anything, and native `sys_fs_stat` requires
(File, METADATA). `self_test_coreutils_runs` granted (File, READ|EXECUTE) and
`self_test_ctest_python_repl` granted nothing at all, so in both the forked
child's `execl` returned -1 with EACCES and the child took `_exit(127)`. That
is why the 2026-09-21 entry could establish that "the exec syscall is never
reached" and go no further: nothing upstream of it printed, because a refused
stat is not an event anyone logs.

**It had been written down already.** `self_test_cpython_on_slateos_libc`
records the identical trap in its own capability comment — *"native
sys_fs_stat is gated on METADATA"*, learned from the same failure in the
`make` rung — and the two rungs that repeated it were written from sketches
that predate that note. A rung that runs a program is really describing a
*session*, and the grant should be written as one: what does a program that
execs, stats and reads need? The REPL rung also lacked `PYTHONHOME`, which the
CPython rung calls MANDATORY; it now passes the same environment.

**The general point, for the next rung.** Three faults in a row produced the
same exit code 11 here (the image, the `/bin` vs `/mnt/bin` path, then this),
and each fix was verified by the code *stopping* after the first cause — so
the next cause looked like a failure to fix the previous one. The legend for
11 now names all three.

**Addendum, same day — the grant was the gate, and behind it are two more.**
With METADATA granted, a direct QEMU boot exec'd `/mnt/bin/true`
successfully, and `true` died at its first instructions: a page fault reading
address 0x36, in `posix::tls::image`. Two faults, both in how `coreutils` is
linked, neither in lane A's tree:

1. `userspace/coreutils/linker.ld` (and `oils`, `shell`) put the ELF header
   outside the only `PT_LOAD`, so lld leaves `__ehdr_start` at 0 and
   `tls::image` reads `e_phentsize` from address 0x36.
2. `coreutils`' binaries carry `EI_OSABI = GNU` (LLVM tags objects GNU when they
   use a GNU extension; the slateos target is an LLVM linux-musl triple), so
   the kernel ran them with the Linux syscall table, and the native
   `SYS_SET_FS_BASE` (528) just before the fault was refused.

Lane A's half of (2) landed the same day: the explicit SlateOS native marker of
design-decisions §33 (`EI_OSABI = 255`, or a `"SlateOS"` / `NT_SLATEOS_ABI`
note) now outranks every Linux signal in `detect_linux_abi`. Emitting the
marker, and mapping the header, are lanes D's and B's:
`requests/a-bd-coreutils-cannot-start-two-link-faults.md`. Measured scope:
`true`, `false`, `echo`, `basename` have both faults; `kill`, `logger`, `cat`,
`ls` and `python3` have neither.

**Second addendum — `ctest-python-repl` now starts the interpreter, and the
environment is what is missing.** With its grant fixed the rung's exec
succeeds and CPython starts, then exits **4** ("output appeared but the answer
never did"). The log shows why: the fixture was spawned with three
environment variables and its child's exec stored **none**
(`[exec] Stored 4 argv, 0 envp entries`). posix's `execv` is
`execve(path, argv, NULL)` — its own doc says it inherits the environment —
and `execvp`, `execl` and `execlp` all reach it, so `PYTHONHOME` never arrives
and the interpreter cannot find its library. Lane D's code:
`requests/a-d-execv-execvp-execl-execlp-start-the-new-program-with-no-environment.md`.

So all three of lane A's long-red ring-3 rungs now have their kernel-side
causes fixed: `ctest-pty` passes; `ctest-coreutils-runs` waits on the link
faults (B, D); `ctest-python-repl` waits on `execv` (D).

**Third addendum (2026-09-26) — the integration boot of `3fd70ae1d`.** Three
rungs red, one of them lane A's own, and then a kernel panic that was also
lane A's:

- The panic: `kshell::self_test` rung 21 asserted `syshealth` exits 0, and
  `syshealth` correctly reported a real fault — lockdep had caught an AB/BA
  inversion in the VFS: `flock`/`funlock`/`lock_query` resolved the file's
  identity (locking the mounted filesystem) while holding `LOCK_TABLE`, and
  procfs's `/proc/locks` takes `LOCK_TABLE` under the procfs lock. The
  assertion turned the report into a panic, and every self-test after it —
  the network checks among them — never ran. Both fixed in the next change:
  the identity is resolved before the table is taken (as `reclock.rs`
  already did), and rung 21 now asserts that each checker's status matches
  its printed verdict, failing the self-test at the end, not panicking, when
  the kernel is unhealthy.

- `ctest-python-repl`: exit 4, as above. Lane D fixed `execv` on its branch
  on 2026-09-25 (`execve(path, argv, current_environ())`); it is not on `main`.
  The rung needs both that and lane A's grant fix, which is on `lane-a` only —
  so each lane's boot stays red on this rung until the other's fix is on
  `main` (A-Q20; resolved by §968: lane A publishes under its three conditions).
- `Path-Z real CMake`, `cmake 01`: exit -8. CMake ran its script, then crashed
  inside `exit`: `cmsys::RegularExpression::~RegularExpression()` called with
  `this = NULL` (fault at 0x220; symbolised from `build/spike/cmake-slateos.elf`).
  libc's `__cxa_atexit` is a stub that drops the object pointer, so every C++
  static destructor runs on a null `this`; its 32-entry table is also far too
  small for CMake. Lane D's code:
  `requests/a-d-cxa-atexit-drops-the-object-so-static-destructors-run-on-null.md`.
- `SYS_PROCESS_SPAWN_EX2 argument-ABI`: probe 0x18. Lane A's own: §960 added
  `cwd_ptr`/`cwd_len` to `SpawnEx2Args` (128 → 144 bytes) and the ring-3
  probe program still aimed "the unknown tail" at byte 128 — now `cwd_ptr`, a
  known field, so accepted. Fixed in the next change: the tail probes moved to
  144/152, and five probes (0x21-0x25) now cover the `cwd` fields' own rules.
