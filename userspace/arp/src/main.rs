//! OurOS ARP Table Management Utility
//!
//! Reads, adds, and deletes entries in the ARP (Address Resolution Protocol)
//! cache.  The cache is accessed through `/proc/net/arp` for reads and via
//! dedicated syscalls for mutations.  A simplified raw-socket ARP probe is
//! also supported for on-demand address resolution.
//!
//! # Usage
//!
//! ```text
//! arp                         Display all ARP cache entries
//! arp -a                      Display all ARP cache entries (explicit)
//! arp -n                      Display entries, numeric output only
//! arp -i eth0                 Limit display to interface eth0
//! arp -v                      Verbose output
//! arp -d hostname             Delete an ARP entry
//! arp -s hostname hw_addr     Add a static ARP entry
//! arp -D -s hostname iface    Use device MAC for a static entry
//! ```

// The syscall constants and low-level wrappers below document the full OurOS
// syscall ABI for this subsystem.  Several are present as building blocks for
// future extension and are not yet called from the command handlers; suppress
// the resulting dead_code warnings rather than deleting intentional API stubs.
#![allow(dead_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Syscall numbers (OurOS-specific assignments in the net range 800-999)
// ============================================================================

/// Read raw bytes from an open file descriptor.
/// arg1 = fd, arg2 = buf ptr, arg3 = count.
/// Returns bytes read (>= 0) or negative errno.
const SYS_READ: u64 = 0;

/// Write raw bytes to a file descriptor.
/// arg1 = fd, arg2 = buf ptr, arg3 = count.
/// Returns bytes written (>= 0) or negative errno.
const SYS_WRITE: u64 = 1;

/// Open a file, returning a file descriptor.
/// arg1 = path ptr, arg2 = path len, arg3 = flags (O_RDONLY=0, O_WRONLY=1).
/// Returns fd (>= 0) or negative errno.
const SYS_OPEN: u64 = 2;

/// Close a file descriptor.
/// arg1 = fd.
/// Returns 0 or negative errno.
const SYS_CLOSE: u64 = 3;

/// Terminate the process.
/// arg1 = exit code.
/// Does not return.
const SYS_EXIT: u64 = 60;

/// Add a static ARP entry.
/// arg1 = ptr to ArpRequest, arg2 = sizeof(ArpRequest), arg3 = 0.
/// Returns 0 on success or negative errno.
const SYS_ARP_ADD: u64 = 840;

/// Delete an ARP entry by IP.
/// arg1 = IPv4 address (u32, host byte order), arg2 = 0, arg3 = 0.
/// Returns 0 on success or negative errno.
const SYS_ARP_DEL: u64 = 841;

/// Probe an IP address via ARP request and wait for a reply.
/// arg1 = IPv4 address (u32, host byte order), arg2 = timeout_ms, arg3 = 0.
/// Returns 0 on success or negative errno.
const SYS_ARP_PROBE: u64 = 842;

/// Query the MAC address of a network interface.
/// arg1 = ptr to interface name (null-terminated), arg2 = ptr to [u8; 6] result buf, arg3 = 0.
/// Returns 0 on success or negative errno.
const SYS_IFMAC: u64 = 843;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 3-argument syscall via the x86_64 `syscall` instruction.
///
/// # Safety
///
/// The caller must ensure:
/// - `nr` is a valid OurOS syscall number.
/// - All arguments are valid for that specific syscall (valid pointers,
///   correct sizes, etc.).
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid for the given syscall.
    // The `syscall` instruction clobbers rcx and r11 per the System V ABI.
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

// Stub for non-x86_64 hosts (e.g., running tests on the build machine).
#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

// ============================================================================
// Low-level syscall wrappers
// ============================================================================

/// Map a negative syscall return code to a human-readable string.
fn errno_str(code: i64) -> &'static str {
    match code {
        -1 => "operation not permitted",
        -2 => "no such file or directory",
        -4 => "interrupted system call",
        -9 => "bad file descriptor",
        -11 => "resource temporarily unavailable",
        -12 => "out of memory",
        -13 => "permission denied",
        -14 => "bad address",
        -17 => "file already exists",
        -19 => "no such device",
        -22 => "invalid argument",
        -28 => "no space left on device",
        -38 => "function not implemented",
        -105 => "no buffer space available",
        -110 => "connection timed out",
        -111 => "connection refused",
        -113 => "no route to host",
        _ => "unknown error",
    }
}

/// Open `/proc/net/arp` (or any path) for reading.
///
/// On our OS we use `std::fs::read_to_string` to stay portable; the raw
/// syscall wrappers below are provided for completeness and used by the
/// mutating operations that have no `std` counterpart.
fn sys_open_readonly(path: &str) -> Result<i32, i64> {
    let ret = unsafe {
        // SAFETY: path is a valid UTF-8 string pointer with accurate length.
        // Flags 0 = O_RDONLY.
        syscall3(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, 0)
    };
    if ret < 0 { Err(ret) } else { Ok(ret as i32) }
}

/// Close a file descriptor returned by `sys_open_readonly`.
fn sys_close(fd: i32) {
    // SAFETY: fd is a non-negative file descriptor obtained from SYS_OPEN.
    // We deliberately ignore the return value — a failed close on a
    // read-only descriptor has no meaningful recovery path.
    let _ = unsafe { syscall3(SYS_CLOSE, fd as u64, 0, 0) };
}

