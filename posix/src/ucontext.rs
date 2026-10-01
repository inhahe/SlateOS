//! `<ucontext.h>`: `getcontext`, `setcontext`, `makecontext` and
//! `swapcontext` -- user-level context switching, which C has had since
//! SUSv2 and which coroutine libraries, green-thread runtimes and some
//! interpreters are built on. musl's headers declare the four and musl
//! itself defines none of them (the `libucontext` package fills the gap
//! there); glibc has all four, and so does this library.
//!
//! A context is the state a function call can observe and must preserve:
//! the callee-saved registers (`rbx`, `rbp`, `r12`-`r15`), the stack
//! pointer, where to resume, the x87 and SSE control state, and the signal
//! mask. `getcontext` records it; `setcontext` resumes it, so that the
//! `getcontext` that recorded it returns a second time, with 0;
//! `makecontext` points a recorded context at a function on a stack of its
//! own, which on returning resumes `uc_link` -- or ends the process with
//! `exit(0)` when there is none, as glibc's does; and `swapcontext` records
//! one context and resumes another in one step.
//!
//! # Layout
//!
//! [`UcontextT`] is musl's x86-64 `ucontext_t`, 936 bytes (`abi_layout.rs`
//! checks it against the headers C programs compile with). The general
//! registers go where the kernel's signal frame puts them,
//! `uc_mcontext.gregs[REG_*]`, so that a context from `getcontext` and one a
//! signal handler receives read alike. Like glibc's, `getcontext` also
//! records the six argument registers, and keeps the floating-point state in
//! the context's own `__fpregs_mem`, with `uc_mcontext.fpregs` pointing
//! there: the x87 environment as `fnstenv` stores it, and the SSE control
//! and status register at `_fpstate`'s `mxcsr` offset. Only the control and
//! status state travels -- the x87 register stack is empty at every call,
//! and the SSE registers are all caller-saved.
//!
//! # The assembly
//!
//! The four C entry points are assembly, since a context switch replaces
//! the stack pointer, which Rust code cannot do. The signal mask is Rust
//! (`__slate_uc_*mask`), which the assembly calls with the System V
//! convention on every target -- `extern "sysv64"` -- so that the host
//! tests run the same code: on Windows, where they run, `extern "C"` would
//! be Microsoft's convention. The host copies of the entry points carry
//! other names (`slate_uc_*`) and are only for the tests.
//!
//! # What the tests can see
//!
//! The host tests switch only with `swapcontext`, which to its caller is an
//! ordinary call that returns once -- Rust has no way to declare a function
//! that returns twice, as `getcontext` does, so a test that resumed a
//! `getcontext` inside Rust code would be on unsound ground. The
//! returning-twice half is `services/ctest-ucontext`'s, in C, where the
//! compiler knows `getcontext` by name.

use core::mem::offset_of;

use crate::signal::{SigsetT, StackT};

/// `mcontext_t`'s register indices (`REG_*` in musl's `bits/signal.h`).
pub const REG_R8: usize = 0;
pub const REG_R9: usize = 1;
pub const REG_R10: usize = 2;
pub const REG_R11: usize = 3;
pub const REG_R12: usize = 4;
pub const REG_R13: usize = 5;
pub const REG_R14: usize = 6;
pub const REG_R15: usize = 7;
pub const REG_RDI: usize = 8;
pub const REG_RSI: usize = 9;
pub const REG_RBP: usize = 10;
pub const REG_RBX: usize = 11;
pub const REG_RDX: usize = 12;
pub const REG_RAX: usize = 13;
pub const REG_RCX: usize = 14;
pub const REG_RSP: usize = 15;
pub const REG_RIP: usize = 16;
pub const REG_EFL: usize = 17;
pub const REG_CSGSFS: usize = 18;
pub const REG_ERR: usize = 19;
pub const REG_TRAPNO: usize = 20;
pub const REG_OLDMASK: usize = 21;
pub const REG_CR2: usize = 22;
/// The number of general registers `gregset_t` holds.
pub const NGREG: usize = 23;

