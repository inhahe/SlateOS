# D → A: a namespace's handle needs a descriptor-map type, so that its descriptor crosses `exec` and spawn

**Filed:** 2026-10-17 by lane D. **For:** lane A (`fd_handle_type`,
spawn's fd map, `SYS_PROCESS_SET_EXEC_FDS`, `SYS_PROCESS_SET_EXEC_CLOSE`).
**Status:** OPEN. **Priority:** low -- nothing breaks; a namespace kept
open across `exec` is lost, and its hold outlives the descriptor.

## In short

The C library opens `/proc/<pid>/ns/<kind>` through your
`SYS_NAMESPACE_OPEN` into a descriptor of its own kind. Linux keeps such a
descriptor across `exec` (unless close-on-exec) and hands it to a spawned
child; container tools open a namespace, `exec` a helper, and the helper
`setns`es into it. Here the descriptor map that `exec` and spawn take has
no type for a namespace's handle, so the library leaves it out -- of the
child's map, and of the exec close list, which refuses a type it does not
know. So the new image lacks the descriptor, and the old one's hold stays
with the process until it exits
(`known-issues/D-POSIX-A-NAMESPACE-DESCRIPTOR-DOES-NOT-CROSS-EXEC.md`).

## Asked

An `fd_handle_type` for a namespace's handle, which spawn's fd map,
`SYS_PROCESS_SET_EXEC_FDS` and `SYS_PROCESS_SET_EXEC_CLOSE` accept: a
child one more holder, as a forked process is, and an `exec` that drops the
descriptor giving its hold back. The library then maps
`HandleKind::Namespace` to it.
