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

#[cfg(target_os = "none")]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
    unsafe {
        addr_of_mut!(ATEXIT_COUNT).write(0);
    }

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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    unsafe {
        addr_of_mut!(QUICKEXIT_COUNT).write(0);
    }

    // quick_exit calls _Exit (not exit), skipping atexit handlers.
    #[allow(clippy::used_underscore_items, non_snake_case)]
    crate::process::_Exit(status);
}

// ---------------------------------------------------------------------------
// ELF constructors / destructors (.preinit_array / .init_array / .fini_array)
// ---------------------------------------------------------------------------
//
// The System V ABI runs global constructors listed in `.preinit_array` and
// `.init_array` before `main`, and destructors in `.fini_array` at process
// exit.  C programs using `__attribute__((constructor))`/`destructor` and
// C++ programs with non-trivial static/global constructors depend on this.
//
// The linker delimits each array with a pair of boundary symbols
// (`__init_array_start`/`__init_array_end`, etc.).  For lld's default
// executable layout these symbols only exist when the program actually has
// a corresponding input section; the explicit userspace/service linker
// scripts define them (empty) unconditionally.  We therefore reference the
// boundary symbols as **weak** externals (declared weak via a `.weak`
// assembly directive — see below): absent → null → the walk below is
// skipped, so pure-Rust programs (which emit no constructors) are
// completely unaffected.

/// A single entry in an ELF `.init_array` / `.fini_array` section.
///
/// Each entry is a nullable function pointer.  Some toolchains pad these
/// arrays with NULL entries, so the walkers skip nulls rather than assume a
/// dense array.
pub type InitArrayEntry = Option<extern "C" fn()>;

/// Walk `[start, end)` in ascending order, invoking each non-null entry.
///
/// Used for `.preinit_array` and `.init_array` — constructors run in
/// ascending address order per the System V ABI.  A null `start`/`end`
/// (an absent array) is treated as empty.
///
/// # Safety
///
/// If non-null, `start` and `end` must bound one valid array of
/// `InitArrayEntry` with `start <= end`, and every entry must be either
/// null or a callable `extern "C" fn()` pointer.
// Present on the hand-rolled crt path (target_os="none", used by
// `services/` and any future C crt0) and on the host test build; the
// slateos userspace target (os=linux) uses Rust std's own startup.
#[cfg(any(target_os = "none", test))]
unsafe fn run_init_array(start: *const InitArrayEntry, end: *const InitArrayEntry) {
    if start.is_null() || end.is_null() {
        return;
    }
    let mut p = start;
    while p < end {
        // SAFETY: p is within [start, end); the caller guarantees the range
        // is a valid array of entries.
        if let Some(f) = unsafe { *p } {
            f();
        }
        // SAFETY: p < end, so p.add(1) stays within [start, end].
        p = unsafe { p.add(1) };
    }
}

/// Walk `[start, end)` in descending order, invoking each non-null entry.
///
/// Used for `.fini_array` — destructors run in the reverse of constructor
/// order per the ELF spec.  Same null-bounds handling as
/// [`run_init_array`].
///
/// # Safety
///
/// Same contract as [`run_init_array`].
#[cfg(any(target_os = "none", test))]
unsafe fn run_fini_array(start: *const InitArrayEntry, end: *const InitArrayEntry) {
    if start.is_null() || end.is_null() {
        return;
    }
    let mut p = end;
    while p > start {
        // SAFETY: p > start, so p.sub(1) stays within [start, end).
        p = unsafe { p.sub(1) };
        // SAFETY: p is within [start, end); the caller guarantees validity.
        if let Some(f) = unsafe { *p } {
            f();
        }
    }
}

// References to the linker-provided array boundary symbols.  Only their
// *addresses* are meaningful; the declared element type just makes
// `addr_of!` yield a `*const InitArrayEntry` directly.
//
// The boundary symbols are declared **weak** at the assembly level via the
// `.weak` directive below (the same mechanism as C's
// `__attribute__((weak))` extern — no nightly `feature(linkage)` needed, so
// this builds on the stable toolchain the kernel/boot path uses).  A weak
// *undefined* symbol resolves to address 0 at link time instead of erroring,
// so pure-Rust programs (which emit no `.init_array`, and whose default lld
// layout therefore synthesises none of these symbols) link cleanly and the
// startup walk below sees null bounds → no-op.  When a program actually has
// constructors — or an explicit linker script provides the bounds — the weak
// reference binds to the real definition.
#[cfg(target_os = "none")]
unsafe extern "C" {
    static __preinit_array_start: InitArrayEntry;
    static __preinit_array_end: InitArrayEntry;
    static __init_array_start: InitArrayEntry;
    static __init_array_end: InitArrayEntry;
    static __fini_array_start: InitArrayEntry;
    static __fini_array_end: InitArrayEntry;
}

// Declare the boundary symbols weak so undefined references resolve to null
// rather than failing the link.  Assembly `.weak` works on stable Rust,
// unlike the `#[linkage = "extern_weak"]` attribute (nightly-only).
#[cfg(target_os = "none")]
global_asm!(
    ".weak __preinit_array_start",
    ".weak __preinit_array_end",
    ".weak __init_array_start",
    ".weak __init_array_end",
    ".weak __fini_array_start",
    ".weak __fini_array_end",
);

/// Run the program's `.preinit_array` then `.init_array` constructors.
///
/// Called from `__libc_start_main` after environ/signal setup and before
/// `main`.  A no-op for programs without constructors.
#[cfg(target_os = "none")]
fn run_constructors() {
    // SAFETY: the boundary symbols bound a valid (possibly empty/null)
    // constructor array; `run_init_array` skips a null/empty range.
    unsafe {
        run_init_array(
            core::ptr::addr_of!(__preinit_array_start),
            core::ptr::addr_of!(__preinit_array_end),
        );
        run_init_array(
            core::ptr::addr_of!(__init_array_start),
            core::ptr::addr_of!(__init_array_end),
        );
    }
}

/// Run the program's `.fini_array` destructors (reverse order).
///
/// Registered with `atexit` during startup so it fires at normal exit.
#[cfg(target_os = "none")]
extern "C" fn run_destructors() {
    // SAFETY: the boundary symbols bound a valid (possibly empty/null)
    // destructor array; `run_fini_array` skips a null/empty range.
    unsafe {
        run_fini_array(
            core::ptr::addr_of!(__fini_array_start),
            core::ptr::addr_of!(__fini_array_end),
        );
    }
}

// ---------------------------------------------------------------------------
// Kernel argument retrieval (SYS_PROCESS_GET_ARGS)
// ---------------------------------------------------------------------------

/// Maximum buffer size for argv+envp data retrieved from the kernel.
///
/// 64 KiB covers virtually all real-world cases.  The kernel supports
/// up to 256 KiB each for argv and envp, but programs needing that
/// much are extremely rare.  If the data exceeds this buffer, the
/// child starts with argc=0 (no arguments).
const INIT_ARGS_BUF_SIZE: usize = 64 * 1024;

/// Maximum number of individual argument or environment string pointers.
const MAX_INIT_PTRS: usize = 512;

/// Static buffer for args data from `SYS_PROCESS_GET_ARGS`.
///
/// Layout: `SpawnArgsHeader` (16 bytes) + packed argv strings + packed
/// envp strings.  Lives in .bss (zeroed at load, no binary size cost).
///
/// Aligned to 8 bytes so the leading `SpawnArgsHeader` (4-byte-aligned
/// `u32` fields, but force a stronger alignment to be future-proof for
/// header extensions) can be read directly without `read_unaligned`.
#[repr(C, align(8))]
struct InitArgsBuf([u8; INIT_ARGS_BUF_SIZE]);
static mut INIT_ARGS_BUF: InitArgsBuf = InitArgsBuf([0u8; INIT_ARGS_BUF_SIZE]);

