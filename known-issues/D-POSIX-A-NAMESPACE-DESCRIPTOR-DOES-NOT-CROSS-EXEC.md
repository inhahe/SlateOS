## D-POSIX-A-NAMESPACE-DESCRIPTOR-DOES-NOT-CROSS-EXEC — a namespace's descriptor is lost at `exec` and spawn, and an `exec` leaves its hold behind (lane D, 2026-10-17) — OPEN
**Status:** OPEN -- needs a descriptor-map type for a namespace's handle from lane A (`requests/d-a-a-namespace-handle-needs-a-descriptor-map-type.md`).

**In short:** a program can open a namespace -- `/proc/self/ns/uts` -- and
keep the descriptor, to enter that namespace later with `setns`. On Linux
the descriptor survives `exec` unless it is close-on-exec, and a spawned
child inherits it. Here neither happens: the kernel's descriptor map,
which `exec` and spawn take, has no type for a namespace's handle, so the
library cannot name it there. The new program does not have the
descriptor, and an `exec` does not close it either: the old image's hold
on the namespace stays with the process until it exits. A `posix_spawn`
file action that opens or names one fails (`EOPNOTSUPP`).

**Where:** `posix/src/spawn.rs` -- `crosses`, which keeps it out of the
child's map and the exec close list; `posix/src/nsfs.rs`.

**The proper fix:** a descriptor-map type for a namespace's handle,
accepted by spawn's map, `SYS_PROCESS_SET_EXEC_FDS` and
`SYS_PROCESS_SET_EXEC_CLOSE`, one more holder for a child as for a forked
process; the library then maps `HandleKind::Namespace` to it.
