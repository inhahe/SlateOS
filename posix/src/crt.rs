//! C runtime startup and exit handlers.
//!
//! Provides `atexit`, `at_quick_exit`, `exit`, `quick_exit`,
//! `__libc_start_main`, and C++ ABI stubs.
//!
//! ## C Program Startup
//!
//! When a C program starts, the kernel sets up the stack with argc/argv
//! and jumps to `_start`.  `_start` (in crt0) calls `__libc_start_main`
//! which initializes the C runtime and calls `main`.  When `main`
//! returns, `exit(main_retval)` is called, which runs `atexit` handlers
//! and then calls `_exit`.
//!
//! ## atexit / at_quick_exit
//!
//! Registered functions are called in reverse order (LIFO) during
//! `exit()` or `quick_exit()` respectively.  Maximum 32 handlers each.
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

/// Registered at_quick_exit handlers (C11), in registration order.
static mut QUICKEXIT_FUNCS: [Option<AtexitFn>; MAX_ATEXIT] = [None; MAX_ATEXIT];
/// Number of registered quick-exit handlers.
static mut QUICKEXIT_COUNT: usize = 0;

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

    // POSIX: flush all open output streams before termination.
    // This ensures buffered printf/fputs output is not lost.
    crate::stdio::fflush(core::ptr::null_mut());

    #[allow(clippy::used_underscore_items)]
    crate::process::_exit(status);
}

/// C11: Register a function to be called by `quick_exit`.
///
/// Unlike `atexit`, these handlers are only called by `quick_exit`,
/// not by normal `exit`.  Returns 0 on success, -1 if full.
#[unsafe(no_mangle)]
pub extern "C" fn at_quick_exit(func: AtexitFn) -> i32 {
    // SAFETY: Single-threaded access.
    let count = unsafe { addr_of_mut!(QUICKEXIT_COUNT).read() };
    if count >= MAX_ATEXIT {
        return -1;
    }

    unsafe {
        let funcs = addr_of_mut!(QUICKEXIT_FUNCS);
        if let Some(slot) = (*funcs).get_mut(count) {
            *slot = Some(func);
        }
        addr_of_mut!(QUICKEXIT_COUNT).write(count.wrapping_add(1));
    }
    0
}

