//! ptrace: one process controlling another's threads -- the kernel half of a
//! debugger. Linux's `kernel/ptrace.c` and the stops of `kernel/signal.c`,
//! for both ABIs: the Linux `ptrace(2)` and the native `SYS_PTRACE`, which
//! takes the same arguments and answers the same way (design-decisions 1547;
//! lane D's `requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`).
//!
//! ## Who may trace whom
//!
//! Tracing is gated by the `DEBUG` right on a `Process` capability over the
//! target (design-decisions 24, the operator's): never by PID or user id. A
//! program a debugger starts consents to it: the child of `gdb ./prog` calls
//! `ptrace(PTRACE_TRACEME)` before it `exec`s the program, and that call gives
//! its parent `DEBUG` over its own process and makes the parent its tracer
//! ([`traceme`]). Every later request checks the right again, so a revoked
//! right ends the access. Attaching to a program the debugger did not start
//! needs a right nothing hands out yet (the broker of design-decisions 24), so
//! `PTRACE_ATTACH` and `PTRACE_SEIZE` answer `EPERM`.
//!
//! ## Stops
//!
//! A traced thread stops -- parks in the kernel, its user registers copied
//! here -- at:
//!
//! - **signal-delivery-stop**: every signal headed for it but `SIGKILL`, on
//!   its way back to user mode, before the signal's disposition is looked at
//!   ([`signal_stop`]). The tracer chooses what is delivered: the signal,
//!   another one, or none. A traced process's signals are therefore all
//!   queued -- none acts when it is sent (`signal::classify`).
//! - **a fault's stop**: an exception it takes from user mode -- `int3`, a
//!   single step, a bad access -- stops it first, from the exception handler,
//!   with the signal the fault raises ([`fault_stop`]).
//! - **the exec stop**: after a successful `exec`, before the new program's
//!   first instruction: `SIGTRAP`, or with `PTRACE_O_TRACEEXEC` the
//!   `PTRACE_EVENT_EXEC` stop ([`after_exec`]).
//!
//! The tracer learns of a stop from `wait` -- `WIFSTOPPED`, `WSTOPSIG` the
//! signal, `status >> 16` the event -- whether or not it asked for
//! `WUNTRACED` ([`take_stop_report`]), and lets the thread go with
//! `PTRACE_CONT`, `PTRACE_SINGLESTEP`, `PTRACE_DETACH` or `PTRACE_KILL`.
//!
//! The stopping thread publishes its stop, wakes the tracer, and parks; the
//! tracer's resume sets what the thread does next and unparks it. Both take
//! the one lock here, and the thread marks itself suspended under it, so a
//! resume that arrives between the publication and the park is not lost.
//!
//! ## Who is "the thread"
//!
//! A request names a thread by its id -- its scheduler task id, which is what
//! `gettid` answers -- and a process's first thread takes the process's own
//! id (`pcb::claim_leader_id`), as on Linux, so the pid a debugger's `fork`
//! returned names the program's first thread ([`resolve`]). `wait` reports a
//! stop under the stopped thread's id.
//!
//! ## Registers
//!
//! `PTRACE_GETREGS` hands out Linux's `struct user_regs_struct` -- with
//! Linux's segment selectors, `0x33` and `0x2b`, whatever this kernel's are:
//! GDB decides whether a program is 64-bit by `cs == 0x33`.
//!
//! The FPU and vector registers are kept with the stop as the general ones
//! are: the stopping thread captures its FPU state ([`FPU`]), the tracer
//! reads and writes that copy -- as `struct user_fpregs_struct`
//! (`PTRACE_GETFPREGS`, `NT_PRFPREG`) or the XSAVE area (`NT_X86_XSTATE`,
//! `sched::fpu`'s views) -- and the thread loads it back on its way out if
//! it changed. The debug registers -- hardware breakpoints and watchpoints,
//! `struct user`'s `u_debugreg` -- live on the thread's scheduler record and
//! are the CPU's while it runs (`sched::debugreg`).
//!
//! ## Memory
//!
//! `PTRACE_PEEK*`/`POKE*` and `/proc/<pid>/mem` read and write the tracee's
//! address space through its page tables ([`read_memory`], [`write_memory`]).
//! A write may land on a read-only page of a private mapping -- a breakpoint
//! into code -- and the tracee then gets its own copy of that page, as
//! Linux's `FOLL_FORCE` gives it: the file and other processes mapping it are
//! not changed.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::cap::{ResourceType, Rights};
use crate::proc::linux_sigframe::LinuxSiginfo;
use crate::proc::pcb::{self, ProcessId};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use crate::syscall::linux::LinuxTrapRegs;

/// Linux's request numbers (x86-64), which the native call shares.
pub mod request {
    /// The caller consents to be traced by its parent.
    pub const TRACEME: u64 = 0;
    /// Read a word of the tracee's memory.
    pub const PEEKTEXT: u64 = 1;
    /// The same; x86-64 has one address space for code and data.
    pub const PEEKDATA: u64 = 2;
    /// Read a word of `struct user`: a register, or a debug register.
    pub const PEEKUSER: u64 = 3;
    /// Write a word of the tracee's memory.
    pub const POKETEXT: u64 = 4;
    /// The same.
    pub const POKEDATA: u64 = 5;
    /// Write a word of `struct user`.
    pub const POKEUSER: u64 = 6;
    /// Resume the tracee, delivering a signal or none.
    pub const CONT: u64 = 7;
    /// End the tracee.
    pub const KILL: u64 = 8;
    /// Resume the tracee for one instruction.
    pub const SINGLESTEP: u64 = 9;
    /// Read `struct user_regs_struct`.
    pub const GETREGS: u64 = 12;
    /// Write it.
    pub const SETREGS: u64 = 13;
    /// Read `struct user_fpregs_struct`: the x87 and SSE registers.
    pub const GETFPREGS: u64 = 14;
    /// Write it.
    pub const SETFPREGS: u64 = 15;
    /// Attach to a running process.
    pub const ATTACH: u64 = 16;
    /// Let the tracee go.
    pub const DETACH: u64 = 17;
    /// The tracee's `arch_prctl`: read or set its `%fs` or `%gs` base.
    pub const ARCH_PRCTL: u64 = 30;
    /// Set the `PTRACE_O_*` options.
    pub const SETOPTIONS: u64 = 0x4200;
    /// The last event stop's message (an exec's former thread id).
    pub const GETEVENTMSG: u64 = 0x4201;
    /// Read the stop's `siginfo_t`.
    pub const GETSIGINFO: u64 = 0x4202;
    /// Replace it.
    pub const SETSIGINFO: u64 = 0x4203;
    /// Read a register set, named by its core-file note type, into an iovec.
    pub const GETREGSET: u64 = 0x4204;
    /// Write one from an iovec.
    pub const SETREGSET: u64 = 0x4205;
    /// Attach without stopping the process.
    pub const SEIZE: u64 = 0x4206;
}

/// The register sets `PTRACE_GETREGSET`/`SETREGSET` name, by their core-file
/// note types (Linux's x86-64 `user_regset_view`).
pub mod note {
    /// `struct user_regs_struct`.
    pub const PRSTATUS: u64 = 1;
    /// `struct user_fpregs_struct`.
    pub const PRFPREG: u64 = 2;
    /// The I/O permission bitmap, which no thread here has.
    pub const X86_IOPERM: u64 = 0x201;
    /// The XSAVE area.
    pub const X86_XSTATE: u64 = 0x202;
}

/// The `PTRACE_O_*` options.
pub mod option {
    /// Report system-call stops as `SIGTRAP | 0x80`.
    pub const TRACESYSGOOD: u32 = 0x01;
    /// Trace the children `fork` makes.
    pub const TRACEFORK: u32 = 0x02;
    /// Trace the children `vfork` makes.
    pub const TRACEVFORK: u32 = 0x04;
    /// Trace the threads `clone` makes.
    pub const TRACECLONE: u32 = 0x08;
    /// Stop with `PTRACE_EVENT_EXEC` after an exec, not `SIGTRAP`.
    pub const TRACEEXEC: u32 = 0x10;
    /// Stop when a `vfork` child releases its parent.
    pub const TRACEVFORKDONE: u32 = 0x20;
    /// Stop before the tracee exits.
    pub const TRACEEXIT: u32 = 0x40;
    /// Stop at a seccomp filter's `SECCOMP_RET_TRACE`.
    pub const TRACESECCOMP: u32 = 0x80;
    /// End the tracee when its tracer exits.
    pub const EXITKILL: u32 = 0x10_0000;
    /// Suspend the tracee's seccomp filters.
    pub const SUSPEND_SECCOMP: u32 = 0x20_0000;
    /// Every option Linux defines (`PTRACE_O_MASK`, 0x3000ff); any other bit
    /// is `EINVAL`.
    pub const MASK: u32 = TRACESYSGOOD
        | TRACEFORK
        | TRACEVFORK
        | TRACECLONE
        | TRACEEXEC
        | TRACEVFORKDONE
        | TRACEEXIT
        | TRACESECCOMP
        | EXITKILL
        | SUSPEND_SECCOMP;
}

