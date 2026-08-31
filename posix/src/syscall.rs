//! Raw syscall primitives.
//!
//! Provides inline assembly wrappers for issuing our native syscalls
//! from userspace via the x86_64 SYSCALL instruction.
//!
//! ## ABI
//!
//! ```text
//! RAX = syscall number
//! RDI = arg0, RSI = arg1, RDX = arg2, R10 = arg3, R8 = arg4, R9 = arg5
//! Return: RAX (negative = error code)
//! ```
//!
//! This matches the Linux x86_64 syscall convention.

// ---------------------------------------------------------------------------
// Native syscall numbers (must match kernel/src/syscall/number.rs)
// ---------------------------------------------------------------------------

pub const SYS_EXIT: u64 = 1;
pub const SYS_TASK_ID: u64 = 2;
// Process ID lives in the kernel-process zone (500–599), not kernel-core.
// (Previously mis-numbered as 3, which the kernel does not implement —
// getpid() was hitting an unimplemented syscall.  See number.rs.)
pub const SYS_PROCESS_ID: u64 = 502;
pub const SYS_CLOCK_MONOTONIC: u64 = 10;
pub const SYS_CLOCK_REALTIME: u64 = 14;
pub const SYS_CLOCK_SETTIME: u64 = 15;
pub const SYS_CLOCK_ADJTIME: u64 = 16;
pub const SYS_SLEEP: u64 = 11;

/// Fill a userspace buffer with kernel-CSPRNG bytes.
///   `(buf_ptr, len)` -> bytes written, capped at 1 MiB per call.
///
/// The only source of key-grade randomness available to userspace: the
/// kernel's ChaCha20 pool is seeded from RDRAND/RDSEED, TSC jitter, the HPET
/// and interrupt timing, none of which an unprivileged process can see.
/// See [`crate::random`].
pub const SYS_GETRANDOM: u64 = 90;

// Console I/O
pub const SYS_CONSOLE_WRITE: u64 = 100;
pub const SYS_CONSOLE_READ_CHAR: u64 = 101;

// Kernel log ring buffer (read-only).
//   READ: (after_seq, buf_ptr, buf_cap) -> (entry_count in value,
//         newest_seq in value2).  Pass `u64::MAX` as after_seq to read
//         from the oldest available entry; otherwise reads entries with
//         seq > after_seq.  Fills the buffer with JSON-lines text (one
//         JSON object per line, each terminated with `\n`).  Non-consuming.
pub const SYS_LOG_READ: u64 = 102;

// Memory management.
//
// These MUST match the kernel's *native* syscall table
// (kernel/src/syscall/number.rs), NOT the Linux-ABI numbers.  They were
// previously mis-numbered 30/31/32, which collide with the kernel's IRQ
// syscalls (SYS_IRQ_REGISTER=30, SYS_IRQ_WAIT=31, SYS_IRQ_RELEASE=32) —
// so a native mmap() actually hit the capability-gated IRQ register path
// and came back PermissionDenied (-400).  That silently broke the crt's
// main-thread TLS setup on native binaries (the fastpy/initiative-F path).
pub const SYS_MMAP: u64 = 20;
pub const SYS_MUNMAP: u64 = 21;
pub const SYS_MPROTECT: u64 = 22;

// Scheduler / thread
pub const SYS_SCHED_SET_PROFILE: u64 = 53;
pub const SYS_CPU_COUNT: u64 = 55;
pub const SYS_PHYS_PAGES_TOTAL: u64 = 56;
pub const SYS_PHYS_PAGES_AVAIL: u64 = 57;
pub const SYS_LOADAVG: u64 = 58;
pub const SYS_CPU_TIMES: u64 = 59;

// Process management
pub const SYS_PROCESS_SPAWN: u64 = 500;
pub const SYS_PROCESS_WAIT: u64 = 501;
pub const SYS_PROCESS_EXEC: u64 = 503;
pub const SYS_PROCESS_TRY_WAIT: u64 = 507;
/// POSIX `waitpid`: `arg0` = pid selector, `arg1` = options
/// (`WNOHANG`/`WUNTRACED`/`WCONTINUED`), `arg2` = `*mut i32` wstatus.
/// Returns the pid whose state changed, or 0 for a `WNOHANG` miss.
///
/// Backs `process::waitpid`. Distinct from `SYS_PROCESS_WAIT` (501) because
/// that one returns the exit *code* in rax, which cannot express a
/// job-control stop — every value is a legitimate exit code — and cannot
/// grow an options argument its existing callers do not set.
pub const SYS_PROCESS_WAIT_STATUS: u64 = 1063;
pub const SYS_PROCESS_IS_READY: u64 = 509;
pub const SYS_THREAD_CREATE: u64 = 510;
pub const SYS_THREAD_EXIT: u64 = 511;
pub const SYS_THREAD_JOIN: u64 = 512;
pub const SYS_PROCESS_KILL: u64 = 506;
pub const SYS_PROCESS_SPAWN_EX: u64 = 517;
/// Spawn with an explicit capability policy. arg0 = `*const SpawnEx2Args`.
///
/// [`SYS_PROCESS_SPAWN_EX`] (517) gives the child the parent's *entire*
/// capability table, which is right for a shell starting a helper it trusts
/// and wrong for one starting a program it does not — "everything I can do"
/// is not a sandbox. 559 is how a parent says "just these".
///
/// A separate number rather than fields appended to `SpawnExArgs`, because
/// that struct carries no length and no version and the kernel reads
/// `size_of::<SpawnExArgs>()` bytes from the pointer: appending would make it
/// read 16 bytes past every existing caller's 96-byte struct and interpret
/// them as a pointer and a count. The `clone3` escape — size in a second
/// register, 0 meaning "legacy" — is unavailable because [`syscall1`] sets
/// only `rax` and `rdi`, so the kernel's `arg1` holds whatever the caller
/// left in `rsi`. See `design-decisions.md` §279 (lane A — why a new number)
/// and §363 (lane B — how userspace reaches it).
///
/// Purely additive: 517 is untouched and still inherits everything.
pub const SYS_PROCESS_SPAWN_EX2: u64 = 559;
pub const SYS_PROCESS_GET_INITIAL_FDS: u64 = 518;
pub const SYS_PROCESS_GET_ARGS: u64 = 519;
/// Record the current userspace fd table so it survives `execve`
/// (the kernel hands it back to the new image via
/// `SYS_PROCESS_GET_INITIAL_FDS`).  arg0 = `FdMapEntry` array ptr,
/// arg1 = entry count.
pub const SYS_PROCESS_SET_EXEC_FDS: u64 = 1061;
pub const SYS_PROCESS_PARENT_ID: u64 = 520;
pub const SYS_PROCESS_COUNT: u64 = 521;
/// Get the calling process's real uid/gid, packed as gid<<32 | uid.
pub const SYS_PROCESS_GET_CREDENTIALS: u64 = 529;
/// Set the calling process's real uid/gid (arg0=uid, arg1=gid;
/// 0xFFFF_FFFF = leave unchanged). Kernel enforces the permission rule.
pub const SYS_PROCESS_SET_CREDENTIALS: u64 = 530;
/// Get the calling process's scheduling nice value, biased by +20
/// (result 0..=39 ⇒ nice -20..=19). Never fails.
pub const SYS_PROCESS_GET_NICE: u64 = 531;
/// Set the calling process's scheduling nice value (arg0 = nice biased
/// by +20, 0..=39) and apply it to the scheduler. Returns the previous
/// nice, biased by +20. CAP_SYS_NICE policy is enforced in userspace.
pub const SYS_PROCESS_SET_NICE: u64 = 532;

// Process groups / sessions (533–536)
//
// These reach the same kernel `proc::pcb` state the Linux ABI shim's
// setpgid/getpgid/setsid/getsid use.  Before they existed the process-group
// functions in `process.rs` kept a userspace guess (a `static mut PGID`),
// which meant a parent and its child could disagree about the child's own
// group — see `known-issues.md`.

/// Move a process into a process group.  arg0 = target PID (0 = caller),
/// arg1 = destination PGID (0 = the resolved target PID).  Returns 0, or a
/// negative error code (`NoSuchProcess` for a dead target,
/// `PermissionDenied` if a POSIX gate rejects the move).
pub const SYS_PROCESS_SET_PGID: u64 = 533;
/// Query a process's process-group ID.  arg0 = target PID (0 = caller).
/// Returns the PGID, or `NoSuchProcess`.
pub const SYS_PROCESS_GET_PGID: u64 = 534;
/// Start a new session led by the caller (`sid = pgid = pid`).  No
/// arguments.  Returns the new SID, or `PermissionDenied` if the caller
/// already leads a process group.
pub const SYS_PROCESS_SET_SID: u64 = 535;
/// Query a process's session ID.  arg0 = target PID (0 = caller).
/// Returns the SID, or `NoSuchProcess`.
pub const SYS_PROCESS_GET_SID: u64 = 536;

// Controlling terminal (537–538)
//
// Same story as 533–536, one layer up: the foreground process group belongs
// to a *session*, so a shell and the job it foregrounds must be able to read
// one shared value.  This crate used to keep it in a per-process `FG_PGRP`
// static, which no other process could observe or contradict.
//
// Neither call takes a terminal argument: there is exactly one console, so
// the caller's session names the terminal unambiguously.  `tcgetpgrp`/
// `tcsetpgrp` still take an `fd` because POSIX says so, and validate it here.

