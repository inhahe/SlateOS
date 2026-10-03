### [A] exit 11 narrowed: the exec syscall is never reached, so the bug is upstream of exec -- 2026-09-21
**Status:** ROOT-CAUSED 2026-09-24 — the rung, not libc and not exec. `load_elf` begins with `SYS_FS_STAT`, gated on (File, METADATA), and the rung granted READ|EXECUTE only, so the stat was refused and the syscall this entry was waiting for never had a chance. Fix and record: `A-TWO-RUNGS-COULD-NOT-EXEC-FOR-WANT-OF-METADATA` at the end of this file.

**In short:** a test program reports it could not run `/mnt/bin/true`. The
message blames the file. The file is fine, the disk is mounted, the
permissions are granted, and the kernel call that would run it is never even
made. Whatever fails, fails before that.

**Eliminated, each by reading this boot's log rather than by reasoning:**

| candidate | evidence it is not the cause |
|---|---|
| the file is missing | `debugfs`: inode 109, mode 0755, 796,064 bytes. The message's *or is not executable* half is false too |
| `/mnt` is not mounted | `[vfs] Mounted ext4 filesystem at '/mnt' (rw)` |
| the exec syscall fails | **no `[exec] NATIVE exec FAILED` line accompanies the failure**, and none for `linux_execve` either. posix reads the ELF *before* calling `SYS_PROCESS_EXEC`, so the syscall was never reached |
| no capability to open it | the rung grants `(File, 0, READ|EXECUTE)`; the comment beside it records this theory as already tested and dropped |
| something about `/mnt/bin` | **fastpy execs from that exact directory and passes** -- three rungs resolve `cat` over `PATH ["/mnt/bin"]`, one of them `fork`+`execv` in the child |

**So the failure is inside posix's read-the-ELF-then-exec sequence, before
the syscall.** That is a much smaller region than *exec is broken*, which is
where this bug has sat while being misattributed twice -- first to a missing
file, then to `execl` losing its path. Both guesses came from the fixture's
own error text.

**The probe earned its place by proving a NEGATIVE.** `log-exec-argv-regs`
was added to say *why* a native exec failed. Its value here was the absence
of its own output: no line means the syscall was never entered, which
converts a whole class of theories into a fact. A probe that only speaks on
failure is still informative when silent, provided you know it would have
spoken.

**What is needed next, and it is small.** posix's `execv` returns -1 without
saying which step failed -- open, fstat, mmap, read, or the syscall. One
diagnostic naming the failing step would finish this. Filed at lane B as
`requests/a-b-execv-should-say-which-step-failed.md`.
