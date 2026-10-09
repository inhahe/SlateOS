### A-RLIMIT-NPROC-COUNTED-BY-EFFECTIVE-UID -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- fixed on lane-a-wip 2026-10-08, awaiting a boot on main.

**In short:** the limit on how many processes one user may have
(`RLIMIT_NPROC`, what `ulimit -u` sets) counted a process as belonging to
whatever user it was *acting as* (its effective id), not the user it *is*
(its real id). So root that had temporarily switched to user 1000 was held
to user 1000's limit, and a user's process acting as someone else did not
count against its own user at all.

**Where.** `pcb::fork_create`. Both the count and the exemption read
`credentials.uid`, the effective id. Linux's `copy_process` counts by the
real user (`real_cred->user`), and exempts the root user (`INIT_USER`, a
real id of 0) and a caller with `CAP_SYS_RESOURCE` or `CAP_SYS_ADMIN`.
Found while adding real, effective and saved ids (design-decisions 1552),
which made the difference observable.

**Fix (lane-a-wip).** It counts processes by real id, and exempts a real
id of 0 or an effective id of 0 (root's authority, `proc::setid`). Test, in
`syscall::linux`'s rlimit self-test: four checks, each of which comes out
the other way under the old rule -- another process of the same real user
with a different effective id counts; two of another real user with
effective id 1000 do not; an effective 0 is exempt; a real 0 is exempt.

**Reproduce (main).** As root, `seteuid(1000)`, set `RLIMIT_NPROC` to the
number of user 1000's processes, then `fork()`: `EAGAIN`, where Linux forks.
