//! Userspace init process — the first process spawned by the kernel.
//!
//! This is PID 1 in our OS.  It runs in ring 3 and communicates with
//! the kernel exclusively through the SYSCALL instruction.
//!
//! ## Current Functionality
//!
//! - Prints a welcome banner via `SYS_CONSOLE_WRITE`.
//! - Runs a poll-based main loop that interleaves keyboard input with
//!   service health monitoring.
//! - Built-in commands: `help`, `echo`, `exit`, `ls`, `cat`, `stat`,
//!   `write`, `mkdir`, `rmdir`, `rm`, `pid`, `uptime`, `logs`, `spawn`,
//!   `svc start|stop|list|status`.
//! - Service manager: registers background services, detects crashes via
//!   non-blocking `SYS_PROCESS_TRY_WAIT`, restarts with exponential
//!   backoff (1s → 2s → 4s → … → 60s cap).
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
const SYS_TASK_ID: u64 = 2;
const SYS_CLOCK_MONOTONIC: u64 = 10;
const SYS_SLEEP: u64 = 11;
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_CONSOLE_READ_CHAR: u64 = 101;
const SYS_CONSOLE_TRY_READ_CHAR: u64 = 103;
const SYS_PROCESS_SPAWN: u64 = 500;
const SYS_PROCESS_WAIT: u64 = 501;
const SYS_PROCESS_TRY_WAIT: u64 = 507;
#[allow(dead_code)] // Services call this, not init itself.
const SYS_NOTIFY_READY: u64 = 508;
const SYS_PROCESS_IS_READY: u64 = 509;
const SYS_PROCESS_SPAWN_EX: u64 = 517;
const SYS_MMAP: u64 = 20;
const SYS_MUNMAP: u64 = 21;
const SYS_FS_READ_FILE: u64 = 600;
const SYS_FS_WRITE_FILE: u64 = 601;
const SYS_FS_DELETE: u64 = 602;
const SYS_FS_LIST_DIR: u64 = 603;
const SYS_FS_MKDIR: u64 = 604;
const SYS_FS_RMDIR: u64 = 605;
const SYS_FS_STAT: u64 = 606;
const SYS_LOG_READ: u64 = 102;

/// Directory entry size from kernel (name[256] + size[4] + type[1] + pad[3]).
const FS_DIR_ENTRY_SIZE: usize = 264;

// ---------------------------------------------------------------------------
// Syscall wrappers
// ---------------------------------------------------------------------------

/// Issue a syscall with 0 arguments.
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