/// `PTRACE_EVENT_*`, the stop's reason in `status >> 16`.
pub mod event {
    /// After a successful exec, with `PTRACE_O_TRACEEXEC`.
    pub const EXEC: u32 = 4;
}

/// Linux's `SIGKILL`, `SIGTRAP`.
const SIGKILL: u32 = 9;
const SIGTRAP: u32 = 5;

/// `struct user_regs_struct`'s length in words, and `struct user`'s offsets
/// that `PTRACE_PEEKUSER`/`POKEUSER` take.
const USER_REGS_WORDS: usize = 27;
/// `offsetof(struct user, u_debugreg)`: DR0-DR7, a word each.
const USER_DEBUGREG_OFFSET: u64 = 848;
/// `sizeof(struct user)`.
const USER_STRUCT_SIZE: u64 = 912;

/// Linux's x86-64 user code and stack selectors, which `GETREGS` reports and
/// `SETREGS` accepts (see the module doc).
const LINUX_USER_CS: u64 = 0x33;
const LINUX_USER_SS: u64 = 0x2b;

/// The trap flag in RFLAGS.
const RFLAGS_TF: u64 = 1 << 8;

/// The RFLAGS bits a tracer may set (Linux's `FLAG_MASK`): CF, PF, AF, ZF,
/// SF, TF, DF, OF, RF and AC.
const RFLAGS_TRACER_MASK: u64 = 0x0005_0DD5;

/// A request refused, as Linux's errno for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtraceError {
    /// `EPERM`: the caller may not trace that process.
    NotPermitted,
    /// `ESRCH`: no such thread, or it is not traced by the caller, or not
    /// stopped for a request that needs it stopped.
    NoSuchThread,
    /// `EIO`: an unknown request, an invalid signal or register value, or
    /// memory that cannot be read or written.
    Io,
    /// `EINVAL`: an unknown option or register set, or a debug register or
    /// FPU state the CPU cannot take.
    Invalid,
    /// `EFAULT`: the caller's own buffer -- or, as on Linux, an XSAVE area
    /// of the wrong size.
    Fault,
    /// `ENODEV`: the XSAVE register set on a CPU without XSAVE.
    NoDevice,
    /// `ENXIO`: the I/O permission bitmap, which no thread has.
    NoDeviceOrAddress,
    /// `EOPNOTSUPP`: a register set that cannot be written.
    NotSupported,
}

impl PtraceError {
    /// The Linux errno.
    #[must_use]
    pub const fn linux_errno(self) -> i32 {
        match self {
            Self::NotPermitted => 1,
            Self::NoSuchThread => 3,
            Self::Io => 5,
            Self::NoDeviceOrAddress => 6,
            Self::Fault => 14,
            Self::NoDevice => 19,
            Self::Invalid => 22,
            Self::NotSupported => 95,
        }
    }
}

/// What a stopped thread does when its tracer lets it go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resume {
    /// Run on, delivering `sig` (0: none); `step`: for one instruction.
    Run { sig: u32, step: bool },
    /// The tracer let go -- `PTRACE_DETACH`, or it exited: run on untraced,
    /// delivering `sig`.
    Detach { sig: u32 },
}

/// A thread in a ptrace-stop.
#[derive(Debug, Clone, Copy)]
struct Stop {
    /// What `wait` reports: the signal, or `SIGTRAP | event << 8`.
    exit_code: u32,
    /// Reported to a `wait` already (and not peeked with `WNOWAIT`).
    reported: bool,
    /// The stop's `siginfo_t`, which the tracer reads and may replace.
    siginfo: LinuxSiginfo,
    /// The thread's user registers.
    regs: LinuxTrapRegs,
    /// The system call the stop is at the exit of, or `u64::MAX` (-1).
    orig_rax: u64,
    /// The tracer changed `regs` or `orig_rax`.
    regs_changed: bool,
    /// Set by the tracer: what the thread does next.
    resume: Option<Resume>,
}

/// A traced thread.
#[derive(Debug, Clone, Copy)]
struct Tracee {
    /// The thread's process.
    pid: ProcessId,
    /// The tracing process.
    tracer: ProcessId,
    /// `PTRACE_O_*`.
    options: u32,
    /// Set while the thread is in a ptrace-stop.
    stop: Option<Stop>,
    /// `PTRACE_GETEVENTMSG`'s answer.
    event_msg: u64,
    /// The trap flag was set by `PTRACE_SINGLESTEP`, not by the program:
    /// hidden from `GETREGS`, and cleared when the thread runs on unstepped
    /// (Linux's `TIF_FORCED_TF`).
    forced_tf: bool,
    /// A signal the tracer put in place of another, queued for the thread
    /// because it could not take it at once: delivered without another stop.
    injected: Option<u32>,
}

/// Every traced thread, by task id. A leaf lock: nothing else is taken while
/// it is held but the scheduler's, by `sched::suspend_pending` and
/// `sched::resume`. Taken with interrupts off: `signal::classify` asks it,
/// and a signal can be sent from an interrupt's context.
static TRACEES: Mutex<BTreeMap<TaskId, Tracee>> = Mutex::new(BTreeMap::new());

/// A stopped thread's FPU state: captured by the thread as it stops, read and
/// written by its tracer, and put back by the thread on its way out if the
/// tracer changed it -- as its general registers are (`Stop::regs`). Kept
/// apart from [`TRACEES`]: it is a heap buffer of up to a few KiB, and that
/// table is asked with interrupts off.
struct FpuStop {
    /// `sched::fpu::capture_signal_image`'s image of the thread's state.
    image: Vec<u8>,
    /// The tracer wrote it.
    changed: bool,
}

/// Every stopped thread's FPU state, by task id. A tracer's access takes it
/// before [`TRACEES`] (to see that the thread is still stopped); nothing
/// takes it inside that lock.
static FPU: Mutex<BTreeMap<TaskId, FpuStop>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Queries the rest of the kernel asks
// ---------------------------------------------------------------------------

/// Whether thread `tid` is traced.
#[must_use]
pub fn is_traced(tid: TaskId) -> bool {
    TRACEES.lock_irqsave().contains_key(&tid)
}

/// Whether any thread of process `pid` is traced: its signals are then all
/// queued for delivery, where the traced thread stops for them, rather than
/// acted on when they are sent (`signal::classify`).
#[must_use]
pub fn process_is_traced(pid: ProcessId) -> bool {
    TRACEES.lock_irqsave().values().any(|t| t.pid == pid)
}

/// The process tracing thread `tid`, if one is (`/proc/<pid>/status`'s
/// `TracerPid`).
#[must_use]
pub fn tracer_of(tid: TaskId) -> Option<ProcessId> {
    TRACEES.lock_irqsave().get(&tid).map(|t| t.tracer)
}

/// Whether thread `tid` is in a ptrace-stop (`/proc`'s state `t`; and a
/// `SIGCONT` must not run it on: it is its tracer's to resume).
#[must_use]
pub fn is_trace_stopped(tid: TaskId) -> bool {
    TRACEES
        .lock_irqsave()
        .get(&tid)
        .is_some_and(|t| t.stop.is_some_and(|s| s.resume.is_none()))
}

/// The thread a request names, and its process: the thread with that id
/// (see the module doc). `None` for an id no live user thread has.
fn resolve(id: u64) -> Option<(ProcessId, TaskId)> {
    crate::proc::thread::owner_process(id)
        .filter(|&p| p != 0)
        .map(|pid| (pid, id))
}

// ---------------------------------------------------------------------------
// Becoming traced
// ---------------------------------------------------------------------------