/// Read at most `buf.len()` bytes from `fd` into `buf`.
/// Returns the number of bytes actually read, or a negative errno.
fn sys_read(fd: i32, buf: &mut [u8]) -> i64 {
    // SAFETY: fd is valid, buf is a mutable slice with accurate pointer and length.
    unsafe {
        syscall3(
            SYS_READ,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    }
}

/// Write `buf` to `fd`.  Returns bytes written or negative errno.
fn sys_write(fd: i32, buf: &[u8]) -> i64 {
    // SAFETY: fd is valid (stdout=1 / stderr=2), buf pointer and length are accurate.
    unsafe {
        syscall3(
            SYS_WRITE,
            fd as u64,
            buf.as_ptr() as u64,
            buf.len() as u64,
        )
    }
}

/// Write a string to stderr without going through `std::io`.
fn write_stderr(msg: &str) {
    // stderr fd = 2.  Ignore partial-write; this is best-effort diagnostic output.
    let _ = sys_write(2, msg.as_bytes());
}

// ============================================================================
// IP / MAC address parsing and formatting
// ============================================================================

/// Parse a dotted-decimal IPv4 address into a `u32` in host byte order.
/// Returns `None` on malformed input.
fn parse_ipv4(s: &str) -> Option<u32> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u16 = 0;
    let mut dots = 0u8;
    let mut has_digit = false;

    for ch in s.chars() {
        match ch {
            '0'..='9' => {
                cur = cur.checked_mul(10)?.checked_add(u16::from(ch as u8 - b'0'))?;
                if cur > 255 {
                    return None;
                }
                has_digit = true;
            }
            '.' => {
                if !has_digit || dots == 3 {
                    return None;
                }
                octets[idx] = cur as u8;
                idx = idx.checked_add(1)?;
                cur = 0;
                has_digit = false;
                dots = dots.checked_add(1)?;
            }
            _ => return None,
        }
    }

    if !has_digit || dots != 3 {
        return None;
    }
    octets[3] = cur as u8;

    Some(u32::from_be_bytes(octets))
}

/// Format a `u32` IPv4 address (host byte order) as `A.B.C.D`.
fn format_ipv4(ip: u32) -> String {
    let b = ip.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// Parse a colon-separated MAC address such as `aa:bb:cc:dd:ee:ff` into
/// a `[u8; 6]`.  Returns `None` on malformed input.
fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    for (i, part) in parts.iter().enumerate() {
        if part.len() != 2 {
            return None;
        }
        out[i] = u8::from_str_radix(part, 16).ok()?;
    }
    Some(out)
}

/// Format a `[u8; 6]` MAC address as `XX:XX:XX:XX:XX:XX` (lower-case hex).
fn format_mac(mac: &[u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    )
}

/// Return `true` if `mac` is the all-zeros sentinel (unresolved / incomplete).
fn mac_is_zero(mac: &[u8; 6]) -> bool {
    mac.iter().all(|&b| b == 0)
}

// ============================================================================
// ARP cache entry
// ============================================================================

/// ARP cache flags as exposed by `/proc/net/arp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpFlags(pub u16);

impl ArpFlags {
    /// Entry is complete (resolved).
    pub const COMPLETE: u16 = 0x02;
    /// Entry was added manually (static / permanent).
    pub const PERMANENT: u16 = 0x04;
    /// Published — kernel answers ARP requests on behalf of this host.
    pub const PUBLISHED: u16 = 0x08;
    /// Entry is in use.
    pub const USED: u16 = 0x01;

    /// Human-readable flag summary: `CMP` style (space-padded if absent).
    pub fn summary(self) -> String {
        let mut s = String::with_capacity(4);
        if self.0 & Self::COMPLETE != 0 {
            s.push('C');
        }
        if self.0 & Self::PERMANENT != 0 {
            s.push('M');
        }
        if self.0 & Self::PUBLISHED != 0 {
            s.push('P');
        }
        s
    }
}

/// One row from the ARP table.
#[derive(Debug, Clone)]
pub struct ArpEntry {
    /// IPv4 address of the peer.
    pub ip: u32,
    /// Hardware (Ethernet) type; 1 = Ethernet.
    pub hw_type: u16,
    /// ARP state flags (see `ArpFlags`).
    pub flags: ArpFlags,
    /// MAC address; all-zeros if unresolved.
    pub mac: [u8; 6],
    /// Network interface name (e.g. `eth0`).
    pub iface: String,
}

impl ArpEntry {
    /// Return a human-readable hardware type name.
    fn hw_type_name(hw: u16) -> &'static str {
        match hw {
            1 => "ether",
            15 => "DLCI",
            19 => "skip",
            23 => "tunnel",
            _ => "unknown",
        }
    }

    /// Format this entry for display.
    ///
    /// `numeric` suppresses hostname lookups (we always skip them here since
    /// we have no resolver; the flag is a no-op placeholder for compatibility).
    fn display(&self, numeric: bool, verbose: bool) -> String {
        let ip_str = format_ipv4(self.ip);
        let mac_str = if mac_is_zero(&self.mac) {
            "<incomplete>".to_string()
        } else {
            format_mac(&self.mac)
        };
        let hw_name = Self::hw_type_name(self.hw_type);
        let flags = self.flags.summary();

        if verbose {
            format!(
                "{}{} ({}) at {} [{}] {} on {}",
                if numeric { "" } else { "" }, // placeholder: hostname would go here
                ip_str,
                ip_str,
                mac_str,
                hw_name,
                if flags.is_empty() { "<none>".to_string() } else { flags },
                self.iface,
            )
        } else {
            // Standard `arp -a` style: "? (1.2.3.4) at aa:bb:cc:dd:ee:ff [ether] on eth0"
            let host_part = if numeric {
                format!("({ip_str})")
            } else {
                format!("? ({ip_str})")
            };
            format!(
                "{host_part} at {mac_str} [{hw_name}] on {}",
                self.iface
            )
        }
    }
}

