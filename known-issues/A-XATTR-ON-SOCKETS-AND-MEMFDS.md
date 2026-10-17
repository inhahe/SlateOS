### A-XATTR-ON-SOCKETS-AND-MEMFDS -- 2026-10-01 -- OPEN (lane A)

**Status:** OPEN for sockets. The memfd half is fixed on lane-a-wip
2026-10-17, awaiting a boot on main: a memfd is a memfs file now
(design-decisions §1573), which keeps attributes as any file does, and
`xattr_unkept` serves the sockets alone.

**In short:** on Linux a memfd is a tmpfs file and keeps extended
attributes, and a socket answers one, `system.sockprotoname` (its
protocol's name: `TCP`, `UDP`, `UNIX`). Here neither keeps any: past the
namespace rules, `fgetxattr` and `fsetxattr` on them are `EOPNOTSUPP` and
`flistxattr` is empty.

**Where:** `kernel/src/syscall/linux.rs`, `xattr_unkept`.

**The fix:** a memfd's attributes beside its pages, through the same
`fs::xattr_policy`; a socket's `system.sockprotoname` from the socket's
kind -- read-only, listed, and `system.` allowed for that one name.