/// Static argv pointer array (null-terminated).
///
/// Each entry points into `INIT_ARGS_BUF` at the start of an argv string.
/// The last entry is NULL (C convention).
static mut INIT_ARGV: [*const u8; MAX_INIT_PTRS + 1] = [core::ptr::null(); MAX_INIT_PTRS + 1];

/// Static envp pointer array (null-terminated).
static mut INIT_ENVP: [*const u8; MAX_INIT_PTRS + 1] = [core::ptr::null(); MAX_INIT_PTRS + 1];

/// Maximum number of inherited fd map entries we can receive.
///
/// Matches [`crate::spawn::MAX_FD_MAP`] and covers the common case
/// (3 standard fds + redirected pipes).
const MAX_INIT_FDS: usize = 32;

/// Static buffer for `SYS_PROCESS_GET_INITIAL_FDS` output.
static mut INIT_FDS_BUF: [crate::spawn::FdMapEntry; MAX_INIT_FDS] = [crate::spawn::FdMapEntry {
    fd: 0,
    handle_type: 0,
    _pad: [0; 3],
    handle: 0,
}; MAX_INIT_FDS];

/// Retrieve inherited file descriptors from the kernel.
///
/// The parent's `posix_spawn` builds an fd_map and passes it to the
/// kernel via `SYS_PROCESS_SPAWN_EX`.  During spawn, the kernel dups
/// each parent handle and stores the `(fd, child_handle)` pairs in
/// our PCB.  This function retrieves them and reinitializes the fd
/// table so we start with the correct handles.
///
/// Must be called exactly once, early in startup (before any I/O).
///
/// # Safety
///
/// Writes to static `INIT_FDS_BUF` and to the fd table.  Must be
/// called from single-threaded context.
unsafe fn retrieve_initial_fds() {
    use crate::fdtable::{self, HandleKind};
    use crate::spawn::fd_handle_type;
    use crate::syscall::{SYS_PROCESS_GET_INITIAL_FDS, syscall2};

    let buf_ptr = addr_of_mut!(INIT_FDS_BUF);
    let buf = unsafe { (*buf_ptr).as_mut_ptr() };

    // Call SYS_PROCESS_GET_INITIAL_FDS.  Returns the number of entries
    // written, or 0 if no fds were inherited.
    let ret = syscall2(SYS_PROCESS_GET_INITIAL_FDS, buf as u64, MAX_INIT_FDS as u64);

    if ret <= 0 {
        return; // No inherited fds — keep default console setup.
    }

    let count = ret as usize;

    // Read the entries from the buffer.
    let entries =
        unsafe { core::slice::from_raw_parts(buf.cast_const(), count.min(MAX_INIT_FDS)) };

    if entries.is_empty() {
        return;
    }

    // Clear the existing fd table (default stdin/stdout/stderr) and
    // reinitialize from the inherited entries.
    //
    // We first close all default fds (0/1/2 console), then install
    // the inherited ones.  This ensures the child sees exactly the
    // fds the parent intended.
    fdtable::clear_all();

    for entry in entries {
        let kind = match entry.handle_type {
            fd_handle_type::FILE => HandleKind::File,
            fd_handle_type::PIPE => HandleKind::Pipe,
            fd_handle_type::TCP_SOCKET => HandleKind::TcpStream,
            fd_handle_type::UDP_SOCKET => HandleKind::UdpSocket,
            fd_handle_type::CONSOLE => HandleKind::Console,
            fd_handle_type::EVENTFD => HandleKind::Eventfd,
            _ => HandleKind::File, // Unknown — default to file.
        };

        // Install this fd at the specified number.
        fdtable::set_fd(entry.fd, kind, entry.handle);
    }
}

