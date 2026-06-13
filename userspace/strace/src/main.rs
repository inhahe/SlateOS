//! SlateOS Syscall Trace Utility
//!
//! Traces system calls made by a target process.  Uses kernel trace
//! syscalls (`SYS_TRACE_ENABLE` / `SYS_TRACE_READ`) to enable per-PID
//! tracing and read back binary trace entries.  Falls back to reading
//! `/proc/<pid>/syscall_trace` if the kernel syscalls are unavailable.
//!
//! # Usage
//!
//! ```text
//! strace -p <pid>              Attach to a running process
//! strace <command> [args...]   Run a command and trace it
//! strace -o <file> -p <pid>    Write trace to a file
//! strace -c -p <pid>           Summary only (call counts and times)
//! strace -e trace=open,read    Filter specific syscalls
//! strace -T -p <pid>           Show time spent in each syscall
//! strace --json -p <pid>       JSON output
//! ```
//!
//! # Trace entry format
//!
//! The kernel writes fixed-size `TraceEntry` structs into a ring buffer.
//! Each entry records the syscall number, PID, arguments, return value,
//! timestamp, and duration.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;
use std::time::Duration;

// ============================================================================
// Syscall numbers for kernel trace interface
// ============================================================================

/// Enable/disable syscall tracing for a target PID.
/// arg1 = target_pid, arg2 = enable(1) / disable(0).
const SYS_TRACE_ENABLE: u64 = 520;

/// Read trace entries from the kernel ring buffer.
/// arg1 = target_pid, arg2 = buf_ptr, arg3 = buf_len.
/// Returns bytes read.
const SYS_TRACE_READ: u64 = 521;

// ============================================================================
// Inline syscall wrappers
// ============================================================================

/// Issue a 3-argument syscall via inline assembly.
///
/// This is the primary interface for kernel interaction from userspace.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: We are issuing a syscall with the correct ABI — nr in rax,
    // args in rdi/rsi/rdx.  The kernel validates all arguments and returns
    // a result or negative error code.  The caller is responsible for
    // ensuring the buffer pointers (if any) are valid.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a 2-argument syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall2(nr: u64, a1: u64, a2: u64) -> i64 {
    // SAFETY: Same ABI guarantees as syscall3; unused arg3 register (rdx)
    // is simply not loaded.
    unsafe { syscall3(nr, a1, a2, 0) }
}

// ============================================================================
// Trace entry — binary format from kernel
// ============================================================================

/// A single syscall trace event read from the kernel ring buffer.
///
/// Must match the kernel's `TraceEvent` layout exactly (C repr, no padding).
#[repr(C)]
#[derive(Clone, Copy)]
struct TraceEntry {
    /// Nanoseconds since boot (or TSC timestamp).
    timestamp_ns: u64,
    /// Syscall number.
    syscall_nr: u32,
    /// Calling PID.
    pid: u32,
    /// First syscall argument.
    arg1: u64,
    /// Second syscall argument.
    arg2: u64,
    /// Third syscall argument.
    arg3: u64,
    /// Syscall return value.
    result: i64,
    /// Time spent in the syscall (nanoseconds or TSC cycles).
    duration_ns: u64,
}

impl TraceEntry {
    const fn zeroed() -> Self {
        Self {
            timestamp_ns: 0,
            syscall_nr: 0,
            pid: 0,
            arg1: 0,
            arg2: 0,
            arg3: 0,
            result: 0,
            duration_ns: 0,
        }
    }
}

/// Size of one trace entry in bytes.
const TRACE_ENTRY_SIZE: usize = core::mem::size_of::<TraceEntry>();

// ============================================================================
// Syscall name table
// ============================================================================

/// Information about a syscall for display purposes.
struct SyscallInfo {
    name: &'static str,
    /// How to format the arguments (for pretty-printing).
    arg_format: ArgFormat,
}

/// Argument display style for different syscall types.
#[derive(Clone, Copy, PartialEq)]
enum ArgFormat {
    /// No arguments.
    None,
    /// Generic: show raw numeric arguments.
    Generic,
    /// First arg is a pointer to a string buffer, second is length.
    PtrLen,
    /// First arg is a fd/handle, second is pointer, third is length.
    FdPtrLen,
    /// Single integer argument.
    Int,
    /// Two integer arguments.
    IntInt,
    /// Pointer and size (e.g., mmap).
    PtrSize,
    /// Path-based (pointer + length for a path string).
    Path,
    /// Handle only.
    Handle,
}

