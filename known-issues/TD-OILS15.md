### TD-OILS15. `osh` `umask` value is tracked but not applied to created-file permissions — OPEN (gated on the target file-mode model)

**Where:** `userspace/oils/src/interp.rs` (`builtin_umask`, `Shell::umask_val`,
`open_out`).

**What:** the `umask` builtin fully implements the value's get/set/print surface
(octal + symbolic, `-S`/`-p`), and the mask is inherited by subshell clones, but
nothing consumes `umask_val` yet. Files created by a redirection (`> file`) are
opened with the OpenOptions defaults; the mask is not subtracted from their
permission bits, and it is not propagated to child processes as a process umask.
On the Windows host (`x86_64-pc-windows-gnu`) there is no Unix mode concept, so
this cannot be exercised there anyway.

**Proper fix:** once SlateOS exposes file modes through its VFS/open path (and a
process-level umask, ideally), (a) subtract `umask_val` from the mode when
`open_out` creates a file, and (b) set the child's umask when spawning external
commands. On a `cfg(unix)` build this can already be wired via
`std::os::unix::fs::OpenOptionsExt::mode(0o666 & !umask)`.
