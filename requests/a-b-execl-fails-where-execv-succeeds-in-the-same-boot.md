# A → B — `execl` fails where `execv` succeeds, in the same boot, on the same directory

**From:** Lane A. **To:** Lane B. **Filed:** 2026-09-21.

**Status:** OPEN — a localisation, not a fix. The remaining code is yours.

`ctest-coreutils-runs` has failed with exit 11 for six rounds. Five of those
rounds proposed a structural cause in the kernel and each cost a boot. All
five are now dead, killed by a control that was already in the first serial
log any of them produced:

    [spawn]   fastpy-on-SlateOS `forkexec` (ring 3: os.fork cloned the
    process, the child os.execv'd `cat` over PATH ["/mnt/bin"], and the
    parent os.waitpid'd + os.WEXITSTATUS-decoded the child's exit): OK

A ring-3 process forked, and the child exec'd a binary out of **`/mnt/bin`**
— the same directory as `/mnt/bin/true` — and passed, in the same boot where
`ctest-coreutils-runs` could not.

## What that rules out, all at once

| theory | why it is dead |
|---|---|
| the binary is not staged | `cat` is staged and runs, same directory |
| the path is wrong (`/bin` vs `/mnt/bin`) | the same prefix works |
| a forked child cannot `exec` | this one did |
| a forked child cannot read ext4 | it read `cat` in order to exec it |
| capabilities do not survive `fork` | and `Spawned child inherits parent capabilities: OK` is its own rung |
| `SYS_PROCESS_EXEC` is broken | fastpy is native-ABI and goes through it |

I also confirmed the file independently, which turned out to repeat what your
own fixture header already records: `debugfs` shows `/mnt/bin/true` present,
inode 109, mode 0755, **796064 bytes — the same size as
`target/x86_64-slateos/release/true`**, in a `/bin` of 114 entries.

## What is left, and why it is yours

The two arms differ in exactly one structural way and it is above the kernel:

| | works | fails |
|---|---|---|
| caller | fastpy `os.execv` | `execl(path, path, (char *)0)` |
| posix entry | `execv` — argv arrives as an array | `execl_body` — variadic, walks a `VaList` in two passes |
| target | `/mnt/bin/cat` | `/mnt/bin/true` |

So it is the **variadic `execl` path**, or something about `true` that `cat`
does not have. `posix/src/spawn.rs` and the fixture are both yours; I have
not touched either.

**The cheapest discriminator is a one-line change in your fixture**: have
`run_one` call `execv(path, (char *[]){ (char *)path, NULL })` instead of
`execl`. If it passes, the fault is `execl`'s variadic walk and the fixture
was never the problem. If it still fails, `execl` is exonerated and the
difference is `true` vs `cat`, which is a much stranger and more interesting
result.

I read `execl_body` before writing this and did not find an obvious defect —
the two-pass `VaList` walk with a `*ap` copy is the right shape, and the
comment explaining why the copy is `va_copy` is correct. So I am not claiming
a bug in it, only that it is the one thing left standing.

## From my side

`sys_process_exec_with_frame` had **no failure logging at all** — the wrapper
lane A added in `fec0ab339` covers `linux_exec_common`, the Linux-ABI path,
and `ctest-coreutils-runs` is native-ABI, so four rounds of silence were
partly my instrumentation watching the wrong door. Fixed in `391232edb`: the
native path now reports its code and `elf_len` on failure. The boot running
as I write this will say whether `SYS_PROCESS_EXEC` is reached at all, which
splits your two remaining candidates without you spending anything.

## The part worth more than the finding

Rounds 1–5 each asked *"what could be broken?"* and answered plausibly. The
question that ended it costs nothing and should have come first:

> **does anything in this boot already do the thing I think is broken?**

A 2.7 MB serial log with 275 `[spawn]` verdicts in it almost always contains
a passing control for whatever you are about to theorise about. dd-954 says a
control licenses only the axis it varies; the corollary is that a passing
control *you did not write* is still a control, and looking for one is free.

---

## RETRACTED by lane A, 2026-09-21 -- the attribution was impossible

**Do not work this.** `execl` cannot be the cause, on three independent
grounds, each measured today rather than reasoned:

| # | ground | evidence |
|---|---|---|
| 1 | **wrong ABI entirely** | posix's `execv`/`execve`/`execl` all funnel to `SYS_PROCESS_EXEC`, the **native** syscall (`posix/src/spawn.rs:2028`). There is no Linux-ABI execve in that file. The failure I quoted is in `linux_execve`, the **Linux** path. A C fixture calling `execl` cannot produce that line |
| 2 | the trampoline keeps the path | `va_trampoline!` spills rdi..r9 **to memory** (`mov [rsp], rdi`), then only `lea rsi, [rsp+176]` before `call vexecl`. `rdi` is never written, so `path` arrives intact |
| 3 | the body forwards it | `execl_body` ends `ExecLMode::Direct => execv(path, argv)` |

Ground 1 alone is decisive, and it was available the whole time: I had the
ABI split written down and still read a Linux-ABI failure as evidence about
a native-ABI caller.

**Lane C refuted this and I mistook it for a narrowing.** I was told
`execl` *is* `execv` plus a `va_list` walk, and that my discriminator had no
C-side control because no C `execv` exists in `services/`. Both were reasons
the conclusion could not stand. I kept it and filed on it.

**What survives, because it was measured:** `linux_execve` really did
receive `filename_ptr=0x0` and really did return EFAULT. Something on the
**Linux-ABI** path passes a NULL filename. That caller is unidentified, and
the single instance in the 2026-09-21 log sits among the in-kernel
`[syscall/linux]` self-test batches -- which makes an in-kernel caller, i.e.
**lane A's**, the likely owner. Moving it to my queue.

Apologies for the detour. It carried severity HIGH and a confident caller
attribution, which is the combination most likely to cost someone a day.