/// Build the syscall name table covering all SlateOS syscall ranges.
///
/// Ranges:
///   0-199:   kernel-core (memory, scheduler, time, misc)
///   200-399: kernel-ipc (channels, pipes, shm, eventfd, completion ports)
///   400-499: kernel-security (capabilities, namespaces)
///   500-599: kernel-process (process/thread lifecycle)
///   600-799: filesystem (VFS, file I/O)
///   800-999: networking (TCP, UDP, DNS, ICMP)
///   1000+:   DRM/display
fn build_syscall_table() -> HashMap<u32, SyscallInfo> {
    let mut t = HashMap::new();

    // Helper to reduce repetition.
    macro_rules! sc {
        ($nr:expr, $name:expr, $fmt:expr) => {
            t.insert($nr, SyscallInfo { name: $name, arg_format: $fmt });
        };
    }

    // -- Core (0-199) --
    sc!(0,   "yield",                 ArgFormat::None);
    sc!(1,   "exit",                  ArgFormat::Int);
    sc!(2,   "task_id",               ArgFormat::None);
    sc!(10,  "clock_monotonic",       ArgFormat::None);
    sc!(11,  "sleep",                 ArgFormat::Int);
    sc!(12,  "timer_create",          ArgFormat::IntInt);
    sc!(13,  "timer_cancel",          ArgFormat::Handle);
    sc!(20,  "mmap",                  ArgFormat::PtrSize);
    sc!(21,  "munmap",                ArgFormat::PtrSize);
    sc!(30,  "irq_register",          ArgFormat::Int);
    sc!(31,  "irq_wait",              ArgFormat::Int);
    sc!(32,  "irq_release",           ArgFormat::Int);
    sc!(40,  "port_read",             ArgFormat::IntInt);
    sc!(41,  "port_write",            ArgFormat::Generic);
    sc!(42,  "dma_alloc",             ArgFormat::IntInt);
    sc!(43,  "dma_free",              ArgFormat::Handle);
    sc!(44,  "dma_domain_create",     ArgFormat::None);
    sc!(45,  "dma_domain_destroy",    ArgFormat::Handle);
    sc!(46,  "dma_map",               ArgFormat::Generic);
    sc!(47,  "dma_unmap",             ArgFormat::Generic);
    sc!(48,  "dma_attach",            ArgFormat::Generic);
    sc!(49,  "dma_detach",            ArgFormat::Generic);
    sc!(50,  "sched_set_timeslice",   ArgFormat::IntInt);
    sc!(51,  "sched_get_timeslice",   ArgFormat::Int);
    sc!(52,  "sched_reconfigure",     ArgFormat::IntInt);
    sc!(53,  "sched_set_profile",     ArgFormat::Int);
    sc!(54,  "sched_get_profile",     ArgFormat::None);
    sc!(60,  "sysctl_get",            ArgFormat::Int);
    sc!(61,  "sysctl_set",            ArgFormat::IntInt);
    sc!(70,  "mm_set_profile",        ArgFormat::Int);
    sc!(71,  "mm_get_profile",        ArgFormat::None);
    sc!(80,  "system_set_profile",    ArgFormat::Int);
    sc!(99,  "debug_print",           ArgFormat::PtrLen);
    sc!(100, "console_write",         ArgFormat::PtrLen);
    sc!(101, "console_read_char",     ArgFormat::Handle);
    sc!(102, "log_read",              ArgFormat::Generic);
    sc!(103, "console_try_read_char", ArgFormat::Handle);

    // -- IPC (200-399) --
    sc!(200, "channel_create",        ArgFormat::Int);
    sc!(201, "channel_send",          ArgFormat::FdPtrLen);
    sc!(202, "channel_recv",          ArgFormat::FdPtrLen);
    sc!(203, "channel_try_recv",      ArgFormat::FdPtrLen);
    sc!(204, "channel_close",         ArgFormat::Handle);
    sc!(205, "channel_recv_timeout",  ArgFormat::Generic);
    sc!(206, "channel_send_caps",     ArgFormat::Generic);
    sc!(207, "channel_recv_caps",     ArgFormat::Generic);
    sc!(208, "channel_send_timeout",  ArgFormat::Generic);
    sc!(209, "channel_send_blocking", ArgFormat::FdPtrLen);
    sc!(210, "futex_wait",            ArgFormat::PtrSize);
    sc!(211, "futex_wake",            ArgFormat::PtrSize);
    sc!(212, "futex_lock_pi",         ArgFormat::Handle);
    sc!(213, "futex_unlock_pi",       ArgFormat::Handle);
    sc!(214, "futex_wait_timeout",    ArgFormat::Generic);
    sc!(220, "pipe_create",           ArgFormat::None);
    sc!(221, "pipe_write",            ArgFormat::FdPtrLen);
    sc!(222, "pipe_read",             ArgFormat::FdPtrLen);
    sc!(223, "pipe_try_write",        ArgFormat::FdPtrLen);
    sc!(224, "pipe_try_read",         ArgFormat::FdPtrLen);
    sc!(225, "pipe_close",            ArgFormat::Handle);
    sc!(226, "pipe_read_timeout",     ArgFormat::Generic);
    sc!(227, "pipe_write_timeout",    ArgFormat::Generic);
    sc!(228, "pipe_poll",             ArgFormat::Handle);
    sc!(229, "pipe_readable_bytes",   ArgFormat::Handle);
    sc!(230, "shm_create",            ArgFormat::Int);
    sc!(231, "shm_size",              ArgFormat::Handle);
    sc!(232, "shm_close",             ArgFormat::Handle);
    sc!(240, "eventfd_create",        ArgFormat::Int);
    sc!(241, "eventfd_write",         ArgFormat::IntInt);
    sc!(242, "eventfd_read",          ArgFormat::Handle);
    sc!(243, "eventfd_try_read",      ArgFormat::Handle);
    sc!(244, "eventfd_close",         ArgFormat::Handle);
    sc!(245, "eventfd_read_timeout",  ArgFormat::IntInt);
    sc!(246, "eventfd_write_timeout", ArgFormat::Generic);
    sc!(250, "cp_create",             ArgFormat::None);
    sc!(251, "cp_register",           ArgFormat::Generic);
    sc!(252, "cp_unregister",         ArgFormat::Generic);
    sc!(253, "cp_wait",               ArgFormat::FdPtrLen);
    sc!(254, "cp_try_wait",           ArgFormat::FdPtrLen);
    sc!(255, "cp_close",              ArgFormat::Handle);
    sc!(256, "cp_notify",             ArgFormat::Generic);
    sc!(260, "io_ring_setup",         ArgFormat::IntInt);
    sc!(261, "io_ring_enter",         ArgFormat::IntInt);
    sc!(262, "io_ring_destroy",       ArgFormat::Handle);
    sc!(270, "sem_create",            ArgFormat::IntInt);
    sc!(271, "sem_signal",            ArgFormat::IntInt);
    sc!(272, "sem_wait",              ArgFormat::Handle);
    sc!(273, "sem_try_wait",          ArgFormat::Handle);
    sc!(274, "sem_close",             ArgFormat::Handle);
    sc!(275, "sem_wait_timeout",      ArgFormat::IntInt);
    sc!(280, "service_register",      ArgFormat::PtrLen);
    sc!(281, "service_connect",       ArgFormat::PtrLen);
    sc!(282, "service_accept",        ArgFormat::Handle);
    sc!(283, "service_try_accept",    ArgFormat::Handle);
    sc!(284, "service_accept_timeout", ArgFormat::IntInt);
    sc!(285, "service_unregister",    ArgFormat::Handle);
    sc!(290, "ns_create",             ArgFormat::Int);
    sc!(291, "ns_bind",               ArgFormat::Generic);
    sc!(292, "ns_unbind",             ArgFormat::Generic);
    sc!(293, "ns_hide",               ArgFormat::Generic);
    sc!(294, "ns_attach",             ArgFormat::IntInt);
    sc!(295, "ns_query",              ArgFormat::Int);

    // -- Security (400-499) --
    sc!(400, "cap_query",             ArgFormat::None);
    sc!(401, "cap_request",           ArgFormat::Generic);
    sc!(402, "cap_request_status",    ArgFormat::Int);
    sc!(403, "cap_request_cancel",    ArgFormat::Int);

    // -- Process (500-599) --
    sc!(500, "process_spawn",         ArgFormat::Path);
    sc!(501, "process_wait",          ArgFormat::Int);
    sc!(502, "process_id",            ArgFormat::None);
    sc!(503, "process_exec",          ArgFormat::Generic);
    sc!(504, "set_exception_handler", ArgFormat::Handle);
    sc!(505, "exception_return",      ArgFormat::Handle);
    sc!(506, "process_kill",          ArgFormat::IntInt);
    sc!(507, "process_try_wait",      ArgFormat::Int);
    sc!(508, "notify_ready",          ArgFormat::None);
    sc!(509, "process_is_ready",      ArgFormat::Int);
    sc!(510, "thread_create",         ArgFormat::Generic);
    sc!(511, "thread_exit",           ArgFormat::Int);
    sc!(512, "thread_join",           ArgFormat::Int);
    sc!(513, "thread_suspend",        ArgFormat::Int);
    sc!(514, "thread_resume",         ArgFormat::Int);
    sc!(515, "thread_set_priority",   ArgFormat::IntInt);
    sc!(516, "process_crash_info",    ArgFormat::IntInt);
    sc!(520, "trace_enable",          ArgFormat::IntInt);
    sc!(521, "trace_read",            ArgFormat::FdPtrLen);

    // -- Filesystem (600-799) --
    sc!(600, "fs_read_file",          ArgFormat::Path);
    sc!(601, "fs_write_file",         ArgFormat::Path);
    sc!(602, "fs_delete",             ArgFormat::Path);
    sc!(603, "fs_list_dir",           ArgFormat::Path);
    sc!(604, "fs_mkdir",              ArgFormat::Path);
    sc!(605, "fs_rmdir",              ArgFormat::Path);
    sc!(606, "fs_stat",               ArgFormat::Path);
    sc!(607, "fs_link",               ArgFormat::Generic);
    sc!(608, "fs_statvfs",            ArgFormat::Path);
    sc!(609, "fs_flock",              ArgFormat::IntInt);
    sc!(610, "fs_open",               ArgFormat::Path);
    sc!(611, "fs_close",              ArgFormat::Handle);
    sc!(612, "fs_read",               ArgFormat::FdPtrLen);
    sc!(613, "fs_write",              ArgFormat::FdPtrLen);
    sc!(614, "fs_seek",               ArgFormat::IntInt);
    sc!(615, "fs_truncate",           ArgFormat::IntInt);
    sc!(616, "fs_rename",             ArgFormat::Generic);
    sc!(617, "fs_fstat",              ArgFormat::Handle);
    sc!(618, "fs_trash",              ArgFormat::Path);
    sc!(619, "fs_trash_list",         ArgFormat::FdPtrLen);
    sc!(620, "fs_trash_restore",      ArgFormat::Path);
    sc!(621, "fs_trash_empty",        ArgFormat::None);
    sc!(622, "fs_watch_create",       ArgFormat::Path);
    sc!(623, "fs_watch_read",         ArgFormat::FdPtrLen);
    sc!(624, "fs_watch_close",        ArgFormat::Handle);
    sc!(625, "fs_journal_cursor",     ArgFormat::Generic);
    sc!(626, "fs_journal_read",       ArgFormat::Generic);
    sc!(627, "fs_journal_flush",      ArgFormat::Handle);
    sc!(628, "fs_metadata",           ArgFormat::Path);
    sc!(629, "fs_set_attr",           ArgFormat::Generic);
    sc!(630, "fs_set_owner",          ArgFormat::Generic);
    sc!(631, "fs_set_perms",          ArgFormat::Generic);
    sc!(632, "fs_set_times",          ArgFormat::Generic);
    sc!(633, "fs_get_xattr",          ArgFormat::Generic);
    sc!(634, "fs_set_xattr",          ArgFormat::Generic);
    sc!(635, "fs_remove_xattr",       ArgFormat::Generic);
    sc!(636, "fs_list_xattrs",        ArgFormat::Generic);
    sc!(637, "fs_symlink",            ArgFormat::Generic);
    sc!(640, "fs_funlock",            ArgFormat::Handle);
    sc!(641, "fs_sync",               ArgFormat::Handle);
    sc!(642, "fs_copy",               ArgFormat::Generic);
    sc!(643, "fs_append",             ArgFormat::FdPtrLen);
    sc!(644, "fs_ftruncate",          ArgFormat::IntInt);
    sc!(645, "fs_dup",                ArgFormat::Handle);
    sc!(646, "fs_handle_path",        ArgFormat::FdPtrLen);
    sc!(647, "fs_readdir_at",         ArgFormat::Generic);
    sc!(648, "fs_tmpfile",            ArgFormat::Path);
    sc!(649, "fs_fallocate",          ArgFormat::Generic);
    sc!(650, "fs_seek_data",          ArgFormat::IntInt);
    sc!(651, "fs_seek_hole",          ArgFormat::IntInt);

    // -- Networking (800-899) --
    sc!(800, "tcp_connect",           ArgFormat::Generic);
    sc!(801, "tcp_send",              ArgFormat::FdPtrLen);
    sc!(802, "tcp_recv",              ArgFormat::FdPtrLen);
    sc!(803, "tcp_close",             ArgFormat::Handle);
    sc!(804, "tcp_bind",              ArgFormat::Generic);
    sc!(805, "tcp_accept",            ArgFormat::Handle);
    sc!(806, "tcp_close_listener",    ArgFormat::Handle);
    sc!(807, "tcp_abort",             ArgFormat::Handle);
    sc!(808, "tcp_peer_addr",         ArgFormat::Handle);
    sc!(810, "udp_bind",              ArgFormat::Generic);
    sc!(811, "udp_send",              ArgFormat::Generic);
    sc!(812, "udp_recv",              ArgFormat::FdPtrLen);
    sc!(813, "udp_close",             ArgFormat::Handle);
    sc!(814, "udp_mcast_join",        ArgFormat::Generic);
    sc!(815, "udp_mcast_leave",       ArgFormat::Generic);
    sc!(816, "udp_connect",           ArgFormat::Generic);
    sc!(817, "udp_local_port",        ArgFormat::Handle);
    sc!(820, "dns_resolve",           ArgFormat::PtrLen);
    sc!(821, "dns_reverse_resolve",   ArgFormat::Generic);
    sc!(825, "net_stat",              ArgFormat::Generic);
    sc!(830, "icmp_ping",             ArgFormat::Generic);
    sc!(831, "icmp_ping_wait",        ArgFormat::Generic);
    sc!(840, "tcp_list",              ArgFormat::FdPtrLen);
    sc!(841, "tcp_listener_list",     ArgFormat::FdPtrLen);
    sc!(842, "net_if_info",           ArgFormat::FdPtrLen);
    sc!(843, "arp_table",             ArgFormat::FdPtrLen);
    sc!(844, "dns_cache_stats",       ArgFormat::Generic);
    sc!(845, "tcp_poll_status",       ArgFormat::Handle);
    sc!(846, "tcp_listener_ready",    ArgFormat::Handle);
    sc!(847, "udp_rx_ready",          ArgFormat::Handle);
    sc!(848, "udp_rx_front_bytes",    ArgFormat::Handle);
    sc!(849, "tcp_info",              ArgFormat::Handle);
    sc!(850, "tcp_set_nodelay",       ArgFormat::IntInt);
    sc!(851, "tcp_set_keepalive",     ArgFormat::IntInt);
    sc!(852, "tcp_set_keepalive_params", ArgFormat::Generic);
    sc!(853, "tcp_last_error",        ArgFormat::Handle);
    sc!(854, "tcp_local_port",        ArgFormat::Handle);
    sc!(855, "tcp_shutdown",          ArgFormat::IntInt);

    // -- DRM/Display (1000+) --
    sc!(1000, "drm_open",             ArgFormat::None);
    sc!(1001, "drm_close",            ArgFormat::Handle);
    sc!(1002, "drm_display_size",     ArgFormat::Handle);
    sc!(1010, "drm_gem_create",       ArgFormat::IntInt);
    sc!(1011, "drm_gem_destroy",      ArgFormat::Handle);
    sc!(1012, "drm_gem_mmap",         ArgFormat::Handle);
    sc!(1020, "drm_fb_create",        ArgFormat::Generic);
    sc!(1021, "drm_fb_destroy",       ArgFormat::Handle);
    sc!(1030, "drm_page_flip",        ArgFormat::IntInt);
    sc!(1031, "drm_flush_region",     ArgFormat::Generic);
    sc!(1040, "drm_connector_status", ArgFormat::Handle);
    sc!(1041, "drm_mode_get",         ArgFormat::Handle);
    sc!(1042, "drm_crtc_info",        ArgFormat::Handle);

    t
}