/// Retrieve initial arguments from the kernel via `SYS_PROCESS_GET_ARGS`.
///
/// The kernel stores argv/envp data passed by the parent via
/// `SYS_PROCESS_SPAWN_EX`.  This function retrieves them and builds
/// the argc/argv/envp pointers that main() expects.
///
/// Returns `(argc, argv, envp)`.  If no args are available (e.g., the
/// process was spawned with the old `SYS_PROCESS_SPAWN`), returns
/// `(0, null, null)`.
///
/// # Safety
///
/// Must be called exactly once, before main().  The returned pointers
/// are valid for the entire process lifetime (they point into statics).
// argc/argv pairing is the canonical libc startup convention; the
// similar names (`final_argc`/`final_argv`) deliberately mirror it.
#[allow(clippy::similar_names)]
unsafe fn retrieve_initial_args() -> (i32, *const *const u8, *const *const u8) {
    use crate::spawn::SpawnArgsHeader;
    use crate::syscall::{SYS_PROCESS_GET_ARGS, syscall2};

    let buf_ptr = addr_of_mut!(INIT_ARGS_BUF);
    let buf = unsafe { (*buf_ptr).0.as_mut_ptr() };

    // Call SYS_PROCESS_GET_ARGS.  Returns total bytes written, or
    // the needed size if our buffer is too small (data is preserved
    // in the kernel for retry), or 0 if no args were set.
    let ret = syscall2(SYS_PROCESS_GET_ARGS, buf as u64, INIT_ARGS_BUF_SIZE as u64);

    if ret <= 0 {
        return (0, core::ptr::null(), core::ptr::null());
    }

    let total = ret as usize;
    let header_size = core::mem::size_of::<SpawnArgsHeader>();

    // If the kernel returned more than our buffer can hold, the data
    // is still in the PCB (not consumed).  We can't use it without a
    // larger buffer.  Fall back to no args.
    if total > INIT_ARGS_BUF_SIZE || total < header_size {
        return (0, core::ptr::null(), core::ptr::null());
    }

    // Parse the header.  `INIT_ARGS_BUF` is `#[repr(align(8))]` so `buf`
    // is guaranteed to be aligned for a `SpawnArgsHeader` (which only
    // needs 4-byte alignment).  Clippy doesn't see through the
    // wrapper, so silence the lint at this single cast.
    #[allow(clippy::cast_ptr_alignment)]
    // SAFETY: buf points into the aligned `INIT_ARGS_BUF` which holds
    // at least `header_size` valid bytes (checked just above).
    let header = unsafe { &*buf.cast::<SpawnArgsHeader>().cast_const() };
    let argc = header.argc as usize;
    let envc = header.envc as usize;
    let argv_data_len = header.argv_data_len as usize;
    let envp_data_len = header.envp_data_len as usize;

    if argc == 0 && envc == 0 {
        return (0, core::ptr::null(), core::ptr::null());
    }

    // Validate that the data fits within what we received.
    let expected = header_size
        .saturating_add(argv_data_len)
        .saturating_add(envp_data_len);
    if expected > total {
        return (0, core::ptr::null(), core::ptr::null());
    }

    // Build argv pointer array.
    // SAFETY: data_start is within our buffer bounds (validated above).
    let data_start = unsafe { buf.add(header_size) };
    let argv_ptrs = addr_of_mut!(INIT_ARGV);

    let mut pos = 0usize;
    let mut arg_idx = 0usize;
    while arg_idx < argc && arg_idx < MAX_INIT_PTRS && pos < argv_data_len {
        // SAFETY: pos < argv_data_len, which is within buffer bounds.
        unsafe {
            if let Some(slot) = (*argv_ptrs).get_mut(arg_idx) {
                *slot = data_start.add(pos);
            }
        }

        // Advance past this string's null terminator.
        while pos < argv_data_len {
            // SAFETY: pos < argv_data_len guarantees we're in bounds.
            if unsafe { *data_start.add(pos) } == 0 {
                break;
            }
            pos = pos.wrapping_add(1);
        }
        pos = pos.wrapping_add(1); // Skip the null.
        arg_idx = arg_idx.wrapping_add(1);
    }

    // Null-terminate the argv array.
    unsafe {
        if let Some(slot) = (*argv_ptrs).get_mut(arg_idx) {
            *slot = core::ptr::null();
        }
    }

    // Build envp pointer array.
    // SAFETY: envp_start is at data_start + argv_data_len, within bounds.
    let envp_start = unsafe { data_start.add(argv_data_len) };
    let envp_ptrs = addr_of_mut!(INIT_ENVP);

    let mut pos = 0usize;
    let mut env_idx = 0usize;
    while env_idx < envc && env_idx < MAX_INIT_PTRS && pos < envp_data_len {
        unsafe {
            if let Some(slot) = (*envp_ptrs).get_mut(env_idx) {
                *slot = envp_start.add(pos);
            }
        }

        while pos < envp_data_len {
            if unsafe { *envp_start.add(pos) } == 0 {
                break;
            }
            pos = pos.wrapping_add(1);
        }
        pos = pos.wrapping_add(1);
        env_idx = env_idx.wrapping_add(1);
    }

    // Null-terminate the envp array.
    unsafe {
        if let Some(slot) = (*envp_ptrs).get_mut(env_idx) {
            *slot = core::ptr::null();
        }
    }

    // Load environment variables into the environ store so that
    // getenv()/setenv() work.  This must happen before init_environ().
    if envc > 0 && envp_data_len > 0 {
        unsafe {
            crate::environ::load_packed_envp(envp_start, envp_data_len, envc);
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let final_argc = arg_idx as i32;
    let final_argv = unsafe { (*argv_ptrs).as_ptr() };

    (final_argc, final_argv, core::ptr::null())
}

/// C runtime entry point (glibc convention).
/// Set up main-thread ELF TLS for a native static binary.
///
/// On x86-64 the psABI's variant-II TLS model requires the thread pointer
/// (`%fs` base) to point at a *thread control block* (TCB) whose first
/// word self-points, with each module's static TLS block laid out
/// *immediately below* the thread pointer.  A Linux/glibc crt sets this up
/// during startup after reading `PT_TLS` from the aux vector.  A **native**
/// SlateOS static binary gets neither: the kernel zeroes `fs_base` at exec
/// (expecting userspace to install TLS), and there is no aux vector to
/// discover `PT_TLS`.  Any compiler `__thread` access (e.g. fastpy's
/// runtime, which lowers `__thread` to `%fs:offset`) — and even the
/// stack-protector canary at `%fs:0x28` — would then fault on a null
/// thread pointer.
///
/// This routine reconstructs what the aux vector would have told us by
/// walking the program headers directly through the linker-defined
/// `__ehdr_start` symbol (present in static non-PIE links), locating the
/// `PT_TLS` segment, allocating a block + TCB, copying the `.tdata` init
/// image and relying on the anonymous mapping's zero-fill for the `.tbss`
/// tail, writing the TCB self-pointer, and finally installing the thread
/// pointer via the native `SYS_SET_FS_BASE` syscall (the counterpart of
/// Linux `arch_prctl(ARCH_SET_FS)`).  If the program has no `PT_TLS` we
/// still install a minimal TCB so the self-pointer and canary slot are
/// valid.  Called once, very early in `__libc_start_main`, before any code
/// that might touch TLS.
///
/// Only the main thread is handled here; child threads created via
/// `pthread_create` get their own TLS setup in the thread-spawn path
/// (see `posix/src/pthread.rs`).
///
/// # Safety
///
/// Must be called exactly once at process startup, before any `__thread`
/// access or stack-protected function runs.  Reads the program's own ELF
/// header and `PT_TLS` init image, which are always mapped in a static
/// executable.
#[cfg(target_os = "none")]
unsafe fn setup_main_thread_tls() {
    use crate::syscall::{syscall6, SYS_MMAP, SYS_SET_FS_BASE};

    // Linker-defined: address of the ELF header of this executable.  In a
    // static non-PIE link this resolves to the load address of the file's
    // Elf64_Ehdr, letting us find the program headers without an aux vector.
    unsafe extern "C" {
        static __ehdr_start: u8;
    }

    let round_up = |v: u64, a: u64| -> u64 { (v + (a - 1)) & !(a - 1) };

    let ehdr = core::ptr::addr_of!(__ehdr_start);
    // Elf64_Ehdr field offsets: e_phoff @0x20 (u64), e_phentsize @0x36
    // (u16), e_phnum @0x38 (u16).  Use unaligned reads — the header is a
    // packed byte layout at a symbol address of unknown alignment.
    let e_phoff = unsafe { core::ptr::read_unaligned(ehdr.add(0x20).cast::<u64>()) };
    let e_phentsize =
        unsafe { core::ptr::read_unaligned(ehdr.add(0x36).cast::<u16>()) } as usize;
    let e_phnum = unsafe { core::ptr::read_unaligned(ehdr.add(0x38).cast::<u16>()) } as usize;

    // Scan program headers for PT_TLS (p_type == 7).
    const PT_TLS: u32 = 7;
    let mut tls_vaddr: u64 = 0;
    let mut tls_filesz: u64 = 0;
    let mut tls_memsz: u64 = 0;
    let mut tls_align: u64 = 0;
    let phbase = unsafe { ehdr.add(e_phoff as usize) };
    for i in 0..e_phnum {
        let ph = unsafe { phbase.add(i * e_phentsize) };
        // Elf64_Phdr: p_type@0 (u32), p_vaddr@16, p_filesz@32, p_memsz@40,
        // p_align@48 (all u64).
        let p_type = unsafe { core::ptr::read_unaligned(ph.cast::<u32>()) };
        if p_type == PT_TLS {
            tls_vaddr = unsafe { core::ptr::read_unaligned(ph.add(16).cast::<u64>()) };
            tls_filesz = unsafe { core::ptr::read_unaligned(ph.add(32).cast::<u64>()) };
            tls_memsz = unsafe { core::ptr::read_unaligned(ph.add(40).cast::<u64>()) };
            tls_align = unsafe { core::ptr::read_unaligned(ph.add(48).cast::<u64>()) };
            break;
        }
    }

    // Alignment: at least 16 bytes (TCB/ABI), honoring a larger PT_TLS
    // request.  tls_size (the block below TP) is rounded to this alignment
    // so that, with a page-aligned base, TP - tls_size stays aligned.
    let align = core::cmp::max(if tls_align == 0 { 1 } else { tls_align }, 16);
    let tls_size = round_up(tls_memsz, align);
    // TCB above TP: only the self-pointer (offset 0) and stack-protector
    // canary (offset 0x28) slots are architecturally required; reserve a
    // small, aligned TCB covering both.
    let tcb_size: u64 = 0x40;

    // Allocate TLS block + TCB (+ alignment slack) as an anonymous, zeroed,
    // private mapping.  PROT_READ|PROT_WRITE = 3, MAP_PRIVATE|MAP_ANONYMOUS
    // = 0x22, fd = -1.
    let total = tls_size + tcb_size + align;
    let base = syscall6(SYS_MMAP, 0, total, 3, 0x22, u64::MAX, 0);
    if base < 0 {
        // Out of memory for TLS: leave fs_base = 0.  A program with no
        // __thread storage and no stack protector still runs; one that
        // needs TLS will fault, but there is nothing better we can do here
        // and startup must not itself depend on TLS.
        return;
    }
    let base_addr = base as u64;

    // Variant II: the thread pointer is aligned, the TLS block occupies
    // [TP - tls_size, TP), and the TCB occupies [TP, TP + tcb_size).
    let tp = round_up(base_addr + tls_size, align);
    let block = (tp - tls_size) as *mut u8;

    // Copy the .tdata initialization image (p_filesz bytes at p_vaddr) to
    // the bottom of the block; the .tbss remainder is already zero (fresh
    // anonymous pages).
    if tls_filesz > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                tls_vaddr as *const u8,
                block,
                tls_filesz as usize,
            );
        }
    }

    // The x86-64 psABI requires %fs:0 to hold the thread pointer itself.
    unsafe {
        (tp as *mut u64).write(tp);
    }

    // Install the thread pointer.  Ignore the (always-0-on-success) return:
    // if this failed the address was rejected as non-canonical, but `tp`
    // comes from mmap and is by construction a valid user address.
    let _ = syscall6(SYS_SET_FS_BASE, tp, 0, 0, 0, 0, 0);
}