/// `struct _fpstate`: the floating-point state in `fxsave`'s layout, which
/// the kernel's signal frame uses. A context from `getcontext` fills only
/// its control and status part (see the module docs).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Fpstate {
    pub cwd: u16,
    pub swd: u16,
    pub ftw: u16,
    pub fop: u16,
    pub rip: u64,
    pub rdp: u64,
    pub mxcsr: u32,
    pub mxcr_mask: u32,
    /// The eight x87 registers, each `significand[4], exponent, padding[3]`.
    pub st: [[u16; 8]; 8],
    /// The sixteen SSE registers.
    pub xmm: [[u32; 4]; 16],
    pub padding: [u32; 24],
}

/// `mcontext_t`: the general registers, and where the floating-point state
/// is.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct McontextT {
    pub gregs: [i64; NGREG],
    pub fpregs: *mut Fpstate,
    pub reserved1: [u64; 8],
}

/// `ucontext_t`, musl's x86-64 layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UcontextT {
    pub uc_flags: u64,
    /// The context resumed when a `makecontext` function returns.
    pub uc_link: *mut UcontextT,
    /// The stack a `makecontext` function runs on.
    pub uc_stack: StackT,
    pub uc_mcontext: McontextT,
    pub uc_sigmask: SigsetT,
    /// The floating-point state `uc_mcontext.fpregs` points at, for a
    /// context from `getcontext`.
    pub fpregs_mem: [u64; 64],
}

impl UcontextT {
    /// An all-zero context, for a caller about to `getcontext` into it.
    pub const ZERO: Self = Self {
        uc_flags: 0,
        uc_link: core::ptr::null_mut(),
        uc_stack: StackT {
            ss_sp: core::ptr::null_mut(),
            ss_flags: 0,
            ss_size: 0,
        },
        uc_mcontext: McontextT {
            gregs: [0; NGREG],
            fpregs: core::ptr::null_mut(),
            reserved1: [0; 8],
        },
        uc_sigmask: SigsetT::EMPTY,
        fpregs_mem: [0; 64],
    };
}

/// Where general register `reg` is in a `UcontextT`.
// Compile-time only, on the register indices above: no overflow.
#[allow(clippy::arithmetic_side_effects)]
const fn greg(reg: usize) -> usize {
    offset_of!(UcontextT, uc_mcontext) + offset_of!(McontextT, gregs) + 8 * reg
}

// The offsets the assembly uses, derived from the structure so the two
// cannot drift apart.
const UC_R8: usize = greg(REG_R8);
const UC_R9: usize = greg(REG_R9);
const UC_R12: usize = greg(REG_R12);
const UC_R13: usize = greg(REG_R13);
const UC_R14: usize = greg(REG_R14);
const UC_R15: usize = greg(REG_R15);
const UC_RDI: usize = greg(REG_RDI);
const UC_RSI: usize = greg(REG_RSI);
const UC_RBP: usize = greg(REG_RBP);
const UC_RBX: usize = greg(REG_RBX);
const UC_RDX: usize = greg(REG_RDX);
const UC_RCX: usize = greg(REG_RCX);
const UC_RSP: usize = greg(REG_RSP);
const UC_RIP: usize = greg(REG_RIP);
const UC_FPREGS: usize = offset_of!(UcontextT, uc_mcontext) + offset_of!(McontextT, fpregs);
const UC_FPMEM: usize = offset_of!(UcontextT, fpregs_mem);
const UC_MXCSR: usize = UC_FPMEM + offset_of!(Fpstate, mxcsr);

// `fnstenv` stores 28 bytes and `_fpstate`'s `mxcsr` sits at 24, so the
// `stmxcsr` after it overwrites the environment's last field, the data
// segment -- which 64-bit mode does not use. glibc's layout, and the reason
// replaying the block with `fldenv` is safe.
const _: () = assert!(core::mem::size_of::<UcontextT>() == 936);
const _: () = assert!(UC_MXCSR - UC_FPMEM == 24);

