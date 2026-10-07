//! POSIX signal-delivery shim (kernel side).
//!
//! Our OS deliberately does **not** use Unix signals for process control
//! (see `design.txt`: "No Unix signals for process control. Use IPC
//! messages."). Hardware faults are delivered as SEH-style exceptions
//! (`proc/exception.rs`), and process lifecycle is driven by IPC.
//!
//! However, the POSIX compatibility layer must still support
//! `signal()`/`sigaction()`/`kill()` for ported software (bash,
//! coreutils, Python). This module provides the *minimum* kernel
//! machinery to make asynchronous signal delivery work, modelled closely
//! on the exception-delivery path:
//!
//! 1. The POSIX runtime registers a single process-wide **trampoline**
//!    (`register_trampoline`). The trampoline is the only thing the
//!    kernel knows how to jump to; the per-signal handler table lives
//!    entirely in userspace.
//! 2. `kill()`/`raise()` post a signal into a target process's **pending
//!    set** (`set_pending`, via the `SYS_SIGNAL_SEND` syscall).
//! 3. When the target process next returns to userspace from a syscall,
//!    the syscall-return path checks for a deliverable signal
//!    (`take_deliverable`) and, if the trampoline is registered, builds a
//!    [`SignalContext`] on the user stack and redirects execution to the
//!    trampoline (see `handlers::deliver_pending_signal`).
//! 4. The trampoline invokes the userspace handler then calls
//!    `SYS_SIGNAL_RETURN` to restore the interrupted context.
//!
//! ## What the kernel does and does not know
//!
//! The kernel tracks the pending set, the blocked mask, the trampoline
//! address -- and, of the per-signal dispositions, only which signals are
//! **ignored** (and `SIGCHLD`'s `SA_NOCLDWAIT`). The rest of the handler
//! table is userspace's: the native libc keeps it and decides there whether
//! to run a handler or take the default action. The ignored set is the
//! exception because it is the part of a disposition that must outlive the
//! program that set it -- `exec` keeps it, `fork` and spawn pass it on, and a
//! new image's libc table starts empty -- and because the kernel has to act
//! on it: an ignored signal is discarded when sent, and a parent ignoring
//! `SIGCHLD` leaves no zombies (`pcb::ExitNotice`). Userspace reports it
//! (`SYS_SIGNAL_SET_IGNORED`, the Linux shim's `rt_sigaction`); see
//! `SignalState::ignored`.
//!
//! The kernel-side *default-action* table decides what happens when a
//! signal is posted to a process that has **no trampoline registered** (a
//! non-POSIX process, or one that has not yet run its libc init):
//! terminating signals kill it, everything else is dropped.
//! `SIGKILL` is always fatal and can never be delivered to a handler.
//!
//! ## Concurrency
//!
//! All per-process state lives behind a single `Mutex`. Posting a signal
//! (possibly from another process) and consuming one (always the running
//! process itself) both take this lock briefly. To keep the syscall hot
//! path cheap, a global pending counter lets the return path skip the
//! lock entirely when no signals are pending anywhere.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::ProcessId;
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Signal numbers
// ---------------------------------------------------------------------------

/// Number of supported signals. POSIX signals are numbered 1..=NSIG.
/// We support a 64-bit pending set, so signals 1..=64.
pub const NSIG: u32 = 64;

/// `SIGHUP` — hangup. Sent to an orphaned process group's stopped members
/// (with `SIGCONT`) and, classically, to a session on controlling-terminal
/// loss. Default action terminates. Standard Linux number.
pub const SIGHUP: u32 = 1;

/// `SIGKILL` — always fatal, never catchable. Standard Linux number.
pub const SIGKILL: u32 = 9;

/// `SIGCHLD` — a child stopped, continued or exited. Default action ignore;
/// a parent that sets it to `SIG_IGN` (or gives it `SA_NOCLDWAIT`) leaves no
/// zombies (`pcb::ExitNotice`). Standard Linux number.
pub const SIGCHLD: u32 = 17;

/// `SIGCONT` — continue (resume) a stopped process. Never catchable as a
/// stop-override (always resumes), but a handler may also run. Standard
/// Linux number.
pub const SIGCONT: u32 = 18;

/// `SIGSTOP` — stop signal, never catchable. Standard Linux number.
pub const SIGSTOP: u32 = 19;

/// `SIGTSTP` — interactive stop (Ctrl-Z). Catchable, unlike `SIGSTOP`.
pub const SIGTSTP: u32 = 20;

/// `SIGTTIN` — background read from controlling terminal. Catchable stop.
pub const SIGTTIN: u32 = 21;

/// `SIGTTOU` — background write to controlling terminal. Catchable stop.
pub const SIGTTOU: u32 = 22;

/// `SIGWINCH` — the controlling terminal's window size changed.
///
/// Raised by `TIOCSWINSZ` when the size *actually* changes, so a full-screen
/// program (an editor, a pager, a shell drawing a right-aligned prompt) knows
/// to re-query `TIOCGWINSZ` and redraw.  Its default action is **ignore** —
/// see [`default_action`] — which is what makes it safe to broadcast to a
/// whole foreground group: a program that does not care is not killed by a
/// resize.  Standard Linux number.
pub const SIGWINCH: u32 = 28;

/// Default disposition of a signal for a process with no handler.
///
/// This mirrors the Linux default-action table closely enough for the
/// kernel's fallback decision when no userspace trampoline is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    /// Terminate the process (optionally with a core dump — we don't
    /// distinguish; both terminate).
    Terminate,
    /// Ignore the signal (no effect).
    Ignore,
    /// Stop the process: suspend all its threads via the scheduler until a
    /// `SIGCONT` resumes it (real job control — see
    /// `handlers::stop_process_for_signal`).
    Stop,
    /// Continue a stopped process: resume all its suspended threads (see
    /// `handlers::continue_process`).
    Continue,
}

/// Look up the default action for a signal number.
///
/// Based on the Linux signal(7) default-action table. Real-time signals
/// (>= 32) default to Terminate, matching Linux.
#[must_use]
pub fn default_action(sig: u32) -> DefaultAction {
    match sig {
        // Ignored by default.
        17 /* SIGCHLD */ | 23 /* SIGURG */ | 28 /* SIGWINCH */ => {
            DefaultAction::Ignore
        }
        // Stop signals: SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU.
        19..=22 => DefaultAction::Stop,
        // Continue.
        18 /* SIGCONT */ => DefaultAction::Continue,
        // Everything else (including SIGKILL and RT signals) terminates.
        _ => DefaultAction::Terminate,
    }
}

/// Returns `true` if `sig` is a valid signal number (1..=NSIG).
#[must_use]
pub fn is_valid_signal(sig: u32) -> bool {
    sig >= 1 && sig <= NSIG
}

/// Convert a 1-based signal number to its bit in a 64-bit set.
///
/// Returns `None` if the signal number is out of range.
///
/// Public because the `sig - 1` offset (signals are 1-based, bits are
/// 0-based) is correct in every open-coded copy right up until one of them is
/// written next to an *unvalidated* signal number.  Callers outside this
/// module — terminal-access job control's blocked-signal test, for one —
/// should ask here rather than reproduce the shift, and the `Option` makes
/// the out-of-range case impossible to forget.
#[inline]
#[must_use]
pub fn signal_bit(sig: u32) -> Option<u64> {
    if is_valid_signal(sig) {
        // sig is 1..=64, so the shift amount is 0..=63 — always valid.
        // `checked_sub`/`checked_shl` keep this arithmetic-side-effect
        // free; both branches are statically guaranteed to be `Some`.
        let shift = sig.checked_sub(1)?;
        1u64.checked_shl(shift)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Signal context (userspace ABI)
// ---------------------------------------------------------------------------

/// Saved CPU context at the point a signal interrupted userspace.
///
/// The kernel writes this onto the user stack before jumping to the
/// trampoline, and restores from it on `SYS_SIGNAL_RETURN`. It captures
/// exactly the register state needed to reconstruct the interrupted
/// [`SyscallFrame`](crate::syscall::entry::SyscallFrame), plus `rax`
/// (the interrupted syscall's return value) and the delivered signal
/// number.
///
/// # ABI
///
/// This struct is part of the userspace ABI. Fields must not be
/// reordered or resized. The trampoline receives `signum` in `rdi` and a
/// pointer to this struct in `rsi`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SignalContext {
    /// The signal number being delivered (1..=NSIG).
    pub signum: u64,
    /// Saved RAX — the interrupted syscall's return value, restored on
    /// `SYS_SIGNAL_RETURN` so the interrupted code sees the correct
    /// result.
    pub rax: u64,
    /// Saved RDI (syscall arg0).
    pub rdi: u64,
    /// Saved RSI (syscall arg1).
    pub rsi: u64,
    /// Saved RDX (syscall arg2).
    pub rdx: u64,
    /// Saved R10 (syscall arg3).
    pub r10: u64,
    /// Saved R8 (syscall arg4).
    pub r8: u64,
    /// Saved R9 (syscall arg5).
    pub r9: u64,
    /// Saved RBX.
    pub rbx: u64,
    /// Saved RBP.
    pub rbp: u64,
    /// Saved R12.
    pub r12: u64,
    /// Saved R13.
    pub r13: u64,
    /// Saved R14.
    pub r14: u64,
    /// Saved R15.
    pub r15: u64,
    /// Interrupted instruction pointer.
    pub rip: u64,
    /// Interrupted stack pointer.
    pub rsp: u64,
    /// Interrupted RFLAGS.
    pub rflags: u64,
}

/// Size of the signal context in bytes (17 × 8 = 136).
pub const SIGNAL_CONTEXT_SIZE: usize = core::mem::size_of::<SignalContext>();

/// What follows the [`SignalContext`] in the **extended** native signal frame:
/// the signal's `siginfo`, for a process that asked for it with
/// `SYS_SIGNAL_REGISTER`'s `SIGNAL_FRAME_SIGINFO` flag.
///
/// # ABI
///
/// Part of the userspace ABI, at `ctx + 136`; fields must not be reordered or
/// resized. The context before it is unchanged, so `SYS_SIGNAL_RETURN` and a
/// libc that never asked read the frame exactly as before. 160 bytes in all,
/// which keeps the context's 16-byte alignment and the fake return slot below
/// it where they were (`requests/d-a-put-each-signal-s-siginfo-in-the-native-
/// frame.md`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalInfoTail {
    /// `si_code`: the sender class ([`si_code`]).
    pub si_code: i32,
    /// `si_pid`: the sender, or for `SIGCHLD` the child.
    pub si_pid: u32,
    /// `si_uid`: the sender's real uid, or the child's.
    pub si_uid: u32,
    /// Zero.
    pub pad: u32,
    /// `si_value` for `SI_QUEUE`; for `SIGCHLD`, `si_status` (the exit status
    /// or the signal), which shares its offset in `siginfo_t`.
    pub si_value: u64,
}

/// Size of the extended frame: the context and the tail (136 + 24 = 160).
pub const SIGNAL_FRAME_EXTENDED_SIZE: usize =
    SIGNAL_CONTEXT_SIZE + core::mem::size_of::<SignalInfoTail>();

const _: () = assert!(core::mem::size_of::<SignalInfoTail>() == 24);
const _: () = assert!(SIGNAL_FRAME_EXTENDED_SIZE == 160);
// The offsets libc reads, as `SYS_SIGNAL_REGISTER`'s docs give them (frame
// offset = 136 + these).
const _: () = assert!(core::mem::offset_of!(SignalInfoTail, si_code) == 0);
const _: () = assert!(core::mem::offset_of!(SignalInfoTail, si_pid) == 4);
const _: () = assert!(core::mem::offset_of!(SignalInfoTail, si_uid) == 8);
const _: () = assert!(core::mem::offset_of!(SignalInfoTail, pad) == 12);
const _: () = assert!(core::mem::offset_of!(SignalInfoTail, si_value) == 16);

/// Where a native signal frame of `frame_size` bytes goes on a stack whose top
/// is `base`: the context's address -- 16-byte aligned, the frame below
/// `base` -- and the handler's `rsp`, the fake return slot 8 bytes under the
/// context, so that `rsp % 16 == 8` at entry as the SysV convention has it.
///
/// The tail of the extended frame lies above the context, so both forms put
/// the context and the return slot in the same relation; only how far below
/// `base` differs.
#[must_use]
pub const fn frame_placement(base: u64, frame_size: u64) -> (u64, u64) {
    let ctx_addr = base.wrapping_sub(frame_size) & !0xF;
    (ctx_addr, ctx_addr.wrapping_sub(8))
}