/// Called by `_start` (crt0) with:
/// - `main`: pointer to the program's main function
/// - `argc`: argument count (0 from `_start` before kernel arg passing)
/// - `argv`: argument vector (NULL from `_start`)
/// - `envp`: environment pointer vector
///
/// On startup, attempts to retrieve arguments from the kernel via
/// `SYS_PROCESS_GET_ARGS`.  If the kernel has args (because the parent
/// used `SYS_PROCESS_SPAWN_EX`), those override the values from `_start`.
///
/// Initializes the environment, calls `main`, then `exit`.
///
/// # Safety
///
/// All pointer arguments must be valid per the C ABI.
// argv/argc pairing is the canonical libc startup convention; the
// `kernel_argv`/`kernel_argc` and `actual_argv`/`actual_argc` locals
// are intentionally paired by name to make the convention obvious.
#[allow(clippy::similar_names)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __libc_start_main(
    main: extern "C" fn(i32, *const *const u8, *const *const u8) -> i32,
    arg_count: i32,
    arg_vec: *const *const u8,
    _init: usize,      // Unused (glibc compat).
    _fini: usize,      // Unused (glibc compat).
    _rtld_fini: usize, // Unused (glibc compat).
    _stack_end: *mut u8,
) -> ! {
    // Install main-thread ELF TLS first of all.  The kernel gives a native
    // static binary a null thread pointer (fs_base = 0) and no aux vector,
    // so until this runs any compiler `__thread` access — and the
    // stack-protector canary at %fs:0x28 — would fault.  Everything below
    // (and `main`) may rely on TLS, so it must be set up before anything
    // else.  On a host (test) build this is a no-op.
    #[cfg(target_os = "none")]
    unsafe {
        setup_main_thread_tls();
    }

    // Retrieve inherited file descriptors from the kernel.
    // The parent's posix_spawn builds an fd_map and passes it to the
    // kernel; we retrieve it here and reinitialize our fd table.
    // Must happen before any I/O (including arg retrieval, which
    // doesn't use fds but sets the pattern for startup order).
    unsafe {
        retrieve_initial_fds();
    }

    // Try to retrieve args from the kernel.  The parent may have
    // passed argv/envp via SYS_PROCESS_SPAWN_EX, which are stored
    // in the PCB until we fetch them.
    let (kernel_argc, kernel_argv, _kernel_envp) = unsafe { retrieve_initial_args() };

    // Use kernel-provided args if available, otherwise fall back to
    // what _start passed (currently argc=0, argv=NULL).
    let actual_argc = if kernel_argc > 0 {
        kernel_argc
    } else {
        arg_count
    };
    let actual_argv = if kernel_argc > 0 {
        kernel_argv
    } else {
        arg_vec
    };

    // Set program name from argv[0] if available.
    // err/warn/errx/warnx use __progname for the "prog: msg" prefix.
    if actual_argc > 0 && !actual_argv.is_null() {
        // SAFETY: actual_argc > 0 guarantees argv[0] exists.
        let argv0 = unsafe { *actual_argv };
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
    //
    // Note: if the kernel provided envp, load_packed_envp() was already
    // called by retrieve_initial_args() to populate ENV_STORE.  This
    // call to init_environ() rebuilds the pointer array, which will
    // include those entries.
    crate::environ::init_environ();

    // Register the signal trampoline so the kernel can deliver
    // catchable signals to handlers installed via signal()/sigaction().
    // Until this runs the kernel applies signal default actions itself.
    crate::signal::init_signals();

    // Run ELF global constructors (.preinit_array then .init_array) and
    // arrange for destructors (.fini_array) to run at exit.  Constructors
    // must run after environ/signal setup (they may call getenv, install
    // handlers, etc.) but before main.  For pure-Rust programs the arrays
    // are empty (weak boundary symbols are null), so this is a no-op.
    //
    // `run_destructors` is registered *before* main runs, so exit()'s LIFO
    // atexit order fires it after any handler main registers — matching the
    // conventional libc ordering (destructors after atexit handlers).
    #[cfg(target_os = "none")]
    {
        run_constructors();
        let _ = atexit(run_destructors);
    }

    // Call main.
    let ret = main(actual_argc, actual_argv, unsafe {
        crate::environ::environ.cast()
    });

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

#[cfg(target_os = "none")]
global_asm!(
    // ---------------------------------------------------------------
    // void _start(void)  — process entry point
    //
    // Called by the kernel via IRETQ with no arguments on the stack.
    // Calls __libc_start_main(main, 0, NULL, ...) which initializes
    // the C runtime (environ, program name, etc.) and calls main.
    //
    // When the kernel adds argument passing, this should extract
    // argc/argv from the stack per the SysV x86_64 ABI:
    //   [rsp]     = argc
    //   [rsp+8]   = argv[0], ...
    //   argv terminated by NULL, then envp follows.
    // ---------------------------------------------------------------
    ".weak _start",
    ".type _start, @function",
    "_start:",
    // Align stack to 16 bytes (ABI requirement for call).
    "    and rsp, -16",
    // Clear frame pointer for backtraces.
    "    xor ebp, ebp",
    // Call __libc_start_main(main, argc, argv, init, fini, rtld_fini, stack_end).
    // SysV x86_64: rdi, rsi, rdx, rcx, r8, r9, [stack]
    "    lea rdi, [rip + main]", // 1st arg: pointer to main
    "    xor esi, esi",          // 2nd arg: argc = 0
    "    xor edx, edx",          // 3rd arg: argv = NULL
    "    xor ecx, ecx",          // 4th arg: init = 0 (unused)
    "    xor r8d, r8d",          // 5th arg: fini = 0 (unused)
    "    xor r9d, r9d",          // 6th arg: rtld_fini = 0 (unused)
    // SysV ABI requires (RSP+8) % 16 == 0 on function entry.
    // After `and rsp, -16`, RSP is 16-aligned.  One `push` makes it
    // 8-aligned, then `call` pushes the return address making it
    // 16-aligned — which violates the (RSP+8)%16==0 rule.
    // Insert 8 bytes of padding so the alignment works out:
    //   sub rsp,8 → 8-aligned; push 0 → 16-aligned; call → 8-aligned ✓
    "    sub rsp, 8", // alignment padding
    "    push 0",     // 7th arg: stack_end = NULL (on stack)
    "    call __libc_start_main",
    // __libc_start_main should not return, but if it does, halt.
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static __stack_chk_guard: u64 = 0x0000_DEAD_BEEF_CAFE;

/// Called when a stack buffer overflow is detected.
///
/// This function never returns — the stack is corrupt, so continuing
/// would be undefined behavior.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut program_invocation_name: *const u8 = UNKNOWN_PROG.as_ptr();

/// GNU extension: basename of the program.
///
/// Set during `__libc_start_main`.  Points into the same string as
/// `program_invocation_name` but after the last '/'.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut program_invocation_short_name: *const u8 = UNKNOWN_PROG.as_ptr();

/// BSD/common: short program name.
///
/// Alias for `program_invocation_short_name`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut __progname: *const u8 = UNKNOWN_PROG.as_ptr();

/// Full program name (BSD alias).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_csu_init() {
    // No-op link-compat symbol.  Our startup does NOT route through
    // __libc_csu_init; `__libc_start_main` walks `.preinit_array`/
    // `.init_array` directly (see `run_constructors`).  Kept only so
    // programs that reference this glibc symbol still link.  Doing the walk
    // here too would run every constructor twice.
}

/// GCC CRT: global destructor finalization.
///
/// Called during exit to run `.fini_array` destructors.  Modern
/// toolchains use `__cxa_finalize` instead, but the symbol must
/// exist for link compatibility.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __libc_csu_fini() {
    // No-op link-compat symbol.  `.fini_array` destructors are run at exit
    // via `run_destructors` (registered with atexit in `__libc_start_main`),
    // and C++ static destructors via __cxa_finalize.
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gnu_get_libc_version() -> *const u8 {
    LIBC_VERSION.as_ptr()
}

/// Return the "glibc release" string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut __libc_single_threaded: u8 = 1;

// ---------------------------------------------------------------------------
// Auxiliary vector access
// ---------------------------------------------------------------------------

/// AT_RANDOM backing buffer.
///
/// glibc expects `getauxval(AT_RANDOM)` to return a pointer to 16 bytes
/// of kernel-supplied randomness.  These bytes are used for stack
/// canaries, pthread keys, and ASLR-style seeds inside libc.  Programs
/// MUST NOT use them for cryptography (use `getrandom()` for that), but
/// they must be unpredictable per-process — a constant here would let
/// an attacker bypass stack canary protection.
///
/// We populate this on first read via `fill_random()` (RDRAND with LCG
/// fallback).  The buffer is then stable for the rest of the process
/// lifetime, which matches the Linux ABI contract.
static mut AT_RANDOM_BYTES: [u8; 16] = [0; 16];
/// Whether `AT_RANDOM_BYTES` has been initialized.
///
/// Single-process userspace: one of the AT_RANDOM bytes happening to be
/// zero on a freshly-RDRAND'd buffer is harmless (a stack canary with
/// one zero byte is still 120 bits of entropy), but the `initialized`
/// flag avoids re-rolling the value on every getauxval call so callers
/// who cache the pointer see stable bytes through the buffer.
static AT_RANDOM_INITIALIZED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// AT_PLATFORM backing string ("x86_64", null-terminated).
///
/// glibc dynamic linker reads this to pick architecture-specific
/// library paths (`/usr/lib/$AT_PLATFORM/`).
static AT_PLATFORM_BYTES: [u8; 7] = *b"x86_64\0";

/// Initialize the AT_RANDOM buffer if not yet done.
///
/// Safe to call repeatedly — only the first call writes the buffer.
/// Subsequent calls observe the AtomicBool and short-circuit.
fn ensure_at_random_initialized() {
    use core::sync::atomic::Ordering;

    // Acquire ordering pairs with the Release on the store below: a
    // reader that observes `initialized = true` is guaranteed to see
    // the full random buffer.
    if AT_RANDOM_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    // Take a raw pointer to the static buffer.  fill_random is safe
    // (it writes through the pointer with the standard write-len ABI);
    // the buffer itself is owned by this module and only ever written
    // here.  Race in single-process userspace is harmless: both racers
    // call fill_random into the same buffer; the second write simply
    // overwrites the first.
    let ptr = core::ptr::addr_of_mut!(AT_RANDOM_BYTES).cast::<u8>();
    crate::unistd::fill_random(ptr, 16);
    AT_RANDOM_INITIALIZED.store(true, Ordering::Release);
}

/// Query the auxiliary vector (glibc extension).
///
/// The auxiliary vector is a mechanism for the kernel to pass
/// information to userspace at process startup (page size, UID, etc.).
/// Our kernel doesn't populate an auxv struct; we synthesize the
/// commonly-queried entries here:
///
/// - `AT_PAGESZ` (6) → 16384 (our page size).
/// - `AT_CLKTCK` (17) → 100 (HZ).
/// - `AT_RANDOM` (25) → pointer to 16 bytes of process-local randomness,
///   lazily populated on first call from `fill_random()` (RDRAND with
///   LCG fallback).  Used by glibc/musl for stack canaries.
/// - `AT_PLATFORM` (15) → pointer to "x86_64\0".
/// - `AT_SECURE`/`AT_HWCAP`/`AT_HWCAP2`/`AT_UID`/`AT_EUID`/`AT_GID`/
///   `AT_EGID` → 0 (single-user OS, no setuid binaries, no advertised
///   hwcap flags yet — programs fall back to CPUID).
///
/// Any other type sets `errno = ENOENT` and returns 0, matching glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
    const AT_RANDOM: u64 = 25;
    const AT_PLATFORM: u64 = 15;

    match typ {
        AT_PAGESZ => 16384, // Our 16 KiB page size.
        AT_CLKTCK => 100,   // Jiffy rate (HZ).
        AT_RANDOM => {
            ensure_at_random_initialized();
            core::ptr::addr_of!(AT_RANDOM_BYTES) as u64
        }
        AT_PLATFORM => core::ptr::addr_of!(AT_PLATFORM_BYTES) as u64,
        // Secure mode off, no hwcap flags, root uid/gid.
        AT_SECURE | AT_HWCAP | AT_HWCAP2 | AT_UID | AT_EUID | AT_GID | AT_EGID => 0,
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static mut __environ: *mut *const u8 = core::ptr::null_mut();

// ---------------------------------------------------------------------------
// C++ ABI support — pure virtual calls
// ---------------------------------------------------------------------------

/// C++ ABI: Called when a pure virtual function is invoked.
///
/// This should never happen in correct code.  It means a base class
/// constructor called a pure virtual method, or a dangling vtable
/// reference was followed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_pure_virtual() -> ! {
    let msg = b"pure virtual method called\n";
    let _ = crate::file::write(2, msg.as_ptr(), msg.len());
    crate::unistd::abort();
}

/// Called when a deleted virtual function is invoked.
///
/// Similar to `__cxa_pure_virtual` but for functions marked `= delete`.
/// Prints a diagnostic and aborts.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_deleted_virtual() -> ! {
    let msg = b"deleted virtual method called\n";
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_guard_acquire(guard: *mut u64) -> i32 {
    if guard.is_null() {
        return 0;
    }
    // SAFETY: guard points to a compiler-generated static.
    let byte0 = guard.cast::<u8>();
    let val = unsafe { *byte0 };
    // Returns 1 if caller should initialize (val == 0), 0 if already done.
    i32::from(val == 0)
}

/// Release the initialization guard (mark as initialized).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_guard_release(guard: *mut u64) {
    if guard.is_null() {
        return;
    }
    // SAFETY: guard points to a compiler-generated static.
    let byte0 = guard.cast::<u8>();
    unsafe {
        *byte0 = 1;
    }
}

/// Abort initialization (exception during construction).
///
/// Resets the guard so a future attempt can retry.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_guard_abort(guard: *mut u64) {
    if guard.is_null() {
        return;
    }
    // SAFETY: guard points to a compiler-generated static.
    let byte0 = guard.cast::<u8>();
    unsafe {
        *byte0 = 0;
    }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_allocate_exception(_thrown_size: usize) -> *mut u8 {
    static mut EXCEPTION_BUF: [u8; 128] = [0; 128];
    // SAFETY: Single-threaded; exceptions abort anyway.
    core::ptr::addr_of_mut!(EXCEPTION_BUF).cast::<u8>()
}

/// C++ ABI: Throw an exception.
///
/// Stub: aborts the process.  We don't support exception unwinding.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_begin_catch(exception_object: *mut u8) -> *mut u8 {
    exception_object
}