/// Syscall category groups for `-e trace=<group>` filtering.
fn syscall_group(name: &str) -> Vec<u32> {
    match name {
        "file" | "fs" => {
            let mut v: Vec<u32> = (600..=651).collect();
            // Include core file ops that might be relevant.
            v.extend([610, 611, 612, 613, 614, 615, 616, 617]);
            v.sort_unstable();
            v.dedup();
            v
        }
        "network" | "net" => (800..=855).collect(),
        "ipc" => (200..=295).collect(),
        "process" | "proc" => (500..=521).collect(),
        "memory" | "mem" => vec![20, 21, 42, 43, 44, 45, 46, 47, 48, 49],
        "signal" | "security" => (400..=403).collect(),
        "drm" | "display" => (1000..=1042).collect(),
        _ => Vec::new(),
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Runtime configuration parsed from command-line arguments.
struct Config {
    /// PID to attach to (if tracing an existing process).
    attach_pid: Option<u32>,
    /// Command to spawn and trace (if not attaching).
    command: Vec<String>,
    /// Output file path (None = stderr).
    output_file: Option<String>,
    /// Show summary only (call counts, times, errors).
    summary_only: bool,
    /// Show summary alongside trace output.
    summary_with_trace: bool,
    /// Syscall number filter (empty = trace all).
    filter_syscalls: Vec<u32>,
    /// Show time spent in each syscall.
    show_duration: bool,
    /// Show wall-clock timestamp per line.
    show_timestamp: bool,
    /// Show wall-clock timestamp with microseconds.
    show_timestamp_us: bool,
    /// Show relative timestamps.
    relative_timestamps: bool,
    /// Trace child processes too.
    follow_forks: bool,
    /// JSON output mode.
    json_output: bool,
    /// Verbose mode (show full struct contents).
    verbose: bool,
}

impl Config {
    fn new() -> Self {
        Self {
            attach_pid: None,
            command: Vec::new(),
            output_file: None,
            summary_only: false,
            summary_with_trace: false,
            filter_syscalls: Vec::new(),
            show_duration: false,
            show_timestamp: false,
            show_timestamp_us: false,
            relative_timestamps: false,
            follow_forks: false,
            json_output: false,
            verbose: false,
        }
    }

    /// Whether trace output lines should be printed (not just summary).
    fn show_trace(&self) -> bool {
        !self.summary_only || self.summary_with_trace
    }
}

// ============================================================================
// Summary statistics
// ============================================================================

/// Per-syscall statistics for summary mode (-c / -C).
struct SyscallStats {
    calls: u64,
    errors: u64,
    total_time_ns: u64,
}

impl SyscallStats {
    const fn new() -> Self {
        Self {
            calls: 0,
            errors: 0,
            total_time_ns: 0,
        }
    }
}

// ============================================================================
// Output formatting
// ============================================================================

/// Format a syscall entry as a human-readable trace line (Linux strace style).
///
/// Example: `read(3, "Hello, world!\n", 4096) = 14`
fn format_trace_line(
    entry: &TraceEntry,
    table: &HashMap<u32, SyscallInfo>,
    config: &Config,
    first_timestamp: u64,
    prev_timestamp: &mut u64,
) -> String {
    let mut line = String::new();

    // Timestamp prefix.
    if config.relative_timestamps {
        let delta_ns = entry.timestamp_ns.saturating_sub(*prev_timestamp);
        let delta_s = delta_ns as f64 / 1_000_000_000.0;
        line.push_str(&format!("{delta_s:>12.6} "));
        *prev_timestamp = entry.timestamp_ns;
    } else if config.show_timestamp_us {
        // Wall-clock time with microseconds (relative to boot since we
        // don't have real wall-clock info).
        let secs = entry.timestamp_ns / 1_000_000_000;
        let us = (entry.timestamp_ns % 1_000_000_000) / 1_000;
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        line.push_str(&format!("{hours:02}:{mins:02}:{s:02}.{us:06} "));
    } else if config.show_timestamp {
        let secs = entry.timestamp_ns / 1_000_000_000;
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let s = secs % 60;
        line.push_str(&format!("{hours:02}:{mins:02}:{s:02} "));
    }

    // If follow_forks, show the PID prefix.
    if config.follow_forks {
        line.push_str(&format!("[pid {:>5}] ", entry.pid));
    }

    // Syscall name and arguments.
    let info = table.get(&entry.syscall_nr);
    let name = info.map_or("unknown", |i| i.name);
    let arg_fmt = info.map_or(ArgFormat::Generic, |i| i.arg_format);

    line.push_str(name);
    line.push('(');
    format_args_into(&mut line, entry, arg_fmt, config.verbose);
    line.push(')');

    // Return value.
    let _ = first_timestamp; // Used indirectly via relative timestamps.
    if entry.syscall_nr == 1 {
        // exit() never returns.
        line.push_str(" = ?");
    } else if entry.result < 0 {
        line.push_str(&format!(" = -1 (err {})", -entry.result));
    } else {
        line.push_str(&format!(" = {}", entry.result));
    }

    // Duration suffix (-T).
    if config.show_duration {
        let dur_s = entry.duration_ns as f64 / 1_000_000_000.0;
        line.push_str(&format!(" <{dur_s:.6}>"));
    }

    line
}

/// Append formatted arguments to a string based on the argument format type.
fn format_args_into(out: &mut String, entry: &TraceEntry, fmt: ArgFormat, verbose: bool) {
    match fmt {
        ArgFormat::None => {}
        ArgFormat::Int => {
            out.push_str(&format!("{}", entry.arg1));
        }
        ArgFormat::IntInt => {
            out.push_str(&format!("{}, {}", entry.arg1, entry.arg2));
        }
        ArgFormat::Handle => {
            out.push_str(&format!("{}", entry.arg1));
        }
        ArgFormat::PtrLen => {
            if verbose {
                out.push_str(&format!("0x{:x}, {}", entry.arg1, entry.arg2));
            } else {
                // Try to show a shortened string representation.
                out.push_str(&format_ptr_as_string(entry.arg1, entry.arg2));
            }
        }
        ArgFormat::FdPtrLen => {
            out.push_str(&format!("{}, ", entry.arg1));
            if verbose {
                out.push_str(&format!("0x{:x}, {}", entry.arg2, entry.arg3));
            } else {
                out.push_str(&format_ptr_as_string(entry.arg2, entry.arg3));
            }
        }
        ArgFormat::PtrSize => {
            out.push_str(&format!("0x{:x}, {}", entry.arg1, entry.arg2));
        }
        ArgFormat::Path => {
            // Path args: pointer + length in arg1/arg2.
            if verbose {
                out.push_str(&format!("0x{:x}, {}", entry.arg1, entry.arg2));
            } else {
                out.push_str(&format_ptr_as_string(entry.arg1, entry.arg2));
            }
        }
        ArgFormat::Generic => {
            // Show all three args as hex/dec.
            if entry.arg3 != 0 {
                out.push_str(&format!(
                    "0x{:x}, 0x{:x}, 0x{:x}",
                    entry.arg1, entry.arg2, entry.arg3
                ));
            } else if entry.arg2 != 0 {
                out.push_str(&format!("0x{:x}, 0x{:x}", entry.arg1, entry.arg2));
            } else if entry.arg1 != 0 {
                out.push_str(&format!("0x{:x}", entry.arg1));
            }
        }
    }
}

/// Format a (pointer, length) pair as a string preview.
///
/// Since we can't read the traced process's memory directly, we show the
/// pointer and length.  In a real implementation with /proc/<pid>/mem, we
/// could read the actual string contents.
fn format_ptr_as_string(ptr: u64, len: u64) -> String {
    if ptr == 0 {
        "NULL".to_string()
    } else if len == 0 {
        format!("0x{ptr:x}, 0")
    } else {
        // Truncate displayed length for readability.
        let display_len = len.min(256);
        format!("0x{ptr:x}, {display_len}")
    }
}

/// Format a trace entry as JSON.
fn format_trace_json(
    entry: &TraceEntry,
    table: &HashMap<u32, SyscallInfo>,
    config: &Config,
) -> String {
    let name = table
        .get(&entry.syscall_nr)
        .map_or("unknown", |i| i.name);

    // Escape the name for JSON (syscall names are ASCII, so this is safe).
    let escaped_name = name.replace('\\', "\\\\").replace('"', "\\\"");

    let mut json = format!(
        "{{\"timestamp_ns\":{},\"pid\":{},\"syscall\":\"{}\",\"nr\":{},\"args\":[{},{},{}],\"result\":{}",
        entry.timestamp_ns,
        entry.pid,
        escaped_name,
        entry.syscall_nr,
        entry.arg1,
        entry.arg2,
        entry.arg3,
        entry.result,
    );

    if config.show_duration {
        json.push_str(&format!(",\"duration_ns\":{}", entry.duration_ns));
    }

    json.push('}');
    json
}

/// Print the summary table (like strace -c).
fn print_summary(
    stats: &HashMap<u32, SyscallStats>,
    table: &HashMap<u32, SyscallInfo>,
    output: &mut dyn Write,
) {
    // Compute totals.
    let total_calls: u64 = stats.values().map(|s| s.calls).sum();
    let total_errors: u64 = stats.values().map(|s| s.errors).sum();
    let total_time: u64 = stats.values().map(|s| s.total_time_ns).sum();

    // Sort by time (descending).
    let mut entries: Vec<(u32, &SyscallStats)> = stats.iter().map(|(k, v)| (*k, v)).collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.1.total_time_ns));

    let _ = writeln!(output,
        "{:>6} {:>11} {:>11} {:>9} {:>9} {:>-16}",
        "% time", "seconds", "usecs/call", "calls", "errors", "syscall"
    );
    let _ = writeln!(output,
        "{:->6} {:->11} {:->11} {:->9} {:->9} {:->16}",
        "", "", "", "", "", ""
    );

    for (nr, stat) in &entries {
        let name = table.get(nr).map_or("unknown", |i| i.name);
        let pct = if total_time > 0 {
            (stat.total_time_ns as f64 / total_time as f64) * 100.0
        } else {
            0.0
        };
        let secs = stat.total_time_ns as f64 / 1_000_000_000.0;
        let usecs_per_call = if stat.calls > 0 {
            stat.total_time_ns / (stat.calls * 1_000)
        } else {
            0
        };

        let errors_str = if stat.errors > 0 {
            format!("{}", stat.errors)
        } else {
            String::new()
        };

        let _ = writeln!(output,
            "{pct:>5.2}% {secs:>11.6} {usecs_per_call:>11} {:>9} {:>9} {name:<16}",
            stat.calls, errors_str
        );
    }

    let _ = writeln!(output,
        "{:->6} {:->11} {:->11} {:->9} {:->9} {:->16}",
        "", "", "", "", "", ""
    );

    let total_secs = total_time as f64 / 1_000_000_000.0;
    let _ = writeln!(output,
        "100.00% {total_secs:>11.6} {:>11} {total_calls:>9} {total_errors:>9} total",
        ""
    );
}