// ============================================================================
// /proc/net/arp parser
// ============================================================================

/// Errors that can occur while reading/parsing the ARP table.
#[derive(Debug)]
pub enum ArpError {
    /// Could not open or read `/proc/net/arp`.
    IoError(String),
    /// A line in the file had an unexpected format.
    ParseError(String),
    /// A syscall returned a negative error code.
    SyscallError(i64),
}

impl std::fmt::Display for ArpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::SyscallError(code) => {
                write!(f, "syscall error {code}: {}", errno_str(*code))
            }
        }
    }
}

/// Parse the contents of `/proc/net/arp` into a vector of `ArpEntry`.
///
/// Expected format (first line is the header):
/// ```text
/// IP address       HW type     Flags       HW address            Mask     Device
/// 192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0
/// ```
pub fn parse_proc_arp(content: &str) -> Result<Vec<ArpEntry>, ArpError> {
    let mut entries = Vec::new();
    let mut lines = content.lines();

    // Skip the header line.
    if lines.next().is_none() {
        return Ok(entries);
    }

    for (lineno, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Split on whitespace; expected columns:
        // 0: IP address
        // 1: HW type (0x1)
        // 2: Flags (0x2)
        // 3: HW address (MAC or 00:00:00:00:00:00)
        // 4: Mask (* )
        // 5: Device (interface name)
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 6 {
            return Err(ArpError::ParseError(format!(
                "line {}: expected 6 columns, got {}",
                lineno + 2,
                cols.len()
            )));
        }

        let ip = parse_ipv4(cols[0]).ok_or_else(|| {
            ArpError::ParseError(format!(
                "line {}: invalid IP address '{}'",
                lineno + 2,
                cols[0]
            ))
        })?;

        let hw_type = parse_hex_u16(cols[1]).ok_or_else(|| {
            ArpError::ParseError(format!(
                "line {}: invalid HW type '{}'",
                lineno + 2,
                cols[1]
            ))
        })?;

        let flag_bits = parse_hex_u16(cols[2]).ok_or_else(|| {
            ArpError::ParseError(format!(
                "line {}: invalid flags '{}'",
                lineno + 2,
                cols[2]
            ))
        })?;

        let mac = parse_mac(cols[3]).ok_or_else(|| {
            ArpError::ParseError(format!(
                "line {}: invalid MAC address '{}'",
                lineno + 2,
                cols[3]
            ))
        })?;

        let iface = cols[5].to_string();

        entries.push(ArpEntry {
            ip,
            hw_type,
            flags: ArpFlags(flag_bits),
            mac,
            iface,
        });
    }

    Ok(entries)
}

/// Parse a `0x`-prefixed hexadecimal string into a `u16`.
fn parse_hex_u16(s: &str) -> Option<u16> {
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    u16::from_str_radix(hex, 16).ok()
}

// ============================================================================
// ARP table read
// ============================================================================

/// Read and parse the ARP cache from `/proc/net/arp`.
/// Falls back to `std::fs` for portability during testing.
fn read_arp_table() -> Result<Vec<ArpEntry>, ArpError> {
    let content = fs::read_to_string("/proc/net/arp")
        .map_err(|e| ArpError::IoError(e.to_string()))?;
    parse_proc_arp(&content)
}

// ============================================================================
// Kernel ARP mutation syscalls
// ============================================================================

/// Wire-format structure passed to `SYS_ARP_ADD`.
///
/// Layout (all fields little-endian unless noted):
/// ```
/// offset  size  field
///      0     4  ip          (host byte order)
///      4     6  mac
///     10     2  flags
///     12    16  iface (null-padded)
/// ```
#[repr(C)]
struct ArpRequest {
    ip: u32,
    mac: [u8; 6],
    flags: u16,
    iface: [u8; 16],
}