/// Read the foreground process group of the caller's controlling terminal.
/// No arguments.  Returns the PGID, `NotSupported` (ENOTTY) if the caller's
/// session has no controlling terminal, or `NoSuchProcess`.
pub const SYS_TTY_GET_PGRP: u64 = 537;
/// Hand the caller's controlling terminal to a process group.  arg0 = the
/// destination PGID, which must name a live group in the caller's own
/// session.  Returns 0, `InvalidArgument`, `NotSupported` (ENOTTY), or
/// `PermissionDenied`.
pub const SYS_TTY_SET_PGRP: u64 = 538;
/// Claim the console as the caller's session's controlling terminal
/// (`ioctl(fd, TIOCSCTTY)`).  No arguments.  Returns 0, or
/// `PermissionDenied` if the caller is not a session leader or another
/// session already holds the console.
pub const SYS_TTY_ACQUIRE_CTTY: u64 = 539;
/// Give up the caller's session's controlling terminal
/// (`ioctl(fd, TIOCNOTTY)`).  No arguments.  Hangs up the foreground group
/// (`SIGHUP` + `SIGCONT`) on success, as Linux's `disassociate_ctty` does.
/// Returns 0, `NotSupported` (ENOTTY), or `PermissionDenied` if the caller
/// is not the session leader.
pub const SYS_TTY_RELEASE_CTTY: u64 = 540;
/// Read the console's `struct termios` (`tcgetattr`/`ioctl(fd, TCGETS)`).
/// `arg0` is a pointer to a 36-byte buffer.  Returns 0 or a negative error.
/// This reaches the terminal's *real* line-discipline settings; `tcgetattr`
/// used to answer from a hardcoded constant in `ioctl.rs`.
pub const SYS_TTY_GET_TERMIOS: u64 = 541;
/// Install a new console `struct termios` (`tcsetattr`/`ioctl(fd, TCSETS)`).
/// `arg0` is a pointer to a 36-byte buffer.  Returns 0 or a negative error.
/// This is what makes raw mode real; `tcsetattr` used to silently discard it.
pub const SYS_TTY_SET_TERMIOS: u64 = 542;
/// Read from the console through the line discipline (`read(2)` on a tty).
/// `arg0` is the destination buffer, `arg1` its capacity.  Honours `ICANON`,
/// `VEOF`, `VMIN`/`VTIME` and `ISIG` — unlike [`SYS_CONSOLE_READ_CHAR`],
/// which returns one raw keyboard byte and so delivered `^C` as byte 0x03.
/// Returns the byte count (0 at EOF) or a negative error.
pub const SYS_TTY_READ: u64 = 543;

// ---------------------------------------------------------------------------
// Pseudo-terminals (544–556)
// ---------------------------------------------------------------------------
//
// Built by lane A and described in
// `requests/a-b-pty-the-tty-layer-is-now-n-devices-and-a-pty-object-exists.md`.
//
// # How these name a terminal
//
// Three of them — `SYS_PTY_{GET,SET}_WINSIZE` and `SYS_PTY_{GET,SET}_TERMIOS` —
// take a *terminal* rather than a pty end, and so do the handle-taking forms of
// `SYS_TTY_ACQUIRE_CTTY`.  One convention covers all of them:
//
// | `arg0` | Means |
// |---|---|
// | `0` | the caller's **controlling terminal** — the console for a process that was never re-parented onto a pty, so this is the pre-pty behaviour unchanged |
// | `1` | **invalid, reserved** — a handle is `(tty_id << 1) \| end`, so `1` decodes as "the slave of tty 0", and tty 0 is the console, which has no slave |
// | `>= 2` | a pty handle the caller owns, either end |
//
// The ownership check on `>= 2` is real and is the point: a pty handle is
// *enumerable*, unlike every other kernel handle here, whose value is
// unguessable — and a master handle is the authority to type arbitrary bytes at
// whatever shell is on the far end.  The check happens **before** the buffer
// argument is looked at, so a caller cannot learn whether a pty exists by
// probing numbers and watching which errno comes back.
//
// # Why there is no "open the slave" syscall
//
// Linux hands you the master from `/dev/ptmx` and makes you find the slave by
// name.  That shape is forced by the slave being a *filesystem path* an
// unrelated process might open first, and `grantpt`/`unlockpt`/`TIOCSPTLCK`
// exist to guard the window between creating the pty and the intended user
// opening it.  `SYS_PTY_CREATE` returns **both ends**, which closes that window
// rather than guarding it.  See [`crate::ioctl::posix_openpt`] for what libc
// does with the slave end it is handed before anyone has asked for it.

/// Create a pseudo-terminal, returning **both** ends: master in `rax`, slave in
/// `rdx` (`syscall*_2ret`).  Takes no arguments.
///
/// Returns: the pair; `OutOfMemory` if the pty id space is exhausted.
pub const SYS_PTY_CREATE: u64 = 544;
/// Write "keystrokes" into a pty — bytes the slave's line discipline sees as
/// terminal *input*.  `arg0` = master handle, `arg1` = bytes, `arg2` = length.
///
/// Blocks while the input ring is full, so a paste into a slow program gets
/// back-pressure rather than a silent truncation; the returned count may be
/// short and the caller must resume from it.
///
/// Returns: bytes accepted; `InvalidHandle`; `ChannelClosed` (→ `EPIPE`) if the
/// slave is closed, because nothing will ever read these bytes.
pub const SYS_PTY_MASTER_WRITE: u64 = 545;
/// Read program output from a pty's master end, blocking.  `arg0` = master
/// handle, `arg1` = buffer, `arg2` = capacity.
///
/// Bytes have already been through output post-processing, so a `\n` written by
/// the program arrives as CRLF under `ONLCR`.
///
/// **At last-slave-close this returns `IoError` (EIO), not 0** — see
/// `design-decisions.md` §259.  Buffered output is delivered first.  A libc that
/// "helpfully" turned that EIO into a zero-length read would break every
/// emulator ported from Linux, which reads `0` as "nothing right now" and spins;
/// [`crate::file`] therefore passes the `EIO` through.
pub const SYS_PTY_MASTER_READ: u64 = 546;
/// Non-blocking [`SYS_PTY_MASTER_READ`]: `WouldBlock` (→ `EAGAIN`) instead of
/// parking when the output ring is empty and the slave is still open.  Used
/// when the master fd carries `O_NONBLOCK`.
pub const SYS_PTY_MASTER_TRY_READ: u64 = 547;
/// Write program output into a pty from its slave end.  `arg0` = slave handle,
/// or `0` for the caller's controlling terminal; `arg1` = bytes, `arg2` = len.
///
/// The counterpart of [`SYS_TTY_READ`]: a program running *on* a pty reads its
/// terminal with 543 and writes it with this.  `OPOST`/`ONLCR` are applied by
/// the kernel, because only the terminal knows a line break is two bytes, and
/// the returned count is in *the caller's* bytes rather than the expanded ones
/// — so a short write can be resumed from without re-sending half a CRLF.
pub const SYS_PTY_SLAVE_WRITE: u64 = 548;
/// Drop one reference to a pty end.  `arg0` = the handle.
///
/// Last *master* reference: the slave's readers see EOF, its writers get
/// `IoError`, and every session holding it as a controlling terminal has its
/// foreground group hung up (`SIGHUP` then `SIGCONT`).  Last *slave* reference:
/// the master drains and then reports `IoError`.
///
/// Returns: 0; `InvalidHandle` if the caller does not own the handle — which is
/// also what a second close of the same value gets, since the reference was
/// already given up.
pub const SYS_PTY_CLOSE: u64 = 549;
/// Take another reference to a pty end.  `arg0` = the handle.
///
/// Returns the **same** raw value with the refcount bumped: a pty end has one
/// identity, and two names for it would make "the last close" ambiguous.  This
/// is what [`crate::fdtable`] needs for `dup`, and what a shell about to `fork`
/// uses so the child's exit does not hang up the parent.
pub const SYS_PTY_DUP: u64 = 550;
/// Report the terminal id behind a pty handle, for `ptsname(3)`.  `arg0` =
/// either end's handle; returns the `TtyId`, which libc formats as
/// `/dev/pts/<id>`.  A *name*, not a way to obtain the end.
pub const SYS_PTY_SLAVE_ID: u64 = 551;
/// Report whether a pty end would read or write without blocking.  `arg0` = the
/// handle; returns a bitmask, bit 0 readable and bit 1 writable.
///
/// **Hangup counts as readable**, because a read at hangup returns immediately
/// and a poller that called it "not ready" would never notice the terminal had
/// gone — the same rule Linux's `poll` follows when it sets `POLLHUP` alongside
/// `POLLIN`.
pub const SYS_PTY_POLL: u64 = 552;
/// Read a terminal's window size (`ioctl(fd, TIOCGWINSZ)`).  `arg0` = terminal
/// under the naming convention above, `arg1` = a `struct winsize` out-buffer.
pub const SYS_PTY_GET_WINSIZE: u64 = 553;
/// Set a terminal's window size, raising `SIGWINCH` on a real change
/// (`ioctl(fd, TIOCSWINSZ)`).  Arguments as for [`SYS_PTY_GET_WINSIZE`].
///
/// The handle form exists for the terminal *emulator*, which owns a master end
/// that is emphatically not its own controlling terminal and is the only party
/// that knows the window was resized.  Without it, dragging a window's corner
/// could not reach the program inside — the one thing `SIGWINCH` exists to
/// prevent.
pub const SYS_PTY_SET_WINSIZE: u64 = 554;
/// Read a terminal's `struct termios` by handle.  `arg0` = terminal, `arg1` = a
/// wire-format out-buffer of the kernel's `TERMIOS_BYTES` — the same encoding
/// `SYS_TTY_GET_TERMIOS` uses, so `ioctl.rs`'s existing `termios_from_wire`
/// decodes it unchanged.
///
/// The handle form is what lets `openpty(3)` install a discipline on the slave
/// *before* forking, which is the only race-free time to do it — the reason
/// `openpty` takes a `termp` argument at all.
pub const SYS_PTY_GET_TERMIOS: u64 = 555;
/// Install a terminal's `struct termios` by handle.  Arguments as for
/// [`SYS_PTY_GET_TERMIOS`].
pub const SYS_PTY_SET_TERMIOS: u64 = 556;

