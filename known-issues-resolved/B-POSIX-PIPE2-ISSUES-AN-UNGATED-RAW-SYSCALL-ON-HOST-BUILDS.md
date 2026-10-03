## `B-POSIX-PIPE2-ISSUES-AN-UNGATED-RAW-SYSCALL-ON-HOST-BUILDS` (lane B, 2026-08-26) — **FIXED**

**In short:** A `SYSCALL` instruction is the CPU instruction that asks the
kernel to do something; which kernel, and which "something", depends entirely
on the machine it runs on. `posix::pipe::pipe2` executed one directly, with
the number 220 — SlateOS's "create a pipe" — and nothing stopped it from
running when the test suite is built for the developer's own PC. So every
`cargo test -p posix` on Windows or Linux asked the *host* kernel to perform
whatever its own syscall number 220 happens to be, with the argument registers
never loaded (whatever values happened to be lying in them). The rest of the
crate had a guard against exactly this, documented at length; `pipe2` was the
one function that hand-rolled the instruction itself and so bypassed it.

**What each host actually did.** Measured, not inferred:

| host | number 220 means | returned | `pipe2`'s verdict |
|---|---|---|---|
| Linux x86-64 | `semtimedop` | a genuine negative `-errno` | error → `-1`, honest |
| Windows x64 | an NT system service | `0xC000_0008` (`STATUS_INVALID_HANDLE`) | **success** |

Windows is the damaging one. NTSTATUS comes back in **EAX**, 32 bits, so
widened to 64 bits `0xC000_0008` is `3221225480` — *positive*. `pipe2`'s only
error check is `if ret < 0`, so an NT failure code was read as a valid handle.
Every pipe on the dev host was registered in the fd table as the pair
`(0xC000_0008, 0)`: the same two fabricated handles, over and over, one of
them an error code and the other zero.

**Why this hid for so long.** The two hosts disagreed, and the quiet one was
the one everybody uses:

- On **Windows** — the host all three lanes actually run `cargo test` on — the
  seventeen affected tests *passed*. They needed only "an fd of kind `Pipe`",
  and the fabricated pair supplied one. Green, for entirely the wrong reason.
- On **Linux** they failed, which is what got reported — and got mis-diagnosed
  as the tests being wrong rather than the code (see the entry above).
- `pipe.rs`'s own tests never caught it because *every one of them passes a
  NULL `pipefd`*, deliberately, so the call returns `EFAULT` before reaching
  the syscall. The module's error paths were thoroughly covered and its
  success path — the half that talks to the kernel and fills in the fd table —
  had no host coverage at all. A test file can have twenty tests and still
  leave the only dangerous line unexecuted.

**The fix** (`posix/src/pipe.rs`, `posix/src/syscall.rs`, `posix/src/file.rs`):

1. `syscall0_ok2(nr) -> (i64, u64)` in `syscall.rs`, carrying the kernel's
   `ok2` two-return-value convention (RAX + RDX). It sits with the other raw
   asm behind the one documented `target_os = "none"` gate, and its host arm
   returns the same `HOST_ENOSYS` sentinel as its siblings. `syscall0` could
   not be reused because it declares RDX nowhere.
2. `host_pipe_sim` in `pipe.rs` — the same shape as the existing
   `host_eventfd_sim` in `epoll.rs` — hands out real, distinct, tracked
   handles on host builds. It deliberately does **not** simulate data
   transfer: nothing needs a host pipe to carry bytes, and `SYS_PIPE_READ`/
   `SYS_PIPE_WRITE` already fail honestly through the gated `syscall3`.
3. Both `file.rs` close sites route through the shared `pipe_kernel_close`, so
   a pipe closed via the fd table and one closed on `pipe2`'s own error path
   release the same simulated handle.

This keeps the seventeen tests *running and meaningful on every host* rather
than gating them off. All four new `pipe2` success-path tests were verified to
be load-bearing by making the simulator return the fabricated Windows pair —
three of the four fail, with the handle-collision test reporting exactly the
condition it was written for.

**Verified:** `cargo test -p posix` 20552 passed / 0 failed on
`x86_64-unknown-linux-gnu` **and** `x86_64-pc-windows-gnu` (was 20531/17 and
20548/0 respectively). Clippy and `rustfmt --check` clean on both plus the
bare-metal `x86_64-unknown-none`. The OS-target arm — which no test can reach
— was checked by reading the generated assembly: `mov eax, 220` / `syscall`,
capturing both RAX and RDX, identical to the code it replaced.
