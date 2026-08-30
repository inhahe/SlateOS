//! Non-local jumps (`setjmp` / `longjmp`).
//!
//! Implements the `<setjmp.h>` interface for x86_64.  `setjmp` saves the
//! callee-saved registers and return address into a `jmp_buf`, and
//! `longjmp` restores them to perform a non-local jump.
//!
//! `sigsetjmp`/`siglongjmp` extend this with signal-mask save/restore.
//! Our OS *does* deliver signals (via the kernel signal shim — see
//! `signal.rs`), and signal dispatch temporarily blocks signals while a
//! handler runs.  A handler that escapes its frame with `siglongjmp`
//! therefore needs the mask that dispatch installed to be unwound, since
//! the normal post-handler restore never runs.  So:
//!
//! - `sigsetjmp(env, savemask)`: behaves like `setjmp`, and if `savemask`
//!   is non-zero, additionally records the current process blocked mask in
//!   the buffer.
//! - `siglongjmp(env, val)`: behaves like `longjmp`, and if the buffer
//!   recorded a mask, restores it (mirroring to the kernel) before jumping.
//! - `_setjmp`/`_longjmp` (BSD): never touch the signal mask — plain
//!   aliases of `setjmp`/`longjmp`.
//!
//! ## Register Layout (x86_64 SysV ABI)
//!
//! Callee-saved registers that must be preserved:
//! - RBX, RBP, R12, R13, R14, R15
//! - RSP (stack pointer)
//! - RIP (return address, from the stack at setjmp call time)
//!
//! ```text
//! jmp_buf[0] = RBX
//! jmp_buf[1] = RBP
//! jmp_buf[2] = R12
//! jmp_buf[3] = R13
//! jmp_buf[4] = R14
//! jmp_buf[5] = R15
//! jmp_buf[6] = RSP (after setjmp returns)
//! jmp_buf[7] = RIP (return address)
//! jmp_buf[8] = mask-was-saved flag   (sigsetjmp only; 0 for setjmp)
//! jmp_buf[9] = saved blocked mask     (sigsetjmp only, when flag != 0)
//! ```

#[cfg(target_os = "none")]
use core::arch::global_asm;

/// Jump buffer: 16 x u64 = 128 bytes.
///
/// The first 8 slots hold the callee-saved registers and return address.
/// `setjmp`/`longjmp` use only those.  The remaining slots provide room for
/// `sigsetjmp`'s mask-was-saved flag (slot 8) and saved blocked mask
/// (slot 9), plus padding to a glibc-comparable size.  Sizing `JmpBuf` and
/// `SigjmpBuf` identically (as glibc does) means a buffer can be passed to
/// either `setjmp` or `sigsetjmp` without risk of an out-of-bounds write.
pub type JmpBuf = [u64; 16];

/// Byte offset of the mask-was-saved flag within a `sigjmp_buf` (slot 8).
const SIGJMP_MASK_FLAG_OFF: usize = 64;
/// Byte offset of the saved blocked mask within a `sigjmp_buf` (slot 9).
const SIGJMP_SAVED_MASK_OFF: usize = 72;
// Compile-time assertions that the offsets the assembly hardcodes match the
// `JmpBuf` layout and stay in bounds.
const _: () = assert!(SIGJMP_MASK_FLAG_OFF == 8 * 8);
const _: () = assert!(SIGJMP_SAVED_MASK_OFF == 9 * 8);
const _: () = assert!(SIGJMP_SAVED_MASK_OFF + 8 <= 16 * 8);