// ============================================================================
// Kernel trace interface
// ============================================================================

/// Enable tracing for a target PID via the kernel syscall.
///
/// Returns true if the syscall succeeded.
fn trace_enable(pid: u32, enable: bool) -> bool {
    let enable_val: u64 = if enable { 1 } else { 0 };
    // SAFETY: SYS_TRACE_ENABLE takes a PID and an enable flag.
    // The kernel validates both arguments.  No memory pointers are passed.
    let ret = unsafe { syscall2(SYS_TRACE_ENABLE, u64::from(pid), enable_val) };
    ret >= 0
}

/// Read trace entries from the kernel.
///
/// Returns the number of entries read, or 0 on failure.
fn trace_read(pid: u32, buf: &mut [TraceEntry]) -> usize {
    if buf.is_empty() {
        return 0;
    }
    let buf_ptr = buf.as_mut_ptr() as u64;
    let buf_len = (buf.len() * TRACE_ENTRY_SIZE) as u64;
    // SAFETY: We pass a valid mutable buffer pointer and its byte length.
    // The kernel writes TraceEntry structs into the buffer and returns the
    // number of bytes written.  The buffer is properly aligned (TraceEntry
    // has natural alignment from #[repr(C)]).
    let bytes_read = unsafe {
        syscall3(SYS_TRACE_READ, u64::from(pid), buf_ptr, buf_len)
    };
    if bytes_read < 0 {
        return 0;
    }
    (bytes_read as usize) / TRACE_ENTRY_SIZE
}

