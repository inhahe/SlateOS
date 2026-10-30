## 1590. A Linux driver process loads firmware through Linux's own loader, with its own authority, and takes module parameters from its arguments

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577), and `request_firmware` is in roadmap 2.1's core shim; how it is done is Claude's call, under §1578 · **Lane:** A

**In short:** Many devices -- WiFi cards above all -- need a blob of their
maker's code, called firmware, loaded into them by their driver each time
they start. A Linux driver asks for it by file name
(`request_firmware("iwlwifi-cc-a0-77.ucode")`), and Linux finds the file
in `/lib/firmware`. A driver process now does the same with Linux's own
loader, which reads the file through SlateOS's ordinary file calls, as
the process itself: a driver reads firmware only if it was given the
right to read files. And the settings Linux drivers take from Linux's
boot command line -- module parameters, such as `iwlwifi.power_save=1`
-- are now the driver process's arguments; the loader's own
`firmware_class.path=<dir>` names a directory to look in first, as on
Linux.

### The decision

1. **Linux's own firmware loader, compiled in**
   (`drivers/base/firmware_loader/main.o` and `builtin/main.o`,
   `linuxdrv/emul/manifest`): `request_firmware()` and every variant --
   direct, without warnings, into the driver's buffer, part of a file from
   an offset, asynchronously -- and its sharing of a file asked for twice.
   Its search is Linux's: `firmware_class.path`, then
   `/lib/firmware/updates/<release>`, `/lib/firmware/updates`,
   `/lib/firmware/<release>`, `/lib/firmware`. As configured
   (`allnoconfig`'s defaults): no user-space fallback
   (`FW_LOADER_USER_HELPER`), no compressed firmware, nothing built in.
2. **The file read, the layer's**: `kernel_read_file_from_path_initns()`
   (`linuxdrv/emul/lx_fs.c`), every check of Linux's `kernel_read_file()`
   kept -- a directory or an empty file `-EINVAL`, a whole file larger
   than the buffer `-EFBIG`, an offset without a buffer `-EINVAL`, a file
   that shrank while read `-EIO` -- over the kit's `slx_file_open`,
   `_stat`, `_read` and `_close` (`linuxdrv/kit/src/files.rs`:
   `SYS_FS_OPEN` with `DIRECTORY_ALLOWED`, as Linux's open for reading
   takes a directory, `SYS_FS_FSTAT`, `SYS_FS_PREAD`, `SYS_FS_CLOSE`).
   The kernel's codes are translated to Linux's errnos as the kernel's
   own Linux entry points translate them (`linux_errno_for`); the kit's
   tests read the kernel's tables, so the two cannot drift apart.
3. **The process's own authority.** Linux reads firmware as the kernel --
   kernel credentials, init's mount namespace -- whoever asked. A driver
   process has one authority, its own: it needs a `File` capability with
   `READ`, and reads as whom it runs. The credentials Linux's loader
   swaps in (`prepare_kernel_cred()`, `lx_emul.c`) are a record kept for
   its count, nothing more. That right is not yet confined to the
   firmware: `known-issues/A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED.md`.
4. **Module parameters from the arguments.** Linux's own
   `kernel/params.c` is compiled in, and its `parse_args()` sets each
   argument `<module>.<parameter>=<value>` (`lx_emul.c`'s `lx_params`):
   a module's own (`module_param`) before anything but memory is set up,
   as `start_kernel()` sets them; each initcall level's just before that
   level runs, as `do_initcall_level()` -- the initcalls now run level by
   level, each bounded in `linuxdrv/link.ld` as Linux bounds it. An
   argument holding a space is quoted for `parse_args()`, as the command
   line would quote it. Arguments with no dot in their names are the
   layer's own (`pci=`, `serve=`, `stacks=`) and a driver's mode words,
   left alone. A dotted name no parameter has, or a value its kind
   refuses, is said by `parse_args()` and ends the process before any
   initcall (`SLX_EXIT_ARGUMENT`, 118): a mistyped option fails at once
   rather than doing nothing.

### Alternatives

- **The layer's own `request_firmware()`**, a hundred lines of C: fewer
  Linux files, but each variant rewritten -- the asynchronous one's
  workqueue, the buffer and partial ones, the sharing, the search order,
  the warnings -- imitating Linux rather than running it. §1578 prefers
  Linux's code wherever it runs as it is, and this does: the loader
  needed only the file read, credentials, a path-name cache and a reboot
  notifier chain from below.
- **The firmware handed to the process** by whoever starts it -- the
  files, or a directory opened for it: confines the driver to its
  firmware. But a driver picks names at run time (`iwlwifi` tries
  `-77.ucode`, then `-76`, down to the oldest it supports), so the
  starter would have to know them, and the kernel has no capability for
  "this directory only" yet. The known issue above keeps it as a fix.
- **Parameters left unsettable**, as the layer had them (their kinds
  defined without operations): every driver at its compiled defaults.
  Real drivers' users need some (`i915.enable_guc`,
  `iwlwifi.11n_disable`), and the loader's own path is one.
- **Unknown parameters ignored**, as Linux hands them on to `init`: a
  typo would silently do nothing, and a driver process has no `init` to
  hand them to.

### Checked by

`linuxdrv/hello` (`hello_firmware_init`), run by the kernel's self-test
(`linuxdrv::firmware_runs`) over a directory it writes in `/tmp`: the
parameters set, each at its level; the layer's read at each of its
edges; Linux's loader whole, shared, into a buffer, a file's tail from an
offset, asynchronously, a missing file `-ENOENT`; with no `File`
capability, nothing read (`-EACCES`); an unknown parameter and a refused
value each ending the process with 118.

### Left

- Compressed firmware (`.zst`, `.xz`), which distributions increasingly
  ship: `FW_LOADER_COMPRESS_ZSTD` and Linux's zstd, when a driver's
  firmware comes that way.
- Confining the right to read files (the known issue above).
