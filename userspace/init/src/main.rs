//! Userspace init process — the first process spawned by the kernel.
//!
//! This is PID 1 in our OS.  It runs in ring 3 and communicates with
//! the kernel exclusively through the SYSCALL instruction.
//!
//! ## Current Functionality
//!
//! - Prints a welcome banner via `SYS_CONSOLE_WRITE`.
//! - Runs a minimal read-eval-print loop (kernel shell replacement).
//! - Built-in commands: `help`, `echo`, `exit`.
//!
//! ## Syscall ABI
//!
//! ```text
//! RAX = syscall number
//! RDI = arg0, RSI = arg1, RDX = arg2, R10 = arg3, R8 = arg4, R9 = arg5
//! Return value in RAX (negative = error).
//! ```
//!
//! ## Build
//!
//! ```sh
//! cd userspace/init
//! cargo build --release
//! ```
//!
//! The resulting ELF at `target/x86_64-unknown-none/release/init` is
//! embedded in the kernel via `include_bytes!()`.

#![no_std]
#![no_main]

// ---------------------------------------------------------------------------
// Syscall numbers (must match kernel/src/syscall/number.rs)
// ---------------------------------------------------------------------------

const SYS_EXIT: u64 = 1;
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_CONSOLE_READ_CHAR: u64 = 101;

// ---------------------------------------------------------------------------
// Syscall wrappers
// ---------------------------------------------------------------------------

/// Issue a syscall with 0 arguments.
#[allow(dead_code)]  // Will be used for SYS_YIELD and similar.
#[inline(always)]
fn syscall0(nr: u64) -> i64 {
    let ret: i64;
    // SAFETY: We trust the kernel to handle the syscall correctly.
    // The SYSCALL instruction saves RIP in RCX and RFLAGS in R11,
    // and clobbers both.  We declare them as clobbers.
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            lateout("rax") ret,
            lateout("rcx") _,  // Clobbered by SYSCALL (saves RIP).
            lateout("r11") _,  // Clobbered by SYSCALL (saves RFLAGS).
            options(nostack),
        );
    }
    ret
}

/// Issue a syscall with 1 argument.
#[inline(always)]
fn syscall1(nr: u64, arg0: u64) -> i64 {
    let ret: i64;
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

/// Issue a syscall with 2 arguments.
#[inline(always)]
fn syscall2(nr: u64, arg0: u64, arg1: u64) -> i64 {
    let ret: i64;
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

// ---------------------------------------------------------------------------
// High-level syscall API
// ---------------------------------------------------------------------------

/// Write a byte slice to the console.
fn console_write(msg: &[u8]) -> i64 {
    syscall2(
        SYS_CONSOLE_WRITE,
        msg.as_ptr() as u64,
        msg.len() as u64,
    )
}

/// Write a string to the console.
fn print(s: &str) {
    console_write(s.as_bytes());
}

/// Read one character from the keyboard (blocking).
/// Returns the ASCII byte, or 0 for non-printable keys.
fn read_char() -> u8 {
    let mut ch: u8 = 0;
    syscall1(SYS_CONSOLE_READ_CHAR, &mut ch as *mut u8 as u64);
    ch
}

/// Exit the process with the given exit code.
fn exit(code: i64) -> ! {
    syscall1(SYS_EXIT, code as u64);
    // The kernel should never return from SYS_EXIT, but just in case:
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

// ---------------------------------------------------------------------------
// Shell implementation
// ---------------------------------------------------------------------------

/// Maximum command line length.
const MAX_LINE: usize = 256;

/// Read a line from the keyboard, echoing characters.
/// Returns the number of valid bytes in `buf`.
fn read_line(buf: &mut [u8; MAX_LINE]) -> usize {
    let mut pos: usize = 0;

    loop {
        let ch = read_char();

        match ch {
            // Enter — end of line.
            b'\r' | b'\n' => {
                print("\n");
                return pos;
            }

            // Backspace / DEL.
            0x08 | 0x7F => {
                if pos > 0 {
                    pos -= 1;
                    // Erase character on screen: backspace, space, backspace.
                    console_write(b"\x08 \x08");
                }
            }

            // Printable ASCII.
            0x20..=0x7E => {
                if pos < MAX_LINE {
                    buf[pos] = ch;
                    pos += 1;
                    // Echo the character.
                    console_write(&[ch]);
                }
            }

            // Ignore non-printable keys (function keys, arrows, etc.).
            _ => {}
        }
    }
}

/// Compare two byte slices for equality.
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Check if `haystack` starts with `needle`.
fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    bytes_eq(&haystack[..needle.len()], needle)
}

/// Execute a command.
fn execute(line: &[u8]) {
    // Trim leading/trailing whitespace.
    let trimmed = trim(line);
    if trimmed.is_empty() {
        return;
    }

    if bytes_eq(trimmed, b"help") {
        print("Available commands:\n");
        print("  help    - show this message\n");
        print("  echo .. - print text\n");
        print("  exit    - shut down\n");
    } else if bytes_eq(trimmed, b"exit") {
        print("Goodbye.\n");
        exit(0);
    } else if starts_with(trimmed, b"echo ") {
        // Print everything after "echo ".
        let rest = &trimmed[5..];
        console_write(rest);
        print("\n");
    } else if bytes_eq(trimmed, b"echo") {
        print("\n");
    } else {
        print("Unknown command: ");
        console_write(trimmed);
        print("\nType 'help' for available commands.\n");
    }
}

/// Trim leading and trailing ASCII whitespace from a byte slice.
fn trim(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\t') {
        start += 1;
    }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t') {
        end -= 1;
    }
    &s[start..end]
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Process entry point.  Called by the kernel via IRETQ to ring 3.
///
/// No Rust runtime is available — no heap, no std, no arguments.
/// We communicate with the kernel exclusively via SYSCALL.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // Welcome banner.
    print("\n");
    print("=======================================\n");
    print("  Welcome to the OS!\n");
    print("  Userspace init process (PID 1)\n");
    print("=======================================\n");
    print("\n");
    print("Type 'help' for available commands.\n\n");

    // Main shell loop.
    let mut line_buf = [0u8; MAX_LINE];

    loop {
        print("user> ");
        let len = read_line(&mut line_buf);
        if len > 0 {
            execute(&line_buf[..len]);
        }
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

/// Panic handler — print message to console and exit with error.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // We can't use format! (no allocator), so just print a static message.
    let _ = info;
    print("!!! PANIC in init process !!!\n");
    exit(-1);
}
