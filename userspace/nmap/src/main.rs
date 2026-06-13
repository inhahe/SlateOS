//! Slate OS Network Port Scanner (nmap)
//!
//! A network reconnaissance tool that supports TCP connect scanning,
//! host discovery via ICMP ping, service version detection via banner
//! grabbing, OS heuristics from TTL, and flexible port/host specification.
//!
//! # Usage
//!
//! ```text
//! nmap <host>                     Scan top-1000 ports (TCP connect)
//! nmap -p 1-1024 <host>           Scan ports 1 through 1024
//! nmap -p 22,80,443 <host>        Scan specific ports
//! nmap -p- <host>                 Scan all 65535 ports
//! nmap -sT <host>                 TCP connect scan (default)
//! nmap -sP 192.168.1.0/24         Ping scan — host discovery only
//! nmap -sV <host>                 Banner-grab open ports for version info
//! nmap -O <host>                  OS detection via TTL heuristic
//! nmap -Pn <host>                 Skip host discovery (assume up)
//! nmap -v <host>                  Verbose (show closed/filtered ports)
//! nmap --open <host>              Only show open ports
//! nmap -oN out.txt <host>         Save normal output to file
//! nmap -T0 .. -T5 <host>          Timing templates (0=paranoid, 5=insane)
//! ```

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, and the defensive lints
// (unwrap_used, expect_used, panic, indexing_slicing, arithmetic_side_effects)
// already set to warn at workspace scope — see the root Cargo.toml.

use std::env;
use std::fs::File;
use std::io::Write;

// ============================================================================
// Syscall numbers
// ============================================================================

/// Blocking TCP connect: arg1=ip(u32), arg2=port(u16), arg3=nonblock_flag(0/1).
/// Returns connection handle (>= 0) or negative errno.
const SYS_TCP_CONNECT: u64 = 800;
/// Send data: arg1=handle, arg2=buf_ptr, arg3=len. Returns bytes sent or neg.
const SYS_TCP_SEND: u64 = 801;
/// Receive data: arg1=handle, arg2=buf_ptr, arg3=len. Returns bytes read or neg.
const SYS_TCP_RECV: u64 = 802;
/// Close connection: arg1=handle. Returns 0 or negative errno.
const SYS_TCP_CLOSE: u64 = 803;
/// Poll connection status: arg1=handle. Returns status code (see TCP_STATUS_*).
const SYS_TCP_POLL_STATUS: u64 = 845;
/// Send ICMP echo request: arg1=ip(u32), arg2=seq(u16), arg3=payload_size(u16).
const SYS_ICMP_SEND: u64 = 830;
/// Wait for ICMP echo reply: arg1=timeout_ms(u64). Returns RTT µs or neg.
const SYS_ICMP_RECV: u64 = 831;
/// DNS resolve: arg1=ptr, arg2=len, arg3=result_ptr(u32). Returns 0 or neg.
const SYS_DNS_RESOLVE: u64 = 820;
/// nanosleep-style sleep: arg1=milliseconds.
const SYS_SLEEP: u64 = 11;
/// Native Slate OS monotonic clock (kernel syscall/number.rs); no-arg, returns
/// boot-relative nanoseconds in rax.  Used only for elapsed-time (RTT)
/// measurement, so monotonic — not realtime — is the correct clock.
/// (Syscall 40 is SYS_PORT_READ; the old SYS_CLOCK_GETTIME=40 was wrong.)
const SYS_CLOCK_MONOTONIC: u64 = 10;

// NOTE: file descriptor I/O (stdout/stderr writes, the -oN output file) and
// process exit go through std (std::io, std::fs, std::process::exit), which is
// backed by the Slate OS libc/posix layer and issues the correct native FS
// syscalls.  An earlier version hand-rolled these with Linux syscall numbers
// (write=1, open=2, close=3, exit=60) — on the native ABI those map to
// SYS_EXIT, SYS_TASK_ID, an unimplemented slot, and SYS_SYSCTL_GET, so writing
// output terminated the process and "exit" hung in a loop.

// TCP poll status codes returned by SYS_TCP_POLL_STATUS
const TCP_STATUS_CONNECTED: i64 = 1;
const TCP_STATUS_REFUSED: i64 = 2;
const TCP_STATUS_TIMEOUT: i64 = 3;
const TCP_STATUS_IN_PROGRESS: i64 = 4;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 1-argument syscall via the x86_64 `syscall` instruction.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and `a1` is valid
/// for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall1(nr: u64, a1: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees all arguments are valid for `nr`.
    // The `syscall` instruction clobbers rcx and r11 per the x86_64 ABI.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Issue a 3-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees all arguments are valid for `nr`.
    // The `syscall` instruction clobbers rcx and r11 per the x86_64 ABI.
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

/// Issue a 4-argument syscall.
///
/// # Safety
///
/// The caller must ensure `nr` is a valid syscall number and all arguments
/// are valid for the specific syscall. The 4th argument goes in r10 (not rcx)
/// because `syscall` clobbers rcx.
// syscall4 is part of the complete syscall ABI; kept for future use.
#[allow(dead_code)]
#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees all arguments are valid for `nr`.
    // r10 carries arg3 per the syscall ABI — rcx is clobbered by `syscall`.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            in("r10") a4,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Open a non-blocking TCP connection to `(ip, port)`.
/// Returns a handle on success, or a negative error code.
fn tcp_connect_nonblock(ip: u32, port: u16) -> Result<u64, i64> {
    // SAFETY: SYS_TCP_CONNECT takes three scalar arguments; arg3=1 requests
    // non-blocking mode. No pointer dereferences in userspace.
    let ret = unsafe { syscall3(SYS_TCP_CONNECT, u64::from(ip), u64::from(port), 1) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as u64)
    }
}

/// Close a TCP connection identified by `handle`.
fn tcp_close(handle: u64) {
    // SAFETY: SYS_TCP_CLOSE takes one scalar handle argument.
    let _ = unsafe { syscall1(SYS_TCP_CLOSE, handle) };
}

/// Poll the connection status of `handle`.
fn tcp_poll_status(handle: u64) -> i64 {
    // SAFETY: SYS_TCP_POLL_STATUS takes one scalar handle argument.
    unsafe { syscall1(SYS_TCP_POLL_STATUS, handle) }
}

