//! Linux x86_64 signal-frame (`struct rt_sigframe`) construction — **Linux-ABI only**.
//!
//! When the kernel delivers a caught signal to a Linux process, it does
//! **not** simply jump to the handler.  It builds a *signal frame* on the
//! user stack that the handler — and glibc's signal-return trampoline
//! (`sa_restorer` → `rt_sigreturn`) — expect to find, then enters the
//! handler with the System V argument registers pointing into that frame:
//!
//! ```text
//!   %rdi = signo            (int)
//!   %rsi = &rt_sigframe.info  (siginfo_t *)   — SA_SIGINFO handlers read this
//!   %rdx = &rt_sigframe.uc    (ucontext_t *)  — SA_SIGINFO handlers read this
//!   %rsp = &rt_sigframe       (points at pretcode = the return address)
//!   %rip = sa_handler
//! ```
//!
//! The frame, from low address (where `%rsp` points) upward:
//!
//! ```text
//!   [ %rsp ] pretcode    (u64)  = sa_restorer  — handler `ret`s here →
//!                                                 glibc __restore_rt → rt_sigreturn
//!            uc          (struct ucontext, 304 bytes)
//!            info        (struct siginfo, 128 bytes)
//!            [ fpstate ] (optional; we set uc_mcontext.fpstate = 0 / no FP save)
//! ```
//!
//! This mirrors the native [`crate::proc::signal::SignalContext`] trampoline
//! model but in the **exact Linux layout** so an unmodified glibc binary
//! (and, eventually, WINE — which relies heavily on `SA_SIGINFO` +
//! `ucontext` for its exception machinery) sees correct `siginfo`/`ucontext`
//! contents and returns cleanly via its own `sa_restorer`.
//!
//! ## Why this is Linux-ABI only
//!
//! Slate OS *native* processes use SEH-style language-level exceptions, not
//! Unix signals, for process control (design.txt; design-decision #4).  The
//! native POSIX-shim signal path ([`crate::syscall::handlers::deliver_pending_signal`])
//! keeps using the compact native `SignalContext` + per-process trampoline.
//! This module's frame is built **only** for `AbiMode::Linux` processes.
//!
//! ## References
//!
//! - `arch/x86/include/uapi/asm/sigcontext.h` — `struct sigcontext_64`.
//! - `include/uapi/asm-generic/ucontext.h` — `struct ucontext`.
//! - `arch/x86/include/asm/sigframe.h` — `struct rt_sigframe`.
//! - `arch/x86/kernel/signal.c` — `align_sigframe` / `setup_rt_frame`.

/// `struct sigcontext_64` (256 bytes) — the saved machine context the kernel
/// writes into `ucontext.uc_mcontext` and reads back on `rt_sigreturn`.
///
/// Field order is the **exact** x86_64 UAPI order and must not change: glibc,
/// WINE, libunwind, and every signal-aware runtime index these by offset.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxSigcontext {
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rsp: u64,
    pub rip: u64,
    /// RFLAGS (the UAPI field is named `eflags`).
    pub eflags: u64,
    pub cs: u16,
    pub gs: u16,
    pub fs: u16,
    pub ss: u16,
    pub err: u64,
    pub trapno: u64,
    pub oldmask: u64,
    pub cr2: u64,
    /// Pointer to the saved `struct _fpstate`, or 0 when no FP context is
    /// saved (Linux permits a NULL fpstate; `rt_sigreturn` then skips the FP
    /// restore).  We currently always write 0 — see module docs / TD note.
    pub fpstate: u64,
    pub reserved1: [u64; 8],
}

/// `stack_t` (24 bytes) — the alternate-signal-stack descriptor embedded in
/// `ucontext.uc_stack`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxStackT {
    /// `void *ss_sp`.
    pub ss_sp: u64,
    /// `int ss_flags` (followed by 4 bytes of natural-alignment padding).
    pub ss_flags: i32,
    pub _pad: i32,
    /// `size_t ss_size`.
    pub ss_size: u64,
}

