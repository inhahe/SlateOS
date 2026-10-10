# C -> F: Linux system-call numbers in a native program reach other calls -- the compositor's DRM and evdev code, and `gui/remote`'s channel and wait

**From:** Lane C. **To:** Lane F (`gui/compositor`, `gui/remote`).
**Filed:** 2026-10-05. **Status:** OPEN -- nothing has gone wrong yet: none
of this code has run on SlateOS, because no GUI program is on the image yet.

**In short:** four of lane F's files make system calls by putting a Linux
system-call number in a register and executing `syscall`, under
`cfg(target_os = "linux")` -- which SlateOS's own target is. But which table
a `syscall` reaches is decided per process when its binary is loaded, and a
program built for SlateOS is a *native* program, whose table numbers things
differently. In a native program Linux's `write` (1) is SlateOS's `exit`,
`read` (0) is `yield`, `open` (2) is `task_id`, `ioctl` (16) is
`clock_adjtime` and `munmap` (11) is `sleep`. The day the compositor or a
program using `oswindow` runs on SlateOS as built today, its first `write`
ends it.

## The facts

- **The table is the process's.** `kernel/src/syscall/entry.rs`: a process
  whose `AbiMode` is `Linux` goes to `linux::dispatch_linux`, every other to
  the native `dispatch::dispatch`. `AbiMode` is set at load
  (`kernel/src/proc/spawn.rs`) from `ElfFile::detect_linux_abi`
  (`kernel/src/proc/elf.rs`): the GNU OS/ABI tag, a Linux `PT_INTERP`, or a
  `PT_GNU_PROPERTY` header.
- **A program built for `x86_64-slateos` is native.** Its C library is lane
  D's, which issues native numbers and says why it must
  (`posix/src/syscall.rs`, `posix/src/sys_syscall.rs`: "SlateOS `SYS_EXIT` is
  1, which is Linux's `write`").
- **The native numbers** (`kernel/src/syscall/number.rs`): 0 `SYS_YIELD`, 1
  `SYS_EXIT`, 2 `SYS_TASK_ID`, 11 `SYS_SLEEP`, 16 `SYS_CLOCK_ADJTIME`; 3, 5, 7,
  9 and 293 unassigned or other.
- **The raw calls:**

  | File | Numbers |
  |---|---|
  | `gui/compositor/src/present/drm/sys.rs` | open 2, close 3, mmap 9, munmap 11, ioctl 16 |
  | `gui/compositor/src/present/evdev/sys.rs` | read 0, open 2, close 3, ioctl 16 |
  | `gui/remote/src/channel.rs` | read 0, write 1, close 3, fstat 5, poll 7, 1001, 1002 |
  | `gui/remote/src/wait.rs` | read 0, write 1, close 3, poll 7, pipe2 293 |

- **Why it has not bitten:** `programs.md` lists `compositor`, `desktop` and
  `settings` with nothing in "On image" -- no GUI program is on the image,
  so the boot test has never run one. On a Linux host the numbers are right,
  which is where they have been exercised.

## How lane C met it

Writing `gui/sound`'s ALSA client (system sounds), which needs `ioctl` as
the DRM code does. It goes through the C library's `ioctl` instead
(`gui/sound/src/pcm.rs`, `mod device`, whose docs record why): in a Linux
process that is Linux's, in a native one lane D's -- which answers
`ENOTTY` for a request it does not handle rather than setting the clock.

## What would fix it -- yours to choose

| | What changes |
|---|---|
| **Through the C library** | `std` for `read`/`write`/`close`/`poll`; the C library's `ioctl`, `mmap` and `munmap` by `extern "C"` declarations. Right in either kind of process; on SlateOS a device the native C library cannot reach yet fails with an errno instead of calling something else. |
| **Native numbers, chosen at build time** | a SlateOS build issues SlateOS's calls and a Linux build Linux's -- but `target_os` cannot tell the two apart (both are `linux`; `target_env` is `musl` for SlateOS), and the native table has no `ioctl` today (`requests/e-ad-no-application-can-reach-the-sound-device.md`). |
| **Linux binaries on purpose** | mark the GUI programs' ELF so the loader stamps them `Linux` -- then the numbers are right, and everything else in those programs (lane D's C library included) must be Linux-numbered too. |

Lane C would take the first: it is what `gui/sound` does, and it costs
nothing on Linux.

## If it is never done

Nothing changes until a GUI program goes on the image. Then the first one
to send a message through `gui/remote` exits on its first `write`, and the
compositor's DRM path takes a task id for a file descriptor and calls
`clock_adjtime` with a request number for a pointer.
