//! The debug registers of user threads: the hardware breakpoints and
//! watchpoints a debugger sets through `ptrace`, into `struct user`'s
//! `u_debugreg` (design-decisions 1547).
//!
//! x86 has four breakpoint address registers (DR0-DR3), a control register
//! (DR7: which of them are enabled, and for each whether it fires on the
//! execution of an instruction, on a write, or on a read or write, of 1, 2, 4
//! or 8 bytes), and a status register (DR6: which fired, and whether the
//! trap flag's single step did). They belong to a CPU; a thread's are its
//! own, so they are kept on its scheduler record ([`DebugRegs`],
//! `Task::debug_regs`), loaded when it is switched in ([`switch_in`], from
//! `sched::note_dispatch`) and cleared when a thread without any is: no other
//! thread runs with them.
//!
//! What a debugger writes is checked as Linux checks it (`ptrace_set_debugreg`,
//! `ptrace_write_dr7`, `arch_bp_generic_fields`, `hw_breakpoint_arch_parse`):
//! an address in user space; for an enabled breakpoint, a length the CPU has
//! (1, 2, 4 or 8 bytes), an address aligned to it and a range wholly in user
//! space, not an I/O breakpoint, and an execution breakpoint of length 1 --
//! anything else is `EINVAL`, and DR4/DR5, which do not exist, are `EIO`. DR7
//! reads back as it was written. The CPU is given only what was checked: the
//! enabled breakpoints, as global ones with `GE` (Linux's `__encode_dr7`),
//! never `GD` -- which would make every debug-register access fault -- nor a
//! reserved bit.
//!
//! DR6 is kept as Linux keeps it (`virtual_dr6`): in positive polarity -- its
//! reserved bits, which read as 1, flipped to 0 -- and set by each debug
//! exception from user mode to what that exception was, the single step and
//! the enabled breakpoints that fired ([`classify_user_exception`]).
//! `PTRACE_PEEKUSER` reads it in the hardware's polarity, so a thread with
//! nothing recorded reads `0xffff0ff0`, as on Linux.
//!
//! A data breakpoint on a user address also fires when the kernel reads or
//! writes that address for the thread -- `read(2)` filling a watched buffer.
//! That exception arrives in kernel mode and is dismissed (`idt`'s
//! `handle_debug`): Linux records it where only a `PEEKUSER` of DR6 before the
//! thread's next user-mode debug exception could see it, and sends no signal;
//! here nothing is recorded.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::mm::page_table::USER_SPACE_END;
use crate::proc::linux_sigframe::si_fault_code;

/// DR6's bits, in positive polarity (see the module doc).
pub mod dr6 {
    /// Breakpoints 0-3 fired: bits 0-3.
    pub const TRAP_BITS: u64 = 0xF;
    /// The trap flag's single step (`BS`).
    pub const STEP: u64 = 1 << 14;
    /// The bits that read as 1 when nothing is recorded (Linux's
    /// `DR6_RESERVED`): the flip between the two polarities.
    pub const RESERVED: u64 = 0xFFFF_0FF0;
}

/// Each breakpoint's enable bits in DR7 (local and global).
const ENABLE: [u64; 4] = [0x03, 0x0C, 0x30, 0xC0];
/// Each breakpoint's global enable bit, which the CPU is given.
const GLOBAL: [u64; 4] = [0x02, 0x08, 0x20, 0x80];
/// Where each breakpoint's condition (R/W, two bits) and length (LEN, two
/// bits above it) sit in DR7.
const FIELD_SHIFT: [u32; 4] = [16, 20, 24, 28];
/// DR7's `GE` ("exact" data breakpoints), set with any breakpoint as Linux
/// sets it.
const DR7_GE: u64 = 1 << 9;
/// R/W: break on executing the instruction at the address.
const RW_EXECUTE: u64 = 0;
/// R/W: break on I/O to the port -- not for user threads.
const RW_IO: u64 = 2;

/// A debug register write refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugRegError {
    /// `EINVAL`: a kernel address, or a breakpoint the CPU cannot make.
    Invalid,
    /// `EIO`: DR4 or DR5, which do not exist.
    NoSuchRegister,
}

/// One thread's debug registers (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugRegs {
    /// DR0-DR3, as the debugger wrote them.
    addr: [u64; 4],
    /// DR7, as the debugger wrote it.
    dr7: u64,
    /// DR6, in positive polarity.
    dr6: u64,
    /// The DR7 the CPU is given: `dr7`'s enabled breakpoints, checked. Zero
    /// when there are none, and then the CPU is left with none.
    hw_dr7: u64,
}

impl Default for DebugRegs {
    fn default() -> Self {
        Self::NONE
    }
}

