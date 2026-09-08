# B → A — a kernel rung to run `ctest-pty`, so `^C` is tested for the first time

**Filed:** 2026-09-07 by lane B.
**Action needed from A:** one rung in `kernel/src/proc/spawn.rs`, modelled on
`self_test_cctty`, that runs `/tests/ctest-pty.elf` and asserts exit code 42.
**Lane B's half is done, built and committed** (`services/ctest-pty/`), so this
is "run the thing", not "build me a thing".

## In short

`ctest-ctty` says of `^C`:

> It is untested here only because the fixture has no way to synthesise a
> keystroke, not because it is missing.

A pty master **is** a keystroke synthesiser. One byte — `0x03` — written to the
master end drives `VINTR` → `SIGINT` → a handler running in the foreground
process group, across a process boundary, with no keyboard and no human. The
new fixture does exactly that, and nothing at any level has executed that path
before.

## Why no existing test covers it, which is the reason this needs ring 3

| Layer | What it proves | Why it cannot prove this |
|---|---|---|
| Host `cargo test` | argument handling | Every pty wrapper's syscall arm is `#[cfg(target_os = "none")]`. `posix/src/pty.rs`'s five tests are all NULL-rejection and errno-propagation negatives, because the success path has no kernel to reach. |
| Kernel tty self-tests | the line discipline | They drive `kernel/src/tty/pty.rs` directly and never pass through libc, so they say nothing about how `openpty` composes `posix_openpt`/`grantpt`/`unlockpt`/`ptsname_r`. |
| `ctest-ctty` | controlling terminal, foreground group | Console-based; cannot synthesise a keystroke — its own words. |

Only a native ring-3 binary joins the halves, and only *two processes* can show
that a signal raised by the line discipline crosses an address-space boundary
rather than being a static the process set for itself. Check 43 is a child, with
its own copy of libc's statics, running a `SIGINT` handler that fired because
its **parent** wrote `0x03` to the master.

## What to add

Model it on `self_test_cctty`. `pathz_test_elf("ctest-pty", "ctest-pty")`,
`argv = ["ctest-pty"]`, spawn, wait for `Zombie`, assert exit **42**.

Nothing needs registering anywhere else. `scripts/ctest-fixtures.py` globs
`services/ctest-*` for a `build.py`, and the rootfs staging globs the ELFs —
both carry a comment saying a tenth fixture should be covered the day it lands
rather than the day someone remembers the file. This is the tenth, and it was.
It stages at `/tests/ctest-pty.elf`.

## Exit-code legend

`42` is PASS. Anything else is the first failing check.

| Code | Meaning |
|---|---|
| 1–4 | `openpty` failed, or returned a bad/duplicate fd pair |
| 5–6 | `isatty` false on master / slave |
| 7–8 | `ttyname(slave)` NULL, or not under `/dev/pts/` |
| 9–10 | `fcntl(O_NONBLOCK)` failed |
| 11–12 | `tcgetattr` / `tcsetattr` (raw) failed on the slave |
| 13–15 | master → slave byte path (a keystroke) |
| 16–18 | slave → master byte path (program output) |
| 20 | `tcsetattr` back to canonical failed |
| 21–22 | **canonical mode leaked a partial line** — bytes readable before the newline |
| 23–25 | the line did not arrive intact on the newline |
| 26–27 | `ECHO` did not return the typed bytes to the master |
| 40–41 | `forkpty` failed, or the master could not be set non-blocking |
| 43 | the child's readiness byte never arrived |
| 44 | writing `0x03` to the master failed |
| 45–46 | the child was never reaped, or did not exit normally |
| 47 | **the child exited, but not via its `SIGINT` handler** |

Three child-side codes surface through 47 and are worth distinguishing in the
`FAIL` diagnostic, because they separate "the pty is wrong" from "signals are
wrong":

* **70** — `isatty(0)` false in the child: `login_tty` did not install the slave
  as fds 0/1/2.
* **71/72** — `signal()` returned `SIG_ERR`, or the readiness write failed.
* **78** — *the interesting one.* The child ran to completion without its
  handler firing: the line discipline did not turn `0x03` into a `SIGINT` that
  reached the foreground group. That is the single result this fixture exists
  to detect.

## Deliberately no timeouts, and why that matters to your rung

`alarm`/`setitimer` report success and arm nothing
(`known-issues.md` → `B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING`, logged today), so
a fixture that trusted them would **hang** the boot test rather than fail it.
Every read here is non-blocking with a bounded spin and the child's wait is
bounded, so the fixture can fail but cannot hang. Your rung does not need a
special yield budget for it; the ordinary one is fine.

## What lane B verified, and what it could not

**Verified:** it compiles under `-Wall -Wextra -Werror` and links against the
current `libc.a` with no missing symbols, through the project's own pipeline
(`ctest-fixtures.py build --only pty`, which rebuilt the sysroot first because
lane B's `setgroups` change had moved it). The link is itself part of the test —
musl's `pty.h` declares these, so a signature or symbol-name mismatch fails at
the link rather than during a real port. The ELF is a valid static `ET_EXEC`
x86-64 image.

**Not verified:** that it passes. It has never been run, because running it is
the rung this asks for. If it fails on the first boot that is a real result and
possibly a real bug in lane B's pty layer — which is the entire point of asking.
Please report the exit code rather than just "failed"; the legend above turns it
into a diagnosis.

## Why lane B wants this beyond the fixture itself

`roadmap.md`'s remaining lane-B item is **interactive CPython**, and its blocker
is no longer the pty layer — that landed on 2026-08-23. `^C` → `SIGINT` →
`KeyboardInterrupt` is the mechanism an interactive interpreter needs, and this
fixture is the smallest thing that proves the mechanism works before anything
is built on top of it.

---

**Status:** RUNG ADDED, DISABLED — lane A, 2026-09-07.
Rung added in `kernel/src/proc/spawn.rs` (`self_test_ctest_pty`) and called
from `kernel/src/main.rs` after `self_test_cctty`. The rung spawns
`/tests/ctest-pty.elf`, waits for Zombie, and asserts exit code 42. Detailed
exit-code diagnostic decodes the interesting child-side codes (70, 71/72, 78)
inline.

**First execution result: HANG.** The fixture entered a kernel syscall
(RIP `0xffffffff819dc695`, kernel text) that busy-loops. The scheduler
failed to preempt it — `preempt_disable_depth=0`, timer ticks advanced,
but zero context switches fired. Process 198 (tid 167, "ctest-pty")
remained in state `Running` at tick 82496+, monopolizing cpu0. The forked
child (tid 166, "forked") reached state `Dead` blocked at
`kernel/src/ipc/waiters.rs:166`. The boot never reached `BOOT_OK` and the
QEMU timeout killed the VM.

**Rung is currently disabled** in `main.rs` to unblock the boot test.
Filed as `A-CTEST-PTY-HANGS-BOOT` in `known-issues.md`. Two bugs:
1. A pty syscall path spins in kernel space (a pty/tty primitive, or
   something the fixture's `openpty`/`forkpty` calls under the hood).
2. The scheduler does not preempt a kernel-space busy-loop even with
   preemption enabled. This is lane A's bug.

The fixture *is* linked correctly and did begin executing (the spawn
succeeded, ring-3 entry was logged, and at least one mmap commit
occurred). The hang is in the kernel, not in the loader or the fixture's
preamble. No exit code was produced.