/// `struct ucontext` (304 bytes) — the kernel's generic ucontext (the
/// `asm-generic` layout x86_64 uses).  `uc_sigmask` is the kernel's 8-byte
/// `sigset_t`; glibc's wider `sigset_t` simply reads the low 8 bytes plus
/// trailing zero.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxUcontext {
    pub uc_flags: u64,
    /// `struct ucontext *uc_link` (0 = none).
    pub uc_link: u64,
    pub uc_stack: LinuxStackT,
    pub uc_mcontext: LinuxSigcontext,
    /// Kernel `sigset_t` (8 bytes): the signal mask to restore on
    /// `rt_sigreturn` (the mask that was in effect before the handler ran).
    pub uc_sigmask: u64,
}

/// `siginfo_t` (128 bytes) — the signal-information block.
///
/// We expose the common leading fields explicitly and keep the remaining
/// bytes as an opaque tail so the struct is exactly 128 bytes regardless of
/// which `_sifields` union member a given signal populates.  For the two
/// cases we currently produce — a process-directed signal (`kill`/`raise`/
/// `tgkill`: `si_pid`/`si_uid` at offset 16/20) and a fault (`SIGSEGV`/
/// `SIGBUS`/`SIGFPE`/`SIGILL`/`SIGTRAP`: `si_addr` at offset 16) — the
/// relevant union member starts at offset 16, which the builders below fill.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LinuxSiginfo {
    pub si_signo: i32,
    pub si_errno: i32,
    pub si_code: i32,
    /// 4 bytes of padding so the `_sifields` union is 8-byte aligned at
    /// offset 16 on x86_64.
    pub _pad0: i32,
    /// `_sifields` union (offset 16), 112 bytes.  Interpreted per `si_code`.
    pub sifields: [u8; 112],
}

impl Default for LinuxSiginfo {
    fn default() -> Self {
        Self {
            si_signo: 0,
            si_errno: 0,
            si_code: 0,
            _pad0: 0,
            sifields: [0u8; 112],
        }
    }
}

impl LinuxSiginfo {
    /// Build a `siginfo_t` for a **process-directed** signal (`kill(2)`,
    /// `raise(3)` → `tgkill(2)`, `sigqueue`).  Linux fills `si_pid`/`si_uid`
    /// from the *sender*; the union layout is
    /// `struct { pid_t _pid; uid_t _uid; }` at offset 16/20.
    ///
    /// `si_code` is the sender class: `SI_USER` (0) for `kill`, `SI_TKILL`
    /// (-6) for `tgkill`/`raise`, etc.
    #[must_use]
    pub fn kill(signo: i32, si_code: i32, sender_pid: i32, sender_uid: u32) -> Self {
        let mut info = Self {
            si_signo: signo,
            si_errno: 0,
            si_code,
            _pad0: 0,
            sifields: [0u8; 112],
        };
        // _kill: _pid @ union+0 (= struct offset 16), _uid @ union+4 (= 20).
        info.sifields[0..4].copy_from_slice(&sender_pid.to_ne_bytes());
        info.sifields[4..8].copy_from_slice(&sender_uid.to_ne_bytes());
        info
    }

    /// Build a `siginfo_t` for a **queued** signal (`sigqueue(3)` /
    /// `rt_sigqueueinfo(2)`, `si_code == SI_QUEUE`).  The `_rt` union member is
    /// `struct { pid_t _pid; uid_t _uid; sigval_t _sigval; }`, a superset of
    /// the `_kill` layout: `si_pid` at offset 16, `si_uid` at 20, and the
    /// 8-byte `si_value` (union of `int sival_int` / `void *sival_ptr`) at
    /// offset 24 (= union+8).  glibc's `siginfo_t` exposes that word as
    /// `si_value`, which a queued-signal handler reads.
    #[must_use]
    pub fn queue(signo: i32, si_code: i32, sender_pid: i32, sender_uid: u32, value: u64) -> Self {
        let mut info = Self {
            si_signo: signo,
            si_errno: 0,
            si_code,
            _pad0: 0,
            sifields: [0u8; 112],
        };
        // _rt: _pid @ union+0 (struct off 16), _uid @ union+4 (20),
        //      _sigval @ union+8 (24, 8 bytes).
        info.sifields[0..4].copy_from_slice(&sender_pid.to_ne_bytes());
        info.sifields[4..8].copy_from_slice(&sender_uid.to_ne_bytes());
        info.sifields[8..16].copy_from_slice(&value.to_ne_bytes());
        info
    }