impl DebugRegs {
    /// No breakpoints, nothing recorded: a new thread's, and every thread's
    /// after an exec (Linux's `flush_ptrace_hw_breakpoint`).
    pub const NONE: Self = Self {
        addr: [0; 4],
        dr7: 0,
        dr6: 0,
        hw_dr7: 0,
    };

    /// Debug register `n` as `PTRACE_PEEKUSER` reads it: DR0-DR3 and DR7 as
    /// written, DR6 in the hardware's polarity, DR4 and DR5 zero.
    #[must_use]
    pub fn get(&self, n: u64) -> u64 {
        match n {
            0..=3 => usize::try_from(n)
                .ok()
                .and_then(|i| self.addr.get(i))
                .copied()
                .unwrap_or(0),
            6 => self.dr6 ^ dr6::RESERVED,
            7 => self.dr7,
            _ => 0,
        }
    }

    /// Debug register `n` as `PTRACE_POKEUSER` writes it, checked as the
    /// module doc says; on a refusal nothing changes.
    ///
    /// # Errors
    ///
    /// [`DebugRegError::Invalid`] for an address or breakpoint Linux refuses
    /// with `EINVAL`; [`DebugRegError::NoSuchRegister`] for DR4 and DR5.
    pub fn set(&mut self, n: u64, value: u64) -> Result<(), DebugRegError> {
        match n {
            0..=3 => {
                // Linux makes the slot a disabled one-byte breakpoint at the
                // address, which must be in user space; an enabled one must
                // still check out there.
                if value >= USER_SPACE_END {
                    return Err(DebugRegError::Invalid);
                }
                let mut addr = self.addr;
                let slot = usize::try_from(n)
                    .ok()
                    .and_then(|i| addr.get_mut(i))
                    .ok_or(DebugRegError::NoSuchRegister)?;
                *slot = value;
                let hw = hw_dr7(&addr, self.dr7)?;
                self.addr = addr;
                self.hw_dr7 = hw;
                Ok(())
            }
            6 => {
                self.dr6 = value ^ dr6::RESERVED;
                Ok(())
            }
            7 => {
                let hw = hw_dr7(&self.addr, value)?;
                self.dr7 = value;
                self.hw_dr7 = hw;
                Ok(())
            }
            _ => Err(DebugRegError::NoSuchRegister),
        }
    }

    /// Whether the CPU must be given these: any breakpoint enabled.
    #[must_use]
    pub const fn armed(&self) -> bool {
        self.hw_dr7 != 0
    }

    /// Record DR6 (positive polarity) for a debug exception the thread took
    /// from user mode ([`UserDebugException::dr6`]).
    pub fn record_dr6(&mut self, dr6: u64) {
        self.dr6 = dr6;
    }
}

/// The DR7 the CPU is given for breakpoint addresses `addr` and a debugger's
/// `dr7`: each enabled breakpoint checked, and made a global one.
fn hw_dr7(addr: &[u64; 4], dr7: u64) -> Result<u64, DebugRegError> {
    let mut hw = 0u64;
    for (((&enable, &global), &shift), &address) in ENABLE
        .iter()
        .zip(GLOBAL.iter())
        .zip(FIELD_SHIFT.iter())
        .zip(addr.iter())
    {
        if dr7 & enable == 0 {
            // A disabled breakpoint is not checked. (Linux also checks a
            // disabled one's fields once its address has been written; no
            // debugger writes fields it does not enable.)
            continue;
        }
        let field = dr7.wrapping_shr(shift) & 0xF;
        let rw = field & 0b11;
        let len_code = field.wrapping_shr(2);
        let len: u64 = match (rw, len_code) {
            (_, 0) => 1,
            // An execution breakpoint is one byte: LEN must be 0.
            (RW_EXECUTE, _) => return Err(DebugRegError::Invalid),
            (_, 1) => 2,
            (_, 3) => 4,
            // (_, 2): eight bytes, which 64-bit mode has.
            _ => 8,
        };
        if rw == RW_IO {
            return Err(DebugRegError::Invalid);
        }
        // Aligned to its length, and wholly in user space.
        if address & len.wrapping_sub(1) != 0 {
            return Err(DebugRegError::Invalid);
        }
        match address.checked_add(len) {
            Some(end) if end <= USER_SPACE_END => {}
            _ => return Err(DebugRegError::Invalid),
        }
        hw |= global | field.wrapping_shl(shift);
    }
    if hw != 0 {
        hw |= DR7_GE;
    }
    Ok(hw)
}

/// What a debug exception a thread took from user mode was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserDebugException {
    /// The thread's DR6 now (positive polarity): the single step, and the
    /// enabled breakpoints that fired -- what `PTRACE_PEEKUSER` reads.
    pub dr6: u64,
    /// `SIGTRAP`'s `si_code`: `TRAP_TRACE` for a step, `TRAP_HWBKPT` for a
    /// breakpoint, `TRAP_BRKPT` for `int1`; `None` for nothing to report.
    pub si_code: Option<i32>,
    /// An execution breakpoint fired: the thread must return with the resume
    /// flag set, or going back to the instruction fires it again.
    pub resume_flag: bool,
}