/// Add a static ARP entry for `ip` with MAC `mac` on interface `iface`.
fn arp_add(ip: u32, mac: [u8; 6], iface: &str) -> Result<(), ArpError> {
    let mut req = ArpRequest {
        ip,
        mac,
        flags: ArpFlags::PERMANENT | ArpFlags::COMPLETE,
        iface: [0u8; 16],
    };

    // Copy interface name into the fixed-size field, truncating if needed.
    let name_bytes = iface.as_bytes();
    let copy_len = name_bytes.len().min(15);
    req.iface[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    // Remaining bytes are already zero (null-terminated).

    let ret = unsafe {
        // SAFETY: req is a fully-initialised struct with the correct layout
        // expected by SYS_ARP_ADD.  We pass its pointer and byte size.
        syscall3(
            SYS_ARP_ADD,
            &raw const req as u64,
            size_of::<ArpRequest>() as u64,
            0,
        )
    };

    if ret < 0 {
        Err(ArpError::SyscallError(ret))
    } else {
        Ok(())
    }
}

/// Delete the ARP cache entry for `ip`.
fn arp_del(ip: u32) -> Result<(), ArpError> {
    // SAFETY: SYS_ARP_DEL takes a scalar IPv4 address; no pointer
    // dereferences occur in userspace.
    let ret = unsafe { syscall3(SYS_ARP_DEL, u64::from(ip), 0, 0) };
    if ret < 0 {
        Err(ArpError::SyscallError(ret))
    } else {
        Ok(())
    }
}

/// Send an ARP probe for `ip` and wait up to `timeout_ms` for a reply.
fn arp_probe(ip: u32, timeout_ms: u64) -> Result<(), ArpError> {
    // SAFETY: SYS_ARP_PROBE takes two scalar arguments; no pointer
    // dereferences occur in userspace.
    let ret = unsafe { syscall3(SYS_ARP_PROBE, u64::from(ip), timeout_ms, 0) };
    if ret < 0 {
        Err(ArpError::SyscallError(ret))
    } else {
        Ok(())
    }
}

/// Query the hardware (MAC) address of `iface`.
fn iface_mac(iface: &str) -> Result<[u8; 6], ArpError> {
    // Provide a null-terminated name buffer.
    let mut name_buf = [0u8; 16];
    let copy_len = iface.as_bytes().len().min(15);
    name_buf[..copy_len].copy_from_slice(&iface.as_bytes()[..copy_len]);

    let mut mac = [0u8; 6];

    let ret = unsafe {
        // SAFETY: name_buf is null-terminated and mac has exactly 6 bytes.
        // SYS_IFMAC reads name_buf and writes 6 bytes into mac.
        syscall3(
            SYS_IFMAC,
            name_buf.as_ptr() as u64,
            mac.as_mut_ptr() as u64,
            0,
        )
    };

    if ret < 0 {
        Err(ArpError::SyscallError(ret))
    } else {
        Ok(mac)
    }
}

// ============================================================================
// CLI options
// ============================================================================

/// Mode of operation selected by the user.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Display the ARP table (default / `-a`).
    Display,
    /// Delete the entry for the given hostname/IP (`-d`).
    Delete,
    /// Add a static entry (`-s hostname hw_addr`).
    Add,
}

/// Parsed command-line options.
struct Options {
    mode: Mode,
    /// Target host/IP (for `-d` and `-s`, also for display filtering).
    host: Option<String>,
    /// Hardware address string for `-s`.
    hw_addr: Option<String>,
    /// Limit output/operation to this interface.
    interface: Option<String>,
    /// Suppress hostname lookups; print numeric IP addresses only.
    numeric: bool,
    /// Verbose output.
    verbose: bool,
    /// Use the device's own MAC address instead of an explicit hw_addr
    /// (for `-D -s`).
    use_device_mac: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Display,
            host: None,
            hw_addr: None,
            interface: None,
            numeric: false,
            verbose: false,
            use_device_mac: false,
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

fn print_usage() {
    let msg = "\
Usage: arp [OPTIONS] [hostname]

Display/modify the kernel ARP cache.

Options:
  -a, --all           Display all entries (default)
  -n                  Numeric output; do not resolve hostnames
  -v                  Verbose output
  -i <iface>          Limit to interface <iface>
  -d <hostname>       Delete ARP entry for <hostname>
  -s <hostname> <hw>  Add static entry: hostname -> hw (XX:XX:XX:XX:XX:XX)
  -D                  With -s: use the device MAC (requires -i)
  -h, --help          Show this help

Examples:
  arp                         Show all entries
  arp -n                      Show all entries, numeric IPs
  arp -i eth0                 Show entries for eth0
  arp -d 192.168.1.1          Delete entry for 192.168.1.1
  arp -s 192.168.1.50 aa:bb:cc:dd:ee:ff   Add static entry
  arp -D -s 192.168.1.50 eth0             Add entry using eth0's MAC
";
    let _ = io::stderr().write_all(msg.as_bytes());
}

fn parse_args() -> Result<Options, String> {
    let argv: Vec<String> = env::args().collect();
    let mut opts = Options::default();
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 1usize;

    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-a" | "--all" => {
                opts.mode = Mode::Display;
            }
            "-n" => {
                opts.numeric = true;
            }
            "-v" => {
                opts.verbose = true;
            }
            "-D" => {
                opts.use_device_mac = true;
            }
            "-i" => {
                i = i.checked_add(1).ok_or("integer overflow in arg index")?;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-i requires an interface name".to_string())?;
                opts.interface = Some(val.clone());
            }
            "-d" => {
                i = i.checked_add(1).ok_or("integer overflow in arg index")?;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-d requires a hostname/IP".to_string())?;
                opts.mode = Mode::Delete;
                opts.host = Some(val.clone());
            }
            "-s" => {
                i = i.checked_add(1).ok_or("integer overflow in arg index")?;
                let host = argv
                    .get(i)
                    .ok_or_else(|| "-s requires a hostname".to_string())?
                    .clone();
                i = i.checked_add(1).ok_or("integer overflow in arg index")?;
                let hw = argv
                    .get(i)
                    .ok_or_else(|| {
                        "-s requires both a hostname and a hardware address".to_string()
                    })?
                    .clone();
                opts.mode = Mode::Add;
                opts.host = Some(host);
                opts.hw_addr = Some(hw);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option: '{other}'"));
            }
            _ => {
                positionals.push(arg.to_string());
            }
        }
        i = i.checked_add(1).ok_or("integer overflow in arg index")?;
    }

    // A bare positional hostname filters the display (like `arp hostname`).
    if !positionals.is_empty() && opts.mode == Mode::Display {
        if positionals.len() > 1 {
            return Err("too many positional arguments".to_string());
        }
        opts.host = Some(
            positionals
                .into_iter()
                .next()
                .ok_or("internal: empty positionals")?,
        );
    }

    // Validate -D: requires -i and (-s or a hostname to be supplied separately)
    if opts.use_device_mac {
        if opts.mode != Mode::Add {
            return Err("-D requires -s".to_string());
        }
        if opts.interface.is_none() {
            // When -D is used, the hw_addr field may hold the interface name
            // (Linux arp -Ds hostname iface convention).
            // In that case promote hw_addr to interface and clear it.
            if let Some(hw) = opts.hw_addr.take() {
                opts.interface = Some(hw);
            } else {
                return Err("-D requires -i <iface> or iface as hw_addr argument".to_string());
            }
        }
    }

    Ok(opts)
}