/// Receive up to `buf.len()` bytes from `handle` into `buf`.
/// Returns the number of bytes read, or a negative error code.
fn tcp_recv(handle: u64, buf: &mut [u8]) -> i64 {
    // SAFETY: handle is valid, buf pointer and len come from a live Rust slice.
    unsafe { syscall3(SYS_TCP_RECV, handle, buf.as_mut_ptr() as u64, buf.len() as u64) }
}

/// Send `data` on `handle`. Returns bytes sent or negative error code.
fn tcp_send(handle: u64, data: &[u8]) -> i64 {
    // SAFETY: handle is valid, data pointer and len come from a live Rust slice.
    unsafe { syscall3(SYS_TCP_SEND, handle, data.as_ptr() as u64, data.len() as u64) }
}

/// Send an ICMP echo request to `ip` with sequence number `seq`.
/// Returns 0 on success, negative on error.
fn icmp_send(ip: u32, seq: u16) -> Result<(), i64> {
    // SAFETY: SYS_ICMP_SEND takes three scalar arguments; payload_size = 32.
    let ret = unsafe { syscall3(SYS_ICMP_SEND, u64::from(ip), u64::from(seq), 32) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(())
    }
}

/// Wait up to `timeout_ms` milliseconds for an ICMP echo reply.
/// Returns round-trip time in microseconds, or negative error code.
fn icmp_recv(timeout_ms: u64) -> i64 {
    // SAFETY: SYS_ICMP_RECV takes one scalar timeout argument in milliseconds.
    unsafe { syscall1(SYS_ICMP_RECV, timeout_ms) }
}

/// Resolve `hostname` to an IPv4 address in network byte order.
/// Returns the address on success, or a negative error code.
fn dns_resolve(hostname: &str) -> Result<u32, i64> {
    let mut result: u32 = 0;
    let bytes = hostname.as_bytes();
    // SAFETY: bytes ptr+len are valid. result is a live local u32.
    let ret = unsafe {
        syscall3(
            SYS_DNS_RESOLVE,
            bytes.as_ptr() as u64,
            bytes.len() as u64,
            (&raw mut result) as u64,
        )
    };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(result)
    }
}

/// Sleep for `ms` milliseconds.
fn sleep_ms(ms: u64) {
    // SAFETY: SYS_SLEEP takes one scalar millisecond argument.
    let _ = unsafe { syscall1(SYS_SLEEP, ms) };
}

/// Exit the process with `code`.
///
/// Delegates to `std::process::exit`, which runs through the Slate OS libc and
/// issues the correct native `SYS_EXIT`.
fn exit(code: i32) -> ! {
    std::process::exit(code)
}

// Boot-relative nanoseconds via SYS_CLOCK_MONOTONIC.  Used only to measure
// elapsed time, so a monotonic (never-stepped) source is exactly right.
fn clock_nanos() -> u64 {
    // SAFETY: SYS_CLOCK_MONOTONIC takes no pointer arguments and returns the
    // nanosecond count directly in rax.
    let ret = unsafe { syscall3(SYS_CLOCK_MONOTONIC, 0, 0, 0) };
    if ret < 0 { 0 } else { ret as u64 }
}

// ============================================================================
// Port → service name database
// ============================================================================

/// Return the well-known service name for `port`, if known.
fn service_name(port: u16) -> &'static str {
    match port {
        7 => "echo",
        13 => "daytime",
        19 => "chargen",
        20 => "ftp-data",
        21 => "ftp",
        22 => "ssh",
        23 => "telnet",
        25 => "smtp",
        37 => "time",
        43 => "whois",
        53 => "domain",
        67 => "bootps",
        68 => "bootpc",
        69 => "tftp",
        70 => "gopher",
        79 => "finger",
        80 => "http",
        88 => "kerberos",
        102 => "iso-tsap",
        110 => "pop3",
        111 => "sunrpc",
        113 => "ident",
        119 => "nntp",
        123 => "ntp",
        135 => "msrpc",
        137 => "netbios-ns",
        138 => "netbios-dgm",
        139 => "netbios-ssn",
        143 => "imap",
        161 => "snmp",
        162 => "snmptrap",
        179 => "bgp",
        194 => "irc",
        220 => "imap3",
        389 => "ldap",
        443 => "https",
        445 => "microsoft-ds",
        465 => "smtps",
        500 => "isakmp",
        513 => "login",
        514 => "shell",
        515 => "printer",
        520 => "router",
        587 => "submission",
        631 => "ipp",
        636 => "ldaps",
        993 => "imaps",
        995 => "pop3s",
        1080 => "socks",
        1194 => "openvpn",
        1433 => "ms-sql-s",
        1521 => "oracle",
        1723 => "pptp",
        2049 => "nfs",
        2181 => "zookeeper",
        3128 => "squid-http",
        3306 => "mysql",
        3389 => "ms-wbt-server",
        3690 => "svn",
        4444 => "krb524",
        4500 => "nat-t-ike",
        5000 => "upnp",
        5432 => "postgresql",
        5900 => "vnc",
        5985 => "wsman",
        5986 => "wsmans",
        6379 => "redis",
        6443 => "sun-sr-https",
        6667 => "irc",
        7001 => "afs3-callback",
        8080 => "http-proxy",
        8443 => "https-alt",
        8888 => "sun-answerbook",
        9090 => "zeus-admin",
        9200 => "elasticsearch",
        9300 => "vrace",
        27017 => "mongodb",
        _ => "",
    }
}

// ============================================================================
// Top-1000 common ports (abridged to 100 representative + top critical ones)
// Full nmap top-1000 is large; this covers the most scanned ports in practice.
// ============================================================================

