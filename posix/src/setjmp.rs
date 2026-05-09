//! Non-local jumps (`setjmp` / `longjmp`).
//!
//! Implements the `<setjmp.h>` interface for x86_64.  `setjmp` saves the
//! callee-saved registers and return address into a `jmp_buf`, and
//! `longjmp` restores them to perform a non-local jump.
//!
//! Also provides `sigsetjmp`/`siglongjmp` (aliased to `setjmp`/`longjmp`
//! since our OS doesn't deliver signals and the signal mask doesn't
//! need saving/restoring) and `_setjmp`/`_longjmp` (BSD aliases).
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
//! ```

use core::arch::global_asm;

/// Jump buffer: 8 x u64 = 64 bytes.
///
/// Stores the callee-saved registers and return address.
pub type JmpBuf = [u64; 8];

// The actual implementations must be in assembly because setjmp
// needs to capture the caller's registers — a Rust function would
// have already clobbered them.
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
    "    mov [rdi],      rbx",        // env[0] = RBX
    "    mov [rdi + 8],  rbp",        // env[1] = RBP
    "    mov [rdi + 16], r12",        // env[2] = R12
    "    mov [rdi + 24], r13",        // env[3] = R13
    "    mov [rdi + 32], r14",        // env[4] = R14
    "    mov [rdi + 40], r15",        // env[5] = R15
    // Save RSP after return (current RSP + 8 to skip return address).
    "    lea rax, [rsp + 8]",
    "    mov [rdi + 48], rax",        // env[6] = RSP (after setjmp returns)
    // Save return address (top of stack).
    "    mov rax, [rsp]",
    "    mov [rdi + 56], rax",        // env[7] = RIP
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
    "    mov rbx, [rdi]",             // RBX = env[0]
    "    mov rbp, [rdi + 8]",         // RBP = env[1]
    "    mov r12, [rdi + 16]",        // R12 = env[2]
    "    mov r13, [rdi + 24]",        // R13 = env[3]
    "    mov r14, [rdi + 32]",        // R14 = env[4]
    "    mov r15, [rdi + 40]",        // R15 = env[5]
    // Restore stack pointer and jump to saved return address.
    "    mov rsp, [rdi + 48]",        // RSP = env[6]
    "    jmp [rdi + 56]",             // JMP to env[7] (RIP)

    // ---------------------------------------------------------------
    // sigsetjmp / siglongjmp — aliases for setjmp / longjmp
    //
    // Our OS doesn't deliver Unix signals, so there is no signal mask
    // to save/restore.  sigsetjmp(env, savemask) ignores savemask and
    // behaves identically to setjmp.
    // ---------------------------------------------------------------
    ".global sigsetjmp",
    ".type sigsetjmp, @function",
    "sigsetjmp:",
    "    jmp setjmp",

    ".global siglongjmp",
    ".type siglongjmp, @function",
    "siglongjmp:",
    "    jmp longjmp",

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

/// `sigjmp_buf` — same as `jmp_buf` since we don't save signal masks.
pub type SigjmpBuf = JmpBuf;