/// `getcontext`'s signal-mask half: the current mask into `ucp`.
///
/// # Safety
///
/// `ucp` is a writable context.
#[unsafe(no_mangle)]
unsafe extern "sysv64" fn __slate_uc_getmask(ucp: *mut UcontextT) -> i32 {
    // SAFETY: the caller's context.
    let mask = unsafe { &raw mut (*ucp).uc_sigmask };
    crate::signal::sigprocmask(crate::signal::SIG_BLOCK, core::ptr::null(), mask)
}

/// `setcontext`'s signal-mask half: `ucp`'s mask made the current one.
///
/// # Safety
///
/// `ucp` is a readable context.
#[unsafe(no_mangle)]
unsafe extern "sysv64" fn __slate_uc_setmask(ucp: *const UcontextT) -> i32 {
    // SAFETY: the caller's context.
    let mask = unsafe { &raw const (*ucp).uc_sigmask };
    crate::signal::sigprocmask(crate::signal::SIG_SETMASK, mask, core::ptr::null_mut())
}

/// `swapcontext`'s: `ucp`'s mask made current, the old one into `oucp`.
///
/// # Safety
///
/// `oucp` is writable and `ucp` readable.
#[unsafe(no_mangle)]
unsafe extern "sysv64" fn __slate_uc_swapmask(oucp: *mut UcontextT, ucp: *const UcontextT) -> i32 {
    // SAFETY: the caller's contexts.
    let (new, old) = unsafe { (&raw const (*ucp).uc_sigmask, &raw mut (*oucp).uc_sigmask) };
    crate::signal::sigprocmask(crate::signal::SIG_SETMASK, new, old)
}

/// Where a `makecontext` function's context ends with no `uc_link`, or with
/// one `setcontext` could not resume: `exit`, as glibc's does -- 0, or -1
/// for the failure.
#[unsafe(no_mangle)]
extern "sysv64" fn __slate_uc_exit(status: i32) -> ! {
    crate::crt::exit(status)
}