impl SignalInfoTail {
    /// The tail for a delivered signal's record.
    #[must_use]
    pub const fn from_info(info: &SigInfo) -> Self {
        Self {
            si_code: info.code,
            si_pid: info.sender_pid,
            si_uid: info.sender_uid,
            pad: 0,
            si_value: info.value,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-process signal state
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Pending-signal source metadata (siginfo)
// ---------------------------------------------------------------------------

/// `si_code` values stamped into a delivered `siginfo_t`. These are the
/// POSIX/Linux-ABI standard sender-class codes, the single source of truth
/// for both the native and the Linux-`rt_sigframe` delivery paths.
pub mod si_code {
    /// Sent by `kill(2)` from a user process.
    pub const SI_USER: i32 = 0;
    /// A POSIX timer expired (`timer_create(2)`): the record's `sender_pid`
    /// is the timer id (`si_timerid`), `sender_uid` the overrun count
    /// (`si_overrun`) and `value` the timer's `sigev_value` -- the offsets
    /// `siginfo_t`'s `_timer` member gives them, the same as `_rt`'s pid, uid
    /// and value.
    pub const SI_TIMER: i32 = -2;
    /// Sent by the kernel itself (timer expiry, kernel-injected signals).
    pub const SI_KERNEL: i32 = 0x80;
    /// Sent by `sigqueue(3)` / `rt_sigqueueinfo(2)` (carries an `si_value`).
    pub const SI_QUEUE: i32 = -1;
    /// Sent by `tkill(2)` / `tgkill(2)` (i.e. `raise`/`pthread_kill`).
    pub const SI_TKILL: i32 = -6;
    /// `SIGCHLD`: child exited normally (`si_code = CLD_EXITED`).
    pub const CLD_EXITED: i32 = 1;
    /// `SIGCHLD`: child was killed by a signal (`si_code = CLD_KILLED`).
    pub const CLD_KILLED: i32 = 2;
    /// `SIGCHLD`: child was killed by a signal and left a core file. The
    /// kernel writes none today, so nothing reports it yet; it is here so that
    /// a status word with the core bit set has a code to map to.
    pub const CLD_DUMPED: i32 = 3;
    /// `SIGCHLD`: child stopped (`si_status` is the stop signal).
    pub const CLD_STOPPED: i32 = 5;
    /// `SIGCHLD`: a stopped child continued (`si_status` is `SIGCONT`).
    pub const CLD_CONTINUED: i32 = 6;
}

/// Source metadata recorded when a signal is posted, used to fill the Linux
/// `siginfo_t` handed to an `SA_SIGINFO` handler at delivery.
///
/// Linux records a `struct sigqueue` per queued signal; for standard
/// (non-real-time) signals only the *first* instance's info is kept — later
/// posts of an already-pending standard signal coalesce. We mirror that with
/// one optional record per signal number, set on the clear→set transition and
/// taken at delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SigInfo {
    /// `si_code` — the sender class (see [`si_code`]).
    pub code: i32,
    /// Sending process pid (`si_pid`); 0 for kernel-generated signals.
    pub sender_pid: u32,
    /// Sending real user id (`si_uid`); 0 for kernel-generated signals.
    pub sender_uid: u32,
    /// `si_value`/`si_ptr` payload for `SI_QUEUE`; 0 otherwise.
    pub value: u64,
}

impl SigInfo {
    /// A user-directed signal (`kill(2)`): `SI_USER` with the sender identity.
    #[must_use]
    pub const fn user(sender_pid: u32, sender_uid: u32) -> Self {
        Self {
            code: si_code::SI_USER,
            sender_pid,
            sender_uid,
            value: 0,
        }
    }

    /// A thread-directed signal (`tkill`/`tgkill`, i.e. `raise`/`pthread_kill`):
    /// `SI_TKILL` with the sender identity.
    #[must_use]
    pub const fn tkill(sender_pid: u32, sender_uid: u32) -> Self {
        Self {
            code: si_code::SI_TKILL,
            sender_pid,
            sender_uid,
            value: 0,
        }
    }

    /// A kernel-generated signal (timer expiry): `SI_KERNEL`, no sender.
    #[must_use]
    pub const fn kernel() -> Self {
        Self {
            code: si_code::SI_KERNEL,
            sender_pid: 0,
            sender_uid: 0,
            value: 0,
        }
    }

    /// A `SIGCHLD` for a child that ended: `CLD_EXITED` with its exit status,
    /// or `CLD_KILLED` with the signal that killed it, as `code_and_status`
    /// gives them; the child's pid and real uid as the sender (Linux's
    /// `si_pid`/`si_uid`). The status travels in `value`, which `siginfo_t`'s
    /// `si_status` shares an offset with.
    #[must_use]
    pub const fn child(child_pid: u32, child_uid: u32, code_and_status: (i32, i32)) -> Self {
        let (code, status) = code_and_status;
        // Reinterpreted, not converted: `si_status` is an `int` at the low
        // half of `si_value`'s slot.
        #[allow(clippy::cast_sign_loss)]
        let value = status as u32 as u64;
        Self {
            code,
            sender_pid: child_pid,
            sender_uid: child_uid,
            value,
        }
    }

    /// A `sigqueue`d signal: `SI_QUEUE` with the sender and its value.
    #[must_use]
    pub const fn queued(sender_pid: u32, sender_uid: u32, value: u64) -> Self {
        Self {
            code: si_code::SI_QUEUE,
            sender_pid,
            sender_uid,
            value,
        }
    }

    /// A POSIX timer's expiry: `SI_TIMER`, the timer id where a sender's pid
    /// goes, the overrun count (0 until delivery says otherwise) where its uid
    /// goes, and the timer's `sigev_value`. See [`si_code::SI_TIMER`].
    #[must_use]
    pub const fn timer(timer_id: i32, value: u64) -> Self {
        // Reinterpreted, not converted: `si_timerid` is an `int` in the slot
        // `si_pid` uses, and timer ids are never negative.
        #[allow(clippy::cast_sign_loss)]
        let sender_pid = timer_id as u32;
        Self {
            code: si_code::SI_TIMER,
            sender_pid,
            sender_uid: 0,
            value,
        }
    }
}

/// A POSIX timer's signal waiting in its process's queue -- Linux's
/// preallocated per-timer `sigqueue`, linked into the shared pending list.
///
/// Unlike the one record a standard signal coalesces to, every timer has a
/// queue entry of its own: two timers that name the same signal deliver twice,
/// as on Linux (`send_sigqueue` never applies the legacy one-instance rule),
/// and a dequeued entry tells [`crate::proc::posix_timer`] which timer to
/// re-arm and charge the overrun to.
#[derive(Debug, Clone, Copy)]
struct QueuedTimerSignal {
    /// The signal it raises.
    sig: u32,
    /// Its timer, within the process.
    timer_id: i32,
    /// The arming that queued it ([`crate::proc::posix_timer`]'s generation):
    /// a timer re-set or deleted since makes the entry stale, and the timer
    /// module drops a stale entry when it is dequeued (Linux 6.13's rule).
    token: u64,
    /// Its record.
    info: SigInfo,
}

/// What [`post_timer_signal`] did with a timer's expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerPost {
    /// Queued; the signal is pending.
    Queued,
    /// The timer already had an entry queued, which now stands for this
    /// expiry (its token is refreshed).
    AlreadyQueued,
    /// The signal is ignored and not blocked, so nothing was queued
    /// (Linux's `prepare_signal`): the timer re-arms itself if periodic.
    Ignored,
    /// The process has no signal state (it is exiting), or no room was
    /// reserved for the entry; nothing was queued.
    Gone,
}

/// Per-process signal bookkeeping.
///
/// Not `Copy`: the timer queue is a `Vec`. Nothing needs a copy of the whole
/// state -- `fork` builds the child's field by field, and starts its queue
/// empty as it starts everything pending empty.
#[derive(Debug, Clone)]
struct SignalState {
    /// Pending set: bit `n-1` set means signal `n` is pending -- its standard
    /// record (`infos`), one or more timer entries (`timer_queue`), or both.
    pending: u64,
    /// Blocked mask: bit `n-1` set means signal `n` is blocked.
    blocked: u64,
    /// Userspace trampoline address (0 = not registered).
    trampoline: u64,
    /// The trampoline asked for the extended frame -- the context and then
    /// the signal's `siginfo` ([`SignalInfoTail`]). Goes with the trampoline:
    /// kept across fork, dropped at exec.
    extended_frame: bool,
    /// Per-signal source metadata (`siginfo`), indexed by `sig - 1`: the one
    /// instance a signal sent by `kill`, `sigqueue` or the kernel coalesces
    /// to. `Some` exactly while that instance is pending -- recorded when the
    /// signal was posted with its bit clear, taken at delivery. A post that
    /// finds the bit already set (by this record or a timer entry) is merged
    /// into what is there, Linux's `legacy_queue`.
    infos: [Option<SigInfo>; NSIG as usize],
    /// POSIX timers' signals, oldest first ([`QueuedTimerSignal`]). At most one
    /// entry per timer, so it never holds more entries than the process has
    /// timers -- and [`reserve_timer_signals`] keeps its capacity at least
    /// that, which is what lets a timer's expiry queue its signal from the
    /// timer interrupt without allocating. For a given signal, the standard
    /// instance (if any) is older than every timer entry: it can only be
    /// recorded while the bit is clear.
    timer_queue: Vec<QueuedTimerSignal>,
    /// Saved blocked mask awaiting restore, set by `sigsuspend`/`rt_sigsuspend`
    /// (Linux's `saved_sigmask` + `TIF_RESTORE_SIGMASK`). While `Some`, the
    /// process is running under a *temporary* blocked mask; the saved mask is
    /// the one to restore. It is consumed (cleared) either by signal delivery
    /// — `emit_linux_rt_frame` writes it into `uc_sigmask` so `rt_sigreturn`
    /// restores it — or by the no-handler tail of `deliver_linux_signal`,
    /// which restores it directly. `None` means no restore is pending.
    saved_sigmask: Option<u64>,
    /// Base and length of the process's alternate signal stack, as last
    /// reported by `SYS_SIGNAL_ALTSTACK`. `(0, 0)` means none is registered.
    ///
    /// The kernel holds this rather than reading libc's copy at delivery time,
    /// because the case an alternate stack exists to serve is a *stack
    /// overflow*: reading userspace memory while building a signal frame for a
    /// fault is the same recursion the feature is meant to escape.
    altstack_sp: u64,
    altstack_size: u64,
    /// Signals whose handler asked for `SA_ONSTACK`: bit `n-1` set means
    /// signal `n` wants the alternate stack.
    ///
    /// Here because **the kernel cannot see `sa_flags`**. They live in libc and
    /// nothing in this module has ever recorded them, so without the mask the
    /// kernel would know the stack but not which signals may use it -- and
    /// either guess breaks POSIX: always using it steals the stack from
    /// handlers that never asked, never using it leaves the feature
    /// unimplemented.
    onstack_mask: u64,
    /// Ignored set: bit `n-1` set means the process's disposition for signal
    /// `n` is `SIG_IGN`. Never holds `SIGKILL` or `SIGSTOP`.
    ///
    /// The one part of a disposition the kernel keeps, for both ABIs: the
    /// native libc reports it (`SYS_SIGNAL_SET_IGNORED`), the Linux shim
    /// records it at each `rt_sigaction` ([`record_disposition`]). It has to
    /// be the kernel's, because it is the part that outlives the program that
    /// set it -- `exec` keeps it and `fork` and spawn pass it on, where a
    /// libc's table starts empty -- and because the kernel acts on it: an
    /// ignored signal is discarded when sent ([`classify_post_info`]),
    /// whether or not anything in the target could have dropped it, and a
    /// parent that ignores `SIGCHLD` leaves no zombies (`pcb::ExitNotice`).
    ignored: u64,
    /// `SA_NOCLDWAIT` on `SIGCHLD`: children are reaped at exit, though
    /// `SIGCHLD` is still sent. Recorded for the same two reasons as
    /// [`Self::ignored`] (`SYS_SIGNAL_SET_IGNORED`'s flag, the Linux
    /// `rt_sigaction`), but unlike it cleared at `exec`, with the rest of a
    /// handler's flags.
    nocldwait: bool,
}

impl Default for SignalState {
    fn default() -> Self {
        Self {
            pending: 0,
            blocked: 0,
            trampoline: 0,
            extended_frame: false,
            infos: [None; NSIG as usize],
            timer_queue: Vec::new(),
            saved_sigmask: None,
            altstack_sp: 0,
            altstack_size: 0,
            onstack_mask: 0,
            ignored: 0,
            nocldwait: false,
        }
    }
}

/// One pending instance of a signal, taken out of a [`SignalState`].
enum Taken {
    /// The standard record -- a signal sent by `kill`, `sigqueue` or the kernel.
    Record(SigInfo),
    /// A POSIX timer's entry, which its timer must see before it is delivered.
    Timer(QueuedTimerSignal),
}

impl SignalState {
    /// Whether signal `sig` (slot `idx = sig - 1`) still has an instance
    /// pending: its standard record or a timer entry.
    fn has_instance(&self, idx: usize, sig: u32) -> bool {
        self.infos.get(idx).is_some_and(Option::is_some)
            || self.timer_queue.iter().any(|q| q.sig == sig)
    }

