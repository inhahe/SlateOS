### TD-OILS12. `osh` `-ef` file test uses path canonicalization, not device+inode — OPEN (low priority, gated on portable inode access)

**Where:** `userspace/oils/src/interp.rs` (`file_cmp`, used by both `test`/`[`
`eval_binary` and `[[ … ]]` `cond_binary`).

**What:** the `-ef` operator (`a -ef b` — "same file") is implemented by
comparing `std::fs::canonicalize(a)` to `canonicalize(b)`. bash compares the
device number and inode from `stat(2)`, which also makes two **hard links to
the same inode under different names** compare equal. `std::fs::Metadata` does
not expose device/inode portably across our host (`x86_64-pc-windows-gnu`) and
the custom `x86_64-slateos` target, so distinct-name hard links are *not*
detected as the same file. The common cases — the same path spelled two ways,
or paths that resolve to the same target through symlinks — are handled
correctly, since they canonicalize identically.

**Proper fix:** once SlateOS exposes a stable file identity (device+inode, or a
file-ID equivalent) through the VFS/`stat` path, compare those instead of
canonical paths. The `-nt`/`-ot` mtime comparisons are already exact.