// The actual implementations must be in assembly because setjmp
// needs to capture the caller's registers — a Rust function would
// have already clobbered them.
#[cfg(target_os = "none")]
global_asm!(
    // ---------------------------------------------------------------
    // int setjmp(jmp_buf env)
    //
    // RDI = pointer to jmp_buf
    // Returns 0 on initial call, non-zero on longjmp return.
    // ---------------------------------------------------------------
    ".global setjmp",
    ".type setjmp, @function",
    "setjmp:",
    // Save callee-saved registers.
    "    mov [rdi],      rbx", // env[0] = RBX
    "    mov [rdi + 8],  rbp", // env[1] = RBP
    "    mov [rdi + 16], r12", // env[2] = R12
    "    mov [rdi + 24], r13", // env[3] = R13
    "    mov [rdi + 32], r14", // env[4] = R14
    "    mov [rdi + 40], r15", // env[5] = R15
    // Save RSP after return (current RSP + 8 to skip return address).
    "    lea rax, [rsp + 8]",
    "    mov [rdi + 48], rax", // env[6] = RSP (after setjmp returns)
    // Save return address (top of stack).
    "    mov rax, [rsp]",
    "    mov [rdi + 56], rax", // env[7] = RIP
    // Return 0 (initial call).
    "    xor eax, eax",
    "    ret",
    // ---------------------------------------------------------------
    // void longjmp(jmp_buf env, int val)
    //
    // RDI = pointer to jmp_buf
    // ESI = return value (0 is coerced to 1)
    // ---------------------------------------------------------------
    ".global longjmp",
    ".type longjmp, @function",
    "longjmp:",
    // If val == 0, force it to 1 (POSIX requirement).
    "    mov eax, esi",
    "    test eax, eax",
    "    jnz 1f",
    "    inc eax",
    "1:",
    // Restore callee-saved registers.
    "    mov rbx, [rdi]",      // RBX = env[0]
    "    mov rbp, [rdi + 8]",  // RBP = env[1]
    "    mov r12, [rdi + 16]", // R12 = env[2]
    "    mov r13, [rdi + 24]", // R13 = env[3]
    "    mov r14, [rdi + 32]", // R14 = env[4]
    "    mov r15, [rdi + 40]", // R15 = env[5]
    // Restore stack pointer and jump to saved return address.
    "    mov rsp, [rdi + 48]", // RSP = env[6]
    "    jmp [rdi + 56]",      // JMP to env[7] (RIP)
    // ---------------------------------------------------------------
    // int sigsetjmp(sigjmp_buf env, int savemask)
    //
    // RDI = pointer to sigjmp_buf
    // ESI = savemask (if non-zero, also save the current signal mask)
    //
    // Saves the register context exactly like setjmp, then records whether
    // the signal mask was saved (env[8]) and, if so, the mask itself
    // (env[9]).  The mask is fetched via __posix_sigjmp_save_mask.
    // ---------------------------------------------------------------
    ".global sigsetjmp",
    ".global __sigsetjmp", // glibc internal alias
    ".type sigsetjmp, @function",
    ".type __sigsetjmp, @function",
    "sigsetjmp:",
    "__sigsetjmp:",
    // Save callee-saved registers (same prologue as setjmp).
    "    mov [rdi],      rbx",
    "    mov [rdi + 8],  rbp",
    "    mov [rdi + 16], r12",
    "    mov [rdi + 24], r13",
    "    mov [rdi + 32], r14",
    "    mov [rdi + 40], r15",
    "    lea rax, [rsp + 8]",
    "    mov [rdi + 48], rax", // RSP after return
    "    mov rax, [rsp]",
    "    mov [rdi + 56], rax", // return address
    // Decide whether to save the signal mask.
    "    test esi, esi",
    "    jz 2f",
    // savemask != 0: mark saved and capture the current blocked mask.
    "    mov qword ptr [rdi + 64], 1",   // env[8] = mask_was_saved
    "    push rdi",                      // preserve env (caller-saved); rsp now 16-aligned
    "    call __posix_sigjmp_save_mask", // -> rax = current blocked mask (low 64)
    "    pop rdi",
    "    mov [rdi + 72], rax", // env[9] = saved mask
    "    xor eax, eax",
    "    ret",
    "2:",
    "    mov qword ptr [rdi + 64], 0", // env[8] = 0 (no mask saved)
    "    xor eax, eax",
    "    ret",
    // ---------------------------------------------------------------
    // void siglongjmp(sigjmp_buf env, int val)
    //
    // RDI = pointer to sigjmp_buf
    // ESI = return value (0 coerced to 1)
    //
    // If the buffer recorded a saved mask (env[8] != 0), restore it via
    // __posix_sigjmp_restore_mask before performing the longjmp.
    // ---------------------------------------------------------------
    ".global siglongjmp",
    ".type siglongjmp, @function",
    "siglongjmp:",
    "    cmp qword ptr [rdi + 64], 0", // mask_was_saved?
    "    je 3f",
    // Restore the saved signal mask.  Preserve env (rdi) and val (rsi)
    // across the call; the extra 8-byte pad keeps rsp 16-aligned (entry
    // rsp ≡ 8 mod 16; +24 bytes ≡ 0 mod 16 at the call).
    "    push rsi",
    "    push rdi",
    "    sub rsp, 8",
    "    mov rdi, [rdi + 72]", // arg0 = saved mask (rdi still = env)
    "    call __posix_sigjmp_restore_mask",
    "    add rsp, 8",
    "    pop rdi",
    "    pop rsi",
    "3:",
    // Standard longjmp: coerce 0 -> 1, restore registers, jump.
    "    mov eax, esi",
    "    test eax, eax",
    "    jnz 4f",
    "    inc eax",
    "4:",
    "    mov rbx, [rdi]",
    "    mov rbp, [rdi + 8]",
    "    mov r12, [rdi + 16]",
    "    mov r13, [rdi + 24]",
    "    mov r14, [rdi + 32]",
    "    mov r15, [rdi + 40]",
    "    mov rsp, [rdi + 48]",
    "    jmp [rdi + 56]",
    // ---------------------------------------------------------------
    // _setjmp / _longjmp — BSD aliases (no signal mask saving)
    // ---------------------------------------------------------------
    ".global _setjmp",
    ".type _setjmp, @function",
    "_setjmp:",
    "    jmp setjmp",
    ".global _longjmp",
    ".type _longjmp, @function",
    "_longjmp:",
    "    jmp longjmp",
);