    /// Clear `sig`'s pending bit `bit` -- and take it off the global count --
    /// once no instance of it is left. The bit means "some instance is
    /// pending", so every path that removes an instance ends here.
    fn settle_bit(&mut self, sig: u32, bit: u64) {
        let idx = bit.trailing_zeros() as usize;
        if self.pending & bit != 0 && !self.has_instance(idx, sig) {
            self.pending &= !bit;
            PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// [`Self::settle_bit`] for every signal in `mask`.
    fn settle_bits(&mut self, mask: u64) {
        let mut rem = mask;
        while rem != 0 {
            let bit = rem & rem.wrapping_neg(); // lowest set bit
            // trailing_zeros is 0..=63, so the signal number is 1..=64.
            self.settle_bit(bit.trailing_zeros().saturating_add(1), bit);
            rem &= !bit;
        }
    }

    /// Take the oldest pending instance of signal `sig` (pending bit `bit`):
    /// its standard record if it has one -- which is always older than any
    /// timer entry of the same signal, see [`Self::timer_queue`] -- else its
    /// oldest timer entry. A bit set with neither (which no path leaves) yields
    /// a generic `SI_USER` record and is cleared.
    fn take_instance(&mut self, sig: u32, bit: u64) -> Taken {
        let idx = bit.trailing_zeros() as usize;
        let taken = if let Some(info) = self.infos.get_mut(idx).and_then(Option::take) {
            Taken::Record(info)
        } else if let Some(pos) = self.timer_queue.iter().position(|q| q.sig == sig) {
            // `remove` shifts the tail down; it never allocates.
            Taken::Timer(self.timer_queue.remove(pos))
        } else {
            Taken::Record(SigInfo::user(0, 0))
        };
        self.settle_bit(sig, bit);
        taken
    }
}

/// All per-process signal state, keyed by process ID.
static SIGNAL_STATES: Mutex<BTreeMap<ProcessId, SignalState>> = Mutex::new(BTreeMap::new());

/// Count of pending signals across all processes.
///
/// Used as a cheap fast-path gate in the syscall-return delivery check so
/// the common case (no signals pending anywhere) avoids taking the lock.
/// May over-count transiently (e.g. while a blocked signal sits pending),
/// which only costs an occasional needless lock acquisition — never a
/// missed delivery.
static PENDING_COUNT: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// signalfd blocking-read wait queue
// ---------------------------------------------------------------------------

/// A thread parked in a *blocking* `signalfd` `read()`, waiting for a signal
/// in `mask` to become pending on its process.
#[derive(Debug, Clone, Copy)]
struct SignalFdWaiter {
    /// The blocked task to wake when a matching signal arrives.
    task: TaskId,
    /// The fd's acceptance mask (bit `n-1` set ⇒ accepts signal `n`).
    mask: u64,
}

/// Per-process list of threads blocked in a `signalfd` read.
///
/// Kept as a **separate** registry rather than a field of [`SignalState`] so
/// that `SignalState` stays `Copy` (it is cloned by value in several places).
/// The cross-lock lost-wakeup hazard this creates is closed by the
/// register-then-recheck protocol in the reader (see
/// `dispatch_signalfd_read`): the reader registers *before* re-checking
/// [`has_pending_in_mask`], and [`set_pending`] sets the pending bit *before*
/// scanning this registry, so any bit set after the reader's re-check is
/// guaranteed to find the reader registered (and wake it), while any bit set
/// before is seen by the re-check (and the reader does not block).
static SIGNALFD_WAITERS: Mutex<BTreeMap<ProcessId, Vec<SignalFdWaiter>>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// IRQ-safe lock accessors
// ---------------------------------------------------------------------------
//
// Both signal registries below are plain spin locks that are taken in
// *syscall* (process) context.  They must ALSO be touchable from *interrupt*
// context — an `hrtimer` callback that posts `SIGALRM` for an expired
// ITIMER_REAL, a future keyboard ISR raising `SIGINT`, etc.  If the timer ISR
// fired on a CPU already holding one of these locks in syscall context and
// then tried to re-acquire it, the CPU would spin forever on its own lock.
//
// The fix is to hold these locks only with interrupts disabled, so a holder
// can never be interrupted mid-critical-section on its own CPU.  Routing
// *every* access through these two helpers makes that invariant hold by
// construction — no call site can forget the `without_interrupts` guard.  The
// critical sections are short (bitmap/`BTreeMap` ops), so masking interrupts
// across them is cheap.

/// Run `f` with the [`SIGNAL_STATES`] lock held and interrupts disabled.
#[inline]
fn with_states<R>(f: impl FnOnce(&mut BTreeMap<ProcessId, SignalState>) -> R) -> R {
    crate::cpu::without_interrupts(|| {
        let mut states = SIGNAL_STATES.lock();
        f(&mut states)
    })
}

/// Run `f` with the [`SIGNALFD_WAITERS`] lock held and interrupts disabled.
#[inline]
fn with_waiters<R>(f: impl FnOnce(&mut BTreeMap<ProcessId, Vec<SignalFdWaiter>>) -> R) -> R {
    crate::cpu::without_interrupts(|| {
        let mut waiters = SIGNALFD_WAITERS.lock();
        f(&mut waiters)
    })
}

/// Register `task` as blocked in a `signalfd` read on `pid`, accepting `mask`.
///
/// Idempotent per `(pid, task)`: a re-registration updates the mask rather than
/// adding a duplicate entry (a thread can only be blocked in one read at a
/// time, so at most one entry per task is meaningful).
pub fn register_signalfd_waiter(pid: ProcessId, task: TaskId, mask: u64) {
    with_waiters(|waiters| {
        let list = waiters.entry(pid).or_default();
        if let Some(existing) = list.iter_mut().find(|w| w.task == task) {
            existing.mask = mask;
        } else {
            list.push(SignalFdWaiter { task, mask });
        }
    });
}

/// Remove `task`'s `signalfd` waiter registration for `pid`, if present.
///
/// A harmless no-op if the task was never registered or was already woken
/// (and thereby removed) by [`set_pending`].
pub fn deregister_signalfd_waiter(pid: ProcessId, task: TaskId) {
    with_waiters(|waiters| {
        if let Some(list) = waiters.get_mut(&pid) {
            list.retain(|w| w.task != task);
            if list.is_empty() {
                waiters.remove(&pid);
            }
        }
    });
}

/// How many waiters one pass of [`wake_signalfd_waiters`] takes out of the
/// registry; it repeats until a pass comes back short.
const WAKE_BATCH: usize = 16;

/// Move up to `out.len()` waiters of `pid` whose mask intersects `bit` out of
/// the registry into `out`, returning how many, and leave the rest registered.
///
/// Allocation-free -- it neither grows nor frees anything, not even a list it
/// empties (`deregister_signalfd_waiter` and [`remove`] drop that, in process
/// context) -- because it runs in the timer interrupt when an `ITIMER_REAL` or
/// POSIX timer expiry posts a signal, and the heap's lock is not one an
/// interrupt may wait for: the code it interrupted may hold it.
fn take_matching_signalfd_waiters_into(pid: ProcessId, bit: u64, out: &mut [TaskId]) -> usize {
    with_waiters(|waiters| {
        let Some(list) = waiters.get_mut(&pid) else {
            return 0;
        };
        let mut taken = 0usize;
        list.retain(|w| {
            if w.mask & bit != 0 {
                if let Some(slot) = out.get_mut(taken) {
                    *slot = w.task;
                    taken = taken.saturating_add(1);
                    return false;
                }
            }
            true
        });
        taken
    })
}

/// Remove and return every `signalfd` waiter of `pid` whose mask intersects
/// `bit`, leaving non-matching waiters registered, and drop the registry's
/// entry for `pid` if that empties it.
///
/// Pure registry mutation (no scheduler interaction), for the self-tests of
/// the partition logic [`wake_signalfd_waiters`] uses; it allocates, so the
/// wake path itself uses [`take_matching_signalfd_waiters_into`].
fn take_matching_signalfd_waiters(pid: ProcessId, bit: u64) -> Vec<TaskId> {
    let mut matched = Vec::new();
    let mut batch: [TaskId; WAKE_BATCH] = [0; WAKE_BATCH];
    loop {
        let n = take_matching_signalfd_waiters_into(pid, bit, &mut batch);
        matched.extend_from_slice(batch.get(..n).unwrap_or(&[]));
        if n < WAKE_BATCH {
            break;
        }
    }
    with_waiters(|waiters| {
        if waiters.get(&pid).is_some_and(Vec::is_empty) {
            waiters.remove(&pid);
        }
    });
    matched
}

/// Wake every `signalfd` reader of `pid` whose mask intersects `bit` (a single
/// signal bit that just transitioned to pending), removing them from the
/// registry first so a burst of arriving signals wakes each reader only once.
///
/// Uses the `try_wake`/`defer_wake` idiom so it is safe to call from any
/// context (it never blocks and never directly enters the scheduler's
/// run-queue manipulation if the target is not currently parked), and takes
/// the waiters out in fixed-size batches so it never allocates (see
/// [`take_matching_signalfd_waiters_into`]).
fn wake_signalfd_waiters(pid: ProcessId, bit: u64) {
    let mut batch: [TaskId; WAKE_BATCH] = [0; WAKE_BATCH];
    loop {
        // Take a batch out, then wake it outside the registry lock.
        let n = take_matching_signalfd_waiters_into(pid, bit, &mut batch);
        for &task in batch.get(..n).unwrap_or(&[]) {
            if !sched::try_wake(task) {
                sched::defer_wake(task);
            }
        }
        if n < WAKE_BATCH {
            break;
        }
    }
}

/// Remove and return **every** signal-waiter registered for `pid`, regardless
/// of its mask.
///
/// Split out from [`wake_all_waiters`] so the registry mutation is unit-testable
/// without a live scheduler.
fn take_all_waiters(pid: ProcessId) -> Vec<TaskId> {
    with_waiters(|waiters| {
        waiters
            .remove(&pid)
            .map(|list| list.into_iter().map(|w| w.task).collect())
            .unwrap_or_default()
    })
}

/// Wake every parked signal-waiter for `pid`, ignoring registered masks.
///
/// Used by `rt_sigprocmask` when it unblocks one or more already-pending
/// signals.  A thread parked in `pause()` (or a future `rt_sigsuspend`)
/// registered its waiter with a snapshot of the deliverable mask (`!blocked`)
/// taken *before* the unblock, so that snapshot necessarily **excludes** the
/// just-unblocked bits — matching by waiter mask (as [`wake_signalfd_waiters`]
/// does) could never wake it.  We therefore wake all waiters and let each
/// re-check its own condition: the register-then-recheck park loops treat a
/// spurious wake as a no-op and simply re-park.  Unblocking an already-pending
/// signal is rare, so the occasional spurious wake of an unrelated `signalfd`
/// reader (which will recheck its mask and re-park) is a negligible,
/// self-correcting cost.
///
/// Uses the `try_wake`/`defer_wake` idiom, so it is safe to call from any
/// context.
pub fn wake_all_waiters(pid: ProcessId) {
    for task in take_all_waiters(pid) {
        if !sched::try_wake(task) {
            sched::defer_wake(task);
        }
    }
}

// ---------------------------------------------------------------------------
// Trampoline registration
// ---------------------------------------------------------------------------

/// Register (or replace) the process-wide signal trampoline.
///
/// `addr == 0` unregisters, reverting to "no asynchronous delivery"
/// (pending signals stay pending but are not delivered).
pub fn register_trampoline(pid: ProcessId, addr: u64) {
    register_trampoline_frame(pid, addr, false);
}

/// Register a trampoline and say which frame it reads: with `extended`, the
/// context followed by the signal's `siginfo` ([`SignalInfoTail`]).
pub fn register_trampoline_frame(pid: ProcessId, addr: u64, extended: bool) {
    with_states(|states| {
        let st = states.entry(pid).or_default();
        st.trampoline = addr;
        st.extended_frame = extended && addr != 0;
    });
}

/// Whether `pid`'s trampoline reads the extended frame.
#[must_use]
pub fn extended_frame(pid: ProcessId) -> bool {
    with_states(|states| states.get(&pid).is_some_and(|s| s.extended_frame))
}

/// `pid`'s trampoline and whether it reads the extended frame, or `None` with
/// no trampoline. Read together, so that a thread re-registering while
/// another's signal is delivered cannot pair one registration's trampoline
/// with the other's frame.
#[must_use]
pub fn trampoline_frame(pid: ProcessId) -> Option<(u64, bool)> {
    with_states(|states| {
        states
            .get(&pid)
            .filter(|s| s.trampoline != 0)
            .map(|s| (s.trampoline, s.extended_frame))
    })
}

/// Record a process's alternate signal stack and the set of signals allowed to
/// use it.
///
/// Both arrive together because libc holds both and they must not drift: the
/// stack comes from `sigaltstack`, the mask from `sigaction`, and a kernel
/// holding one without the other can decide nothing. `size == 0` unregisters.
pub fn set_altstack(pid: ProcessId, sp: u64, size: u64, onstack_mask: u64) {
    with_states(|states| {
        let st = states.entry(pid).or_default();
        st.altstack_sp = sp;
        st.altstack_size = size;
        st.onstack_mask = onstack_mask;
    });
}

/// Where `sig`'s signal frame should be built, if it belongs on the alternate
/// stack.
///
/// Returns the address one past the top of the alternate stack -- the base a
/// frame grows down from -- or `None` to use the interrupted stack.
///
/// `None` in three cases, and the third is not an optimisation:
///
/// * the handler did not ask (`SA_ONSTACK` clear for this signal);
/// * no stack is registered;
/// * `current_rsp` is **already inside** the alternate stack. A signal arriving
///   while a handler is already running there must not restart at the top, or it
///   overwrites the frames of the handler it interrupted. libc's
///   `altstack_entry` refuses for the same reason, and the two have to agree or
///   one of them is wrong.
#[must_use]
pub fn altstack_top_for(pid: ProcessId, sig: u32, current_rsp: u64) -> Option<u64> {
    // Takes the signal number in the width the delivery path already has it
    // (`take_deliverable` yields `u32`), so the range check happens here rather
    // than being lost in a cast at the call site.
    if sig == 0 || sig > NSIG {
        return None;
    }
    let bit = 1u64 << (sig - 1);
    with_states(|states| {
        let st = states.get(&pid)?;
        if st.onstack_mask & bit == 0 || st.altstack_size == 0 {
            return None;
        }
        let top = st.altstack_sp.checked_add(st.altstack_size)?;
        if current_rsp >= st.altstack_sp && current_rsp < top {
            return None;
        }
        Some(top)
    })
}

/// Get the registered trampoline address for a process, if any.
#[must_use]
pub fn trampoline(pid: ProcessId) -> Option<u64> {
    with_states(|states| states.get(&pid).map(|s| s.trampoline).filter(|&a| a != 0))
}

/// Returns `true` if the process has a non-zero trampoline registered.
#[must_use]
pub fn has_trampoline(pid: ProcessId) -> bool {
    trampoline(pid).is_some()
}

/// Clear all signal state for a process. Called on process death.
///
/// Decrements the global pending counter by the number of pending
/// signals this process had, keeping the fast-path gate accurate.
pub fn remove(pid: ProcessId) {
    with_states(|states| {
        if let Some(state) = states.remove(&pid) {
            let n = state.pending.count_ones() as usize;
            if n != 0 {
                PENDING_COUNT.fetch_sub(n, Ordering::Relaxed);
            }
        }
    });
    // Drop any signalfd waiter registrations for the dying process so the
    // registry never accumulates stale entries for tasks being torn down.
    with_waiters(|waiters| {
        waiters.remove(&pid);
    });
}

/// Reset signal state across `exec()`.
///
/// POSIX resets all caught signals to their default disposition on a
/// successful `exec` (the new image's handler table is empty until its
/// libc init re-registers). Since our per-signal dispositions live in
/// userspace and are discarded with the old address space, the kernel's
/// job is simply to drop the now-stale trampoline so we never jump to a
/// garbage address in the new image.
///
/// **Kept:** the pending signals and the blocked mask. POSIX `execve` lists
/// "process signal mask" among what the new image inherits, and Linux keeps
/// `current->blocked` untouched; a shell blocks SIGCHLD or SIGINT around a
/// fork, execs the command, and relies on it starting with them blocked. This
/// cleared the mask until 2026-10-01, under a comment claiming POSIX asked
/// for that (`requests/d-a-exec-must-keep-the-signal-mask.md`). Pending
/// signals are delivered once the new image registers its trampoline and
/// unblocks them.
///
/// **Kept, too:** the ignored set. POSIX: "Signals set to the default action
/// (SIG_DFL) in the calling process image shall be set to the default action
/// in the new process image ... Signals set to be ignored (SIG_IGN) by the
/// calling process image shall be set to be ignored by the new process
/// image." It is how `nohup cmd` keeps `cmd` alive when its terminal closes,
/// and it is why the set is the kernel's (see [`SignalState::ignored`]).
/// **Cleared:** `SA_NOCLDWAIT`, which is a handler's flag, and goes where the
/// new image's handler table does.
///
/// **Deleted:** the process's POSIX timers ([`crate::proc::posix_timer`]),
/// with any signal of theirs still queued -- POSIX: "timers created by the
/// calling process ... shall be deleted before replacing the current process
/// image" (Linux's `exit_itimers` in `begin_new_exec`). `ITIMER_REAL` is a
/// different timer and survives, as POSIX requires.
pub fn on_exec(pid: ProcessId) {
    crate::proc::posix_timer::on_exec(pid);
    with_states(|states| {
        if let Some(state) = states.get_mut(&pid) {
            state.nocldwait = false;
            state.trampoline = 0;
            // The new image registers its own trampoline, and says then which
            // frame it reads.
            state.extended_frame = false;
            // An alternate signal stack is NOT preserved across execve (see
            // sigaltstack(2)), and here it must not be: the address named a
            // buffer in the old image's address space, which has just been
            // discarded. Keeping it would mean building the next signal frame
            // at an address belonging to a program that no longer exists --
            // strictly worse than having no alternate stack, because the fault
            // would land in the new image's delivery path rather than being a
            // missing feature. The mask goes with it, since the new image's
            // handlers have not asked for anything yet.
            state.altstack_sp = 0;
            state.altstack_size = 0;
            state.onstack_mask = 0;
        }
    });
}

/// Inherit signal state from a parent across `fork()`.
///
/// POSIX semantics: the child inherits the parent's blocked-signal mask
/// and signal dispositions, but the set of pending signals is **empty**
/// in the child.  Our per-signal dispositions live in userspace (and are
/// carried over automatically by the copy-on-write address space), so the
/// kernel's job is to copy the blocked mask and the trampoline address
/// (the child's CoW-copied trampoline lives at the same user address) and
/// to start the child with no pending signals.
///
/// Overwrites any existing child state (the child is freshly created, so
/// there should be none, but this is idempotent).
pub fn inherit_for_fork(parent: ProcessId, child: ProcessId) {
    with_states(|states| {
        // What the child takes from the parent; everything else starts empty
        // -- the pending set and the timer queue among it (the parent's POSIX
        // timers are not inherited either, so nothing of theirs belongs here).
        let inherited = states
            .get(&parent)
            .map(|s| SignalState {
                blocked: s.blocked,
                trampoline: s.trampoline,
                extended_frame: s.extended_frame,
                altstack_sp: s.altstack_sp,
                altstack_size: s.altstack_size,
                onstack_mask: s.onstack_mask,
                ignored: s.ignored,
                nocldwait: s.nocldwait,
                ..SignalState::default()
            })
            .unwrap_or_default();
        // If the child somehow already had pending signals recorded, drop
        // them from the global counter before overwriting.
        if let Some(existing) = states.get(&child) {
            let n = existing.pending.count_ones() as usize;
            if n != 0 {
                PENDING_COUNT.fetch_sub(n, Ordering::Relaxed);
            }
        }
        states.insert(
            child,
            SignalState {
                pending: 0,
                blocked: inherited.blocked,
                trampoline: inherited.trampoline,
                // The trampoline's frame goes with it.
                extended_frame: inherited.extended_frame,
                // POSIX: the child starts with no pending signals, so no
                // per-signal siginfo records carry over.
                infos: [None; NSIG as usize],
                // Nor timer signals: the child has no POSIX timers (they are
                // not inherited), so it reserves room as it creates its own.
                timer_queue: Vec::new(),
                // No sigsuspend in flight in a freshly-forked child.
                saved_sigmask: None,
                // Inherited, per sigaltstack(2): "a child created via fork(2)
                // inherits a copy of its parent's alternate signal stack
                // settings". The child's CoW-copied stack lives at the same
                // user address, exactly as the trampoline does above. (Not
                // preserved across execve -- see the exec path, which clears
                // them.)
                altstack_sp: inherited.altstack_sp,
                altstack_size: inherited.altstack_size,
                onstack_mask: inherited.onstack_mask,
                // A forked child has its parent's dispositions -- its libc's
                // table is a copy of the parent's memory -- so the kernel's
                // part of them is copied too.
                ignored: inherited.ignored,
                nocldwait: inherited.nocldwait,
            },
        );
    });
}

/// Set up the signal state of a process `spawn` has just created, before its
/// first instruction: what `posix_spawn` promises, which is what a fork and
/// an exec would have left.
///
/// - **Blocked:** `sigmask` if the spawn asked for one
///   (`POSIX_SPAWN_SETSIGMASK`), else the parent's -- POSIX: "If the
///   POSIX_SPAWN_SETSIGMASK flag is not set ... the child process shall
///   inherit the parent's signal mask." `SIGKILL` and `SIGSTOP` are taken out,
///   as [`set_blocked`] does.
/// - **Ignored:** the parent's, less `sigdefault` (`POSIX_SPAWN_SETSIGDEF`):
///   an exec keeps ignored signals ignored, and the child is a new image.
/// - **Everything else** starts empty, as after an exec: nothing pending, no
///   trampoline, no alternate stack, no `SA_NOCLDWAIT`.
///
/// `parent` 0 is the kernel, which has nothing to inherit. Any state already
/// recorded for `child` is replaced.
pub fn start_spawned(parent: ProcessId, child: ProcessId, sigmask: Option<u64>, sigdefault: u64) {
    with_states(|states| {
        let (parent_blocked, parent_ignored) = if parent == 0 {
            (0, 0)
        } else {
            states
                .get(&parent)
                .map_or((0, 0), |s| (s.blocked, s.ignored))
        };
        if let Some(existing) = states.get(&child) {
            let n = existing.pending.count_ones() as usize;
            if n != 0 {
                PENDING_COUNT.fetch_sub(n, Ordering::Relaxed);
            }
        }
        states.insert(
            child,
            SignalState {
                blocked: sigmask.unwrap_or(parent_blocked) & !uncatchable_mask(),
                ignored: parent_ignored & !sigdefault,
                ..SignalState::default()
            },
        );
    });
}

/// `SIGKILL` and `SIGSTOP`: never blocked, never ignored, never caught.
#[inline]
#[must_use]
fn uncatchable_mask() -> u64 {
    signal_bit(SIGKILL).unwrap_or(0) | signal_bit(SIGSTOP).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Ignored set
// ---------------------------------------------------------------------------

/// Replace `pid`'s ignored set with `mask` -- signal `n` is bit `n - 1` -- and
/// `SA_NOCLDWAIT` with `nocldwait`, returning the previous set.
/// `SYS_SIGNAL_SET_IGNORED`'s core: the native libc's report of every signal
/// whose disposition is `SIG_IGN`, whenever one moves to or from it.
///
/// A signal newly ignored that is pending is discarded, blocked or not --
/// POSIX: "Setting a signal action to SIG_IGN for a signal that is pending
/// shall cause the pending signal to be discarded, whether or not it is
/// blocked." Only *newly* ignored ones: the call carries the whole set, so a
/// signal already ignored before it is one whose action did not change, and a
/// pending instance of it (it was blocked when sent) stays for whoever
/// unblocks or changes it.
///
/// # Errors
///
/// `InvalidArgument` if `mask` names `SIGKILL` or `SIGSTOP`, which cannot be
/// ignored; nothing is changed.
pub fn set_ignored(pid: ProcessId, mask: u64, nocldwait: bool) -> KernelResult<u64> {
    if mask & uncatchable_mask() != 0 {
        return Err(KernelError::InvalidArgument);
    }
    let old = with_states(|states| {
        let state = states.entry(pid).or_default();
        let old = state.ignored;
        state.ignored = mask;
        state.nocldwait = nocldwait;
        old
    });
    clear_pending(pid, mask & !old);
    Ok(old)
}

/// `pid`'s ignored set: bit `n - 1` for each signal `n` whose disposition is
/// `SIG_IGN`. 0 for a process with no signal state.
#[must_use]
pub fn ignored(pid: ProcessId) -> u64 {
    with_states(|states| states.get(&pid).map_or(0, |s| s.ignored))
}

/// Whether `pid`'s disposition for `sig` is `SIG_IGN`. `false` for a number
/// that is not a signal.
#[must_use]
pub fn is_ignored(pid: ProcessId, sig: u32) -> bool {
    signal_bit(sig).is_some_and(|bit| ignored(pid) & bit != 0)
}

/// A process's pending, blocked and ignored sets, read together (bit `n - 1`
/// for signal `n` in each).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SignalSets {
    /// Signals sent and not yet delivered.
    pub pending: u64,
    /// Signals the process blocks.
    pub blocked: u64,
    /// Signals whose disposition is `SIG_IGN`.
    pub ignored: u64,
}

/// `pid`'s [`SignalSets`], under one hold of the lock so the three agree --
/// what `/proc/<pid>/status` and `/proc/<pid>/stat` report. All empty for a
/// process with no signal state.
#[must_use]
pub fn sets(pid: ProcessId) -> SignalSets {
    with_states(|states| {
        states
            .get(&pid)
            .map_or_else(SignalSets::default, |s| SignalSets {
                pending: s.pending,
                blocked: s.blocked,
                ignored: s.ignored,
            })
    })
}

/// Whether `pid`'s `SIGCHLD` carries `SA_NOCLDWAIT`.
#[must_use]
pub fn nocldwait(pid: ProcessId) -> bool {
    with_states(|states| states.get(&pid).is_some_and(|s| s.nocldwait))
}

/// Drop `pid`'s `SA_NOCLDWAIT`, as a reset of its handlers does: an exec
/// ([`on_exec`] does this itself) or a Linux `clone(CLONE_CLEAR_SIGHAND)`.
pub fn clear_nocldwait(pid: ProcessId) {
    with_states(|states| {
        if let Some(state) = states.get_mut(&pid) {
            state.nocldwait = false;
        }
    });
}

/// What one signal's new disposition is, for [`record_disposition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// `SIG_DFL`.
    Default,
    /// `SIG_IGN`.
    Ignore,
    /// A handler.
    Handler,
}

/// Record the kernel's part of one signal's new disposition, for a caller
/// that changes them one at a time -- the Linux shim's `rt_sigaction`, whose
/// table holds the rest. Linux's `do_sigaction`, in what the kernel keeps:
///
/// - the signal is in the ignored set exactly when the action is `SIG_IGN`;
/// - a pending instance is discarded, blocked or not, whenever the new action
///   ignores it -- `SIG_IGN`, or `SIG_DFL` for a signal whose default is to be
///   ignored (`sig_handler_ignored`);
/// - for `SIGCHLD`, `nocldwait` is the action's `SA_NOCLDWAIT`.
///
/// No effect for `SIGKILL`, `SIGSTOP` or a number that is not a signal: their
/// actions cannot be changed, and the caller has refused the request.
pub fn record_disposition(pid: ProcessId, sig: u32, action: Disposition, nocldwait: bool) {
    let Some(bit) = signal_bit(sig) else {
        return;
    };
    if bit & uncatchable_mask() != 0 {
        return;
    }
    with_states(|states| {
        let state = states.entry(pid).or_default();
        if action == Disposition::Ignore {
            state.ignored |= bit;
        } else {
            state.ignored &= !bit;
        }
        if sig == SIGCHLD {
            state.nocldwait = nocldwait;
        }
    });
    let discards = action == Disposition::Ignore
        || (action == Disposition::Default && default_action(sig) == DefaultAction::Ignore);
    if discards {
        clear_pending(pid, bit);
    }
}

// ---------------------------------------------------------------------------
// Blocked mask
// ---------------------------------------------------------------------------

/// Set the blocked-signal mask for a process, returning the previous mask.
///
/// `SIGKILL` and `SIGSTOP` cannot be blocked (their bits are always
/// cleared from the stored mask), matching POSIX.
pub fn set_blocked(pid: ProcessId, mask: u64) -> u64 {
    // SIGKILL (bit 8) and SIGSTOP (bit 18) can never be blocked.
    let unblockable = (1u64 << (SIGKILL - 1)) | (1u64 << (SIGSTOP - 1));
    let mask = mask & !unblockable;
    with_states(|states| {
        let state = states.entry(pid).or_default();
        let old = state.blocked;
        state.blocked = mask;
        old
    })
}

/// Get the blocked-signal mask for a process.
#[must_use]
pub fn blocked(pid: ProcessId) -> u64 {
    with_states(|states| states.get(&pid).map(|s| s.blocked).unwrap_or(0))
}

/// Whether `sig` is currently blocked for `pid`.
///
/// An invalid signal number is reported as not blocked — there is no bit for
/// it to be blocked in.
#[must_use]
pub fn is_blocked(pid: ProcessId, sig: u32) -> bool {
    signal_bit(sig).is_some_and(|bit| blocked(pid) & bit != 0)
}

/// Get the pending-signal set for a process (without clearing it).
#[must_use]
pub fn pending(pid: ProcessId) -> u64 {
    with_states(|states| states.get(&pid).map(|s| s.pending).unwrap_or(0))
}

/// Record a *saved* blocked mask awaiting restore (Linux `saved_sigmask` +
/// `TIF_RESTORE_SIGMASK`).
///
/// Called by `rt_sigsuspend` after it installs the temporary suspend mask:
/// `mask` is the original (pre-suspend) blocked mask to restore once a signal
/// has been handled. Consumed by [`take_saved_sigmask`] — either by
/// `emit_linux_rt_frame` (which writes it into `uc_sigmask` for `rt_sigreturn`
/// to restore) or by the no-handler tail of the Linux delivery loop.
pub fn set_saved_sigmask(pid: ProcessId, mask: u64) {
    with_states(|states| {
        states.entry(pid).or_default().saved_sigmask = Some(mask);
    });
}

/// Take (and clear) the saved blocked mask for a process, if one is pending.
///
/// Returns `Some(mask)` exactly once per [`set_saved_sigmask`]; subsequent
/// calls return `None` until the next sigsuspend. A `None` return means no
/// sigsuspend-style mask restore is outstanding.
#[must_use]
pub fn take_saved_sigmask(pid: ProcessId) -> Option<u64> {
    with_states(|states| states.get_mut(&pid).and_then(|s| s.saved_sigmask.take()))
}

// ---------------------------------------------------------------------------
// Posting and consuming signals
// ---------------------------------------------------------------------------

/// Set a signal pending on a process.
///
/// The signal number must be valid (caller checks via [`is_valid_signal`]).
/// Returns `true` if the bit transitioned from clear to set (so the
/// global counter was incremented). This is a pure state operation — it
/// does **not** make any delivery or termination decision; callers that
/// need the no-trampoline fallback should consult [`classify_post`].
pub fn set_pending(pid: ProcessId, sig: u32) -> bool {
    // No recorded sender: default to a generic user-directed siginfo, which
    // is the historical (pre-sender-faithful) behaviour. Callers that know
    // the sender should use [`set_pending_info`].
    set_pending_info(pid, sig, SigInfo::user(0, 0))
}

/// Set a signal pending on a process, recording its source metadata for
/// `siginfo` delivery.
///
/// On a clear→set transition the `info` is recorded and `true` is returned.
/// Re-posting an already-pending standard signal keeps the **first** `info`
/// (Linux standard-signal coalescing) and returns `false` -- so does posting
/// a signal that is pending only through a POSIX timer's entry, as Linux's
/// `legacy_queue` drops a `kill` of a signal any instance of which is queued.
/// (A real-time signal coalesces here too, where Linux would queue it: the
/// standard record is one per signal.) Like
/// [`set_pending`], this is a pure state operation and makes no delivery or
/// termination decision.
pub fn set_pending_info(pid: ProcessId, sig: u32, info: SigInfo) -> bool {
    let Some(bit) = signal_bit(sig) else {
        return false;
    };
    let newly = with_states(|states| {
        let state = states.entry(pid).or_default();
        if state.pending & bit == 0 {
            state.pending |= bit;
            // `bit == 1 << (sig - 1)`, so the slot index is `sig - 1`,
            // computed lint-free from the bit position (0..=63).
            let idx = bit.trailing_zeros() as usize;
            if let Some(slot) = state.infos.get_mut(idx) {
                *slot = Some(info);
            }
            PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    });
    // Wake any signalfd reader blocked on this signal — but only on a
    // clear→set transition (a re-post of an already-pending signal delivers
    // nothing new to drain).  Done after releasing SIGNAL_STATES (leaf-lock
    // discipline); the SIGNAL_STATES → SIGNALFD_WAITERS ordering is the same
    // happens-before edge the reader's register-then-recheck relies on.
    if newly {
        wake_signalfd_waiters(pid, bit);
    }
    newly
}

/// How many timer entries one pass of [`clear_pending`] takes out under the
/// lock; it repeats until a pass comes back short.
const CLEAR_BATCH: usize = 16;

/// Discard every pending instance of the signals in `mask` for a process.
///
/// Used for stop/continue mutual cancellation (a `SIGCONT` discards pending
/// stop signals and a stop discards a pending `SIGCONT`), and when a signal
/// becomes ignored ([`set_ignored`], [`record_disposition`]). Takes each
/// cleared bit off the global pending counter so the fast-path gate in the
/// delivery checkpoint stays accurate.
///
/// A POSIX timer's entry among them is handed to its timer as dequeued --
/// outside the lock, which the timer module must not be called under -- so a
/// periodic timer re-arms rather than waiting for ever on a signal that is
/// gone (the hole Linux 6.6's `flush_sigqueue_mask` leaves, which 6.13 closed
/// with its ignored list).
pub fn clear_pending(pid: ProcessId, mask: u64) {
    if mask == 0 {
        return;
    }
    loop {
        let mut batch: [Option<QueuedTimerSignal>; CLEAR_BATCH] = [None; CLEAR_BATCH];
        let taken = with_states(|states| {
            let Some(state) = states.get_mut(&pid) else {
                return 0;
            };
            let cleared = state.pending & mask;
            if cleared == 0 {
                return 0;
            }
            // Drop the standard record of every signal being cleared.
            let mut rem = cleared;
            while rem != 0 {
                let idx = rem.trailing_zeros() as usize;
                if let Some(slot) = state.infos.get_mut(idx) {
                    *slot = None;
                }
                rem &= rem.wrapping_sub(1); // clear lowest set bit
            }
            // Take their timer entries, a batch at a time (no allocation).
            let mut taken = 0usize;
            state.timer_queue.retain(|q| {
                let hit = signal_bit(q.sig).is_some_and(|b| cleared & b != 0);
                if hit {
                    if let Some(slot) = batch.get_mut(taken) {
                        *slot = Some(*q);
                        taken = taken.saturating_add(1);
                        return false;
                    }
                }
                true
            });
            // A bit stays set only while entries beyond this batch remain.
            state.settle_bits(cleared);
            taken
        });
        for q in batch.iter().take(taken).flatten() {
            // Discarded, not delivered: what the timer answers does not
            // matter, only that it re-arms a periodic timer.
            let _ = crate::proc::posix_timer::signal_dequeued(pid, q.timer_id, q.token, q.info);
        }
        if taken < CLEAR_BATCH {
            break;
        }
    }
}

/// Bit mask of the stop-class signals (`SIGSTOP`, `SIGTSTP`, `SIGTTIN`,
/// `SIGTTOU`). A `SIGCONT` discards any of these that are pending.
#[inline]
#[must_use]
fn stop_signals_mask() -> u64 {
    let mut m = 0u64;
    for s in [SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU] {
        if let Some(b) = signal_bit(s) {
            m |= b;
        }
    }
    m
}

/// Bit for `SIGCONT`. A stop signal discards a pending `SIGCONT`.
#[inline]
#[must_use]
fn sigcont_bit() -> u64 {
    signal_bit(SIGCONT).unwrap_or(0)
}

/// Discard any pending `SIGCONT` for `pid`.
///
/// Called when a stop takes effect (including at the syscall-return
/// checkpoint for a blocked-then-unblocked stop signal), so a `SIGCONT`
/// posted before the stop does not spuriously resume the just-stopped
/// process. The classify-on-post path clears this eagerly for an
/// immediately-effective stop; this is the deferred-stop equivalent.
pub fn discard_pending_cont(pid: ProcessId) {
    clear_pending(pid, sigcont_bit());
}

/// The kernel's decision about what to do with a posted signal.
///
/// Returned by [`classify_post`] so the syscall handler can perform the
/// side effect (set-pending vs. terminate vs. drop) without this module
/// reaching into the process-termination machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostDecision {
    /// Mark the signal pending; it will be delivered to the trampoline on
    /// the target's next return to userspace.
    Deliver,
    /// The target has no trampoline and the signal's default action is
    /// fatal: the caller must terminate the process as killed by this
    /// signal (`pcb::set_killed_by_signal`), which its parent's `wait` then
    /// reports as `WIFSIGNALED`.
    Terminate(u32),
    /// The signal was dropped (ignored), or kept pending for later (a stop
    /// signal that is currently blocked on a process with no handler — it
    /// will take effect at the syscall-return checkpoint once unblocked).
    Drop,
    /// The signal's effect is to **stop** (suspend) the target for job
    /// control. The caller must suspend the target's threads and record the
    /// stop (see `handlers::stop_process_for_signal`). The payload is the
    /// stop signal number, used for the parent's wait-status report.
    Stop(u32),
    /// The signal's effect is to **continue** (resume) a stopped target.
    /// The caller must resume the target's threads and record the continue
    /// (see `handlers::continue_process`). If a handler is also registered,
    /// the signal was additionally marked pending so the handler runs after
    /// the process resumes.
    Continue,
}

/// Decide what to do with a signal posted to `pid`, and record pending
/// state if delivery is chosen.
///
/// * `SIGKILL` is always `Terminate` (never catchable).
/// * `SIGCONT` always continues; `SIGSTOP` always stops.
/// * An **ignored** signal ([`set_ignored`]) is discarded -- unless it is
///   blocked, when it stays pending as Linux keeps it (`sig_ignored`: the
///   action may have changed by the time it is unblocked), to be discarded at
///   delivery if it is still ignored then ([`take_deliverable_info`]).
/// * If the process has a trampoline registered, the signal is marked
///   pending (`Deliver`).
/// * Otherwise the default action decides: terminating signals →
///   `Terminate(sig)`, or kept pending if blocked; everything else →
///   `Drop`.
///
/// The caller is responsible for the actual termination (the kernel's
/// process-kill path) when `Terminate` is returned.
#[must_use]
pub fn classify_post(pid: ProcessId, sig: u32) -> PostDecision {
    classify_post_info(pid, sig, SigInfo::user(0, 0))
}

/// Like [`classify_post`], but records `info` as the source metadata for the
/// posted signal (used to fill the delivered `siginfo_t`). Every pending bit
/// this sets carries the supplied `info`.
#[must_use]
pub fn classify_post_info(pid: ProcessId, sig: u32, info: SigInfo) -> PostDecision {
    // SIGKILL is unconditionally fatal and never delivered to a handler.
    if sig == SIGKILL {
        return PostDecision::Terminate(sig);
    }

    let blocked_now = blocked(pid) & signal_bit(sig).unwrap_or(0) != 0;

    // SIGCONT always resumes a stopped process, regardless of whether a
    // handler is registered -- or of whether it is ignored: POSIX continues
    // the process either way -- and discards any pending stop signal (mutual
    // cancellation). If a handler is registered it is *also* marked pending
    // so the handler runs once the process resumes; an ignored SIGCONT has
    // nothing to run.
    if sig == SIGCONT {
        clear_pending(pid, stop_signals_mask());
        if has_trampoline(pid) && !is_ignored(pid, SIGCONT) {
            set_pending_info(pid, sig, info);
        }
        return PostDecision::Continue;
    }

    // SIGSTOP always stops and is never catchable or blockable. It discards
    // any pending SIGCONT (mutual cancellation).
    if sig == SIGSTOP {
        clear_pending(pid, sigcont_bit());
        return PostDecision::Stop(sig);
    }

    // An ignored signal does nothing -- including the catchable stop
    // signals, which a shell ignores so that ^Z cannot stop it -- whether the
    // target has a handler trampoline or not. Before the kernel knew the
    // ignored set, a target with no trampoline took the default action here:
    // SIGHUP killed a program `nohup` had started.
    if is_ignored(pid, sig) {
        if blocked_now {
            set_pending_info(pid, sig, info);
        }
        return PostDecision::Drop;
    }

    // A registered handler takes precedence for every other catchable
    // signal — including the catchable stop signals (SIGTSTP/TTIN/TTOU),
    // which a handler may choose to ignore rather than stop. Mark it
    // pending for trampoline delivery.
    if has_trampoline(pid) {
        set_pending_info(pid, sig, info);
        return PostDecision::Deliver;
    }

    // No trampoline: the kernel default action decides.
    match default_action(sig) {
        DefaultAction::Terminate => {
            // A blocked signal is not acted on until it is unblocked, fatal
            // or not: it waits, pending, and the syscall-return checkpoint
            // takes the default action once it is deliverable
            // (`deliver_pending_signal`). Until 2026-10-01 a blocked fatal
            // signal killed a process with no trampoline on the spot.
            if blocked_now {
                set_pending_info(pid, sig, info);
                PostDecision::Drop
            } else {
                PostDecision::Terminate(sig)
            }
        }
        DefaultAction::Stop => {
            // Catchable stop signal with no handler. If currently blocked,
            // keep it pending: it stops the process at the syscall-return
            // checkpoint once unblocked (deliver_pending_signal). Otherwise
            // stop now, discarding any pending SIGCONT.
            if blocked_now {
                set_pending_info(pid, sig, info);
                PostDecision::Drop
            } else {
                clear_pending(pid, sigcont_bit());
                PostDecision::Stop(sig)
            }
        }
        DefaultAction::Continue => {
            // Only SIGCONT has the Continue default, handled above; this is
            // reachable only if the default-action table changes. Resume to
            // stay consistent.
            clear_pending(pid, stop_signals_mask());
            PostDecision::Continue
        }
        DefaultAction::Ignore => PostDecision::Drop,
    }
}

/// Pick and consume the lowest-numbered deliverable signal for a process.
///
/// A signal is deliverable if it is pending and not blocked. The chosen
/// signal's pending bit is cleared. Returns the signal number, or `None`
/// if nothing is deliverable.
///
/// A pending signal that is unblocked but **ignored** is discarded here
/// rather than delivered: it was blocked when it was sent, so it was kept
/// ([`classify_post_info`]), and it is still ignored now that it could be
/// delivered -- Linux's `get_signal`, which drops a dequeued `SIG_IGN` signal.
#[must_use]
pub fn take_deliverable(pid: ProcessId) -> Option<u32> {
    take_deliverable_info(pid).map(|(sig, _)| sig)
}

/// What one pass of [`take_deliverable_info`] found under the lock.
enum DeliveryStep {
    /// A standard instance to deliver.
    Deliver(u32, SigInfo),
    /// A timer entry to deliver, once its timer has seen it.
    Timer(QueuedTimerSignal),
    /// A timer entry of an ignored signal, discarded -- its timer still has to
    /// see it, so a periodic one re-arms.
    Discarded(QueuedTimerSignal),
}

/// Like [`take_deliverable`], but also returns the recorded source metadata
/// ([`SigInfo`]) so the Linux `rt_sigframe` delivery path can fill a faithful
/// `siginfo_t`. The chosen instance is removed, and the signal's pending bit
/// cleared once no instance of it is left. Falls back to a generic
/// `SI_USER`/0/0 record if (unexpectedly) a bit is set with no instance.
///
/// A POSIX timer's entry is handed to its timer first, outside the lock
/// ([`crate::proc::posix_timer::signal_dequeued`]): that re-arms a periodic
/// timer and stamps the overrun count into the record, as Linux's
/// `dequeue_signal` does -- or drops an entry its timer has been re-set or
/// deleted since, in which case this looks again.
#[must_use]
pub fn take_deliverable_info(pid: ProcessId) -> Option<(u32, SigInfo)> {
    loop {
        let step = with_states(|states| {
            let state = states.get_mut(&pid)?;
            // Ignored and deliverable: discarded, with their records (see
            // `take_deliverable`).
            let discard = state.pending & !state.blocked & state.ignored;
            if discard != 0 {
                let mut rem = discard;
                while rem != 0 {
                    if let Some(slot) = state.infos.get_mut(rem.trailing_zeros() as usize) {
                        *slot = None;
                    }
                    rem &= rem.wrapping_sub(1); // clear lowest set bit
                }
                // Their timer entries go one per pass, each to its timer.
                let pos = state
                    .timer_queue
                    .iter()
                    .position(|q| signal_bit(q.sig).is_some_and(|b| discard & b != 0));
                let gone = pos.map(|p| state.timer_queue.remove(p));
                state.settle_bits(discard);
                if let Some(q) = gone {
                    return Some(DeliveryStep::Discarded(q));
                }
            }
            let deliverable = state.pending & !state.blocked;
            if deliverable == 0 {
                return None;
            }
            // Lowest set bit = lowest-numbered signal (POSIX delivers low first).
            let bit_index = deliverable.trailing_zeros();
            let bit = 1u64 << bit_index;
            // bit_index is 0..=63, so +1 is 1..=64 — a valid signal number.
            // `saturating_add` keeps this arithmetic-side-effect free.
            let sig = bit_index.saturating_add(1);
            Some(match state.take_instance(sig, bit) {
                Taken::Record(info) => DeliveryStep::Deliver(sig, info),
                Taken::Timer(q) => DeliveryStep::Timer(q),
            })
        })?;
        match step {
            DeliveryStep::Deliver(sig, info) => return Some((sig, info)),
            DeliveryStep::Timer(q) => {
                if let Some(info) =
                    crate::proc::posix_timer::signal_dequeued(pid, q.timer_id, q.token, q.info)
                {
                    return Some((q.sig, info));
                }
                // Stale (its timer re-set or deleted since): dropped; look again.
            }
            DeliveryStep::Discarded(q) => {
                // Ignored, so discarded whatever the timer answers -- Linux's
                // `get_signal` drops it after `dequeue_signal` re-armed the
                // timer, which is the part that matters here.
                let _ = crate::proc::posix_timer::signal_dequeued(pid, q.timer_id, q.token, q.info);
            }
        }
    }
}

/// Pick and consume the lowest-numbered pending signal that is also in
/// `mask`, for a `signalfd` read or a `sigtimedwait`, and return it with its
/// record.
///
/// Unlike [`take_deliverable`], this does **not** consult the blocked
/// mask: a `signalfd` consumes any pending signal that is in the fd's
/// acceptance mask (the process is expected to have blocked those signals
/// so they aren't first delivered to a handler, but the dequeue itself is
/// gated only by the fd mask, matching Linux's `signalfd_dequeue`). The
/// chosen instance is removed, and the bit cleared once no instance is left.
/// Returns `None` if no pending signal falls within `mask`.
///
/// A POSIX timer's entry goes through its timer first, as in
/// [`take_deliverable_info`].
#[must_use]
pub fn take_pending_info_in_mask(pid: ProcessId, mask: u64) -> Option<(u32, SigInfo)> {
    loop {
        let (sig, taken) = with_states(|states| {
            let state = states.get_mut(&pid)?;
            let eligible = state.pending & mask;
            if eligible == 0 {
                return None;
            }
            let bit_index = eligible.trailing_zeros();
            let bit = 1u64 << bit_index;
            // bit_index is 0..=63, so +1 is 1..=64 — a valid signal number.
            let sig = bit_index.saturating_add(1);
            Some((sig, state.take_instance(sig, bit)))
        })?;
        match taken {
            Taken::Record(info) => return Some((sig, info)),
            Taken::Timer(q) => {
                if let Some(info) =
                    crate::proc::posix_timer::signal_dequeued(pid, q.timer_id, q.token, q.info)
                {
                    return Some((q.sig, info));
                }
                // Stale: dropped; look again.
            }
        }
    }
}

/// [`take_pending_info_in_mask`] without the record.
#[must_use]
pub fn take_pending_in_mask(pid: ProcessId, mask: u64) -> Option<u32> {
    take_pending_info_in_mask(pid, mask).map(|(sig, _)| sig)
}

// ---------------------------------------------------------------------------
// POSIX timers' signals
// ---------------------------------------------------------------------------

/// Make room in `pid`'s timer queue for one entry per timer of a process
/// that will have `timers` POSIX timers, creating its signal state if it has
/// none. Called as a timer is created, in process context, so that the
/// expiry -- in the timer interrupt -- never allocates ([`post_timer_signal`]).
///
/// # Errors
///
/// `OutOfMemory` if the room cannot be had; nothing is changed.
pub fn reserve_timer_signals(pid: ProcessId, timers: usize) -> KernelResult<()> {
    with_states(|states| {
        let state = states.entry(pid).or_default();
        if state.timer_queue.capacity() < timers {
            let more = timers.saturating_sub(state.timer_queue.len());
            state
                .timer_queue
                .try_reserve_exact(more)
                .map_err(|_| KernelError::OutOfMemory)?;
        }
        Ok(())
    })
}

/// Queue POSIX timer `timer_id`'s signal `sig` on `pid` with record `info`,
/// for the arming `token` names -- Linux's `send_sigqueue` of the timer's
/// preallocated entry. Safe in the timer interrupt: it never allocates (see
/// [`reserve_timer_signals`]) and wakes with `try_wake`/`defer_wake`.
///
/// - The timer's entry already queued: it now stands for this expiry too (its
///   token and record refreshed), [`TimerPost::AlreadyQueued`].
/// - The signal ignored and not blocked: nothing queued, Linux's
///   `prepare_signal` -- [`TimerPost::Ignored`]. (A blocked signal is queued
///   even if ignored: the action may change before it is unblocked.)
/// - Otherwise queued behind every older instance, whether or not the signal
///   was already pending -- each timer's expiry is delivered on its own.
///
/// Wakes the process's signal waiters when the signal's bit goes from clear
/// to set, as [`set_pending_info`] does.
pub fn post_timer_signal(
    pid: ProcessId,
    sig: u32,
    timer_id: i32,
    token: u64,
    info: SigInfo,
) -> TimerPost {
    let Some(bit) = signal_bit(sig) else {
        return TimerPost::Gone;
    };
    let mut newly = false;
    let result = with_states(|states| {
        let Some(state) = states.get_mut(&pid) else {
            return TimerPost::Gone;
        };
        if let Some(q) = state
            .timer_queue
            .iter_mut()
            .find(|q| q.timer_id == timer_id)
        {
            q.sig = sig;
            q.token = token;
            q.info = info;
            return TimerPost::AlreadyQueued;
        }
        if state.ignored & bit != 0 && state.blocked & bit == 0 {
            return TimerPost::Ignored;
        }
        if state.timer_queue.len() >= state.timer_queue.capacity() {
            // Never, while every timer reserves its room; refuse rather than
            // allocate here.
            return TimerPost::Gone;
        }
        state.timer_queue.push(QueuedTimerSignal {
            sig,
            timer_id,
            token,
            info,
        });
        if state.pending & bit == 0 {
            state.pending |= bit;
            PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
            newly = true;
        }
        TimerPost::Queued
    });
    if newly {
        wake_signalfd_waiters(pid, bit);
    }
    result
}

/// Take timer `timer_id`'s queued signal off `pid`'s queue, if it has one --
/// for a timer being re-set or deleted, whose pending expiry Linux 6.13 no
/// longer delivers. Answers whether there was one.
pub fn remove_timer_signal(pid: ProcessId, timer_id: i32) -> bool {
    with_states(|states| {
        let Some(state) = states.get_mut(&pid) else {
            return false;
        };
        let Some(pos) = state
            .timer_queue
            .iter()
            .position(|q| q.timer_id == timer_id)
        else {
            return false;
        };
        let q = state.timer_queue.remove(pos);
        if let Some(bit) = signal_bit(q.sig) {
            state.settle_bit(q.sig, bit);
        }
        true
    })
}

/// How many POSIX timer signals `pid` has queued, and the room reserved for
/// them -- for the self-tests and `/proc`.
#[must_use]
pub fn timer_signals(pid: ProcessId) -> (usize, usize) {
    with_states(|states| {
        states
            .get(&pid)
            .map_or((0, 0), |s| (s.timer_queue.len(), s.timer_queue.capacity()))
    })
}

/// Returns `true` if any pending signal for `pid` falls within `mask`.
///
/// Used by the `poll`/`select`/`epoll` readiness check for a `signalfd`:
/// the fd is readable exactly when a masked signal is pending. Does not
/// consume anything.
#[must_use]
pub fn has_pending_in_mask(pid: ProcessId, mask: u64) -> bool {
    with_states(|states| states.get(&pid).is_some_and(|s| s.pending & mask != 0))
}

/// Cheap fast-path gate: `true` if any signal might be pending anywhere.
///
/// The syscall-return delivery path calls this before doing any
/// per-process work, so the common (no-signals) case is a single relaxed
/// atomic load.
#[inline]
#[must_use]
pub fn any_pending() -> bool {
    PENDING_COUNT.load(Ordering::Relaxed) != 0
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Synthetic PID base for self-tests (well outside any real PID range).
const TEST_PID_BASE: ProcessId = 0xFFFF_5160_0000;

/// Signal-shim self-tests — pure state machinery (no userspace delivery).
///
/// Verifies the pending/blocked/trampoline bookkeeping, the default-action
/// table, the no-trampoline post classification, and ABI struct layout.
/// The actual asynchronous-delivery path (stack frame building + frame
/// rewrite) is exercised by the userspace POSIX test programs; it cannot
/// be unit-tested here without a ring-3 harness.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[signal] Running signal-shim self-test...");

    test_context_abi()?;
    test_signal_validity()?;
    test_default_actions()?;
    test_trampoline_registry()?;
    test_pending_and_take()?;
    test_blocked_masking()?;
    test_classify_post()?;
    test_on_exec()?;
    test_pending_count_accounting()?;
    test_signalfd_dequeue()?;
    test_signalfd_waiter_registry()?;
    test_take_all_waiters()?;
    test_siginfo_record()?;
    test_altstack_placement()?;
    test_extended_frame()?;
    test_ignored_set()?;
    test_ignored_across_images()?;

    serial_println!("[signal] Signal-shim self-test PASSED (17 tests)");
    Ok(())
}

/// The ignored set at the moment a signal is sent and delivered: discarded
/// when sent unblocked -- with or without a trampoline, which is the `nohup`
/// case -- kept when blocked and discarded once unblocked if still ignored,
/// delivered if the action changed meanwhile; `SIGCONT` continues even
/// ignored; what may be ignored and what a change discards.
fn test_ignored_set() -> KernelResult<()> {
    const SIGHUP_BIT: u64 = 1 << (SIGHUP - 1);
    const SIGUSR1: u32 = 10;
    const SIGUSR1_BIT: u64 = 1 << (SIGUSR1 - 1);
    const SIGTERM: u32 = 15;
    const SIGTERM_BIT: u64 = 1 << (SIGTERM - 1);
    let p = TEST_PID_BASE + 50;
    let q = TEST_PID_BASE + 51;
    let cleanup = || {
        remove(p);
        remove(q);
    };
    let result = (|| -> KernelResult<()> {
        // What may be ignored.
        for bad in [1u64 << (SIGKILL - 1), 1u64 << (SIGSTOP - 1)] {
            check(
                set_ignored(p, SIGHUP_BIT | bad, false) == Err(KernelError::InvalidArgument),
                "SIGKILL/SIGSTOP in the ignored set is refused",
            )?;
            check(ignored(p) == 0, "a refused set changes nothing")?;
        }
        check(
            set_ignored(p, SIGHUP_BIT, false) == Ok(0),
            "first set answers 0",
        )?;
        check(
            is_ignored(p, SIGHUP) && !is_ignored(p, SIGTERM) && !is_ignored(p, 0),
            "is_ignored reads the set",
        )?;

        // Sent unblocked to a process with no trampoline: dropped, where the
        // default action would have killed it.
        check(
            classify_post(p, SIGHUP) == PostDecision::Drop && pending(p) == 0,
            "an ignored SIGHUP with no trampoline is dropped, not fatal",
        )?;
        // ...and with a trampoline: not even pended for it.
        register_trampoline(p, 0x4000);
        check(
            classify_post(p, SIGHUP) == PostDecision::Drop && pending(p) == 0,
            "an ignored signal is not pended for the trampoline",
        )?;
        check(
            classify_post(p, SIGUSR1) == PostDecision::Deliver,
            "a signal not ignored is still delivered",
        )?;
        check(take_deliverable(p) == Some(SIGUSR1), "and taken")?;

        // Blocked when sent: kept pending...
        set_blocked(p, SIGHUP_BIT);
        check(
            classify_post(p, SIGHUP) == PostDecision::Drop && pending(p) == SIGHUP_BIT,
            "an ignored but blocked signal stays pending",
        )?;
        // ...and discarded, not delivered, once unblocked while still ignored.
        set_blocked(p, 0);
        check(
            take_deliverable(p).is_none() && pending(p) == 0,
            "unblocked and still ignored: discarded at delivery",
        )?;
        // A change of action while it waits is honoured.
        set_blocked(p, SIGHUP_BIT);
        let _ = classify_post(p, SIGHUP);
        check(
            set_ignored(p, 0, false) == Ok(SIGHUP_BIT),
            "previous set returned",
        )?;
        check(
            pending(p) == SIGHUP_BIT,
            "un-ignoring keeps the pending one",
        )?;
        set_blocked(p, 0);
        check(
            take_deliverable(p) == Some(SIGHUP),
            "un-ignored before unblocking: delivered",
        )?;

        // Newly ignored pending signals go, blocked or not; one already
        // ignored stays.
        set_blocked(p, SIGUSR1_BIT | SIGTERM_BIT);
        set_ignored(p, SIGUSR1_BIT, false)?;
        let _ = classify_post(p, SIGUSR1); // ignored + blocked: kept
        set_pending(p, SIGTERM);
        set_ignored(p, SIGUSR1_BIT | SIGTERM_BIT, false)?;
        check(
            pending(p) == SIGUSR1_BIT,
            "newly ignored SIGTERM discarded, already-ignored SIGUSR1 kept",
        )?;
        set_ignored(p, 0, false)?;
        set_blocked(p, 0);
        let _ = take_deliverable(p);

        // SIGCONT continues even ignored, and runs no handler.
        set_ignored(p, 1 << (SIGCONT - 1), false)?;
        check(
            classify_post(p, SIGCONT) == PostDecision::Continue && pending(p) == 0,
            "an ignored SIGCONT continues without pending a handler",
        )?;
        set_ignored(p, 0, false)?;

        // A blocked fatal signal on a process with no trampoline waits.
        check(
            classify_post(q, SIGTERM) == PostDecision::Terminate(SIGTERM),
            "unblocked SIGTERM with no trampoline is fatal",
        )?;
        set_blocked(q, SIGTERM_BIT);
        check(
            classify_post(q, SIGTERM) == PostDecision::Drop && pending(q) == SIGTERM_BIT,
            "blocked SIGTERM with no trampoline is kept pending, not fatal yet",
        )?;

        // The Linux shim's one-at-a-time record.
        set_pending(q, SIGCHLD);
        record_disposition(q, SIGCHLD, Disposition::Default, true);
        check(
            pending(q) & (1 << (SIGCHLD - 1)) == 0 && nocldwait(q) && !is_ignored(q, SIGCHLD),
            "SIG_DFL for a default-ignored signal discards it; SA_NOCLDWAIT kept",
        )?;
        set_pending(q, SIGUSR1);
        record_disposition(q, SIGUSR1, Disposition::Default, false);
        check(
            pending(q) & SIGUSR1_BIT != 0,
            "SIG_DFL for a default-fatal signal discards nothing",
        )?;
        record_disposition(q, SIGUSR1, Disposition::Ignore, false);
        check(
            is_ignored(q, SIGUSR1) && pending(q) & SIGUSR1_BIT == 0,
            "SIG_IGN records it and discards the pending one",
        )?;
        record_disposition(q, SIGUSR1, Disposition::Handler, false);
        check(!is_ignored(q, SIGUSR1), "a handler takes it out")?;
        record_disposition(q, SIGKILL, Disposition::Ignore, false);
        check(
            !is_ignored(q, SIGKILL),
            "SIGKILL can never be recorded ignored",
        )?;
        record_disposition(q, SIGCHLD, Disposition::Handler, false);
        record_disposition(q, SIGUSR1, Disposition::Ignore, true);
        check(!nocldwait(q), "SA_NOCLDWAIT is SIGCHLD's alone")?;
        Ok(())
    })();
    cleanup();
    if result.is_ok() {
        serial_println!("[signal]   ignored set at send and delivery: OK");
    }
    result
}

/// The ignored set across images: `exec` keeps it and drops `SA_NOCLDWAIT`,
/// `fork` copies both, a spawned child gets its parent's blocked mask and
/// ignored set less `sigdefault` (or the mask it was given), and a kernel
/// parent passes nothing on.
fn test_ignored_across_images() -> KernelResult<()> {
    const SIGHUP_BIT: u64 = 1 << (SIGHUP - 1);
    const SIGINT_BIT: u64 = 1 << 1;
    const SIGQUIT_BIT: u64 = 1 << 2;
    let parent = TEST_PID_BASE + 60;
    let forked = TEST_PID_BASE + 61;
    let spawned = TEST_PID_BASE + 62;
    let given = TEST_PID_BASE + 63;
    let orphan = TEST_PID_BASE + 64;
    let all = [parent, forked, spawned, given, orphan];
    let result = (|| -> KernelResult<()> {
        set_ignored(parent, SIGHUP_BIT | SIGQUIT_BIT, true)?;
        set_blocked(parent, SIGINT_BIT);

        inherit_for_fork(parent, forked);
        check(
            ignored(forked) == SIGHUP_BIT | SIGQUIT_BIT && nocldwait(forked),
            "fork copies the ignored set and SA_NOCLDWAIT",
        )?;

        start_spawned(parent, spawned, None, SIGQUIT_BIT);
        check(
            ignored(spawned) == SIGHUP_BIT && blocked(spawned) == SIGINT_BIT,
            "spawn: the parent's ignored set less sigdefault, the parent's mask",
        )?;
        check(
            !nocldwait(spawned),
            "a spawned image starts without SA_NOCLDWAIT",
        )?;

        let unblockable = (1u64 << (SIGKILL - 1)) | (1u64 << (SIGSTOP - 1));
        start_spawned(parent, given, Some(SIGQUIT_BIT | unblockable), 0);
        check(
            blocked(given) == SIGQUIT_BIT && ignored(given) == SIGHUP_BIT | SIGQUIT_BIT,
            "spawn with a mask: that mask, less SIGKILL/SIGSTOP",
        )?;

        start_spawned(0, orphan, None, 0);
        check(
            ignored(orphan) == 0 && blocked(orphan) == 0,
            "a kernel parent passes nothing on",
        )?;

        on_exec(parent);
        check(
            ignored(parent) == SIGHUP_BIT | SIGQUIT_BIT && !nocldwait(parent),
            "exec keeps the ignored set and drops SA_NOCLDWAIT",
        )?;
        Ok(())
    })();
    for pid in all {
        remove(pid);
    }
    if result.is_ok() {
        serial_println!("[signal]   ignored set across exec, fork and spawn: OK");
    }
    result
}

/// Verify the `SA_ONSTACK` frame placement decision: every branch of
/// [`altstack_top_for`], plus inheritance across `fork` and the clear on `exec`.
///
/// Worth the length because the decision is invisible from userspace until it is
/// wrong, and wrong in two opposite directions. Placing a frame on the alternate
/// stack when the handler did not ask steals a stack it never offered; *not*
/// placing it there when the handler did ask is the bug this exists to fix, and
/// it only shows up when the interrupted stack has already overflowed -- i.e. in
/// the one situation nobody can debug comfortably.
fn test_altstack_placement() -> KernelResult<()> {
    let p = TEST_PID_BASE + 10;
    const SP: u64 = 0x5000_0000;
    const SIZE: u64 = 64 * 1024;
    const TOP: u64 = SP + SIZE;
    const SIGSEGV: u32 = 11;
    const SIGUSR1: u32 = 10;

    // Nothing registered: always the interrupted stack.
    check(
        altstack_top_for(p, SIGSEGV, 0x7fff_0000).is_none(),
        "no alternate stack registered -> interrupted stack",
    )?;

    // Registered, but this signal's handler never asked for it. Using it here
    // would hand a handler a stack it did not request.
    set_altstack(p, SP, SIZE, 1 << (SIGUSR1 - 1));
    check(
        altstack_top_for(p, SIGSEGV, 0x7fff_0000).is_none(),
        "SA_ONSTACK clear for this signal -> interrupted stack",
    )?;
    check(
        altstack_top_for(p, SIGUSR1, 0x7fff_0000) == Some(TOP),
        "SA_ONSTACK set for this signal -> top of the alternate stack",
    )?;

    // The frame grows down from one past the end, not from the base.
    set_altstack(p, SP, SIZE, 1 << (SIGSEGV - 1));
    check(
        altstack_top_for(p, SIGSEGV, 0x7fff_0000) == Some(TOP),
        "top is sp + size",
    )?;

    // The load-bearing refusal: a signal arriving while a handler is already
    // running on the alternate stack must not restart at the top, or it
    // overwrites the frames of the handler it interrupted.
    check(
        altstack_top_for(p, SIGSEGV, SP).is_none(),
        "already at the base of the alternate stack -> refuse",
    )?;
    check(
        altstack_top_for(p, SIGSEGV, SP + SIZE / 2).is_none(),
        "already inside the alternate stack -> refuse",
    )?;
    check(
        altstack_top_for(p, SIGSEGV, TOP - 1).is_none(),
        "last byte of the alternate stack is inside it -> refuse",
    )?;
    // One past the end is *not* inside: that is the first address a frame built
    // at the top occupies, so treating it as inside would refuse the very case
    // this feature serves.
    check(
        altstack_top_for(p, SIGSEGV, TOP) == Some(TOP),
        "one past the end is outside -> place the frame",
    )?;
    check(
        altstack_top_for(p, SIGSEGV, SP - 1) == Some(TOP),
        "one below the base is outside -> place the frame",
    )?;

    // size == 0 unregisters, even with the mask still set.
    set_altstack(p, SP, 0, 1 << (SIGSEGV - 1));
    check(
        altstack_top_for(p, SIGSEGV, 0x7fff_0000).is_none(),
        "size 0 unregisters",
    )?;

    // Signal 0 and out-of-range numbers are refused rather than shifting by a
    // bogus amount.
    set_altstack(p, SP, SIZE, u64::MAX);
    check(
        altstack_top_for(p, 0, 0x7fff_0000).is_none(),
        "signal 0 is not a signal",
    )?;
    check(
        altstack_top_for(p, NSIG + 1, 0x7fff_0000).is_none(),
        "signal above NSIG is refused",
    )?;

    // fork inherits both, per sigaltstack(2).
    let child = TEST_PID_BASE + 11;
    set_altstack(p, SP, SIZE, 1 << (SIGSEGV - 1));
    inherit_for_fork(p, child);
    check(
        altstack_top_for(child, SIGSEGV, 0x7fff_0000) == Some(TOP),
        "fork inherits the alternate stack and the mask",
    )?;

    // exec clears them: the address named a buffer in an address space that no
    // longer exists, so keeping it would put the next frame in a dead program.
    on_exec(child);
    check(
        altstack_top_for(child, SIGSEGV, 0x7fff_0000).is_none(),
        "exec clears the alternate stack",
    )?;

    // Leave no state behind for the next test.
    set_altstack(p, 0, 0, 0);
    set_altstack(child, 0, 0, 0);

    serial_println!("[signal]   SA_ONSTACK frame placement: OK");
    Ok(())
}

/// Verify the per-signal `siginfo` record path: posted source metadata is
/// recorded on the clear→set transition, coalesces (first-wins) on re-post of
/// a standard signal, is delivered by `take_deliverable_info`, and is dropped
/// by `clear_pending`.
fn test_siginfo_record() -> KernelResult<()> {
    let p = TEST_PID_BASE + 9;
    register_trampoline(p, 0x4000);

    // Record SI_TKILL with a sender identity for signal 10.
    let tk = SigInfo::tkill(4321, 1000);
    check(set_pending_info(p, 10, tk), "set_pending_info 10 newly")?;
    // First-wins coalescing: a second post with different info is ignored.
    check(
        !set_pending_info(p, 10, SigInfo::user(9, 9)),
        "re-post 10 coalesces (false)",
    )?;
    // Plain set_pending on a fresh signal records the SI_USER/0/0 default.
    check(set_pending(p, 11), "set_pending 11 newly")?;

    // take_deliverable_info delivers the *first* recorded info (lowest sig first).
    match take_deliverable_info(p) {
        Some((10, info)) => {
            check(info.code == si_code::SI_TKILL, "delivered code SI_TKILL")?;
            check(info.sender_pid == 4321, "delivered sender_pid")?;
            check(info.sender_uid == 1000, "delivered sender_uid")?;
        }
        other => {
            serial_println!("[signal]   FAIL: expected (10, SI_TKILL), got {other:?}");
            return Err(KernelError::InternalError);
        }
    }
    match take_deliverable_info(p) {
        Some((11, info)) => {
            check(info.code == si_code::SI_USER, "default code SI_USER")?;
            check(
                info.sender_pid == 0 && info.sender_uid == 0,
                "default no sender",
            )?;
        }
        other => {
            serial_println!("[signal]   FAIL: expected (11, SI_USER), got {other:?}");
            return Err(KernelError::InternalError);
        }
    }
    check(take_deliverable_info(p).is_none(), "nothing left")?;

    // clear_pending drops the info slot too: a re-post after clear records anew.
    set_pending_info(p, 12, SigInfo::kernel());
    clear_pending(p, signal_bit(12).unwrap_or(0));
    check(pending(p) == 0, "cleared 12")?;
    check(
        set_pending_info(p, 12, SigInfo::user(7, 7)),
        "12 newly again",
    )?;
    match take_deliverable_info(p) {
        Some((12, info)) => {
            check(
                info.code == si_code::SI_USER && info.sender_pid == 7,
                "fresh info after clear",
            )?;
        }
        other => {
            serial_println!("[signal]   FAIL: expected (12, SI_USER/7), got {other:?}");
            return Err(KernelError::InternalError);
        }
    }

    remove(p);
    serial_println!("[signal]   siginfo record/deliver/coalesce: OK");
    Ok(())
}

/// The extended native frame
/// (`requests/d-a-put-each-signal-s-siginfo-in-the-native-frame.md`): which
/// frame a trampoline reads, how that travels with it, where the frame goes,
/// and the records the tail is filled from -- `SIGCHLD`'s status and
/// `sigqueue`'s value among them.
fn test_extended_frame() -> KernelResult<()> {
    let p = TEST_PID_BASE + 12;
    let child = TEST_PID_BASE + 13;

    // The choice goes with the trampoline.
    register_trampoline_frame(p, 0x4000, true);
    check(extended_frame(p), "registered with the flag -> extended")?;
    check(
        trampoline_frame(p) == Some((0x4000, true)),
        "trampoline and frame read together",
    )?;
    inherit_for_fork(p, child);
    check(
        trampoline_frame(child) == Some((0x4000, true)),
        "a fork keeps the trampoline's frame",
    )?;
    on_exec(child);
    check(
        !extended_frame(child) && trampoline_frame(child).is_none(),
        "exec drops the trampoline and its frame",
    )?;
    register_trampoline(p, 0x5000);
    check(
        trampoline_frame(p) == Some((0x5000, false)),
        "registering without the flag -> short frame",
    )?;
    register_trampoline_frame(p, 0, true);
    check(
        !extended_frame(p) && trampoline_frame(p).is_none(),
        "unregistering cannot leave an extended frame behind",
    )?;

    // Placement: the context 16-byte aligned, the frame below the top with
    // less than 16 bytes of slack, the return slot 8 below the context.
    for base in [0x7fff_0000u64, 0x7fff_0008, 0x7fff_fff7] {
        for size in [
            SIGNAL_CONTEXT_SIZE as u64,
            SIGNAL_FRAME_EXTENDED_SIZE as u64,
        ] {
            let (ctx, rsp) = frame_placement(base, size);
            let end = ctx.wrapping_add(size);
            check(ctx % 16 == 0, "context 16-byte aligned")?;
            check(
                rsp == ctx.wrapping_sub(8) && rsp % 16 == 8,
                "return slot 8 below",
            )?;
            check(
                end <= base && base.wrapping_sub(end) < 16,
                "frame just below the top",
            )?;
        }
    }

    // The tail is the record, field for field.
    let q = SigInfo::queued(77, 1000, 0x1234_5678_9abc_def0);
    check(
        SignalInfoTail::from_info(&q)
            == SignalInfoTail {
                si_code: si_code::SI_QUEUE,
                si_pid: 77,
                si_uid: 1000,
                pad: 0,
                si_value: 0x1234_5678_9abc_def0,
            },
        "sigqueue's tail",
    )?;
    check(
        SignalInfoTail::from_info(&SigInfo::kernel()).si_code == si_code::SI_KERNEL,
        "a kernel signal's tail says SI_KERNEL",
    )?;

    // SIGCHLD carries how the child ended, as wait reports it.
    use crate::proc::pcb::{CrashInfo, ExitInfo, JobControlEvent};
    let exited = ExitInfo::exited(3);
    let killed = ExitInfo::killed(9);
    // An exit with a status a signal death's code would read as is still
    // an exit (requests/b-ad-an-exit-status-of-128-to-255-...).
    let exited_137 = ExitInfo::exited(137);
    let crashed = ExitInfo {
        exit_code: crate::proc::pcb::crash_exit_code(8),
        crash: Some(CrashInfo {
            exception_code: 8,
            faulting_rip: 0,
            aux: 0,
            thread_id: 0,
        }),
        term_signal: None,
    };
    check(
        exited.sigchld_code_and_status() == (si_code::CLD_EXITED, 3),
        "exit 3 -> CLD_EXITED, 3",
    )?;
    check(
        killed.sigchld_code_and_status() == (si_code::CLD_KILLED, 9),
        "killed by 9 -> CLD_KILLED, 9",
    )?;
    check(
        exited_137.sigchld_code_and_status() == (si_code::CLD_EXITED, 137),
        "exit 137 -> CLD_EXITED, 137, not a kill",
    )?;
    check(
        crashed.sigchld_code_and_status() == (si_code::CLD_KILLED, 11),
        "a crash -> CLD_KILLED, SIGSEGV (no core file is written)",
    )?;
    check(
        JobControlEvent::Stopped(SIGTSTP).sigchld_code_and_status() == (si_code::CLD_STOPPED, 20),
        "stopped by SIGTSTP -> CLD_STOPPED, 20",
    )?;
    check(
        JobControlEvent::Continued.sigchld_code_and_status() == (si_code::CLD_CONTINUED, 18),
        "continued -> CLD_CONTINUED, SIGCONT",
    )?;
    let chld = SigInfo::child(42, 1000, killed.sigchld_code_and_status());
    check(
        chld.code == si_code::CLD_KILLED
            && chld.sender_pid == 42
            && chld.sender_uid == 1000
            && chld.value == 9,
        "SIGCHLD's record: code, child, uid, status in the value",
    )?;
    // si_status is an int: a negative one is reinterpreted, not widened.
    check(
        SigInfo::child(42, 0, (si_code::CLD_EXITED, -1)).value == 0xFFFF_FFFF,
        "si_status fills the low half of the value slot",
    )?;

    remove(p);
    remove(child);
    serial_println!("[signal]   extended frame, its placement and records: OK");
    Ok(())
}

/// Helper: assert a condition, logging and returning an error on failure.
fn check(cond: bool, what: &str) -> KernelResult<()> {
    if cond {
        Ok(())
    } else {
        serial_println!("[signal]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

fn test_context_abi() -> KernelResult<()> {
    check(SIGNAL_CONTEXT_SIZE == 17 * 8, "SignalContext size == 136")?;
    check(
        SIGNAL_CONTEXT_SIZE == core::mem::size_of::<SignalContext>(),
        "constant matches size_of",
    )?;
    check(
        core::mem::align_of::<SignalContext>() == 8,
        "SignalContext align == 8",
    )?;
    serial_println!("[signal]   context ABI ({SIGNAL_CONTEXT_SIZE}B): OK");
    Ok(())
}

fn test_signal_validity() -> KernelResult<()> {
    check(!is_valid_signal(0), "0 invalid")?;
    check(is_valid_signal(1), "1 valid")?;
    check(is_valid_signal(NSIG), "NSIG valid")?;
    check(!is_valid_signal(NSIG + 1), "NSIG+1 invalid")?;
    check(signal_bit(1) == Some(1), "bit(1)==1")?;
    check(signal_bit(64) == Some(1u64 << 63), "bit(64)==1<<63")?;
    check(signal_bit(0).is_none(), "bit(0)==None")?;
    serial_println!("[signal]   signal validity: OK");
    Ok(())
}

fn test_default_actions() -> KernelResult<()> {
    check(
        default_action(9) == DefaultAction::Terminate,
        "SIGKILL term",
    )?;
    check(
        default_action(15) == DefaultAction::Terminate,
        "SIGTERM term",
    )?;
    check(default_action(17) == DefaultAction::Ignore, "SIGCHLD ign")?;
    check(default_action(28) == DefaultAction::Ignore, "SIGWINCH ign")?;
    check(default_action(19) == DefaultAction::Stop, "SIGSTOP stop")?;
    check(
        default_action(18) == DefaultAction::Continue,
        "SIGCONT cont",
    )?;
    check(default_action(34) == DefaultAction::Terminate, "RT term")?;
    serial_println!("[signal]   default-action table: OK");
    Ok(())
}

fn test_trampoline_registry() -> KernelResult<()> {
    let p = TEST_PID_BASE + 1;
    check(!has_trampoline(p), "initially no trampoline")?;
    register_trampoline(p, 0x4000);
    check(trampoline(p) == Some(0x4000), "trampoline stored")?;
    check(has_trampoline(p), "has_trampoline true")?;
    register_trampoline(p, 0);
    check(!has_trampoline(p), "unregister clears")?;
    remove(p);
    serial_println!("[signal]   trampoline registry: OK");
    Ok(())
}

fn test_pending_and_take() -> KernelResult<()> {
    let p = TEST_PID_BASE + 2;
    register_trampoline(p, 0x4000);
    check(pending(p) == 0, "no pending initially")?;
    check(set_pending(p, 10), "set 10 returns true")?;
    check(set_pending(p, 2), "set 2 returns true")?;
    check(!set_pending(p, 10), "re-set 10 returns false")?;
    check(pending(p) == ((1 << 9) | (1 << 1)), "pending mask")?;
    // Lowest-numbered first.
    check(take_deliverable(p) == Some(2), "take 2 first")?;
    check(take_deliverable(p) == Some(10), "take 10 next")?;
    check(take_deliverable(p).is_none(), "nothing left")?;
    remove(p);
    serial_println!("[signal]   pending set/take: OK");
    Ok(())
}

fn test_blocked_masking() -> KernelResult<()> {
    let p = TEST_PID_BASE + 3;
    register_trampoline(p, 0x4000);
    set_pending(p, 5);
    set_blocked(p, 1 << 4); // block signal 5
    check(take_deliverable(p).is_none(), "blocked not deliverable")?;
    set_blocked(p, 0);
    check(take_deliverable(p) == Some(5), "unblocked deliverable")?;
    // SIGKILL/SIGSTOP cannot be blocked.
    let p2 = TEST_PID_BASE + 30;
    let requested = (1u64 << (SIGKILL - 1)) | (1u64 << (SIGSTOP - 1)) | (1u64 << 0);
    set_blocked(p2, requested);
    check(blocked(p2) == (1u64 << 0), "KILL/STOP unblockable")?;
    remove(p);
    remove(p2);
    serial_println!("[signal]   blocked masking: OK");
    Ok(())
}

fn test_classify_post() -> KernelResult<()> {
    let p = TEST_PID_BASE + 4;
    check(
        classify_post(p, SIGKILL) == PostDecision::Terminate(SIGKILL),
        "no-tramp SIGKILL terminate",
    )?;
    check(
        classify_post(p, 15) == PostDecision::Terminate(15),
        "no-tramp SIGTERM terminate",
    )?;
    check(
        classify_post(p, 17) == PostDecision::Drop,
        "no-tramp SIGCHLD drop",
    )?;
    register_trampoline(p, 0x4000);
    check(
        classify_post(p, 15) == PostDecision::Deliver,
        "tramp SIGTERM deliver",
    )?;
    check(
        pending(p) & (1 << 14) == (1 << 14),
        "SIGTERM pending after deliver",
    )?;
    check(
        classify_post(p, SIGKILL) == PostDecision::Terminate(SIGKILL),
        "SIGKILL terminate even with tramp",
    )?;
    remove(p);

    // --- Stop / continue classification ---
    let s = TEST_PID_BASE + 40;
    // SIGSTOP stops even with no handler and is uncatchable.
    check(
        classify_post(s, SIGSTOP) == PostDecision::Stop(SIGSTOP),
        "no-tramp SIGSTOP stop",
    )?;
    register_trampoline(s, 0x4000);
    check(
        classify_post(s, SIGSTOP) == PostDecision::Stop(SIGSTOP),
        "SIGSTOP stop even with tramp (uncatchable)",
    )?;
    // Catchable stop signal with a handler → delivered to the trampoline.
    check(
        classify_post(s, SIGTSTP) == PostDecision::Deliver,
        "tramp SIGTSTP deliver",
    )?;
    // SIGCONT with a handler → Continue *and* marked pending for the handler.
    check(
        classify_post(s, SIGCONT) == PostDecision::Continue,
        "tramp SIGCONT continue",
    )?;
    check(
        pending(s) & (1 << (SIGCONT - 1)) != 0,
        "SIGCONT pending for handler after continue",
    )?;
    remove(s);

    // Catchable stop signal with no handler → stop now.
    let s2 = TEST_PID_BASE + 41;
    check(
        classify_post(s2, SIGTSTP) == PostDecision::Stop(SIGTSTP),
        "no-tramp SIGTSTP stop",
    )?;
    remove(s2);

    // Mutual cancellation: a pending stop is discarded by SIGCONT, and a
    // pending SIGCONT is discarded by a stop.
    let s3 = TEST_PID_BASE + 42;
    register_trampoline(s3, 0x4000);
    set_pending(s3, SIGTSTP); // pretend a stop was queued for the handler
    let _ = classify_post(s3, SIGCONT);
    check(
        pending(s3) & (1 << (SIGTSTP - 1)) == 0,
        "SIGCONT discards pending stop",
    )?;
    set_pending(s3, SIGCONT);
    let _ = classify_post(s3, SIGSTOP);
    check(
        pending(s3) & (1 << (SIGCONT - 1)) == 0,
        "stop discards pending SIGCONT",
    )?;
    remove(s3);

    // Blocked catchable stop with no handler → kept pending (Drop), not an
    // immediate stop.
    let s4 = TEST_PID_BASE + 43;
    set_blocked(s4, 1u64 << (SIGTSTP - 1));
    check(
        classify_post(s4, SIGTSTP) == PostDecision::Drop,
        "blocked no-tramp SIGTSTP kept pending",
    )?;
    check(
        pending(s4) & (1 << (SIGTSTP - 1)) != 0,
        "blocked SIGTSTP is pending",
    )?;
    remove(s4);

    serial_println!("[signal]   classify_post: OK");
    Ok(())
}

fn test_on_exec() -> KernelResult<()> {
    let p = TEST_PID_BASE + 5;
    register_trampoline(p, 0x4000);
    set_pending(p, 12);
    set_blocked(p, 1 << 0);
    on_exec(p);
    check(!has_trampoline(p), "exec clears trampoline")?;
    check(
        blocked(p) == 1 << 0,
        "exec keeps the blocked mask (POSIX execve)",
    )?;
    check(
        pending(p) & (1 << 11) == (1 << 11),
        "exec preserves pending",
    )?;
    remove(p);
    serial_println!("[signal]   on_exec semantics: OK");
    Ok(())
}

fn test_pending_count_accounting() -> KernelResult<()> {
    let p = TEST_PID_BASE + 6;
    register_trampoline(p, 0x4000);
    let before = PENDING_COUNT.load(Ordering::Relaxed);
    set_pending(p, 3);
    set_pending(p, 4);
    check(
        PENDING_COUNT.load(Ordering::Relaxed) == before.saturating_add(2),
        "count incremented by 2",
    )?;
    remove(p); // removing a process with 2 pending should drop the count
    check(
        PENDING_COUNT.load(Ordering::Relaxed) == before,
        "count restored after remove",
    )?;
    serial_println!("[signal]   pending-count accounting: OK");
    Ok(())
}

fn test_signalfd_dequeue() -> KernelResult<()> {
    let p = TEST_PID_BASE + 7;
    // Signal 5 and signal 10 pending; a signalfd masks only {5, 7}.
    set_pending(p, 5);
    set_pending(p, 10);
    let fd_mask = (1u64 << 4) | (1u64 << 6); // signals 5 and 7
    check(
        has_pending_in_mask(p, fd_mask),
        "signal 5 visible to fd mask",
    )?;
    check(
        !has_pending_in_mask(p, 1u64 << 6),
        "signal 7 not pending → mask {7} not readable",
    )?;
    // Dequeue: only signal 5 is in the mask (10 is not), so 5 comes out
    // and 10 stays pending.
    check(
        take_pending_in_mask(p, fd_mask) == Some(5),
        "dequeue 5 from fd mask",
    )?;
    check(
        take_pending_in_mask(p, fd_mask).is_none(),
        "no further masked signal after 5 consumed",
    )?;
    check(
        pending(p) & (1u64 << 9) == (1u64 << 9),
        "signal 10 still pending",
    )?;
    // Dequeue ignores the blocked mask (unlike take_deliverable).
    set_blocked(p, 1u64 << 9); // block signal 10
    check(
        take_pending_in_mask(p, 1u64 << 9) == Some(10),
        "blocked signal still dequeued by signalfd",
    )?;
    remove(p);
    serial_println!("[signal]   signalfd dequeue (mask/blocked-independent): OK");
    Ok(())
}

/// Exercise the signalfd waiter registry's pure partition logic
/// (register / take-matching / deregister) without touching the scheduler.
fn test_signalfd_waiter_registry() -> KernelResult<()> {
    let p = TEST_PID_BASE + 8;
    let mask_a = 1u64 << 4; // accepts signal 5
    let mask_b = 1u64 << 9; // accepts signal 10
    let task_a: TaskId = 0xA11;
    let task_b: TaskId = 0xB22;

    // Two tasks waiting on disjoint masks.
    register_signalfd_waiter(p, task_a, mask_a);
    register_signalfd_waiter(p, task_b, mask_b);

    // A non-matching bit wakes nobody and leaves both registered.
    check(
        take_matching_signalfd_waiters(p, 1u64 << 20).is_empty(),
        "non-matching bit takes no waiter",
    )?;

    // Signal 5's bit matches only task_a.
    let woken = take_matching_signalfd_waiters(p, mask_a);
    check(
        woken.len() == 1 && woken.first() == Some(&task_a),
        "bit{5} takes only task_a",
    )?;
    // task_a is now gone; re-taking the same bit yields nothing.
    check(
        take_matching_signalfd_waiters(p, mask_a).is_empty(),
        "task_a not taken twice",
    )?;

    // Idempotent re-registration updates the mask rather than duplicating.
    register_signalfd_waiter(p, task_b, mask_a | mask_b);
    let woken = take_matching_signalfd_waiters(p, mask_a);
    check(
        woken.len() == 1 && woken.first() == Some(&task_b),
        "remask wakes task_b on bit{5}",
    )?;

    // Deregister of an already-taken / unknown task is a harmless no-op,
    // and the registry entry for p is cleaned up once empty.
    deregister_signalfd_waiter(p, task_a);
    deregister_signalfd_waiter(p, task_b);
    check(
        take_matching_signalfd_waiters(p, !0u64).is_empty(),
        "registry empty after deregister",
    )?;
    serial_println!("[signal]   signalfd waiter registry (partition logic): OK");
    Ok(())
}

/// Exercise `take_all_waiters` — the mask-independent drain used by
/// [`wake_all_waiters`] on the `rt_sigprocmask` unblock path.
fn test_take_all_waiters() -> KernelResult<()> {
    let p = TEST_PID_BASE + 9;
    let task_a: TaskId = 0xA1;
    let task_b: TaskId = 0xB2;
    let task_c: TaskId = 0xC3;

    // Empty registry drains to nothing.
    check(
        take_all_waiters(p).is_empty(),
        "empty pid drains to nothing",
    )?;

    // Disjoint masks (including one that covers no real signal) all drain —
    // the point of wake_all_waiters is that the mask is ignored. task_b's
    // mask deliberately excludes signal 5 to prove drain ignores the mask.
    register_signalfd_waiter(p, task_a, 1u64 << 4); // accepts signal 5
    register_signalfd_waiter(p, task_b, 1u64 << 20); // accepts signal 21 only
    register_signalfd_waiter(p, task_c, 0); // accepts nothing

    let mut drained = take_all_waiters(p);
    drained.sort_unstable();
    check(
        drained == [task_a, task_b, task_c],
        "all three waiters drained",
    )?;

    // Registry is empty afterwards (entry removed), so a re-drain is empty
    // and a mask-based take finds nothing either.
    check(take_all_waiters(p).is_empty(), "registry empty after drain")?;
    check(
        take_matching_signalfd_waiters(p, !0u64).is_empty(),
        "no stale entries after drain",
    )?;
    serial_println!("[signal]   take_all_waiters (mask-independent drain): OK");
    Ok(())
}