// ---------------------------------------------------------------------------
// Resource limits (557-558) — the kernel's per-process `Process::rlimits`
// ---------------------------------------------------------------------------
//
// **557/558, not 544/545.**  `requests/b-a-native-rlimit-syscalls.md` proposed
// the pair at 544/545 on the strength of "544-599 is entirely free"; that was
// true when it was written and the pty block above landed in between.  Writing
// `SYS_RLIMIT_GET = 544` here would not fail to build and would not fail at
// run time either — it would create a pty and hand back a handle where an
// rlimit was expected, which is the failure shape a wrong syscall number
// always has.  The numbers came back from lane A in
// `requests/a-b-native-rlimit-syscalls-landed.md`.
//
// The buffer both calls use is byte-identical to Linux's `struct rlimit64`, so
// `crate::resource::Rlimit` (`#[repr(C)]`, two `u64`s) can be handed over as
// itself rather than marshalled.

/// Read one resource limit.  `arg0` = target pid (**`0` means the caller**),
/// `arg1` = resource number in `0..=15` (the Linux `RLIMIT_*` numbering),
/// `arg2` = pointer to a 16-byte `[rlim_cur: u64, rlim_max: u64]` buffer to
/// fill.  Returns 0 or a negative `KernelError`.
///
/// `arg1` is **narrowed** to `u32` by the kernel rather than rejected, matching
/// how the x86-64 ABI truncates an `unsigned int` argument: `1 << 32` names
/// resource 0.  A probe that expects `InvalidArgument` from a huge ordinal will
/// not get one.
///
/// Errors: `InvalidArgument` (null buffer, `resource >= 16`),
/// `PermissionDenied` (any pid that is neither `0` nor the caller's own),
/// `NoSuchProcess` (the caller named *itself* and its own PCB is gone), or a
/// fault error if the buffer is not writable.
///
/// **A foreign pid is `PermissionDenied` even when no such process exists**,
/// deliberately: answering `NoSuchProcess` for a dead pid and
/// `PermissionDenied` for a live one would make this call a process-existence
/// oracle for any process on the system.  Linux's `prlimit64` does distinguish
/// them and [`crate::linux_rlimit`] keeps doing so, because reproducing Linux's
/// observable behaviour is that layer's whole job; the native ABI is not
/// obliged to inherit the leak.  See §723.
pub const SYS_RLIMIT_GET: u64 = 557;

/// Write one resource limit.  Arguments as for [`SYS_RLIMIT_GET`], with `arg2`
/// pointing at the 16-byte pair to install.
///
/// Additional errors over the read: `InvalidArgument` if `rlim_cur >
/// rlim_max`; `PermissionDenied` if `rlim_max` is above the existing hard
/// limit (**every** raise is refused today, for every resource — nothing yet
/// projects `ResourceType::ResourceLimit` into a raise permission), or if
/// `RLIMIT_NOFILE`'s `rlim_max` is above the kernel's `MAX_FDS`.
///
/// The `RLIMIT_NOFILE` ceiling is absolute and is checked separately from the
/// blanket no-raise rule, so it survives the day the blanket rule is relaxed.
/// In particular `setrlimit(RLIMIT_NOFILE, {RLIM_INFINITY, RLIM_INFINITY})` is
/// refused, not accepted: the kernel reads `RLIM_INFINITY` as "skip the fd
/// check", so accepting it would switch off the only thing standing between a
/// program and an `EMFILE` it had been told could not happen.  Daemons that
/// lift their own NOFILE to infinity at startup must handle the refusal.
///
/// Gate order is **resource before pid** on both calls, so that a caller
/// probing whether a resource number is understood gets the same answer
/// whoever they are.  [`crate::linux_rlimit`]'s `prlimit64` keeps Linux's own
/// order (copy-in, pid, permission, resource); the two ABIs agree on outcomes,
/// not on which of two simultaneous errors wins.
pub const SYS_RLIMIT_SET: u64 = 558;

// The three later additions (869–871) are numbered apart from the 544–556 block
// because they were added after it closed, not because they differ in kind.

/// Count the bytes readable on a pty end, for `ioctl(fd, FIONREAD)`.  `arg0` =
/// either end's handle; returns the count.
///
/// **How exact the answer is depends on the end and the discipline**, and a
/// caller that cares must be told rather than left to assume:
///
/// | End | Mode | Answer |
/// |---|---|---|
/// | master | — | exact |
/// | slave | raw | exact |
/// | slave | canonical | **upper bound** |
/// | either | anything | **zero is exact** |
///
/// The master's count is of *post*-discipline bytes, i.e. after `ONLCR` has
/// expanded newlines, so a four-byte slave write containing one newline reports
/// 5 — which is the number a reader must size by to avoid stranding the `\r`.
/// The canonical slave's is of *pre*-discipline bytes: the line editor has not
/// run, so an erase will consume a byte rather than deliver one, and an
/// unterminated line delivers nothing at all until its newline arrives.
///
/// Only the upper bound is ever wrong, and it is harmless: `read()` returns what
/// is actually there regardless of what this said.  **Zero is exact everywhere**,
/// which is the property that makes this usable for the common caller — a
/// polling loop testing emptiness.
///
/// **A hung-up end with an empty buffer answers 0, not an error**, and
/// deliberately differs from [`SYS_PTY_POLL`], where hangup sets the readable
/// bit.  "Would a read return immediately" is yes at hangup; "how many bytes are
/// there" is none, and this call's caller believes the number.
pub const SYS_PTY_READABLE_BYTES: u64 = 869;
/// Read a terminal's foreground process group by handle, for `TIOCGPGRP` on a
/// master.  `arg0` = the terminal; returns the pgid.
///
/// **This is not a widened [`SYS_TTY_GET_PGRP`] (537), and could not have
/// been.** libc invokes 537 as `syscall0`, which never writes `rdi`, so giving
/// `arg0` a meaning would read whatever the caller happened to leave in that
/// register — sometimes 0, sometimes a live handle naming an unrelated
/// terminal.  A compatibility break that fails nondeterministically with the
/// caller's register allocation is one nobody would ever have diagnosed.  537
/// and 538 are unchanged and remain correct for the slave.
///
/// **`arg0 == 0` is `ENOTTY`, not the console.**  Unlike 553–556, "my terminal"
/// is not a useful reading here: a daemon has no foreground process group, and
/// answering with the console's would report a group it has no relationship to
/// as its own.
///
/// **A terminal nobody has claimed is also `ENOTTY`** — a pty whose slave has
/// not yet run `TIOCSCTTY` genuinely has no foreground group, and a caller must
/// read that as "nothing is running in there yet" rather than receive a `0` it
/// might try to signal.
pub const SYS_PTY_GET_PGRP: u64 = 870;
/// Set a terminal's foreground process group by handle, for `TIOCSPGRP` on a
/// master.  `arg0` = the terminal, **`arg1` = the pgid** — note the shift from
/// [`SYS_TTY_SET_PGRP`] (538), where the pgid is `arg0`.
///
/// **The group is validated against the terminal's session, not the caller's.**
/// For a master those are different sessions by construction, so validating
/// against the caller would be simultaneously too strict and too lax: it would
/// reject every group actually running on the pty, and accept groups from the
/// emulator's own unrelated session — the terminal-theft case the POSIX rule
/// exists to prevent, merely pointed the other way.
///
/// **`SIGTTOU` follows the terminal, not the caller.**  An emulator holding a
/// master is neither foreground nor background in that terminal's session, and
/// stopping it for being a background job on some *other* terminal would
/// deadlock — the emulator is often exactly the process that would have to be
/// resumed to make itself foreground.
pub const SYS_PTY_SET_PGRP: u64 = 871;

// POSIX signal shim (522–526)
pub const SYS_SIGNAL_REGISTER: u64 = 522;
pub const SYS_SIGNAL_SEND: u64 = 523;
pub const SYS_SIGNAL_RETURN: u64 = 524;
pub const SYS_SIGNAL_MASK: u64 = 525;
pub const SYS_SIGNAL_PENDING: u64 = 526;

