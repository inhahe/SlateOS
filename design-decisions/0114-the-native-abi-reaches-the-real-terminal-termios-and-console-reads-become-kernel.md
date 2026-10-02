## §114 — The native ABI reaches the real terminal: `termios` and console reads become kernel state

**Date:** 2026-08-12

**Decided by:** Claude (autonomous). Mine to revisit; the operator may overrule.

**Context.** §113 unified the foreground process group and observed, in
passing, that the terminal's *other* state was still split. Following that
thread found the same bug shape twice more, both on the native-ABI side:

- **`tcgetattr` answered from a constant.** `posix/src/ioctl.rs`'s
  `handle_tcgets` returned `default_termios()` — cooked mode with echo —
  unconditionally, no matter what the terminal was actually doing.
- **`tcsetattr` was a silent no-op.** `handle_tcsets` accepted the call and
  discarded it. Its comment explained why: *"our console has no configurable
  line discipline"*. That was true when written. It stopped being true when
  `kernel/src/tty.rs` gained `ICANON`/`ISIG`/`ECHO`/`NOFLSH`/`VMIN`/`VTIME`,
  and the comment outlived the fact.
- **`read()` on a console fd bypassed the line discipline entirely.**
  `posix/src/file.rs` issued `SYS_CONSOLE_READ_CHAR`, which reads one raw
  byte straight from the keyboard driver. So a native-ABI program got no
  line editing, no `VEOF`, no `VMIN`/`VTIME`, and — the serious one — no
  `ISIG`: `^C` arrived as byte 0x03 instead of generating `SIGINT`.

Meanwhile the Linux shim had all three for real (`tty::get_termios`,
`tty::set_termios`, and a `dispatch_console_read` driven by
`tty::console_read`). Two programs on the same console, differing only in
ABI, therefore saw two different terminals — and the native one saw a
terminal that could not be configured and could not generate a signal.

**Decision.** Add three native syscalls (541–543) and make libc use them:

- `SYS_TTY_GET_TERMIOS` / `SYS_TTY_SET_TERMIOS` — read and write the same
  `tty::get_termios()`/`tty::set_termios()` the line discipline consults.
- `SYS_TTY_READ` — a console read *through* the line discipline, replacing
  `SYS_CONSOLE_READ_CHAR` as libc's `read()` path for `HandleKind::Console`.

The read implementation is shared, not duplicated:
`handlers::tty_read_into_user` returns a `TtyReadOutcome`, and both the
native `sys_tty_read` and the Linux `dispatch_console_read` encode it. They
differ only in error representation (a `KernelError` versus a negative Linux
errno), which is the one thing the enum leaves to the caller. Terminal-signal
generation likewise moved to the shared `handlers::deliver_console_signal`.
This is the same anti-drift move as §113's `hangup_released_ctty`.

**Alternatives considered.**

- **Make `SYS_CONSOLE_READ_CHAR` itself go through the line discipline.**
  Rejected: it is a *raw single-keystroke* primitive with real callers who
  want exactly that (and returning up to `count` bytes from a syscall
  documented to return one would break them). Its doc comment now says so
  explicitly instead, and points at 543.
- **Give libc its own line discipline in userspace.** Rejected outright —
  it is precisely the fourth-copy mistake §113 was about. The signal
  generation has to happen somewhere that can `kill_pgrp`, which is the
  kernel.
- **Multiplex termios onto an existing syscall rather than spend two
  numbers.** Rejected for the same reason as §113: a multiplexed `ioctl`
  with a command word is exactly the ABI shape this kernel avoids, and
  535–599 is not a scarce range.
- **Reject unknown `c_lflag` bits in `SYS_TTY_SET_TERMIOS`.** Rejected:
  `termios` has no invalid encodings, and every program builds its argument
  by reading the current settings and OR-ing, so rejecting a superset would
  break the normal usage pattern. The Linux shim's `TCSETS` already installs
  what it is given.

**Consequences.**

- Raw mode is real for native-ABI programs: `tcsetattr` changes what the
  next `read()` does, and `tcgetattr` reports it back.