/// Returns the default set of ports to scan (top ~100 common ports).
fn default_ports() -> Vec<u16> {
    vec![
        1, 3, 4, 6, 7, 9, 13, 17, 19, 20, 21, 22, 23, 24, 25, 26, 30, 32, 33, 37, 42, 43, 49, 53,
        70, 79, 80, 81, 82, 83, 84, 85, 88, 89, 90, 99, 100, 106, 109, 110, 111, 113, 119, 125,
        135, 139, 143, 144, 146, 161, 163, 179, 199, 211, 212, 222, 254, 255, 256, 259, 264, 280,
        301, 306, 311, 340, 366, 389, 406, 407, 416, 417, 425, 427, 443, 444, 445, 458, 464, 465,
        481, 497, 500, 512, 513, 514, 515, 524, 541, 543, 544, 545, 548, 554, 555, 563, 587, 593,
        616, 617, 625, 631, 636, 646, 648, 666, 667, 668, 683, 687, 691, 700, 705, 711, 714, 720,
        722, 726, 749, 765, 777, 783, 787, 800, 801, 808, 843, 873, 880, 888, 898, 900, 901, 902,
        903, 911, 912, 981, 987, 990, 992, 993, 995, 999, 1000, 1001, 1002, 1007, 1009, 1010,
        1011, 1021, 1022, 1023, 1024, 1025, 1026, 1027, 1028, 1029, 1030, 1110, 1194, 1234, 1433,
        1521, 1720, 1723, 1755, 1900, 2000, 2001, 2049, 2100, 2181, 3000, 3128, 3306, 3389, 3690,
        4000, 4444, 4500, 5000, 5432, 5900, 6379, 6667, 7001, 8000, 8080, 8443, 8888, 9000, 9090,
        9200, 9300, 9999, 10000, 27017,
    ]
}

// ============================================================================
// CIDR / host range parsing
// ============================================================================

/// Parse a dotted-decimal IPv4 string into a u32 (network byte order).
fn parse_ipv4(s: &str) -> Option<u32> {
    let mut parts = s.splitn(4, '.');
    let a: u8 = parts.next()?.parse().ok()?;
    let b: u8 = parts.next()?.parse().ok()?;
    let c: u8 = parts.next()?.parse().ok()?;
    let d: u8 = parts.next()?.parse().ok()?;
    // Verify no trailing text after the 4th octet.
    Some(u32::from_be_bytes([a, b, c, d]))
}

/// Format a u32 network-byte-order IP as "a.b.c.d".
fn fmt_ipv4(ip: u32) -> String {
    let [a, b, c, d] = ip.to_be_bytes();
    format!("{a}.{b}.{c}.{d}")
}

/// Expand a host specification into a list of IPs to scan.
///
/// Accepts:
/// - A dotted-decimal IP: `192.168.1.1`
/// - A CIDR range: `192.168.1.0/24`
/// - A hostname: resolved via DNS
///
/// Returns `(display_name, list_of_ip_u32)`.
fn expand_target(spec: &str) -> Result<(String, Vec<u32>), String> {
    // CIDR notation
    if let Some(slash) = spec.find('/') {
        let ip_str = &spec[..slash];
        let prefix_str = &spec[slash.saturating_add(1)..];
        let prefix: u32 = prefix_str
            .parse()
            .map_err(|_| format!("bad CIDR prefix: {prefix_str}"))?;
        if prefix > 32 {
            return Err(format!("CIDR prefix /{prefix} out of range"));
        }
        let base_ip = parse_ipv4(ip_str)
            .ok_or_else(|| format!("invalid IP in CIDR: {ip_str}"))?;
        let mask = if prefix == 0 {
            0u32
        } else {
            !0u32 << (32u32.saturating_sub(prefix))
        };
        let network = base_ip & mask;
        let host_bits = 32u32.saturating_sub(prefix);
        let count = 1u32 << host_bits;
        let mut ips = Vec::with_capacity(count as usize);
        for i in 0..count {
            ips.push(network.wrapping_add(i));
        }
        return Ok((spec.to_string(), ips));
    }

    // Plain dotted-decimal IP
    if let Some(ip) = parse_ipv4(spec) {
        return Ok((spec.to_string(), vec![ip]));
    }

    // Hostname — resolve via DNS
    match dns_resolve(spec) {
        Ok(ip) => Ok((format!("{spec} ({})", fmt_ipv4(ip)), vec![ip])),
        Err(e) => Err(format!("DNS resolution failed for {spec}: error {e}")),
    }
}

// ============================================================================
// Port specification parsing
// ============================================================================

/// Parse a port specification string into a sorted, deduplicated list of u16.
///
/// Accepts:
/// - `-` alone: all 65535 ports
/// - `80`: single port
/// - `1-1024`: inclusive range
/// - `22,80,443`: comma-separated list or ranges
fn parse_ports(spec: &str) -> Result<Vec<u16>, String> {
    if spec == "-" {
        return Ok((1u16..=65535).collect());
    }
    let mut ports = Vec::new();
    for segment in spec.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if let Some(dash) = segment.find('-') {
            let lo_str = &segment[..dash];
            let hi_str = &segment[dash.saturating_add(1)..];
            let lo: u16 = lo_str
                .parse()
                .map_err(|_| format!("invalid port: {lo_str}"))?;
            let hi: u16 = hi_str
                .parse()
                .map_err(|_| format!("invalid port: {hi_str}"))?;
            if lo > hi {
                return Err(format!("port range {lo}-{hi} is backwards"));
            }
            for p in lo..=hi {
                ports.push(p);
            }
        } else {
            let p: u16 = segment
                .parse()
                .map_err(|_| format!("invalid port number: {segment}"))?;
            ports.push(p);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

// ============================================================================
// Timing templates
// ============================================================================

/// Timing parameters derived from a `-T<n>` template.
#[derive(Debug, Clone, Copy)]
struct Timing {
    /// How long to wait for a connect to complete (ms).
    connect_timeout_ms: u64,
    /// Maximum simultaneous in-flight connections.
    max_parallel: usize,
    /// Delay between batches (ms), 0 for insane timing.
    inter_batch_delay_ms: u64,
}

impl Timing {
    fn from_level(level: u8) -> Self {
        match level {
            0 => Self {
                connect_timeout_ms: 5000,
                max_parallel: 1,
                inter_batch_delay_ms: 300_000,
            },
            1 => Self {
                connect_timeout_ms: 3000,
                max_parallel: 1,
                inter_batch_delay_ms: 15_000,
            },
            2 => Self {
                connect_timeout_ms: 2000,
                max_parallel: 8,
                inter_batch_delay_ms: 400,
            },
            3 => Self {
                // default
                connect_timeout_ms: 1000,
                max_parallel: 64,
                inter_batch_delay_ms: 0,
            },
            4 => Self {
                connect_timeout_ms: 500,
                max_parallel: 128,
                inter_batch_delay_ms: 0,
            },
            _ => Self {
                // T5 insane
                connect_timeout_ms: 250,
                max_parallel: 256,
                inter_batch_delay_ms: 0,
            },
        }
    }
}

// ============================================================================
// Scan types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanType {
    /// TCP connect scan (default).
    TcpConnect,
    /// Ping scan — host discovery only, no port scanning.
    PingScan,
}

// ============================================================================
// Port state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortState {
    Open,
    Closed,
    Filtered,
}

