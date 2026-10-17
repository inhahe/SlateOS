## 1573. A memfd is an unnamed file on a kernel-internal memfs

**Date:** 2026-10-17 · **Decided by:** Claude (operator-approved scope: A-Q24,
§1566, "programs may change a file by writing to a shared mapping of it"; a
memfd is the file Linux programs most often map that way) · **Lane:** A

**In short:** a Linux program's `memfd_create` gives a file that lives only
in memory. Wayland clients, Mesa, Chromium and PulseAudio make one, map it
shared and writable, and hand it to another process to map too: that is how
they share a buffer. Here a memfd is not a file but a separate kernel object
(`ipc::memfd`), its bytes in a private buffer, so a writable shared mapping
of it answers `ENOSYS` and every one of those programs fails. The memfd
becomes an unnamed file on a memory filesystem of the kernel's own: then a
mapping of it, a seal, a truncate, `fstat`, `sendfile`, an `exec` of it all
take the paths a file takes, writable shared mappings included (§1572), and
the separate object -- about 800 lines, and some 65 special cases in the
Linux system calls -- goes.

### The design

1. **An internal memfs.** One memfs instance the VFS mounts outside every
   mount namespace (Linux's `shm_mnt`): it has a filesystem id and a mount
   id, so its files have stable identities (the page cache's key) and the
   holds on them are counted, but no path reaches it, `/proc/mounts` does
   not list it, and it cannot be unmounted. A memfd does not depend on
   `/tmp` or `/dev/shm` being mounted where the caller looks.
2. **`memfd_create` makes an unnamed file there** (the `O_TMPFILE`
   machinery, `Vfs::create_unnamed_object`, without the path walk) and
   opens it read-write. Mode `0777`, or `0666` with `MFD_NOEXEC_SEAL`, as
   Linux. Not linkable: `linkat` cannot give it a name.
3. **Its name is shown, not used.** `readlink("/proc/self/fd/N")` and
   `/proc/self/maps` show `/memfd:<name> (deleted)`, as on Linux. The
   handle holds the file, so nothing ever resolves that string.
4. **Linux's memfd seals, for memfds.** `fs::sealing` keeps the seals of
   every file (§1526), stricter for files in general: there `WRITE` also
   freezes the size. A memfd's entry follows Linux's rules exactly:
   - `WRITE` refuses writes and writable shared mappings, but lets the
     file be truncated (size is `SHRINK`'s and `GROW`'s business).
   - `FUTURE_WRITE` refuses new writes and new writable mappings, but lets
     an existing writable mapping go on writing.
   - Without `MFD_ALLOW_SEALING`, `SEAL` is set at creation.
   - `fcntl(F_ADD_SEALS)` and `F_GET_SEALS` are accepted for a memfd
     alone; any other file answers `EINVAL`, as Linux.
5. **It goes when its last holder does.** That is an unnamed file's life
   already: the last descriptor, or the last mapping, which holds one.
   Its seal entry goes with it.

### What it costs

memfs keeps a file's bytes in its own buffer, and the page cache keeps a
second copy of each page that is read or mapped (§38, §1572). So a mapped
memfd, like every mapped `/tmp` file today, takes its memory twice. The fix
is memfs keeping its data in the page cache, as Linux's tmpfs does. It is
independent of this change, and better done once for every memfs file.
Known issue `TD-A-MEMFS-PAGES-ARE-KEPT-TWICE` records it.

### The alternatives not taken

| Alternative | For | Against |
|---|---|---|
| **The memfd as a memfs file (chosen)** | One kind of file. Everything a file can do, a memfd does, and nothing is written twice. The separate object and its special cases go. | Mapped pages kept twice until memfs moves into the page cache. A memfd mode in the seal rules. |
| Keep `ipc::memfd`, its bytes in frames it maps itself | No second copy. | A second implementation of a shared file mapping (the fault, `SIGBUS` past the end, unmapping on a cut, `fork`, `/proc/*/maps`) for one object type. Every special case stays, and memfd and file behaviour drift apart, as they have. |
| memfs into the page cache first, then the memfd | The end state in one go. | Larger, and the memfd change works the same either way. Doing it second lets each be tested on its own. |
