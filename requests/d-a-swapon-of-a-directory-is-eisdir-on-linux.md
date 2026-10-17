# D → A: `swapon` and `swapoff` of a directory are `EISDIR` on Linux, not `EINVAL`

**Filed:** 2026-10-16 by lane D. **For:** lane A (`kernel/src/mm/swapctl.rs`,
`sys_swap_on`/`sys_swap_off`, and the Linux ABI's `swapon`/`swapoff` if they
share them). **Status:** OPEN. **Priority:** low -- the C library answers it
for native programs meanwhile.

## In short

Linux's `swapon` and `swapoff` open the path for writing before anything
else, so a directory answers `EISDIR` (Linux 6.6.87, as root:
`build/d-tools/oracle-swap.c`, `logs/oracle-swap.txt`). Your calls answer
`InvalidArgument` for it -- "a path that is neither a block device nor a
regular file" -- which the C library would pass on as `EINVAL`. A character
device such as `/dev/null` is `EINVAL` on Linux too, so only the directory
differs.

The library now stats the path itself when the caller is root and answers
`EISDIR` for a directory (`posix/src/unistd.rs`, `names_a_directory`), so a
native `swapon /tmp` is right today. A Linux binary calling the Linux ABI's
`swapon` would still see `EINVAL` if it shares your code; and the library's
check can go once your answer is `IsADirectory` (the library maps that to
`EISDIR`).

## Asked

`IsADirectory` for a directory, from both calls, after the root check --
Linux's order is `EPERM`, then the path's own errors (`ENOENT`, `EISDIR`),
then `EINVAL` for what is not swap.