impl PortState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Filtered => "filtered",
        }
    }
}

// ============================================================================
// Scan result per port
// ============================================================================

#[derive(Debug, Clone)]
struct PortResult {
    port: u16,
    state: PortState,
    /// Banner grabbed from the port (if -sV and open).
    banner: Option<String>,
}

// ============================================================================
// Host result
// ============================================================================

#[derive(Debug, Clone)]
struct HostResult {
    ip: u32,
    display: String,
    is_up: bool,
    /// RTT of the ping probe in µs, or None if skipped / down.
    ping_rtt_us: Option<i64>,
    ports: Vec<PortResult>,
    /// TTL-based OS guess (if -O flag).
    os_guess: Option<String>,
}

// ============================================================================
// Config (parsed command-line arguments)
// ============================================================================

#[derive(Debug, Clone)]
struct Config {
    targets: Vec<String>,
    ports: Vec<u16>,
    scan_type: ScanType,
    /// -sV: grab banners from open ports.
    version_detect: bool,
    /// -O: TTL-based OS detection.
    os_detect: bool,
    /// -Pn: skip host discovery.
    no_ping: bool,
    /// -v: show closed/filtered ports.
    verbose: bool,
    /// --open: only print open ports.
    only_open: bool,
    /// -oN file: output file path.
    output_file: Option<String>,
    timing: Timing,
}

impl Config {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            ports: default_ports(),
            scan_type: ScanType::TcpConnect,
            version_detect: false,
            os_detect: false,
            no_ping: false,
            verbose: false,
            only_open: false,
            output_file: None,
            timing: Timing::from_level(3),
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut i = 1usize;
    let mut port_spec_set = false;

    while let Some(arg_owned) = args.get(i) {
        let arg = arg_owned.as_str();
        match arg {
            "-sT" => {
                cfg.scan_type = ScanType::TcpConnect;
            }
            "-sP" | "-sn" => {
                cfg.scan_type = ScanType::PingScan;
            }
            "-sV" => {
                cfg.version_detect = true;
            }
            "-O" => {
                cfg.os_detect = true;
            }
            "-Pn" => {
                cfg.no_ping = true;
            }
            "-v" => {
                cfg.verbose = true;
            }
            "--open" => {
                cfg.only_open = true;
            }
            "-p" => {
                i = i.saturating_add(1);
                let spec = args.get(i).ok_or("missing argument for -p")?;
                cfg.ports = parse_ports(spec)?;
                port_spec_set = true;
            }
            "-p-" => {
                cfg.ports = parse_ports("-")?;
                port_spec_set = true;
            }
            "-oN" => {
                i = i.saturating_add(1);
                let file = args.get(i).ok_or("missing argument for -oN")?;
                cfg.output_file = Some(file.clone());
            }
            "-T0" => {
                cfg.timing = Timing::from_level(0);
            }
            "-T1" => {
                cfg.timing = Timing::from_level(1);
            }
            "-T2" => {
                cfg.timing = Timing::from_level(2);
            }
            "-T3" => {
                cfg.timing = Timing::from_level(3);
            }
            "-T4" => {
                cfg.timing = Timing::from_level(4);
            }
            "-T5" => {
                cfg.timing = Timing::from_level(5);
            }
            "--help" | "-h" => {
                print_usage();
                exit(0);
            }
            _ if arg.starts_with("-p") => {
                // -p1-1024 style (no space)
                let spec = &arg[2..];
                cfg.ports = parse_ports(spec)?;
                port_spec_set = true;
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                cfg.targets.push(arg.to_string());
            }
        }
        i = i.saturating_add(1);
    }

    // -sP overrides any port spec; ports are irrelevant for ping scan.
    if cfg.scan_type == ScanType::PingScan && port_spec_set {
        // Quietly ignore — ping scan doesn't use port list.
        cfg.ports.clear();
    }

    if cfg.targets.is_empty() {
        return Err("no target specified".to_string());
    }

    Ok(cfg)
}

// ============================================================================
// Output helpers
// ============================================================================

/// Write `s` to stdout (and optionally to an output file).
fn out(s: &str, out_file: Option<&mut File>) {
    let bytes = s.as_bytes();
    // Best-effort: a failed write to stdout or the report file should not abort
    // an in-progress scan.
    let _ = std::io::stdout().write_all(bytes);
    if let Some(f) = out_file {
        let _ = f.write_all(bytes);
    }
}

fn print_usage() {
    let usage = "\
Usage: nmap [options] <target> [<target>...]

Targets:
  192.168.1.1           single IP
  192.168.1.0/24        CIDR range
  hostname              resolved via DNS

Scan types:
  -sT                   TCP connect scan (default)
  -sP / -sn             Ping scan (host discovery only)
  -sV                   Service version detection (banner grab)

Port specification:
  -p 80                 single port
  -p 1-1024             range
  -p 22,80,443          comma list
  -p-                   all 65535 ports
  (default: top-100 common ports)

Host discovery:
  -Pn                   skip ping, assume host is up

Timing (-T0 paranoid .. -T5 insane):
  -T0  -T1  -T2  -T3  -T4  -T5

Detection:
  -O                    OS detection (TTL heuristic)

Output:
  -v                    verbose (show closed/filtered ports)
  --open                only show open ports
  -oN <file>            save normal output to file

Examples:
  nmap 192.168.1.1
  nmap -sV -p 1-1024 192.168.1.1
  nmap -sP 192.168.1.0/24
  nmap -T4 -p- 10.0.0.1
";
    let _ = std::io::stdout().write_all(usage.as_bytes());
}

/// Print to stderr.
fn eprint_str(s: &str) {
    let _ = std::io::stderr().write_all(s.as_bytes());
}

// ============================================================================
// Host discovery (ICMP ping)
// ============================================================================

