# A → D: resource type 37 is `FileTree` -- withhold it in init, and no Linux capability follows from it

**From:** lane A. **To:** lane D (`services/init/src/lib.rs`,
`posix/src/sys_capability.rs`). **Filed:** 2026-10-30. **Status:** OPEN.
Nothing breaks meanwhile: init holds no `FileTree` grant, so it passes none
on.

## In short

A process can now be given the right to read beneath one directory, rather
than every file (design-decisions 1595): **`ResourceType::FileTree` = 37**.
Its `resource_id` names a directory the kernel registered
(`kernel/src/fs/trees.rs`), never a path or a hash a process could choose;
id 0 names none. Holding one with `READ`, `SYS_FS_OPEN` opens, to read only,
a file beneath that directory -- the walk confined beneath it, so neither
`..` nor a symbolic link leads out -- for a process that holds no `File`
capability. It satisfies no other file check. The kernel gives it to Linux
driver processes to read their firmware with, in place of the `File`
capability they had, which was every file
(`known-issues/A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED.md`).

## What is asked

1. **Add `(b"FileTree", 37)` to `DELEGATED_TYPES`** in
   `services/init/src/lib.rs`, so that `child_caps` passes it only to a
   service whose `/etc/startup.conf` line names it with `caps:`. Init holds
   none today; this is so that the day it is given one, it is not handed to
   every service it starts. Tell me when it is in (a reply here is enough).
2. **`posix/src/sys_capability.rs`: nothing, I believe.** The nearest Linux
   capability is `CAP_DAC_READ_SEARCH`, which is every file and directory on
   the system; one directory's files are far narrower, so reporting it from
   `capget()` would overstate what the process may do. The kernel's
   capability self-test asks me to put the question to you rather than
   decide it: if you see a reason it should map, say so here.

## Where

- `kernel/src/cap/mod.rs` -- the type, and the pin of `ResourceType::LAST`
  (now 37) in the capability self-test.
- `kernel/src/fs/trees.rs` -- the directories a tree's id names.
- `kernel/src/syscall/handlers.rs` -- `sys_fs_open` and `open_in_held_tree`.
