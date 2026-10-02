### [A] A-VFS-APPEND-RACES: two appends to one file at once could overwrite each other -- 2026-09-26
**Status:** FIXED on lane-a 2026-09-26, awaiting a boot. Found reading the VFS
while bounding the file syscalls' kernel buffers
(`A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`, class 3).

**In short:** when two programs added to the end of the same file at the same
moment -- two services writing one log, say -- both could decide where the end
was before either wrote, and then write at that same place. The second write
silently replaced the first. Nothing reported an error; a line of the log was
just gone. Creating a file by appending to it had the same race: two creators
could each make the file, and the second replaced the first's.

**Where.** `kernel/src/fs/vfs.rs` `Vfs::append` did `Self::stat(path)` for the
size and then `Self::write_at(path, size, data)`. The filesystem lock was taken
inside each call and released between them, so another append could run in
the gap. A missing file took `Self::write_file`, which the other creator could
have run too.

**The fix.** `Vfs::append_resolved` reads the end and writes at it, or creates
the file, under one hold of the filesystem's lock. Its checks and bookkeeping
are `write_file_resolved`'s, less the version-history snapshot, since an append
overwrites nothing. `sys_fs_append` streams its data a 1 MiB chunk at a time,
each chunk one atomic append. So a record up to 1 MiB -- any log line --
never interleaves with another appender's; a longer one may, at chunk
boundaries.

**Not the same guarantee through a handle.** `fs::handle::write` with
`APPEND` still writes at the handle's cached size. Its own comment says it was
never atomic across handles: that needs the handle path to append through the
filesystem too, and it is not changed here.

**Test.** `fs::vfs::self_test_append_is_atomic`: two tasks append 200 five-byte
records each (`A000`..`A199`, `B000`..`B199`) to one file, yielding every
sixteen. The file must then hold exactly the 400 records, each once and whole.
It is probabilistic, since the race needed two appenders interleaved in a
window a boot cannot force, so it catches a regression rather than proving
absence. The absence holds by construction.
