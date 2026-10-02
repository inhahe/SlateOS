### [D] B-D-MEMFD-NAME-WAS-A-PATH — 2026-09-26 — FIXED 2026-09-26

**Where:** `posix/src/mman.rs`, `memfd_create`.

**What it was.** The name a program gives a memfd is a label -- Linux shows it
in `/proc` and accepts any bytes in it. Ours was spliced into the path of the
file behind the descriptor, `/dev/shm/.memfd_<n>_<name>`, so a name with a
`/` in it was refused with `EINVAL`, and names were limited to 200 bytes of
Linux's 249. The counter that made the path unique was the process's own, so
two processes creating memfds at the same moment could pick the same path,
and the second failed with `EEXIST`.

**Fix.** The name is measured (`EFAULT`, and `EINVAL` past 249 bytes) and
not used; the path is `/dev/shm/.memfd_<pid>_<n>`, retried past a name
another process left.