/// Ping `ip` up to 3 times. Returns (is_up, rtt_µs) on the first reply.
fn ping_host(ip: u32, timeout_ms: u64) -> (bool, Option<i64>) {
    for seq in 0u16..3 {
        if icmp_send(ip, seq).is_err() {
            return (false, None);
        }
        let rtt = icmp_recv(timeout_ms);
        if rtt >= 0 {
            return (true, Some(rtt));
        }
        // Negative return from icmp_recv means timeout; try next probe.
    }
    (false, None)
}

// ============================================================================
// Banner grabbing (-sV)
// ============================================================================

/// Connect to `(ip, port)` and read the first 256 bytes of the greeting.
/// Returns `None` if the port doesn't send an unsolicited banner.
fn grab_banner(ip: u32, port: u16, timeout_ms: u64) -> Option<String> {
    let handle = match tcp_connect_nonblock(ip, port) {
        Ok(h) => h,
        Err(_) => return None,
    };

    // Wait for connect.
    let deadline = clock_nanos().saturating_add(timeout_ms.saturating_mul(1_000_000));
    loop {
        let status = tcp_poll_status(handle);
        match status {
            TCP_STATUS_CONNECTED => break,
            TCP_STATUS_REFUSED | TCP_STATUS_TIMEOUT => {
                tcp_close(handle);
                return None;
            }
            _ => {
                if clock_nanos() >= deadline {
                    tcp_close(handle);
                    return None;
                }
                sleep_ms(5);
            }
        }
    }

    // Send a probe for HTTP ports to elicit a response.
    if matches!(port, 80 | 8080 | 8000 | 8443 | 443) {
        let req = b"HEAD / HTTP/1.0\r\nHost: x\r\n\r\n";
        let _ = tcp_send(handle, req);
    }

    // Read with a short additional timeout.
    let mut buf = [0u8; 256];
    let banner_deadline = clock_nanos().saturating_add(2_000_000_000); // 2s
    let mut total = 0usize;
    while let Some(tail) = buf.get_mut(total..) {
        let n = tcp_recv(handle, tail);
        if n > 0 {
            total = total.saturating_add(n as usize).min(256);
            break;
        }
        if n < 0 || clock_nanos() >= banner_deadline {
            break;
        }
        sleep_ms(20);
    }
    tcp_close(handle);

    if total == 0 {
        return None;
    }

    // Convert to printable string, replacing non-ASCII with '.'
    let raw = buf.get(..total).unwrap_or(&[]);
    let banner: String = raw
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(80)
        .collect();

    if banner.is_empty() {
        None
    } else {
        Some(banner)
    }
}

// ============================================================================
// OS detection (TTL heuristic)
// ============================================================================

/// Return an OS guess based on ICMP TTL value. The TTL is typically 64
/// (Linux/macOS), 128 (Windows), or 255 (some routers/BSD).
///
/// We infer TTL from the ICMP ping RTT sign convention: the kernel embeds
/// TTL in the upper 16 bits of the RTT return value when `ip` matches the
/// last reply source.
fn guess_os_from_ttl(rtt_raw: i64) -> Option<String> {
    // TTL is packed into bits 48..63 of a non-negative rtt result.
    // Kernel encodes it as: result = (ttl as i64) << 48 | rtt_us
    // A value of 0 means TTL was not provided.
    let ttl = ((rtt_raw as u64) >> 48) as u8;
    match ttl {
        0 => None,
        1..=64 => Some("Linux / macOS (TTL ≤ 64)".to_string()),
        65..=128 => Some("Windows (TTL ≤ 128)".to_string()),
        129..=255 => Some("Router / BSD / embedded (TTL ≤ 255)".to_string()),
    }
}

// ============================================================================
// Batch TCP scan (respects timing parallelism)
// ============================================================================

/// Scan a list of ports on `ip` using TCP connect probes.
/// Processes ports in batches of `timing.max_parallel`.
fn scan_ports(
    ip: u32,
    ports: &[u16],
    timing: &Timing,
    version_detect: bool,
) -> Vec<PortResult> {
    let mut results = Vec::with_capacity(ports.len());

    let chunk_size = timing.max_parallel.max(1);
    for chunk in ports.chunks(chunk_size) {
        // Issue non-blocking connects for the whole batch, collect handles.
        let mut handles: Vec<(u16, u64)> = Vec::with_capacity(chunk.len());
        for &port in chunk {
            match tcp_connect_nonblock(ip, port) {
                Ok(h) => handles.push((port, h)),
                Err(_) => {
                    results.push(PortResult {
                        port,
                        state: PortState::Filtered,
                        banner: None,
                    });
                }
            }
        }

        // Poll all handles until they resolve or timeout.
        let deadline = clock_nanos()
            .saturating_add(timing.connect_timeout_ms.saturating_mul(1_000_000));

        // Track which handles are still pending.
        let mut pending: Vec<(u16, u64, PortState)> = handles
            .iter()
            .map(|&(port, h)| (port, h, PortState::Filtered))
            .collect();
        let mut resolved = vec![false; pending.len()];
        let mut all_done = false;

        while !all_done && clock_nanos() < deadline {
            all_done = true;
            for ((port, handle, state), resolved_flag) in
                pending.iter_mut().zip(resolved.iter_mut())
            {
                if *resolved_flag {
                    continue;
                }
                let status = tcp_poll_status(*handle);
                match status {
                    TCP_STATUS_CONNECTED => {
                        *state = PortState::Open;
                        tcp_close(*handle);
                        *resolved_flag = true;
                    }
                    TCP_STATUS_REFUSED => {
                        *state = PortState::Closed;
                        tcp_close(*handle);
                        *resolved_flag = true;
                    }
                    TCP_STATUS_TIMEOUT => {
                        *state = PortState::Filtered;
                        tcp_close(*handle);
                        *resolved_flag = true;
                    }
                    TCP_STATUS_IN_PROGRESS => {
                        all_done = false;
                        let _ = port; // suppress unused warning in this branch
                    }
                    _ => {
                        *state = PortState::Filtered;
                        tcp_close(*handle);
                        *resolved_flag = true;
                    }
                }
            }
            if !all_done {
                sleep_ms(5);
            }
        }

        // Close any still-pending handles (timeout hit).
        for ((_, handle, state), resolved_flag) in
            pending.iter_mut().zip(resolved.iter())
        {
            if !*resolved_flag {
                *state = PortState::Filtered;
                tcp_close(*handle);
            }
        }

        // Now grab banners for open ports if -sV.
        for (port, _handle, state) in &pending {
            let banner = if *state == PortState::Open && version_detect {
                grab_banner(ip, *port, timing.connect_timeout_ms)
            } else {
                None
            };
            results.push(PortResult {
                port: *port,
                state: *state,
                banner,
            });
        }

        // Inter-batch delay for lower timing levels.
        if timing.inter_batch_delay_ms > 0 {
            sleep_ms(timing.inter_batch_delay_ms);
        }
    }

    // Sort by port number.
    results.sort_unstable_by_key(|r| r.port);
    results
}