/// Issue a syscall with 3 arguments.
#[inline(always)]
fn syscall3(nr: u64, arg0: u64, arg1: u64, arg2: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a syscall with 4 arguments.
#[inline(always)]
fn syscall4(nr: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") nr,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            in("r10") arg3,
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
#[allow(dead_code)]
fn read_char() -> u8 {
    let mut ch: u8 = 0;
    syscall1(SYS_CONSOLE_READ_CHAR, &mut ch as *mut u8 as u64);
    ch
}

/// Try to read one character without blocking.
/// Returns `Some(ch)` if a key was available, `None` otherwise.
fn try_read_char() -> Option<u8> {
    let mut ch: u8 = 0;
    let ret = syscall1(SYS_CONSOLE_TRY_READ_CHAR, &mut ch as *mut u8 as u64);
    if ret == 1 { Some(ch) } else { None }
}

/// Sleep for `ns` nanoseconds.
fn sleep_ns(ns: u64) {
    syscall1(SYS_SLEEP, ns);
}

/// Exit the process with the given exit code.
fn exit(code: i64) -> ! {
    syscall1(SYS_EXIT, code as u64);
    // The kernel should never return from SYS_EXIT, but just in case:
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

/// Get current task ID.
fn task_id() -> i64 {
    syscall0(SYS_TASK_ID)
}

/// Get monotonic clock in nanoseconds since boot.
fn clock_monotonic() -> i64 {
    syscall0(SYS_CLOCK_MONOTONIC)
}

/// Map anonymous memory pages.  Returns the virtual address or
/// negative error.  `size` is rounded up to the 16 KiB frame size.
fn mmap(size: u64) -> i64 {
    const MAP_WRITE: u64 = 1 << 1;
    syscall4(SYS_MMAP, 0, size, MAP_WRITE, 0)
}

/// Unmap previously mapped memory.
fn munmap(addr: u64, size: u64) -> i64 {
    syscall2(SYS_MUNMAP, addr, size)
}

/// Maximum ELF size for stack-based loading (64 KiB).
/// Larger files use mmap for dynamic allocation.
const STACK_ELF_MAX: usize = 65536;

/// Read a file into a buffer.  Returns bytes read or negative error.
fn fs_read_file(path: &[u8], buf: &mut [u8]) -> i64 {
    syscall4(
        SYS_FS_READ_FILE,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    )
}

/// Write data to a file (create or overwrite).  Returns 0 or error.
fn fs_write_file(path: &[u8], data: &[u8]) -> i64 {
    syscall4(
        SYS_FS_WRITE_FILE,
        path.as_ptr() as u64,
        path.len() as u64,
        data.as_ptr() as u64,
        data.len() as u64,
    )
}

/// Delete a file.  Returns 0 or negative error.
fn fs_delete(path: &[u8]) -> i64 {
    syscall2(
        SYS_FS_DELETE,
        path.as_ptr() as u64,
        path.len() as u64,
    )
}

/// List directory entries into a buffer.  Returns entry count or error.
fn fs_list_dir(path: &[u8], buf: &mut [u8]) -> i64 {
    syscall4(
        SYS_FS_LIST_DIR,
        path.as_ptr() as u64,
        path.len() as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    )
}

/// Create a directory.  Returns 0 or negative error.
fn fs_mkdir(path: &[u8]) -> i64 {
    syscall2(
        SYS_FS_MKDIR,
        path.as_ptr() as u64,
        path.len() as u64,
    )
}

/// Remove an empty directory.  Returns 0 or negative error.
fn fs_rmdir(path: &[u8]) -> i64 {
    syscall2(
        SYS_FS_RMDIR,
        path.as_ptr() as u64,
        path.len() as u64,
    )
}

/// Spawn a new process from ELF data in memory (simple variant).
/// Returns the child PID (positive) or negative error.
#[allow(dead_code)]
fn process_spawn(elf: &[u8], name: &[u8]) -> i64 {
    syscall4(
        SYS_PROCESS_SPAWN,
        elf.as_ptr() as u64,
        elf.len() as u64,
        name.as_ptr() as u64,
        name.len() as u64,
    )
}

/// Extended spawn arguments struct (matches kernel's SpawnExArgs layout).
#[repr(C)]
struct SpawnExArgs {
    elf_ptr: u64,
    elf_len: u64,
    name_ptr: u64,
    name_len: u64,
    fd_map_ptr: u64,
    fd_map_count: u64,
    argv_ptr: u64,
    argv_len: u64,
    argc: u64,
    envp_ptr: u64,
    envp_len: u64,
    envc: u64,
}

/// Spawn a process with arguments and environment variables.
///
/// `elf`: raw ELF binary data.
/// `name`: process name.
/// `argv_data`: packed null-terminated argument strings.
/// `argc`: number of arguments.
/// `envp_data`: packed null-terminated environment strings.
/// `envc`: number of environment variables.
///
/// Returns: child PID (positive) or negative error.
fn process_spawn_ex(
    elf: &[u8],
    name: &[u8],
    argv_data: &[u8],
    argc: usize,
    envp_data: &[u8],
    envc: usize,
) -> i64 {
    let args = SpawnExArgs {
        elf_ptr: elf.as_ptr() as u64,
        elf_len: elf.len() as u64,
        name_ptr: name.as_ptr() as u64,
        name_len: name.len() as u64,
        fd_map_ptr: 0,
        fd_map_count: 0,
        argv_ptr: argv_data.as_ptr() as u64,
        argv_len: argv_data.len() as u64,
        argc: argc as u64,
        envp_ptr: envp_data.as_ptr() as u64,
        envp_len: envp_data.len() as u64,
        envc: envc as u64,
    };
    syscall1(SYS_PROCESS_SPAWN_EX, &args as *const SpawnExArgs as u64)
}

/// Wait for a child process to exit.
/// Returns the exit code or negative error.
fn process_wait(pid: u64) -> i64 {
    syscall1(SYS_PROCESS_WAIT, pid)
}

/// Non-blocking check if a child has exited.
/// Returns exit code if exited, -4 (WouldBlock) if still running.
fn process_try_wait(pid: u64) -> i64 {
    syscall1(SYS_PROCESS_TRY_WAIT, pid)
}

/// Query whether a process has signaled readiness.
/// Returns 1 (ready), 0 (not yet), or negative error.
fn process_is_ready(pid: u64) -> i64 {
    syscall1(SYS_PROCESS_IS_READY, pid)
}

/// Kernel error code for "still running".
const ERR_WOULD_BLOCK: i64 = -4;

/// Stat a file.  Returns 0 or negative error.
/// On success, fills `out` with the 80-byte FsStatResult.  Only the
/// low 16 bytes (size + type + link count) are consumed here:
///   bytes 0-7: file size (u64 LE)
///   byte 8: type (0=file, 1=directory, 2=volume label, 3=symlink)
///   bytes 9-15: reserved
/// The remaining bytes carry permissions/ownership/timestamps/inode that
/// init does not use, but the buffer must be the full size the kernel
/// writes (otherwise the kernel's write overruns the stack buffer).
fn fs_stat(path: &[u8], out: &mut [u8; 80]) -> i64 {
    // syscall3 — we need 3 args: path_ptr, path_len, out_ptr
    let ret: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_FS_STAT,
            in("rdi") path.as_ptr() as u64,
            in("rsi") path.len() as u64,
            in("rdx") out.as_mut_ptr() as u64,
            lateout("rax") ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// ---------------------------------------------------------------------------
// Number formatting (no allocator — format into stack buffer)
// ---------------------------------------------------------------------------

/// Format a u64 as decimal into `buf`, returning the slice of digits.
fn format_u64(value: u64, buf: &mut [u8; 20]) -> &[u8] {
    if value == 0 {
        buf[19] = b'0';
        return &buf[19..];
    }

    let mut pos = 20;
    let mut v = value;
    while v > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    &buf[pos..]
}

/// Format an i64 as decimal into `buf`, returning the slice.
fn format_i64(value: i64, buf: &mut [u8; 21]) -> &[u8] {
    if value >= 0 {
        let mut ubuf = [0u8; 20];
        let digits = format_u64(value as u64, &mut ubuf);
        let start = 21 - digits.len();
        buf[start..21].copy_from_slice(digits);
        &buf[start..21]
    } else {
        // Negative: format absolute value, prepend '-'.
        let abs = if value == i64::MIN {
            // i64::MIN has no positive counterpart, handle specially.
            (i64::MAX as u64) + 1
        } else {
            (-value) as u64
        };
        let mut ubuf = [0u8; 20];
        let digits = format_u64(abs, &mut ubuf);
        let start = 20 - digits.len();
        buf[start] = b'-';
        buf[start + 1..start + 1 + digits.len()].copy_from_slice(digits);
        &buf[start..start + 1 + digits.len()]
    }
}

/// Print a u64 value.
fn print_u64(v: u64) {
    let mut buf = [0u8; 20];
    let s = format_u64(v, &mut buf);
    console_write(s);
}

/// Print an i64 value.
fn print_i64(v: i64) {
    let mut buf = [0u8; 21];
    let s = format_i64(v, &mut buf);
    console_write(s);
}

// ---------------------------------------------------------------------------
// Shell implementation
// ---------------------------------------------------------------------------

/// Maximum command line length.
const MAX_LINE: usize = 256;

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
#[allow(dead_code)]
fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    bytes_eq(&haystack[..needle.len()], needle)
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

/// Get the first word and the rest of the line.
fn split_first_word(s: &[u8]) -> (&[u8], &[u8]) {
    let s = trim(s);
    let mut i = 0;
    while i < s.len() && s[i] != b' ' && s[i] != b'\t' {
        i += 1;
    }
    let cmd = &s[..i];
    let rest = if i < s.len() { trim(&s[i..]) } else { &[] };
    (cmd, rest)
}

// ---------------------------------------------------------------------------
// Service manager
// ---------------------------------------------------------------------------

/// Maximum number of managed services.
const MAX_SERVICES: usize = 16;

/// Maximum length of a service name.
const MAX_SVC_NAME: usize = 32;

/// Maximum length of a service path (filesystem path to ELF binary).
const MAX_SVC_PATH: usize = 128;

/// Maximum number of dependencies per service.
const MAX_DEPS: usize = 4;

/// Maximum number of extra arguments per service (argv[1..]).
const MAX_SVC_ARGS: usize = 8;

/// Maximum length of a single service argument.
const MAX_SVC_ARG_LEN: usize = 64;

/// Maximum number of per-service environment variables.
const MAX_SVC_ENV: usize = 4;

/// Maximum length of a single environment variable (KEY=VALUE).
const MAX_SVC_ENV_LEN: usize = 128;

/// Initial restart delay in nanoseconds (1 second).
const BACKOFF_INITIAL_NS: u64 = 1_000_000_000;

/// Maximum restart delay in nanoseconds (60 seconds).
const BACKOFF_MAX_NS: u64 = 60_000_000_000;

/// Backoff multiplier (shift left by 1 = multiply by 2).
const BACKOFF_MULTIPLIER: u32 = 1;

/// How long a service must run before we reset its backoff (10 seconds).
const BACKOFF_RESET_THRESHOLD_NS: u64 = 10_000_000_000;

/// Maximum size of packed argv/envp data for `process_spawn_ex()`.
const MAX_PACKED_ARGS: usize = 1024;

/// Default environment passed to spawned processes.
/// Packed null-terminated format: "PATH=/bin\0".
const DEFAULT_ENVP: &[u8] = b"PATH=/bin\0";

/// Number of entries in `DEFAULT_ENVP`.
const DEFAULT_ENVP_COUNT: usize = 1;

/// A registered service entry.
struct Service {
    /// Human-readable name (extracted from path, null-terminated).
    name: [u8; MAX_SVC_NAME],
    name_len: usize,

    /// Filesystem path to the ELF binary.
    path: [u8; MAX_SVC_PATH],
    path_len: usize,

    /// PID of the running instance, or 0 if not running.
    pid: u64,

    /// Whether the service manager should restart this on crash.
    auto_restart: bool,

    /// Whether this slot is in use.
    active: bool,

    /// Current backoff delay (nanoseconds).  Doubles on each crash,
    /// resets after the service runs for `BACKOFF_RESET_THRESHOLD_NS`.
    backoff_ns: u64,

    /// Timestamp (monotonic ns) when the service was last started.
    started_at_ns: u64,

    /// Timestamp when we should next attempt a restart (0 = now).
    restart_after_ns: u64,

    /// Total number of times this service has crashed.
    crash_count: u64,

    /// Whether the service has signaled it is fully initialized.
    ready: bool,

    /// Dependency names.  This service won't start until all named
    /// dependencies have signaled ready.  Each entry is a name stored
    /// as `[u8; MAX_SVC_NAME]` with a length.
    dep_names: [[u8; MAX_SVC_NAME]; MAX_DEPS],
    dep_name_lens: [usize; MAX_DEPS],
    dep_count: usize,

    /// Whether this service is waiting on dependencies before first start.
    waiting_on_deps: bool,

    /// Extra arguments (argv[1..]).  argv[0] is always the service path.
    /// Parsed from `args:` in /etc/services.
    svc_args: [[u8; MAX_SVC_ARG_LEN]; MAX_SVC_ARGS],
    svc_arg_lens: [usize; MAX_SVC_ARGS],
    svc_arg_count: usize,

    /// Per-service environment variables (KEY=VALUE format).
    /// Parsed from `env:` in /etc/services.  These are appended
    /// after the default environment (PATH=/bin).
    svc_env: [[u8; MAX_SVC_ENV_LEN]; MAX_SVC_ENV],
    svc_env_lens: [usize; MAX_SVC_ENV],
    svc_env_count: usize,
}

impl Service {
    const fn empty() -> Self {
        Self {
            name: [0u8; MAX_SVC_NAME],
            name_len: 0,
            path: [0u8; MAX_SVC_PATH],
            path_len: 0,
            pid: 0,
            auto_restart: true,
            active: false,
            backoff_ns: BACKOFF_INITIAL_NS,
            started_at_ns: 0,
            restart_after_ns: 0,
            crash_count: 0,
            ready: false,
            dep_names: [[0u8; MAX_SVC_NAME]; MAX_DEPS],
            dep_name_lens: [0; MAX_DEPS],
            dep_count: 0,
            waiting_on_deps: false,
            svc_args: [[0u8; MAX_SVC_ARG_LEN]; MAX_SVC_ARGS],
            svc_arg_lens: [0; MAX_SVC_ARGS],
            svc_arg_count: 0,
            svc_env: [[0u8; MAX_SVC_ENV_LEN]; MAX_SVC_ENV],
            svc_env_lens: [0; MAX_SVC_ENV],
            svc_env_count: 0,
        }
    }
}

/// The service registry.  Fixed-size, no heap.
struct ServiceRegistry {
    services: [Service; MAX_SERVICES],
    count: usize,
}

impl ServiceRegistry {
    const fn new() -> Self {
        Self {
            services: [
                Service::empty(), Service::empty(), Service::empty(), Service::empty(),
                Service::empty(), Service::empty(), Service::empty(), Service::empty(),
                Service::empty(), Service::empty(), Service::empty(), Service::empty(),
                Service::empty(), Service::empty(), Service::empty(), Service::empty(),
            ],
            count: 0,
        }
    }

    /// Register a new service by filesystem path.  Extracts the filename
    /// as the service name.  Returns the slot index or `None` if full.
    fn register(&mut self, path: &[u8]) -> Option<usize> {
        if self.count >= MAX_SERVICES || path.is_empty() {
            return None;
        }

        // Find a free slot (prefer the first unused).
        let mut slot = None;
        let mut i = 0;
        while i < MAX_SERVICES {
            if !self.services[i].active {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let idx = slot?;

        let svc = &mut self.services[idx];

        // Copy path.
        let plen = if path.len() > MAX_SVC_PATH { MAX_SVC_PATH } else { path.len() };
        svc.path[..plen].copy_from_slice(&path[..plen]);
        svc.path_len = plen;

        // Extract filename from path as service name.
        let mut last_slash = 0;
        let mut j = 0;
        while j < plen {
            if path[j] == b'/' {
                last_slash = j + 1;
            }
            j += 1;
        }
        let name_src = &path[last_slash..plen];
        let nlen = if name_src.len() > MAX_SVC_NAME { MAX_SVC_NAME } else { name_src.len() };
        svc.name[..nlen].copy_from_slice(&name_src[..nlen]);
        svc.name_len = nlen;

        svc.active = true;
        svc.auto_restart = true;
        svc.pid = 0;
        svc.backoff_ns = BACKOFF_INITIAL_NS;
        svc.started_at_ns = 0;
        svc.restart_after_ns = 0;
        svc.crash_count = 0;
        svc.ready = false;
        svc.dep_count = 0;
        svc.waiting_on_deps = false;
        svc.svc_arg_count = 0;
        svc.svc_env_count = 0;

        self.count += 1;
        Some(idx)
    }

    /// Set extra arguments (argv[1..]) for a registered service.
    ///
    /// `args_str` is a comma-separated list of arguments, e.g.
    /// `b"--verbose,-d,--port=8080"`.  Returns the number of args set.
    fn set_arguments(&mut self, idx: usize, args_str: &[u8]) -> usize {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return 0;
        }

        let svc = &mut self.services[idx];
        svc.svc_arg_count = 0;

        if args_str.is_empty() {
            return 0;
        }

        // Parse comma-separated arguments.
        let mut start = 0;
        while start < args_str.len() && svc.svc_arg_count < MAX_SVC_ARGS {
            let mut end = start;
            while end < args_str.len() && args_str[end] != b',' {
                end += 1;
            }

            let arg = trim(&args_str[start..end]);
            if !arg.is_empty() {
                let alen = if arg.len() > MAX_SVC_ARG_LEN {
                    MAX_SVC_ARG_LEN
                } else {
                    arg.len()
                };
                svc.svc_args[svc.svc_arg_count][..alen].copy_from_slice(&arg[..alen]);
                svc.svc_arg_lens[svc.svc_arg_count] = alen;
                svc.svc_arg_count += 1;
            }

            start = end + 1;
        }

        svc.svc_arg_count
    }

    /// Set per-service environment variables.
    ///
    /// `env_str` is a comma-separated list of KEY=VALUE pairs, e.g.
    /// `b"PORT=8080,WORKERS=4"`.  Returns the number of env vars set.
    fn set_env(&mut self, idx: usize, env_str: &[u8]) -> usize {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return 0;
        }

        let svc = &mut self.services[idx];
        svc.svc_env_count = 0;

        if env_str.is_empty() {
            return 0;
        }

        // Parse comma-separated KEY=VALUE pairs.
        let mut start = 0;
        while start < env_str.len() && svc.svc_env_count < MAX_SVC_ENV {
            let mut end = start;
            while end < env_str.len() && env_str[end] != b',' {
                end += 1;
            }

            let entry = trim(&env_str[start..end]);
            if !entry.is_empty() {
                let elen = if entry.len() > MAX_SVC_ENV_LEN {
                    MAX_SVC_ENV_LEN
                } else {
                    entry.len()
                };
                svc.svc_env[svc.svc_env_count][..elen].copy_from_slice(&entry[..elen]);
                svc.svc_env_lens[svc.svc_env_count] = elen;
                svc.svc_env_count += 1;
            }

            start = end + 1;
        }

        svc.svc_env_count
    }

    /// Set dependencies for a registered service.
    ///
    /// `deps_str` is a comma-separated list of service names, e.g.
    /// `b"logger,network"`.  Returns the number of dependencies set.
    fn set_dependencies(&mut self, idx: usize, deps_str: &[u8]) -> usize {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return 0;
        }

        let svc = &mut self.services[idx];
        svc.dep_count = 0;

        if deps_str.is_empty() {
            return 0;
        }

        // Parse comma-separated names.
        let mut start = 0;
        while start < deps_str.len() && svc.dep_count < MAX_DEPS {
            let mut end = start;
            while end < deps_str.len() && deps_str[end] != b',' {
                end += 1;
            }

            let name = trim(&deps_str[start..end]);
            if !name.is_empty() {
                let nlen = if name.len() > MAX_SVC_NAME {
                    MAX_SVC_NAME
                } else {
                    name.len()
                };
                svc.dep_names[svc.dep_count][..nlen].copy_from_slice(&name[..nlen]);
                svc.dep_name_lens[svc.dep_count] = nlen;
                svc.dep_count += 1;
            }

            start = end + 1;
        }

        svc.dep_count
    }

    /// Check if all dependencies of a service are satisfied (ready).
    fn deps_satisfied(&self, idx: usize) -> bool {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return false;
        }

        let svc = &self.services[idx];
        if svc.dep_count == 0 {
            return true; // No dependencies.
        }

        let mut d = 0;
        while d < svc.dep_count {
            let dep_name = &svc.dep_names[d][..svc.dep_name_lens[d]];

            // Find the dependency service.
            let mut found_ready = false;
            let mut j = 0;
            while j < MAX_SERVICES {
                if j != idx
                    && self.services[j].active
                    && self.services[j].name_len == dep_name.len()
                    && bytes_eq(
                        &self.services[j].name[..self.services[j].name_len],
                        dep_name,
                    )
                {
                    if self.services[j].ready {
                        found_ready = true;
                    }
                    break;
                }
                j += 1;
            }

            if !found_ready {
                return false; // This dependency not satisfied.
            }

            d += 1;
        }

        true // All deps ready.
    }

    /// Find a service by name.  Returns the slot index or `None`.
    fn find_by_name(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i < MAX_SERVICES {
            if self.services[i].active
                && self.services[i].name_len == name.len()
                && bytes_eq(&self.services[i].name[..self.services[i].name_len], name)
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Start a service (read ELF from VFS, spawn process).
    /// Returns the child PID or a negative error code.
    fn start_service(&mut self, idx: usize) -> i64 {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return -1;
        }

        // Copy path, name, args, and env to local buffers to avoid
        // borrowing `self` across the mutable update below.
        let mut path_buf = [0u8; MAX_SVC_PATH];
        let path_len = self.services[idx].path_len;
        path_buf[..path_len].copy_from_slice(
            &self.services[idx].path[..path_len],
        );

        let mut name_buf = [0u8; MAX_SVC_NAME];
        let name_len = self.services[idx].name_len;
        name_buf[..name_len].copy_from_slice(
            &self.services[idx].name[..name_len],
        );

        // Snapshot service args and env before releasing the borrow.
        let arg_count = self.services[idx].svc_arg_count;
        let mut args_copy = [[0u8; MAX_SVC_ARG_LEN]; MAX_SVC_ARGS];
        let mut arg_lens_copy = [0usize; MAX_SVC_ARGS];
        let mut a = 0;
        while a < arg_count {
            let alen = self.services[idx].svc_arg_lens[a];
            args_copy[a][..alen].copy_from_slice(
                &self.services[idx].svc_args[a][..alen],
            );
            arg_lens_copy[a] = alen;
            a += 1;
        }

        let env_count = self.services[idx].svc_env_count;
        let mut env_copy = [[0u8; MAX_SVC_ENV_LEN]; MAX_SVC_ENV];
        let mut env_lens_copy = [0usize; MAX_SVC_ENV];
        let mut e = 0;
        while e < env_count {
            let elen = self.services[idx].svc_env_lens[e];
            env_copy[e][..elen].copy_from_slice(
                &self.services[idx].svc_env[e][..elen],
            );
            env_lens_copy[e] = elen;
            e += 1;
        }

        let path = &path_buf[..path_len];
        let name = &name_buf[..name_len];

        // Stat the ELF binary to get its size.
        let mut stat_out = [0u8; 80];
        let stat_ret = fs_stat(path, &mut stat_out);
        if stat_ret < 0 {
            print("[svc] Failed to stat ");
            console_write(path);
            print(": error ");
            print_i64(stat_ret);
            print("\n");
            return stat_ret;
        }
        let file_size = u64::from_le_bytes([
            stat_out[0], stat_out[1], stat_out[2], stat_out[3],
            stat_out[4], stat_out[5], stat_out[6], stat_out[7],
        ]) as usize;

        if file_size == 0 {
            print("[svc] Empty ELF: ");
            console_write(path);
            print("\n");
            return -1;
        }

        // Read the ELF binary.  For files ≤ 64 KiB, use a stack buffer
        // to avoid the mmap/munmap overhead.  Larger files use mmap.
        let mut stack_buf = [0u8; STACK_ELF_MAX];
        let (elf_ptr, elf_mmap_addr, elf_mmap_size): (*const u8, u64, u64);

        if file_size <= STACK_ELF_MAX {
            let result = fs_read_file(path, &mut stack_buf);
            if result < 0 {
                print("[svc] Failed to read ");
                console_write(path);
                print(": error ");
                print_i64(result);
                print("\n");
                return result;
            }
            elf_ptr = stack_buf.as_ptr();
            elf_mmap_addr = 0;
            elf_mmap_size = 0;
        } else {
            // Allocate a buffer via mmap for large ELFs.
            let mapped = mmap(file_size as u64);
            if mapped < 0 {
                print("[svc] mmap failed for ");
                console_write(path);
                print(": error ");
                print_i64(mapped);
                print("\n");
                return mapped;
            }
            #[allow(clippy::cast_sign_loss)]
            let addr = mapped as u64;
            // SAFETY: mmap returned a valid writable buffer of at least
            // `file_size` bytes.  We use it as a &mut [u8] for reading.
            let buf = unsafe {
                core::slice::from_raw_parts_mut(addr as *mut u8, file_size)
            };
            let result = fs_read_file(path, buf);
            if result < 0 {
                munmap(addr, file_size as u64);
                print("[svc] Failed to read ");
                console_write(path);
                print(": error ");
                print_i64(result);
                print("\n");
                return result;
            }
            elf_ptr = addr as *const u8;
            elf_mmap_addr = addr;
            elf_mmap_size = file_size as u64;
        }

        // SAFETY: elf_ptr points to `file_size` valid bytes (either
        // stack_buf or mmap'd region).
        let elf_data = unsafe {
            core::slice::from_raw_parts(elf_ptr, file_size)
        };

        // Build packed argv: [path\0arg1\0arg2\0...]
        let mut argv_buf = [0u8; MAX_PACKED_ARGS];
        let mut argc: usize = 0;

        // argv[0] = service path (POSIX convention).
        argv_buf[..path_len].copy_from_slice(path);
        let mut argv_pos = path_len;
        argv_buf[argv_pos] = 0;
        argv_pos += 1;
        argc += 1;

        // argv[1..] = per-service extra arguments.
        a = 0;
        while a < arg_count {
            let alen = arg_lens_copy[a];
            if argv_pos + alen < MAX_PACKED_ARGS {
                argv_buf[argv_pos..argv_pos + alen].copy_from_slice(
                    &args_copy[a][..alen],
                );
                argv_pos += alen;
                argv_buf[argv_pos] = 0;
                argv_pos += 1;
                argc += 1;
            }
            a += 1;
        }

        // Build packed envp: default env + per-service env.
        let mut envp_buf = [0u8; MAX_PACKED_ARGS];
        let mut envc: usize = 0;

        // Always include PATH=/bin.
        let default_env = b"PATH=/bin";
        envp_buf[..default_env.len()].copy_from_slice(default_env);
        let mut envp_pos = default_env.len();
        envp_buf[envp_pos] = 0;
        envp_pos += 1;
        envc += 1;

        // Append per-service env vars.
        e = 0;
        while e < env_count {
            let elen = env_lens_copy[e];
            if envp_pos + elen < MAX_PACKED_ARGS {
                envp_buf[envp_pos..envp_pos + elen].copy_from_slice(
                    &env_copy[e][..elen],
                );
                envp_pos += elen;
                envp_buf[envp_pos] = 0;
                envp_pos += 1;
                envc += 1;
            }
            e += 1;
        }

        let pid = process_spawn_ex(
            elf_data, name,
            &argv_buf[..argv_pos], argc,
            &envp_buf[..envp_pos], envc,
        );

        // Free mmap'd buffer now that spawn has copied the ELF data.
        if elf_mmap_addr != 0 {
            munmap(elf_mmap_addr, elf_mmap_size);
        }

        if pid < 0 {
            print("[svc] Failed to spawn ");
            console_write(name);
            print(": error ");
            print_i64(pid);
            print("\n");
            return pid;
        }

        // Update service state.
        let svc = &mut self.services[idx];
        #[allow(clippy::cast_sign_loss)]
        {
            svc.pid = pid as u64;
        }
        svc.started_at_ns = clock_monotonic() as u64;
        svc.restart_after_ns = 0;
        svc.ready = false;

        print("[svc] Started ");
        console_write(name);
        print(" (PID ");
        print_i64(pid);
        print(")\n");

        pid
    }

    /// Stop a service by sending a kill and marking it for no-restart.
    fn stop_service(&mut self, idx: usize) {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return;
        }

        let svc = &mut self.services[idx];
        svc.auto_restart = false;

        if svc.pid != 0 {
            // Kill the process.
            let ret = syscall2(506, svc.pid, 0); // SYS_PROCESS_KILL = 506
            if ret >= 0 {
                // Reap the zombie.
                let _ = process_try_wait(svc.pid);
            }
            let name = &svc.name[..svc.name_len];
            print("[svc] Stopped ");
            console_write(name);
            print(" (PID ");
            print_u64(svc.pid);
            print(")\n");
            svc.pid = 0;
        }
    }

    /// Unregister a service entirely.
    fn unregister(&mut self, idx: usize) {
        if idx >= MAX_SERVICES || !self.services[idx].active {
            return;
        }
        self.stop_service(idx);
        self.services[idx].active = false;
        if self.count > 0 {
            self.count -= 1;
        }
    }

    /// Poll all registered services.  For any that have exited,
    /// handle crash detection and restart scheduling.  Also starts
    /// services waiting on dependencies once all deps are ready.
    fn poll(&mut self) {
        let now_ns = clock_monotonic() as u64;

        let mut i = 0;
        while i < MAX_SERVICES {
            if !self.services[i].active || self.services[i].pid == 0 {
                // Check if this service is waiting on dependencies.
                if self.services[i].active
                    && self.services[i].waiting_on_deps
                    && self.deps_satisfied(i)
                {
                    let name_len = self.services[i].name_len;
                    let mut name = [0u8; MAX_SVC_NAME];
                    name[..name_len].copy_from_slice(
                        &self.services[i].name[..name_len],
                    );
                    print("[svc] Dependencies satisfied for ");
                    console_write(&name[..name_len]);
                    print(" — starting\n");
                    self.services[i].waiting_on_deps = false;
                    self.start_service(i);
                    i += 1;
                    continue;
                }

                // Not active or not currently running — check if pending restart.
                if self.services[i].active
                    && self.services[i].auto_restart
                    && self.services[i].restart_after_ns > 0
                    && now_ns >= self.services[i].restart_after_ns
                {
                    // Time to restart.
                    let name = &self.services[i].name[..self.services[i].name_len];
                    print("[svc] Restarting ");
                    console_write(name);
                    print(" (crash #");
                    print_u64(self.services[i].crash_count);
                    print(", backoff ");
                    print_u64(self.services[i].backoff_ns / 1_000_000_000);
                    print("s)\n");
                    self.start_service(i);
                }
                i += 1;
                continue;
            }

            let pid = self.services[i].pid;
            let ret = process_try_wait(pid);

            if ret == ERR_WOULD_BLOCK {
                // Still running — check for readiness transition.
                if !self.services[i].ready && process_is_ready(pid) == 1 {
                    self.services[i].ready = true;
                    let name = &self.services[i].name[..self.services[i].name_len];
                    print("[svc] ");
                    console_write(name);
                    print(" signaled ready\n");
                }
                i += 1;
                continue;
            }

            // Process has exited (ret = exit code) or we got an error
            // (e.g., NoSuchProcess if it was already reaped).
            let name_len = self.services[i].name_len;
            let mut name_copy = [0u8; MAX_SVC_NAME];
            name_copy[..name_len].copy_from_slice(
                &self.services[i].name[..name_len],
            );

            let svc = &mut self.services[i];
            let runtime_ns = now_ns.saturating_sub(svc.started_at_ns);

            print("[svc] ");
            console_write(&name_copy[..name_len]);
            print(" (PID ");
            print_u64(pid);
            print(") exited with code ");
            print_i64(ret);
            print(" after ");
            print_u64(runtime_ns / 1_000_000_000);
            print("s\n");

            svc.pid = 0;
            svc.crash_count += 1;

            // If it ran long enough, reset backoff.
            if runtime_ns >= BACKOFF_RESET_THRESHOLD_NS {
                svc.backoff_ns = BACKOFF_INITIAL_NS;
            }

            if svc.auto_restart {
                // Schedule restart with current backoff.
                svc.restart_after_ns = now_ns.saturating_add(svc.backoff_ns);
                print("[svc] Will restart ");
                console_write(&name_copy[..name_len]);
                print(" in ");
                print_u64(svc.backoff_ns / 1_000_000_000);
                print("s\n");

                // Increase backoff for next time (exponential, capped).
                svc.backoff_ns = svc.backoff_ns.checked_shl(BACKOFF_MULTIPLIER)
                    .unwrap_or(BACKOFF_MAX_NS);
                if svc.backoff_ns > BACKOFF_MAX_NS {
                    svc.backoff_ns = BACKOFF_MAX_NS;
                }
            }

            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

/// `ls [path]` — list directory contents.
fn cmd_ls(args: &[u8]) {
    let path = if args.is_empty() { b"/" as &[u8] } else { args };

    // Buffer for directory entries.  Each entry is FS_DIR_ENTRY_SIZE bytes.
    // Support up to 32 entries.
    let mut buf = [0u8; FS_DIR_ENTRY_SIZE * 32];

    let result = fs_list_dir(path, &mut buf);
    if result < 0 {
        print("ls: error ");
        print_i64(result);
        print("\n");
        return;
    }

    let count = result as usize;
    if count == 0 {
        print("(empty directory)\n");
        return;
    }

    let mut i = 0;
    while i < count {
        let offset = i * FS_DIR_ENTRY_SIZE;
        // Name: null-terminated string at offset 0, up to 255 bytes.
        let name_end = {
            let mut j = 0;
            while j < 256 && buf[offset + j] != 0 {
                j += 1;
            }
            j
        };
        let name = &buf[offset..offset + name_end];

        // Size: u32 LE at offset 256.
        let size_bytes = [
            buf[offset + 256],
            buf[offset + 257],
            buf[offset + 258],
            buf[offset + 259],
        ];
        let size = u32::from_le_bytes(size_bytes);

        // Type: byte at offset 260 (0=file, 1=directory).
        let entry_type = buf[offset + 260];

        if entry_type == 1 {
            print("  [DIR]  ");
        } else {
            print("  ");
            // Right-align size in a 6-char field.
            let mut sbuf = [0u8; 20];
            let sstr = format_u64(size as u64, &mut sbuf);
            let pad = if sstr.len() < 6 { 6 - sstr.len() } else { 0 };
            let mut p = 0;
            while p < pad {
                console_write(b" ");
                p += 1;
            }
            console_write(sstr);
            print("  ");
        }
        console_write(name);
        print("\n");

        i += 1;
    }
}

/// `cat <path>` — print file contents.
fn cmd_cat(args: &[u8]) {
    if args.is_empty() {
        print("cat: missing filename\n");
        return;
    }

    let mut buf = [0u8; 4096];
    let result = fs_read_file(args, &mut buf);
    if result < 0 {
        print("cat: error ");
        print_i64(result);
        print("\n");
        return;
    }

    let len = result as usize;
    console_write(&buf[..len]);
    // Add a trailing newline if the file doesn't end with one.
    if len > 0 && buf[len - 1] != b'\n' {
        print("\n");
    }
}

/// `write <path> <data>` — write text to a file.
fn cmd_write(args: &[u8]) {
    let (path, data) = split_first_word(args);
    if path.is_empty() {
        print("write: usage: write <path> <data>\n");
        return;
    }

    let result = fs_write_file(path, data);
    if result < 0 {
        print("write: error ");
        print_i64(result);
        print("\n");
    } else {
        print("wrote ");
        print_u64(data.len() as u64);
        print(" bytes\n");
    }
}

/// `stat <path>` — show file metadata.
fn cmd_stat(args: &[u8]) {
    if args.is_empty() {
        print("stat: missing path\n");
        return;
    }

    let mut out = [0u8; 80];
    let result = fs_stat(args, &mut out);
    if result < 0 {
        print("stat: error ");
        print_i64(result);
        print("\n");
        return;
    }

    // Parse the 16-byte FsStatResult.
    let size = u64::from_le_bytes([
        out[0], out[1], out[2], out[3],
        out[4], out[5], out[6], out[7],
    ]);
    let entry_type = out[8];

    console_write(args);
    print(": ");
    match entry_type {
        0 => print("file"),
        1 => print("directory"),
        2 => print("volume label"),
        _ => print("unknown"),
    }
    print(", size=");
    print_u64(size);
    print(" bytes\n");
}

/// `mkdir <path>` — create a directory.
fn cmd_mkdir(args: &[u8]) {
    if args.is_empty() {
        print("mkdir: missing path\n");
        return;
    }
    let result = fs_mkdir(args);
    if result < 0 {
        print("mkdir: error ");
        print_i64(result);
        print("\n");
    }
}

/// `rmdir <path>` — remove an empty directory.
fn cmd_rmdir(args: &[u8]) {
    if args.is_empty() {
        print("rmdir: missing path\n");
        return;
    }
    let result = fs_rmdir(args);
    if result < 0 {
        print("rmdir: error ");
        print_i64(result);
        print("\n");
    }
}

/// `rm <path>` — delete a file.
fn cmd_rm(args: &[u8]) {
    if args.is_empty() {
        print("rm: missing path\n");
        return;
    }
    let result = fs_delete(args);
    if result < 0 {
        print("rm: error ");
        print_i64(result);
        print("\n");
    }
}

/// `spawn <path> [args...]` — load an ELF from the filesystem and run
/// it, optionally passing command-line arguments.  The path becomes
/// argv[0]; any additional words become argv[1..].  A default
/// environment (PATH=/bin) is always passed.
fn cmd_spawn(args: &[u8]) {
    if args.is_empty() {
        print("spawn: missing path\n");
        return;
    }

    // First word is the ELF path, remaining words are arguments.
    let (path, remaining_args) = split_first_word(args);

    // Stat the file to get its size.
    let mut stat_out = [0u8; 80];
    let stat_ret = fs_stat(path, &mut stat_out);
    if stat_ret < 0 {
        print("spawn: failed to stat ");
        console_write(path);
        print(": error ");
        print_i64(stat_ret);
        print("\n");
        return;
    }
    let file_size = u64::from_le_bytes([
        stat_out[0], stat_out[1], stat_out[2], stat_out[3],
        stat_out[4], stat_out[5], stat_out[6], stat_out[7],
    ]) as usize;
    if file_size == 0 {
        print("spawn: empty file\n");
        return;
    }

    // Read the ELF binary.  Stack buffer for small files, mmap for large.
    let mut stack_buf = [0u8; STACK_ELF_MAX];
    let (elf_ptr, elf_mmap_addr, elf_mmap_size): (*const u8, u64, u64);

    if file_size <= STACK_ELF_MAX {
        let result = fs_read_file(path, &mut stack_buf);
        if result < 0 {
            print("spawn: failed to read ");
            console_write(path);
            print(": error ");
            print_i64(result);
            print("\n");
            return;
        }
        elf_ptr = stack_buf.as_ptr();
        elf_mmap_addr = 0;
        elf_mmap_size = 0;
    } else {
        let mapped = mmap(file_size as u64);
        if mapped < 0 {
            print("spawn: mmap failed: error ");
            print_i64(mapped);
            print("\n");
            return;
        }
        #[allow(clippy::cast_sign_loss)]
        let addr = mapped as u64;
        // SAFETY: mmap returned a valid writable buffer.
        let buf = unsafe {
            core::slice::from_raw_parts_mut(addr as *mut u8, file_size)
        };
        let result = fs_read_file(path, buf);
        if result < 0 {
            munmap(addr, file_size as u64);
            print("spawn: failed to read ");
            console_write(path);
            print(": error ");
            print_i64(result);
            print("\n");
            return;
        }
        elf_ptr = addr as *const u8;
        elf_mmap_addr = addr;
        elf_mmap_size = file_size as u64;
    }

    // SAFETY: elf_ptr points to file_size valid bytes.
    let elf_data = unsafe {
        core::slice::from_raw_parts(elf_ptr, file_size)
    };

    // Extract filename from path for the process name.
    let name = {
        let mut last_slash = 0;
        let mut i = 0;
        while i < path.len() {
            if path[i] == b'/' {
                last_slash = i + 1;
            }
            i += 1;
        }
        &path[last_slash..]
    };

    // Build packed argv: [path\0arg1\0arg2\0...]
    let mut argv_data = [0u8; MAX_PACKED_ARGS];
    let mut argv_pos: usize = 0;
    let mut argc: usize = 0;

    // argv[0] = path.
    if path.len() < MAX_PACKED_ARGS {
        argv_data[argv_pos..argv_pos + path.len()].copy_from_slice(path);
        argv_pos += path.len();
        argv_data[argv_pos] = 0;
        argv_pos += 1;
        argc += 1;
    }

    // Parse remaining arguments (space-separated).
    let mut rest = remaining_args;
    while !rest.is_empty() && argc < 32 {
        let (word, next) = split_first_word(rest);
        if word.is_empty() {
            break;
        }
        if argv_pos + word.len() + 1 > MAX_PACKED_ARGS {
            print("spawn: argument list too long\n");
            if elf_mmap_addr != 0 { munmap(elf_mmap_addr, elf_mmap_size); }
            return;
        }
        argv_data[argv_pos..argv_pos + word.len()].copy_from_slice(word);
        argv_pos += word.len();
        argv_data[argv_pos] = 0;
        argv_pos += 1;
        argc += 1;
        rest = next;
    }

    print("Spawning ");
    console_write(path);
    if argc > 1 {
        print(" with ");
        #[allow(clippy::arithmetic_side_effects)]
        let extra = argc as u64 - 1;
        print_u64(extra);
        print(" arg(s)");
    }
    print(" (");
    print_u64(file_size as u64);
    print(" bytes)...\n");

    let pid = process_spawn_ex(
        elf_data, name,
        &argv_data[..argv_pos], argc,
        DEFAULT_ENVP, DEFAULT_ENVP_COUNT,
    );

    // Free mmap'd buffer after spawn has copied the ELF.
    if elf_mmap_addr != 0 {
        munmap(elf_mmap_addr, elf_mmap_size);
    }

    if pid < 0 {
        print("spawn: error ");
        print_i64(pid);
        print("\n");
        return;
    }

    print("Started PID ");
    print_i64(pid);
    print(", waiting...\n");

    // Wait for the child to exit.
    let exit_code = process_wait(pid as u64);
    print("Process exited with code ");
    print_i64(exit_code);
    print("\n");
}

/// Show recent kernel log entries (JSON-lines).
fn cmd_logs() {
    // Read log entries from the kernel ring buffer.
    // Use u64::MAX as after_seq to read from the oldest available.
    let mut buf = [0u8; 4096];
    let ret = syscall3(
        SYS_LOG_READ,
        u64::MAX, // Read from beginning.
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );

    if ret < 0 {
        print("logs: error ");
        print_i64(ret);
        print("\n");
        return;
    }

    let count = ret as u64;
    print("Kernel log (");
    print_u64(count);
    print(" entries):\n");

    // Find the actual data length (scan for last non-zero byte).
    let data_len = buf.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    if data_len > 0 {
        if let Some(data) = buf.get(..data_len) {
            console_write(data);
        }
    }
}

/// `svc` — service manager commands.
///
/// Subcommands:
///   `svc start <path>`   — register and start a service.
///   `svc stop <name>`    — stop a service (disable auto-restart).
///   `svc restart <name>` — restart a stopped/crashed service.
///   `svc remove <name>`  — stop and unregister a service.
///   `svc list`           — list all registered services.
///   `svc status <name>`  — show detailed status of one service.
fn cmd_svc(args: &[u8], registry: &mut ServiceRegistry) {
    let (sub, rest) = split_first_word(args);

    if bytes_eq(sub, b"start") {
        if rest.is_empty() {
            print("svc start: missing path\n");
            return;
        }
        match registry.register(rest) {
            Some(idx) => {
                registry.start_service(idx);
            }
            None => print("svc start: registry full or invalid path\n"),
        }
    } else if bytes_eq(sub, b"stop") {
        if rest.is_empty() {
            print("svc stop: missing service name\n");
            return;
        }
        match registry.find_by_name(rest) {
            Some(idx) => registry.stop_service(idx),
            None => {
                print("svc stop: unknown service '");
                console_write(rest);
                print("'\n");
            }
        }
    } else if bytes_eq(sub, b"restart") {
        if rest.is_empty() {
            print("svc restart: missing service name\n");
            return;
        }
        match registry.find_by_name(rest) {
            Some(idx) => {
                // Re-enable auto-restart and start immediately.
                registry.services[idx].auto_restart = true;
                registry.services[idx].backoff_ns = BACKOFF_INITIAL_NS;
                registry.services[idx].restart_after_ns = 0;
                if registry.services[idx].pid != 0 {
                    // Stop first, then restart.
                    registry.stop_service(idx);
                    registry.services[idx].auto_restart = true;
                }
                registry.start_service(idx);
            }
            None => {
                print("svc restart: unknown service '");
                console_write(rest);
                print("'\n");
            }
        }
    } else if bytes_eq(sub, b"remove") {
        if rest.is_empty() {
            print("svc remove: missing service name\n");
            return;
        }
        match registry.find_by_name(rest) {
            Some(idx) => {
                let name_len = registry.services[idx].name_len;
                let mut name = [0u8; MAX_SVC_NAME];
                name[..name_len].copy_from_slice(
                    &registry.services[idx].name[..name_len],
                );
                registry.unregister(idx);
                print("[svc] Removed ");
                console_write(&name[..name_len]);
                print("\n");
            }
            None => {
                print("svc remove: unknown service '");
                console_write(rest);
                print("'\n");
            }
        }
    } else if bytes_eq(sub, b"list") {
        if registry.count == 0 {
            print("No registered services.\n");
            return;
        }
        print("Services (");
        print_u64(registry.count as u64);
        print("):\n");
        let mut i = 0;
        while i < MAX_SERVICES {
            if registry.services[i].active {
                let svc = &registry.services[i];
                print("  ");
                console_write(&svc.name[..svc.name_len]);
                if svc.pid != 0 {
                    if svc.ready {
                        print("  [ready, PID ");
                    } else {
                        print("  [running, PID ");
                    }
                    print_u64(svc.pid);
                    print("]");
                } else if svc.waiting_on_deps {
                    print("  [waiting on deps]");
                } else if svc.restart_after_ns > 0 {
                    print("  [pending restart]");
                } else {
                    print("  [stopped]");
                }
                if !svc.auto_restart {
                    print("  (no-restart)");
                }
                if svc.crash_count > 0 {
                    print("  crashes=");
                    print_u64(svc.crash_count);
                }
                print("\n");
            }
            i += 1;
        }
    } else if bytes_eq(sub, b"status") {
        if rest.is_empty() {
            print("svc status: missing service name\n");
            return;
        }
        match registry.find_by_name(rest) {
            Some(idx) => {
                let svc = &registry.services[idx];
                print("Service: ");
                console_write(&svc.name[..svc.name_len]);
                print("\n  Path:     ");
                console_write(&svc.path[..svc.path_len]);
                print("\n  PID:      ");
                if svc.pid != 0 {
                    print_u64(svc.pid);
                } else {
                    print("(not running)");
                }
                print("\n  Ready:    ");
                if svc.ready { print("yes"); } else { print("no"); }
                print("\n  Deps:     ");
                if svc.dep_count == 0 {
                    print("(none)");
                } else {
                    let mut d = 0;
                    while d < svc.dep_count {
                        if d > 0 { print(", "); }
                        console_write(&svc.dep_names[d][..svc.dep_name_lens[d]]);
                        d += 1;
                    }
                }
                if svc.waiting_on_deps {
                    print(" [waiting]");
                }
                print("\n  Args:     ");
                if svc.svc_arg_count == 0 {
                    print("(none)");
                } else {
                    let mut a = 0;
                    while a < svc.svc_arg_count {
                        if a > 0 { print(" "); }
                        console_write(&svc.svc_args[a][..svc.svc_arg_lens[a]]);
                        a += 1;
                    }
                }
                print("\n  Env:      ");
                if svc.svc_env_count == 0 {
                    print("(default)");
                } else {
                    let mut e = 0;
                    while e < svc.svc_env_count {
                        if e > 0 { print(", "); }
                        console_write(&svc.svc_env[e][..svc.svc_env_lens[e]]);
                        e += 1;
                    }
                }
                print("\n  Restart:  ");
                if svc.auto_restart { print("yes"); } else { print("no"); }
                print("\n  Crashes:  ");
                print_u64(svc.crash_count);
                print("\n  Backoff:  ");
                print_u64(svc.backoff_ns / 1_000_000_000);
                print("s\n");
            }
            None => {
                print("svc status: unknown service '");
                console_write(rest);
                print("'\n");
            }
        }
    } else if bytes_eq(sub, b"cfg") {
        // Reload /etc/services — register and start any new entries.
        // Already-running services are not duplicated because register
        // doesn't check for duplicates (the user should stop first).
        print("[svc] Reloading /etc/services...\n");
        load_startup_services(registry);
    } else {
        print("svc: unknown subcommand '");
        console_write(sub);
        print("'\nUsage: svc start|stop|restart|remove|list|status|cfg\n");
    }
}

/// Execute a command line.
///
/// The `registry` parameter allows commands (especially `svc`) to
/// interact with the service manager state.
fn execute(line: &[u8], registry: &mut ServiceRegistry) {
    let trimmed = trim(line);
    if trimmed.is_empty() {
        return;
    }

    let (cmd, args) = split_first_word(trimmed);

    if bytes_eq(cmd, b"help") {
        print("Available commands:\n");
        print("  help           - show this message\n");
        print("  echo <text>    - print text\n");
        print("  ls [path]      - list directory\n");
        print("  cat <path>     - print file contents\n");
        print("  write <p> <d>  - write data to file\n");
        print("  stat <path>    - show file metadata\n");
        print("  mkdir <path>   - create directory\n");
        print("  rmdir <path>   - remove empty directory\n");
        print("  rm <path>      - delete a file\n");
        print("  spawn <p> [args] - run an ELF program (blocking)\n");
        print("  svc start <p>  - register & start a service\n");
        print("  svc stop <n>   - stop a service\n");
        print("  svc restart <n>- restart a service\n");
        print("  svc remove <n> - unregister a service\n");
        print("  svc list       - list all services\n");
        print("  svc status <n> - detailed service info\n");
        print("  svc cfg        - reload /etc/services\n");
        print("  pid            - show task ID\n");
        print("  uptime         - show time since boot\n");
        print("  logs           - show kernel log entries\n");
        print("  exit           - shut down\n");
    } else if bytes_eq(cmd, b"exit") {
        print("Goodbye.\n");
        exit(0);
    } else if bytes_eq(cmd, b"echo") {
        if !args.is_empty() {
            console_write(args);
        }
        print("\n");
    } else if bytes_eq(cmd, b"ls") {
        cmd_ls(args);
    } else if bytes_eq(cmd, b"cat") {
        cmd_cat(args);
    } else if bytes_eq(cmd, b"write") {
        cmd_write(args);
    } else if bytes_eq(cmd, b"stat") {
        cmd_stat(args);
    } else if bytes_eq(cmd, b"mkdir") {
        cmd_mkdir(args);
    } else if bytes_eq(cmd, b"rmdir") {
        cmd_rmdir(args);
    } else if bytes_eq(cmd, b"rm") {
        cmd_rm(args);
    } else if bytes_eq(cmd, b"spawn") {
        cmd_spawn(args);
    } else if bytes_eq(cmd, b"svc") {
        cmd_svc(args, registry);
    } else if bytes_eq(cmd, b"pid") {
        let tid = task_id();
        print("Task ID: ");
        print_i64(tid);
        print("\n");
    } else if bytes_eq(cmd, b"uptime") {
        let ns = clock_monotonic();
        if ns < 0 {
            print("uptime: clock error\n");
        } else {
            let secs = (ns as u64) / 1_000_000_000;
            let ms = ((ns as u64) % 1_000_000_000) / 1_000_000;
            print("Uptime: ");
            print_u64(secs);
            print(".");
            // Zero-pad ms to 3 digits.
            if ms < 100 { print("0"); }
            if ms < 10 { print("0"); }
            print_u64(ms);
            print("s\n");
        }
    } else if bytes_eq(cmd, b"logs") {
        cmd_logs();
    } else {
        print("Unknown command: ");
        console_write(cmd);
        print("\nType 'help' for available commands.\n");
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Poll interval for the main loop when no services are registered.
/// 50 ms in nanoseconds — long enough to avoid busy-waiting, short
/// enough to feel responsive for keyboard input.
const POLL_INTERVAL_IDLE_NS: u64 = 50_000_000;

/// Poll interval when services are registered and may need monitoring.
/// 100 ms — balance between responsiveness and CPU usage.
const POLL_INTERVAL_ACTIVE_NS: u64 = 100_000_000;

/// Load the startup service list from `/etc/services`.
///
/// Format (one service per line):
/// ```text
/// # Comment lines start with '#'
/// /bin/logger
/// /bin/logger args:--verbose,-d
/// /bin/network args:--dhcp depends:logger
/// /bin/webserver args:--port,8080 env:PORT=8080 depends:logger,network
/// ```
///
/// Each non-empty, non-comment line is a path optionally followed by
/// keyword sections:
///   - `args:a,b,c` — extra arguments (argv[1..])
///   - `env:K=V,K2=V2` — per-service environment variables
///   - `depends:svc1,svc2` — dependency names
///
/// Keywords can appear in any order.  Services with no dependencies
/// are started immediately.  Services with dependencies are queued
/// and started automatically once all deps have signaled ready.
///
/// If the file doesn't exist, this is a no-op.
fn load_startup_services(registry: &mut ServiceRegistry) {
    let mut buf = [0u8; 2048];
    let result = fs_read_file(b"/etc/services", &mut buf);
    if result < 0 {
        // File not found or other error — silently skip.
        return;
    }

    let data = &buf[..result as usize];
    print("[init] Loading startup services from /etc/services\n");

    // Parse line by line.  Lines are separated by '\n'.
    let mut start = 0;
    while start < data.len() {
        // Find end of line.
        let mut end = start;
        while end < data.len() && data[end] != b'\n' {
            end += 1;
        }

        let line = trim(&data[start..end]);

        // Skip empty lines and comments.
        if !line.is_empty() && line[0] != b'#' {
            let parsed = parse_service_line(line);

            match registry.register(parsed.path) {
                Some(idx) => {
                    if !parsed.deps.is_empty() {
                        registry.set_dependencies(idx, parsed.deps);
                    }
                    if !parsed.args.is_empty() {
                        registry.set_arguments(idx, parsed.args);
                    }
                    if !parsed.env.is_empty() {
                        registry.set_env(idx, parsed.env);
                    }

                    if registry.services[idx].dep_count > 0 {
                        // Has dependencies — don't start yet, mark as waiting.
                        registry.services[idx].waiting_on_deps = true;
                        print("[init] ");
                        console_write(parsed.path);
                        print(" waiting on dependencies\n");
                    } else {
                        // No dependencies — start immediately.
                        registry.start_service(idx);
                    }
                }
                None => {
                    print("[init] Warning: could not register ");
                    console_write(parsed.path);
                    print(" (registry full?)\n");
                }
            }
        }

        start = end + 1;
    }
}

/// Parsed fields from a service config line.
struct ServiceLine<'a> {
    path: &'a [u8],
    args: &'a [u8],
    env: &'a [u8],
    deps: &'a [u8],
}

/// Find a keyword prefix (`"keyword:"`) in `line` and return the
/// value after the colon (up to the next space or end of line) and
/// the line with that keyword section removed.  Used by
/// `parse_service_line` to extract `args:`, `env:`, `depends:`.
fn find_keyword<'a>(line: &'a [u8], keyword: &[u8]) -> (Option<&'a [u8]>, &'a [u8]) {
    let kw_len = keyword.len();
    let mut i = 0;
    while i + kw_len <= line.len() {
        let mut matches = true;
        let mut k = 0;
        while k < kw_len {
            if line[i + k] != keyword[k] {
                matches = false;
                break;
            }
            k += 1;
        }
        if matches {
            // Find end of value (next space or end of line).
            let val_start = i + kw_len;
            let mut val_end = val_start;
            while val_end < line.len()
                && line[val_end] != b' '
                && line[val_end] != b'\t'
            {
                val_end += 1;
            }
            let value = trim(&line[val_start..val_end]);
            // Return the portion before this keyword as the remaining
            // line.  Keywords always appear after the path, so the
            // path is preserved.  We can't concatenate slices without
            // allocation, so each caller gets progressively shorter
            // remaining text — but since keywords don't overlap, this
            // correctly isolates the path.
            let before = trim(&line[..i]);
            return (Some(value), before);
        }
        i += 1;
    }
    (None, line)
}

/// Parse a service config line into components.
///
/// Format: `/path/to/binary [args:a,b,c] [env:K=V,K2=V2] [depends:svc1,svc2]`
///
/// Keywords can appear in any order after the path.  Values after
/// each keyword run until the next whitespace.
///
/// Examples:
/// ```text
/// /bin/logger
/// /bin/logger args:--verbose
/// /bin/network args:--dhcp depends:logger
/// /bin/webserver args:--port,8080 env:PORT=8080 depends:logger,network
/// ```
fn parse_service_line(line: &[u8]) -> ServiceLine<'_> {
    let mut remaining = line;
    let mut deps: &[u8] = &[];
    let mut args: &[u8] = &[];
    let mut env: &[u8] = &[];

    // Extract depends: keyword.
    let (d, rest) = find_keyword(remaining, b"depends:");
    if let Some(d_val) = d {
        deps = d_val;
        remaining = rest;
    }

    // Extract args: keyword.
    let (a, rest) = find_keyword(remaining, b"args:");
    if let Some(a_val) = a {
        args = a_val;
        remaining = rest;
    }

    // Extract env: keyword.
    let (e, rest) = find_keyword(remaining, b"env:");
    if let Some(e_val) = e {
        env = e_val;
        remaining = rest;
    }

    // What's left (trimmed) is the path.
    let path = trim(remaining);

    ServiceLine { path, args, env, deps }
}

/// Process entry point.  Called by the kernel via IRETQ to ring 3.
///
/// No Rust runtime is available — no heap, no std, no arguments.
/// We communicate with the kernel exclusively via SYSCALL.
///
/// The main loop is poll-based: it alternates between checking for
/// keyboard input (non-blocking) and monitoring registered services.
/// When no keys are pressed and no services need attention, it sleeps
/// briefly to avoid burning CPU.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // Welcome banner.
    print("\n");
    print("=======================================\n");
    print("  Welcome to the OS!\n");
    print("  Userspace init process (PID 1)\n");
    print("=======================================\n");
    print("\n");

    let mut registry = ServiceRegistry::new();

    // Auto-start services from /etc/services (if it exists).
    load_startup_services(&mut registry);

    print("Type 'help' for available commands.\n\n");
    let mut line_buf = [0u8; MAX_LINE];
    let mut line_pos: usize = 0;
    let mut prompt_shown = false;

    loop {
        // 1. Show prompt if we haven't yet.
        if !prompt_shown {
            print("user> ");
            prompt_shown = true;
        }

        // 2. Poll for keyboard input (non-blocking).
        //    Drain all available characters in a burst to avoid
        //    missing fast typists.
        let mut got_input = false;
        loop {
            match try_read_char() {
                Some(ch) => {
                    got_input = true;
                    match ch {
                        // Enter — execute the command line.
                        b'\r' | b'\n' => {
                            print("\n");
                            if line_pos > 0 {
                                execute(&line_buf[..line_pos], &mut registry);
                            }
                            line_pos = 0;
                            prompt_shown = false;
                            // Break out of input drain to re-show prompt.
                            break;
                        }

                        // Backspace / DEL.
                        0x08 | 0x7F => {
                            if line_pos > 0 {
                                line_pos -= 1;
                                console_write(b"\x08 \x08");
                            }
                        }

                        // Printable ASCII.
                        0x20..=0x7E => {
                            if line_pos < MAX_LINE {
                                line_buf[line_pos] = ch;
                                line_pos += 1;
                                console_write(&[ch]);
                            }
                        }

                        // Non-printable: ignore.
                        _ => {}
                    }
                }
                None => break, // No more characters in buffer.
            }
        }

        // 3. Poll registered services for crashes / pending restarts.
        if registry.count > 0 {
            registry.poll();
        }

        // 4. If nothing happened this iteration, sleep briefly to
        //    yield the CPU.  Use a shorter sleep when the user might
        //    be typing (no services) vs. when we're monitoring.
        if !got_input {
            let interval = if registry.count > 0 {
                POLL_INTERVAL_ACTIVE_NS
            } else {
                POLL_INTERVAL_IDLE_NS
            };
            sleep_ns(interval);
        }
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

/// Panic handler — print message to console and exit with error.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = info;
    print("!!! PANIC in init process !!!\n");
    exit(-1);
}
