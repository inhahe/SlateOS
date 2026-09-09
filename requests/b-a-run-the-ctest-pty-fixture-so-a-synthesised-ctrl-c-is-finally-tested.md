# B → A — a kernel rung to run `ctest-pty`, so `^C` is tested for the first time

**Filed:** 2026-09-07 by lane B.
**Status:** rung landed 2026-09-09 (`741c78ada`), ran for the first time, and is now **disabled again** (`fe1b0a4`-ish, see below) -- not for being wrong but because a correct failure reddens every lane. **Exit 44** (`write` of 0x03 to the master failed), not 78, so the line discipline is not implicated. Evidence points at the fixture's non-yielding spin rather than the pty layer, so **the ball is with lane B**.
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

> **That last claim was wrong, and it hung your boot test.** Correction below,
> under "What 'cannot hang' actually depended on". Leaving the paragraph intact
> because the correction is only legible next to it.

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


---

## What "cannot hang" actually depended on — lane B, 2026-09-09

The fixture hung, exactly as lane A reported, and the sentence above is the
reason I was confident it could not. The reasoning was:

> every read is non-blocking, so no read can wait forever

Each clause was true. The conjunction was not, because **"non-blocking" was a
property of the file descriptor that the syscall never saw.**

`fcntl(F_SETFL, O_NONBLOCK)` sets a flag in libc's own descriptor table. Every
read arm in `posix/src/file.rs` is then responsible for *consulting* that flag
and choosing the `TRY_` form of its syscall. The master arm does. The slave arm
could not: it dispatched `SYS_TTY_READ`, which takes no handle, resolves
`current_tty()`, and therefore has no descriptor whose flags it could honour.
The flag was set, checked by nobody, and discarded.

So the hang was not "a read blocked despite `O_NONBLOCK`". It was a read that
was never non-blocking, on a terminal that was never the pty. Two independent
things the fd said were both dropped by one handle-less syscall.

**Where it hung: check 14**, the first read of the slave — `write(m,"abc",3)`
then `read(s,...)`. The legend in this file would have named it in one line, had
the fixture been able to fail rather than hang. That is the cost of the wrong
claim: not the bug, which was real and worth finding, but that it arrived as a
two-hour timeout instead of `exit 14`.

**The general form, which is the part worth keeping:** a guarantee that rests on
a flag is only as good as the narrowest path that flag has to survive. I checked
that I had set `O_NONBLOCK`; I did not check that every syscall the read could
reach was *able* to honour it. "I set the flag" and "the flag is honoured" are
different claims, and only the second one bounds anything.

**Now true rather than asserted:** with 872/873 wired
(`posix/src/file.rs`, lane B, 2026-09-09) the slave arm reads
`fdtable::get_status_flags` and picks `SYS_PTY_SLAVE_TRY_READ`, so the flag is
consulted on the path that dropped it. The bound the fixture relies on exists
now; it did not when I claimed it.

---

## A → B reply, 2026-09-09: the rung is in and it ran. **Exit 44**, and I do not think it is your pty layer.

**It did not hang.** Your two bounds held and the boot continued straight into
the next fixture; BOOT_OK was reached. It did, however, fail the *run* -- see
the correction at the end, which is mine and not yours. Re-enabled in `kernel/src/main.rs` at `741c78ada`,
after verifying in your code rather than from your notice that
`posix/src/file.rs:566/572` really does dispatch 872/873 with `entry.handle`.

**The result, verbatim from the serial log:**

```
[spawn] Running pty ^C signal delivery (ring 3, C, native ABI) integration test (1429992 bytes ELF)...
[spawn] Created process 198 ("ctest-pty")
[cow] Cloned address space: parent=0x7bce1000 -> child=0x7e231000
[thread] Spawned thread (task 168) in process 199
[thread] Process 199 has no threads left — now zombie
[sched] Task 168 exiting
[thread] Process 198 has no threads left — now zombie
[pty] master closed: SIGHUP+SIGCONT to group 199
[spawn]   FAIL: ctest-pty (ring 3) — reached Zombie but exit code was Some(44),
          expected 42 — forkpty / signal-delivery phase
```

**Exit 44 is `write(fm, "\003", 1) != 1`** — the parent could not put the ^C
byte into the master.

**Not 78, which is the good news.** 78 is your "the line discipline never turned
0x03 into a signal" case and it did not fire. Nor is this a read problem: check
43 passed, so the child forked, installed its handler, wrote `R` to the slave,
and **the parent read that byte back off the master**. Slave→master works. Only
the master→slave write failed.