// ============================================================================
// Output formatting
// ============================================================================

/// Format the port-table header.
fn fmt_header() -> String {
    format!("{:<10} {:<12} {}\n", "PORT", "STATE", "SERVICE")
}

/// Format a single port result row.
fn fmt_port_row(pr: &PortResult) -> String {
    let svc = service_name(pr.port);
    let port_label = format!("{}/tcp", pr.port);
    let base = format!(
        "{:<10} {:<12} {}",
        port_label,
        pr.state.as_str(),
        svc
    );
    if let Some(ref banner) = pr.banner {
        format!("{base}  [{banner}]\n")
    } else {
        format!("{base}\n")
    }
}

/// Format the full result for one host.
fn fmt_host_result(
    host: &HostResult,
    cfg: &Config,
    scan_type: ScanType,
) -> String {
    let mut s = String::new();

    // Host header — show both display name and raw IP when they differ.
    let ip_str = fmt_ipv4(host.ip);
    let header = if host.display.contains(&ip_str) {
        format!("\nNmap scan report for {}\n", host.display)
    } else {
        format!("\nNmap scan report for {} ({})\n", host.display, ip_str)
    };
    s.push_str(&header);

    if host.is_up {
        if let Some(rtt) = host.ping_rtt_us {
            let rtt_ms = rtt / 1000;
            s.push_str(&format!("Host is up ({rtt_ms}ms latency).\n"));
        } else {
            s.push_str("Host is up.\n");
        }
    } else {
        s.push_str("Host seems down.\n");
    }

    if let Some(ref os) = host.os_guess {
        s.push_str(&format!("OS guess: {os}\n"));
    }

    if scan_type == ScanType::PingScan {
        return s;
    }

    // Port table
    let filtered: Vec<&PortResult> = host
        .ports
        .iter()
        .filter(|pr| {
            if cfg.only_open {
                pr.state == PortState::Open
            } else if cfg.verbose {
                true
            } else {
                pr.state == PortState::Open
            }
        })
        .collect();

    if filtered.is_empty() {
        s.push_str("All scanned ports are closed or filtered.\n");
    } else {
        s.push_str(&fmt_header());
        for pr in filtered {
            s.push_str(&fmt_port_row(pr));
        }
    }

    s
}

/// Build the scan summary line.
fn fmt_summary(
    total_hosts: usize,
    up: usize,
    elapsed_ms: u64,
    total_open: usize,
    total_closed: usize,
    total_filtered: usize,
) -> String {
    let elapsed_s = elapsed_ms / 1000;
    let elapsed_frac = (elapsed_ms % 1000) / 100;
    let host_plural = if total_hosts == 1 { "" } else { "es" };
    let up_plural = if up == 1 { "" } else { "s" };
    format!(
        "\nNmap done: {total_hosts} IP address{host_plural} ({up} host{up_plural} up) \
        scanned in {elapsed_s}.{elapsed_frac}s\n\
        Ports: {total_open} open, {total_closed} closed, {total_filtered} filtered\n"
    )
}

// ============================================================================
// Main scan orchestration
// ============================================================================