/// The four entry points: `getcontext`, `setcontext`, `swapcontext` and
/// the trampoline a `makecontext` function returns into. `$get`, `$set`,
/// `$swap` and `$start` name them; the `$*t`/`$*s` literals are the lines
/// that give each its symbol type and size -- ELF's, or nothing on the
/// host.
macro_rules! context_asm {
    ($get:literal, $set:literal, $swap:literal, $start:literal,
     $gt:literal, $gs:literal, $st:literal, $ss:literal,
     $wt:literal, $ws:literal, $tt:literal, $ts:literal) => {
        core::arch::global_asm!(
            // getcontext(ucp): record, return 0 (or the mask's -1).
            concat!(".global ", $get),
            $gt,
            concat!($get, ":"),
            "mov [rdi + {rbx}], rbx",
            "mov [rdi + {rbp}], rbp",
            "mov [rdi + {r12}], r12",
            "mov [rdi + {r13}], r13",
            "mov [rdi + {r14}], r14",
            "mov [rdi + {r15}], r15",
            "mov [rdi + {rdi}], rdi",
            "mov [rdi + {rsi}], rsi",
            "mov [rdi + {rdx}], rdx",
            "mov [rdi + {rcx}], rcx",
            "mov [rdi + {r8}], r8",
            "mov [rdi + {r9}], r9",
            // Resume where the call returns, with the return address gone.
            "mov rcx, [rsp]",
            "mov [rdi + {rip}], rcx",
            "lea rcx, [rsp + 8]",
            "mov [rdi + {rsp}], rcx",
            // The floating-point control state, in the context's own block.
            "lea rcx, [rdi + {fpmem}]",
            "mov [rdi + {fpregs}], rcx",
            "fnstenv [rcx]",
            // `fnstenv` masks every x87 exception; put them back as they were.
            "fldenv [rcx]",
            "stmxcsr [rdi + {mxcsr}]",
            // The mask, with the stack aligned for the call.
            "sub rsp, 8",
            "call {getmask}",
            "add rsp, 8",
            "ret",
            $gs,

            // setcontext(ucp): resume; returns only on failure, with -1.
            concat!(".global ", $set),
            $st,
            concat!($set, ":"),
            "push rdi",
            "call {setmask}",
            "pop rdx",
            "test eax, eax",
            "jnz 2f",
            "3:",
            "mov rcx, [rdx + {fpregs}]",
            "fldenv [rcx]",
            "ldmxcsr [rdx + {mxcsr}]",
            "mov rsp, [rdx + {rsp}]",
            "mov rbx, [rdx + {rbx}]",
            "mov rbp, [rdx + {rbp}]",
            "mov r12, [rdx + {r12}]",
            "mov r13, [rdx + {r13}]",
            "mov r14, [rdx + {r14}]",
            "mov r15, [rdx + {r15}]",
            // `ret` below goes where the context resumes.
            "push qword ptr [rdx + {rip}]",
            "mov rdi, [rdx + {rdi}]",
            "mov rsi, [rdx + {rsi}]",
            "mov rcx, [rdx + {rcx}]",
            "mov r8, [rdx + {r8}]",
            "mov r9, [rdx + {r9}]",
            "mov rdx, [rdx + {rdx}]",
            "xor eax, eax",
            "2:",
            "ret",
            $ss,

            // swapcontext(oucp, ucp): record into oucp, resume ucp.
            concat!(".global ", $swap),
            $wt,
            concat!($swap, ":"),
            "mov [rdi + {rbx}], rbx",
            "mov [rdi + {rbp}], rbp",
            "mov [rdi + {r12}], r12",
            "mov [rdi + {r13}], r13",
            "mov [rdi + {r14}], r14",
            "mov [rdi + {r15}], r15",
            "mov [rdi + {rdi}], rdi",
            "mov [rdi + {rsi}], rsi",
            "mov [rdi + {rdx}], rdx",
            "mov [rdi + {rcx}], rcx",
            "mov [rdi + {r8}], r8",
            "mov [rdi + {r9}], r9",
            "mov rcx, [rsp]",
            "mov [rdi + {rip}], rcx",
            "lea rcx, [rsp + 8]",
            "mov [rdi + {rsp}], rcx",
            "lea rcx, [rdi + {fpmem}]",
            "mov [rdi + {fpregs}], rcx",
            "fnstenv [rcx]",
            "fldenv [rcx]",
            "stmxcsr [rdi + {mxcsr}]",
            // rdi and rsi still hold oucp and ucp; keep ucp across the call.
            "push rsi",
            "call {swapmask}",
            "pop rdx",
            "test eax, eax",
            "jnz 4f",
            // A resumed `oucp` returns 0 from here.
            "jmp 3b",
            "4:",
            "ret",
            $ws,

            // Where a makecontext function returns: rbx is the uc_link slot
            // above its stack arguments, kept across the call as the ABI
            // requires.
            concat!(".global ", $start),
            $tt,
            concat!($start, ":"),
            "mov rsp, rbx",
            "mov rdi, [rsp]",
            // Nothing returns here; align for the calls.
            "and rsp, -16",
            "test rdi, rdi",
            "jz 5f",
            concat!("call ", $set),
            "mov edi, eax",
            "5:",
            "call {exit}",
            "ud2",
            $ts,

            rbx = const UC_RBX,
            rbp = const UC_RBP,
            r12 = const UC_R12,
            r13 = const UC_R13,
            r14 = const UC_R14,
            r15 = const UC_R15,
            rdi = const UC_RDI,
            rsi = const UC_RSI,
            rdx = const UC_RDX,
            rcx = const UC_RCX,
            r8 = const UC_R8,
            r9 = const UC_R9,
            rip = const UC_RIP,
            rsp = const UC_RSP,
            fpmem = const UC_FPMEM,
            fpregs = const UC_FPREGS,
            mxcsr = const UC_MXCSR,
            getmask = sym __slate_uc_getmask,
            setmask = sym __slate_uc_setmask,
            swapmask = sym __slate_uc_swapmask,
            exit = sym __slate_uc_exit,
        );
    };
}