    /// Build a `siginfo_t` for a **fault** signal (`SIGSEGV`/`SIGBUS`/
    /// `SIGFPE`/`SIGILL`/`SIGTRAP`).  The `_sigfault` union member is
    /// `struct { void *_addr; ... }`, so `si_addr` sits at offset 16.
    #[must_use]
    pub fn fault(signo: i32, si_code: i32, addr: u64) -> Self {
        let mut info = Self {
            si_signo: signo,
            si_errno: 0,
            si_code,
            _pad0: 0,
            sifields: [0u8; 112],
        };
        info.sifields[0..8].copy_from_slice(&addr.to_ne_bytes());
        info
    }
}

/// Fault-specific `si_code` values for synchronous CPU exceptions delivered
/// as Linux signals (filled into `siginfo._sigfault.si_code`).  These mirror
/// the constants in Linux's `<asm-generic/siginfo.h>`.
pub mod si_fault_code {
    /// `SEGV_MAPERR` — address not mapped to an object (#PF, not-present).
    pub const SEGV_MAPERR: i32 = 1;
    /// `SEGV_ACCERR` — invalid permissions for mapped object (#PF, protection).
    pub const SEGV_ACCERR: i32 = 2;
    /// `FPE_INTDIV` — integer divide by zero (#DE).
    pub const FPE_INTDIV: i32 = 1;
    /// `FPE_INTOVF` — integer overflow (#OF / divide overflow).
    pub const FPE_INTOVF: i32 = 2;
    /// `FPE_FLTINV` — invalid floating-point operation (#MF / #XM).
    pub const FPE_FLTINV: i32 = 7;
    /// `ILL_ILLOPN` — illegal operand (#UD).
    pub const ILL_ILLOPN: i32 = 2;
    /// `BUS_ADRALN` — invalid address alignment (#AC).
    pub const BUS_ADRALN: i32 = 1;
}

/// Size of `struct rt_sigframe` *excluding* any trailing fpstate, i.e. the
/// `pretcode` word + `ucontext` + `siginfo`.
pub const RT_SIGFRAME_SIZE: usize = 8
    + core::mem::size_of::<LinuxUcontext>()
    + core::mem::size_of::<LinuxSiginfo>();

/// Computed placement of an `rt_sigframe` on the user stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    /// Address of `pretcode` — this is the new `%rsp` at handler entry.
    pub frame_addr: u64,
    /// Address of the embedded `ucontext` (= `frame_addr + 8`).  Goes in `%rdx`.
    pub uc_addr: u64,
    /// Address of the embedded `siginfo` (= `uc_addr + sizeof(ucontext)`).
    /// Goes in `%rsi`.
    pub info_addr: u64,
}

/// Compute where on the user stack to place an `rt_sigframe`, given the
/// pre-signal `%rsp`.
///
/// Linux's `align_sigframe` (x86_64) does `sp = round_down(sp - size, 16) - 8`,
/// so the frame begins at an address `≡ 8 (mod 16)`.  Because `pretcode`
/// occupies that first word and acts as the handler's return address, the
/// handler sees `%rsp ≡ 8 (mod 16)` at its first instruction — exactly the
/// state a `call`ed function expects under the System V ABI.
///
/// Returns `None` if the subtraction would underflow the address space
/// (a degenerate `%rsp` near 0), so callers map that to a delivery failure.
#[must_use]
pub fn compute_layout(user_rsp: u64) -> Option<FrameLayout> {
    let size = RT_SIGFRAME_SIZE as u64;
    // round_down(sp - size, 16) - 8
    let lowered = user_rsp.checked_sub(size)?;
    let frame_addr = (lowered & !0xFu64).checked_sub(8)?;
    let uc_addr = frame_addr.checked_add(8)?;
    let info_addr = uc_addr.checked_add(core::mem::size_of::<LinuxUcontext>() as u64)?;
    Some(FrameLayout {
        frame_addr,
        uc_addr,
        info_addr,
    })
}