/// `PTRACE_TRACEME` for thread `tid` of process `pid`: the process consents
/// to be debugged by its parent, which gets `DEBUG` over it and becomes the
/// thread's tracer. `EPERM` if the thread is traced already, or the process
/// has no parent to trace it.
fn traceme(pid: ProcessId, tid: TaskId) -> Result<(), PtraceError> {
    if is_traced(tid) {
        return Err(PtraceError::NotPermitted);
    }
    let parent = pcb::parent(pid)
        .filter(|&p| p != 0 && p != pid)
        .ok_or(PtraceError::NotPermitted)?;
    // The consent: the parent may now debug this process -- the right every
    // later request is checked against. Granted before the record is made,
    // and outside this module's lock (the process table's is taken).
    pcb::insert_caps(parent, &[(ResourceType::Process, pid, Rights::DEBUG)])
        .map_err(|_| PtraceError::NotPermitted)?;
    let mut table = TRACEES.lock_irqsave();
    if table.contains_key(&tid) {
        return Err(PtraceError::NotPermitted);
    }
    table.insert(
        tid,
        Tracee {
            pid,
            tracer: parent,
            options: 0,
            stop: None,
            event_msg: 0,
            forced_tf: false,
            injected: None,
        },
    );
    drop(table);
    serial_println!(
        "[ptrace] task {} of process {} traced by its parent {}",
        tid,
        pid,
        parent
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Stopping, from the traced thread
// ---------------------------------------------------------------------------

/// What a stop came to, for the thread that stopped.
#[derive(Debug, Clone, Copy)]
pub struct StopOutcome {
    /// The signal to deliver now: the one the thread stopped for, another the
    /// tracer chose, or 0 for none.
    pub sig: u32,
    /// The `siginfo_t` to deliver it with.
    pub siginfo: LinuxSiginfo,
    /// The tracer changed the registers; they are back in the caller's
    /// `regs`, and must reach the user frame whatever is delivered.
    pub regs_changed: bool,
    /// The system call the stop was at the exit of, as the tracer left it:
    /// `u64::MAX` (-1) means do not restart it.
    pub orig_rax: u64,
}

/// Stop the current thread `tid` of process `pid` -- traced -- with
/// `exit_code` for `wait` and `siginfo` for `PTRACE_GETSIGINFO`, its user
/// registers `regs`, at the exit of system call `orig_rax` (`u64::MAX` for
/// none); and wait for its tracer to let it go. `None` if the thread is not
/// traced -- nothing happened.
///
/// From the thread's own kernel stack on its way to user mode, holding no
/// lock, interrupts on: it parks. A `SIGKILL` while it is parked ends it
/// there, and this never returns.
///
/// The thread's FPU state is kept with the stop as its registers are: the
/// CPU's FPU registers hold it here (the kernel uses none of them), so it is
/// captured before the stop is published, and put back on the way out if
/// the tracer changed it ([`leave_stop`]).
fn stop_current(
    pid: ProcessId,
    tid: TaskId,
    exit_code: u32,
    siginfo: LinuxSiginfo,
    regs: &mut LinuxTrapRegs,
    orig_rax: u64,
) -> Option<StopOutcome> {
    if !is_traced(tid) {
        return None;
    }
    let image = crate::sched::fpu::capture_signal_image();
    FPU.lock().insert(
        tid,
        FpuStop {
            image,
            changed: false,
        },
    );
    // Publish the stop.
    let published = {
        let mut table = TRACEES.lock_irqsave();
        table.get_mut(&tid).map(|t| {
            t.stop = Some(Stop {
                exit_code,
                reported: false,
                siginfo,
                regs: *regs,
                orig_rax,
                regs_changed: false,
                resume: None,
            });
            t.tracer
        })
    };
    let Some(tracer) = published else {
        // Let go between the two looks (its tracer exited): no stop.
        FPU.lock().remove(&tid);
        return None;
    };
    // Tell the tracer: a `wait` it is blocked in -- for this process, or for
    // any of its children -- looks again.
    let (child_waiter, any_waiter) = pcb::take_trace_waiters(pid, tracer);
    for waiter in [child_waiter, any_waiter].into_iter().flatten() {
        sched::wake(waiter);
    }
    serial_println!(
        "[ptrace] task {} of process {} stopped ({:#x}) for its tracer {}",
        tid,
        pid,
        exit_code,
        tracer
    );
    // Park until the tracer sets what happens next. The check and the mark
    // are made under the lock the tracer's resume takes, so a resume either
    // came first (and is seen here) or comes after the mark (and unparks).
    let outcome = loop {
        {
            let mut table = TRACEES.lock_irqsave();
            let Some(t) = table.get_mut(&tid) else {
                // The record went away under us -- only the thread's own exit
                // removes it, so this cannot happen; run on, delivering what
                // the thread stopped for.
                break StopOutcome {
                    sig: if exit_code >> 8 == 0 { exit_code } else { 0 },
                    siginfo,
                    regs_changed: false,
                    orig_rax,
                };
            };
            if let Some(stop) = t.stop
                && let Some(resume) = stop.resume
            {
                t.stop = None;
                if stop.regs_changed {
                    *regs = stop.regs;
                }
                let sig = match resume {
                    Resume::Run { sig, .. } | Resume::Detach { sig } => sig,
                };
                if matches!(resume, Resume::Detach { .. }) {
                    table.remove(&tid);
                }
                break StopOutcome {
                    sig,
                    siginfo: stop.siginfo,
                    regs_changed: stop.regs_changed,
                    orig_rax: stop.orig_rax,
                };
            }
            sched::suspend_pending(tid);
        }
        sched::park_if_suspended();
    };
    leave_stop(tid);
    Some(outcome)
}

/// The end of thread `tid`'s stop, on the thread: its FPU state loaded back
/// if the tracer changed it, and the `%fs`/`%gs` bases and debug registers --
/// which the tracer changes on the thread's scheduler record, for a switch in
/// to load -- given to the CPU now, since a thread resumed before it ever
/// parked was never switched out and back in.
fn leave_stop(tid: TaskId) {
    let fpu = FPU.lock().remove(&tid);
    if let Some(FpuStop {
        image,
        changed: true,
    }) = fpu
    {
        crate::sched::fpu::restore_signal_image(Some(&image));
    }
    sched::reload_current_user_state();
}

// ---------------------------------------------------------------------------
// Letting go, from the tracer
// ---------------------------------------------------------------------------

/// Set what stopped thread `tid` does next, and unpark it. The trap flag is
/// set for a step -- recorded as the tracer's, unless the program had set it
/// itself -- and a flag the tracer set is cleared for anything else (Linux's
/// `user_enable_single_step`/`user_disable_single_step`). `ESRCH` if the
/// thread is not in a stop that awaits its tracer.
fn resume(tid: TaskId, how: Resume) -> Result<(), PtraceError> {
    let mut table = TRACEES.lock_irqsave();
    let t = table.get_mut(&tid).ok_or(PtraceError::NoSuchThread)?;
    let mut forced_tf = t.forced_tf;
    let stop = t
        .stop
        .as_mut()
        .filter(|s| s.resume.is_none())
        .ok_or(PtraceError::NoSuchThread)?;
    let step = matches!(how, Resume::Run { step: true, .. });
    let tf = stop.regs.rflags & RFLAGS_TF != 0;
    if step && !tf {
        stop.regs.rflags |= RFLAGS_TF;
        stop.regs_changed = true;
        forced_tf = true;
    } else if !step && forced_tf {
        stop.regs.rflags &= !RFLAGS_TF;
        stop.regs_changed = true;
        forced_tf = false;
    }
    stop.resume = Some(how);
    t.forced_tf = forced_tf;
    // Under the lock, so the thread either finds the resume when it looks or
    // is already marked suspended and is unparked here (see `stop_current`).
    sched::resume(tid);
    Ok(())
}

// ---------------------------------------------------------------------------
// Registers
// ---------------------------------------------------------------------------

/// `rax` as a tracer sees it: an interrupted call's restart sentinel as the
/// raw `-ERESTART*` Linux leaves in `rax`, not this kernel's biased form
/// (`syscall::linux::restart`).
fn rax_out(rax: u64) -> u64 {
    // The register's bits, as a syscall's return value.
    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    match crate::syscall::linux::restart::sentinel_magnitude(rax as i64) {
        Some(n) => n.wrapping_neg() as u64,
        None => rax,
    }
}

/// [`rax_out`], for the delivery path: a call its tracer said not to restart
/// (`orig_rax` -1) returns Linux's raw `-ERESTART*`, never this kernel's
/// sentinel.
#[must_use]
pub fn raw_rax(rax: u64) -> u64 {
    rax_out(rax)
}

/// `rax` as the tracer set it, at a stop at the exit of a call (`at_call`):
/// a raw `-ERESTART*` it left there is this kernel's sentinel again, so the
/// call restarts as it would have.
fn rax_in(value: u64, at_call: bool) -> u64 {
    use crate::syscall::linux::restart;
    #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    let n = (value as i64).wrapping_neg();
    let is_restart = matches!(
        n,
        restart::ERESTARTSYS
            | restart::ERESTARTNOINTR
            | restart::ERESTARTNOHAND
            | restart::ERESTART_RESTARTBLOCK
    );
    #[allow(clippy::cast_sign_loss)]
    if at_call && is_restart {
        restart::encode(n) as u64
    } else {
        value
    }
}

/// A stop's `struct user_regs_struct`. A trap flag the tracer set is hidden
/// (Linux's `get_flags`).
fn user_regs(stop: &Stop, forced_tf: bool, fs_base: u64, gs_base: u64) -> [u64; USER_REGS_WORDS] {
    let r = &stop.regs;
    let rflags = if forced_tf {
        r.rflags & !RFLAGS_TF
    } else {
        r.rflags
    };
    [
        r.r15,
        r.r14,
        r.r13,
        r.r12,
        r.rbp,
        r.rbx,
        r.r11,
        r.r10,
        r.r9,
        r.r8,
        rax_out(r.rax),
        r.rcx,
        r.rdx,
        r.rsi,
        r.rdi,
        stop.orig_rax,
        r.rip,
        LINUX_USER_CS,
        rflags,
        r.rsp,
        LINUX_USER_SS,
        fs_base,
        gs_base,
        0,
        0,
        0,
        0,
    ]
}

/// Set word `index` of a stop's `struct user_regs_struct` to `value`, as
/// Linux's `putreg` would: `EIO` for a value no user context may hold.
/// `tls` collects a changed `fs_base`/`gs_base` (`(fs, gs)`), which live on
/// the thread's scheduler record rather than in the stop.
fn set_user_reg(
    stop: &mut Stop,
    forced_tf: &mut bool,
    tls: &mut (Option<u64>, Option<u64>),
    index: usize,
    value: u64,
) -> Result<(), PtraceError> {
    use crate::mm::page_table::USER_SPACE_END;
    let at_call = stop.orig_rax != u64::MAX;
    let r = &mut stop.regs;
    match index {
        0 => r.r15 = value,
        1 => r.r14 = value,
        2 => r.r13 = value,
        3 => r.r12 = value,
        4 => r.rbp = value,
        5 => r.rbx = value,
        6 => r.r11 = value,
        7 => r.r10 = value,
        8 => r.r9 = value,
        9 => r.r8 = value,
        10 => r.rax = rax_in(value, at_call),
        11 => r.rcx = value,
        12 => r.rdx = value,
        13 => r.rsi = value,
        14 => r.rdi = value,
        15 => stop.orig_rax = value,
        // Not a user address: the return to user mode could not load it.
        16 if value < USER_SPACE_END => r.rip = value,
        17 if value == LINUX_USER_CS => {}
        18 => {
            // Linux's `set_flags`: only the arithmetic, trap, direction,
            // resume and alignment-check flags are the tracer's to change --
            // the rest stay as the thread's own (interrupts on, I/O privilege
            // 0), which made them safe. A trap flag in the value is the
            // program's from now on; without one, a flag the tracer set stays
            // set.
            let mut flags = (r.rflags & !RFLAGS_TRACER_MASK) | (value & RFLAGS_TRACER_MASK);
            if flags & RFLAGS_TF != 0 {
                *forced_tf = false;
            } else if *forced_tf {
                flags |= RFLAGS_TF;
            }
            r.rflags = flags;
        }
        19 if value < USER_SPACE_END => r.rsp = value,
        20 if value == LINUX_USER_SS => {}
        21 if value < USER_SPACE_END => tls.0 = Some(value),
        22 if value < USER_SPACE_END => tls.1 = Some(value),
        // ds, es, fs, gs: not used in 64-bit mode; Linux refuses only a
        // selector that is not ring 3's.
        23..=26 if value == 0 || (value <= 0xffff && value & 3 == 3) => {}
        _ => return Err(PtraceError::Io),
    }
    stop.regs_changed = true;
    Ok(())
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// Process `pid`'s address space.
fn address_space(pid: ProcessId) -> Result<u64, PtraceError> {
    pcb::get_pml4(pid)
        .filter(|&p| p != 0)
        .ok_or(PtraceError::Io)
}

/// Read `buf.len()` bytes of process `pid`'s memory at `addr`: `EIO` for an
/// address it does not map readably.
///
/// # Errors
///
/// [`PtraceError::Io`].
pub fn read_memory(pid: ProcessId, addr: u64, buf: &mut [u8]) -> Result<(), PtraceError> {
    let pml4 = address_space(pid)?;
    crate::mm::user::copy_from_user_as(pml4, addr, buf).map_err(|_| PtraceError::Io)
}

/// Write `bytes` into process `pid`'s memory at `addr`, as Linux's
/// `FOLL_FORCE` write does: a page the process may write takes the bytes as
/// its own write would (breaking a copy-on-write share); a read-only page of a
/// private mapping -- code -- is made the process's own copy and takes them,
/// staying read-only to the process (`mm::cow::write_private`). A shared
/// mapping's read-only page, or an address it does not map, is `EIO`.
///
/// # Errors
///
/// [`PtraceError::Io`].
pub fn write_memory(pid: ProcessId, addr: u64, bytes: &[u8]) -> Result<(), PtraceError> {
    const PAGE: u64 = 4096;
    let pml4 = address_space(pid)?;
    let mut done = 0usize;
    while done < bytes.len() {
        let va = u64::try_from(done)
            .ok()
            .and_then(|d| addr.checked_add(d))
            .ok_or(PtraceError::Io)?;
        // This page's share of the write: 1..=PAGE bytes. (4 KiB steps never
        // straddle one of the 16 KiB pages either.)
        let room = usize::try_from(PAGE.saturating_sub(va % PAGE)).unwrap_or(1);
        let end = done.saturating_add(room).min(bytes.len());
        let chunk = bytes.get(done..end).ok_or(PtraceError::Io)?;
        if crate::mm::user::copy_to_user_as(pml4, va, chunk).is_err() {
            // Not writable to the process. Brought in as a read would bring it
            // in, then written as its own copy.
            let mut probe = [0u8; 1];
            crate::mm::user::copy_from_user_as(pml4, va, &mut probe)
                .map_err(|_| PtraceError::Io)?;
            crate::mm::cow::write_private(pml4, va, chunk).map_err(|_| PtraceError::Io)?;
        }
        done = end;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The call
// ---------------------------------------------------------------------------

/// Copy `bytes` to the caller's memory at `addr` (`EFAULT` if it cannot).
fn put_user(addr: u64, bytes: &[u8]) -> Result<(), PtraceError> {
    crate::mm::user::validate_user_write(addr, bytes.len()).map_err(|_| PtraceError::Fault)?;
    // SAFETY: `bytes.len()` bytes at `addr`, validated writable just above.
    unsafe { crate::mm::user::copy_to_user(bytes.as_ptr(), addr, bytes.len()) }
        .map_err(|_| PtraceError::Fault)
}

/// Copy `buf.len()` bytes of the caller's memory at `addr` (`EFAULT`).
fn get_user(addr: u64, buf: &mut [u8]) -> Result<(), PtraceError> {
    crate::mm::user::validate_user_read(addr, buf.len()).map_err(|_| PtraceError::Fault)?;
    // SAFETY: `buf.len()` bytes at `addr`, validated readable just above.
    unsafe { crate::mm::user::copy_from_user(addr, buf.as_mut_ptr(), buf.len()) }
        .map_err(|_| PtraceError::Fault)
}

/// A resuming request's signal: 0 (none) or a valid one; `EIO` otherwise
/// (Linux's `valid_signal`).
fn resume_signal(data: u64) -> Result<u32, PtraceError> {
    u32::try_from(data)
        .ok()
        .filter(|&s| s <= 64)
        .ok_or(PtraceError::Io)
}

/// `ptrace(request, id, addr, data)` from thread `caller_tid` of process
/// `caller` -- both ABIs' call. Returns what the call returns: 0 for every
/// request, the word a `PEEK*` read being stored at `data` as the raw Linux
/// call stores it (a C library's `ptrace()` returns it).
///
/// Linux's order of refusals: `PTRACE_TRACEME` looks at nothing else; an id
/// that names no thread is `ESRCH`; attaching is `EPERM` (see the module
/// doc); a thread the caller does not trace is `ESRCH`, and -- for every
/// request but `PTRACE_KILL` -- one that is not stopped; a tracer that no
/// longer holds `DEBUG` over the process is `EPERM`; an unknown request is
/// `EIO`.
///
/// # Errors
///
/// [`PtraceError`], Linux's errno for each refusal.
pub fn ptrace(
    caller: ProcessId,
    caller_tid: TaskId,
    request: u64,
    id: u64,
    addr: u64,
    data: u64,
) -> Result<u64, PtraceError> {
    if request == request::TRACEME {
        traceme(caller, caller_tid)?;
        return Ok(0);
    }
    // The id is a C `pid_t`: only the low 32 bits arrive.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let id = id as u32 as i32;
    let id = u64::try_from(id)
        .ok()
        .filter(|&i| i > 0)
        .ok_or(PtraceError::NoSuchThread)?;
    let (pid, tid) = resolve(id).ok_or(PtraceError::NoSuchThread)?;
    if request == request::ATTACH || request == request::SEIZE {
        return Err(PtraceError::NotPermitted);
    }
    if tracer_of(tid) != Some(caller) {
        return Err(PtraceError::NoSuchThread);
    }
    if !pcb::has_capability_for(caller, ResourceType::Process, pid, Rights::DEBUG) {
        return Err(PtraceError::NotPermitted);
    }
    if request == request::KILL {
        // Linux's `send_sig_info(SIGKILL, ...)`, stopped or not. A process
        // already on its way out has nothing left to end.
        let _ = crate::syscall::handlers::post_kernel_signal(pid, SIGKILL);
        return Ok(0);
    }
    if !is_trace_stopped(tid) {
        return Err(PtraceError::NoSuchThread);
    }
    match request {
        request::PEEKTEXT | request::PEEKDATA => {
            let mut word = [0u8; 8];
            read_memory(pid, addr, &mut word)?;
            put_user(data, &word)?;
            Ok(0)
        }
        request::POKETEXT | request::POKEDATA => {
            write_memory(pid, addr, &data.to_ne_bytes())?;
            Ok(0)
        }
        request::PEEKUSER => {
            let word = peek_user(tid, addr)?;
            put_user(data, &word.to_ne_bytes())?;
            Ok(0)
        }
        request::POKEUSER => {
            poke_user(tid, addr, data)?;
            Ok(0)
        }
        request::GETREGS => {
            let regs = get_regs(tid)?;
            let mut bytes = [0u8; USER_REGS_WORDS * 8];
            for (chunk, word) in bytes.chunks_exact_mut(8).zip(regs) {
                chunk.copy_from_slice(&word.to_ne_bytes());
            }
            put_user(data, &bytes)?;
            Ok(0)
        }
        request::SETREGS => {
            let mut bytes = [0u8; USER_REGS_WORDS * 8];
            get_user(data, &mut bytes)?;
            let mut words = [0u64; USER_REGS_WORDS];
            for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(8)) {
                let mut w = [0u8; 8];
                w.copy_from_slice(chunk);
                *word = u64::from_ne_bytes(w);
            }
            set_regs(tid, &words)?;
            Ok(0)
        }
        request::GETFPREGS => {
            let fx = with_fpu(tid, crate::sched::fpu::fxsave_view)?;
            put_user(data, &fx)?;
            Ok(0)
        }
        request::SETFPREGS => {
            let mut fx = [0u8; crate::sched::fpu::FXSAVE_SIZE];
            get_user(data, &mut fx)?;
            set_fpu(tid, |image| crate::sched::fpu::set_from_fxsave(image, &fx))?;
            Ok(0)
        }
        request::GETREGSET | request::SETREGSET => regset(tid, request, addr, data),
        request::ARCH_PRCTL => arch_prctl(tid, data, addr),
        request::GETSIGINFO => {
            let siginfo = with_stop(tid, |stop, _| stop.siginfo)?;
            put_user(data, &siginfo.to_bytes())?;
            Ok(0)
        }
        request::SETSIGINFO => {
            let mut bytes = [0u8; 128];
            get_user(data, &mut bytes)?;
            let siginfo = LinuxSiginfo::from_bytes(&bytes);
            with_stop(tid, |stop, _| stop.siginfo = siginfo)?;
            Ok(0)
        }
        request::GETEVENTMSG => {
            let msg = TRACEES
                .lock_irqsave()
                .get(&tid)
                .map(|t| t.event_msg)
                .ok_or(PtraceError::NoSuchThread)?;
            put_user(data, &msg.to_ne_bytes())?;
            Ok(0)
        }
        request::SETOPTIONS => {
            let options = u32::try_from(data)
                .ok()
                .filter(|o| o & !option::MASK == 0)
                .ok_or(PtraceError::Invalid)?;
            let mut table = TRACEES.lock_irqsave();
            let t = table.get_mut(&tid).ok_or(PtraceError::NoSuchThread)?;
            t.options = options;
            Ok(0)
        }
        request::CONT | request::SINGLESTEP => {
            let sig = resume_signal(data)?;
            resume(
                tid,
                Resume::Run {
                    sig,
                    step: request == request::SINGLESTEP,
                },
            )?;
            Ok(0)
        }
        request::DETACH => {
            let sig = resume_signal(data)?;
            resume(tid, Resume::Detach { sig })?;
            Ok(0)
        }
        _ => Err(PtraceError::Io),
    }
}

/// Run `f` on thread `tid`'s stop (and its forced-trap-flag bit), under the
/// lock. `ESRCH` if it is not stopped.
fn with_stop<R>(tid: TaskId, f: impl FnOnce(&mut Stop, &mut bool) -> R) -> Result<R, PtraceError> {
    let mut table = TRACEES.lock_irqsave();
    let t = table.get_mut(&tid).ok_or(PtraceError::NoSuchThread)?;
    let Tracee {
        stop, forced_tf, ..
    } = t;
    let stop = stop
        .as_mut()
        .filter(|s| s.resume.is_none())
        .ok_or(PtraceError::NoSuchThread)?;
    Ok(f(stop, forced_tf))
}

/// Thread `tid`'s `struct user_regs_struct`, at its stop.
fn get_regs(tid: TaskId) -> Result<[u64; USER_REGS_WORDS], PtraceError> {
    // The TLS bases live on the scheduler record, read before the lock.
    let (fs_base, gs_base) = sched::task_tls_bases(tid).ok_or(PtraceError::NoSuchThread)?;
    with_stop(tid, |stop, forced_tf| {
        user_regs(stop, *forced_tf, fs_base, gs_base)
    })
}

/// Set thread `tid`'s registers from the first `words.len()` words of a
/// `struct user_regs_struct` (all 27 for `PTRACE_SETREGS`; `NT_PRSTATUS` may
/// write fewer): every word checked first, so a refused one changes nothing.
fn set_regs(tid: TaskId, words: &[u64]) -> Result<(), PtraceError> {
    let tls = with_stop(tid, |stop, forced_tf| {
        let mut trial = *stop;
        let mut trial_tf = *forced_tf;
        let mut tls = (None, None);
        for (index, &value) in words.iter().enumerate() {
            set_user_reg(&mut trial, &mut trial_tf, &mut tls, index, value)?;
        }
        *stop = trial;
        *forced_tf = trial_tf;
        Ok(tls)
    })??;
    apply_tls(tid, tls);
    Ok(())
}

/// A changed `fs_base`/`gs_base`, onto the thread's scheduler record, which
/// the switch back to it loads -- or, if it never parked, the end of its stop
/// ([`leave_stop`]).
fn apply_tls(tid: TaskId, tls: (Option<u64>, Option<u64>)) {
    if let Some(fs) = tls.0 {
        sched::set_task_fs_base(tid, fs);
    }
    if let Some(gs) = tls.1 {
        sched::set_task_gs_base(tid, gs);
    }
}

/// Run `f` on stopped thread `tid`'s FPU image. `ESRCH` if it is not stopped
/// (or has been let go, and its image is about to be put back).
fn with_fpu<R>(tid: TaskId, f: impl FnOnce(&[u8]) -> R) -> Result<R, PtraceError> {
    let fpu = FPU.lock();
    with_stop(tid, |_, _| ())?;
    let stop = fpu.get(&tid).ok_or(PtraceError::NoSuchThread)?;
    Ok(f(&stop.image))
}

/// Change stopped thread `tid`'s FPU image with `f`, which checks what it is
/// given before it changes anything; the thread loads the image on its way
/// out ([`leave_stop`]).
fn set_fpu(
    tid: TaskId,
    f: impl FnOnce(&mut [u8]) -> Result<(), crate::sched::fpu::FpuImageError>,
) -> Result<(), PtraceError> {
    use crate::sched::fpu::FpuImageError;
    let mut fpu = FPU.lock();
    // Still stopped, under the lock its way out takes first: a change made
    // now is one it will load.
    with_stop(tid, |_, _| ())?;
    let stop = fpu.get_mut(&tid).ok_or(PtraceError::NoSuchThread)?;
    f(&mut stop.image).map_err(|e| match e {
        FpuImageError::Invalid => PtraceError::Invalid,
        FpuImageError::WrongSize => PtraceError::Fault,
        FpuImageError::NoXsave => PtraceError::NoDevice,
    })?;
    stop.changed = true;
    Ok(())
}

/// `PTRACE_GETREGSET`/`SETREGSET` (`request`) of register set `kind` -- a
/// core-file note type ([`note`]) -- for stopped thread `tid`, through the
/// caller's `struct iovec` at `iov` (Linux's `ptrace_regset`). The length must
/// be whole words (`EINVAL`), is cut to the set's size, and is written back
/// as what was done. A read may be short; a write of `NT_PRSTATUS` may set
/// the first registers only, `NT_PRFPREG` must be whole (`EINVAL`) and
/// `NT_X86_XSTATE` must be the whole XSAVE area (`EFAULT`, as on Linux).
fn regset(tid: TaskId, request: u64, kind: u64, iov: u64) -> Result<u64, PtraceError> {
    use crate::sched::fpu;
    /// `IO_BITMAP_BYTES`: the I/O permission bitmap's set.
    const IO_BITMAP_BYTES: usize = 8192;
    let mut raw = [0u8; 16];
    get_user(iov, &mut raw)?;
    let mut word = [0u8; 8];
    word.copy_from_slice(raw.get(8..16).ok_or(PtraceError::Fault)?);
    let asked = u64::from_ne_bytes(word);
    word.copy_from_slice(raw.get(..8).ok_or(PtraceError::Fault)?);
    let base = u64::from_ne_bytes(word);
    let size = match kind {
        note::PRSTATUS => USER_REGS_WORDS * 8,
        note::PRFPREG => fpu::FXSAVE_SIZE,
        note::X86_XSTATE => fpu::user_xstate_size(),
        note::X86_IOPERM => IO_BITMAP_BYTES,
        _ => return Err(PtraceError::Invalid),
    };
    if !asked.is_multiple_of(8) {
        return Err(PtraceError::Invalid);
    }
    let len = usize::try_from(asked).unwrap_or(usize::MAX).min(size);
    if request == request::GETREGSET {
        let set: Vec<u8> = match kind {
            note::PRSTATUS => get_regs(tid)?
                .iter()
                .flat_map(|w| w.to_ne_bytes())
                .collect(),
            note::PRFPREG => with_fpu(tid, fpu::fxsave_view)?.to_vec(),
            note::X86_XSTATE => with_fpu(tid, fpu::xstate_view)?.ok_or(PtraceError::NoDevice)?,
            _ => return Err(PtraceError::NoDeviceOrAddress),
        };
        put_user(base, set.get(..len).ok_or(PtraceError::Fault)?)?;
    } else {
        let mut set = alloc::vec![0u8; len];
        get_user(base, &mut set)?;
        match kind {
            note::PRSTATUS => {
                let words: Vec<u64> = set
                    .chunks_exact(8)
                    .map(|c| {
                        let mut w = [0u8; 8];
                        w.copy_from_slice(c);
                        u64::from_ne_bytes(w)
                    })
                    .collect();
                set_regs(tid, &words)?;
            }
            note::PRFPREG => {
                let fx: [u8; fpu::FXSAVE_SIZE] = set
                    .as_slice()
                    .try_into()
                    .map_err(|_| PtraceError::Invalid)?;
                set_fpu(tid, |image| fpu::set_from_fxsave(image, &fx))?;
            }
            note::X86_XSTATE => set_fpu(tid, |image| fpu::set_from_xstate(image, &set))?,
            _ => return Err(PtraceError::NotSupported),
        }
    }
    let done = u64::try_from(len).map_err(|_| PtraceError::Fault)?;
    put_user(
        iov.checked_add(8).ok_or(PtraceError::Fault)?,
        &done.to_ne_bytes(),
    )?;
    Ok(0)
}

/// `PTRACE_ARCH_PRCTL`: the tracee's `arch_prctl(code, arg)` (Linux's
/// `do_arch_prctl_64` for a tracee) -- its `%fs` or `%gs` base read into the
/// caller's word at `arg`, or set to `arg`: `EPERM` for a base outside user
/// space, `EINVAL` for any other code.
fn arch_prctl(tid: TaskId, code: u64, arg: u64) -> Result<u64, PtraceError> {
    const ARCH_SET_GS: u64 = 0x1001;
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_GET_FS: u64 = 0x1003;
    const ARCH_GET_GS: u64 = 0x1004;
    match code {
        ARCH_GET_FS | ARCH_GET_GS => {
            let (fs, gs) = sched::task_tls_bases(tid).ok_or(PtraceError::NoSuchThread)?;
            let base = if code == ARCH_GET_FS { fs } else { gs };
            put_user(arg, &base.to_ne_bytes())?;
        }
        ARCH_SET_FS | ARCH_SET_GS => {
            if arg >= crate::mm::page_table::USER_SPACE_END {
                return Err(PtraceError::NotPermitted);
            }
            with_stop(tid, |_, _| ())?;
            apply_tls(
                tid,
                if code == ARCH_SET_FS {
                    (Some(arg), None)
                } else {
                    (None, Some(arg))
                },
            );
        }
        _ => return Err(PtraceError::Invalid),
    }
    Ok(0)
}

/// Which debug register the word of `struct user` at `offset` is, if one
/// (`u_debugreg[0..8]`).
fn debugreg_index(offset: u64) -> Option<u64> {
    offset
        .checked_sub(USER_DEBUGREG_OFFSET)
        .map(|d| d / 8)
        .filter(|&n| n < 8)
}

/// `PTRACE_PEEKUSER`: the word of `struct user` at `offset` -- a register, a
/// debug register (`sched::debugreg`), or 0 for the rest, as Linux answers;
/// `EIO` for an offset that is unaligned or past the structure.
fn peek_user(tid: TaskId, offset: u64) -> Result<u64, PtraceError> {
    if !offset.is_multiple_of(8) || offset >= USER_STRUCT_SIZE {
        return Err(PtraceError::Io);
    }
    let index = usize::try_from(offset / 8).map_err(|_| PtraceError::Io)?;
    if index < USER_REGS_WORDS {
        let regs = get_regs(tid)?;
        return regs.get(index).copied().ok_or(PtraceError::Io);
    }
    if let Some(n) = debugreg_index(offset) {
        return sched::task_debug_regs(tid)
            .map(|regs| regs.get(n))
            .ok_or(PtraceError::NoSuchThread);
    }
    Ok(0)
}

/// `PTRACE_POKEUSER`: set the register or debug register at `offset` of
/// `struct user` -- a hardware breakpoint or watchpoint, checked as Linux
/// checks it (`sched::debugreg`: `EINVAL` for one the CPU cannot make, `EIO`
/// for DR4 and DR5). Any other offset is `EIO`, as on Linux.
fn poke_user(tid: TaskId, offset: u64, value: u64) -> Result<(), PtraceError> {
    use crate::sched::debugreg::DebugRegError;
    if !offset.is_multiple_of(8) || offset >= USER_STRUCT_SIZE {
        return Err(PtraceError::Io);
    }
    let index = usize::try_from(offset / 8).map_err(|_| PtraceError::Io)?;
    if index >= USER_REGS_WORDS {
        let n = debugreg_index(offset).ok_or(PtraceError::Io)?;
        return sched::update_task_debug_regs(tid, |regs| regs.set(n, value))
            .ok_or(PtraceError::NoSuchThread)?
            .map_err(|e| match e {
                DebugRegError::Invalid => PtraceError::Invalid,
                DebugRegError::NoSuchRegister => PtraceError::Io,
            });
    }
    let tls = with_stop(tid, |stop, forced_tf| {
        let mut tls = (None, None);
        set_user_reg(stop, forced_tf, &mut tls, index, value).map(|()| tls)
    })??;
    apply_tls(tid, tls);
    Ok(())
}

// ---------------------------------------------------------------------------
// The stops, from the paths that make them
// ---------------------------------------------------------------------------

/// The `siginfo_t` of a signal a tracer put in place of the one the thread
/// stopped for: `SI_USER`, from the tracer (Linux's `ptrace_signal`).
fn siginfo_from_tracer(sig: u32, tracer: ProcessId) -> LinuxSiginfo {
    let uid = pcb::process_uid(tracer).unwrap_or(0);
    let info = crate::proc::signal::SigInfo::user(u32::try_from(tracer).unwrap_or(u32::MAX), uid);
    LinuxSiginfo::from_record(i32::try_from(sig).unwrap_or(0), &info)
}

/// Signal-delivery-stop: the current thread `tid` of process `pid` took
/// signal `sig` (its `siginfo`) for delivery at `regs` -- at the exit of
/// system call `orig_rax`, or `u64::MAX` for none. A traced thread stops
/// first, and its tracer decides what it gets ([`StopOutcome`]): the signal,
/// another one -- with a `siginfo` of the tracer's, as Linux's
/// `ptrace_signal` makes it -- or none. `None` if the thread is not traced,
/// or the signal is `SIGKILL`: deliver it as taken.
///
/// A signal the tracer injected that the thread blocks waits for it, queued
/// again (Linux's `ptrace_signal` too): the outcome is then no signal.
pub fn signal_stop(
    pid: ProcessId,
    tid: TaskId,
    sig: u32,
    siginfo: LinuxSiginfo,
    regs: &mut LinuxTrapRegs,
    orig_rax: u64,
) -> Option<StopOutcome> {
    if sig == SIGKILL {
        return None;
    }
    let tracer = {
        let mut table = TRACEES.lock_irqsave();
        let t = table.get_mut(&tid)?;
        // A signal the tracer itself put in place of another was decided
        // already: delivered without a second stop.
        if t.injected == Some(sig) {
            t.injected = None;
            return None;
        }
        t.tracer
    };
    let mut out = stop_current(pid, tid, sig, siginfo, regs, orig_rax)?;
    if out.sig != 0 && i32::try_from(out.sig).ok() != Some(out.siginfo.si_signo) {
        out.siginfo = siginfo_from_tracer(out.sig, tracer);
    }
    if out.sig != 0 && crate::proc::signal::thread_blocks(pid, tid, out.sig) {
        requeue(pid, tid, out.sig, &out.siginfo);
        out.sig = 0;
    }
    Some(out)
}

/// Queue signal `sig` (with `siginfo`) for thread `tid` again, to be
/// delivered without another stop once the thread can take it.
fn requeue(pid: ProcessId, tid: TaskId, sig: u32, siginfo: &LinuxSiginfo) {
    if let Some(t) = TRACEES.lock_irqsave().get_mut(&tid) {
        t.injected = Some(sig);
    }
    // The record's `SigInfo` carries what the native frame can hold; the
    // Linux frame takes the same fields.
    let info = crate::proc::signal::SigInfo::from_linux(siginfo);
    // A thread that has gone has nothing left to deliver it to.
    let _ = crate::proc::signal::set_thread_pending_info(pid, tid, sig, info);
}

/// A fault's stop: the current thread `tid` of process `pid` took an
/// exception from user mode that raises `sig` (its `siginfo`) at `regs`. A
/// traced thread stops first, from the exception handler, and the outcome
/// says what the fault comes to: no signal -- run on, or run the faulting
/// instruction again; `sig` itself -- the fault goes on as it would untraced;
/// another signal -- queued for the thread, delivered without another stop on
/// its way out. `None` if the thread is not traced.
pub fn fault_stop(
    pid: ProcessId,
    tid: TaskId,
    sig: u32,
    siginfo: LinuxSiginfo,
    regs: &mut LinuxTrapRegs,
) -> Option<StopOutcome> {
    if !is_traced(tid) {
        return None;
    }
    let tracer = tracer_of(tid)?;
    let mut out = stop_current(pid, tid, sig, siginfo, regs, u64::MAX)?;
    if out.sig != 0 && out.sig != sig {
        if i32::try_from(out.sig).ok() != Some(out.siginfo.si_signo) {
            out.siginfo = siginfo_from_tracer(out.sig, tracer);
        }
        requeue(pid, tid, out.sig, &out.siginfo);
        out.sig = 0;
    }
    Some(out)
}

/// After the current thread `tid` of process `pid` exec'd successfully,
/// before the new program's first instruction at `regs`: its exec stop, if it
/// is traced. With `PTRACE_O_TRACEEXEC`, the `PTRACE_EVENT_EXEC` stop, at the
/// exit of the exec call `nr`; otherwise a `SIGTRAP` queued for the thread,
/// which stops it on its way out (Linux's `ptrace_event(PTRACE_EVENT_EXEC)`).
/// Returns whether the tracer changed `regs`.
pub fn after_exec(pid: ProcessId, tid: TaskId, regs: &mut LinuxTrapRegs, nr: u64) -> bool {
    let options = {
        let mut table = TRACEES.lock_irqsave();
        let Some(t) = table.get_mut(&tid) else {
            return false;
        };
        // The thread's id before the exec -- the same, as only the thread
        // that runs the exec survives it here.
        t.event_msg = tid;
        t.options
    };
    let uid = pcb::process_uid(pid).unwrap_or(0);
    let me = u32::try_from(pid).unwrap_or(u32::MAX);
    if options & option::TRACEEXEC != 0 {
        let exit_code = SIGTRAP | event::EXEC << 8;
        // Linux's `ptrace_do_notify`: `si_code` is the stop's whole code.
        let mut siginfo = LinuxSiginfo::from_record(
            i32::try_from(SIGTRAP).unwrap_or(0),
            &crate::proc::signal::SigInfo::user(me, uid),
        );
        siginfo.si_code = i32::try_from(exit_code).unwrap_or(0);
        stop_current(pid, tid, exit_code, siginfo, regs, nr).is_some_and(|out| out.regs_changed)
    } else {
        // Linux's `send_sig(SIGTRAP, current, 0)`: SI_USER, from the thread
        // itself. A thread that has gone has nothing to stop.
        let info = crate::proc::signal::SigInfo::user(me, uid);
        let _ = crate::proc::signal::set_thread_pending_info(pid, tid, SIGTRAP, info);
        false
    }
}

// ---------------------------------------------------------------------------
// wait
// ---------------------------------------------------------------------------

/// A stop of one of `tracer`'s tracees that no `wait` has reported:
/// `(id, exit code)`, the id being the one `wait` returns (see the module
/// doc). `target` is the id waited for -- a process or a thread -- or `None`
/// for any; `pgid` restricts to that process group; `consume` marks the stop
/// reported (not under `WNOWAIT`). The lowest-numbered thread first.
#[must_use]
pub fn take_stop_report(
    tracer: ProcessId,
    target: Option<u64>,
    pgid: Option<ProcessId>,
    consume: bool,
) -> Option<(u64, u32)> {
    // Candidates under this module's lock; ids and groups, which the process
    // table answers, outside it.
    let candidates: Vec<(TaskId, ProcessId, u32)> = TRACEES
        .lock_irqsave()
        .iter()
        .filter(|(_, t)| t.tracer == tracer)
        .filter_map(|(&tid, t)| {
            t.stop
                .filter(|s| s.resume.is_none() && !s.reported)
                .map(|s| (tid, t.pid, s.exit_code))
        })
        .collect();
    for (tid, pid, exit_code) in candidates {
        let id = tid;
        if target.is_some_and(|want| want != id) {
            continue;
        }
        if pgid.is_some_and(|g| pcb::get_pgid(pid) != Some(g)) {
            continue;
        }
        if consume {
            let mut table = TRACEES.lock_irqsave();
            let still = table
                .get_mut(&tid)
                .and_then(|t| t.stop.as_mut())
                .filter(|s| s.resume.is_none() && !s.reported && s.exit_code == exit_code);
            match still {
                Some(stop) => stop.reported = true,
                // Resumed meanwhile -- not a stop any more.
                None => continue,
            }
        }
        return Some((id, exit_code));
    }
    None
}

// ---------------------------------------------------------------------------
// Ends
// ---------------------------------------------------------------------------

/// Thread `tid` is gone: its trace ends -- and the FPU state of a stop it
/// was ended in.
pub fn on_thread_exit(tid: TaskId) {
    TRACEES.lock_irqsave().remove(&tid);
    FPU.lock().remove(&tid);
}

/// Process `pid` is exiting: the threads it traces are let go -- each in a
/// stop runs on, delivering the signal it stopped for (none for an event
/// stop), as Linux's `exit_ptrace` leaves them -- or, those whose tracer set
/// `PTRACE_O_EXITKILL`, ended.
pub fn on_process_exit(pid: ProcessId) {
    let mut kill: Vec<ProcessId> = Vec::new();
    {
        let mut table = TRACEES.lock_irqsave();
        let mine: Vec<TaskId> = table
            .iter()
            .filter(|(_, t)| t.tracer == pid)
            .map(|(&tid, _)| tid)
            .collect();
        for tid in mine {
            let Some(t) = table.get_mut(&tid) else {
                continue;
            };
            if t.options & option::EXITKILL != 0 && !kill.contains(&t.pid) {
                kill.push(t.pid);
            }
            match t.stop.as_mut() {
                Some(stop) if stop.resume.is_none() => {
                    let sig = if stop.exit_code >> 8 == 0 {
                        stop.exit_code
                    } else {
                        0
                    };
                    if t.forced_tf {
                        stop.regs.rflags &= !RFLAGS_TF;
                        stop.regs_changed = true;
                    }
                    stop.resume = Some(Resume::Detach { sig });
                    sched::resume(tid);
                }
                // Running, or already let go: it ends its record itself when
                // it next stops (`stop_current` finds no record and runs on)
                // -- or here, now.
                _ => {
                    table.remove(&tid);
                }
            }
        }
    }
    for victim in kill {
        // Gone already: nothing left to end.
        let _ = crate::syscall::handlers::post_kernel_signal(victim, SIGKILL);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The parts of ptrace that need no second process: the register image and
/// its checks, the restart-value mapping, and the call's refusals from a
/// kernel caller. The stops themselves, and the memory, are driven by the
/// ring-3 test (`spawn::self_test_linux_ptrace`).
///
/// # Errors
///
/// [`crate::error::KernelError::InternalError`] on the first check that
/// fails.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use crate::syscall::linux::restart;

    let fail = |what: &str| {
        serial_println!("[ptrace]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    serial_println!("[ptrace] Running self-test...");

    // A call's restart sentinel reads as Linux's raw -ERESTARTSYS, and goes
    // back in as the sentinel -- at a call's exit only.
    #[allow(clippy::cast_sign_loss)]
    let sentinel = restart::encode(restart::ERESTARTSYS) as u64;
    #[allow(clippy::cast_sign_loss)]
    let raw = (-restart::ERESTARTSYS) as u64;
    if rax_out(sentinel) != raw || rax_in(raw, true) != sentinel || rax_in(raw, false) != raw {
        return fail("the restart sentinel did not read out as -512 and back");
    }
    if rax_out(42) != 42 || rax_in(42, true) != 42 {
        return fail("an ordinary rax was changed");
    }

    // The register image: Linux's order, Linux's selectors, the tracer's
    // trap flag hidden.
    let regs = LinuxTrapRegs {
        rax: 1,
        rbx: 2,
        rcx: 3,
        rdx: 4,
        rsi: 5,
        rdi: 6,
        rbp: 7,
        r8: 8,
        r9: 9,
        r10: 10,
        r11: 11,
        r12: 12,
        r13: 13,
        r14: 14,
        r15: 15,
        rip: 0x40_1000,
        rsp: 0x7fff_0000,
        rflags: 0x202 | RFLAGS_TF,
    };
    let mut stop = Stop {
        exit_code: SIGTRAP,
        reported: false,
        siginfo: LinuxSiginfo::default(),
        regs,
        orig_rax: u64::MAX,
        regs_changed: false,
        resume: None,
    };
    let image = user_regs(&stop, true, 0x1111, 0x2222);
    let want: [u64; USER_REGS_WORDS] = [
        15,
        14,
        13,
        12,
        7,
        2,
        11,
        10,
        9,
        8,
        1,
        3,
        4,
        5,
        6,
        u64::MAX,
        0x40_1000,
        LINUX_USER_CS,
        0x202,
        0x7fff_0000,
        LINUX_USER_SS,
        0x1111,
        0x2222,
        0,
        0,
        0,
        0,
    ];
    if image != want {
        serial_println!("[ptrace]   image {:x?}", image);
        return fail("struct user_regs_struct is not Linux's layout");
    }
    if user_regs(&stop, false, 0, 0).get(18) != Some(&(0x202 | RFLAGS_TF)) {
        return fail("the program's own trap flag was hidden");
    }

    // What a tracer may set: a user address, Linux's selectors, ring-3 data
    // selectors, and flags as a signal return may restore them.
    let mut forced = true;
    let mut tls = (None, None);
    let refused = [
        (16, crate::mm::page_table::USER_SPACE_END),
        (17, 0x23),
        (19, u64::MAX),
        (20, 0x18),
        (21, crate::mm::page_table::USER_SPACE_END),
        (23, 0x10),
        (USER_REGS_WORDS, 0),
    ];
    for (index, value) in refused {
        if set_user_reg(&mut stop, &mut forced, &mut tls, index, value) != Err(PtraceError::Io) {
            serial_println!("[ptrace]   word {} = {:#x} was accepted", index, value);
            return fail("a register value no user context may hold was accepted");
        }
    }
    let accepted = [
        (16, 0x40_2000),
        (17, LINUX_USER_CS),
        (20, LINUX_USER_SS),
        (23, 0x2b),
        (21, 0x5000),
    ];
    for (index, value) in accepted {
        if set_user_reg(&mut stop, &mut forced, &mut tls, index, value).is_err() {
            serial_println!("[ptrace]   word {} = {:#x} was refused", index, value);
            return fail("a register value a user context holds was refused");
        }
    }
    if stop.regs.rip != 0x40_2000 || tls.0 != Some(0x5000) || !stop.regs_changed {
        return fail("accepted register values did not take");
    }
    // IOPL dropped, IF forced on; with the tracer's TF set and none in the
    // value, it stays set.
    if set_user_reg(&mut stop, &mut forced, &mut tls, 18, 0x3000).is_err()
        || stop.regs.rflags != 0x202 | RFLAGS_TF
        || !forced
    {
        return fail("flags were not sanitised, or the tracer's trap flag was lost");
    }
    // A trap flag in the value is the program's from then on.
    if set_user_reg(&mut stop, &mut forced, &mut tls, 18, 0x202 | RFLAGS_TF).is_err() || forced {
        return fail("a trap flag the tracer wrote stayed the tracer's");
    }
    // The resume flag is the tracer's to set (Linux's FLAG_MASK); the ID
    // flag is not. (The trap flag, the program's now, goes with a value
    // without it.)
    if set_user_reg(&mut stop, &mut forced, &mut tls, 18, 0x0021_0202).is_err()
        || stop.regs.rflags != 0x0001_0202
    {
        return fail("RF was not the tracer's to set, or ID was");
    }
    // `struct user`'s debug registers: u_debugreg[0..8], nothing around them.
    if debugreg_index(USER_DEBUGREG_OFFSET) != Some(0)
        || debugreg_index(USER_DEBUGREG_OFFSET + 56) != Some(7)
        || debugreg_index(USER_DEBUGREG_OFFSET + 64).is_some()
        || debugreg_index(USER_DEBUGREG_OFFSET - 8).is_some()
    {
        return fail("the debug registers' offsets in struct user");
    }

    // Signals a resume may carry.
    if resume_signal(0) != Ok(0) || resume_signal(64) != Ok(64) || resume_signal(65).is_ok() {
        return fail("the resume signal range is not 0..=64");
    }

    // The call's refusals, from a kernel caller (no process, no parent).
    let caller = crate::proc::thread::owner_process(sched::current_task_id()).unwrap_or(0);
    let me = sched::current_task_id();
    let checks: [(u64, u64, PtraceError, &str); 4] = [
        (
            request::TRACEME,
            0,
            PtraceError::NotPermitted,
            "TRACEME with no parent",
        ),
        (request::CONT, 0, PtraceError::NoSuchThread, "a pid of 0"),
        (
            request::CONT,
            u64::from(u32::MAX),
            PtraceError::NoSuchThread,
            "a pid of -1",
        ),
        (
            request::CONT,
            0x7fff_fff0,
            PtraceError::NoSuchThread,
            "a pid nothing has",
        ),
    ];
    for (req, id, want_err, what) in checks {
        if ptrace(caller, me, req, id, 0, 0) != Err(want_err) {
            serial_println!("[ptrace]   {}: not {:?}", what, want_err);
            return fail("a refusal was not Linux's");
        }
    }
    // A process with no thread names no thread: ESRCH, for attaching too
    // (Linux finds the task first); and no stop is reported for it.
    let target = pcb::create("ptrace-self-test", 0);
    let r1 = ptrace(caller, me, request::GETREGS, target, 0, 0);
    let r2 = ptrace(caller, me, request::ATTACH, target, 0, 0);
    let r3 = take_stop_report(caller, Some(target), None, false);
    pcb::destroy(target);
    if r1 != Err(PtraceError::NoSuchThread) || r2 != Err(PtraceError::NoSuchThread) {
        return fail("a process with no thread was not ESRCH");
    }
    if r3.is_some() {
        return fail("a stop was reported for a process no one traces");
    }

    serial_println!("[ptrace] Self-test PASSED");
    Ok(())
}