#[cfg(target_os = "none")]
context_asm!(
    "getcontext",
    "setcontext",
    "swapcontext",
    "__slate_start_context",
    ".type getcontext, @function",
    ".size getcontext, . - getcontext",
    ".type setcontext, @function",
    ".size setcontext, . - setcontext",
    ".type swapcontext, @function",
    ".size swapcontext, . - swapcontext",
    ".type __slate_start_context, @function",
    ".size __slate_start_context, . - __slate_start_context"
);

// The host's copies, under their own names, for the tests: COFF has no
// `.type` or `.size`.
#[cfg(all(not(target_os = "none"), target_arch = "x86_64"))]
context_asm!(
    "slate_uc_getcontext",
    "slate_uc_setcontext",
    "slate_uc_swapcontext",
    "slate_uc_start_context",
    "",
    "",
    "",
    "",
    "",
    "",
    "",
    ""
);

/// The trampoline's address, for `makecontext` to put under a function.
fn start_context() -> u64 {
    unsafe extern "sysv64" {
        #[cfg_attr(target_os = "none", link_name = "__slate_start_context")]
        #[cfg_attr(not(target_os = "none"), link_name = "slate_uc_start_context")]
        fn start() -> !;
    }
    (start as *const ()).addr() as u64
}

/// `makecontext(ucp, func, argc, ...)` with its variadic arguments as a
/// `va_list`: `ucp`, recorded by `getcontext`, made to call `func` with
/// `argc` arguments on `ucp->uc_stack` when it is resumed, and to resume
/// `ucp->uc_link` -- or exit -- when `func` returns. The C symbol is a
/// variadic trampoline into this (`va_trampoline!`).
///
/// Each argument is taken as a full 64-bit register's worth, as glibc takes
/// it: POSIX says `int`, and a pointer passed as one works on x86-64 either
/// way. The first six go in the argument registers, the rest on the new
/// stack above the return address, as a call would put them.
///
/// # Safety
///
/// `ucp` is a context from `getcontext` whose `uc_stack` names memory the
/// function may use; `ap` holds at least `argc` integer arguments.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub unsafe extern "C" fn __slate_vmakecontext(
    ucp: *mut UcontextT,
    func: usize,
    argc: i32,
    ap: *mut crate::printf::VaList,
) {
    if ucp.is_null() {
        return;
    }
    // SAFETY: the caller's context.
    let uc = unsafe { &mut *ucp };
    let args = usize::try_from(argc).unwrap_or(0);
    let on_stack = args.saturating_sub(6);
    // From the stack's top down: the uc_link slot, the stack arguments, and
    // the return address -- where `rsp` starts, which the ABI wants 8 past a
    // 16-byte boundary at a function's first instruction.
    let top = (uc.uc_stack.ss_sp as usize).wrapping_add(uc.uc_stack.ss_size);
    let lowest = top.wrapping_sub(8 * (on_stack + 2));
    let rsp = (lowest.wrapping_sub(8) & !15) + 8;
    let slot = |i: usize| (rsp + 8 * i) as *mut u64;
    let link_slot = slot(on_stack + 1);
    // SAFETY: every slot is within `uc_stack`, which the caller gave the
    // function to use; each is 8-byte aligned.
    unsafe {
        slot(0).write(start_context());
        link_slot.write(uc.uc_link as u64);
    }
    let gregs = &mut uc.uc_mcontext.gregs;
    let regs = [REG_RDI, REG_RSI, REG_RDX, REG_RCX, REG_R8, REG_R9];
    for i in 0..args {
        // SAFETY: the caller promised `argc` arguments in `ap`.
        let value = unsafe { crate::printf::va_arg_int(&mut *ap) };
        match regs.get(i) {
            Some(&r) => set_greg(gregs, r, value),
            // SAFETY: argument `i` past the sixth goes in slot `i - 5`.
            None => unsafe { slot(i - 5).write(value) },
        }
    }
    set_greg(gregs, REG_RIP, func as u64);
    set_greg(gregs, REG_RSP, rsp as u64);
    set_greg(gregs, REG_RBX, link_slot as u64);
}

