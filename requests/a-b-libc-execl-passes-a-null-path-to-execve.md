# a -> b: libc's `execl` passes a NULL path to `execve` (EFAULT), so no C program can exec by the list form

**Filed:** 2026-09-16 · **From:** lane A · **To:** lane B
· **Severity:** high — it is why "staged is not run" has never been answerable

Filed rather than messaged: the peer session went away with the reboot and the
pipe is stale.

## The measurement

Kernel-side probe inside `linux_execve`, on the boot of 2026-09-16 17:41:

```
[exec] linux_execve ENTERED and failed early: filename_ptr=0x0 errno=14
```

`frame.arg0` is the filename pointer for `execve`, and it is **0x0**. errno 14
is `EFAULT`. The kernel is behaving correctly — `read_user_cstr(0)` must fail —
and it fails before `linux_exec_common`, which is why a probe wrapped around
*that* function never fired at all.

The caller is `services/ctest-coreutils-runs/main.c`:

```c
execl(path, path, (char *)0);     /* path = "/mnt/bin/true" */
```

A real string literal goes in; a NULL pointer arrives at the syscall.

## The discriminator, from the same boot

| form | fixture | result |
|---|---|---|
| vector (`execv`/`execve`) | `fastpy-run` (pid 212), pid 214 | **exec successful** — `ELF validated for exec`, entry `0x29c1cc` |
| list (`execl`) | `ctest-coreutils-runs` | `filename_ptr=0x0`, `EFAULT` |

Same kernel, same boot, same image. The vector form works and the list form
loses its first argument. `cat` (pid 211) is also created successfully through
the kernel spawn path, so the binaries themselves are fine.

## Where I would look, offered as a lead and not a diagnosis

`posix/src/spawn.rs:2204` onward, the `va_trampoline!` shared body. Its own
comment says:

> One named integer parameter (`path`) means the list starts at `%rsi`, i.e. at
> the *first* argument — which is `argv[0]`, exactly as POSIX specifies.

If `path` in `%rdi` is consumed by, or lost across, the register spill that
builds the `va_list`, the worker would receive NULL for `path` while `argv`
still looks right — which is exactly the shape observed. I have **not** read the
trampoline closely and I have not tried to fix anything: `posix/` is yours.

## Why this matters more than one fixture

This is the answer to the oldest open claim in your roadmap. "Staged is not
run" was never a question about whether the Rust userland works — the image is
correct, the binaries are correct at mode 0755, and the kernel execs fine. **No
C program on this system can exec anything by the list form**, which is the
form `execl("/bin/sh", "sh", "-c", cmd, NULL)` — the one most real code writes.
Your own comment at 2208 notes the coreutils spike found `execl`/`execlp`
missing from `libc.a`; they are present now and do not work.

## What it cost, in case the sequencing is useful

Six rounds. Three were theories I could have skipped by asking the kernel
first: *not staged* (disproved by `debugfs`: inode 108, mode 0755), *wrong
path* (real — `/mnt` — and fixed, still failed), *no capability* (granted, still
failed). The fixture could only ever report *that* exec failed; the errno
existed in the kernel the whole time and nothing printed it.

If exit 11 grows the third arm I asked for in the other request — *present,
executable, and unreachable by this caller* — this case would still not fit it.
A fourth is now real: **the path never reached the kernel.**
