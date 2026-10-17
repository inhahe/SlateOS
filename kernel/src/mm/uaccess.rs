//! The kernel's accesses to user memory that may fault: a byte copy and the
//! futex word's atomics, as assembly whose faulting instructions are known.
//!
//! A user address the kernel validated can still fault when it is touched:
//! another thread unmaps it, a truncate drops the file page it maps, a save
//! makes a shared file mapping's page read-only again (design-decisions
//! §1572), the page is swapped out. The page-fault handler first resolves
//! such a fault as the program's own would be resolved. What it cannot
//! resolve must not halt the kernel: the fault of an instruction listed here
//! ([`fixup_for`]) resumes at that routine's fixup, which returns its "fault"
//! answer, and the system call answers `EFAULT`. Linux's exception table
//! (`_ASM_EXTABLE_UA`), by instruction address, so that it holds with SMAP
//! or without it.
//!
//! Every routine here is called inside a user-access window
//! (`smep_smap::stac`), by `mm::user`, which validates the range first.

use core::arch::global_asm;

// Each routine is a leaf function of the SysV ABI. The instruction at each
// `_insn` label is the only one that touches user memory; its fixup is at
// the label beside it in `fixup_for`.
global_asm!(
    ".section .text.uaccess,\"ax\",@progbits",
    // copy(dst rdi, src rsi, len rdx) -> rax: the bytes not copied.
    ".global slateos_uaccess_copy",
    "slateos_uaccess_copy:",
    "mov rcx, rdx",
    ".global slateos_uaccess_copy_insn",
    "slateos_uaccess_copy_insn:",
    "rep movsb",
    "xor eax, eax",
    "ret",
    // A faulting `rep movsb` stops with RCX the count still to copy.
    ".global slateos_uaccess_copy_fixup",
    "slateos_uaccess_copy_fixup:",
    "mov rax, rcx",
    "ret",
    // load32(addr rdi, out rsi) -> rax: 0, or 1 on a fault.
    ".global slateos_uaccess_load32",
    "slateos_uaccess_load32:",
    ".global slateos_uaccess_load32_insn",
    "slateos_uaccess_load32_insn:",
    "mov eax, dword ptr [rdi]",
    "mov dword ptr [rsi], eax",
    "xor eax, eax",
    "ret",
    // store32(addr rdi, value esi) -> rax.
    ".global slateos_uaccess_store32",
    "slateos_uaccess_store32:",
    ".global slateos_uaccess_store32_insn",
    "slateos_uaccess_store32_insn:",
    "mov dword ptr [rdi], esi",
    "xor eax, eax",
    "ret",
    // cas32(addr rdi, expected esi, new edx, out rcx) -> rax; `out` gets
    // the word as it was, `expected` when the swap happened.
    ".global slateos_uaccess_cas32",
    "slateos_uaccess_cas32:",
    "mov eax, esi",
    ".global slateos_uaccess_cas32_insn",
    "slateos_uaccess_cas32_insn:",
    "lock cmpxchg dword ptr [rdi], edx",
    "mov dword ptr [rcx], eax",
    "xor eax, eax",
    "ret",
    // xchg32(addr rdi, value esi, out rdx) -> rax; `out` gets the old word.
    ".global slateos_uaccess_xchg32",
    "slateos_uaccess_xchg32:",
    ".global slateos_uaccess_xchg32_insn",
    "slateos_uaccess_xchg32_insn:",
    "xchg dword ptr [rdi], esi",
    "mov dword ptr [rdx], esi",
    "xor eax, eax",
    "ret",
    // xadd32(addr rdi, value esi, out rdx) -> rax; `out` gets the old word.
    ".global slateos_uaccess_xadd32",
    "slateos_uaccess_xadd32:",
    ".global slateos_uaccess_xadd32_insn",
    "slateos_uaccess_xadd32_insn:",
    "lock xadd dword ptr [rdi], esi",
    "mov dword ptr [rdx], esi",
    "xor eax, eax",
    "ret",
    // bits32(addr rdi, operand esi, out rdx, op ecx: 0 or, 1 and, 2 xor)
    // -> rax; `out` gets the old word. A compare-exchange loop: x86 has no
    // fetch-or. The load and the exchange both touch the word.
    ".global slateos_uaccess_bits32",
    "slateos_uaccess_bits32:",
    "mov r8d, ecx",
    ".global slateos_uaccess_bits32_load_insn",
    "slateos_uaccess_bits32_load_insn:",
    "mov eax, dword ptr [rdi]",
    "2:",
    "mov ecx, eax",
    "cmp r8d, 1",
    "je 3f",
    "ja 4f",
    "or ecx, esi",
    "jmp 5f",
    "3:",
    "and ecx, esi",
    "jmp 5f",
    "4:",
    "xor ecx, esi",
    "5:",
    ".global slateos_uaccess_bits32_cas_insn",
    "slateos_uaccess_bits32_cas_insn:",
    "lock cmpxchg dword ptr [rdi], ecx",
    "jnz 2b",
    "mov dword ptr [rdx], eax",
    "xor eax, eax",
    "ret",
    // The atomics' fixup: the word was not touched, or not all of it.
    ".global slateos_uaccess_fault_ret",
    "slateos_uaccess_fault_ret:",
    "mov eax, 1",
    "ret",
    ".text",
);

