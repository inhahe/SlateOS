//! POSIX compatibility library for the OS.
//!
//! Provides C-compatible function signatures (`extern "C"`) for standard
//! POSIX operations, backed by our native syscall interface.  Userspace
//! programs written in C (via our cross-toolchain) or Rust can link
//! against this library to get familiar POSIX semantics.
//!
//! ## Design
//!
//! This is a thin translation layer, not a full libc.  It maps POSIX
//! function signatures to our native syscalls with minimal overhead:
//!
//! - **File I/O**: `open`, `close`, `read`, `write`, `lseek`, `stat`,
//!   `fstat`, `unlink`, `mkdir`, `rmdir`, `rename`, `dup`, `dup2`
//! - **Process**: `_exit`, `getpid`, `getppid`, `fork` (via spawn),
//!   `execve`, `waitpid`, `sleep`, `nanosleep`
//! - **Memory**: `mmap`, `munmap`, `mprotect`
//! - **Signals**: Translated to native IPC messages (partial)
//! - **Misc**: `getcwd`, `chdir`, `errno` thread-local
//!
//! ## Error Handling
//!
//! POSIX functions return -1 on error and set `errno`.  Our native
//! syscalls return negative error codes.  The translation layer converts
//! native error codes to POSIX errno values.
//!
//! ## What This Is NOT
//!
//! This is not a complete C runtime (no `printf`, `malloc`, `stdio`).
//! Those higher-level facilities will be built on top of these primitives
//! or provided by porting musl/newlib.
//!
//! ## References
//!
//! - POSIX.1-2024 (IEEE Std 1003.1-2024)
//! - Linux man pages (for practical POSIX semantics)
//! - Redox relibc (Rust POSIX libc for a custom OS)
//! - musl libc (minimal Linux libc, good reference for what to implement)

#![no_std]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::missing_safety_doc,       // extern "C" functions are inherently unsafe
    clippy::not_unsafe_ptr_arg_deref, // POSIX functions take raw pointers by design
    clippy::inline_always,            // syscall wrappers must be inlined
    clippy::wildcard_imports,         // syscall constant imports
    clippy::doc_markdown,             // POSIX identifiers (O_CREAT, x86_64) used extensively in docs
    clippy::large_stack_arrays,       // Dir pool is intentionally large (~544 KiB)
)]

// Panic handler for no_std staticlib.
// When linked into a binary that provides its own panic handler,
// the linker will use the binary's version.  This is a fallback.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        // SAFETY: hlt is a safe instruction in any privilege level.
        unsafe { core::arch::asm!("hlt", options(nostack, nomem)); }
    }
}

pub mod errno;
pub mod fcntl;
pub mod fcntl_ops;
pub mod fdtable;
pub mod file;
pub mod mman;
pub mod pipe;
pub mod process;
pub mod spawn;
pub mod stat;
pub mod stdlib;
pub mod string;
pub mod syscall;
pub mod time;
pub mod types;
pub mod unistd;
pub mod dirent;