/// `sigjmp_buf` — same underlying type as `jmp_buf`.
///
/// Both are sized to hold the register context *and* the `sigsetjmp`
/// mask-save fields, so the same storage can back either API (matching
/// glibc, where `jmp_buf` and `sigjmp_buf` are identical).
pub type SigjmpBuf = JmpBuf;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jmp_buf_size_is_128_bytes() {
        // 16 × u64: 8 register slots + sigsetjmp mask fields + padding.
        assert_eq!(
            core::mem::size_of::<JmpBuf>(),
            128,
            "jmp_buf must be 128 bytes (16 × u64)"
        );
    }

    #[test]
    fn sigjmp_mask_fields_are_in_bounds() {
        // The assembly hardcodes these byte offsets; verify they land
        // within the buffer and on the expected slots.
        assert_eq!(SIGJMP_MASK_FLAG_OFF, 64);
        assert_eq!(SIGJMP_SAVED_MASK_OFF, 72);
        assert!(SIGJMP_SAVED_MASK_OFF + 8 <= core::mem::size_of::<JmpBuf>());
    }

    #[test]
    fn jmp_buf_alignment_is_u64() {
        assert_eq!(
            core::mem::align_of::<JmpBuf>(),
            core::mem::align_of::<u64>(),
            "jmp_buf alignment must match u64"
        );
    }

    #[test]
    fn sigjmp_buf_is_same_as_jmp_buf() {
        assert_eq!(
            core::mem::size_of::<SigjmpBuf>(),
            core::mem::size_of::<JmpBuf>(),
            "SigjmpBuf must be same size as JmpBuf"
        );
        assert_eq!(
            core::mem::align_of::<SigjmpBuf>(),
            core::mem::align_of::<JmpBuf>(),
            "SigjmpBuf must have same alignment as JmpBuf"
        );
    }

    #[test]
    fn jmp_buf_holds_16_slots() {
        // 8 register slots + mask flag + saved mask + padding.
        let buf: JmpBuf = [0u64; 16];
        assert_eq!(buf.len(), 16);
    }

    #[test]
    fn jmp_buf_initializes_to_zero() {
        let buf: JmpBuf = [0u64; 16];
        for &val in &buf {
            assert_eq!(val, 0);
        }
    }
}