/// Stop the calling process for job control, on a disposition this crate
/// has *already* resolved to the POSIX `Stop` default action.  `arg0` is
/// the stop signal (SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU), recorded as the
/// parent's `WSTOPSIG`.  Returns 0 once a `SIGCONT` resumes us.
///
/// Not expressible as `SYS_SIGNAL_SEND` to self: the kernel does not hold
/// our `sigaction` table, only whether a trampoline is registered — and one
/// always is — so it would mark a catchable stop signal pending for handler
/// delivery, landing back in the dispatcher that just resolved it to
/// `SIG_DFL`.  See the kernel's `SYS_SIGNAL_STOP_SELF` docs.
pub const SYS_SIGNAL_STOP_SELF: u64 = 1062;

/// Fork the calling process (copy-on-write).  Returns the child PID to
/// the parent and 0 to the child, or a negative error code to the
/// parent on failure.
pub const SYS_PROCESS_FORK: u64 = 527;

/// Set the calling thread's `fs_base` (the x86-64 thread pointer / TLS
/// base).  `arg0` is the new base address, which must be < 2^47.  The
/// kernel writes `IA32_FS_BASE` and persists the value on the task so it
/// survives context switches.  Native counterpart of Linux
/// `arch_prctl(ARCH_SET_FS, addr)`.  Used by the crt to install
/// main-thread ELF TLS on a native (aux-vector-less) static binary.
///
/// Returns: 0 on success, or a negative error code (InvalidArgument if
/// the address is out of range).
pub const SYS_SET_FS_BASE: u64 = 528;

// Filesystem
pub const SYS_FS_READ_FILE: u64 = 600;
pub const SYS_FS_WRITE_FILE: u64 = 601;
pub const SYS_FS_DELETE: u64 = 602;
pub const SYS_FS_LIST_DIR: u64 = 603;
pub const SYS_FS_MKDIR: u64 = 604;
/// mkdir with a caller-supplied (umask-masked) permission mode in arg2.
/// Separate number from `SYS_FS_MKDIR` so the 2-arg ABI stays intact for
/// already-built binaries.  See kernel `sys_fs_mkdir_mode`.
pub const SYS_FS_MKDIR_MODE: u64 = 660;
pub const SYS_FS_RMDIR: u64 = 605;
pub const SYS_FS_STAT: u64 = 606;
pub const SYS_FS_LINK: u64 = 607;
pub const SYS_FS_STATVFS: u64 = 608;
// Advisory whole-file locks (flock).  FLOCK args: (path_ptr, path_len,
// lock_type, owner) where lock_type 0=shared, 1=exclusive and owner is
// the lock-holder ID (we use the process ID).  Non-blocking: returns
// WouldBlock on contention.  FUNLOCK args: (path_ptr, path_len, owner).
pub const SYS_FS_FLOCK: u64 = 609;
pub const SYS_FS_FUNLOCK: u64 = 640;
pub const SYS_FS_OPEN: u64 = 610;
/// open with a caller-supplied (umask-masked) create mode in arg3, used for
/// `O_CREAT`.  Separate number from `SYS_FS_OPEN` so the 3-arg ABI stays
/// intact for already-built binaries.  See kernel `sys_fs_open_mode`.
pub const SYS_FS_OPEN_MODE: u64 = 659;
/// `openat2` with a resolve word: `(path_ptr, path_len, flags, mode,
/// resolve, dirfd) -> handle`.
///
/// The first four arguments are [`SYS_FS_OPEN_MODE`]'s, in its positions,
/// so the two calls read line for line against each other.  `dirfd == 0`
/// means the *kernel's* process working directory — which is not this
/// libc's, so `file::openat2` never passes 0 for a relative path.  See
/// `kernel/src/syscall/handlers.rs::sys_fs_openat2` and
/// `requests/a-b-openat2-is-661-and-the-mode-is-twelve-bits.md`.
pub const SYS_FS_OPENAT2: u64 = 661;

// The pinned `*at` family: fd-relative calls where the kernel resolves the
// *handle* rather than a path this libc reconstructed from it.
//
// The difference matters because `resolve_dirfd_path` — which every other
// `*at` here still uses — turns `unlinkat(dirfd, "x")` back into
// `unlink("/the/path/dirfd/had/when/it/was/opened/x")`.  Between the open and
// the unlink, anything may have swapped a component of that path for a
// symlink; the walk then leaves the directory the caller thought it held.
// That is `requests/b-a-the-at-family-resolves-by-path-so-no-toctou-fix-is-\
// possible.md`, and these three calls are lane A's answer to it.
//
// Each takes a `dirfd` **kernel handle** (not a libc fd) and a *single*
// component: no `/`, not `.`, not `..`, non-empty, at most 255 bytes.  The
// strictness is the point — verifying that a handle still denotes the
// directory it was opened on proves nothing if the name may then climb out of
// it.  The kernel captures `(fs_id, inode)` at open and re-checks it inside
// the same filesystem lock as the operation, so the check is atomic rather
// than a smaller copy of the race.  A mismatch is `StaleHandle` → `ESTALE`,
// which means *re-open*, never *retry*.
//
// See `requests/a-b-the-at-family-now-has-three-primitives-that-resolve-the-\
// handle.md` for the wire formats and for the two things they do not fix.
/// `(dirfd, name ptr, name len, flags) -> 0`.  `AT_REMOVEDIR_PINNED` (0x200)
/// selects `rmdir` over `unlink`; unknown bits are `EINVAL`, not ignored.
pub const SYS_FS_UNLINKAT_PINNED: u64 = 662;
/// `(dirfd, name ptr, name len, flags, out buf) -> 0`.  Writes the same
/// `FS_META_SIZE` record `SYS_FS_METADATA` writes, from the same encoder.
/// `AT_SYMLINK_NOFOLLOW_PINNED` (0x100) selects `lstat` over `stat`.
///
/// **Not yet usable for [`crate::file::fstatat`], and the reason is the
/// record.**  The 64-byte `FS_META_SIZE` layout has no inode number, no hard
/// link count and no block count; the 80-byte one `SYS_FS_STAT` writes — the
/// one `crate::stat::fill_from_fsstat` decodes — has all three.  Wiring
/// `fstatat` onto this constant as it stands would make `st_ino` zero for
/// every file, which does not fail loudly anywhere: it silently breaks `cp`'s
/// refusal to copy a file onto itself, `ls -i`, hardlink coalescing in `du`
/// and `tar`, and `find -samefile`.  The constant is declared so the number is
/// recorded in one place, and is deliberately unused pending a wider record.
pub const SYS_FS_FSTATAT_PINNED: u64 = 663;
/// `(dirfd, out buf, out cap) -> size of the complete listing`.
///
/// The return is **not** bytes written.  `ret <= cap` means the whole listing
/// arrived; `ret > cap` means it truncated and `ret` is the buffer size to
/// re-issue with.  Unpaginated, so a `getdents64`-style bytes-written return
/// could not distinguish a listing that exactly filled the buffer from one
/// that overflowed it — and a recursive caller would then delete a subtree it
/// had half enumerated and report success.  Truncation is always at a record
/// boundary.
pub const SYS_FS_GETDENTS_PINNED: u64 = 664;

pub const SYS_FS_CLOSE: u64 = 611;
pub const SYS_FS_READ: u64 = 612;
pub const SYS_FS_WRITE: u64 = 613;
pub const SYS_FS_SEEK: u64 = 614;
// Sparse-file seek: find the next data region (SEEK_DATA) or hole
// (SEEK_HOLE) at or after a byte offset.  Args: (handle, offset) ->
// resulting position.  Used by lseek's SEEK_DATA/SEEK_HOLE whence values.
pub const SYS_FS_SEEK_DATA: u64 = 650;
pub const SYS_FS_SEEK_HOLE: u64 = 651;
pub const SYS_FS_TRUNCATE: u64 = 615;
pub const SYS_FS_RENAME: u64 = 616;
pub const SYS_FS_FSTAT: u64 = 617;
pub const SYS_FS_DUP: u64 = 645;
pub const SYS_FS_COPY: u64 = 642;
pub const SYS_FS_APPEND: u64 = 643;
pub const SYS_FS_FTRUNCATE: u64 = 644;

// Symlinks
pub const SYS_FS_SYMLINK: u64 = 637;
pub const SYS_FS_READLINK: u64 = 638;
pub const SYS_FS_LSTAT: u64 = 639;

// Timestamps: set (a)ccess/(m)odify times.  Args: (path_ptr, path_len,
// accessed_ns, modified_ns) where 0 means "leave this timestamp unchanged".
pub const SYS_FS_SET_TIMES: u64 = 632;

// Ownership: set uid/gid.  Args: (path_ptr, path_len, uid, gid) where
// u32::MAX in a field means "leave that field unchanged" (POSIX chown).
pub const SYS_FS_SET_OWNER: u64 = 630;

// Permissions: set Unix mode bits.  Args: (path_ptr, path_len, perms) where
// perms is masked to the low 0o7777 bits by the kernel.
pub const SYS_FS_SET_PERMS: u64 = 631;