/// Classify a debug exception from user mode: DR6 as the CPU left it, in
/// positive polarity, with `hw_dr7` the breakpoints the CPU had
/// ([`loaded_dr7`]) -- Linux's `exc_debug_user` and `hw_breakpoint_handler`.
///
/// A breakpoint that is not enabled is not reported (the CPU may set its bit
/// anyway). A single step and an execution breakpoint together report the
/// step; the breakpoint, a fault taken before its instruction runs, fires
/// again when the thread goes back to it (Linux: "take TF as that has
/// precedence"). A DR6 with no cause at all is `int1`.
#[must_use]
pub fn classify_user_exception(dr6_positive: u64, hw_dr7: u64) -> UserDebugException {
    let step = dr6_positive & dr6::STEP != 0;
    let mut dr6 = dr6_positive & dr6::STEP;
    let mut resume_flag = false;
    for (i, (&enable, &shift)) in ENABLE.iter().zip(FIELD_SHIFT.iter()).enumerate() {
        let bit = 1u64.wrapping_shl(u32::try_from(i).unwrap_or(0));
        if dr6_positive & bit == 0 || hw_dr7 & enable == 0 {
            continue;
        }
        let execute = hw_dr7.wrapping_shr(shift) & 0b11 == RW_EXECUTE;
        if execute && step {
            continue;
        }
        dr6 |= bit;
        resume_flag |= execute;
    }
    let si_code = if step {
        Some(si_fault_code::TRAP_TRACE)
    } else if dr6 & dr6::TRAP_BITS != 0 {
        Some(si_fault_code::TRAP_HWBKPT)
    } else if dr6_positive == 0 {
        Some(si_fault_code::TRAP_BRKPT)
    } else {
        None
    };
    UserDebugException {
        dr6,
        si_code,
        resume_flag,
    }
}

// ---------------------------------------------------------------------------
// The CPU's registers
// ---------------------------------------------------------------------------

/// The DR7 each CPU was last given ([`switch_in`]): 0 when it has no
/// breakpoints. Read by the debug exception handler of the same CPU.
static LOADED_DR7: [AtomicU64; crate::smp::MAX_CPUS] = {
    const NONE: AtomicU64 = AtomicU64::new(0);
    [NONE; crate::smp::MAX_CPUS]
};

/// The breakpoints CPU `cpu` has: the DR7 it was given (0: none).
#[must_use]
pub fn loaded_dr7(cpu: usize) -> u64 {
    LOADED_DR7.get(cpu).map_or(0, |d| d.load(Ordering::Relaxed))
}

