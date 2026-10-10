//! A thread's first entry to user mode -- a forked child, a cloned or native
//! thread, a new process's first thread -- from the trampoline the scheduler
//! first runs it on: its whole register set loaded, then `IRETQ`.
//!
//! One routine for every trampoline (`fork`, `thread_clone`, `spawn`). Each
//! used to build its own `IRETQ` with the registers it restored and leave the
//! rest as the kernel had them: a forked child and a cloned thread started
//! with RCX holding the kernel-stack address of their register image and R11
//! the kernel's scratch -- a kernel address handed to ring 3 at its first
//! instruction (the fresh-spawn path had closed the same leak for itself, by
//! zeroing every register). Here every general register comes from the
//! [`UserEntry`] image, which the trampoline fills: a child's inherited set
//! (with RCX and R11 as a `SYSRET` from the creating call leaves them: the
//! return address and the flags), or a new image's zeros.
//!
//! Between the two, a traced thread takes its first stop
//! (`crate::proc::ptrace::first_entry`): the `SIGSTOP` Linux queues for a
//! thread or child attached at its creation, before its first instruction.

use crate::syscall::linux::LinuxTrapRegs;

/// The register set a thread enters user mode with. `repr(C)`: [`enter`]
/// reads it by offset, as the comments give them.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UserEntry {
    /// 0.
    pub rip: u64,
    /// 8.
    pub rsp: u64,
    /// 16: sanitized by whoever fills it (a parent's own flags, `0x202`).
    pub rflags: u64,
    /// 24.
    pub rax: u64,
    /// 32.
    pub rbx: u64,
    /// 40: loaded last -- the image's address is in RCX until then.
    pub rcx: u64,
    /// 48.
    pub rdx: u64,
    /// 56.
    pub rsi: u64,
    /// 64.
    pub rdi: u64,
    /// 72.
    pub rbp: u64,
    /// 80.
    pub r8: u64,
    /// 88.
    pub r9: u64,
    /// 96.
    pub r10: u64,
    /// 104.
    pub r11: u64,
    /// 112.
    pub r12: u64,
    /// 120.
    pub r13: u64,
    /// 128.
    pub r14: u64,
    /// 136.
    pub r15: u64,
}

// The offsets `enter` reads.
const _: () = {
    assert!(core::mem::offset_of!(UserEntry, rip) == 0);
    assert!(core::mem::offset_of!(UserEntry, rsp) == 8);
    assert!(core::mem::offset_of!(UserEntry, rflags) == 16);
    assert!(core::mem::offset_of!(UserEntry, rax) == 24);
    assert!(core::mem::offset_of!(UserEntry, rcx) == 40);
    assert!(core::mem::offset_of!(UserEntry, r15) == 136);
};

impl UserEntry {
    /// The image as the register set a tracer reads (`LinuxTrapRegs`).
    #[must_use]
    pub const fn to_trap_regs(self) -> LinuxTrapRegs {
        LinuxTrapRegs {
            rax: self.rax,
            rbx: self.rbx,
            rcx: self.rcx,
            rdx: self.rdx,
            rsi: self.rsi,
            rdi: self.rdi,
            rbp: self.rbp,
            r8: self.r8,
            r9: self.r9,
            r10: self.r10,
            r11: self.r11,
            r12: self.r12,
            r13: self.r13,
            r14: self.r14,
            r15: self.r15,
            rip: self.rip,
            rsp: self.rsp,
            rflags: self.rflags,
        }
    }

    /// The image of register set `regs` (one a tracer changed, which its
    /// checks kept to what a user context may hold).
    #[must_use]
    pub const fn from_trap_regs(regs: &LinuxTrapRegs) -> Self {
        Self {
            rip: regs.rip,
            rsp: regs.rsp,
            rflags: regs.rflags,
            rax: regs.rax,
            rbx: regs.rbx,
            rcx: regs.rcx,
            rdx: regs.rdx,
            rsi: regs.rsi,
            rdi: regs.rdi,
            rbp: regs.rbp,
            r8: regs.r8,
            r9: regs.r9,
            r10: regs.r10,
            r11: regs.r11,
            r12: regs.r12,
            r13: regs.r13,
            r14: regs.r14,
            r15: regs.r15,
        }
    }
}

/// Enter user mode with `entry`: every general register from it, the user
/// code and stack selectors, `IRETQ`. Never returns.
///
/// # Safety
///
/// The current thread is a user thread on its own kernel stack, its address
/// space the active one; `entry.rip` and `entry.rsp` are user addresses and
/// `entry.rflags` a value a user context may hold (IF set, IOPL 0, no
/// reserved bit): `IRETQ` to anything else faults in the kernel or hands ring
/// 3 what it must not have.
pub unsafe fn enter(entry: &UserEntry) -> ! {
    let image = core::ptr::from_ref(entry);
    let user_cs = u64::from(crate::gdt::USER_CS);
    let user_ss = u64::from(crate::gdt::USER_DS);
    // SAFETY: the caller's contract. The frame is pushed in IRETQ's order (SS,
    // RSP, RFLAGS, CS, RIP) before any general register is loaded; RCX holds
    // the image's address until its own load, the last before the IRETQ, and
    // the image is a live object the caller borrows for the whole sequence.
    unsafe {
        core::arch::asm!(
            "push rsi",                 // SS
            "push qword ptr [rcx + 8]", // RSP
            "push qword ptr [rcx + 16]", // RFLAGS
            "push rdx",                 // CS
            "push qword ptr [rcx]",     // RIP
            "mov rax, [rcx + 24]",
            "mov rbx, [rcx + 32]",
            "mov rdx, [rcx + 48]",
            "mov rsi, [rcx + 56]",
            "mov rdi, [rcx + 64]",
            "mov rbp, [rcx + 72]",
            "mov r8, [rcx + 80]",
            "mov r9, [rcx + 88]",
            "mov r10, [rcx + 96]",
            "mov r11, [rcx + 104]",
            "mov r12, [rcx + 112]",
            "mov r13, [rcx + 120]",
            "mov r14, [rcx + 128]",
            "mov r15, [rcx + 136]",
            "mov rcx, [rcx + 40]",
            "iretq",
            in("rcx") image,
            in("rdx") user_cs,
            in("rsi") user_ss,
            options(noreturn),
        );
    }
}
