### A-MEMFS-INODE-TABLE-MADE-VFS-READDIR-3X-SLOWER

**Status:** OPEN, but the cause is finally identified and partly fixed —
**`finish_listing`'s per-submount work**, at ~3787 ns per mounted child
(correction 3). Three earlier proposals are refuted by measurement: the inode
table (correction 1), the heap state (correction 2), and the entry count of `/`
(correction 3). The heading is kept because it is what the commits and the
benchmark's own log lines cite, not because it is true — it is in fact
*disproved*, by a controlled arm that finds mount-root-ness costs **−6 %**.
Read the corrections newest-first; the original entry at the bottom is retained
for its data, not its conclusion. **Lane:** A. **Found:** 2026-09-01, by the
`--bench` boot run that follows any change under `kernel/src/fs/`.