/// C11: Terminate the process, running `at_quick_exit` handlers.
///
/// Unlike `exit`, does NOT call `atexit` handlers or flush stdio.
/// Calls handlers registered with `at_quick_exit` in LIFO order,
/// then calls `_Exit`.
#[unsafe(no_mangle)]
pub extern "C" fn quick_exit(status: i32) -> ! {
    // Run at_quick_exit handlers in LIFO order.
    let count = unsafe { addr_of_mut!(QUICKEXIT_COUNT).read() };

    let mut i = count;
    while i > 0 {
        i = i.wrapping_sub(1);
        let func = unsafe {
            let funcs = addr_of_mut!(QUICKEXIT_FUNCS);
            (*funcs).get(i).copied().flatten()
        };
        if let Some(f) = func {
            f();
        }
    }

    unsafe { addr_of_mut!(QUICKEXIT_COUNT).write(0); }

    // quick_exit calls _Exit (not exit), skipping atexit handlers.
    #[allow(clippy::used_underscore_items, non_snake_case)]
    crate::process::_Exit(status);
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

    // Set program name from argv[0] if available.
    // err/warn/errx/warnx use __progname for the "prog: msg" prefix.
    if arg_count > 0 && !arg_vec.is_null() {
        // SAFETY: arg_count > 0 guarantees argv[0] exists.
        let argv0 = unsafe { *arg_vec };
        if !argv0.is_null() {
            unsafe {
                addr_of_mut!(program_invocation_name).write(argv0);
                addr_of_mut!(__progname_full).write(argv0);
            }
            // Find basename (after last '/').
            let mut last_slash: *const u8 = core::ptr::null();
            let mut scan = argv0;
            unsafe {
                while *scan != 0 {
                    if *scan == b'/' {
                        last_slash = scan;
                    }
                    scan = scan.add(1);
                }
            }
            let short = if last_slash.is_null() {
                argv0
            } else {
                unsafe { last_slash.add(1) }
            };
            unsafe {
                addr_of_mut!(program_invocation_short_name).write(short);
                addr_of_mut!(__progname).write(short);
            }
        }
    }

    // Ensure `environ` points at a valid (empty) null-terminated array.
    // POSIX requires environ to be non-NULL so programs can safely
    // iterate it without checking for NULL first.
    crate::environ::init_environ();

    // Call main.
    let ret = main(arg_count, arg_vec, unsafe { crate::environ::environ.cast() });

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

// ---------------------------------------------------------------------------
// GCC initialization/finalization stubs
// ---------------------------------------------------------------------------

/// GCC CRT: global constructor initialization.
///
/// Older GCC-compiled programs reference this symbol for running
/// constructors in `.init_array`.  Modern toolchains use
/// `.init_array` entries directly, but the symbol must exist for
/// link compatibility.
#[unsafe(no_mangle)]
pub extern "C" fn __libc_csu_init() {
    // No-op: we don't have a .init_array processing loop yet.
    // Static constructors (if any) would be called here.
}

/// GCC CRT: global destructor finalization.
///
/// Called during exit to run `.fini_array` destructors.  Modern
/// toolchains use `__cxa_finalize` instead, but the symbol must
/// exist for link compatibility.
#[unsafe(no_mangle)]
pub extern "C" fn __libc_csu_fini() {
    // No-op: destructors are handled by atexit/__cxa_finalize.
}

// ---------------------------------------------------------------------------
// C++ thread-local destructor support
// ---------------------------------------------------------------------------

/// C++ ABI: Register a thread-local destructor.
///
/// Called by the C++ runtime for objects with `thread_local` storage
/// duration that have non-trivial destructors.  Since we don't support
/// thread-local storage cleanup yet, we ignore the registration.
/// The destructor will leak (not be called at thread exit).
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_thread_atexit_impl(
    _dtor: extern "C" fn(*mut u8),
    _obj: *mut u8,
    _dso_handle: *mut u8,
) -> i32 {
    // Stub: accept registration but never call the destructor.
    // When thread-local storage is fully supported, these destructors
    // should run at thread exit in reverse registration order.
    0
}

// ---------------------------------------------------------------------------
// glibc atfork registration
// ---------------------------------------------------------------------------

/// glibc internal: Register fork handlers.
///
/// Some glibc-linked code calls this directly instead of
/// `pthread_atfork`.  We delegate to our existing stub.
#[unsafe(no_mangle)]
pub extern "C" fn __register_atfork(
    prepare: Option<extern "C" fn()>,
    parent: Option<extern "C" fn()>,
    child: Option<extern "C" fn()>,
    _dso_handle: *mut u8,
) -> i32 {
    crate::pthread::pthread_atfork(prepare, parent, child)
}

// ---------------------------------------------------------------------------
// glibc identification
// ---------------------------------------------------------------------------

/// Version string for our POSIX compatibility layer.
///
/// Programs compiled with glibc headers may call `gnu_get_libc_version()`
/// to check the C library version.  We return a plausible version string.
static LIBC_VERSION: [u8; 5] = *b"2.38\0";

/// Release string.
static LIBC_RELEASE: [u8; 8] = *b"stable\0\0";

/// Return the "glibc version" string.
///
/// We're not actually glibc, but programs that check this at runtime
/// (rather than link time) need a non-null result.
#[unsafe(no_mangle)]
pub extern "C" fn gnu_get_libc_version() -> *const u8 {
    LIBC_VERSION.as_ptr()
}

/// Return the "glibc release" string.
#[unsafe(no_mangle)]
pub extern "C" fn gnu_get_libc_release() -> *const u8 {
    LIBC_RELEASE.as_ptr()
}

// ---------------------------------------------------------------------------
// glibc thread safety flag
// ---------------------------------------------------------------------------

/// glibc 2.32+: Flag indicating the process is single-threaded.
///
/// glibc uses this to optimize mutex operations (skip atomic ops when
/// single-threaded).  Value: 1 = single-threaded, 0 = multi-threaded.
/// We start as single-threaded; `pthread_create` should set this to 0.
#[unsafe(no_mangle)]
pub static mut __libc_single_threaded: u8 = 1;

// ---------------------------------------------------------------------------
// Auxiliary vector access
// ---------------------------------------------------------------------------

/// Query the auxiliary vector (glibc extension).
///
/// The auxiliary vector is a mechanism for the kernel to pass
/// information to userspace at process startup (page size, UID, etc.).
/// Since our kernel doesn't populate an auxv yet, we return sensible
/// defaults for known types and 0 for everything else.
#[unsafe(no_mangle)]
pub extern "C" fn getauxval(typ: u64) -> u64 {
    // Common AT_* values (from Linux headers).
    const AT_PAGESZ: u64 = 6;
    const AT_CLKTCK: u64 = 17;
    const AT_SECURE: u64 = 23;
    const AT_HWCAP: u64 = 16;
    const AT_HWCAP2: u64 = 26;
    const AT_UID: u64 = 11;
    const AT_EUID: u64 = 12;
    const AT_GID: u64 = 13;
    const AT_EGID: u64 = 14;

    match typ {
        AT_PAGESZ => 16384, // Our 16 KiB page size.
        AT_CLKTCK => 100,   // Jiffy rate (HZ).
        AT_SECURE => 0,     // Not running in secure mode.
        AT_HWCAP | AT_HWCAP2 => 0, // No hardware capability flags.
        AT_UID | AT_EUID => 0, // Root.
        AT_GID | AT_EGID => 0, // Root group.
        _ => {
            crate::errno::set_errno(crate::errno::ENOENT);
            0
        }
    }
}

// ---------------------------------------------------------------------------
// __environ — glibc alias for environ
// ---------------------------------------------------------------------------

/// glibc internal name for the environment pointer.
///
/// Some programs reference `__environ` directly instead of `environ`.
/// Must point to the same location as `crate::environ::environ`.
// NOTE: This is a separate static that should ideally alias
// `crate::environ::environ`, but Rust doesn't support symbol aliasing.
// Programs that reference __environ will get this (initially null)
// pointer.  `init_environ` in environ.rs sets the real `environ`.
// For programs that need __environ, they should use `environ` instead.
// This exists purely for link compatibility.
#[unsafe(no_mangle)]
pub static mut __environ: *mut *const u8 = core::ptr::null_mut();

// ---------------------------------------------------------------------------
// C++ ABI support — pure virtual calls
// ---------------------------------------------------------------------------

/// C++ ABI: Called when a pure virtual function is invoked.
///
/// This should never happen in correct code.  It means a base class
/// constructor called a pure virtual method, or a dangling vtable
/// reference was followed.
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_pure_virtual() -> ! {
    let msg = b"pure virtual method called\n";
    let _ = crate::file::write(2, msg.as_ptr(), msg.len());
    crate::unistd::abort();
}

// ---------------------------------------------------------------------------
// C++ ABI — static initialization guards
// ---------------------------------------------------------------------------
//
// C++ static local variables with non-trivial constructors need
// thread-safe one-time initialization.  The compiler emits a guard
// variable and calls __cxa_guard_acquire before initialization,
// __cxa_guard_release after success, and __cxa_guard_abort on
// exception.
//
// Guard layout (Itanium C++ ABI):
//   byte 0: 0 = uninitialized, 1 = initialized
//   bytes 1-7: reserved (used for futex on some platforms)
//
// Since we're single-threaded (for now), these are simple flag checks.

/// Acquire the initialization guard.
///
/// Returns 1 if the caller should perform initialization (guard was
/// uninitialized), or 0 if initialization already completed.
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_guard_acquire(guard: *mut u64) -> i32 {
    if guard.is_null() {
        return 0;
    }
    // SAFETY: guard points to a compiler-generated static.
    let byte0 = guard.cast::<u8>();
    let val = unsafe { *byte0 };
    if val != 0 {
        0 // Already initialized.
    } else {
        1 // Caller should initialize.
    }
}

/// Release the initialization guard (mark as initialized).
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_guard_release(guard: *mut u64) {
    if guard.is_null() {
        return;
    }
    // SAFETY: guard points to a compiler-generated static.
    let byte0 = guard.cast::<u8>();
    unsafe { *byte0 = 1; }
}

/// Abort initialization (exception during construction).
///
/// Resets the guard so a future attempt can retry.
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_guard_abort(guard: *mut u64) {
    if guard.is_null() {
        return;
    }
    // SAFETY: guard points to a compiler-generated static.
    let byte0 = guard.cast::<u8>();
    unsafe { *byte0 = 0; }
}

// ---------------------------------------------------------------------------
// C++ exception handling stubs
// ---------------------------------------------------------------------------
//
// We don't support C++ exceptions, but these symbols must exist for
// link compatibility with C++ code compiled with exceptions enabled.
// All exception-throwing paths will abort.

/// C++ ABI: Allocate memory for an exception object.
///
/// Stub: always returns a pointer to a static buffer (only one
/// exception can be in-flight at a time, but since we abort on throw
/// this doesn't matter).
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_allocate_exception(_thrown_size: usize) -> *mut u8 {
    static mut EXCEPTION_BUF: [u8; 128] = [0; 128];
    // SAFETY: Single-threaded; exceptions abort anyway.
    core::ptr::addr_of_mut!(EXCEPTION_BUF).cast::<u8>()
}

/// C++ ABI: Throw an exception.
///
/// Stub: aborts the process.  We don't support exception unwinding.
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_throw(
    _thrown_exception: *mut u8,
    _tinfo: *mut u8,
    _dest: Option<extern "C" fn(*mut u8)>,
) -> ! {
    let msg = b"C++ exception thrown (not supported)\n";
    let _ = crate::file::write(2, msg.as_ptr(), msg.len());
    crate::unistd::abort();
}

/// C++ ABI: Begin catching an exception.
///
/// Stub: returns the exception object pointer (or null).
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_begin_catch(exception_object: *mut u8) -> *mut u8 {
    exception_object
}

/// C++ ABI: End catching an exception.
///
/// Stub: no-op.
#[unsafe(no_mangle)]
pub extern "C" fn __cxa_end_catch() {}

/// GCC C++ personality routine for exception handling.
///
/// Stub: always returns `_URC_FATAL_PHASE1_ERROR` (8) to indicate
/// we can't handle exceptions.
#[unsafe(no_mangle)]
pub extern "C" fn __gxx_personality_v0() -> i32 {
    8 // _URC_FATAL_PHASE1_ERROR
}

/// Unwind library: Resume exception propagation.
///
/// Stub: aborts.  We don't support stack unwinding.
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Resume(_exception_object: *mut u8) -> ! {
    let msg = b"_Unwind_Resume called (not supported)\n";
    let _ = crate::file::write(2, msg.as_ptr(), msg.len());
    crate::unistd::abort();
}

// ---------------------------------------------------------------------------
// __stack_chk_fail_local — local stack canary check
// ---------------------------------------------------------------------------

/// Local variant of `__stack_chk_fail`.
///
/// GCC's `-fstack-protector-strong` may emit calls to this symbol
/// instead of `__stack_chk_fail` for functions with local visibility.
/// Same behavior: stack is corrupt, abort immediately.
#[unsafe(no_mangle)]
pub extern "C" fn __stack_chk_fail_local() -> ! {
    __stack_chk_fail()
}
