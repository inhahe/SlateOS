## 31. `access(2)` family semantics — grant F_OK/R_OK/X_OK for any existing file under the no-DAC capability model (consistent with `execve` ignoring x-bits)

**Date:** 2026-06-16

**Decided by:** Claude (autonomous) — reversible; the operator may overrule. Made
while wiring the `access`/`faccessat`/`faccessat2` syscalls (Path Z Part 34) to
get unmodified GNU `make` running, after `strace` showed make calls
`access(shell, X_OK)` before spawning a recipe and bails on failure.

**Context:**
`sys_access`/`sys_faccessat`/`sys_faccessat2` were stale stubs returning
`ENOENT` unconditionally (a "we have no backing filesystem" skeleton from before
the VFS was writable). With a real VFS this is simply wrong: every existence and
accessibility probe failed. GNU make issues `access("/bin/sh", X_OK)` *before*
spawning its recipe shell and, on failure, reports `"/bin/sh: No such file or
directory"` + `Error 127` without spawning — so the stub blocked the entire
toolchain initiative.

The open question once the path is resolved through the VFS: what does `X_OK`
(and `W_OK`) *mean* in an OS whose security model is capability-based, not Unix
DAC?

**Options:**
- **A — Faithful Unix mode-bit check.** Read the file's mode bits and grant
  X_OK only if an execute bit is set, W_OK only if a write bit is set, etc.
  *Con:* our memfs/FAT report `permissions == 0` (no Unix bits), so a faithful
  X_OK on a staged binary would *fail* even though `execve` of that same binary
  *succeeds* (execve ignores on-disk x-bits here). That inconsistency is worse
  than no check: `access(X_OK)==fail` then `execve==ok` breaks make's own logic.
- **B (chosen) — No-DAC: existence implies F_OK/R_OK/X_OK.** Resolve the path;
  if it exists, grant read/execute. This is *consistent* with what a subsequent
  `execve`/`open` actually does in this OS (authority comes from capabilities,
  not file mode bits). `W_OK` is granted unless the backing FS is known
  read-only (then `EROFS`, as on Linux).

**Reasoning:** Authority in this OS is conferred by capabilities, not by file
owner/group/other permission bits — and `execve` already ignores the on-disk
execute bits. The *only* self-consistent answer for `access(X_OK)` is therefore
"grantable iff the file exists," matching the `execve` that the caller is about
to perform. Option A would make `access` and `execve` disagree, which is exactly
the failure mode that breaks real programs. The check still resolves the path
through the VFS, so a *missing* file correctly returns `ENOENT`.

**Known limitation (tracked):** `W_OK` does not yet consult per-mount read-only
state (we don't track it at this layer), so writes are always granted — see
known-issues.md. A read-only mount should return `EROFS` for `W_OK`.

**How to reverse:** if a real per-user/per-mode policy is ever needed, gate
`X_OK`/`W_OK` on the actual mode bits (and a future read-only-mount flag) in
`access_path_common`; the path-resolution plumbing stays unchanged.
