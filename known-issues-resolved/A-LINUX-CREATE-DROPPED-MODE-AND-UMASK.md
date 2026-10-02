### A-LINUX-CREATE-DROPPED-MODE-AND-UMASK -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a Linux program creating a file or directory got 0644 or 0755
whatever it asked for. The `mode` argument of `open`, `openat`, `openat2`,
`creat`, `mkdir` and `mkdirat` was dropped, and so was the process's
umask. A key file a program made 0600 was readable by every user, and a
`umask 077` shell made world-readable files.

**Two more faults on the same path, found with it:**
- **`open` read at most 255 bytes of a path.** Anything longer was
  `ENAMETOOLONG`, where Linux allows 4095. A deep build tree's paths are
  longer than that.
- **`openat` relative to a directory descriptor, and `openat2`'s
  `RESOLVE_BENEATH`, refused a name that is not UTF-8** with `EINVAL`. A
  name may hold any byte but `/` and NUL, and plain `open` already took
  such names.

**Where:** `syscall/linux.rs`: `open_common`, `open_kernel_path_install`,
`sys_openat_ex`, `sys_openat_beneath`, `sys_creat`, `mkdir_common`.

**Fixed:**
- `linux_create_mode` takes the twelve permission bits of `mode` and
  clears the caller's umask (`pcb::get_umask`; 022 for a kernel caller).
- Every create in the list passes it: the open family through
  `fs::handle::open_with_mode` and `open_beneath_with_mode`, the mkdir pair
  through `Vfs::mkdir_mode`.
- `open_common` reads its path with `read_user_cstr` up to `PATH_MAX`.
- The installer takes `&Path`, so it takes bytes.

**Test:** `linux::self_test_fs`'s `test_linux_create_modes`:
- `open`, `creat` and `mkdir` with several modes, read back with `stat`;
- the umask of a lent process;
- a 270-byte path, and a 4099-byte one;
- a name holding bytes 0xff and 0xfe.