// ============================================================================
// Fallback: /proc/<pid>/syscall_trace
// ============================================================================

/// Try reading trace data from /proc/<pid>/syscall_trace as a fallback.
///
/// Returns parsed trace entries, or None if the file doesn't exist.
fn try_proc_trace(pid: u32) -> Option<Vec<TraceEntry>> {
    let path = format!("/proc/{pid}/syscall_trace");
    let data = fs::read(&path).ok()?;

    if data.len() < TRACE_ENTRY_SIZE {
        return Some(Vec::new());
    }

    let entry_count = data.len() / TRACE_ENTRY_SIZE;
    let mut entries = Vec::with_capacity(entry_count);

    for i in 0..entry_count {
        let offset = i * TRACE_ENTRY_SIZE;
        let slice = data.get(offset..offset + TRACE_ENTRY_SIZE)?;
        // SAFETY: TraceEntry is #[repr(C)] with all-numeric fields, no
        // padding requirements beyond natural alignment.  We copy from a
        // byte slice into a properly typed value.
        let entry: TraceEntry = unsafe {
            core::ptr::read_unaligned(slice.as_ptr().cast::<TraceEntry>())
        };
        if entry.timestamp_ns != 0 {
            entries.push(entry);
        }
    }

    Some(entries)
}

// ============================================================================
// Filter parsing
// ============================================================================

/// Parse a `-e` filter expression.
///
/// Supported forms:
/// - `trace=open,read,write` — filter by syscall name
/// - `trace=file` — filter by category (file, network, ipc, process, memory)
/// - `open,read,write` — shorthand without `trace=`
fn parse_filter(expr: &str, table: &HashMap<u32, SyscallInfo>) -> Vec<u32> {
    let names_str = expr.strip_prefix("trace=").unwrap_or(expr);
    let mut result = Vec::new();

    for name in names_str.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        // Check if it's a group name.
        let group = syscall_group(name);
        if !group.is_empty() {
            result.extend(group);
            continue;
        }

        // Look up by name.
        for (nr, info) in table {
            if info.name == name {
                result.push(*nr);
                break;
            }
        }
    }

    result.sort_unstable();
    result.dedup();
    result
}

// ============================================================================
// Main trace loop
// ============================================================================

