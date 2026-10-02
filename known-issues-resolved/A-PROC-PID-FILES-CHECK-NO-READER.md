### A-PROC-PID-FILES-CHECK-NO-READER -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** any program can read what `/proc/<pid>/` says about any other
program, whoever runs either of them. That includes `environ` (the other
program's environment variables, where passwords and access tokens are often
passed), `auxv` and `maps` (where its code and data sit in memory, which
undoes address randomisation), `io`, the `cwd`/`root`/`exe` links, and since
2026-10-01 `wchan`. Linux lets only a reader allowed to trace the program
(the same user, with the program not marked undumpable, or an administrator)
read those. Nothing is exposed in practice yet -- every process still runs as
uid 0, the "administrator" who may read them anyway -- but the first login
service that starts a second user's programs makes this a real leak between
users.

**Where:** `kernel/src/fs/procfs.rs` -- `generate_pid`, `generate_task` and
`ProcFs::readlink` serve every file to every reader; `ProcFs::stat` reports
no owner or mode, so the VFS has nothing to check either.

**Fixed** (design-decisions 1516): one predicate, `pcb::may_inspect`, as
Linux's `ptrace_may_access(PTRACE_MODE_READ_FSCREDS)`. The reader may inspect
if it is the kernel, the process itself, or uid 0; or has the target's uid
and gid while the target is dumpable; or holds a `Process` capability for it
with `READ`.
- Refused to anyone else (`EACCES`): `environ`, `auxv`, `maps`, `io`, the
  `cwd`/`root`/`exe`/`fd/<n>` links, `fd/` and `fdinfo/`.
- Blanked instead: `wchan` reads `0` and `stat` field 35 is 0, as on Linux.
- Writing `oom_score_adj` needs the same rule with `WRITE`, and lowering it
  needs uid 0.

**Still open:** `stat` of these files reports no owner or mode, so
`ls -l /proc/<pid>` does not show Linux's `0400`. The VFS's directory entry
has no field for them; the check when a file is generated is the
enforcement either way.
