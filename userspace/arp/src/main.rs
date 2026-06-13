//! SlateOS ARP Table Management Utility
//!
//! Displays the ARP (Address Resolution Protocol) cache.  The cache is read
//! through the dedicated `SYS_ARP_TABLE` syscall, which returns the resolved
//! IP→MAC mappings the kernel currently holds.
//!
//! # Usage
//!
//! ```text
//! arp                         Display all ARP cache entries
//! arp -a                      Display all ARP cache entries (explicit)
//! arp -n                      Display entries, numeric output only
//! arp -i eth0                 Limit display to interface eth0
//! arp -v                      Verbose output
//! arp -d hostname             Delete an ARP entry (unsupported on SlateOS)
//! arp -s hostname hw_addr     Add a static ARP entry (unsupported on SlateOS)
//! arp -D -s hostname iface    Use device MAC for a static entry (unsupported)
//! ```
//!
//! # ABI note
//!
//! SlateOS exposes `SYS_ARP_TABLE` (read) and `SYS_NET_IF_INFO` (interface
//! configuration, including the local MAC) but has **no** syscall to add,
//! delete, or probe ARP entries.  Those operations therefore report a clear
//! "not supported" error rather than silently invoking the wrong syscall.
//! See `todo.txt` for the design-gap note.

use std::env;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Syscall numbers (authoritative values from kernel/src/syscall/number.rs)
// ============================================================================

/// `SYS_ARP_TABLE` — read the ARP cache.
/// arg0 = ptr to output buffer, arg1 = buffer length in bytes.
/// Writes one 12-byte record per entry: [0..4] IPv4 (network order),
/// [4..10] MAC, [10..12] TTL seconds (u16 LE).  Returns the record count.
const SYS_ARP_TABLE: u64 = 843;

/// Size in bytes of a single `SYS_ARP_TABLE` record.
const ARP_RECORD_SIZE: usize = 12;

/// Maximum number of ARP records we are willing to read in one call.
const MAX_ARP_RECORDS: usize = 1024;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 3-argument syscall via the x86_64 `syscall` instruction.
///
/// # Safety
///
/// The caller must ensure:
/// - `nr` is a valid SlateOS syscall number.
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
// Diagnostics
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

/// Write a diagnostic string to stderr (best-effort).
fn write_stderr(msg: &str) {
    // A failed diagnostic write has no meaningful recovery path; ignore it.
    let _ = io::stderr().write_all(msg.as_bytes());
}

// ============================================================================
// IP / MAC address parsing and formatting
// ============================================================================

/// Parse a dotted-decimal IPv4 address into a `u32` whose big-endian byte
/// representation is `A.B.C.D`.  Returns `None` on malformed input.
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

/// Format a `u32` IPv4 address (big-endian byte order) as `A.B.C.D`.
fn format_ipv4(ip: u32) -> String {
    let b = ip.to_be_bytes();
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
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

/// ARP cache flags.  SlateOS reports only resolved/unresolved state, so the
/// vocabulary mirrors Linux's `arp` output for familiarity even though only
/// `COMPLETE` is currently synthesised from kernel data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArpFlags(pub u16);

impl ArpFlags {
    /// Entry is complete (resolved).
    pub const COMPLETE: u16 = 0x02;
    /// Entry was added manually (static / permanent).
    pub const PERMANENT: u16 = 0x04;
    /// Published — kernel answers ARP requests on behalf of this host.
    pub const PUBLISHED: u16 = 0x08;

    /// Human-readable flag summary in `CMP` style.
    pub fn summary(self) -> String {
        let mut s = String::with_capacity(3);
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
    /// IPv4 address of the peer (big-endian byte order: `to_be_bytes` = A.B.C.D).
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
    /// we have no resolver; the flag controls only the leading `?` host
    /// placeholder for compatibility with Linux `arp`).
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
            let flag_str = if flags.is_empty() {
                "<none>".to_string()
            } else {
                flags
            };
            format!(
                "{ip_str} ({ip_str}) at {mac_str} [{hw_name}] {flag_str} on {}",
                self.iface
            )
        } else {
            // Standard `arp -a` style: "? (1.2.3.4) at aa:bb:cc:dd:ee:ff [ether] on eth0"
            let host_part = if numeric {
                format!("({ip_str})")
            } else {
                format!("? ({ip_str})")
            };
            format!("{host_part} at {mac_str} [{hw_name}] on {}", self.iface)
        }
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur while reading the ARP table or handling a command.
#[derive(Debug)]
pub enum ArpError {
    /// An I/O error occurred while writing output.
    IoError(String),
    /// A syscall returned a negative error code.
    SyscallError(i64),
    /// The requested operation is not supported on SlateOS.
    Unsupported(&'static str),
}

impl std::fmt::Display for ArpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(msg) => write!(f, "I/O error: {msg}"),
            Self::SyscallError(code) => {
                write!(f, "syscall error {code}: {}", errno_str(*code))
            }
            Self::Unsupported(msg) => write!(f, "{msg}"),
        }
    }
}