// Extended attributes.
//   GET:    (path_ptr, path_len, key_ptr, val_ptr, val_cap) -> true value
//           length (val_cap 0 = size query; copies min(len, cap) bytes).
//   SET:    (path_ptr, path_len, key_ptr, val_ptr, val_len) -> 0.
//   REMOVE: (path_ptr, path_len, key_ptr) -> 0.
//   LIST:   (path_ptr, path_len, buf_ptr, buf_cap) -> total bytes of the
//           null-terminated key list (buf_cap 0 = size query; only fills
//           when the whole list fits).
pub const SYS_FS_GET_XATTR: u64 = 633;
pub const SYS_FS_SET_XATTR: u64 = 634;
pub const SYS_FS_REMOVE_XATTR: u64 = 635;
pub const SYS_FS_LIST_XATTRS: u64 = 636;

// Sync
pub const SYS_FS_SYNC: u64 = 641;

// Filesystem change notification (inotify backend).
//   CREATE: (path_ptr, path_len, event_mask, flags) -> watch id.
//           event_mask bits: 0=CREATE 1=DELETE 2=MODIFY 3=RENAME
//           4=METADATA 5=ACCESS; flags bit0 = recursive.
//   READ:   (watch_id, buf_ptr, max_events) -> event count.  Each event
//           is FS_WATCH_EVENT_SIZE bytes: [0..256] affected path,
//           [256..512] new path (rename), [512..520] watch id (u64),
//           [520..524] event type (u32: 0=created 1=deleted 2=modified
//           3=renamed 4=metadata 5=accessed 255=overflow), [524..528] pad.
//   CLOSE:  (watch_id) -> 0.
pub const SYS_FS_WATCH_CREATE: u64 = 622;
pub const SYS_FS_WATCH_READ: u64 = 623;
pub const SYS_FS_WATCH_CLOSE: u64 = 624;

/// Size in bytes of one event record returned by `SYS_FS_WATCH_READ`.
pub const FS_WATCH_EVENT_SIZE: usize = 528;

// Pipes (IPC range 200-399)
pub const SYS_PIPE_CREATE: u64 = 220;
pub const SYS_PIPE_WRITE: u64 = 221;
pub const SYS_PIPE_READ: u64 = 222;
pub const SYS_PIPE_TRY_WRITE: u64 = 223;
pub const SYS_PIPE_TRY_READ: u64 = 224;
pub const SYS_PIPE_CLOSE: u64 = 225;
pub const SYS_PIPE_POLL: u64 = 228;
pub const SYS_PIPE_READABLE_BYTES: u64 = 229;
// Later pipe additions live in the free extension range (657+): the original
// 220-229 block is full (230 starts shared memory). Backs tee(2) — peek copies
// buffered bytes without consuming, wait_readable blocks for data/EOF.
pub const SYS_PIPE_PEEK: u64 = 657;
pub const SYS_PIPE_WAIT_READABLE: u64 = 658;

// Stream sockets (IPC range 300-310) — bidirectional byte streams backing
// socketpair(AF_UNIX, SOCK_STREAM, ...).  Mirrors kernel/src/syscall/number.rs.
pub const SYS_SOCKETPAIR_CREATE: u64 = 300;
pub const SYS_SOCKETPAIR_SEND: u64 = 301;
pub const SYS_SOCKETPAIR_RECV: u64 = 302;
pub const SYS_SOCKETPAIR_TRY_SEND: u64 = 303;
pub const SYS_SOCKETPAIR_TRY_RECV: u64 = 304;
pub const SYS_SOCKETPAIR_CLOSE: u64 = 305;
pub const SYS_SOCKETPAIR_SEND_TIMEOUT: u64 = 306;
pub const SYS_SOCKETPAIR_RECV_TIMEOUT: u64 = 307;
pub const SYS_SOCKETPAIR_POLL: u64 = 308;
pub const SYS_SOCKETPAIR_READABLE_BYTES: u64 = 309;
pub const SYS_SOCKETPAIR_SHUTDOWN: u64 = 310;

// Futexes (IPC range 210-214)
pub const SYS_FUTEX_WAIT: u64 = 210;
pub const SYS_FUTEX_WAKE: u64 = 211;
pub const SYS_FUTEX_LOCK_PI: u64 = 212;
pub const SYS_FUTEX_UNLOCK_PI: u64 = 213;
pub const SYS_FUTEX_WAIT_TIMEOUT: u64 = 214;

// Eventfd (IPC range 240-249)
pub const SYS_EVENTFD_CREATE: u64 = 240;
pub const SYS_EVENTFD_WRITE: u64 = 241;
pub const SYS_EVENTFD_READ: u64 = 242;
pub const SYS_EVENTFD_TRY_READ: u64 = 243;
pub const SYS_EVENTFD_CLOSE: u64 = 244;
pub const SYS_EVENTFD_READ_TIMEOUT: u64 = 245;
pub const SYS_EVENTFD_WRITE_TIMEOUT: u64 = 246;
pub const SYS_EVENTFD_HAS_VALUE: u64 = 247;

// Capabilities (400-409)
//
// Counts or enumerates the calling process's kernel capabilities.  `arg0` is
// a `CapEntryInfo` array (0 to only count), `arg1` its capacity **in entries**;
// truncation is `BufferTooSmall`/`ERANGE` with nothing written, never a short
// answer.  See `sys_capability::kernel_view` for the consumer and
// `requests/a-b-cap-query-enumeration-landed.md` for the ABI.
pub const SYS_CAP_QUERY: u64 = 400;

// Networking (800-999)
pub const SYS_TCP_CONNECT: u64 = 800;
pub const SYS_TCP_SEND: u64 = 801;
pub const SYS_TCP_RECV: u64 = 802;
pub const SYS_TCP_CLOSE: u64 = 803;
pub const SYS_TCP_BIND: u64 = 804;
pub const SYS_TCP_ACCEPT: u64 = 805;
pub const SYS_TCP_CLOSE_LISTENER: u64 = 806;
pub const SYS_TCP_ABORT: u64 = 807;
pub const SYS_TCP_PEER_ADDR: u64 = 808;

pub const SYS_UDP_BIND: u64 = 810;
pub const SYS_UDP_SEND: u64 = 811;
pub const SYS_UDP_RECV: u64 = 812;
pub const SYS_UDP_CLOSE: u64 = 813;
pub const SYS_UDP_MCAST_JOIN: u64 = 814;
pub const SYS_UDP_MCAST_LEAVE: u64 = 815;
pub const SYS_UDP_CONNECT: u64 = 816;
pub const SYS_UDP_LOCAL_PORT: u64 = 817;

pub const SYS_DNS_RESOLVE: u64 = 820;
pub const SYS_DNS_REVERSE_RESOLVE: u64 = 821;
pub const SYS_NET_STAT: u64 = 825;
pub const SYS_ICMP_PING: u64 = 830;
pub const SYS_ICMP_PING_WAIT: u64 = 831;
pub const SYS_TCP_LIST: u64 = 840;
pub const SYS_TCP_LISTENER_LIST: u64 = 841;
pub const SYS_NET_IF_INFO: u64 = 842;
pub const SYS_ARP_TABLE: u64 = 843;
pub const SYS_DNS_CACHE_STATS: u64 = 844;
pub const SYS_TCP_POLL_STATUS: u64 = 845;
pub const SYS_TCP_LISTENER_READY: u64 = 846;
pub const SYS_UDP_RX_READY: u64 = 847;
pub const SYS_UDP_RX_FRONT_BYTES: u64 = 848;
pub const SYS_TCP_SHUTDOWN: u64 = 855;
pub const SYS_TCP_INFO: u64 = 849;
pub const SYS_TCP_SET_NODELAY: u64 = 850;
pub const SYS_TCP_SET_KEEPALIVE: u64 = 851;
pub const SYS_TCP_SET_KEEPALIVE_PARAMS: u64 = 852;
pub const SYS_TCP_LAST_ERROR: u64 = 853;
pub const SYS_TCP_LOCAL_PORT: u64 = 854;

// ---------------------------------------------------------------------------
// Inline syscall wrappers
// ---------------------------------------------------------------------------
//
// Host-build safety gate
// ----------------------
// `syscallN()` issues a raw `SYSCALL` x86_64 instruction.  On our OS
// target (`target_os = "none"`, the bare-metal posix staticlib) that
// instruction transfers control to the kernel's syscall entry.  On any
// host build (`not(target_os = "none")`, used by `cargo test` against
// the host triple) the same instruction transfers control to whatever
// the host OS placed at SYSCALL — on Windows it dispatches to NT
// system services, with completely different ABI and semantics.
//
// To prevent that UB during host test runs we gate the inline asm
// behind `cfg(target_os = "none")` and have host builds return a
// documented sentinel (`-ENOSYS`).  Wrapper functions that need
// host-meaningful behaviour (e.g. `getpid`, `eventfd`, `timerfd_create`)
// detect this sentinel via `errno::translate` and either fall back to
// a host-friendly implementation or fail cleanly.  Tests that need to
// exercise post-syscall validator logic on host install a fd table entry
// directly — `fdtable::alloc_fd(HandleKind::…, handle)` and
// `fdtable::close_fd` — rather than calling the real wrappers, since the
// wrapper would only report `HOST_ENOSYS` and the validator would never run.
// (This paragraph named a `fdtable::test_install_handle_kind` helper that has
// never existed; `alloc_fd` is what the tests that do this actually use.)

