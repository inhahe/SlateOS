//! C runtime startup and exit handlers.
//!
//! Provides `atexit`, `exit`, and `__libc_start_main`.
//!
//! ## C Program Startup
//!
//! When a C program starts, the kernel sets up the stack with argc/argv
//! and jumps to `_start`.  `_start` (in crt0) calls `__libc_start_main`
//! which initializes the C runtime and calls `main`.  When `main`
//! returns, `exit(main_retval)` is called, which runs `atexit` handlers
//! and then calls `_exit`.
//!
//! ## atexit
//!
//! Registered functions are called in reverse order (LIFO) during
//! `exit()`.  Maximum 32 handlers.

use core::arch::global_asm;
use core::ptr::addr_of_mut;

/// Maximum number of atexit handlers.
const MAX_ATEXIT: usize = 32;

/// atexit handler function pointer type.
type AtexitFn = extern "C" fn();

/// Registered atexit handlers, in registration order.
static mut ATEXIT_FUNCS: [Option<AtexitFn>; MAX_ATEXIT] = [None; MAX_ATEXIT];
/// Number of registered handlers.
static mut ATEXIT_COUNT: usize = 0;

/// Register a function to be called at normal process termination.
///
/// Returns 0 on success, -1 if the atexit table is full.
#[unsafe(no_mangle)]
pub extern "C" fn atexit(func: AtexitFn) -> i32 {
    // SAFETY: Single-threaded access.
    let count = unsafe { addr_of_mut!(ATEXIT_COUNT).read() };
    if count >= MAX_ATEXIT {
        return -1;
    }

    // SAFETY: count < MAX_ATEXIT, so index is valid.
    // SAFETY: count < MAX_ATEXIT verified above.
    unsafe {
        let funcs = addr_of_mut!(ATEXIT_FUNCS);
        if let Some(slot) = (*funcs).get_mut(count) {
            *slot = Some(func);
        }
        addr_of_mut!(ATEXIT_COUNT).write(count.wrapping_add(1));
    }
    0
}

/// Terminate the process, running atexit handlers first.
///
/// Calls registered atexit functions in reverse order, then
/// calls `_exit(status)`.
#[unsafe(no_mangle)]
pub extern "C" fn exit(status: i32) -> ! {
    // Run atexit handlers in LIFO order.
    // SAFETY: Single-threaded access.
    let count = unsafe { addr_of_mut!(ATEXIT_COUNT).read() };

    let mut i = count;
    while i > 0 {
        i = i.wrapping_sub(1);
        // SAFETY: i < count <= MAX_ATEXIT.
        // SAFETY: i < count <= MAX_ATEXIT, so index is valid.
        let func = unsafe {
            let funcs = addr_of_mut!(ATEXIT_FUNCS);
            (*funcs).get(i).copied().flatten()
        };
        if let Some(f) = func {
            f();
        }
    }

    // Reset count (in case an atexit handler calls exit again).
    unsafe { addr_of_mut!(ATEXIT_COUNT).write(0); }

    #[allow(clippy::used_underscore_items)]
    crate::process::_exit(status);
}

/// C runtime entry point (glibc convention).
///
/// Called by `_start` (crt0) with:
/// - `main`: pointer to the program's main function
/// - `argc`: argument count
/// - `argv`: argument vector
/// - `envp`: environment pointer vector
///
/// Initializes the environment, calls `main`, then `exit`.
///
/// # Safety
///
/// All pointer arguments must be valid per the C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __libc_start_main(
    main: extern "C" fn(i32, *const *const u8, *const *const u8) -> i32,
    arg_count: i32,
    arg_vec: *const *const u8,
    _init: usize,  // Unused (glibc compat).
    _fini: usize,  // Unused (glibc compat).
    _rtld_fini: usize, // Unused (glibc compat).
    _stack_end: *mut u8,
) -> ! {
    // Initialize the fd table (stdin/stdout/stderr).
    // The fd table is statically initialized, so this is a no-op,
    // but explicit initialization would go here.

    // Set up the environ pointer.
    // If envp is provided by the kernel, we could populate ENV_STORE.
    // For now, environ is empty.

    // Call main.
    let ret = main(arg_count, arg_vec, core::ptr::null());

    // Exit with main's return value.
    exit(ret);
}

// ---------------------------------------------------------------------------
// _start — the ELF entry point (crt0)
// ---------------------------------------------------------------------------

// The kernel jumps here with no arguments on the stack (argc/argv not
// yet supported).  When the kernel adds argument passing, this stub
// will extract them from the stack per the SysV x86_64 ABI:
//
//   [rsp]       = argc
//   [rsp+8]     = argv[0]
//   ...
//   [rsp+8*argc+8] = NULL  (argv terminator)
//   envp follows argv
//
// For now we call main(0, NULL, NULL) since the kernel doesn't provide
// arguments.  The `weak` linkage lets programs provide their own _start
// if they prefer raw entry (like the current hello/ticker programs).

global_asm!(
    // ---------------------------------------------------------------
    // void _start(void)  — process entry point
    //
    // Called by the kernel via IRETQ with no arguments.
    // Calls main(0, NULL, NULL), then exit(retval).
    // ---------------------------------------------------------------
    ".weak _start",
    ".type _start, @function",
    "_start:",
    // Align stack to 16 bytes (should already be, but be safe).
    "    and rsp, -16",
    // Clear frame pointer for backtraces.
    "    xor ebp, ebp",
    // Call main(argc=0, argv=NULL, envp=NULL).
    "    xor edi, edi",          // argc = 0
    "    xor esi, esi",          // argv = NULL
    "    xor edx, edx",          // envp = NULL
    "    call main",
    // main returned in EAX — pass to exit.
    "    mov edi, eax",
    "    call exit",
    // exit should not return, but if it does, halt.
    "    ud2",
);