// ============================================================================
// ARP table read (SYS_ARP_TABLE)
// ============================================================================

/// Parse a flat buffer of 12-byte `SYS_ARP_TABLE` records into `ArpEntry`s.
///
/// Each record: [0..4] IPv4 (network order = `A.B.C.D`), [4..10] MAC,
/// [10..12] TTL seconds (u16 LE).  A trailing partial record (if any) is
/// ignored by `chunks_exact`.
fn parse_arp_records(buf: &[u8]) -> Vec<ArpEntry> {
    buf.chunks_exact(ARP_RECORD_SIZE)
        .map(|rec| {
            // rec.len() == ARP_RECORD_SIZE (12) is guaranteed by chunks_exact.
            let ip = u32::from_be_bytes([rec[0], rec[1], rec[2], rec[3]]);
            let mac = [rec[4], rec[5], rec[6], rec[7], rec[8], rec[9]];
            // TTL (rec[10..12]) is read but not displayed; reserved for future use.
            let flags = if mac_is_zero(&mac) {
                ArpFlags(0)
            } else {
                ArpFlags(ArpFlags::COMPLETE)
            };
            ArpEntry {
                ip,
                hw_type: 1, // SYS_ARP_TABLE only tracks Ethernet peers.
                flags,
                mac,
                // The kernel exposes a single global interface; name it eth0
                // for output compatibility (see InterfaceInfo in net/interface).
                iface: "eth0".to_string(),
            }
        })
        .collect()
}

/// Read and parse the ARP cache via `SYS_ARP_TABLE`.
fn read_arp_table() -> Result<Vec<ArpEntry>, ArpError> {
    let mut buf = vec![0u8; MAX_ARP_RECORDS * ARP_RECORD_SIZE];
    let ret = unsafe {
        // SAFETY: buf is a valid, writable slice; we pass its pointer and exact
        // byte length.  SYS_ARP_TABLE writes at most that many bytes and returns
        // the number of 12-byte records written.
        syscall3(SYS_ARP_TABLE, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
    };
    if ret < 0 {
        return Err(ArpError::SyscallError(ret));
    }
    let count = usize::try_from(ret).unwrap_or(0);
    let byte_len = count.saturating_mul(ARP_RECORD_SIZE).min(buf.len());
    let records = buf.get(..byte_len).unwrap_or(&[]);
    Ok(parse_arp_records(records))
}

// ============================================================================
// CLI options
// ============================================================================

/// Mode of operation selected by the user.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Display the ARP table (default / `-a`).
    Display,
    /// Delete the entry for the given hostname/IP (`-d`) — unsupported.
    Delete,
    /// Add a static entry (`-s hostname hw_addr`) — unsupported.
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

Display the kernel ARP cache.

Options:
  -a, --all           Display all entries (default)
  -n                  Numeric output; do not resolve hostnames
  -v                  Verbose output
  -i <iface>          Limit to interface <iface>
  -d <hostname>       Delete ARP entry (unsupported on SlateOS)
  -s <hostname> <hw>  Add static entry (unsupported on SlateOS)
  -D                  With -s: use the device MAC (unsupported on SlateOS)
  -h, --help          Show this help

Note: SlateOS has no kernel syscall to add, delete, or probe ARP entries, so
the -d/-s/-D operations report \"not supported\".

Examples:
  arp                         Show all entries
  arp -n                      Show all entries, numeric IPs
  arp -i eth0                 Show entries for eth0
";
    let _ = io::stderr().write_all(msg.as_bytes());
}