unsafe extern "C" {
    fn slateos_uaccess_copy(dst: *mut u8, src: *const u8, len: usize) -> usize;
    fn slateos_uaccess_copy_insn();
    fn slateos_uaccess_copy_fixup();
    fn slateos_uaccess_load32(addr: u64, out: *mut u32) -> u64;
    fn slateos_uaccess_load32_insn();
    fn slateos_uaccess_store32(addr: u64, value: u32) -> u64;
    fn slateos_uaccess_store32_insn();
    fn slateos_uaccess_cas32(addr: u64, expected: u32, new: u32, out: *mut u32) -> u64;
    fn slateos_uaccess_cas32_insn();
    fn slateos_uaccess_xchg32(addr: u64, value: u32, out: *mut u32) -> u64;
    fn slateos_uaccess_xchg32_insn();
    fn slateos_uaccess_xadd32(addr: u64, value: u32, out: *mut u32) -> u64;
    fn slateos_uaccess_xadd32_insn();
    fn slateos_uaccess_bits32(addr: u64, operand: u32, out: *mut u32, op: u32) -> u64;
    fn slateos_uaccess_bits32_load_insn();
    fn slateos_uaccess_bits32_cas_insn();
    fn slateos_uaccess_fault_ret();
}

/// Where a fault at kernel instruction `rip` resumes, when `rip` is one of
/// this module's user-memory accesses: its routine's fixup, which returns the
/// routine's fault answer. `None` for every other instruction.
#[must_use]
pub fn fixup_for(rip: u64) -> Option<u64> {
    let at = |f: unsafe extern "C" fn()| f as usize as u64;
    let atomic = at(slateos_uaccess_fault_ret);
    [
        (
            at(slateos_uaccess_copy_insn),
            at(slateos_uaccess_copy_fixup),
        ),
        (at(slateos_uaccess_load32_insn), atomic),
        (at(slateos_uaccess_store32_insn), atomic),
        (at(slateos_uaccess_cas32_insn), atomic),
        (at(slateos_uaccess_xchg32_insn), atomic),
        (at(slateos_uaccess_xadd32_insn), atomic),
        (at(slateos_uaccess_bits32_load_insn), atomic),
        (at(slateos_uaccess_bits32_cas_insn), atomic),
    ]
    .into_iter()
    .find_map(|(insn, fixup)| (insn == rip).then_some(fixup))
}