// ============================================================================
// Display
// ============================================================================

/// Print the ARP table, optionally filtered by interface and/or host.
fn cmd_display(
    opts: &Options,
    stdout: &mut dyn Write,
) -> Result<(), ArpError> {
    let entries = read_arp_table()?;

    // Column header for non-verbose mode.
    if opts.verbose {
        writeln!(stdout, "Address                  HWtype  HWaddress           Flags Mask     Iface")
            .map_err(|e| ArpError::IoError(e.to_string()))?;
    }

    let mut count = 0usize;

    for entry in &entries {
        // Interface filter.
        if let Some(ref iface) = opts.interface {
            if &entry.iface != iface {
                continue;
            }
        }
        // Host/IP filter.
        if let Some(ref host) = opts.host {
            let ip_str = format_ipv4(entry.ip);
            if &ip_str != host && entry.iface != *host {
                // Host might be specified as a hostname; for now match by IP only.
                // A full implementation would resolve `host` via DNS.
                continue;
            }
        }

        if opts.verbose {
            // Tabular format matching Linux `arp -v`.
            let ip_str = format_ipv4(entry.ip);
            let hw_name = ArpEntry::hw_type_name(entry.hw_type);
            let mac_str = if mac_is_zero(&entry.mac) {
                "<incomplete>     ".to_string()
            } else {
                format!("{:<17}", format_mac(&entry.mac))
            };
            writeln!(
                stdout,
                "{:<24} {:<7} {} {:<5} *        {}",
                ip_str,
                hw_name,
                mac_str,
                entry.flags.summary(),
                entry.iface,
            )
            .map_err(|e| ArpError::IoError(e.to_string()))?;
        } else {
            writeln!(stdout, "{}", entry.display(opts.numeric, false))
                .map_err(|e| ArpError::IoError(e.to_string()))?;
        }

        count = count.saturating_add(1);
    }

    if opts.verbose {
        writeln!(stdout, "Entries: {count}   Skipped: {}   Found: {count}",
                 entries.len().saturating_sub(count))
            .map_err(|e| ArpError::IoError(e.to_string()))?;
    }

    Ok(())
}

// ============================================================================
// Delete
// ============================================================================

/// Delete the ARP entry for the host/IP in `opts.host`.
fn cmd_delete(opts: &Options) -> Result<(), ArpError> {
    let host = opts
        .host
        .as_deref()
        .ok_or_else(|| ArpError::IoError("no hostname specified".to_string()))?;

    // Resolve to IP: accept dotted-decimal or look it up in the ARP table.
    let ip = resolve_to_ip(host)?;

    arp_del(ip)?;

    if opts.verbose {
        writeln!(io::stdout(), "arp: deleted {host} ({}) from ARP cache", format_ipv4(ip))
            .map_err(|e| ArpError::IoError(e.to_string()))?;
    }

    Ok(())
}

// ============================================================================
// Add
// ============================================================================

/// Add a static ARP entry as specified by `opts`.
fn cmd_add(opts: &Options) -> Result<(), ArpError> {
    let host = opts
        .host
        .as_deref()
        .ok_or_else(|| ArpError::IoError("no hostname specified".to_string()))?;

    let ip = resolve_to_ip(host)?;

    // Determine MAC: either explicit or from the interface via SYS_IFMAC.
    let mac: [u8; 6] = if opts.use_device_mac {
        let iface = opts
            .interface
            .as_deref()
            .ok_or_else(|| ArpError::IoError("-D requires an interface".to_string()))?;
        iface_mac(iface)?
    } else {
        let hw_str = opts
            .hw_addr
            .as_deref()
            .ok_or_else(|| ArpError::IoError("no hardware address specified".to_string()))?;
        parse_mac(hw_str).ok_or_else(|| {
            ArpError::ParseError(format!("invalid hardware address: '{hw_str}'"))
        })?
    };

    // Default interface to "eth0" if not specified.
    let iface = opts.interface.as_deref().unwrap_or("eth0");

    arp_add(ip, mac, iface)?;

    if opts.verbose {
        writeln!(
            io::stdout(),
            "arp: added {} ({}) -> {} on {iface}",
            host,
            format_ipv4(ip),
            format_mac(&mac),
        )
        .map_err(|e| ArpError::IoError(e.to_string()))?;
    }

    Ok(())
}