fn run_scan(cfg: &Config, out_file: &mut Option<File>) -> i32 {
    let scan_start_nanos = clock_nanos();

    // Expand targets.
    let mut all_target_ips: Vec<(String, Vec<u32>)> = Vec::new();
    for spec in &cfg.targets {
        match expand_target(spec) {
            Ok(pair) => all_target_ips.push(pair),
            Err(e) => {
                eprint_str(&format!("nmap: {e}\n"));
                return 1;
            }
        }
    }

    let total_ips: usize = all_target_ips.iter().map(|(_, v)| v.len()).sum();

    // Print scan header.
    let now_nanos = clock_nanos();
    let _ = now_nanos;
    out(
        &format!(
            "Starting nmap scan of {} host{}\n",
            total_ips,
            if total_ips == 1 { "" } else { "s" }
        ),
        out_file.as_mut(),
    );

    let mut host_results: Vec<HostResult> = Vec::new();

    for (display, ips) in &all_target_ips {
        for &ip in ips {
            let ip_str = fmt_ipv4(ip);
            let host_display = if *display == ip_str {
                ip_str.clone()
            } else {
                format!("{} ({})", display, ip_str)
            };

            // Step 1: host discovery
            let (is_up, ping_rtt, os_guess) = if cfg.no_ping {
                (true, None, None)
            } else {
                let ping_timeout = cfg.timing.connect_timeout_ms.min(2000);
                let (up, rtt) = ping_host(ip, ping_timeout);
                let os = if cfg.os_detect {
                    rtt.and_then(guess_os_from_ttl)
                } else {
                    None
                };
                (up, rtt, os)
            };

            if !is_up && !cfg.no_ping {
                host_results.push(HostResult {
                    ip,
                    display: host_display,
                    is_up: false,
                    ping_rtt_us: None,
                    ports: Vec::new(),
                    os_guess: None,
                });
                continue;
            }

            // Step 2: port scan (skip for ping-only scan)
            let ports = if cfg.scan_type == ScanType::PingScan {
                Vec::new()
            } else {
                scan_ports(ip, &cfg.ports, &cfg.timing, cfg.version_detect)
            };

            host_results.push(HostResult {
                ip,
                display: host_display,
                is_up,
                ping_rtt_us: ping_rtt,
                ports,
                os_guess,
            });
        }
    }

    // Print results.
    for host in &host_results {
        let s = fmt_host_result(host, cfg, cfg.scan_type);
        out(&s, out_file.as_mut());
    }

    // Summary.
    let elapsed_ms = (clock_nanos().saturating_sub(scan_start_nanos)) / 1_000_000;
    let up_count = host_results.iter().filter(|h| h.is_up).count();
    let total_open: usize = host_results
        .iter()
        .flat_map(|h| h.ports.iter())
        .filter(|p| p.state == PortState::Open)
        .count();
    let total_closed: usize = host_results
        .iter()
        .flat_map(|h| h.ports.iter())
        .filter(|p| p.state == PortState::Closed)
        .count();
    let total_filtered: usize = host_results
        .iter()
        .flat_map(|h| h.ports.iter())
        .filter(|p| p.state == PortState::Filtered)
        .count();

    let summary = fmt_summary(
        total_ips,
        up_count,
        elapsed_ms,
        total_open,
        total_closed,
        total_filtered,
    );
    out(&summary, out_file.as_mut());

    0
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        exit(1);
    }

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprint_str(&format!("nmap: {e}\nRun `nmap --help` for usage.\n"));
            exit(1);
        }
    };

    // Open output file if requested.  The File is dropped (and thus closed) at
    // the end of main.
    let mut out_file: Option<File> = if let Some(ref path) = cfg.output_file {
        match File::create(path) {
            Ok(f) => Some(f),
            Err(e) => {
                eprint_str(&format!("nmap: cannot open output file '{path}': {e}\n"));
                exit(1);
            }
        }
    } else {
        None
    };

    let rc = run_scan(&cfg, &mut out_file);

    // Flush any buffered report data before exit.
    if let Some(ref mut f) = out_file {
        let _ = f.flush();
    }

    exit(rc);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    // --- parse_ipv4 ---

    #[test]
    fn test_parse_ipv4_loopback() {
        assert_eq!(parse_ipv4("127.0.0.1"), Some(0x7f00_0001));
    }

    #[test]
    fn test_parse_ipv4_broadcast() {
        assert_eq!(parse_ipv4("255.255.255.255"), Some(0xffff_ffff));
    }

    #[test]
    fn test_parse_ipv4_all_zeros() {
        assert_eq!(parse_ipv4("0.0.0.0"), Some(0));
    }

    #[test]
    fn test_parse_ipv4_private() {
        assert_eq!(parse_ipv4("192.168.1.100"), Some(0xc0a8_0164));
    }

    #[test]
    fn test_parse_ipv4_invalid_too_few_octets() {
        assert!(parse_ipv4("192.168.1").is_none());
    }

    #[test]
    fn test_parse_ipv4_invalid_too_many_octets() {
        // The 4th call to next() on splitn(4, '.') stops at position 3;
        // "1.2" has no '.'-split in the last segment so parse fails on "1.2".
        assert!(parse_ipv4("192.168.1.1.2").is_none());
    }

    #[test]
    fn test_parse_ipv4_invalid_out_of_range() {
        assert!(parse_ipv4("256.0.0.1").is_none());
    }

    #[test]
    fn test_parse_ipv4_invalid_empty() {
        assert!(parse_ipv4("").is_none());
    }

    // --- fmt_ipv4 ---

    #[test]
    fn test_fmt_ipv4_roundtrip() {
        let ip = parse_ipv4("10.20.30.40").unwrap();
        assert_eq!(fmt_ipv4(ip), "10.20.30.40");
    }

    #[test]
    fn test_fmt_ipv4_loopback() {
        assert_eq!(fmt_ipv4(0x7f00_0001), "127.0.0.1");
    }

    // --- parse_ports ---

    #[test]
    fn test_parse_ports_single() {
        assert_eq!(parse_ports("80").unwrap(), vec![80]);
    }

    #[test]
    fn test_parse_ports_range() {
        let ports = parse_ports("1-5").unwrap();
        assert_eq!(ports, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_parse_ports_csv() {
        let ports = parse_ports("22,80,443").unwrap();
        assert_eq!(ports, vec![22, 80, 443]);
    }

    #[test]
    fn test_parse_ports_mixed() {
        let mut ports = parse_ports("22,80-82,443").unwrap();
        ports.sort_unstable();
        assert_eq!(ports, vec![22, 80, 81, 82, 443]);
    }

    #[test]
    fn test_parse_ports_all() {
        let ports = parse_ports("-").unwrap();
        assert_eq!(ports.len(), 65535);
        assert_eq!(ports[0], 1);
        assert_eq!(ports[65534], 65535);
    }

    #[test]
    fn test_parse_ports_deduplication() {
        let ports = parse_ports("80,80,80").unwrap();
        assert_eq!(ports, vec![80]);
    }

    #[test]
    fn test_parse_ports_sorted() {
        let ports = parse_ports("443,22,80").unwrap();
        assert_eq!(ports, vec![22, 80, 443]);
    }

    #[test]
    fn test_parse_ports_backwards_range_error() {
        assert!(parse_ports("100-10").is_err());
    }

    #[test]
    fn test_parse_ports_invalid_number() {
        assert!(parse_ports("abc").is_err());
    }

    #[test]
    fn test_parse_ports_single_port_22() {
        assert_eq!(parse_ports("22").unwrap(), vec![22]);
    }

    // --- expand_target ---

    #[test]
    fn test_expand_target_single_ip() {
        let (display, ips) = expand_target("10.0.0.1").unwrap();
        assert_eq!(display, "10.0.0.1");
        assert_eq!(ips, vec![parse_ipv4("10.0.0.1").unwrap()]);
    }

    #[test]
    fn test_expand_target_cidr_slash32() {
        let (_display, ips) = expand_target("192.168.1.5/32").unwrap();
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0], parse_ipv4("192.168.1.5").unwrap());
    }

    #[test]
    fn test_expand_target_cidr_slash30() {
        let (_display, ips) = expand_target("10.0.0.0/30").unwrap();
        assert_eq!(ips.len(), 4);
        assert_eq!(ips[0], parse_ipv4("10.0.0.0").unwrap());
        assert_eq!(ips[3], parse_ipv4("10.0.0.3").unwrap());
    }

    #[test]
    fn test_expand_target_cidr_slash31() {
        let (_display, ips) = expand_target("10.0.0.0/31").unwrap();
        assert_eq!(ips.len(), 2);
    }

    #[test]
    fn test_expand_target_cidr_slash24_count() {
        let (_display, ips) = expand_target("192.168.1.0/24").unwrap();
        assert_eq!(ips.len(), 256);
    }

    #[test]
    fn test_expand_target_cidr_bad_prefix() {
        assert!(expand_target("10.0.0.0/33").is_err());
    }

    #[test]
    fn test_expand_target_cidr_bad_ip() {
        assert!(expand_target("999.0.0.0/24").is_err());
    }

    // --- service_name ---

    #[test]
    fn test_service_name_ssh() {
        assert_eq!(service_name(22), "ssh");
    }

    #[test]
    fn test_service_name_http() {
        assert_eq!(service_name(80), "http");
    }

    #[test]
    fn test_service_name_https() {
        assert_eq!(service_name(443), "https");
    }

    #[test]
    fn test_service_name_unknown() {
        assert_eq!(service_name(12345), "");
    }

    #[test]
    fn test_service_name_ftp() {
        assert_eq!(service_name(21), "ftp");
    }

    #[test]
    fn test_service_name_smtp() {
        assert_eq!(service_name(25), "smtp");
    }

    #[test]
    fn test_service_name_mysql() {
        assert_eq!(service_name(3306), "mysql");
    }

    // --- Timing ---

    #[test]
    fn test_timing_level0_is_paranoid() {
        let t = Timing::from_level(0);
        assert_eq!(t.max_parallel, 1);
        assert!(t.connect_timeout_ms >= 5000);
    }

    #[test]
    fn test_timing_level3_is_default() {
        let t = Timing::from_level(3);
        assert_eq!(t.inter_batch_delay_ms, 0);
        assert!(t.max_parallel >= 64);
    }

    #[test]
    fn test_timing_level5_is_fastest() {
        let t = Timing::from_level(5);
        assert!(t.connect_timeout_ms <= 250);
        assert!(t.max_parallel >= 256);
    }

    // --- guess_os_from_ttl ---

    #[test]
    fn test_guess_os_ttl_linux() {
        // TTL=64 packed into upper 16 bits: 64u64 << 48 | 1000
        let rtt = (64u64 << 48 | 1000) as i64;
        let guess = guess_os_from_ttl(rtt);
        assert!(guess.is_some());
        let g = guess.unwrap();
        assert!(g.contains("Linux") || g.contains("macOS"));
    }

    #[test]
    fn test_guess_os_ttl_windows() {
        let rtt = (128u64 << 48 | 2000) as i64;
        let guess = guess_os_from_ttl(rtt);
        assert!(guess.is_some());
        assert!(guess.unwrap().contains("Windows"));
    }

    #[test]
    fn test_guess_os_ttl_zero_returns_none() {
        // TTL = 0 → no TTL info provided
        let rtt: i64 = 5000;
        assert!(guess_os_from_ttl(rtt).is_none());
    }

    // --- parse_args ---

    #[test]
    fn test_parse_args_default_scan_type() {
        let args: Vec<String> = vec!["nmap".into(), "10.0.0.1".into()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.scan_type, ScanType::TcpConnect);
    }

    #[test]
    fn test_parse_args_ping_scan() {
        let args: Vec<String> = vec!["nmap".into(), "-sP".into(), "10.0.0.0/24".into()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.scan_type, ScanType::PingScan);
    }

    #[test]
    fn test_parse_args_sn_alias() {
        let args: Vec<String> = vec!["nmap".into(), "-sn".into(), "10.0.0.1".into()];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.scan_type, ScanType::PingScan);
    }

    #[test]
    fn test_parse_args_port_spec() {
        let args: Vec<String> = vec!["nmap".into(), "-p".into(), "22,80".into(), "10.0.0.1".into()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.ports.contains(&22));
        assert!(cfg.ports.contains(&80));
    }

    #[test]
    fn test_parse_args_verbose_flag() {
        let args: Vec<String> = vec!["nmap".into(), "-v".into(), "10.0.0.1".into()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.verbose);
    }

    #[test]
    fn test_parse_args_no_target_error() {
        let args: Vec<String> = vec!["nmap".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_unknown_flag_error() {
        let args: Vec<String> = vec!["nmap".into(), "--zzz".into(), "10.0.0.1".into()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_args_output_file() {
        let args: Vec<String> = vec![
            "nmap".into(),
            "-oN".into(),
            "/tmp/out.txt".into(),
            "10.0.0.1".into(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.output_file.as_deref(), Some("/tmp/out.txt"));
    }

    #[test]
    fn test_parse_args_timing_t4() {
        let args: Vec<String> = vec!["nmap".into(), "-T4".into(), "10.0.0.1".into()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.timing.connect_timeout_ms <= 500);
    }

    #[test]
    fn test_parse_args_only_open() {
        let args: Vec<String> = vec!["nmap".into(), "--open".into(), "10.0.0.1".into()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.only_open);
    }

    // --- fmt_host_result / port table ---

    #[test]
    fn test_fmt_port_row_open_known() {
        let pr = PortResult {
            port: 22,
            state: PortState::Open,
            banner: None,
        };
        let row = fmt_port_row(&pr);
        assert!(row.contains("22/tcp"));
        assert!(row.contains("open"));
        assert!(row.contains("ssh"));
    }

    #[test]
    fn test_fmt_port_row_closed_unknown() {
        let pr = PortResult {
            port: 12345,
            state: PortState::Closed,
            banner: None,
        };
        let row = fmt_port_row(&pr);
        assert!(row.contains("12345/tcp"));
        assert!(row.contains("closed"));
    }

    #[test]
    fn test_fmt_port_row_with_banner() {
        let pr = PortResult {
            port: 21,
            state: PortState::Open,
            banner: Some("220 ProFTPD".to_string()),
        };
        let row = fmt_port_row(&pr);
        assert!(row.contains("220 ProFTPD"));
    }

    #[test]
    fn test_fmt_summary_single_host() {
        let s = fmt_summary(1, 1, 1500, 3, 10, 2);
        assert!(s.contains("1 IP address"));
        assert!(s.contains("1 host"));
        assert!(s.contains("3 open"));
    }

    #[test]
    fn test_fmt_summary_plural_hosts() {
        let s = fmt_summary(5, 3, 2000, 0, 0, 0);
        assert!(s.contains("5 IP addresses"));
        assert!(s.contains("3 hosts"));
    }

    // --- default_ports ---

    #[test]
    fn test_default_ports_contains_common() {
        let ports = default_ports();
        assert!(ports.contains(&22));
        assert!(ports.contains(&80));
        assert!(ports.contains(&443));
        assert!(ports.contains(&3306));
    }

    #[test]
    fn test_default_ports_nonempty() {
        assert!(!default_ports().is_empty());
    }
}