/// Copy `len` bytes from `src` to `dst`, one side of them user memory.
/// Answers how many bytes were **not** copied: 0, or the rest from the first
/// byte whose page could not be had.
///
/// # Safety
///
/// Inside a user-access window. The kernel side is a valid buffer of `len`
/// bytes; the user side lies wholly in user space (validated), so a fault
/// there is a user page's.
#[must_use]
pub unsafe fn copy(dst: *mut u8, src: *const u8, len: usize) -> usize {
    // SAFETY: the caller's contract; a fault on the user side resumes at the
    // fixup, which answers the count left.
    unsafe { slateos_uaccess_copy(dst, src, len) }
}

/// The ways a futex word's read-modify-write combines it with an operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bits {
    /// `old | operand`.
    Or,
    /// `old & operand`.
    And,
    /// `old ^ operand`.
    Xor,
}

/// One of the futex word's accesses. `Err(())` when the word's page could
/// not be had.
type Faulted<T> = Result<T, ()>;

/// The routine's status (0 done, 1 faulted) as a [`Faulted`].
fn done<T>(status: u64, value: T) -> Faulted<T> {
    if status == 0 { Ok(value) } else { Err(()) }
}

/// Load the user word at `addr`.
///
/// # Safety
///
/// Inside a user-access window; `addr` a validated, 4-byte-aligned user
/// address.
pub unsafe fn load32(addr: u64) -> Faulted<u32> {
    let mut out = 0u32;
    // SAFETY: the caller's contract; `out` is ours.
    let status = unsafe { slateos_uaccess_load32(addr, &raw mut out) };
    done(status, out)
}

/// Store `value` to the user word at `addr` (a release store on x86).
///
/// # Safety
///
/// As [`load32`].
pub unsafe fn store32(addr: u64, value: u32) -> Faulted<()> {
    // SAFETY: the caller's contract.
    let status = unsafe { slateos_uaccess_store32(addr, value) };
    done(status, ())
}

/// Compare-and-exchange the user word at `addr`: `Ok(Ok(expected))` when it
/// held `expected` and now holds `new`, `Ok(Err(actual))` when it held
/// `actual` instead -- `AtomicU32::compare_exchange`'s answer.
///
/// # Safety
///
/// As [`load32`].
pub unsafe fn cas32(addr: u64, expected: u32, new: u32) -> Faulted<Result<u32, u32>> {
    let mut out = 0u32;
    // SAFETY: the caller's contract; `out` is ours.
    let status = unsafe { slateos_uaccess_cas32(addr, expected, new, &raw mut out) };
    done(status, if out == expected { Ok(out) } else { Err(out) })
}

/// Exchange the user word at `addr` for `value`: the old word.
///
/// # Safety
///
/// As [`load32`].
pub unsafe fn xchg32(addr: u64, value: u32) -> Faulted<u32> {
    let mut out = 0u32;
    // SAFETY: the caller's contract; `out` is ours.
    let status = unsafe { slateos_uaccess_xchg32(addr, value, &raw mut out) };
    done(status, out)
}

/// Add `value` to the user word at `addr`, wrapping: the old word.
///
/// # Safety
///
/// As [`load32`].
pub unsafe fn xadd32(addr: u64, value: u32) -> Faulted<u32> {
    let mut out = 0u32;
    // SAFETY: the caller's contract; `out` is ours.
    let status = unsafe { slateos_uaccess_xadd32(addr, value, &raw mut out) };
    done(status, out)
}

/// Combine the user word at `addr` with `operand` as `op` says, atomically:
/// the old word.
///
/// # Safety
///
/// As [`load32`].
pub unsafe fn bits32(addr: u64, op: Bits, operand: u32) -> Faulted<u32> {
    let code = match op {
        Bits::Or => 0,
        Bits::And => 1,
        Bits::Xor => 2,
    };
    let mut out = 0u32;
    // SAFETY: the caller's contract; `out` is ours.
    let status = unsafe { slateos_uaccess_bits32(addr, operand, &raw mut out, code) };
    done(status, out)
}