/// Sentinel returned by every `syscallN()` on host builds.  Equals
/// `-(errno::ENOSYS as i64)`.  Pinned by `host_enosys_matches_errno_module`
/// so a future renumbering of ENOSYS won't drift this value.
///
/// Defined for both targets, and `pub(crate)`, because it is not only
/// *produced* here but *recognised* elsewhere: `file.rs`'s pinned `*at` fast
/// path treats it as "this build has no syscalls, fall back to the
/// path-based route".  That comparison has to name the same constant the
/// wrappers return, or a renumbering would leave the fast path testing for a
/// value nothing produces — and it would fail open, taking the pinned route's
/// answer on a host build where there is no kernel to have given one.
///
/// It is a safe sentinel to overload this way because no `KernelError`
/// discriminant is -38: `kernel/src/error.rs` uses -1..-9 and then banded
/// hundreds (-100, -200, -300, -400, -500, -600, -700).
pub(crate) const HOST_ENOSYS: i64 = -38;

// Host-only shim for the wall-clock / monotonic-clock syscalls.  These
// are by far the most-called bare `syscall0` in the posix crate (>20
// call sites in epoll/poll/socket/file/time/sys_times/unistd), and on
// the host build the raw SYSCALL is gated off so they would otherwise
// all return `HOST_ENOSYS` and silently break time-dependent tests.
//
// We back them with `std::time` here so any wrapper that calls
// `clock_gettime` / reads SYS_CLOCK_MONOTONIC for a timeout deadline /
// stamps a record with the realtime clock just works on host — no
// per-call-site intercepts needed.  Same pattern as `host_eventfd_sim`
// in `epoll.rs`.
#[cfg(not(target_os = "none"))]
mod host_clock {
    extern crate std;
    use std::sync::OnceLock;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    /// First call captures the "boot" instant; subsequent calls return
    /// nanoseconds since then.  Monotonic, non-decreasing, no wall-clock
    /// dependency — matches `SYS_CLOCK_MONOTONIC`'s contract.
    static BOOT: OnceLock<Instant> = OnceLock::new();

    pub fn monotonic_ns() -> i64 {
        let boot = *BOOT.get_or_init(Instant::now);
        let ns = Instant::now().saturating_duration_since(boot).as_nanos();
        // Saturate to i64::MAX (~292 years from boot) rather than wrap.
        i64::try_from(ns).unwrap_or(i64::MAX)
    }

    pub fn realtime_ns() -> i64 {
        // `SystemTime` can predate UNIX_EPOCH on systems with broken
        // clocks; clamp to 0 in that case (matches what an uninitialised
        // RTC would return on the OS target).
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
            Err(_) => 0,
        }
    }
}

/// Issue a syscall with 0 arguments.
#[inline(always)]
#[must_use]
pub fn syscall0(nr: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: The SYSCALL instruction is the defined kernel entry
        // point on our OS target.  RCX and R11 are clobbered by SYSCALL
        // (saves RIP and RFLAGS).
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        // Host-side intercepts: the clock syscalls are routed to
        // std::time so time-dependent code paths work in unit tests.
        // Everything else returns the ENOSYS sentinel.
        match nr {
            SYS_CLOCK_MONOTONIC => host_clock::monotonic_ns(),
            SYS_CLOCK_REALTIME => host_clock::realtime_ns(),
            _ => HOST_ENOSYS,
        }
    }
}

/// Issue a syscall with 0 arguments that answers with *two* values.
///
/// A handful of our syscalls use the kernel's `ok2` convention: the
/// first value comes back in RAX and the second in RDX.  `SYS_PIPE_CREATE`
/// is the only current user (read handle, write handle).  `syscall0`
/// cannot express this because it declares RDX nowhere and would let the
/// compiler treat it as untouched across the instruction.
///
/// Returns `(rax, rdx)`.  As everywhere else in this module, a negative
/// `rax` is an error and `rdx` is then meaningless.
///
/// The host arm returns the same `HOST_ENOSYS` sentinel as its siblings —
/// see the host-build safety gate above.  This is the arm that matters
/// most here: until 2026-08-26 `pipe2` open-coded this asm *without* the
/// gate, so `cargo test -p posix` on a Linux host executed a real
/// `syscall` with RAX=220, which is Linux's `semtimedop`, and with
/// RDI/RSI/RDX never loaded — a live kernel call with whatever the
/// compiler had left in the argument registers.  See known-issues
/// `B-POSIX-PIPE2-ISSUES-AN-UNGATED-RAW-SYSCALL-ON-HOST-BUILDS`.
#[inline(always)]
#[must_use]
pub fn syscall0_ok2(nr: u64) -> (i64, u64) {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        let second: u64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.  RDX is an output here
        // rather than a clobber because this is the `ok2` convention.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                lateout("rax") ret,
                lateout("rdx") second,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        (ret, second)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = nr;
        (HOST_ENOSYS, 0)
    }
}

/// Issue a syscall with 1 argument.
#[inline(always)]
#[must_use]
pub fn syscall1(nr: u64, arg0: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 2 arguments.
#[inline(always)]
#[must_use]
pub fn syscall2(nr: u64, arg0: u64, arg1: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 3 arguments.
#[inline(always)]
#[must_use]
pub fn syscall3(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 3 arguments, capturing both return values.
///
/// Returns `(value, value2)` = `(rax, rdx)`.  Used for syscalls that
/// reply with `SyscallResult::ok2` (two-value returns), e.g.
/// `SYS_LOG_READ` returns `(entry_count, newest_seq)`.
#[inline(always)]
#[must_use]
pub fn syscall3_2ret(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> (i64, i64) {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        let ret2: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.  RAX holds `value`,
        // RDX holds `value2` on return.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                lateout("rax") ret,
                lateout("rdx") ret2,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        (ret, ret2)
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2);
        (HOST_ENOSYS, 0)
    }
}

/// Issue a syscall with 4 arguments.
#[inline(always)]
#[must_use]
pub fn syscall4(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                in("r10") arg3,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2, arg3);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 5 arguments.
#[inline(always)]
#[must_use]
pub fn syscall5(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                in("r10") arg3,
                in("r8") arg4,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2, arg3, arg4);
        HOST_ENOSYS
    }
}

