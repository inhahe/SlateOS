### A-SPAWNED-CHILD-RUNS-AS-ROOT -- 2026-10-08 -- FIXED 2026-10-09 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-08, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264)
(design-decisions 1552, section 3).

**In short:** a program started with spawn ran as root (user id 0), whoever
started it. So a program a user's shell started could read any file on the
disk, past its permission bits, and a Linux program among them could switch
to any user. Fork was not affected.

**Where.** `proc::pcb::Process::new` gives every process record root's
credentials, and `proc::spawn::spawn_process` changed them only when a kernel
caller named a uid (`SpawnOptions::uid_gid`: a container's `User`, the kernel
shell). The native `SYS_PROCESS_SPAWN*` calls, the libc's `posix_spawn` among
their users, never do. `fs::vfs` lets uid 0 through every permission check
(`check_acl`, `path_access_verdict`), and the Linux `setuid` family took uid 0
as privilege. The child did get only its parent's capabilities, and a user's
table has none of root's (§1502). That is why the uid was the whole hole.

**Fix (lane-a-wip).** `pcb::inherit_credentials`: a spawned child takes its
parent's credentials as a fork and an exec leave them. The spawn fails if the
parent's credentials cannot be read. A kernel-spawned child is still root.
Test: `spawn::test_spawn_inherits_credentials`.

**Reproduce (main).** As a non-root process, spawn any program and have it
call `getuid()`: it answers 0.