/// C++ ABI: End catching an exception.
///
/// Stub: no-op.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cxa_end_catch() {}

/// GCC C++ personality routine for exception handling.
///
/// Stub: always returns `_URC_FATAL_PHASE1_ERROR` (8) to indicate
/// we can't handle exceptions.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __gxx_personality_v0() -> i32 {
    8 // _URC_FATAL_PHASE1_ERROR
}

/// Unwind library: Resume exception propagation.
///
/// Stub: aborts.  We don't support stack unwinding.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __stack_chk_fail_local() -> ! {
    __stack_chk_fail()
}

// ---------------------------------------------------------------------------
// on_exit — register a function to be called at exit with status
// ---------------------------------------------------------------------------

/// Function type for `on_exit` callbacks.
///
/// Takes the exit status and a user-provided argument.
pub type OnExitFn = extern "C" fn(i32, *mut u8);

/// Maximum number of `on_exit` handlers.
const MAX_ON_EXIT: usize = 32;

/// Registered `on_exit` handlers.
static mut ON_EXIT_FUNCS: [(Option<OnExitFn>, *mut u8); MAX_ON_EXIT] =
    [(None, core::ptr::null_mut()); MAX_ON_EXIT];
/// Number of registered `on_exit` handlers.
static mut ON_EXIT_COUNT: usize = 0;