/// `si_code` constants.
///
/// The complete set of `si_code` values the signal-delivery path can stamp
/// into a `siginfo_t`. All are now produced by the sender-faithful path:
/// `SI_USER` (`kill(2)`), `SI_KERNEL` (timer/kernel-generated), `SI_TKILL`
/// (`tgkill`/`raise`), and `SI_QUEUE` (`sigqueue`/`rt_sigqueueinfo`, which
/// additionally carries an `si_value`). Kept here as the canonical ABI
/// enumeration so the builders above have a single source of truth.
#[allow(dead_code)] // SI_KERNEL: also defined in proc::signal; kept for ABI completeness.
pub mod si_code {
    /// Sent by `kill(2)` (sender is a user process).
    pub const SI_USER: i32 = 0;
    /// Sent by the kernel.
    pub const SI_KERNEL: i32 = 0x80;
    /// Sent by `tgkill(2)` / `raise(3)` (thread-directed).
    pub const SI_TKILL: i32 = -6;
    /// Sent by `sigqueue(3)` / `rt_sigqueueinfo(2)` (carries an `si_value`).
    pub const SI_QUEUE: i32 = -1;
}

/// Boot self-test: assert every Linux signal-frame struct is the exact
/// byte-size and field-offset the x86_64 UAPI mandates.  A drift here would
/// silently corrupt the `siginfo`/`ucontext` an unmodified glibc handler
/// reads, so this is a fatal-on-failure layout gate.
///
/// # Errors
/// Returns [`crate::error::KernelError::InvalidArgument`] on any mismatch.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use crate::serial_println;

    // ---- struct sizes ----
    if core::mem::size_of::<LinuxSigcontext>() != 256 {
        serial_println!(
            "[sigframe]   FAIL: sizeof(sigcontext) = {} (want 256)",
            core::mem::size_of::<LinuxSigcontext>()
        );
        return Err(KernelError::InvalidArgument);
    }
    if core::mem::size_of::<LinuxStackT>() != 24 {
        serial_println!(
            "[sigframe]   FAIL: sizeof(stack_t) = {} (want 24)",
            core::mem::size_of::<LinuxStackT>()
        );
        return Err(KernelError::InvalidArgument);
    }
    if core::mem::size_of::<LinuxUcontext>() != 304 {
        serial_println!(
            "[sigframe]   FAIL: sizeof(ucontext) = {} (want 304)",
            core::mem::size_of::<LinuxUcontext>()
        );
        return Err(KernelError::InvalidArgument);
    }
    if core::mem::size_of::<LinuxSiginfo>() != 128 {
        serial_println!(
            "[sigframe]   FAIL: sizeof(siginfo) = {} (want 128)",
            core::mem::size_of::<LinuxSiginfo>()
        );
        return Err(KernelError::InvalidArgument);
    }

    // ---- key field offsets (via a zeroed instance + pointer math) ----
    let uc = LinuxUcontext::default();
    let base = core::ptr::addr_of!(uc) as usize;
    let off_flags = core::ptr::addr_of!(uc.uc_flags) as usize - base;
    let off_link = core::ptr::addr_of!(uc.uc_link) as usize - base;
    let off_stack = core::ptr::addr_of!(uc.uc_stack) as usize - base;
    let off_mctx = core::ptr::addr_of!(uc.uc_mcontext) as usize - base;
    let off_mask = core::ptr::addr_of!(uc.uc_sigmask) as usize - base;
    if (off_flags, off_link, off_stack, off_mctx, off_mask) != (0, 8, 16, 40, 296) {
        serial_println!(
            "[sigframe]   FAIL: ucontext offsets = ({}, {}, {}, {}, {}) want (0,8,16,40,296)",
            off_flags, off_link, off_stack, off_mctx, off_mask
        );
        return Err(KernelError::InvalidArgument);
    }

    // sigcontext: rip @ 16*8=128, eflags @ 17*8=136, fpstate @ ... .
    let sc = LinuxSigcontext::default();
    let scbase = core::ptr::addr_of!(sc) as usize;
    let off_rip = core::ptr::addr_of!(sc.rip) as usize - scbase;
    let off_eflags = core::ptr::addr_of!(sc.eflags) as usize - scbase;
    let off_cs = core::ptr::addr_of!(sc.cs) as usize - scbase;
    let off_fpstate = core::ptr::addr_of!(sc.fpstate) as usize - scbase;
    if (off_rip, off_eflags, off_cs, off_fpstate) != (128, 136, 144, 184) {
        serial_println!(
            "[sigframe]   FAIL: sigcontext offsets rip/eflags/cs/fpstate = ({}, {}, {}, {}) want (128,136,144,184)",
            off_rip, off_eflags, off_cs, off_fpstate
        );
        return Err(KernelError::InvalidArgument);
    }

    // siginfo: si_signo/errno/code @ 0/4/8, union @ 16.
    let info = LinuxSiginfo::kill(11, si_code::SI_USER, 42, 1000);
    let ibase = core::ptr::addr_of!(info) as usize;
    let off_signo = core::ptr::addr_of!(info.si_signo) as usize - ibase;
    let off_code = core::ptr::addr_of!(info.si_code) as usize - ibase;
    let off_union = core::ptr::addr_of!(info.sifields) as usize - ibase;
    if (off_signo, off_code, off_union) != (0, 8, 16) {
        serial_println!(
            "[sigframe]   FAIL: siginfo offsets signo/code/union = ({}, {}, {}) want (0,8,16)",
            off_signo, off_code, off_union
        );
        return Err(KernelError::InvalidArgument);
    }
    // si_pid / si_uid round-trip.
    let pid = i32::from_ne_bytes(info.sifields[0..4].try_into().unwrap_or([0; 4]));
    let uid = u32::from_ne_bytes(info.sifields[4..8].try_into().unwrap_or([0; 4]));
    if info.si_signo != 11 || info.si_code != si_code::SI_USER || pid != 42 || uid != 1000 {
        serial_println!("[sigframe]   FAIL: siginfo::kill field round-trip");
        return Err(KernelError::InvalidArgument);
    }
    let fault = LinuxSiginfo::fault(11, 1, 0xDEAD_BEEF);
    let faddr = u64::from_ne_bytes(fault.sifields[0..8].try_into().unwrap_or([0; 8]));
    if faddr != 0xDEAD_BEEF {
        serial_println!("[sigframe]   FAIL: siginfo::fault si_addr round-trip");
        return Err(KernelError::InvalidArgument);
    }
    // si_pid / si_uid / si_value (`_rt` layout) round-trip for the queued
    // (`sigqueue`/`rt_sigqueueinfo`) path: pid@16, uid@20, value@24.
    let q = LinuxSiginfo::queue(10, si_code::SI_QUEUE, 7, 1000, 0x1234_5678_9ABC_DEF0);
    let q_pid = i32::from_ne_bytes(q.sifields[0..4].try_into().unwrap_or([0; 4]));
    let q_uid = u32::from_ne_bytes(q.sifields[4..8].try_into().unwrap_or([0; 4]));
    let q_val = u64::from_ne_bytes(q.sifields[8..16].try_into().unwrap_or([0; 8]));
    if q.si_signo != 10
        || q.si_code != si_code::SI_QUEUE
        || q_pid != 7
        || q_uid != 1000
        || q_val != 0x1234_5678_9ABC_DEF0
    {
        serial_println!("[sigframe]   FAIL: siginfo::queue si_pid/si_uid/si_value round-trip");
        return Err(KernelError::InvalidArgument);
    }

    // ---- frame layout / alignment ----
    // For any 16-aligned input rsp, the frame must end up ≡ 8 (mod 16) and
    // strictly below rsp - RT_SIGFRAME_SIZE region.
    let layout = match compute_layout(0x0000_7fff_ffff_0000) {
        Some(l) => l,
        None => {
            serial_println!("[sigframe]   FAIL: compute_layout returned None for a valid rsp");
            return Err(KernelError::InvalidArgument);
        }
    };
    if layout.frame_addr % 16 != 8 {
        serial_println!(
            "[sigframe]   FAIL: frame_addr {:#x} not ≡ 8 (mod 16)",
            layout.frame_addr
        );
        return Err(KernelError::InvalidArgument);
    }
    if layout.uc_addr != layout.frame_addr + 8
        || layout.info_addr != layout.uc_addr + 304
    {
        serial_println!(
            "[sigframe]   FAIL: layout linkage uc={:#x} info={:#x} frame={:#x}",
            layout.uc_addr, layout.info_addr, layout.frame_addr
        );
        return Err(KernelError::InvalidArgument);
    }
    // Underflow guard: a tiny rsp yields None rather than wrapping.
    if compute_layout(8).is_some() {
        serial_println!("[sigframe]   FAIL: compute_layout(8) should underflow to None");
        return Err(KernelError::InvalidArgument);
    }

    serial_println!(
        "[sigframe]   Linux rt_sigframe ABI layout (sigcontext 256 / ucontext 304 / siginfo 128, align): OK"
    );
    Ok(())
}