/// Run the trace loop, reading and displaying syscall events.
fn run_trace(config: &Config) {
    let table = build_syscall_table();
    let mut stats: HashMap<u32, SyscallStats> = HashMap::new();

    // Determine the target PID.
    let target_pid = match config.attach_pid {
        Some(pid) => pid,
        None => {
            if config.command.is_empty() {
                eprintln!("strace: must specify -p <pid> or a command to trace");
                process::exit(1);
            }
            // Spawn the command and trace it.
            match spawn_and_trace(&config.command) {
                Some(pid) => pid,
                None => {
                    eprintln!(
                        "strace: failed to spawn command: {}",
                        config.command.first().map_or("(empty)", |s| s.as_str())
                    );
                    process::exit(1);
                }
            }
        }
    };

    // Open output file or use stderr.
    let mut output: Box<dyn Write> = if let Some(ref path) = config.output_file {
        match fs::File::create(path) {
            Ok(f) => Box::new(io::BufWriter::new(f)),
            Err(e) => {
                eprintln!("strace: cannot open output file '{path}': {e}");
                process::exit(1);
            }
        }
    } else {
        Box::new(io::BufWriter::new(io::stderr()))
    };

    // Try enabling kernel tracing.
    let kernel_tracing = trace_enable(target_pid, true);
    if !kernel_tracing {
        // Try /proc fallback.
        eprintln!(
            "strace: kernel trace syscalls not available, \
             trying /proc/{target_pid}/syscall_trace..."
        );

        match try_proc_trace(target_pid) {
            Some(entries) => {
                display_entries(
                    &entries, &table, config, &mut stats, &mut *output,
                );
                if config.summary_only || config.summary_with_trace {
                    let _ = writeln!(output);
                    print_summary(&stats, &table, &mut *output);
                }
                return;
            }
            None => {
                eprintln!(
                    "strace: cannot trace PID {target_pid}: \
                     kernel trace syscalls unavailable and \
                     /proc/{target_pid}/syscall_trace not found.\n\
                     Tracing requires kernel support (syscalls 520/521) \
                     or a /proc/PID/syscall_trace file."
                );
                process::exit(1);
            }
        }
    }

    // Kernel tracing is active.  Poll for events in a loop.
    let mut buf = [TraceEntry::zeroed(); 64];
    let mut total_events = 0u64;
    let poll_interval = Duration::from_millis(10);

    // Track whether the process is still alive.
    let mut process_alive = true;

    while process_alive {
        let count = trace_read(target_pid, &mut buf);

        if count > 0 {
            let entries: Vec<TraceEntry> = buf[..count]
                .iter()
                .filter(|e| {
                    config.filter_syscalls.is_empty()
                        || config.filter_syscalls.contains(&e.syscall_nr)
                })
                .copied()
                .collect();

            display_entries(
                &entries, &table, config, &mut stats, &mut *output,
            );
            total_events = total_events.saturating_add(entries.len() as u64);

            // Check if the process exited (saw an exit syscall).
            for entry in &entries {
                if entry.syscall_nr == 1 {
                    // SYS_EXIT
                    process_alive = false;
                    break;
                }
            }
        } else {
            // No events available.  Check if the process is still alive.
            let stat_path = format!("/proc/{target_pid}/stat");
            if fs::metadata(&stat_path).is_err() {
                process_alive = false;
            } else {
                std::thread::sleep(poll_interval);
            }
        }
    }

    // Disable tracing.
    trace_enable(target_pid, false);

    // Flush and print summary.
    if config.summary_only || config.summary_with_trace {
        let _ = writeln!(output);
        print_summary(&stats, &table, &mut *output);
    }

    let _ = output.flush();

    if total_events == 0 {
        eprintln!("strace: no trace events captured for PID {target_pid}");
    }
}

/// Display a batch of trace entries to the output.
fn display_entries(
    entries: &[TraceEntry],
    table: &HashMap<u32, SyscallInfo>,
    config: &Config,
    stats: &mut HashMap<u32, SyscallStats>,
    output: &mut dyn Write,
) {
    // Track timestamps for relative mode.
    let first_ts = entries.first().map_or(0, |e| e.timestamp_ns);
    let mut prev_ts = first_ts;

    for entry in entries {
        // Update statistics.
        let stat = stats
            .entry(entry.syscall_nr)
            .or_insert_with(SyscallStats::new);
        stat.calls = stat.calls.saturating_add(1);
        stat.total_time_ns = stat.total_time_ns.saturating_add(entry.duration_ns);
        if entry.result < 0 {
            stat.errors = stat.errors.saturating_add(1);
        }

        // Print trace line (unless summary-only).
        if config.show_trace() {
            if config.json_output {
                let json = format_trace_json(entry, table, config);
                let _ = writeln!(output, "{json}");
            } else {
                let line = format_trace_line(
                    entry, table, config, first_ts, &mut prev_ts,
                );
                let _ = writeln!(output, "{line}");
            }
        }
    }
}

// ============================================================================
// Process spawning
// ============================================================================