/// Each routine on kernel memory, where nothing faults, gives the answers
/// its Rust counterpart would; every faulting instruction has its fixup and
/// nothing else does; and on a user address this address space does not map
/// -- a kernel task's maps none -- each faults, and its fixup answers: the
/// copy every byte left, the atomics a fault. That runs the page-fault
/// handler's whole path for them, with no process to resolve the fault for.
///
/// # Errors
///
/// `InternalError` for a failed check.
pub fn self_test() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    use crate::mm::page_table::{self, VirtAddr};
    crate::serial_println!("[uaccess] Running self-test...");
    let fail = |what: &str| -> crate::error::KernelResult<()> {
        crate::serial_println!("[uaccess]   FAIL: {}", what);
        Err(KernelError::InternalError)
    };
    let src = *b"user-copy";
    let mut dst = [0u8; 9];
    // SAFETY: two kernel buffers of 9 bytes; nothing faults.
    let left = unsafe { copy(dst.as_mut_ptr(), src.as_ptr(), src.len()) };
    if left != 0 || dst != src {
        return fail("a copy of kernel memory did not copy every byte");
    }
    let mut word = 5u32;
    let at = (&raw mut word) as u64;
    // SAFETY: `at` is our own aligned word; nothing faults, and no other
    // reference to `word` is used while the routines run.
    let answers = unsafe {
        [
            load32(at) == Ok(5),
            store32(at, 6) == Ok(()),
            cas32(at, 6, 7) == Ok(Ok(6)),
            cas32(at, 6, 8) == Ok(Err(7)),
            xchg32(at, 9) == Ok(7),
            xadd32(at, 1) == Ok(9),
            bits32(at, Bits::Or, 0b0101) == Ok(10),
            bits32(at, Bits::And, 0b0110) == Ok(15),
            bits32(at, Bits::Xor, 0b0011) == Ok(6),
            load32(at) == Ok(5),
        ]
    };
    if let Some(i) = answers.iter().position(|ok| !ok) {
        crate::serial_println!("[uaccess]     answer {} of the sequence was wrong", i);
        return fail("a futex-word routine did not answer as its atomic would");
    }
    let at = |f: unsafe extern "C" fn()| f as usize as u64;
    if fixup_for(at(slateos_uaccess_copy_insn)) != Some(at(slateos_uaccess_copy_fixup))
        || fixup_for(at(slateos_uaccess_bits32_cas_insn)) != Some(at(slateos_uaccess_fault_ret))
        || fixup_for(at(slateos_uaccess_copy_fixup)).is_some()
        || fixup_for(self_test as usize as u64).is_some()
    {
        return fail("the fixup table is wrong");
    }
    // A user address nothing maps here: high in user space, far from
    // anything boot maps.
    let hole: u64 = 0x0000_7000_0000_0000;
    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());
    if crate::proc::thread::owner_process(crate::sched::current_task_id()).is_some_and(|p| p != 0)
        || page_table::translate(pml4, VirtAddr::new(hole)).is_some()
    {
        return fail("the self-test does not run where a user address is unmapped");
    }
    let mut buf = [0xAAu8; 32];
    // SAFETY: inside a user-access window; `buf` is ours; `hole` is a user
    // address, unmapped, so the access faults and resumes at its fixup.
    let (left, loaded, swapped) = unsafe {
        crate::smep_smap::stac();
        let left = copy(buf.as_mut_ptr(), hole as *const u8, buf.len());
        let loaded = load32(hole);
        let swapped = bits32(hole, Bits::Or, 1);
        crate::smep_smap::clac();
        (left, loaded, swapped)
    };
    if left != buf.len() || buf != [0xAA; 32] || loaded != Err(()) || swapped != Err(()) {
        return fail("an access to an unmapped user page did not end in its fixup's answer");
    }
    crate::serial_println!("[uaccess] Self-test PASSED");
    Ok(())
}