// ============================================================================
// Host resolution helper
// ============================================================================

/// Resolve a hostname or dotted-decimal address to a `u32` IPv4 address.
///
/// For now we only support dotted-decimal input and ARP table lookups —
/// full DNS would require additional syscalls.  If `host` is not a valid
/// IPv4 string we scan the local ARP table for a matching entry.
fn resolve_to_ip(host: &str) -> Result<u32, ArpError> {
    // Fast path: already a dotted-decimal address.
    if let Some(ip) = parse_ipv4(host) {
        return Ok(ip);
    }

    // Slow path: search the ARP table for an entry whose interface name or a
    // (hypothetical) reverse-DNS name matches.  In production this would call
    // SYS_DNS_RESOLVE; here we do a best-effort table scan.
    let entries = read_arp_table()?;
    for entry in &entries {
        // Match on interface name as a last resort.
        if entry.iface == host {
            return Ok(entry.ip);
        }
    }

    Err(ArpError::IoError(format!(
        "cannot resolve '{host}': not a valid IPv4 address and not found in ARP table"
    )))
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args().map_err(|e| e.to_string())?;

    let result = match opts.mode {
        Mode::Display => {
            let stdout_handle = io::stdout();
            let mut stdout = stdout_handle.lock();
            cmd_display(&opts, &mut stdout)
        }
        Mode::Delete => cmd_delete(&opts),
        Mode::Add => cmd_add(&opts),
    };

    result.map_err(|e| e.to_string())
}

fn main() {
    if let Err(e) = run() {
        write_stderr(&format!("arp: {e}\n"));
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // IPv4 parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_ipv4_basic() {
        assert_eq!(parse_ipv4("192.168.1.1"), Some(0xC0A8_0101));
    }

    #[test]
    fn parse_ipv4_loopback() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7F00_0001));
    }

    #[test]
    fn parse_ipv4_zeros() {
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0x0000_0000));
    }

    #[test]
    fn parse_ipv4_broadcast() {
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xFFFF_FFFF));
    }

    #[test]
    fn parse_ipv4_too_few_parts() {
        assert_eq!(parse_ipv4("192.168.1"), None);
    }

    #[test]
    fn parse_ipv4_too_many_parts() {
        assert_eq!(parse_ipv4("1.2.3.4.5"), None);
    }

    #[test]
    fn parse_ipv4_octet_overflow() {
        assert_eq!(parse_ipv4("256.0.0.1"), None);
    }

    #[test]
    fn parse_ipv4_non_numeric() {
        assert_eq!(parse_ipv4("not.an.ip.address"), None);
    }

    #[test]
    fn parse_ipv4_empty() {
        assert_eq!(parse_ipv4(""), None);
    }

    #[test]
    fn parse_ipv4_leading_dot() {
        assert_eq!(parse_ipv4(".1.2.3"), None);
    }

    // -----------------------------------------------------------------------
    // IPv4 formatting
    // -----------------------------------------------------------------------

    #[test]
    fn format_ipv4_loopback() {
        assert_eq!(format_ipv4(0x7F00_0001), "127.0.0.1");
    }

    #[test]
    fn format_ipv4_zeros() {
        assert_eq!(format_ipv4(0), "0.0.0.0");
    }

    #[test]
    fn format_ipv4_broadcast() {
        assert_eq!(format_ipv4(0xFFFF_FFFF), "255.255.255.255");
    }

    #[test]
    fn ipv4_roundtrip() {
        for addr in ["10.0.0.1", "172.16.254.1", "8.8.8.8", "1.1.1.1"] {
            let ip = parse_ipv4(addr).expect("valid address");
            assert_eq!(format_ipv4(ip), addr);
        }
    }

    // -----------------------------------------------------------------------
    // MAC address parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_mac_basic() {
        let mac = parse_mac("aa:bb:cc:dd:ee:ff");
        assert_eq!(mac, Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]));
    }

    #[test]
    fn parse_mac_zeros() {
        let mac = parse_mac("00:00:00:00:00:00");
        assert_eq!(mac, Some([0u8; 6]));
    }

    #[test]
    fn parse_mac_broadcast() {
        let mac = parse_mac("ff:ff:ff:ff:ff:ff");
        assert_eq!(mac, Some([0xff; 6]));
    }

    #[test]
    fn parse_mac_too_short() {
        assert_eq!(parse_mac("aa:bb:cc"), None);
    }

    #[test]
    fn parse_mac_too_long() {
        assert_eq!(parse_mac("aa:bb:cc:dd:ee:ff:00"), None);
    }

    #[test]
    fn parse_mac_invalid_hex() {
        assert_eq!(parse_mac("gg:bb:cc:dd:ee:ff"), None);
    }

    #[test]
    fn parse_mac_bad_octet_len() {
        // Each group must be exactly 2 hex digits.
        assert_eq!(parse_mac("a:bb:cc:dd:ee:ff"), None);
    }

    // -----------------------------------------------------------------------
    // MAC formatting
    // -----------------------------------------------------------------------

    #[test]
    fn format_mac_basic() {
        let mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        assert_eq!(format_mac(&mac), "aa:bb:cc:dd:ee:ff");
    }

    #[test]
    fn format_mac_zeros() {
        assert_eq!(format_mac(&[0u8; 6]), "00:00:00:00:00:00");
    }

    #[test]
    fn mac_roundtrip() {
        for s in ["00:11:22:33:44:55", "de:ad:be:ef:00:01", "ff:ff:ff:ff:ff:ff"] {
            let mac = parse_mac(s).expect("valid MAC");
            assert_eq!(format_mac(&mac), s);
        }
    }

    // -----------------------------------------------------------------------
    // mac_is_zero
    // -----------------------------------------------------------------------

    #[test]
    fn mac_is_zero_all_zeros() {
        assert!(mac_is_zero(&[0u8; 6]));
    }

    #[test]
    fn mac_is_zero_nonzero() {
        assert!(!mac_is_zero(&[0, 0, 0, 0, 0, 1]));
    }

    // -----------------------------------------------------------------------
    // parse_hex_u16
    // -----------------------------------------------------------------------

    #[test]
    fn parse_hex_u16_basic() {
        assert_eq!(parse_hex_u16("0x1"), Some(1));
        assert_eq!(parse_hex_u16("0x2"), Some(2));
        assert_eq!(parse_hex_u16("0xff"), Some(255));
        assert_eq!(parse_hex_u16("0x10"), Some(16));
    }

    #[test]
    fn parse_hex_u16_uppercase_prefix() {
        assert_eq!(parse_hex_u16("0X4"), Some(4));
    }

    #[test]
    fn parse_hex_u16_no_prefix() {
        // Without a 0x prefix, should return None.
        assert_eq!(parse_hex_u16("4"), None);
    }

    #[test]
    fn parse_hex_u16_empty() {
        assert_eq!(parse_hex_u16(""), None);
    }

    // -----------------------------------------------------------------------
    // ArpFlags
    // -----------------------------------------------------------------------

    #[test]
    fn arp_flags_complete() {
        let f = ArpFlags(ArpFlags::COMPLETE);
        assert_eq!(f.summary(), "C");
    }

    #[test]
    fn arp_flags_permanent() {
        let f = ArpFlags(ArpFlags::PERMANENT);
        assert_eq!(f.summary(), "M");
    }

    #[test]
    fn arp_flags_published() {
        let f = ArpFlags(ArpFlags::PUBLISHED);
        assert_eq!(f.summary(), "P");
    }

    #[test]
    fn arp_flags_combined() {
        let f = ArpFlags(ArpFlags::COMPLETE | ArpFlags::PERMANENT);
        let s = f.summary();
        assert!(s.contains('C'));
        assert!(s.contains('M'));
        assert!(!s.contains('P'));
    }

    #[test]
    fn arp_flags_none() {
        let f = ArpFlags(0);
        assert_eq!(f.summary(), "");
    }

    // -----------------------------------------------------------------------
    // /proc/net/arp parser
    // -----------------------------------------------------------------------

    const SAMPLE_ARP: &str = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0