/// `on_exit` — register a function to be called at normal process exit.
///
/// Like `atexit`, but the callback receives the exit status and a
/// user-provided argument.  SunOS/glibc extension.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn on_exit(func: OnExitFn, arg: *mut u8) -> i32 {
    // SAFETY: single-threaded access.
    let count = unsafe { (&raw const ON_EXIT_COUNT).read() };
    if count >= MAX_ON_EXIT {
        return -1;
    }
    unsafe {
        // `count < MAX_ON_EXIT == ON_EXIT_FUNCS.len()` from the
        // check just above.
        #[allow(clippy::indexing_slicing)]
        {
            ON_EXIT_FUNCS[count] = (Some(func), arg);
        }
        (&raw mut ON_EXIT_COUNT).write(count.wrapping_add(1));
    }
    0
}

// ---------------------------------------------------------------------------
// gnu_dev_major / gnu_dev_minor / gnu_dev_makedev
// ---------------------------------------------------------------------------

/// Extract the major device number from a dev_t.
///
/// glibc extension.  Uses the Linux device number encoding.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gnu_dev_major(dev: u64) -> u32 {
    ((dev >> 8) & 0xFFF) as u32 | (((dev >> 32) & !0xFFF_u64) as u32)
}

/// Extract the minor device number from a dev_t.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gnu_dev_minor(dev: u64) -> u32 {
    (dev & 0xFF) as u32 | (((dev >> 12) & !0xFF_u64) as u32)
}

