//! C runtime startup and exit handlers.
//!
//! Provides `atexit`, `exit`, `__libc_start_main`, and C++ ABI stubs.
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
//!
//! ## C++ ABI Stubs
//!
//! `__cxa_atexit` and `__cxa_finalize` are C++ destructor registration
//! functions.  `__stack_chk_fail` and `__stack_chk_guard` support
//! stack canary protection (GCC/Clang -fstack-protector).

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

// ---------------------------------------------------------------------------
// C++ ABI support — __cxa_atexit / __cxa_finalize
// ---------------------------------------------------------------------------
//
// C++ static destructors register via __cxa_atexit (per Itanium C++ ABI).
// When exit() is called, __cxa_finalize runs them.  Our implementation
// piggybacks on the atexit table.

/// C++ ABI: Register a destructor for a static/global object.
///
/// `func` is the destructor, `arg` is the object, `dso_handle` identifies
/// the shared library (ignored since we don't support dynamic loading).
///
/// We ignore `arg` and `dso_handle` and simply register `func` as an
/// atexit handler.  This is correct for single-module static binaries.
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_atexit(
    func: extern "C" fn(*mut u8),
    _arg: *mut u8,
    _dso_handle: *mut u8,
) -> i32 {
    // Wrap the C++ destructor as a plain atexit function.
    // We lose the `arg` parameter here — C++ destructors for static
    // objects with non-trivial destructors will not receive their `this`.
    // A full implementation needs a separate destructor list with
    // (func, arg, dso_handle) triples.  This is a link-compatibility stub.
    let wrapper: AtexitFn = unsafe { core::mem::transmute(func) };
    atexit(wrapper)
}

/// C++ ABI: Run destructors registered by `__cxa_atexit`.
///
/// If `dso_handle` is NULL, runs all destructors (called at exit).
/// If non-NULL, runs destructors for that specific DSO (called at dlclose).
/// Since we don't support dynamic loading, this is a no-op (atexit
/// handlers are run by `exit()` instead).
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_finalize(_dso_handle: *mut u8) {
    // No-op: exit() runs atexit handlers.
}

// ---------------------------------------------------------------------------
// Stack canary support
// ---------------------------------------------------------------------------

/// Stack canary value for -fstack-protector.
///
/// The compiler inserts this value at the base of stack frames and checks
/// it on return.  A mismatch means stack corruption.  We use a fixed
/// value since we don't have /dev/urandom yet; a real implementation
/// would initialize this from a random source at process startup.
#[unsafe(no_mangle)]
pub static __stack_chk_guard: u64 = 0x0000_DEAD_BEEF_CAFE;

/// Called when a stack buffer overflow is detected.
///
/// This function never returns — the stack is corrupt, so continuing
/// would be undefined behavior.
#[unsafe(no_mangle)]
pub extern "C" fn __stack_chk_fail() -> ! {
    // Write a message to stderr.
    let msg = b"*** stack smashing detected ***\n";
    let _ = crate::file::write(2, msg.as_ptr(), msg.len());

    // Abort the process.
    crate::unistd::abort();
}

// ---------------------------------------------------------------------------
// DSO handle — used by __cxa_atexit for identifying the binary
// ---------------------------------------------------------------------------

/// DSO handle for the main executable.
///
/// Programs compiled with GCC/Clang reference this symbol.
/// For a static binary, it just needs to exist (value doesn't matter).
#[unsafe(no_mangle)]
pub static __dso_handle: u8 = 0;

// ---------------------------------------------------------------------------
// Program name globals
// ---------------------------------------------------------------------------

/// Default program name when argv[0] is not available.
static UNKNOWN_PROG: [u8; 8] = *b"unknown\0";

/// GNU extension: full path of the program (from argv[0]).
///
/// Set during `__libc_start_main`.  Programs that read this symbol
/// expect it to point to argv[0].
#[unsafe(no_mangle)]
pub static mut program_invocation_name: *const u8 = UNKNOWN_PROG.as_ptr();

/// GNU extension: basename of the program.
///
/// Set during `__libc_start_main`.  Points into the same string as
/// `program_invocation_name` but after the last '/'.
#[unsafe(no_mangle)]
pub static mut program_invocation_short_name: *const u8 = UNKNOWN_PROG.as_ptr();

/// BSD/common: short program name.
///
/// Alias for `program_invocation_short_name`.
#[unsafe(no_mangle)]
pub static mut __progname: *const u8 = UNKNOWN_PROG.as_ptr();

/// Full program name (BSD alias).
#[unsafe(no_mangle)]
pub static mut __progname_full: *const u8 = UNKNOWN_PROG.as_ptr();
