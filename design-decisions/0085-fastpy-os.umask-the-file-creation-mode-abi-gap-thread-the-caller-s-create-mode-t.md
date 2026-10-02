## 85. fastpy `os.umask` + the file-creation-mode ABI gap — thread the caller's create mode through a *new backward-compatible syscall number* rather than overloading the existing one (initiative F)

**Decided by:** Claude (autonomous)

**Context.** `os.umask` was untestable-as-real because of a deeper gap: the
native VFS create path **ignored the caller's mode entirely**. `SYS_FS_OPEN`
(610) and `SYS_FS_MKDIR` (604) took no mode argument, so every newly-created
file was stamped a hardcoded `0o644` and every new directory `0o755`,
regardless of what the caller requested or what the umask was. A program could
`umask(0o022)` then `open(path, O_CREAT, 0o777)` and still get `0o644` on disk —
the umask (and the mode) had **zero observable effect**. Making umask real
therefore first requires closing the create-mode ABI gap. Two sub-decisions
fall out: (a) **how** to thread the mode through without breaking already-built
binaries, and (b) **where** umask masking is computed.

**Decision.**
1. **New syscall numbers, not an overload.** Add `SYS_FS_OPEN_MODE` (659,
   arg3 = create mode) and `SYS_FS_MKDIR_MODE` (660, arg2 = mode) as *distinct*
   numbers. The old `SYS_FS_OPEN`/`SYS_FS_MKDIR` keep their exact prior
   default-mode behavior. The kernel treats an arg mode of `0` as "unspecified →
   historical default" (`0o644` file / `0o755` dir), so a non-`O_CREAT` open that
   passes 0 never creates a `0o000` file.
2. **umask lives in userspace.** The posix create wrappers compute
   `mode & ~umask` (`apply_umask`) and pass the **final on-disk permission bits**
   to the kernel, which stamps them verbatim. `UMASK_VALUE` is a process-local
   static in the posix libc (default `0o022`); the kernel has no umask concept.
   This matches decisions #83 (setuid) and #84 (nice): the kernel is a thin
   mutation primitive, POSIX policy is userspace.

**Rationale.** Overloading arg3 of the *existing* `SYS_FS_OPEN` (my first
attempt) is a real regression: `syscall3` only loads rdi/rsi/rdx and `syscall2`
only rdi/rsi, but the kernel populates `SyscallArgs` from **all** arg registers
uniformly. Every already-built embedded ELF (there are ~48 fastpy self-test
binaries plus the whole userspace) calls the old `open`/`mkdir` **without**
setting the mode register — so an overload would make them pass whatever garbage
was left in that register as the file mode, corrupting permissions on file
creation *during boot*. A new number is the only backward-compatible way: stale
binaries keep hitting the old default-mode handler, and only freshly-rebuilt
posix wrappers use the mode-carrying number. Keeping umask masking in userspace
keeps the kernel free of per-process mask state and keeps the on-disk-bits
contract identical whether a file is created via the native ABI or any future
one.

**Alternatives considered.**
- *Overload arg3/arg2 of the existing syscalls*: rejected — breaks every
  pre-built binary (garbage mode register → corrupted create permissions at
  boot), as caught before committing.
- *Kernel-side umask state* (a per-process mask the kernel subtracts): rejected —
  adds per-process kernel state for a pure-POSIX-policy concept, and would
  duplicate the masking logic the userspace wrapper already needs for the
  host-test build. Thin primitive + userspace policy is consistent with #83/#84.
- *Mode 0 means literally 0o000*: rejected — a non-`O_CREAT` open legitimately
  passes 0 (no create), so 0 must mean "use the default", not "create a
  no-permission file". (Documented limitation: a mask that computes to exactly
  `0` falls back to `0o644`; vanishingly rare and harmless.)

**Where it lives.** `kernel/src/syscall/number.rs` (consts + rationale docs),
`kernel/src/fs/handle.rs` (`DEFAULT_CREATE_MODE`, `open_with_mode`),
`kernel/src/fs/vfs.rs` (`DEFAULT_DIR_MODE`, `mkdir_mode`),
`kernel/src/syscall/handlers.rs` (`sys_fs_open_mode`/`sys_fs_mkdir_mode`),
`kernel/src/syscall/dispatch.rs`, `posix/src/syscall.rs` (const mirror),
`posix/src/file.rs` (`apply_umask` + rewired `open`/`mkdir` wrappers). fastpy
lowering: `compiler/codegen.py`, `runtime/objects.c`, `runtime/runtime.c`.
Self-test: `kernel/src/proc/spawn.rs::self_test_fastpy_slateos_umask`; tool:
`services/fastpy-umask/` (umask(0o077)→prior 18, umask(0o022)→prior 63, then
`os.open(mode 0o777)` under mask 0o022 must yield `0o755` on disk — distinct
from both the `0o644` default and the `0o777` request, so neither a
"umask-ignored" nor a "mode-ignored" bug can false-pass).

**How to reverse.** To move umask into the kernel, add a per-process mask to the
PCB and subtract it in `sys_fs_open_mode`/`sys_fs_mkdir_mode`, then drop
`apply_umask` from the posix wrappers. To retire the old numbers once every
binary is rebuilt, point `SYS_FS_OPEN`/`SYS_FS_MKDIR` at the mode handlers with a
default mode and delete 659/660.
