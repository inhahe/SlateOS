//! Userspace init process — the first process spawned by the kernel.
//!
//! This is PID 1 in our OS.  It runs in ring 3 and communicates with
//! the kernel exclusively through the SYSCALL instruction.
//!
//! ## Current Functionality
//!
//! - Prints a welcome banner via `SYS_CONSOLE_WRITE`.
//! - Runs a minimal read-eval-print loop (kernel shell replacement).
//! - Built-in commands: `help`, `echo`, `exit`, `ls`, `cat`, `stat`,
//!   `write`, `mkdir`, `rmdir`, `rm`, `pid`, `uptime`.
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
const SYS_CONSOLE_WRITE: u64 = 100;
const SYS_CONSOLE_READ_CHAR: u64 = 101;
const SYS_PROCESS_SPAWN: u64 = 500;
const SYS_PROCESS_WAIT: u64 = 501;
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

/// Get current task ID.
fn task_id() -> i64 {
    syscall0(SYS_TASK_ID)
}

/// Get monotonic clock in nanoseconds since boot.
fn clock_monotonic() -> i64 {
    syscall0(SYS_CLOCK_MONOTONIC)
}

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

/// Spawn a new process from ELF data in memory.
/// Returns the child PID (positive) or negative error.
fn process_spawn(elf: &[u8], name: &[u8]) -> i64 {
    syscall4(
        SYS_PROCESS_SPAWN,
        elf.as_ptr() as u64,
        elf.len() as u64,
        name.as_ptr() as u64,
        name.len() as u64,
    )
}

/// Wait for a child process to exit.
/// Returns the exit code or negative error.
fn process_wait(pid: u64) -> i64 {
    syscall1(SYS_PROCESS_WAIT, pid)
}

/// Stat a file.  Returns 0 or negative error.
/// On success, fills `out` with 16-byte FsStatResult:
///   bytes 0-7: file size (u64 LE)
///   byte 8: type (0=file, 1=directory, 2=volume label)
///   bytes 9-15: reserved
fn fs_stat(path: &[u8], out: &mut [u8; 16]) -> i64 {
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

    let mut out = [0u8; 16];
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

/// `spawn <path>` — load an ELF from the filesystem and run it.
fn cmd_spawn(args: &[u8]) {
    if args.is_empty() {
        print("spawn: missing path\n");
        return;
    }

    // Read the ELF from the filesystem.
    // 64 KiB max — init has no heap, so this is a stack buffer.
    let mut elf_buf = [0u8; 65536];
    let result = fs_read_file(args, &mut elf_buf);
    if result < 0 {
        print("spawn: failed to read ");
        console_write(args);
        print(": error ");
        print_i64(result);
        print("\n");
        return;
    }

    let elf_len = result as usize;
    let elf_data = &elf_buf[..elf_len];

    // Extract filename from path for the process name.
    let name = {
        let mut last_slash = 0;
        let mut i = 0;
        while i < args.len() {
            if args[i] == b'/' {
                last_slash = i + 1;
            }
            i += 1;
        }
        &args[last_slash..]
    };

    print("Spawning ");
    console_write(args);
    print(" (");
    print_u64(elf_len as u64);
    print(" bytes)...\n");

    let pid = process_spawn(elf_data, name);
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

/// Execute a command.
fn execute(line: &[u8]) {
    let trimmed = trim(line);
    if trimmed.is_empty() {
        return;
    }

    let (cmd, args) = split_first_word(trimmed);

    if bytes_eq(cmd, b"help") {
        print("Available commands:\n");
        print("  help        - show this message\n");
        print("  echo <text> - print text\n");
        print("  ls [path]   - list directory\n");
        print("  cat <path>  - print file contents\n");
        print("  write <p> <d> - write data to file\n");
        print("  stat <path> - show file metadata\n");
        print("  mkdir <path> - create directory\n");
        print("  rmdir <path> - remove empty directory\n");
        print("  rm <path>   - delete a file\n");
        print("  spawn <path> - run an ELF program\n");
        print("  pid         - show task ID\n");
        print("  uptime      - show time since boot\n");
        print("  logs        - show kernel log entries\n");
        print("  exit        - shut down\n");
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
    let _ = info;
    print("!!! PANIC in init process !!!\n");
    exit(-1);
}