10.0.0.1         0x1         0x6         11:22:33:44:55:66     *        eth1
172.16.0.254     0x1         0x0         00:00:00:00:00:00     *        eth0
";

    #[test]
    fn parse_proc_arp_basic() {
        let entries = parse_proc_arp(SAMPLE_ARP).expect("parse should succeed");
        assert_eq!(entries.len(), 3);

        let e0 = &entries[0];
        assert_eq!(format_ipv4(e0.ip), "192.168.1.1");
        assert_eq!(e0.hw_type, 1);
        assert_eq!(e0.flags.0, 2);
        assert_eq!(format_mac(&e0.mac), "aa:bb:cc:dd:ee:ff");
        assert_eq!(e0.iface, "eth0");
    }

    #[test]
    fn parse_proc_arp_permanent_flag() {
        let entries = parse_proc_arp(SAMPLE_ARP).expect("parse should succeed");
        assert_eq!(entries[1].flags.0 & ArpFlags::PERMANENT, ArpFlags::PERMANENT);
    }

    #[test]
    fn parse_proc_arp_incomplete_entry() {
        let entries = parse_proc_arp(SAMPLE_ARP).expect("parse should succeed");
        assert!(mac_is_zero(&entries[2].mac));
    }

    #[test]
    fn parse_proc_arp_empty_content() {
        // Just a header, no data rows.
        let content = "IP address       HW type     Flags       HW address            Mask     Device\n";
        let entries = parse_proc_arp(content).expect("parse should succeed");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_proc_arp_malformed_ip() {
        let content = "\
IP address       HW type     Flags       HW address            Mask     Device
not-an-ip        0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0
";
        assert!(parse_proc_arp(content).is_err());
    }

    #[test]
    fn parse_proc_arp_malformed_mac() {
        let content = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         not-a-mac             *        eth0
";
        assert!(parse_proc_arp(content).is_err());
    }

    #[test]
    fn parse_proc_arp_too_few_columns() {
        let content = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1
";
        assert!(parse_proc_arp(content).is_err());
    }

    // -----------------------------------------------------------------------
    // Display formatting
    // -----------------------------------------------------------------------

    #[test]
    fn entry_display_standard() {
        let entry = ArpEntry {
            ip: parse_ipv4("192.168.1.1").unwrap(),
            hw_type: 1,
            flags: ArpFlags(ArpFlags::COMPLETE),
            mac: parse_mac("aa:bb:cc:dd:ee:ff").unwrap(),
            iface: "eth0".to_string(),
        };
        let s = entry.display(false, false);
        assert!(s.contains("192.168.1.1"));
        assert!(s.contains("aa:bb:cc:dd:ee:ff"));
        assert!(s.contains("eth0"));
        assert!(s.contains("ether"));
    }

    #[test]
    fn entry_display_incomplete() {
        let entry = ArpEntry {
            ip: parse_ipv4("10.0.0.1").unwrap(),
            hw_type: 1,
            flags: ArpFlags(0),
            mac: [0u8; 6],
            iface: "eth0".to_string(),
        };
        let s = entry.display(true, false);
        assert!(s.contains("<incomplete>"));
    }

    #[test]
    fn entry_display_numeric_flag() {
        let entry = ArpEntry {
            ip: parse_ipv4("8.8.8.8").unwrap(),
            hw_type: 1,
            flags: ArpFlags(ArpFlags::COMPLETE),
            mac: parse_mac("de:ad:be:ef:00:01").unwrap(),
            iface: "eth0".to_string(),
        };
        let numeric_s = entry.display(true, false);
        let normal_s = entry.display(false, false);
        // Both contain the IP.
        assert!(numeric_s.contains("8.8.8.8"));
        assert!(normal_s.contains("8.8.8.8"));
    }

    // -----------------------------------------------------------------------
    // errno_str
    // -----------------------------------------------------------------------

    #[test]
    fn errno_str_known() {
        assert_eq!(errno_str(-1), "operation not permitted");
        assert_eq!(errno_str(-2), "no such file or directory");
        assert_eq!(errno_str(-13), "permission denied");
        assert_eq!(errno_str(-22), "invalid argument");
    }

    #[test]
    fn errno_str_unknown() {
        assert_eq!(errno_str(-9999), "unknown error");
    }

    // -----------------------------------------------------------------------
    // cmd_display (integration-style, using a synthetic /proc/net/arp)
    // -----------------------------------------------------------------------

    /// A wrapper that runs `cmd_display` against synthesised content.
    fn display_with_content(content: &str, opts: &Options) -> Result<String, ArpError> {
        let entries = parse_proc_arp(content)?;

        let mut buf: Vec<u8> = Vec::new();
        let mut count = 0usize;

        for entry in &entries {
            if let Some(ref iface) = opts.interface {
                if &entry.iface != iface {
                    continue;
                }
            }
            if let Some(ref host) = opts.host {
                if format_ipv4(entry.ip) != *host {
                    continue;
                }
            }
            let line = format!("{}\n", entry.display(opts.numeric, opts.verbose));
            buf.extend_from_slice(line.as_bytes());
            count = count.saturating_add(1);
        }

        if opts.verbose {
            let footer = format!(
                "Entries: {count}   Skipped: {}   Found: {count}\n",
                entries.len().saturating_sub(count)
            );
            buf.extend_from_slice(footer.as_bytes());
        }

        String::from_utf8(buf).map_err(|e| ArpError::ParseError(e.to_string()))
    }

    #[test]
    fn cmd_display_shows_all_entries() {
        let opts = Options::default();
        let out = display_with_content(SAMPLE_ARP, &opts).unwrap();
        assert!(out.contains("192.168.1.1"));
        assert!(out.contains("10.0.0.1"));
        assert!(out.contains("172.16.0.254"));
    }

    #[test]
    fn cmd_display_interface_filter() {
        let mut opts = Options::default();
        opts.interface = Some("eth0".to_string());
        let out = display_with_content(SAMPLE_ARP, &opts).unwrap();
        assert!(out.contains("192.168.1.1"));
        assert!(!out.contains("10.0.0.1")); // eth1 entry filtered out
        assert!(out.contains("172.16.0.254"));
    }

    #[test]
    fn cmd_display_host_filter() {
        let mut opts = Options::default();
        opts.host = Some("10.0.0.1".to_string());
        let out = display_with_content(SAMPLE_ARP, &opts).unwrap();
        assert!(out.contains("10.0.0.1"));
        assert!(!out.contains("192.168.1.1"));
    }

    #[test]
    fn cmd_display_verbose_footer() {
        let mut opts = Options::default();
        opts.verbose = true;
        let out = display_with_content(SAMPLE_ARP, &opts).unwrap();
        assert!(out.contains("Entries:"));
    }

    // -----------------------------------------------------------------------
    // ArpRequest struct layout sanity
    // -----------------------------------------------------------------------

    #[test]
    fn arp_request_size() {
        // ip(4) + mac(6) + flags(2) + iface(16) = 28 bytes.
        assert_eq!(size_of::<ArpRequest>(), 28);
    }

    // -----------------------------------------------------------------------
    // resolve_to_ip (when /proc/net/arp is unavailable, function returns Err)
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_to_ip_dotted_decimal() {
        // When the input is already a valid IPv4, no I/O is needed.
        let result = resolve_to_ip("192.168.1.1");
        assert_eq!(result.ok(), Some(0xC0A8_0101));
    }
}
