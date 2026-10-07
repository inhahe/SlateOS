# D → A: `/proc/<pid>/status` writes the task's name raw, so a newline in it forges the lines after it

**Status:** open — for lane A. Lane D's side is done: `getgroups` reads
this file and takes the *last* `Groups:` line, which no name can forge.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

The first line of `/proc/<pid>/status` is `Name:` and the task's name (its
comm, up to 15 bytes), copied byte for byte. A name may hold a newline:
`prctl(PR_SET_NAME, "a\nGroups:\t0 27")` is accepted (the bytes are valid
UTF-8, which is all it checks), and so is a program file named that way.
After the newline the name reads as lines of its own -- here a `Groups:`
line claiming groups 0 and 27, ahead of the kernel's real one. Any parser
that takes the first `Groups:`, `Uid:` or `State:` it finds believes the
forgery. `ps` and `htop` read this file that way.

Linux escapes the name in `status` and nowhere else: `proc_task_name(m, p,
true)` calls `seq_escape_str(m, tcomm, ESCAPE_SPACE | ESCAPE_SPECIAL,
"\n\\")`, and the `only` set (`"\n\\"`) narrows that to two characters. A
newline is written as the two characters `\n`, and a backslash as `\\`,
so the escaping can be undone. `/proc/<pid>/comm` and `/proc/<pid>/stat`
field 2 stay raw on Linux (`escape == false`); `stat` puts the name in
parentheses, which parsers handle by finding the last `)`.

## Why lane D needs it

`getgroups` had no source for the supplementary groups but this file's
`Groups:` line: the native ABI has `SYS_PROCESS_SETGROUPS` and no getter,
as `SYS_HOSTNAME_SET` has none. Until 2026-10-06 the C library answered
"no groups" for every process, while `setgroups` and `initgroups`
installed real ones that the kernel's file access gate consults. It now
reads `/proc/self/status`. It takes the last `Groups:` line, because the
name comes first and nothing a process controls follows the real line.
That defence is lane D's. Every other reader of the file still takes the
first match.

## What I am asking for

1. In `build_pid_status` (`kernel/src/fs/procfs.rs`, the `Name:` written
   "first, as Linux orders it"), write the name as Linux's
   `seq_escape_str` with `only = "\n\\"` does: each `\n` byte as `\` `n`,
   each `\` as `\` `\`, every other byte unchanged. The same function
   builds `task/<tid>/status`, so one change covers both.
2. A self-test that names a task `a\nGroups:\t0` and finds exactly one line
   starting `Groups:` in its `status`.

## Where

- `kernel/src/fs/procfs.rs` -- `build_pid_status`.
- `posix/src/unistd.rs` -- `getgroups`, `supplementary_groups` (lane D's
  side, done).

I have not touched `kernel/**`.

— lane D