/// Issue a syscall with 6 arguments.
#[inline(always)]
#[must_use]
pub fn syscall6(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i64 {
    #[cfg(target_os = "none")]
    {
        let ret: i64;
        // SAFETY: SYSCALL is the OS-target kernel entry; RCX/R11 are
        // clobbered by the instruction itself.
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") nr,
                in("rdi") arg0,
                in("rsi") arg1,
                in("rdx") arg2,
                in("r10") arg3,
                in("r8") arg4,
                in("r9") arg5,
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        ret
    }
    #[cfg(not(target_os = "none"))]
    {
        let _ = (nr, arg0, arg1, arg2, arg3, arg4, arg5);
        HOST_ENOSYS
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Syscall numbers are non-zero --

    #[test]
    fn syscall_numbers_nonzero() {
        // Syscall number 0 is reserved (invalid).
        let all_numbers = [
            SYS_EXIT,
            SYS_TASK_ID,
            SYS_PROCESS_ID,
            SYS_CLOCK_MONOTONIC,
            SYS_SLEEP,
            SYS_CONSOLE_WRITE,
            SYS_CONSOLE_READ_CHAR,
            SYS_LOG_READ,
            SYS_MMAP,
            SYS_MUNMAP,
            SYS_MPROTECT,
            SYS_SCHED_SET_PROFILE,
            SYS_PROCESS_SPAWN,
            SYS_PROCESS_WAIT,
            SYS_PROCESS_EXEC,
            SYS_PROCESS_TRY_WAIT,
            SYS_PROCESS_FORK,
            SYS_THREAD_CREATE,
            SYS_THREAD_EXIT,
            SYS_THREAD_JOIN,
            SYS_PROCESS_SPAWN_EX,
            SYS_PROCESS_GET_INITIAL_FDS,
            SYS_PROCESS_GET_ARGS,
            SYS_FS_READ_FILE,
            SYS_FS_WRITE_FILE,
            SYS_FS_DELETE,
            SYS_FS_LIST_DIR,
            SYS_FS_MKDIR,
            SYS_FS_MKDIR_MODE,
            SYS_FS_RMDIR,
            SYS_FS_STAT,
            SYS_FS_LINK,
            SYS_FS_STATVFS,
            SYS_FS_OPEN,
            SYS_FS_OPEN_MODE,
            SYS_FS_OPENAT2,
            SYS_FS_CLOSE,
            SYS_FS_READ,
            SYS_FS_WRITE,
            SYS_FS_SEEK,
            SYS_FS_TRUNCATE,
            SYS_FS_RENAME,
            SYS_FS_FSTAT,
            SYS_FS_DUP,
            SYS_FS_COPY,
            SYS_FS_APPEND,
            SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK,
            SYS_FS_READLINK,
            SYS_FS_LSTAT,
            SYS_FS_SYNC,
            SYS_FS_FLOCK,
            SYS_FS_FUNLOCK,
            SYS_FS_SEEK_DATA,
            SYS_FS_SEEK_HOLE,
            SYS_FS_WATCH_CREATE,
            SYS_FS_WATCH_READ,
            SYS_FS_WATCH_CLOSE,
            SYS_FS_SET_TIMES,
            SYS_FS_SET_OWNER,
            SYS_FS_SET_PERMS,
            SYS_FS_GET_XATTR,
            SYS_FS_SET_XATTR,
            SYS_FS_REMOVE_XATTR,
            SYS_FS_LIST_XATTRS,
            SYS_PIPE_CREATE,
            SYS_PIPE_WRITE,
            SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE,
            SYS_PIPE_TRY_READ,
            SYS_PIPE_CLOSE,
            SYS_PIPE_POLL,
            SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT,
            SYS_FUTEX_WAKE,
            SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI,
            SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE,
            SYS_EVENTFD_WRITE,
            SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ,
            SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT,
            SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
            SYS_TCP_CONNECT,
            SYS_TCP_SEND,
            SYS_TCP_RECV,
            SYS_TCP_CLOSE,
            SYS_TCP_BIND,
            SYS_TCP_ACCEPT,
            SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT,
            SYS_TCP_PEER_ADDR,
            SYS_UDP_BIND,
            SYS_UDP_SEND,
            SYS_UDP_RECV,
            SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN,
            SYS_UDP_MCAST_LEAVE,
            SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_DNS_RESOLVE,
            SYS_DNS_REVERSE_RESOLVE,
            SYS_NET_STAT,
            SYS_ICMP_PING,
            SYS_ICMP_PING_WAIT,
            SYS_TCP_LIST,
            SYS_TCP_LISTENER_LIST,
            SYS_NET_IF_INFO,
            SYS_ARP_TABLE,
            SYS_DNS_CACHE_STATS,
            SYS_TCP_POLL_STATUS,
            SYS_TCP_LISTENER_READY,
            SYS_UDP_RX_READY,
            SYS_UDP_RX_FRONT_BYTES,
            SYS_TCP_SHUTDOWN,
            SYS_TCP_INFO,
            SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE,
            SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR,
            SYS_TCP_LOCAL_PORT,
        ];
        for &nr in &all_numbers {
            assert_ne!(nr, 0, "syscall number must not be zero");
        }
    }

    // -- Process-control syscall numbers match the kernel ABI --

    #[test]
    fn process_syscall_numbers_match_kernel() {
        // These must equal the values in kernel/src/syscall/number.rs.
        // A mismatch silently routes a POSIX call to the wrong (or an
        // unimplemented) kernel syscall.
        assert_eq!(SYS_EXIT, 1);
        assert_eq!(SYS_TASK_ID, 2);
        assert_eq!(SYS_PROCESS_ID, 502, "getpid ABI number drifted");
        assert_eq!(SYS_PROCESS_FORK, 527, "fork ABI number drifted");
        assert_eq!(SYS_PROCESS_SPAWN, 500);
        assert_eq!(SYS_PROCESS_EXEC, 503);
        assert_eq!(SYS_PROCESS_PARENT_ID, 520);
    }

    // -- All syscall numbers are unique --

    /// Every `pub const SYS_*: u64 = <number>;` in this file, parsed out of the
    /// file itself.
    ///
    /// **Derived, not listed.**  This test used to compare a hand-written array
    /// of 123 names, while the file declared 193 constants — so 70 of them,
    /// including the whole 544-556 pty block, were never checked against
    /// anything.  That is the gap that made the rlimit pair's original proposed
    /// numbers (544/545) dangerous: they collided with `SYS_PTY_CREATE` and
    /// `SYS_PTY_WRITE_INPUT`, and nothing here would have said so.  A list of
    /// names is exactly the artefact that stops being complete the moment
    /// someone adds a constant without remembering it exists — which is the
    /// same failure `scripts/check-variant-lists.py` gates elsewhere in the
    /// tree, and it cannot gate this one because the array is not named `ALL`
    /// and holds `u64`s rather than enum variants.
    ///
    /// Reading the source is not elegant, but it is the only form that cannot
    /// drift: a constant that exists is checked because it exists.
    fn declared_syscall_numbers() -> Vec<(String, u64)> {
        let src = include_str!("syscall.rs");
        let mut out = Vec::new();
        for line in src.lines() {
            let Some(rest) = line.strip_prefix("pub const SYS_") else {
                continue;
            };
            let Some((name, value)) = rest.split_once(": u64 = ") else {
                continue;
            };
            let Some(value) = value.strip_suffix(';') else {
                continue;
            };
            // Only literal numbers.  A constant defined in terms of another
            // (`SYS_A: u64 = SYS_B;`) would be a deliberate alias and must not
            // be reported as a collision; none exist today, and if one is added
            // the count assertion below is what will bring it to attention.
            if let Ok(value) = value.trim().parse::<u64>() {
                out.push((format!("SYS_{name}"), value));
            }
        }
        out
    }

    #[test]
    fn syscall_numbers_unique() {
        let declared = declared_syscall_numbers();

        // A parser that silently matched nothing would make this test vacuous
        // and green forever.  The bound is deliberately loose — it is a
        // liveness check on the parse, not a count of the ABI.
        assert!(
            declared.len() > 150,
            "parsed only {} syscall constants out of this file; the \
             `pub const SYS_...: u64 = <n>;` shape this test reads has changed",
            declared.len()
        );

        for i in 0..declared.len() {
            for j in (i + 1)..declared.len() {
                let (ref a_name, a) = declared[i];
                let (ref b_name, b) = declared[j];
                assert_ne!(
                    a, b,
                    "{a_name} and {b_name} are both {a}; a syscall number names \
                     one call, so the second name silently gets the first's \
                     behaviour rather than failing to build"
                );
            }
        }
    }

    /// The numbers lane A allocated for the rlimit pair, pinned against the
    /// block they sit beside.
    ///
    /// Separate from the uniqueness sweep because uniqueness is not the
    /// property that was nearly lost: 544/545 would have been unique *within
    /// this file* if the pty block had not been written down here, and would
    /// still have named the wrong kernel calls.  These assert the values
    /// themselves, against `requests/a-b-native-rlimit-syscalls-landed.md`.
    #[test]
    fn rlimit_syscalls_sit_after_the_pty_block_not_on_top_of_it() {
        assert_eq!(SYS_RLIMIT_GET, 557);
        assert_eq!(SYS_RLIMIT_SET, 558);
        assert_eq!(SYS_PTY_CREATE, 544);
        assert_eq!(SYS_PTY_SET_TERMIOS, 556);
        assert!(SYS_RLIMIT_GET > SYS_PTY_SET_TERMIOS);
        assert!(SYS_RLIMIT_SET < SYS_PROCESS_SPAWN_EX2);
    }

    // -- Syscall number ranges match zone allocation --

    #[test]
    fn syscall_ranges_by_zone() {
        // kernel-core: 0-199
        assert!(SYS_EXIT <= 199);
        assert!(SYS_TASK_ID <= 199);
        assert!(SYS_CLOCK_MONOTONIC <= 199);
        assert!(SYS_SLEEP <= 199);
        assert!(SYS_CONSOLE_WRITE <= 199);
        assert!(SYS_CONSOLE_READ_CHAR <= 199);
        assert!(SYS_LOG_READ <= 199);
        assert!(SYS_MMAP <= 199);
        assert!(SYS_MUNMAP <= 199);
        assert!(SYS_MPROTECT <= 199);
        assert!(SYS_SCHED_SET_PROFILE <= 199);

        // kernel-ipc: 200-399
        assert!((200..400).contains(&SYS_PIPE_CREATE));
        assert!((200..400).contains(&SYS_PIPE_WRITE));
        assert!((200..400).contains(&SYS_PIPE_READ));
        assert!((200..400).contains(&SYS_PIPE_CLOSE));
        assert!((200..400).contains(&SYS_EVENTFD_CREATE));
        assert!((200..400).contains(&SYS_EVENTFD_WRITE));
        assert!((200..400).contains(&SYS_EVENTFD_READ));
        assert!((200..400).contains(&SYS_EVENTFD_CLOSE));

        // kernel-process: 500-599
        assert!((500..600).contains(&SYS_PROCESS_ID));
        assert!((500..600).contains(&SYS_PROCESS_SPAWN));
        assert!((500..600).contains(&SYS_PROCESS_WAIT));
        assert!((500..600).contains(&SYS_PROCESS_EXEC));
        assert!((500..600).contains(&SYS_THREAD_CREATE));
        assert!((500..600).contains(&SYS_THREAD_EXIT));
        assert!((500..600).contains(&SYS_THREAD_JOIN));
        assert!((500..600).contains(&SYS_PROCESS_SPAWN_EX));
        assert!((500..600).contains(&SYS_PROCESS_SPAWN_EX2));
        assert!((500..600).contains(&SYS_PROCESS_GET_INITIAL_FDS));
        assert!((500..600).contains(&SYS_PROCESS_GET_ARGS));

        // fs: 600-799
        assert!((600..800).contains(&SYS_FS_READ_FILE));
        assert!((600..800).contains(&SYS_FS_WRITE_FILE));
        assert!((600..800).contains(&SYS_FS_OPEN));
        assert!((600..800).contains(&SYS_FS_CLOSE));
        assert!((600..800).contains(&SYS_FS_DUP));

        // net: 800-999
        assert!((800..1000).contains(&SYS_TCP_CONNECT));
        assert!((800..1000).contains(&SYS_UDP_BIND));
        assert!((800..1000).contains(&SYS_DNS_RESOLVE));
    }

    // -- All IPC syscall numbers (pipe + eventfd) in IPC range --

    #[test]
    fn ipc_syscalls_in_ipc_range() {
        let ipc_nrs = [
            SYS_PIPE_CREATE,
            SYS_PIPE_WRITE,
            SYS_PIPE_READ,
            SYS_PIPE_TRY_WRITE,
            SYS_PIPE_TRY_READ,
            SYS_PIPE_CLOSE,
            SYS_PIPE_POLL,
            SYS_PIPE_READABLE_BYTES,
            SYS_FUTEX_WAIT,
            SYS_FUTEX_WAKE,
            SYS_FUTEX_LOCK_PI,
            SYS_FUTEX_UNLOCK_PI,
            SYS_FUTEX_WAIT_TIMEOUT,
            SYS_EVENTFD_CREATE,
            SYS_EVENTFD_WRITE,
            SYS_EVENTFD_READ,
            SYS_EVENTFD_TRY_READ,
            SYS_EVENTFD_CLOSE,
            SYS_EVENTFD_READ_TIMEOUT,
            SYS_EVENTFD_WRITE_TIMEOUT,
            SYS_EVENTFD_HAS_VALUE,
        ];
        for &nr in &ipc_nrs {
            assert!(
                (200..400).contains(&nr),
                "IPC syscall {nr} must be in IPC range 200-399"
            );
        }
    }

    // -- All TCP syscall numbers in net range --

    #[test]
    fn tcp_syscalls_in_net_range() {
        let tcp_nrs = [
            SYS_TCP_CONNECT,
            SYS_TCP_SEND,
            SYS_TCP_RECV,
            SYS_TCP_CLOSE,
            SYS_TCP_BIND,
            SYS_TCP_ACCEPT,
            SYS_TCP_CLOSE_LISTENER,
            SYS_TCP_ABORT,
            SYS_TCP_PEER_ADDR,
            SYS_TCP_POLL_STATUS,
            SYS_TCP_LISTENER_READY,
            SYS_TCP_SHUTDOWN,
            SYS_TCP_INFO,
            SYS_TCP_SET_NODELAY,
            SYS_TCP_SET_KEEPALIVE,
            SYS_TCP_SET_KEEPALIVE_PARAMS,
            SYS_TCP_LAST_ERROR,
            SYS_TCP_LOCAL_PORT,
            SYS_TCP_LIST,
            SYS_TCP_LISTENER_LIST,
        ];
        for &nr in &tcp_nrs {
            assert!(
                (800..1000).contains(&nr),
                "TCP syscall {nr} must be in net range 800-999"
            );
        }
    }

    // -- All UDP syscall numbers in net range --

    #[test]
    fn udp_syscalls_in_net_range() {
        let udp_nrs = [
            SYS_UDP_BIND,
            SYS_UDP_SEND,
            SYS_UDP_RECV,
            SYS_UDP_CLOSE,
            SYS_UDP_MCAST_JOIN,
            SYS_UDP_MCAST_LEAVE,
            SYS_UDP_CONNECT,
            SYS_UDP_LOCAL_PORT,
            SYS_UDP_RX_READY,
            SYS_UDP_RX_FRONT_BYTES,
        ];
        for &nr in &udp_nrs {
            assert!(
                (800..1000).contains(&nr),
                "UDP syscall {nr} must be in net range 800-999"
            );
        }
    }

    // -- DNS/ICMP/Net info syscalls in net range --

    #[test]
    fn dns_net_syscalls_in_net_range() {
        let nrs = [
            SYS_DNS_RESOLVE,
            SYS_DNS_REVERSE_RESOLVE,
            SYS_NET_STAT,
            SYS_ICMP_PING,
            SYS_ICMP_PING_WAIT,
            SYS_NET_IF_INFO,
            SYS_ARP_TABLE,
            SYS_DNS_CACHE_STATS,
        ];
        for &nr in &nrs {
            assert!(
                (800..1000).contains(&nr),
                "net info syscall {nr} must be in net range 800-999"
            );
        }
    }

    // -- All FS syscall numbers in fs range --

    #[test]
    fn fs_syscalls_in_fs_range() {
        let fs_nrs = [
            SYS_FS_READ_FILE,
            SYS_FS_WRITE_FILE,
            SYS_FS_DELETE,
            SYS_FS_LIST_DIR,
            SYS_FS_MKDIR,
            SYS_FS_MKDIR_MODE,
            SYS_FS_RMDIR,
            SYS_FS_STAT,
            SYS_FS_LINK,
            SYS_FS_STATVFS,
            SYS_FS_OPEN,
            SYS_FS_OPEN_MODE,
            SYS_FS_OPENAT2,
            SYS_FS_CLOSE,
            SYS_FS_READ,
            SYS_FS_WRITE,
            SYS_FS_SEEK,
            SYS_FS_TRUNCATE,
            SYS_FS_RENAME,
            SYS_FS_FSTAT,
            SYS_FS_DUP,
            SYS_FS_COPY,
            SYS_FS_APPEND,
            SYS_FS_FTRUNCATE,
            SYS_FS_SYMLINK,
            SYS_FS_READLINK,
            SYS_FS_LSTAT,
            SYS_FS_SYNC,
            SYS_FS_FLOCK,
            SYS_FS_FUNLOCK,
            SYS_FS_SEEK_DATA,
            SYS_FS_SEEK_HOLE,
            SYS_FS_WATCH_CREATE,
            SYS_FS_WATCH_READ,
            SYS_FS_WATCH_CLOSE,
            SYS_FS_SET_TIMES,
            SYS_FS_SET_OWNER,
            SYS_FS_SET_PERMS,
            SYS_FS_GET_XATTR,
            SYS_FS_SET_XATTR,
            SYS_FS_REMOVE_XATTR,
            SYS_FS_LIST_XATTRS,
        ];
        for &nr in &fs_nrs {
            assert!(
                (600..800).contains(&nr),
                "FS syscall {nr} must be in fs range 600-799"
            );
        }
    }

    // -- Memory syscalls in kernel-core range --

    #[test]
    fn memory_syscalls_in_core_range() {
        assert!(SYS_MMAP <= 199);
        assert!(SYS_MUNMAP <= 199);
        assert!(SYS_MPROTECT <= 199);
    }

    // -- Host-build safety gate --
    //
    // On host builds (`not(target_os = "none")`), every `syscallN()`
    // returns -ENOSYS rather than emitting a real SYSCALL instruction.
    // These tests pin that contract so a future refactor cannot
    // regress us into executing UB against NT system services on the
    // Windows test host.

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_enosys_matches_errno_module() {
        // If `errno::ENOSYS` ever changes, `HOST_ENOSYS` must move with
        // it.  Pin both ends here.
        assert_eq!(HOST_ENOSYS, -(crate::errno::ENOSYS as i64));
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall0_returns_enosys() {
        assert_eq!(syscall0(SYS_EXIT), HOST_ENOSYS);
    }

    // The clock syscalls are special-cased inside `syscall0` so that
    // time-dependent code paths (timeouts, timestamps) work in host
    // tests.  Pin the contract: they must NOT return the ENOSYS
    // sentinel and they must return non-negative, non-decreasing
    // monotonic-ns / non-zero realtime-ns values.
    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall0_clock_monotonic_returns_non_decreasing_nanos() {
        let a = syscall0(SYS_CLOCK_MONOTONIC);
        let b = syscall0(SYS_CLOCK_MONOTONIC);
        assert!(a >= 0, "monotonic must be non-negative, got {a}");
        assert!(b >= a, "monotonic must be non-decreasing ({a} -> {b})");
        assert_ne!(a, HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall0_clock_realtime_returns_post_epoch_nanos() {
        let ns = syscall0(SYS_CLOCK_REALTIME);
        assert!(ns >= 0, "realtime must be non-negative, got {ns}");
        assert_ne!(ns, HOST_ENOSYS);
        // 2020-01-01 UTC was ~1577836800 s = 1.577e18 ns since epoch.
        // Anything older than that on a host running these tests is
        // a broken clock.  Lower bound chosen conservatively.
        const YEAR_2020_NS: i64 = 1_577_836_800_000_000_000;
        assert!(ns >= YEAR_2020_NS, "realtime clock looks unset: {ns}");
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall1_returns_enosys() {
        assert_eq!(syscall1(SYS_EXIT, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall2_returns_enosys() {
        assert_eq!(syscall2(SYS_EVENTFD_CREATE, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall3_returns_enosys() {
        assert_eq!(syscall3(SYS_FS_READ, 0, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall3_2ret_returns_enosys_and_zero() {
        assert_eq!(syscall3_2ret(SYS_LOG_READ, 0, 0, 0), (HOST_ENOSYS, 0));
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall4_returns_enosys() {
        assert_eq!(syscall4(SYS_FS_LINK, 0, 0, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall5_returns_enosys() {
        assert_eq!(syscall5(SYS_MMAP, 0, 0, 0, 0, 0), HOST_ENOSYS);
    }

    #[cfg(not(target_os = "none"))]
    #[test]
    fn host_syscall6_returns_enosys() {
        assert_eq!(syscall6(SYS_MMAP, 0, 0, 0, 0, 0, 0), HOST_ENOSYS);
    }
}