/// Spawn a command and return its PID for tracing.
///
/// Uses `std::process::Command` to launch the process, then returns
/// its PID so we can attach tracing to it.
fn spawn_and_trace(command: &[String]) -> Option<u32> {
    let program = command.first()?;
    let args = command.get(1..)?;

    let child = process::Command::new(program)
        .args(args)
        .spawn()
        .ok()?;

    Some(child.id())
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    let usage = "\
SlateOS Syscall Trace Utility v0.1.0

USAGE:
  strace [options] -p <pid>
  strace [options] <command> [args...]

OPTIONS:
  -p <pid>         Attach to a running process
  -o <file>        Write output to file instead of stderr
  -c               Summary only: count time, calls, errors per syscall
  -C               Like -c but also show trace output
  -e <expr>        Filter expression (e.g., trace=open,read,write)
                   Groups: file, network, ipc, process, memory, signal, drm
  -T               Show time spent in each syscall
  -t               Show wall-clock timestamp per line
  -tt              Show wall-clock timestamp with microseconds
  -r               Show relative timestamps
  -f               Follow forks: trace child processes too
  -v               Verbose: show full struct contents
  --json           JSON output
  --help, -h       Show this help

FILTER EXPRESSION (-e):
  trace=open,read,write    Trace specific syscalls by name
  trace=file               Trace all filesystem syscalls
  trace=network            Trace all network syscalls
  trace=ipc                Trace all IPC syscalls (channels, pipes, etc.)
  trace=process            Trace all process/thread syscalls
  trace=memory             Trace memory syscalls (mmap, munmap, dma)

EXAMPLES:
  strace -p 42                       Trace PID 42
  strace -p 42 -c                    Summary of PID 42's syscalls
  strace -p 42 -T -tt                Trace with timing and timestamps
  strace -e trace=file -p 42         Only file-related syscalls
  strace -o trace.log -p 42          Write trace to file
  strace --json -p 42                JSON trace output
  strace /bin/hello                   Trace a launched command";

    println!("{usage}");
}

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();
    let mut config = Config::new();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    // Pre-build the syscall table for filter parsing.
    let table = build_syscall_table();

    let mut i = 1;
    let mut saw_double_dash = false;

    while i < args.len() {
        if saw_double_dash {
            // Everything after `--` is the command to trace.
            config.command.push(args[i].clone());
            i += 1;
            continue;
        }

        let arg = args[i].as_str();
        match arg {
            "--" => {
                saw_double_dash = true;
                i += 1;
            }
            "-p" => {
                if i + 1 >= args.len() {
                    eprintln!("strace: -p requires a PID value");
                    process::exit(1);
                }
                match args[i + 1].parse::<u32>() {
                    Ok(pid) => config.attach_pid = Some(pid),
                    Err(_) => {
                        eprintln!("strace: invalid PID: {}", args[i + 1]);
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("strace: -o requires a filename");
                    process::exit(1);
                }
                config.output_file = Some(args[i + 1].clone());
                i += 2;
            }
            "-c" | "--summary-only" => {
                config.summary_only = true;
                i += 1;
            }
            "-C" => {
                config.summary_with_trace = true;
                i += 1;
            }
            "-e" => {
                if i + 1 >= args.len() {
                    eprintln!("strace: -e requires a filter expression");
                    process::exit(1);
                }
                let filter = parse_filter(&args[i + 1], &table);
                if filter.is_empty() {
                    eprintln!(
                        "strace: warning: filter '{}' matched no syscalls",
                        args[i + 1]
                    );
                }
                config.filter_syscalls.extend(filter);
                i += 2;
            }
            "-T" | "--syscall-times" => {
                config.show_duration = true;
                i += 1;
            }
            "-t" => {
                config.show_timestamp = true;
                i += 1;
            }
            "-tt" => {
                config.show_timestamp_us = true;
                i += 1;
            }
            "-r" | "--relative-timestamps" => {
                config.relative_timestamps = true;
                i += 1;
            }
            "-f" | "--follow-forks" => {
                config.follow_forks = true;
                i += 1;
            }
            "-v" | "--verbose" => {
                config.verbose = true;
                i += 1;
            }
            "--json" => {
                config.json_output = true;
                i += 1;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                // If it starts with '-', it's an unknown flag.
                // Otherwise, it's the start of a command to trace.
                if other.starts_with('-') {
                    eprintln!("strace: unknown option: {other}");
                    eprintln!("Run 'strace --help' for usage.");
                    process::exit(1);
                }
                // Treat this and everything after as the command.
                config.command.extend(args[i..].iter().cloned());
                break;
            }
        }
    }

    // Validate: need either -p or a command.
    if config.attach_pid.is_none() && config.command.is_empty() {
        eprintln!("strace: must specify -p <pid> or a command to trace");
        eprintln!("Run 'strace --help' for usage.");
        process::exit(1);
    }

    // Dedup filter list.
    config.filter_syscalls.sort_unstable();
    config.filter_syscalls.dedup();

    config
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let config = parse_args();
    run_trace(&config);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- TraceEntry --------------------------------------------------------

    #[test]
    fn trace_entry_size_is_stable() {
        // Layout is wire-format with the kernel; if this size changes, the
        // kernel side must be updated in lockstep.  8 u64-equivalent fields
        // = 64 bytes on a target with natural alignment.
        // timestamp_ns u64 (8) + syscall_nr u32 + pid u32 (8) +
        // arg1/2/3 u64 (24) + result i64 (8) + duration_ns u64 (8) = 56.
        assert_eq!(TRACE_ENTRY_SIZE, 56);
    }

    #[test]
    fn trace_entry_zeroed_has_all_zero_fields() {
        let e = TraceEntry::zeroed();
        assert_eq!(e.timestamp_ns, 0);
        assert_eq!(e.syscall_nr, 0);
        assert_eq!(e.pid, 0);
        assert_eq!(e.arg1, 0);
        assert_eq!(e.arg2, 0);
        assert_eq!(e.arg3, 0);
        assert_eq!(e.result, 0);
        assert_eq!(e.duration_ns, 0);
    }

    // ---- build_syscall_table -----------------------------------------------

    #[test]
    fn syscall_table_has_known_entries() {
        let t = build_syscall_table();
        assert_eq!(t.get(&0).map(|i| i.name), Some("yield"));
        assert_eq!(t.get(&1).map(|i| i.name), Some("exit"));
        assert_eq!(t.get(&200).map(|i| i.name), Some("channel_create"));
        assert_eq!(t.get(&500).map(|i| i.name), Some("process_spawn"));
        assert_eq!(t.get(&520).map(|i| i.name), Some("trace_enable"));
        assert_eq!(t.get(&521).map(|i| i.name), Some("trace_read"));
        assert_eq!(t.get(&610).map(|i| i.name), Some("fs_open"));
        assert_eq!(t.get(&800).map(|i| i.name), Some("tcp_connect"));
        assert_eq!(t.get(&1000).map(|i| i.name), Some("drm_open"));
    }

    #[test]
    fn syscall_table_does_not_contain_unknown_numbers() {
        let t = build_syscall_table();
        // Number from the gap between known ranges.
        assert!(!t.contains_key(&999_999));
        assert!(!t.contains_key(&7));
    }

    // ---- syscall_group -----------------------------------------------------

    #[test]
    fn syscall_group_file_includes_fs_range() {
        let g = syscall_group("file");
        assert!(g.contains(&600));
        assert!(g.contains(&610));
        assert!(g.contains(&651));
    }

    #[test]
    fn syscall_group_network_alias_matches_net() {
        assert_eq!(syscall_group("network"), syscall_group("net"));
        assert!(syscall_group("network").contains(&800));
        assert!(syscall_group("network").contains(&855));
    }

    #[test]
    fn syscall_group_ipc_covers_range() {
        let g = syscall_group("ipc");
        assert!(g.contains(&200));
        assert!(g.contains(&295));
    }

    #[test]
    fn syscall_group_process_alias_matches_proc() {
        assert_eq!(syscall_group("process"), syscall_group("proc"));
    }

    #[test]
    fn syscall_group_memory_lists_mm_syscalls() {
        let g = syscall_group("memory");
        assert!(g.contains(&20));
        assert!(g.contains(&21));
        assert!(g.contains(&42));
    }

    #[test]
    fn syscall_group_unknown_returns_empty() {
        assert!(syscall_group("nonexistent-group").is_empty());
    }

    // ---- parse_filter ------------------------------------------------------

    #[test]
    fn parse_filter_by_name_with_trace_prefix() {
        let table = build_syscall_table();
        let result = parse_filter("trace=fs_open,fs_close", &table);
        assert!(result.contains(&610));
        assert!(result.contains(&611));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn parse_filter_by_name_without_trace_prefix() {
        let table = build_syscall_table();
        let result = parse_filter("yield,exit", &table);
        assert!(result.contains(&0));
        assert!(result.contains(&1));
    }

    #[test]
    fn parse_filter_by_group_expands_range() {
        let table = build_syscall_table();
        let result = parse_filter("trace=ipc", &table);
        // Group "ipc" covers 200..=295 — check both ends.
        assert!(result.contains(&200));
        assert!(result.contains(&295));
        assert!(result.len() > 1);
    }

    #[test]
    fn parse_filter_unknown_name_yields_empty() {
        let table = build_syscall_table();
        let result = parse_filter("trace=nonexistent_syscall_name", &table);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_filter_results_are_sorted_and_deduped() {
        let table = build_syscall_table();
        // Add duplicates and out-of-order names.
        let result = parse_filter("trace=fs_close,fs_open,fs_close,fs_open", &table);
        // Should be sorted and unique.
        assert_eq!(result, vec![610, 611]);
    }

    // ---- format_ptr_as_string ----------------------------------------------

    #[test]
    fn format_ptr_as_string_null_pointer() {
        assert_eq!(format_ptr_as_string(0, 16), "NULL");
    }

    #[test]
    fn format_ptr_as_string_zero_length_keeps_pointer() {
        // ptr non-zero, len zero -> show "0x{ptr}, 0".
        assert_eq!(format_ptr_as_string(0xDEAD, 0), "0xdead, 0");
    }

    #[test]
    fn format_ptr_as_string_caps_display_length() {
        // length > 256 gets clamped to 256 in the printed form.
        let s = format_ptr_as_string(0x1000, 1024);
        assert_eq!(s, "0x1000, 256");
    }

    #[test]
    fn format_ptr_as_string_small_length() {
        let s = format_ptr_as_string(0x2000, 14);
        assert_eq!(s, "0x2000, 14");
    }

    // ---- format_args_into --------------------------------------------------

    fn entry_with(nr: u32, a1: u64, a2: u64, a3: u64, result: i64) -> TraceEntry {
        TraceEntry {
            timestamp_ns: 0,
            syscall_nr: nr,
            pid: 0,
            arg1: a1,
            arg2: a2,
            arg3: a3,
            result,
            duration_ns: 0,
        }
    }

    #[test]
    fn format_args_into_none_emits_nothing() {
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(0, 1, 2, 3, 0), ArgFormat::None, false);
        assert!(out.is_empty());
    }

    #[test]
    fn format_args_into_int_shows_only_arg1() {
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(1, 42, 99, 0, 0), ArgFormat::Int, false);
        assert_eq!(out, "42");
    }

    #[test]
    fn format_args_into_intint_shows_two_args() {
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(0, 7, 8, 0, 0), ArgFormat::IntInt, false);
        assert_eq!(out, "7, 8");
    }

    #[test]
    fn format_args_into_ptrlen_verbose_shows_hex_and_dec() {
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(0, 0x1000, 16, 0, 0), ArgFormat::PtrLen, true);
        assert_eq!(out, "0x1000, 16");
    }

    #[test]
    fn format_args_into_ptrlen_brief_uses_format_ptr() {
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(0, 0, 16, 0, 0), ArgFormat::PtrLen, false);
        // NULL pointer goes through format_ptr_as_string.
        assert_eq!(out, "NULL");
    }

    #[test]
    fn format_args_into_fdptrlen_shows_fd_then_buffer() {
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(0, 3, 0x1000, 256, 0), ArgFormat::FdPtrLen, true);
        assert_eq!(out, "3, 0x1000, 256");
    }

    #[test]
    fn format_args_into_generic_collapses_zero_args() {
        // Generic format trims trailing zero args.
        let mut out = String::new();
        format_args_into(&mut out, &entry_with(0, 0xABCD, 0, 0, 0), ArgFormat::Generic, false);
        assert_eq!(out, "0xabcd");

        let mut out2 = String::new();
        format_args_into(&mut out2, &entry_with(0, 1, 2, 0, 0), ArgFormat::Generic, false);
        assert_eq!(out2, "0x1, 0x2");

        let mut out3 = String::new();
        format_args_into(&mut out3, &entry_with(0, 1, 2, 3, 0), ArgFormat::Generic, false);
        assert_eq!(out3, "0x1, 0x2, 0x3");

        let mut out4 = String::new();
        format_args_into(&mut out4, &entry_with(0, 0, 0, 0, 0), ArgFormat::Generic, false);
        assert!(out4.is_empty());
    }

    // ---- format_trace_line -------------------------------------------------

    #[test]
    fn format_trace_line_basic_success_return() {
        let table = build_syscall_table();
        let config = Config::new();
        let entry = entry_with(2, 0, 0, 0, 7); // task_id => returns 7
        let mut prev = 0u64;
        let line = format_trace_line(&entry, &table, &config, 0, &mut prev);
        assert_eq!(line, "task_id() = 7");
    }

    #[test]
    fn format_trace_line_negative_result_formats_as_error() {
        let table = build_syscall_table();
        let config = Config::new();
        let entry = entry_with(2, 0, 0, 0, -13);
        let mut prev = 0u64;
        let line = format_trace_line(&entry, &table, &config, 0, &mut prev);
        assert_eq!(line, "task_id() = -1 (err 13)");
    }

    #[test]
    fn format_trace_line_exit_shows_question_mark() {
        let table = build_syscall_table();
        let config = Config::new();
        let entry = entry_with(1, 0, 0, 0, 0); // exit
        let mut prev = 0u64;
        let line = format_trace_line(&entry, &table, &config, 0, &mut prev);
        // exit() never returns -> "= ?"
        assert!(line.ends_with("= ?"), "got: {line}");
        assert!(line.contains("exit("));
    }

    #[test]
    fn format_trace_line_unknown_syscall_uses_unknown_label() {
        let table = build_syscall_table();
        let config = Config::new();
        let entry = entry_with(99_999, 0, 0, 0, 0);
        let mut prev = 0u64;
        let line = format_trace_line(&entry, &table, &config, 0, &mut prev);
        assert!(line.starts_with("unknown("), "got: {line}");
    }

    #[test]
    fn format_trace_line_with_duration_appends_angle_brackets() {
        let table = build_syscall_table();
        let mut config = Config::new();
        config.show_duration = true;
        let mut entry = entry_with(2, 0, 0, 0, 7);
        entry.duration_ns = 1_500_000; // 1.5ms = 0.001500s
        let mut prev = 0u64;
        let line = format_trace_line(&entry, &table, &config, 0, &mut prev);
        assert!(line.contains("<0.001500>"), "got: {line}");
    }

    #[test]
    fn format_trace_line_follow_forks_prefixes_pid() {
        let table = build_syscall_table();
        let mut config = Config::new();
        config.follow_forks = true;
        let mut entry = entry_with(2, 0, 0, 0, 7);
        entry.pid = 1234;
        let mut prev = 0u64;
        let line = format_trace_line(&entry, &table, &config, 0, &mut prev);
        assert!(line.contains("[pid  1234]"), "got: {line}");
    }

    // ---- format_trace_json -------------------------------------------------

    #[test]
    fn format_trace_json_includes_required_fields() {
        let table = build_syscall_table();
        let config = Config::new();
        let mut entry = entry_with(2, 10, 20, 30, 7);
        entry.timestamp_ns = 1_000;
        entry.pid = 42;
        let s = format_trace_json(&entry, &table, &config);
        assert!(s.contains("\"timestamp_ns\":1000"));
        assert!(s.contains("\"pid\":42"));
        assert!(s.contains("\"syscall\":\"task_id\""));
        assert!(s.contains("\"nr\":2"));
        assert!(s.contains("\"args\":[10,20,30]"));
        assert!(s.contains("\"result\":7"));
        // Without -T, no duration_ns key.
        assert!(!s.contains("duration_ns"));
        assert!(s.ends_with('}'));
    }

    #[test]
    fn format_trace_json_with_duration_includes_duration_field() {
        let table = build_syscall_table();
        let mut config = Config::new();
        config.show_duration = true;
        let mut entry = entry_with(2, 0, 0, 0, 7);
        entry.duration_ns = 999;
        let s = format_trace_json(&entry, &table, &config);
        assert!(s.contains("\"duration_ns\":999"));
    }

    // ---- Config ------------------------------------------------------------

    #[test]
    fn config_show_trace_default_is_true() {
        // Default Config (no -c, no -C) prints trace.
        assert!(Config::new().show_trace());
    }

    #[test]
    fn config_show_trace_false_when_summary_only() {
        let mut c = Config::new();
        c.summary_only = true;
        assert!(!c.show_trace());
    }

    #[test]
    fn config_show_trace_true_when_summary_with_trace() {
        let mut c = Config::new();
        c.summary_only = true;
        c.summary_with_trace = true;
        // -C overrides -c: still show trace lines.
        assert!(c.show_trace());
    }

    // ---- SyscallStats ------------------------------------------------------

    #[test]
    fn syscall_stats_starts_zeroed() {
        let s = SyscallStats::new();
        assert_eq!(s.calls, 0);
        assert_eq!(s.errors, 0);
        assert_eq!(s.total_time_ns, 0);
    }

    // ---- print_summary smoke test ------------------------------------------

    #[test]
    fn print_summary_writes_header_and_rows() {
        let table = build_syscall_table();
        let mut stats = HashMap::new();
        let mut s1 = SyscallStats::new();
        s1.calls = 3;
        s1.errors = 1;
        s1.total_time_ns = 5_000_000; // 5ms
        stats.insert(2u32, s1);

        let mut buf: Vec<u8> = Vec::new();
        print_summary(&stats, &table, &mut buf);
        let text = String::from_utf8(buf).expect("ASCII summary");

        assert!(text.contains("% time"));
        assert!(text.contains("syscall"));
        assert!(text.contains("task_id"));
        assert!(text.contains("total"));
    }
}