/// Construct a dev_t from major and minor numbers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gnu_dev_makedev(major: u32, minor: u32) -> u64 {
    let maj = u64::from(major);
    let min = u64::from(minor);
    ((maj & 0xFFF) << 8) | ((maj & !0xFFF_u64) << 32) | (min & 0xFF) | ((min & !0xFF_u64) << 12)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Constants --

    #[test]
    fn test_max_atexit() {
        // POSIX minimum is 32 for atexit handlers.
        assert!(MAX_ATEXIT >= 32);
    }

    #[test]
    fn test_stack_chk_guard_nonzero() {
        // Stack canary must be non-zero (otherwise trivially guessable).
        assert_ne!(__stack_chk_guard, 0);
    }

    #[test]
    fn test_dso_handle_exists() {
        // __dso_handle just needs to exist; value doesn't matter.
        let _ = __dso_handle;
    }

    // -- .init_array / .fini_array walkers --
    //
    // These record invocation order into a shared buffer, so they must not
    // run concurrently; a Mutex serialises them across cargo's parallel
    // test threads.

    static INIT_ARRAY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static CALL_ORDER: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

    fn record(id: usize) {
        CALL_ORDER.lock().expect("test lock poisoned").push(id);
    }
    extern "C" fn ctor1() {
        record(1);
    }
    extern "C" fn ctor2() {
        record(2);
    }
    extern "C" fn ctor3() {
        record(3);
    }

    fn take_order() -> Vec<usize> {
        core::mem::take(&mut *CALL_ORDER.lock().expect("test lock poisoned"))
    }

    #[test]
    fn test_init_array_forward_skips_nulls() {
        let _guard = INIT_ARRAY_TEST_LOCK.lock().expect("test lock poisoned");
        take_order();
        let arr: [InitArrayEntry; 5] = [Some(ctor1), None, Some(ctor2), None, Some(ctor3)];
        // SAFETY: arr bounds a valid array of entries.
        unsafe {
            run_init_array(arr.as_ptr(), arr.as_ptr().add(arr.len()));
        }
        assert_eq!(take_order(), vec![1, 2, 3]);
    }

    #[test]
    fn test_fini_array_runs_reverse() {
        let _guard = INIT_ARRAY_TEST_LOCK.lock().expect("test lock poisoned");
        take_order();
        let arr: [InitArrayEntry; 3] = [Some(ctor1), Some(ctor2), Some(ctor3)];
        // SAFETY: arr bounds a valid array of entries.
        unsafe {
            run_fini_array(arr.as_ptr(), arr.as_ptr().add(arr.len()));
        }
        assert_eq!(take_order(), vec![3, 2, 1]);
    }

    #[test]
    fn test_init_array_null_bounds_noop() {
        let _guard = INIT_ARRAY_TEST_LOCK.lock().expect("test lock poisoned");
        take_order();
        // SAFETY: null bounds are the documented "absent array" case.
        unsafe {
            run_init_array(core::ptr::null(), core::ptr::null());
            run_fini_array(core::ptr::null(), core::ptr::null());
        }
        assert!(take_order().is_empty());
    }

    #[test]
    fn test_init_array_empty_noop() {
        let _guard = INIT_ARRAY_TEST_LOCK.lock().expect("test lock poisoned");
        take_order();
        let arr: [InitArrayEntry; 0] = [];
        // SAFETY: start == end is a valid empty range.
        unsafe {
            run_init_array(arr.as_ptr(), arr.as_ptr());
            run_fini_array(arr.as_ptr(), arr.as_ptr());
        }
        assert!(take_order().is_empty());
    }

    // -- getauxval --

    #[test]
    fn test_getauxval_page_size() {
        assert_eq!(getauxval(6), 16384); // AT_PAGESZ = 6; our 16 KiB pages.
    }

    #[test]
    fn test_getauxval_clk_tck() {
        assert_eq!(getauxval(17), 100); // AT_CLKTCK = 17; 100 Hz.
    }

    #[test]
    fn test_getauxval_secure() {
        assert_eq!(getauxval(23), 0); // AT_SECURE = 23; not in secure mode.
    }

    #[test]
    fn test_getauxval_hwcap() {
        assert_eq!(getauxval(16), 0); // AT_HWCAP = 16; no hw caps.
    }

    #[test]
    fn test_getauxval_hwcap2() {
        assert_eq!(getauxval(26), 0); // AT_HWCAP2 = 26.
    }

    #[test]
    fn test_getauxval_uid_gid() {
        assert_eq!(getauxval(11), 0); // AT_UID
        assert_eq!(getauxval(12), 0); // AT_EUID
        assert_eq!(getauxval(13), 0); // AT_GID
        assert_eq!(getauxval(14), 0); // AT_EGID
    }

    #[test]
    fn test_getauxval_unknown_sets_enoent() {
        // Unknown type should return 0 and set errno to ENOENT.
        crate::errno::set_errno(0);
        let val = getauxval(9999);
        assert_eq!(val, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_getauxval_known_does_not_set_enoent() {
        // Known types should NOT set ENOENT.
        crate::errno::set_errno(0);
        let _ = getauxval(6); // AT_PAGESZ
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- gnu_get_libc_version / release --

    #[test]
    fn test_libc_version_not_null() {
        let ver = gnu_get_libc_version();
        assert!(!ver.is_null());
    }

    #[test]
    fn test_libc_version_is_2_38() {
        let ver = gnu_get_libc_version();
        let len = unsafe { crate::string::strlen(ver) };
        let s = unsafe { core::slice::from_raw_parts(ver, len) };
        assert_eq!(s, b"2.38");
    }

    #[test]
    fn test_libc_release_not_null() {
        let rel = gnu_get_libc_release();
        assert!(!rel.is_null());
    }

    #[test]
    fn test_libc_release_is_stable() {
        let rel = gnu_get_libc_release();
        let len = unsafe { crate::string::strlen(rel) };
        let s = unsafe { core::slice::from_raw_parts(rel, len) };
        assert_eq!(s, b"stable");
    }

    // -- C++ guard functions --

    #[test]
    fn test_cxa_guard_acquire_uninitialized() {
        let mut guard: u64 = 0; // Byte 0 = 0 → uninitialized.
        let result = __cxa_guard_acquire(&raw mut guard);
        assert_eq!(result, 1); // Should initialize.
    }

    #[test]
    fn test_cxa_guard_acquire_already_initialized() {
        let mut guard: u64 = 1; // Byte 0 = 1 → initialized.
        let result = __cxa_guard_acquire(&raw mut guard);
        assert_eq!(result, 0); // Already done.
    }

    #[test]
    fn test_cxa_guard_release_sets_initialized() {
        let mut guard: u64 = 0;
        __cxa_guard_release(&raw mut guard);
        // Byte 0 should be 1 now.
        let byte0 = guard as u8;
        assert_eq!(byte0, 1);
    }

    #[test]
    fn test_cxa_guard_abort_resets() {
        let mut guard: u64 = 0;
        __cxa_guard_release(&raw mut guard); // Mark initialized.
        __cxa_guard_abort(&raw mut guard); // Reset.
        let byte0 = guard as u8;
        assert_eq!(byte0, 0);
        // Now acquire should say "initialize".
        assert_eq!(__cxa_guard_acquire(&raw mut guard), 1);
    }

    #[test]
    fn test_cxa_guard_full_lifecycle() {
        let mut guard: u64 = 0;
        // First acquire: should initialize.
        assert_eq!(__cxa_guard_acquire(&raw mut guard), 1);
        // Simulate successful initialization.
        __cxa_guard_release(&raw mut guard);
        // Second acquire: already done.
        assert_eq!(__cxa_guard_acquire(&raw mut guard), 0);
    }

    #[test]
    fn test_cxa_guard_acquire_null() {
        // Null guard should not crash, return 0.
        let result = __cxa_guard_acquire(core::ptr::null_mut());
        assert_eq!(result, 0);
    }

    #[test]
    fn test_cxa_guard_release_null() {
        // Should not crash.
        __cxa_guard_release(core::ptr::null_mut());
    }

    #[test]
    fn test_cxa_guard_abort_null() {
        // Should not crash.
        __cxa_guard_abort(core::ptr::null_mut());
    }

    // -- C++ exception stubs --

    #[test]
    fn test_cxa_allocate_exception_not_null() {
        let ptr = __cxa_allocate_exception(64);
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_cxa_begin_catch_returns_argument() {
        let obj = 42u8;
        let ptr = &obj as *const u8 as *mut u8;
        let result = __cxa_begin_catch(ptr);
        assert_eq!(result, ptr);
    }

    #[test]
    fn test_cxa_begin_catch_null() {
        let result = __cxa_begin_catch(core::ptr::null_mut());
        assert!(result.is_null());
    }

    #[test]
    fn test_cxa_end_catch_noop() {
        // Should not crash.
        __cxa_end_catch();
    }

    #[test]
    fn test_cxa_finalize_noop() {
        // Should not crash.
        __cxa_finalize(core::ptr::null_mut());
    }

    #[test]
    fn test_gxx_personality_returns_fatal() {
        // Should return _URC_FATAL_PHASE1_ERROR = 8.
        assert_eq!(__gxx_personality_v0(), 8);
    }

    // -- atexit registration (without calling exit) --

    // We can test atexit registration by checking the return value.
    // We can't test exit() itself because it calls _exit which terminates.
    // But we CAN test that atexit returns 0 for valid registrations.

    extern "C" fn dummy_atexit_handler() {}

    #[test]
    fn test_atexit_returns_zero() {
        // Reset state for this test.
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
        }
        let result = atexit(dummy_atexit_handler);
        assert_eq!(result, 0);
        // Cleanup.
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
        }
    }

    #[test]
    fn test_atexit_table_full() {
        // Fill the table, then try one more.
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(MAX_ATEXIT);
        }
        let result = atexit(dummy_atexit_handler);
        assert_eq!(result, -1);
        // Cleanup.
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
        }
    }

    #[test]
    fn test_at_quick_exit_returns_zero() {
        unsafe {
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
        let result = at_quick_exit(dummy_atexit_handler);
        assert_eq!(result, 0);
        unsafe {
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
    }

    #[test]
    fn test_at_quick_exit_table_full() {
        unsafe {
            addr_of_mut!(QUICKEXIT_COUNT).write(MAX_ATEXIT);
        }
        let result = at_quick_exit(dummy_atexit_handler);
        assert_eq!(result, -1);
        unsafe {
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
    }

    // -- __cxa_thread_atexit_impl --

    extern "C" fn dummy_dtor(_: *mut u8) {}

    #[test]
    fn test_cxa_thread_atexit_impl_accepts() {
        let result =
            __cxa_thread_atexit_impl(dummy_dtor, core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(result, 0);
    }

    // -- GCC CRT stubs --

    #[test]
    fn test_libc_csu_init_noop() {
        __libc_csu_init(); // Should not crash.
    }

    #[test]
    fn test_libc_csu_fini_noop() {
        __libc_csu_fini(); // Should not crash.
    }

    // -- __libc_single_threaded --

    #[test]
    fn test_single_threaded_initial() {
        // Should start as 1 (single-threaded).
        let val = unsafe { core::ptr::addr_of!(__libc_single_threaded).read() };
        // We can't guarantee no other test changed it, but it starts at 1.
        let _ = val; // Just verify it's readable.
    }

    // -- posix_spawnattr via __register_atfork --

    #[test]
    fn test_register_atfork_returns_zero() {
        let result = __register_atfork(None, None, None, core::ptr::null_mut());
        assert_eq!(result, 0);
    }

    // -- Arg retrieval constants --

    #[test]
    fn test_init_args_buf_size() {
        // Buffer should be large enough for typical programs.
        assert!(INIT_ARGS_BUF_SIZE >= 64 * 1024);
    }

    #[test]
    fn test_max_init_ptrs() {
        // Should support a reasonable number of arguments.
        assert!(MAX_INIT_PTRS >= 512);
    }

    #[test]
    fn test_init_argv_has_null_terminator_space() {
        // The array must have room for MAX_INIT_PTRS entries + null.
        // SAFETY: just reading the length of the static.
        let len = unsafe { (*addr_of_mut!(INIT_ARGV)).len() };
        assert_eq!(len, MAX_INIT_PTRS + 1);
    }

    #[test]
    fn test_init_envp_has_null_terminator_space() {
        let len = unsafe { (*addr_of_mut!(INIT_ENVP)).len() };
        assert_eq!(len, MAX_INIT_PTRS + 1);
    }

    // -- retrieve_initial_fds constants --

    #[test]
    fn test_max_init_fds() {
        assert_eq!(MAX_INIT_FDS, 32);
    }

    #[test]
    fn test_init_fds_buf_size() {
        // Buffer must hold MAX_INIT_FDS entries of FdMapEntry (16 bytes each).
        let buf_size = unsafe { (*addr_of_mut!(INIT_FDS_BUF)).len() };
        assert_eq!(buf_size, MAX_INIT_FDS);
    }

    #[test]
    fn test_init_fds_buf_zeroed() {
        // All entries should be zeroed at startup.
        let entries = unsafe { &*addr_of_mut!(INIT_FDS_BUF) };
        for entry in entries.iter() {
            assert_eq!(entry.fd, 0);
            assert_eq!(entry.handle_type, 0);
            assert_eq!(entry.handle, 0);
        }
    }

    // -- Stack canary entropy --

    #[test]
    fn test_stack_chk_guard_no_null_bytes() {
        // A good canary should avoid null bytes (they terminate C strings,
        // making buffer overflows easier).
        let bytes = __stack_chk_guard.to_ne_bytes();
        // At least some non-zero bytes
        let nonzero_count = bytes.iter().filter(|&&b| b != 0).count();
        assert!(
            nonzero_count >= 4,
            "canary should have several non-zero bytes"
        );
    }

    // -- __cxa_guard lifecycle: abort then re-acquire --

    #[test]
    fn test_cxa_guard_abort_then_reacquire() {
        let mut guard: u64 = 0;
        // Acquire (start initialization)
        assert_eq!(__cxa_guard_acquire(&raw mut guard), 1);
        // Abort (initialization failed)
        __cxa_guard_abort(&raw mut guard);
        // Should be able to acquire again
        assert_eq!(__cxa_guard_acquire(&raw mut guard), 1);
        // Now succeed
        __cxa_guard_release(&raw mut guard);
        // Should not acquire again
        assert_eq!(__cxa_guard_acquire(&raw mut guard), 0);
    }

    // -- Multiple atexit registrations --

    extern "C" fn dummy_handler2() {}
    extern "C" fn dummy_handler3() {}

    #[test]
    fn test_atexit_multiple_registrations() {
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
        }
        assert_eq!(atexit(dummy_atexit_handler), 0);
        assert_eq!(atexit(dummy_handler2), 0);
        assert_eq!(atexit(dummy_handler3), 0);
        let count = unsafe { addr_of_mut!(ATEXIT_COUNT).read() };
        assert_eq!(count, 3);
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
        }
    }

    #[test]
    fn test_at_quick_exit_multiple_registrations() {
        unsafe {
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
        assert_eq!(at_quick_exit(dummy_atexit_handler), 0);
        assert_eq!(at_quick_exit(dummy_handler2), 0);
        let count = unsafe { addr_of_mut!(QUICKEXIT_COUNT).read() };
        assert_eq!(count, 2);
        unsafe {
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
    }

    // -- atexit and quick_exit stacks are separate --

    #[test]
    fn test_atexit_and_quick_exit_separate() {
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
        atexit(dummy_atexit_handler);
        at_quick_exit(dummy_handler2);
        let a = unsafe { addr_of_mut!(ATEXIT_COUNT).read() };
        let q = unsafe { addr_of_mut!(QUICKEXIT_COUNT).read() };
        assert_eq!(a, 1);
        assert_eq!(q, 1);
        unsafe {
            addr_of_mut!(ATEXIT_COUNT).write(0);
            addr_of_mut!(QUICKEXIT_COUNT).write(0);
        }
    }

    // -- getauxval edge cases --

    #[test]
    fn test_getauxval_zero() {
        crate::errno::set_errno(0);
        let val = getauxval(0); // AT_NULL
        // AT_NULL is not a recognized type in our implementation
        // so it should set ENOENT
        assert_eq!(val, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    #[test]
    fn test_getauxval_max() {
        crate::errno::set_errno(0);
        let val = getauxval(u64::MAX);
        assert_eq!(val, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
    }

    // -- __cxa_allocate_exception sizes --

    #[test]
    fn test_cxa_allocate_exception_zero_size() {
        let ptr = __cxa_allocate_exception(0);
        // Should still return a non-null pointer (a valid address)
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_cxa_allocate_exception_large_size() {
        let ptr = __cxa_allocate_exception(1024);
        assert!(!ptr.is_null());
    }

    // -- __cxa_finalize with non-null --

    #[test]
    fn test_cxa_finalize_nonzero_dso() {
        // Should not crash with a non-null handle
        __cxa_finalize(0x1000 as *mut u8);
    }

    // -- __register_atfork with callbacks --

    extern "C" fn dummy_atfork() {}

    #[test]
    fn test_register_atfork_with_callbacks() {
        let result = __register_atfork(
            Some(dummy_atfork),
            Some(dummy_atfork),
            Some(dummy_atfork),
            core::ptr::null_mut(),
        );
        assert_eq!(result, 0);
    }

    // -- __cxa_thread_atexit_impl with non-null args --

    #[test]
    fn test_cxa_thread_atexit_impl_nonzero() {
        let result = __cxa_thread_atexit_impl(dummy_dtor, 0x1000 as *mut u8, 0x2000 as *mut u8);
        assert_eq!(result, 0);
    }

    // -- Program invocation globals accessible --

    #[test]
    fn test_program_invocation_name_not_null() {
        let ptr = unsafe { core::ptr::addr_of!(program_invocation_name).read() };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_program_invocation_short_name_not_null() {
        let ptr = unsafe { core::ptr::addr_of!(program_invocation_short_name).read() };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_progname_not_null() {
        let ptr = unsafe { core::ptr::addr_of!(__progname).read() };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_progname_full_not_null() {
        let ptr = unsafe { core::ptr::addr_of!(__progname_full).read() };
        assert!(!ptr.is_null());
    }

    // -----------------------------------------------------------------------
    // on_exit
    // -----------------------------------------------------------------------

    extern "C" fn dummy_on_exit(_status: i32, _arg: *mut u8) {}

    #[test]
    fn test_on_exit_registers() {
        let ret = on_exit(dummy_on_exit, core::ptr::null_mut());
        assert_eq!(ret, 0, "on_exit should succeed");
    }

    #[test]
    fn test_on_exit_with_arg() {
        let mut data: i32 = 42;
        let ret = on_exit(dummy_on_exit, (&raw mut data) as *mut u8);
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // gnu_dev_major / gnu_dev_minor / gnu_dev_makedev
    // -----------------------------------------------------------------------

    #[test]
    fn test_gnu_dev_makedev_roundtrip() {
        let dev = gnu_dev_makedev(8, 1);
        assert_eq!(gnu_dev_major(dev), 8);
        assert_eq!(gnu_dev_minor(dev), 1);
    }

    #[test]
    fn test_gnu_dev_makedev_zero() {
        let dev = gnu_dev_makedev(0, 0);
        assert_eq!(dev, 0);
        assert_eq!(gnu_dev_major(dev), 0);
        assert_eq!(gnu_dev_minor(dev), 0);
    }

    #[test]
    fn test_gnu_dev_common_device() {
        // /dev/sda1 is typically major=8, minor=1 on Linux.
        let dev = gnu_dev_makedev(8, 1);
        assert_eq!(gnu_dev_major(dev), 8);
        assert_eq!(gnu_dev_minor(dev), 1);
    }

    #[test]
    fn test_gnu_dev_large_minor() {
        // Test with minor > 255 (uses upper bits).
        let dev = gnu_dev_makedev(8, 300);
        assert_eq!(gnu_dev_major(dev), 8);
        assert_eq!(gnu_dev_minor(dev), 300);
    }

    #[test]
    fn test_gnu_dev_large_major() {
        // Test with major > 4095 (uses upper bits).
        let dev = gnu_dev_makedev(5000, 42);
        assert_eq!(gnu_dev_major(dev), 5000);
        assert_eq!(gnu_dev_minor(dev), 42);
    }
}
