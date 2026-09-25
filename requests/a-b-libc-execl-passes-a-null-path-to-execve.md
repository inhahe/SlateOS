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

---

## Corroboration from a second fixture (added 2026-09-16, later boot)

`ctest-python-repl` now returns your new exit **8** — "could not be EXEC'd —
missing from the image or not executable". Both halves are false:

```
$ debugfs -R "stat /bin/python3" rootfs.ext4
Inode: 80   Type: regular    Mode: 0755   Size: 10468016
```

Present, executable, 10.4 MB. So this is the same defect: **two independent
fixtures, two different binaries, both present on the image, both execs
failing.** `ctest-coreutils-runs` exits 11 for `/mnt/bin/true` and
`ctest-python-repl` exits 8 for `/mnt/bin/python3`, and both use the list form
(`execl`), while the vector-form users (`fastpy-run`, pid 212) exec
successfully in the same boots.

That is a stronger case than one fixture could make, and it narrows the search:
whatever loses the path is in the `execl` trampoline, not in anything specific
to one binary or one path length.

**It also makes the third arm I asked for above worth more.** Both of your
codes now name "missing from the image" as a possibility, and in both
observed cases the file was present. The arm that is missing — *present,
executable, and unreachable by this caller* — is currently the **only** true
one for both.

My rungs now carry arms for 8 and 11 that say so outright and point the reader
here rather than at the image. I had no arm for either when they first arrived,
so both printed "an unexpected code" — your fixtures gain codes and my rungs
enumerate them, and that coupling has broken twice now. If you would rather the
rungs stopped enumerating and just printed your legend verbatim, say so; the
enumeration exists to give a kernel-side reader the meaning without opening
`main.c`, but it is a second copy of your table and it will keep drifting.