/// Give CPU `cpu` -- the one this runs on, with interrupts off -- the debug
/// registers of the thread it is switching to: its breakpoints, or none. A
/// CPU that has none and is given none is not touched (a debug register write
/// serializes the CPU).
pub fn switch_in(cpu: usize, regs: &DebugRegs) {
    let Some(loaded) = LOADED_DR7.get(cpu) else {
        return;
    };
    if !regs.armed() {
        if loaded.load(Ordering::Relaxed) != 0 {
            // SAFETY: CPL 0 may write DR7, and zero disables every breakpoint.
            unsafe { write_dr7(0) };
            loaded.store(0, Ordering::Relaxed);
        }
        return;
    }
    // Disabled first, so no breakpoint is ever armed at another thread's
    // address; the addresses; then the control register.
    // SAFETY: CPL 0 may write the debug registers. The addresses are user
    // addresses (`DebugRegs::set` refuses any other), and `hw_dr7` has only
    // enable, condition, length and GE bits -- no GD, no reserved bit, nothing
    // above bit 31 -- so no write can fault.
    unsafe {
        write_dr7(0);
        let [a0, a1, a2, a3] = regs.addr;
        core::arch::asm!("mov dr0, {}", in(reg) a0, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov dr1, {}", in(reg) a1, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov dr2, {}", in(reg) a2, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov dr3, {}", in(reg) a3, options(nomem, nostack, preserves_flags));
        write_dr7(regs.hw_dr7);
    }
    loaded.store(regs.hw_dr7, Ordering::Relaxed);
}

/// Write DR7.
///
/// # Safety
///
/// CPL 0, and `value` a valid DR7: bits 32-63, GD and the reserved bits clear.
unsafe fn write_dr7(value: u64) {
    // SAFETY: the caller's contract.
    unsafe {
        core::arch::asm!("mov dr7, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

/// Read DR6 and clear it, as the CPU never does: in positive polarity
/// (Linux's `debug_read_clear_dr6`). For the debug exception handler.
#[must_use]
pub fn read_clear_dr6() -> u64 {
    let raw: u64;
    // SAFETY: CPL 0 may read and write DR6; writing the reserved pattern
    // records nothing.
    unsafe {
        core::arch::asm!("mov {}, dr6", out(reg) raw, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov dr6, {}", in(reg) dr6::RESERVED, options(nomem, nostack, preserves_flags));
    }
    raw ^ dr6::RESERVED
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot self-test of the checks and the classification (not the hardware:
/// the ring-3 ptrace test sets real breakpoints).
pub fn self_test() -> Result<(), &'static str> {
    let mut regs = DebugRegs::NONE;
    if regs.get(6) != 0xFFFF_0FF0 || regs.get(7) != 0 || regs.armed() {
        return Err("a new thread's registers are not empty");
    }
    // Addresses: user space only.
    if regs.set(0, 0x40_1000) != Ok(()) || regs.get(0) != 0x40_1000 {
        return Err("DR0 refused a user address");
    }
    if regs.set(1, USER_SPACE_END) != Err(DebugRegError::Invalid) {
        return Err("DR1 took a kernel address");
    }
    if regs.set(4, 0) != Err(DebugRegError::NoSuchRegister)
        || regs.set(5, 0) != Err(DebugRegError::NoSuchRegister)
    {
        return Err("DR4/DR5 were writable");
    }
    // DR0 as a 4-byte write watchpoint: L0, R/W 01, LEN 11.
    let watch4 = 0x1 | (0b1101 << 16);
    if regs.set(7, watch4) != Ok(()) || !regs.armed() || regs.get(7) != watch4 {
        return Err("a 4-byte watchpoint was refused");
    }
    if regs.hw_dr7 != 0x2 | (0b1101 << 16) | DR7_GE {
        return Err("the CPU's DR7 is not the global form");
    }
    // Moving an enabled watchpoint to an unaligned address is refused, and
    // leaves it where it was.
    if regs.set(0, 0x40_1002) != Err(DebugRegError::Invalid) || regs.get(0) != 0x40_1000 {
        return Err("an unaligned enabled watchpoint was taken");
    }
    // An execution breakpoint must be one byte; an I/O one is refused.
    if regs.set(7, 0x1 | (0b0100 << 16)) != Err(DebugRegError::Invalid) {
        return Err("a 2-byte execution breakpoint was taken");
    }
    if regs.set(7, 0x1 | (0b0010 << 16)) != Err(DebugRegError::Invalid) {
        return Err("an I/O breakpoint was taken");
    }
    // An 8-byte one must be 8-aligned and end in user space.
    if regs.set(7, 0x1 | (0b1001 << 16)) != Err(DebugRegError::Invalid) {
        return Err("an 8-byte watchpoint at a 4-aligned address was taken");
    }
    // Disabled: anything goes, and the CPU gets nothing.
    if regs.set(7, 0x400) != Ok(()) || regs.armed() || regs.get(7) != 0x400 {
        return Err("DR7 with nothing enabled was mishandled");
    }
    // DR6 round-trips in the hardware's polarity.
    if regs.set(6, 0) != Ok(()) || regs.get(6) != 0 {
        return Err("DR6 did not read back");
    }
    // Classification: a step; a step with an execution breakpoint (the step
    // wins); a watchpoint; int1; a breakpoint that is not enabled.
    let exec0 = 0x2 | DR7_GE;
    let watch0 = 0x2 | (0b1101 << 16) | DR7_GE;
    let step = classify_user_exception(dr6::STEP, 0);
    if step.si_code != Some(si_fault_code::TRAP_TRACE) || step.dr6 != dr6::STEP {
        return Err("a single step was misread");
    }
    let both = classify_user_exception(dr6::STEP | 1, exec0);
    if both.dr6 != dr6::STEP || both.resume_flag {
        return Err("a step and an execution breakpoint did not report the step alone");
    }
    let exec = classify_user_exception(1, exec0);
    if exec.si_code != Some(si_fault_code::TRAP_HWBKPT) || !exec.resume_flag || exec.dr6 != 1 {
        return Err("an execution breakpoint was misread");
    }
    let watch = classify_user_exception(1, watch0);
    if watch.si_code != Some(si_fault_code::TRAP_HWBKPT) || watch.resume_flag {
        return Err("a watchpoint was misread");
    }
    if classify_user_exception(0, 0).si_code != Some(si_fault_code::TRAP_BRKPT) {
        return Err("int1 was misread");
    }
    if classify_user_exception(0b10, watch0).si_code.is_some() {
        return Err("a breakpoint that is not enabled was reported");
    }
    Ok(())
}