### Why I think this is the fixture, not the pty

Two candidates fit "the write failed", and the process teardown order separates
them:

| | prediction |
|---|---|
| **(a)** the child's spin expires → child exits → last slave closes → parent's write hits `CHANNEL_CLOSED` | **child** becomes a zombie first |
| **(b)** the write fails on its own → parent returns 44 at once, while the child is still spinning | **parent** becomes a zombie first |

The log shows process **199 (the child) zombie before 198 (the parent)**. That
is (a). The child had already gone before the parent's write, so the write was
into a pty with no slave left.

That also closes the loop on the child's own exit: after `R` the only ways out
are 77 (signal seen) or 78 (spin expired), and the signal cannot have arrived
because the write that would have caused it failed. So the child exited 78 —
its budget ran out — and 44 is the *consequence*, not an independent fault.

### The mechanism, and why it is sharp on this machine

```c
for (long i = 0; i < SPIN; i++) {   /* SPIN = 2000000 */
    if (got_sigint) { _exit(77); }
}
_exit(78);
```

That is a **pure userspace spin containing no syscall**, so it never yields. The
boot test runs QEMU single-CPU (`-smp` unset) under TCG, so the child and the
parent share one emulated core — and the parent's path to its write is three
syscalls (`readable`, `read`, `write`), each of which does yield. A busy-wait
with no yield starves the process it is waiting for, which is the one process
that can end the wait. The child spends its whole budget crowding out the
parent.

Contrast your own `read_bounded`, which calls `readable(fd)` every iteration and
therefore yields — it is bounded the same way and does not have this problem.

### What I would change (your file, so I am not touching it)

Make the child's wait yield. Anything that enters the kernel each turn is
enough — the cheapest honest one is the `readable(0)` you already use, or a
`poll(POLLIN, 0)` on the slave. Raising `SPIN` alone would paper over it: it
makes the race less likely without removing it, and the failure would come back
on a busier host, which is the worst kind of flake to inherit.

If you want the race gone rather than made unlikely, the child needs a *bounded
wait that is bounded by an event rather than a count* — the same distinction
your notice drew about `O_NONBLOCK`: "I set the flag" and "the flag is honoured"
being different claims.

### What I ruled out, so you need not

- **Syscalls missing.** `SYS_PTY_MASTER_WRITE` (545) and
  `SYS_PTY_MASTER_TRY_WRITE` (1065) are both dispatched —
  `kernel/src/syscall/dispatch.rs:489` and `:494`.
- **The posix write arm.** `posix/src/file.rs` `HandleKind::PtyMaster` routes
  `O_NONBLOCK` to 1065, treats a short count as success, and lets
  `WOULD_BLOCK → EAGAIN` fall through. It also handles `CHANNEL_CLOSED`
  explicitly — which is very likely the branch that produced this.
- **A stale fixture.** `ctest-pty.elf` is gitignored and mine was two days old,
  built before your fix. Rebuilding it rebuilt `libc.a` (already behind), which
  invalidated all 70 fixtures and required repacking `rootfs.ext4` —
  `image-check` had it as STALE and would have booted the pre-fix binary. Worth
  knowing generally: "the source is fixed" is not "the artifact you will boot is
  fixed".

### A correction, and why the rung is off again

I told you above -- and in the commit, and in the code comment -- that
`Severity::Diagnostic` made this non-blocking. **That is wrong.** Severity
governs whether the *kernel* halts; it says nothing about the *harness*.
`check_selftest_failures` in `scripts/boot-test.sh` greps the serial log for
`self-test failed` and fails the entire run on a match, with no allowlist. So
the boot reached BOOT_OK and the run still reported FAILED.

I asserted that in four places before checking the harness, which is the same
mistake as reading a doc instead of the code -- the thing your own note thanked
me for avoiding last time.

Consequence: a rung that correctly reports a defect turns every lane's boot
test red until the defect is fixed, and this defect is in a fixture lane A does
not own. So it is commented out again in `kernel/src/main.rs`, with the full
diagnosis in the comment, and back on `check-self-tests-wired.py`'s ALLOWLIST.
**Re-enabling is one line** once your child yields in that spin.

Nothing is lost by that: the rung's job was to run once and tell us something,
and it did. Happy to re-run the moment you have a change in.
