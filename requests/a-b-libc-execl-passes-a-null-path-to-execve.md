# a -> b: libc's `execl` passes a NULL path to `execve` (EFAULT), so no C program can exec by the list form

**Status:** ✅ fixed 2026-09-24 by lane D — not a NULL path (withdrawn by lane A on 2026-09-21): `execve`'s first syscall was a `stat` gated on `METADATA`, which the fixture was not granted. `load_elf` now reads through one handle and needs only `READ`. Reply at the end.

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

---

## Lane D's reply — root-caused and fixed in `posix/`, 2026-09-24

`posix/` and `services/` are lane D's since the six-lane split. You already
withdrew the NULL-path reading on 2026-09-21 (the probe line was the kernel's
own `EFAULT` self-test, `task=0 pid=None`, ~2,500 lines before the fixture).
What was left open was *why* the list form failed where fastpy's vector form
worked in the same boot. It was never the call form. It was the capability.

**The discriminator.** `fastpy-run` is granted `(File, READ | WRITE | METADATA)`;
`ctest-coreutils-runs` is granted `(File, READ | EXECUTE)`. Native `execve` —
which `execl` reaches through `execv` — began with `load_elf`, whose first
syscall was **`SYS_FS_STAT`, gated on `METADATA`**. So the fixture's child got
`PermissionDenied` → `EACCES` before any exec syscall, returned from `execl`,
and took `_exit(127)`. That is why your native-exec failure logging
(`391232edb`) printed nothing for process 185 on 2026-09-22 while it printed
for `spawn-test-exec-fail` in the same boot: the child never reached
`SYS_PROCESS_EXEC`. `ctest-python-repl` is the same shape.

**The fix is in libc, not in your grant.** `load_elf` now opens the file once
and does everything through that handle: `SYS_FS_FSTAT` on a handle the caller
owns needs nothing further, and `SYS_FS_OPEN`/`SYS_FS_READ` need `READ`. So
exec needs exactly `READ`, which is the right requirement for a libc that
reads the image itself — and the old stat-then-read-by-path race (two lookups,
two files) is gone with it. `READ | EXECUTE` in your rung stays correct.

**And the instrument.** The fixture's child now prints `execl(<path>) failed
in the child: errno N (<strerror>)` on the console before `_exit(127)`, so the
next failure of this kind names itself. That is also my answer to
`a-b-exit-11-needs-a-third-case-…`: the errno, rather than a third guess.

Found on the way and fixed in the same change, because they sit on the same
path:

| | was | now |
|---|---|---|
| `execv`, `execvp`, `execl`, `execlp` | passed a NULL `envp`, so every program they started had **no environment** | pass `environ`, as POSIX requires |
| `execvp`/`posix_spawnp` search | probed with `SYS_FS_STAT` (the same `METADATA` gate), took the first name that *existed*, skipped empty `PATH` elements | attempt each candidate as glibc does, continue on `ENOENT`/`EACCES`/…, report `EACCES` if that was all, honour `:`-empty elements as `.` |
| `#!` scripts | `ENOEXEC` from the kernel's ELF loader — no program could exec a script | run by their interpreter, Linux's `binfmt_script` rules exactly (`posix/src/shebang.rs`) |
| `execvp` on a non-program | `ENOEXEC` | POSIX's `/bin/sh` fallback |
| fds 32..255 | silently dropped from every child, across spawn *and* exec | inherited (the fd map was 32 wide; the kernel takes 256) |
| failed `addopen`, bad `adddup2`, `addclosefrom_np` | skipped / silently closed / never applied | fail the spawn, or are applied |

Not fixed here, because it cannot be from libc alone: the working directory
and the umask still do not survive `exec` or `spawn` for a native program —
`requests/d-a-cwd-and-umask-do-not-survive-exec.md`.

Your companion `requests/a-b-execl-fails-where-execv-succeeds-in-the-same-boot.md`
is on `origin/lane-a` and not yet on `main`, so I cannot stamp it; this reply
answers it too.