fn parse_args() -> Result<Options, String> {
    let argv: Vec<String> = env::args().collect();
    let mut opts = Options::default();
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 1usize;

    while i < argv.len() {
        let arg = argv
            .get(i)
            .ok_or("internal: argv index out of range")?
            .as_str();
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

    // Validate -D: requires -s and an interface (Linux `arp -Ds host iface`
    // convention promotes the hw_addr argument to the interface name).
    if opts.use_device_mac {
        if opts.mode != Mode::Add {
            return Err("-D requires -s".to_string());
        }
        if opts.interface.is_none() {
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
fn cmd_display(opts: &Options, stdout: &mut dyn Write) -> Result<(), ArpError> {
    let entries = read_arp_table()?;

    // Column header for verbose mode.
    if opts.verbose {
        writeln!(
            stdout,
            "Address                  HWtype  HWaddress           Flags Mask     Iface"
        )
        .map_err(|e| ArpError::IoError(e.to_string()))?;
    }

    // Pre-resolve a host filter: a dotted-decimal IP is compared numerically
    // (robust against zero-padding), otherwise we fall back to matching the
    // interface name (no DNS resolver is available).
    let host_ip = opts.host.as_deref().and_then(parse_ipv4);

    let mut count = 0usize;

    for entry in &entries {
        // Interface filter.
        if let Some(ref iface) = opts.interface
            && &entry.iface != iface
        {
            continue;
        }
        // Host/IP filter.
        if let Some(ref host) = opts.host {
            let matches = host_ip == Some(entry.ip) || &entry.iface == host;
            if !matches {
                continue;
            }
        }

        if opts.verbose {
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
        writeln!(
            stdout,
            "Entries: {count}   Skipped: {}   Found: {count}",
            entries.len().saturating_sub(count)
        )
        .map_err(|e| ArpError::IoError(e.to_string()))?;
    }

    Ok(())
}

// ============================================================================
// Unsupported mutating operations
// ============================================================================

/// Deleting ARP entries is unsupported: the kernel exposes no ARP-delete
/// syscall.  Report a clear error instead of invoking the wrong syscall.
fn cmd_delete(_opts: &Options) -> Result<(), ArpError> {
    Err(ArpError::Unsupported(
        "deleting ARP entries is not supported on SlateOS: the kernel exposes no ARP-delete syscall",
    ))
}

/// Adding static ARP entries is unsupported: the kernel exposes no ARP-add
/// syscall.  Report a clear error instead of invoking the wrong syscall.
fn cmd_add(_opts: &Options) -> Result<(), ArpError> {
    Err(ArpError::Unsupported(
        "adding static ARP entries is not supported on SlateOS: the kernel exposes no ARP-add syscall",
    ))
}

// ============================================================================
// Entry point
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;

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
    // SYS_ARP_TABLE record parsing
    // -----------------------------------------------------------------------

    /// Build a 12-byte record: IP (network order), MAC, TTL (LE).
    fn make_record(ip: [u8; 4], mac: [u8; 6], ttl: u16) -> [u8; 12] {
        let mut r = [0u8; 12];
        r[0..4].copy_from_slice(&ip);
        r[4..10].copy_from_slice(&mac);
        r[10..12].copy_from_slice(&ttl.to_le_bytes());
        r
    }

    #[test]
    fn parse_records_basic() {
        let rec = make_record([192, 168, 1, 1], [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff], 60);
        let entries = parse_arp_records(&rec);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(format_ipv4(e.ip), "192.168.1.1");
        assert_eq!(format_mac(&e.mac), "aa:bb:cc:dd:ee:ff");
        assert_eq!(e.hw_type, 1);
        assert_eq!(e.iface, "eth0");
        // Resolved MAC → COMPLETE flag set.
        assert_eq!(e.flags.0 & ArpFlags::COMPLETE, ArpFlags::COMPLETE);
    }

    #[test]
    fn parse_records_incomplete_entry() {
        // All-zero MAC means unresolved → no COMPLETE flag.
        let rec = make_record([10, 0, 0, 1], [0u8; 6], 0);
        let entries = parse_arp_records(&rec);
        assert_eq!(entries.len(), 1);
        assert!(mac_is_zero(&entries[0].mac));
        assert_eq!(entries[0].flags.0 & ArpFlags::COMPLETE, 0);
    }

    #[test]
    fn parse_records_multiple() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&make_record(
            [192, 168, 1, 1],
            [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
            60,
        ));
        buf.extend_from_slice(&make_record(
            [10, 0, 0, 1],
            [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
            30,
        ));
        let entries = parse_arp_records(&buf);
        assert_eq!(entries.len(), 2);
        assert_eq!(format_ipv4(entries[0].ip), "192.168.1.1");
        assert_eq!(format_ipv4(entries[1].ip), "10.0.0.1");
    }

    #[test]
    fn parse_records_empty() {
        let entries = parse_arp_records(&[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_records_ignores_trailing_partial() {
        // 12 valid bytes + 5 trailing bytes that don't form a record.
        let mut buf = make_record([1, 2, 3, 4], [1, 2, 3, 4, 5, 6], 10).to_vec();
        buf.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0x00]);
        let entries = parse_arp_records(&buf);
        assert_eq!(entries.len(), 1);
        assert_eq!(format_ipv4(entries[0].ip), "1.2.3.4");
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
            mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
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
            mac: [0xde, 0xad, 0xbe, 0xef, 0x00, 0x01],
            iface: "eth0".to_string(),
        };
        let numeric_s = entry.display(true, false);
        let normal_s = entry.display(false, false);
        assert!(numeric_s.contains("8.8.8.8"));
        assert!(normal_s.contains("8.8.8.8"));
        // Non-numeric output carries the leading "?" host placeholder.
        assert!(normal_s.contains('?'));
        assert!(!numeric_s.contains('?'));
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
    // Unsupported operations
    // -----------------------------------------------------------------------

    #[test]
    fn cmd_delete_is_unsupported() {
        let opts = Options {
            mode: Mode::Delete,
            host: Some("192.168.1.1".to_string()),
            ..Options::default()
        };
        match cmd_delete(&opts) {
            Err(ArpError::Unsupported(_)) => {}
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn cmd_add_is_unsupported() {
        let opts = Options {
            mode: Mode::Add,
            host: Some("192.168.1.50".to_string()),
            hw_addr: Some("aa:bb:cc:dd:ee:ff".to_string()),
            ..Options::default()
        };
        match cmd_add(&opts) {
            Err(ArpError::Unsupported(_)) => {}
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // ArpError Display
    // -----------------------------------------------------------------------

    #[test]
    fn arp_error_display_unsupported() {
        let e = ArpError::Unsupported("nope");
        assert_eq!(e.to_string(), "nope");
    }

    #[test]
    fn arp_error_display_syscall() {
        let e = ArpError::SyscallError(-22);
        assert!(e.to_string().contains("invalid argument"));
    }

    // -----------------------------------------------------------------------
    // cmd_display (integration-style) against synthesised records
    // -----------------------------------------------------------------------

    /// Run the same filtering/formatting logic as `cmd_display` against an
    /// in-memory set of entries (avoids the SYS_ARP_TABLE syscall in tests).
    fn display_entries(entries: &[ArpEntry], opts: &Options) -> String {
        let host_ip = opts.host.as_deref().and_then(parse_ipv4);
        let mut buf: Vec<u8> = Vec::new();
        for entry in entries {
            if let Some(ref iface) = opts.interface
                && &entry.iface != iface
            {
                continue;
            }
            if opts.host.is_some() && host_ip != Some(entry.ip) {
                continue;
            }
            let line = format!("{}\n", entry.display(opts.numeric, opts.verbose));
            buf.extend_from_slice(line.as_bytes());
        }
        String::from_utf8(buf).unwrap_or_default()
    }

    fn sample_entries() -> Vec<ArpEntry> {
        vec![
            ArpEntry {
                ip: parse_ipv4("192.168.1.1").unwrap(),
                hw_type: 1,
                flags: ArpFlags(ArpFlags::COMPLETE),
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                iface: "eth0".to_string(),
            },
            ArpEntry {
                ip: parse_ipv4("10.0.0.1").unwrap(),
                hw_type: 1,
                flags: ArpFlags(ArpFlags::COMPLETE),
                mac: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
                iface: "eth1".to_string(),
            },
        ]
    }

    #[test]
    fn cmd_display_shows_all_entries() {
        let opts = Options::default();
        let out = display_entries(&sample_entries(), &opts);
        assert!(out.contains("192.168.1.1"));
        assert!(out.contains("10.0.0.1"));
    }

    #[test]
    fn cmd_display_interface_filter() {
        let opts = Options {
            interface: Some("eth0".to_string()),
            ..Options::default()
        };
        let out = display_entries(&sample_entries(), &opts);
        assert!(out.contains("192.168.1.1"));
        assert!(!out.contains("10.0.0.1"));
    }

    #[test]
    fn cmd_display_host_filter() {
        let opts = Options {
            host: Some("10.0.0.1".to_string()),
            ..Options::default()
        };
        let out = display_entries(&sample_entries(), &opts);
        assert!(out.contains("10.0.0.1"));
        assert!(!out.contains("192.168.1.1"));
    }
}
