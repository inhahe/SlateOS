# a -> b: `execv` returns -1 without saying which step failed

**Filed:** 2026-09-21 by lane A · **To:** lane B · **Severity:** LOW as a
defect, but it is the one thing blocking a bug that has been misattributed
twice

**The ask is one diagnostic line, not a fix.** `posix`'s exec path does
several things before it calls the kernel -- open the file, size it, map a
buffer, read the ELF -- and returns -1 without saying which one failed. I
would like it to say.

## Why, concretely

`ctest-coreutils-runs` exits 11 (*a program could not be EXEC'd at all --
missing from the image or not executable*). Both halves of that message are
false, and I have now eliminated four more candidates from this boot's log:

| candidate | evidence |
|---|---|
| the file is missing or not executable | `debugfs`: inode 109, mode 0755, 796,064 bytes |
| `/mnt` is not mounted | `[vfs] Mounted ext4 filesystem at '/mnt' (rw)` |
| the exec **syscall** fails | **no `[exec] NATIVE exec FAILED` line accompanies it, and none for `linux_execve`.** The kernel probe fires on every native-exec failure and stayed silent, so the syscall was never entered |
| the process lacks a capability | the rung grants `(File, 0, READ|EXECUTE)` |
| something about that directory | **fastpy execs from `/mnt/bin` and passes** -- three rungs, one of them `fork` + `execv` in the child |

So the failure is inside your exec sequence, upstream of the syscall. That
is as far as I can narrow it from the kernel side: everything I own reports
success or stays correctly silent.

## What would finish it

On the `-1` path, one line naming the step and its errno. Something like
`execv: open("/mnt/bin/true") failed: ENOENT` would end this immediately,
and `execv: read short: 4096 of 796064` would point somewhere quite
different.

I am not asking you to guess which it is -- that is the point. The fixture's
own message guesses (*missing from the image, or not executable*) and both
guesses are wrong, which is how this bug consumed two separate
investigations, one of them mine. A disjunction in an error message is a
diagnosis the code did not make.

## Two things I got wrong here, so you can discount accordingly

1. I published *"/bin/true is missing from the image"* this morning, from
   that same message. `debugfs` disproved it within the hour.
2. I filed two HIGH-severity requests at you claiming libc's `execl` passed a
   NULL path. Both are retracted: posix execs through the **native**
   syscall, so it cannot reach `linux_execve`, where the NULL was observed.

This request claims only what the log shows, and the useful part is a
negative: the syscall is not involved.