/// `gregs[reg] = value`, a register's bits as C's `greg_t` holds them.
fn set_greg(gregs: &mut [i64; NGREG], reg: usize, value: u64) {
    if let Some(g) = gregs.get_mut(reg) {
        *g = value.cast_signed();
    }
}

#[cfg(target_os = "none")]
crate::printf::va_trampoline!("makecontext", "__slate_vmakecontext", "24", "rcx");

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, Ordering::SeqCst};
    use std::boxed::Box;

    unsafe extern "sysv64" {
        fn slate_uc_getcontext(ucp: *mut UcontextT) -> i32;
        fn slate_uc_swapcontext(oucp: *mut UcontextT, ucp: *const UcontextT) -> i32;
    }

    /// A stack for a context: 64 KiB, 16-byte aligned.
    #[repr(C, align(16))]
    struct Stack([u8; 65536]);

    impl Stack {
        fn new() -> Box<Self> {
            Box::new(Self([0; 65536]))
        }

        fn as_stack_t(&mut self) -> StackT {
            StackT {
                ss_sp: self.0.as_mut_ptr(),
                ss_flags: 0,
                ss_size: self.0.len(),
            }
        }
    }

    /// A context ready to run `func(args...)` on `stack`, resuming `link`
    /// when it returns: `getcontext`, then `makecontext` -- through a
    /// `va_list` whose registers are spent, so every argument comes from its
    /// overflow area, as the ninth of a C call's would.
    fn context_for(
        stack: &mut Stack,
        link: *mut UcontextT,
        func: usize,
        args: &[u64],
    ) -> Box<UcontextT> {
        let mut uc = Box::new(UcontextT::ZERO);
        // SAFETY: a writable context.
        assert_eq!(unsafe { slate_uc_getcontext(&raw mut *uc) }, 0);
        uc.uc_stack = stack.as_stack_t();
        uc.uc_link = link;
        let mut overflow = args.to_vec();
        let mut va = crate::printf::VaList {
            gp_offset: 48,
            fp_offset: 176,
            overflow_arg_area: overflow.as_mut_ptr().cast(),
            reg_save_area: core::ptr::null_mut(),
        };
        let argc = i32::try_from(args.len()).unwrap();
        // SAFETY: a context from `getcontext` with a stack, and a va_list
        // holding `argc` arguments.
        unsafe { __slate_vmakecontext(&raw mut *uc, func, argc, &raw mut va) };
        uc
    }

    static SEEN: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

    extern "sysv64" fn eight(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64) {
        for (slot, v) in SEEN.iter().zip([a, b, c, d, e, f, g, h]) {
            slot.store(v, SeqCst);
        }
    }

    /// `makecontext`'s function runs on its own stack with every argument
    /// -- six in registers, two on the stack above the return address --
    /// and returning resumes `uc_link`: here, the `swapcontext` that started
    /// it, which then returns 0.
    #[test]
    fn a_made_context_runs_its_function_with_eight_arguments_and_returns_to_uc_link() {
        let mut stack = Stack::new();
        let mut main = Box::new(UcontextT::ZERO);
        let args = [11, 22, 33, 44, 55, 66, 0x7777_0000_0000_0077, u64::MAX];
        let ctx = context_for(
            &mut stack,
            &raw mut *main,
            eight as *const () as usize,
            &args,
        );
        // SAFETY: two valid contexts; the made one returns to `main`.
        assert_eq!(
            unsafe { slate_uc_swapcontext(&raw mut *main, &raw const *ctx) },
            0
        );
        let seen: Vec<u64> = SEEN.iter().map(|s| s.load(SeqCst)).collect();
        assert_eq!(seen, args);
        // The function's frames were on the given stack: the return address
        // and the stack arguments sit just below its top.
        let top = stack.0.as_ptr() as usize + stack.0.len();
        let rsp = usize::try_from(ctx.uc_mcontext.gregs[REG_RSP]).unwrap();
        assert!(rsp < top && top - rsp <= 64, "rsp {rsp:#x}, top {top:#x}");
        assert_eq!(
            rsp % 16,
            8,
            "rsp is 8 past a 16-byte boundary at a function's entry"
        );
    }

    static MAIN: AtomicPtr<UcontextT> = AtomicPtr::new(core::ptr::null_mut());
    static CORO: AtomicPtr<UcontextT> = AtomicPtr::new(core::ptr::null_mut());
    static YIELDS: AtomicU64 = AtomicU64::new(0);
    static RESUMED: AtomicU64 = AtomicU64::new(0);

    extern "sysv64" fn coroutine(rounds: u64) {
        for _ in 0..rounds {
            YIELDS.fetch_add(1, SeqCst);
            // SAFETY: both contexts outlive the switches; `CORO` is ours.
            if unsafe { slate_uc_swapcontext(CORO.load(SeqCst), MAIN.load(SeqCst)) } == 0 {
                RESUMED.fetch_add(1, SeqCst);
            }
        }
    }

    /// Two contexts handing control back and forth, a coroutine's shape: each
    /// `swapcontext` records the side it leaves and resumes the other, and
    /// returns 0 when it is itself resumed.
    #[test]
    fn swapcontext_passes_control_back_and_forth() {
        let mut stack = Stack::new();
        let mut main = Box::new(UcontextT::ZERO);
        MAIN.store(&raw mut *main, SeqCst);
        let mut coro = context_for(
            &mut stack,
            &raw mut *main,
            coroutine as *const () as usize,
            &[3],
        );
        CORO.store(&raw mut *coro, SeqCst);
        for round in 1..=3u64 {
            // SAFETY: as above.
            assert_eq!(
                unsafe { slate_uc_swapcontext(&raw mut *main, &raw const *coro) },
                0
            );
            assert_eq!(YIELDS.load(SeqCst), round, "yields, round {round}");
            assert_eq!(RESUMED.load(SeqCst), round - 1, "resumes, round {round}");
        }
        // The last resume lets the coroutine finish; its return resumes `main`.
        // SAFETY: as above.
        assert_eq!(
            unsafe { slate_uc_swapcontext(&raw mut *main, &raw const *coro) },
            0
        );
        assert_eq!((YIELDS.load(SeqCst), RESUMED.load(SeqCst)), (3, 3));
    }

    static MASK_IN_CONTEXT: AtomicU64 = AtomicU64::new(u64::MAX);
    static ROUNDING_IN_CONTEXT: AtomicI32 = AtomicI32::new(-1);

    extern "sysv64" fn report_mask_and_rounding() {
        let mut now = SigsetT::EMPTY;
        let rc =
            crate::signal::sigprocmask(crate::signal::SIG_BLOCK, core::ptr::null(), &raw mut now);
        MASK_IN_CONTEXT.store(if rc == 0 { now.bits[0] } else { u64::MAX - 1 }, SeqCst);
        ROUNDING_IN_CONTEXT.store(crate::fenv::fegetround(), SeqCst);
    }

    /// A context carries the signal mask and the rounding direction it was
    /// recorded with, and resuming the one that switched to it brings that
    /// one's back.
    #[test]
    fn a_context_carries_its_signal_mask_and_rounding_direction() {
        use crate::signal::{SIG_SETMASK, sigprocmask};
        let mut before = SigsetT::EMPTY;
        assert_eq!(
            sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut before),
            0
        );
        let mut recorded = SigsetT::EMPTY;
        recorded.bits[0] = 1 << (10 - 1); // SIGUSR1
        assert_eq!(
            sigprocmask(SIG_SETMASK, &raw const recorded, core::ptr::null_mut()),
            0
        );
        assert_eq!(crate::fenv::fesetround(crate::fenv::FE_UPWARD), 0);

        let mut stack = Stack::new();
        let mut main = Box::new(UcontextT::ZERO);
        let ctx = context_for(
            &mut stack,
            &raw mut *main,
            report_mask_and_rounding as *const () as usize,
            &[],
        );

        // Change both before switching; the context must bring its own.
        let other = SigsetT::EMPTY;
        assert_eq!(
            sigprocmask(SIG_SETMASK, &raw const other, core::ptr::null_mut()),
            0
        );
        assert_eq!(crate::fenv::fesetround(crate::fenv::FE_TONEAREST), 0);
        // SAFETY: as above.
        assert_eq!(
            unsafe { slate_uc_swapcontext(&raw mut *main, &raw const *ctx) },
            0
        );

        assert_eq!(MASK_IN_CONTEXT.load(SeqCst), 1 << (10 - 1));
        assert_eq!(ROUNDING_IN_CONTEXT.load(SeqCst), crate::fenv::FE_UPWARD);
        // And back here, this side's.
        let mut now = SigsetT::EMPTY;
        assert_eq!(sigprocmask(SIG_SETMASK, core::ptr::null(), &raw mut now), 0);
        assert_eq!(now.bits[0], 0);
        assert_eq!(crate::fenv::fegetround(), crate::fenv::FE_TONEAREST);
        assert_eq!(
            sigprocmask(SIG_SETMASK, &raw const before, core::ptr::null_mut()),
            0
        );
    }

    /// `getcontext` records where it was called from and the stack as the
    /// caller had it, the callee-saved registers, and a pointer to its own
    /// floating-point block with the SSE control register in it.
    #[test]
    fn getcontext_records_the_callers_state() {
        let mut uc = Box::new(UcontextT::ZERO);
        // SAFETY: a writable context.
        assert_eq!(unsafe { slate_uc_getcontext(&raw mut *uc) }, 0);
        let fp = uc.uc_mcontext.fpregs;
        assert_eq!(fp.cast::<u64>(), uc.fpregs_mem.as_mut_ptr());
        // SAFETY: `fpregs` points into `uc`.
        let mxcsr = unsafe { (*fp).mxcsr };
        let mut now = 0u32;
        // SAFETY: stores the current MXCSR into a local.
        unsafe { core::arch::asm!("stmxcsr [{}]", in(reg) &raw mut now, options(nostack)) };
        assert_eq!(mxcsr, now);
        assert_ne!(uc.uc_mcontext.gregs[REG_RIP], 0);
        assert_ne!(uc.uc_mcontext.gregs[REG_RSP], 0);
    }

    /// The layout is musl's (`abi_layout.rs` checks it against the headers
    /// when the gate runs; this is the same claim, on every test run).
    #[test]
    fn the_layout_is_musls() {
        assert_eq!(core::mem::size_of::<UcontextT>(), 936);
        assert_eq!(offset_of!(UcontextT, uc_link), 8);
        assert_eq!(offset_of!(UcontextT, uc_stack), 16);
        assert_eq!(offset_of!(UcontextT, uc_mcontext), 40);
        assert_eq!(offset_of!(UcontextT, uc_sigmask), 296);
        assert_eq!(offset_of!(UcontextT, fpregs_mem), 424);
        assert_eq!(core::mem::size_of::<McontextT>(), 256);
        assert_eq!(core::mem::size_of::<Fpstate>(), 512);
        assert_eq!(UC_RIP, 40 + 8 * 16);
    }
}