- `^C`/`^\`/`^Z` at a native-ABI program now generate signals for the
  session's foreground group instead of being delivered as data bytes. With
  §113 aiming them at the right group, terminal job control now works from
  *either* ABI.
- **`sys_tty_read` is the first native syscall to emit a restart sentinel.**
  This needed no new *machinery*: `entry.rs` applies
  `resolve_syscall_restart` to both ABIs' dispatch results, so a `^C` meant
  for another process transparently restarts the read, and a `^C` that runs
  a native handler becomes `EINTR` (native handlers have no `SA_RESTART`, so
  restarting would hide the interruption from a program that asked to see
  it). The stale "no native syscall emits a sentinel today" comment in
  `deliver_pending_signal` was corrected. But it did expose two latent
  *encoding* bugs, both unreachable until a native syscall could be
  interrupted:
  - **The native delivery path was folding sentinels into Linux's `EINTR`.**
    `deliver_pending_signal`'s native branch called what was then
    `leaked_sentinel_to_eintr`, which substitutes Linux's `EINTR` = 4, i.e.
    a return value of `-4`. A native process reads its return value as a
    `KernelError` code, and `-4` there is `WouldBlock`/`EAGAIN`. An
    interrupted read would have reported "try again" — the one errno a
    caller most needs to distinguish from "interrupted". Fixed to
    `KernelError::Interrupted` (`-8`), and the helper renamed
    `leaked_sentinel_to_linux_eintr` so the ABI assumption is in the name
    rather than in a comment. The `restart` module stays inside `linux.rs`:
    everything in it *except* that one function is ABI-neutral, so moving
    the module wholesale would be as wrong as leaving it unlabelled.
  - **`posix`'s `errno::native` table was missing `INTERRUPTED` (-8) and
    `DEADLOCK` (-7).** Both fell through to `_ => EIO`, so even after the
    kernel returned the right code libc would have told the program the
    disk had failed. Added, with a test that pins the numeric collision
    (`-EINTR` is bit-identical to `native::WOULD_BLOCK`) so a future
    refactor that merges the two encoding paths fails loudly.
- **Verification had to reach ring 3 to mean anything.** A set/get round
  trip cannot prove the two structs agree — the kernel stores and returns
  whatever it is handed, so *mutually wrong* marshalling round-trips
  perfectly. The proof is `ctest-ctty` checks 90–116, which read
  kernel-authored values (Linux's `INIT_C_CC`, `B38400`) at *musl-computed*
  indices, then set raw mode and confirm the untouched fields did not
  shift. Host `cargo test` sees only the marshalling (the syscall arms are
  `#[cfg(target_os = "none")]`) and the kernel self-test calls the handlers
  directly, never through libc; only ring 3 joins the two halves. A first
  boot run appeared to pass while actually executing the *previous* fixture
  ELF — the full chain (sysroot → fixture build → rootfs image → boot) has
  to be re-run or the test silently proves nothing.
- **`default_termios()` is now `#[cfg(not(target_os = "none"))]`.** It is
  host-test scaffolding, and compiling it out of the bare-metal build is
  what guarantees no target-side path can quietly start answering from a
  constant again — which is the exact failure §114 exists to remove.
- The two wire formats stay distinct on purpose: musl's user `struct
  termios` is 60 bytes (`NCCS` 32, plus `c_ispeed`/`c_ospeed`) and the
  kernel's is 36 (`NCCS` 19, baud carried in `c_cflag`'s `CBAUD` bits).
  `posix` marshals between them exactly as glibc and musl do, so a program's
  struct never has to match the kernel's. Both sizes are asserted in tests.
- What is still *not* here: `SIGTTIN`/`SIGTTOU` enforcement (unchanged from
  §113 — the predicate `pcb::ctty_is_background` exists, the call sites do
  not; **closed by §115**), and there is still no TTY *device* layer (no
  `/dev/tty`, no PTYs). A line discipline is not a tty driver; this closes
  the ABI gap, not the device gap.
