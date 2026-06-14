// Slate OS ss — socket statistics
//
// Multi-personality binary:
//   ss       — display socket statistics (like Linux ss, replaces netstat)
//   sockstat — BSD-style socket status display
//
// Usage:
//   ss [OPTIONS] [FILTER]
//   sockstat [OPTIONS]

#![cfg_attr(not(test), no_main)]
// SocketState::long_str and SocketEntry::timer document the netlink
// NETLINK_SOCK_DIAG / INET_DIAG message vocabulary the real ss
// implementation must speak. Dead-code lint cannot see across
// that future boundary.
#![allow(dead_code)]

#[cfg(not(test))]
use std::env;
use std::io::{self, Write};

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Ss,
    Sockstat,
}

fn detect_personality(argv0: &str) -> Personality {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let lower = base.to_ascii_lowercase();
    let lower = lower.strip_suffix(".exe").unwrap_or(&lower);
    match lower {
        "sockstat" => Personality::Sockstat,
        _ => Personality::Ss,
    }
}

// ---------------------------------------------------------------------------
// Socket state and type enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketState {
    Established,
    SynSent,
    SynRecv,
    FinWait1,
    FinWait2,
    TimeWait,
    Close,
    CloseWait,
    LastAck,
    Listen,
    Closing,
    Unknown,
}

impl SocketState {
    fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "ESTABLISHED" | "ESTAB" | "01" => Self::Established,
            "SYN_SENT" | "SYN-SENT" | "02" => Self::SynSent,
            "SYN_RECV" | "SYN-RECV" | "03" => Self::SynRecv,
            "FIN_WAIT1" | "FIN-WAIT-1" | "04" => Self::FinWait1,
            "FIN_WAIT2" | "FIN-WAIT-2" | "05" => Self::FinWait2,
            "TIME_WAIT" | "TIME-WAIT" | "06" => Self::TimeWait,
            "CLOSE" | "07" => Self::Close,
            "CLOSE_WAIT" | "CLOSE-WAIT" | "08" => Self::CloseWait,
            "LAST_ACK" | "LAST-ACK" | "09" => Self::LastAck,
            "LISTEN" | "LISTENING" | "0A" => Self::Listen,
            "CLOSING" | "0B" => Self::Closing,
            _ => Self::Unknown,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Established => "ESTAB",
            Self::SynSent => "SYN-SENT",
            Self::SynRecv => "SYN-RECV",
            Self::FinWait1 => "FIN-WAIT-1",
            Self::FinWait2 => "FIN-WAIT-2",
            Self::TimeWait => "TIME-WAIT",
            Self::Close => "UNCONN",
            Self::CloseWait => "CLOSE-WAIT",
            Self::LastAck => "LAST-ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSING",
            Self::Unknown => "UNKNOWN",
        }
    }

    fn long_str(&self) -> &'static str {
        match self {
            Self::Established => "ESTABLISHED",
            Self::SynSent => "SYN_SENT",
            Self::SynRecv => "SYN_RECV",
            Self::FinWait1 => "FIN_WAIT1",
            Self::FinWait2 => "FIN_WAIT2",
            Self::TimeWait => "TIME_WAIT",
            Self::Close => "CLOSE",
            Self::CloseWait => "CLOSE_WAIT",
            Self::LastAck => "LAST_ACK",
            Self::Listen => "LISTEN",
            Self::Closing => "CLOSING",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocketProto {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
    Unix,
    Raw,
    Raw6,
}

impl SocketProto {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Tcp6 => "tcp6",
            Self::Udp => "udp",
            Self::Udp6 => "udp6",
            Self::Unix => "u_str",
            Self::Raw => "raw",
            Self::Raw6 => "raw6",
        }
    }

    fn netid(&self) -> &'static str {
        match self {
            Self::Tcp | Self::Tcp6 => "tcp",
            Self::Udp | Self::Udp6 => "udp",
            Self::Unix => "u_str",
            Self::Raw | Self::Raw6 => "raw",
        }
    }
}

// ---------------------------------------------------------------------------
// Socket entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct SocketEntry {
    proto: SocketProto,
    state: SocketState,
    recv_q: u64,
    send_q: u64,
    local_addr: String,
    local_port: u16,
    remote_addr: String,
    remote_port: u16,
    pid: Option<u32>,
    process_name: Option<String>,
    inode: u64,
    uid: u32,
    // Unix-specific
    unix_path: Option<String>,
    // Timer info
    timer: Option<String>,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Config {
    personality: Personality,
    show_tcp: bool,
    show_udp: bool,
    show_unix: bool,
    show_raw: bool,
    show_listening: bool,
    show_all: bool,
    show_processes: bool,
    show_numeric: bool,
    show_extended: bool,
    show_memory: bool,
    show_timer: bool,
    show_info: bool,
    no_header: bool,
    resolve_names: bool,
    ipv4_only: bool,
    ipv6_only: bool,
    state_filter: Option<SocketState>,
    show_help: bool,
    show_version: bool,
    show_summary: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            personality: Personality::Ss,
            show_tcp: false,
            show_udp: false,
            show_unix: false,
            show_raw: false,
            show_listening: false,
            show_all: false,
            show_processes: false,
            show_numeric: true,
            show_extended: false,
            show_memory: false,
            show_timer: false,
            show_info: false,
            no_header: false,
            resolve_names: false,
            ipv4_only: false,
            ipv6_only: false,
            state_filter: None,
            show_help: false,
            show_version: false,
            show_summary: false,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let personality = args
        .first()
        .map(|a| detect_personality(a))
        .unwrap_or(Personality::Ss);

    let mut cfg = Config {
        personality,
        ..Default::default()
    };

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => cfg.show_help = true,
            "-V" | "--version" => cfg.show_version = true,
            "-t" | "--tcp" => cfg.show_tcp = true,
            "-u" | "--udp" => cfg.show_udp = true,
            "-x" | "--unix" => cfg.show_unix = true,
            "-w" | "--raw" => cfg.show_raw = true,
            "-l" | "--listening" => cfg.show_listening = true,
            "-a" | "--all" => cfg.show_all = true,
            "-p" | "--processes" => cfg.show_processes = true,
            "-n" | "--numeric" => cfg.show_numeric = true,
            "-r" | "--resolve" => {
                cfg.resolve_names = true;
                cfg.show_numeric = false;
            }
            "-e" | "--extended" => cfg.show_extended = true,
            "-m" | "--memory" => cfg.show_memory = true,
            "-o" | "--options" => cfg.show_timer = true,
            "-i" | "--info" => cfg.show_info = true,
            "-H" | "--no-header" => cfg.no_header = true,
            "-4" | "--ipv4" => cfg.ipv4_only = true,
            "-6" | "--ipv6" => cfg.ipv6_only = true,
            "-s" | "--summary" => cfg.show_summary = true,
            "state" => {
                i += 1;
                if let Some(state_str) = args.get(i) {
                    cfg.state_filter = Some(SocketState::from_str(state_str));
                }
            }
            // Combined flags like -tlnp
            other if other.starts_with('-') && !other.starts_with("--") && other.len() > 2 => {
                for ch in other[1..].chars() {
                    match ch {
                        't' => cfg.show_tcp = true,
                        'u' => cfg.show_udp = true,
                        'x' => cfg.show_unix = true,
                        'w' => cfg.show_raw = true,
                        'l' => cfg.show_listening = true,
                        'a' => cfg.show_all = true,
                        'p' => cfg.show_processes = true,
                        'n' => cfg.show_numeric = true,
                        'r' => {
                            cfg.resolve_names = true;
                            cfg.show_numeric = false;
                        }
                        'e' => cfg.show_extended = true,
                        'm' => cfg.show_memory = true,
                        'o' => cfg.show_timer = true,
                        'i' => cfg.show_info = true,
                        'H' => cfg.no_header = true,
                        '4' => cfg.ipv4_only = true,
                        '6' => cfg.ipv6_only = true,
                        's' => cfg.show_summary = true,
                        _ => return Err(format!("ss: unknown flag: -{ch}")),
                    }
                }
            }
            other if other.starts_with('-') => {
                return Err(format!("ss: unknown option: {other}"));
            }
            _ => {} // positional filter expressions (ignored for now)
        }
        i += 1;
    }

    // Default: if no protocol selected, show all
    if !cfg.show_tcp && !cfg.show_udp && !cfg.show_unix && !cfg.show_raw {
        cfg.show_tcp = true;
        cfg.show_udp = true;
        cfg.show_unix = true;
        cfg.show_raw = true;
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// /proc parsing
// ---------------------------------------------------------------------------

fn parse_hex_ip(hex: &str) -> String {
    if hex.len() == 8 {
        // IPv4: stored in network byte order as hex
        if let Ok(val) = u32::from_str_radix(hex, 16) {
            let a = val & 0xff;
            let b = (val >> 8) & 0xff;
            let c = (val >> 16) & 0xff;
            let d = (val >> 24) & 0xff;
            return format!("{a}.{b}.{c}.{d}");
        }
    }
    if hex.len() == 32 {
        // IPv6
        return format!("::{}",
            &hex.chars()
                .collect::<Vec<_>>()
                .chunks(4)
                .map(|chunk| chunk.iter().collect::<String>())
                .collect::<Vec<_>>()
                .join(":")
        );
    }
    hex.to_string()
}

fn parse_hex_port(hex: &str) -> u16 {
    u16::from_str_radix(hex, 16).unwrap_or(0)
}

fn read_proc_net(path: &str, proto: SocketProto) -> Vec<SocketEntry> {
    let mut entries = Vec::new();

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return entries,
    };

    for (idx, line) in content.lines().enumerate() {
        if idx == 0 {
            continue; // skip header
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }

        // Parse local address:port
        let local_parts: Vec<&str> = fields[1].split(':').collect();
        let remote_parts: Vec<&str> = fields[2].split(':').collect();

        if local_parts.len() < 2 || remote_parts.len() < 2 {
            continue;
        }

        let local_addr = parse_hex_ip(local_parts[0]);
        let local_port = parse_hex_port(local_parts[1]);
        let remote_addr = parse_hex_ip(remote_parts[0]);
        let remote_port = parse_hex_port(remote_parts[1]);

        let state = SocketState::from_str(fields[3]);
        let uid = fields[7].parse().unwrap_or(0);
        let inode = fields[9].parse().unwrap_or(0);

        // Parse queues
        let queues: Vec<&str> = fields[4].split(':').collect();
        let send_q = queues.first().and_then(|s| u64::from_str_radix(s, 16).ok()).unwrap_or(0);
        let recv_q = queues.get(1).and_then(|s| u64::from_str_radix(s, 16).ok()).unwrap_or(0);

        entries.push(SocketEntry {
            proto,
            state,
            recv_q,
            send_q,
            local_addr,
            local_port,
            remote_addr,
            remote_port,
            pid: None,
            process_name: None,
            inode,
            uid,
            unix_path: None,
            timer: None,
        });
    }

    entries
}

fn read_unix_sockets() -> Vec<SocketEntry> {
    let mut entries = Vec::new();

    let content = match std::fs::read_to_string("/proc/net/unix") {
        Ok(c) => c,
        Err(_) => return entries,
    };

    for (idx, line) in content.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 7 {
            continue;
        }

        let inode = fields[6].parse().unwrap_or(0);
        let path = if fields.len() > 7 {
            Some(fields[7].to_string())
        } else {
            None
        };

        let state_num = fields[5].parse::<u32>().unwrap_or(0);
        let state = match state_num {
            1 => SocketState::Established,
            2 => SocketState::SynSent, // connecting
            3 => SocketState::Close,   // disconnecting
            _ => SocketState::Unknown,
        };

        entries.push(SocketEntry {
            proto: SocketProto::Unix,
            state,
            recv_q: 0,
            send_q: 0,
            local_addr: String::new(),
            local_port: 0,
            remote_addr: String::new(),
            remote_port: 0,
            pid: None,
            process_name: None,
            inode,
            uid: 0,
            unix_path: path,
            timer: None,
        });
    }

    entries
}

// ---------------------------------------------------------------------------
// Kernel read-path fallback (SYS_TCP_LIST / SYS_TCP_LISTENER_LIST)
//
// The kernel does not populate /proc/net/{tcp,tcp6,...}, so the procfs parser
// above always comes back empty on Slate OS. These read-only diagnostic
// syscalls are the live data source for the TCP views (TD18 read-path wiring);
// we fall back to them whenever the procfs read yields nothing. There is no
// kernel-side enumeration for UDP/raw/unix sockets yet, so those views stay
// empty until the matching syscalls land.
// ---------------------------------------------------------------------------

/// List active TCP connections (20-byte records). Returns count.
const SYS_TCP_LIST: u64 = 840;
/// List active TCP listeners (4-byte records). Returns count.
const SYS_TCP_LISTENER_LIST: u64 = 841;

const TCP_LIST_RECORD_SIZE: usize = 20;
const TCP_LISTENER_RECORD_SIZE: usize = 4;

/// Upper bound on records requested in one listing call.
const MAX_TCP_RECORDS: usize = 1024;

// Real syscall path: only when building the Slate OS binary (non-test). The
// `gather_sockets` test paths reach the query functions, and executing a raw
// `syscall` instruction on the host build machine is undefined — so under
// `cargo test` we compile the ENOSYS stub instead and the fallback returns
// empty (the pure record decoders are unit-tested directly).
#[cfg(all(target_arch = "x86_64", not(test)))]
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

// Stub for non-x86_64 hosts and for host test builds (see note above).
#[cfg(any(not(target_arch = "x86_64"), test))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

/// Format four network-order octets as a dotted-quad string.
fn fmt_octets(b: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
}

/// Map the kernel's `net::tcp::TcpState` discriminant (byte `[12]` of a
/// `SYS_TCP_LIST` record) to ss's [`SocketState`]. The kernel enum order is
/// `Closed, Listen, SynSent, SynReceived, Established, FinWait1, FinWait2,
/// TimeWait, CloseWait, LastAck` (see `kernel/src/net/tcp.rs`).
fn kernel_tcp_state(raw: u8) -> SocketState {
    match raw {
        0 => SocketState::Close,
        1 => SocketState::Listen,
        2 => SocketState::SynSent,
        3 => SocketState::SynRecv,
        4 => SocketState::Established,
        5 => SocketState::FinWait1,
        6 => SocketState::FinWait2,
        7 => SocketState::TimeWait,
        8 => SocketState::CloseWait,
        9 => SocketState::LastAck,
        _ => SocketState::Unknown,
    }
}

/// Decode a flat buffer of 20-byte `SYS_TCP_LIST` records into socket entries.
/// A trailing partial record (if any) is ignored by `chunks_exact`.
fn parse_tcp_list_records(buf: &[u8]) -> Vec<SocketEntry> {
    buf.chunks_exact(TCP_LIST_RECORD_SIZE)
        .map(|rec| {
            // rec.len() == 20 guaranteed by chunks_exact.
            let local_addr = fmt_octets([rec[0], rec[1], rec[2], rec[3]]);
            let local_port = u16::from_be_bytes([rec[4], rec[5]]);
            let remote_addr = fmt_octets([rec[6], rec[7], rec[8], rec[9]]);
            let remote_port = u16::from_be_bytes([rec[10], rec[11]]);
            let state = kernel_tcp_state(rec[12]);
            // rx/tx buffered are u24 LE in [13..16] / [16..19].
            let recv_q =
                u64::from(rec[13]) | (u64::from(rec[14]) << 8) | (u64::from(rec[15]) << 16);
            let send_q =
                u64::from(rec[16]) | (u64::from(rec[17]) << 8) | (u64::from(rec[18]) << 16);
            SocketEntry {
                proto: SocketProto::Tcp,
                state,
                recv_q,
                send_q,
                local_addr,
                local_port,
                remote_addr,
                remote_port,
                pid: None,
                process_name: None,
                inode: 0,
                uid: 0,
                unix_path: None,
                timer: None,
            }
        })
        .collect()
}

/// Decode a flat buffer of 4-byte `SYS_TCP_LISTENER_LIST` records into socket
/// entries in the `LISTEN` state.
fn parse_tcp_listener_records(buf: &[u8]) -> Vec<SocketEntry> {
    buf.chunks_exact(TCP_LISTENER_RECORD_SIZE)
        .map(|rec| {
            // rec.len() == 4 guaranteed by chunks_exact.
            let local_port = u16::from_be_bytes([rec[0], rec[1]]);
            // rec[2]/rec[3] = backlog used/max — not shown in the socket list.
            SocketEntry {
                proto: SocketProto::Tcp,
                state: SocketState::Listen,
                recv_q: 0,
                send_q: 0,
                local_addr: "0.0.0.0".to_string(),
                local_port,
                remote_addr: "0.0.0.0".to_string(),
                remote_port: 0,
                pid: None,
                process_name: None,
                inode: 0,
                uid: 0,
                unix_path: None,
                timer: None,
            }
        })
        .collect()
}

/// Query active TCP connections via `SYS_TCP_LIST`. Empty on failure.
fn query_tcp_connections() -> Vec<SocketEntry> {
    let mut buf = vec![0u8; MAX_TCP_RECORDS * TCP_LIST_RECORD_SIZE];
    // SAFETY: buf is a valid writable slice; the kernel writes at most buf.len()
    // bytes and returns the number of 20-byte records written.
    let ret =
        unsafe { syscall3(SYS_TCP_LIST, buf.as_mut_ptr() as u64, buf.len() as u64, 0) };
    if ret < 0 {
        return Vec::new();
    }
    let count = usize::try_from(ret).unwrap_or(0);
    let byte_len = count.saturating_mul(TCP_LIST_RECORD_SIZE).min(buf.len());
    parse_tcp_list_records(buf.get(..byte_len).unwrap_or(&[]))
}

/// Query active TCP listeners via `SYS_TCP_LISTENER_LIST`. Empty on failure.
fn query_tcp_listeners() -> Vec<SocketEntry> {
    let mut buf = vec![0u8; MAX_TCP_RECORDS * TCP_LISTENER_RECORD_SIZE];
    // SAFETY: as above; records are 4 bytes and the return is the count.
    let ret = unsafe {
        syscall3(SYS_TCP_LISTENER_LIST, buf.as_mut_ptr() as u64, buf.len() as u64, 0)
    };
    if ret < 0 {
        return Vec::new();
    }
    let count = usize::try_from(ret).unwrap_or(0);
    let byte_len = count.saturating_mul(TCP_LISTENER_RECORD_SIZE).min(buf.len());
    parse_tcp_listener_records(buf.get(..byte_len).unwrap_or(&[]))
}

fn gather_sockets(cfg: &Config) -> Vec<SocketEntry> {
    let mut all = Vec::new();

    let mut tcp = Vec::new();
    if cfg.show_tcp && !cfg.ipv6_only {
        tcp.extend(read_proc_net("/proc/net/tcp", SocketProto::Tcp));
    }
    if cfg.show_tcp && !cfg.ipv4_only {
        tcp.extend(read_proc_net("/proc/net/tcp6", SocketProto::Tcp6));
    }
    // The kernel does not populate /proc/net/tcp; fall back to SYS_TCP_LIST +
    // SYS_TCP_LISTENER_LIST when the procfs read came back empty (TD18). These
    // syscalls expose IPv4 connections only, so skip the fallback under -6.
    if cfg.show_tcp && tcp.is_empty() && !cfg.ipv6_only {
        tcp.extend(query_tcp_connections());
        tcp.extend(query_tcp_listeners());
    }
    all.extend(tcp);

    if cfg.show_udp && !cfg.ipv6_only {
        all.extend(read_proc_net("/proc/net/udp", SocketProto::Udp));
    }
    if cfg.show_udp && !cfg.ipv4_only {
        all.extend(read_proc_net("/proc/net/udp6", SocketProto::Udp6));
    }
    if cfg.show_raw && !cfg.ipv6_only {
        all.extend(read_proc_net("/proc/net/raw", SocketProto::Raw));
    }
    if cfg.show_raw && !cfg.ipv4_only {
        all.extend(read_proc_net("/proc/net/raw6", SocketProto::Raw6));
    }
    if cfg.show_unix {
        all.extend(read_unix_sockets());
    }

    // Filter by state
    if cfg.show_listening && !cfg.show_all {
        all.retain(|e| e.state == SocketState::Listen);
    } else if !cfg.show_all && !cfg.show_listening {
        // Default: non-listening only
        all.retain(|e| e.state != SocketState::Listen || e.state == SocketState::Close);
    }

    // Filter by specific state
    if let Some(ref state) = cfg.state_filter {
        all.retain(|e| &e.state == state);
    }

    all
}

// ---------------------------------------------------------------------------
// Service name resolution
// ---------------------------------------------------------------------------

fn resolve_port(port: u16, proto: &str) -> String {
    // Common port -> service name mappings
    match (port, proto) {
        (22, _) => "ssh".to_string(),
        (25, _) => "smtp".to_string(),
        (53, _) => "domain".to_string(),
        (80, _) => "http".to_string(),
        (110, _) => "pop3".to_string(),
        (143, _) => "imap".to_string(),
        (443, _) => "https".to_string(),
        (993, _) => "imaps".to_string(),
        (995, _) => "pop3s".to_string(),
        (3306, _) => "mysql".to_string(),
        (5432, _) => "postgresql".to_string(),
        (6379, _) => "redis".to_string(),
        (8080, _) => "http-alt".to_string(),
        _ => port.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Summary mode
// ---------------------------------------------------------------------------

fn print_summary(writer: &mut dyn Write) -> io::Result<()> {
    writeln!(writer, "Total: 0")?;
    writeln!(writer, "TCP:   0 (estab 0, closed 0, orphaned 0, timewait 0)")?;
    writeln!(writer)?;
    writeln!(writer, "Transport Total   IP   IPv6")?;
    writeln!(writer, "RAW       0       0    0")?;
    writeln!(writer, "UDP       0       0    0")?;
    writeln!(writer, "TCP       0       0    0")?;
    writeln!(writer, "INET      0       0    0")?;
    writeln!(writer, "FRAG      0       0    0")?;

    // Try to read actual stats from /proc
    if let Ok(content) = std::fs::read_to_string("/proc/net/sockstat") {
        writeln!(writer)?;
        writeln!(writer, "--- /proc/net/sockstat ---")?;
        write!(writer, "{content}")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn format_addr_port(addr: &str, port: u16, numeric: bool, proto_str: &str) -> String {
    let port_str = if numeric {
        port.to_string()
    } else {
        resolve_port(port, proto_str)
    };

    if addr.is_empty() || addr == "0.0.0.0" {
        format!("*:{port_str}")
    } else if addr.contains("::") || addr.starts_with("::") {
        format!("[{addr}]:{port_str}")
    } else {
        format!("{addr}:{port_str}")
    }
}

fn run_ss(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    if cfg.show_summary {
        return print_summary(writer);
    }

    let entries = gather_sockets(cfg);

    if !cfg.no_header {
        if cfg.show_extended {
            writeln!(
                writer,
                "Netid  State      Recv-Q Send-Q  Local Address:Port    Peer Address:Port  Process  UID   Ino"
            )?;
        } else if cfg.show_processes {
            writeln!(
                writer,
                "Netid  State      Recv-Q Send-Q  Local Address:Port    Peer Address:Port  Process"
            )?;
        } else {
            writeln!(
                writer,
                "Netid  State      Recv-Q Send-Q  Local Address:Port    Peer Address:Port"
            )?;
        }
    }

    for entry in &entries {
        let netid = entry.proto.as_str();
        let state = entry.state.as_str();

        let local = if entry.proto == SocketProto::Unix {
            entry
                .unix_path
                .as_deref()
                .unwrap_or("*")
                .to_string()
        } else {
            format_addr_port(
                &entry.local_addr,
                entry.local_port,
                cfg.show_numeric,
                entry.proto.netid(),
            )
        };

        let remote = if entry.proto == SocketProto::Unix {
            "*".to_string()
        } else {
            format_addr_port(
                &entry.remote_addr,
                entry.remote_port,
                cfg.show_numeric,
                entry.proto.netid(),
            )
        };

        if cfg.show_extended {
            let proc_str = entry
                .process_name
                .as_deref()
                .unwrap_or("-");
            writeln!(
                writer,
                "{:<6} {:<10} {:>6} {:>6}  {:<22} {:<18} {:<8} {:<5} {}",
                netid,
                state,
                entry.recv_q,
                entry.send_q,
                local,
                remote,
                proc_str,
                entry.uid,
                entry.inode,
            )?;
        } else if cfg.show_processes {
            let proc_str = if let (Some(pid), Some(name)) =
                (&entry.pid, &entry.process_name)
            {
                format!("users:((\"{name}\",pid={pid}))")
            } else {
                "-".to_string()
            };
            writeln!(
                writer,
                "{:<6} {:<10} {:>6} {:>6}  {:<22} {:<18} {}",
                netid, state, entry.recv_q, entry.send_q, local, remote, proc_str,
            )?;
        } else {
            writeln!(
                writer,
                "{:<6} {:<10} {:>6} {:>6}  {:<22} {}",
                netid, state, entry.recv_q, entry.send_q, local, remote,
            )?;
        }
    }

    Ok(())
}

fn run_sockstat(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    // BSD sockstat format: USER COMMAND PID FD PROTO LOCAL_ADDR FOREIGN_ADDR
    let entries = gather_sockets(cfg);

    if !cfg.no_header {
        writeln!(
            writer,
            "USER     COMMAND    PID   FD PROTO  LOCAL ADDRESS         FOREIGN ADDRESS"
        )?;
    }

    for entry in &entries {
        if entry.proto == SocketProto::Unix {
            continue; // sockstat typically doesn't show unix sockets
        }

        let user = "?";
        let cmd = entry.process_name.as_deref().unwrap_or("?");
        let pid = entry.pid.unwrap_or(0);

        let local = format_addr_port(
            &entry.local_addr,
            entry.local_port,
            true,
            entry.proto.netid(),
        );
        let remote = format_addr_port(
            &entry.remote_addr,
            entry.remote_port,
            true,
            entry.proto.netid(),
        );

        writeln!(
            writer,
            "{:<8} {:<10} {:<5} {:>2} {:<6} {:<21} {}",
            user,
            cmd,
            pid,
            "?",
            entry.proto.as_str(),
            local,
            remote,
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_help(personality: Personality) {
    match personality {
        Personality::Ss => {
            println!("Usage: ss [OPTIONS] [FILTER]");
            println!();
            println!("Display socket statistics.");
            println!();
            println!("Options:");
            println!("  -t, --tcp         Show TCP sockets");
            println!("  -u, --udp         Show UDP sockets");
            println!("  -x, --unix        Show Unix domain sockets");
            println!("  -w, --raw         Show RAW sockets");
            println!("  -l, --listening   Show listening sockets");
            println!("  -a, --all         Show all sockets (listening and non-listening)");
            println!("  -p, --processes   Show process using socket");
            println!("  -n, --numeric     Don't resolve service names");
            println!("  -r, --resolve     Resolve hostnames");
            println!("  -e, --extended    Show extended info (UID, inode)");
            println!("  -m, --memory      Show socket memory usage");
            println!("  -o, --options     Show timer information");
            println!("  -i, --info        Show TCP internal info");
            println!("  -H, --no-header   Suppress header line");
            println!("  -4, --ipv4        Show IPv4 sockets only");
            println!("  -6, --ipv6        Show IPv6 sockets only");
            println!("  -s, --summary     Show summary statistics");
            println!("  -h, --help        Show this help");
            println!("  -V, --version     Show version");
            println!();
            println!("Filter:");
            println!("  state <STATE>     Filter by socket state");
            println!("  States: established, syn-sent, syn-recv, fin-wait-1, etc.");
        }
        Personality::Sockstat => {
            println!("Usage: sockstat [OPTIONS]");
            println!();
            println!("List open Internet or UNIX domain sockets.");
            println!();
            println!("Options:");
            println!("  -4               Show IPv4 sockets");
            println!("  -6               Show IPv6 sockets");
            println!("  -l               Show listening sockets");
            println!("  -t               Show TCP sockets");
            println!("  -u               Show UDP sockets");
            println!("  -h, --help       Show this help");
            println!("  -V, --version    Show version");
        }
    }
}

fn print_version(personality: Personality) {
    let name = match personality {
        Personality::Ss => "ss",
        Personality::Sockstat => "sockstat",
    };
    println!("{name} (Slate OS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help(cfg.personality);
        return 0;
    }

    if cfg.show_version {
        print_version(cfg.personality);
        return 0;
    }

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let result = match cfg.personality {
        Personality::Ss => run_ss(&cfg, &mut writer),
        Personality::Sockstat => run_sockstat(&cfg, &mut writer),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("ss: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_personality_ss() {
        assert_eq!(detect_personality("ss"), Personality::Ss);
        assert_eq!(detect_personality("/usr/bin/ss"), Personality::Ss);
    }

    #[test]
    fn test_detect_personality_sockstat() {
        assert_eq!(detect_personality("sockstat"), Personality::Sockstat);
    }

    #[test]
    fn test_parse_args_basic() {
        let args = vec!["ss".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_tcp);
        assert!(cfg.show_udp);
    }

    #[test]
    fn test_parse_args_tcp_only() {
        let args = vec!["ss".to_string(), "-t".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_tcp);
        assert!(!cfg.show_udp);
    }

    #[test]
    fn test_parse_args_combined() {
        let args = vec!["ss".to_string(), "-tlnp".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_tcp);
        assert!(cfg.show_listening);
        assert!(cfg.show_numeric);
        assert!(cfg.show_processes);
    }

    #[test]
    fn test_parse_args_all() {
        let args = vec!["ss".to_string(), "-a".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_all);
    }

    #[test]
    fn test_parse_args_ipv4() {
        let args = vec!["ss".to_string(), "-4".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.ipv4_only);
    }

    #[test]
    fn test_parse_args_ipv6() {
        let args = vec!["ss".to_string(), "-6".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.ipv6_only);
    }

    #[test]
    fn test_parse_args_summary() {
        let args = vec!["ss".to_string(), "-s".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_summary);
    }

    #[test]
    fn test_parse_args_state_filter() {
        let args = vec![
            "ss".to_string(),
            "state".to_string(),
            "established".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.state_filter, Some(SocketState::Established));
    }

    #[test]
    fn test_parse_args_resolve() {
        let args = vec!["ss".to_string(), "-r".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.resolve_names);
        assert!(!cfg.show_numeric);
    }

    #[test]
    fn test_parse_args_help() {
        let args = vec!["ss".to_string(), "--help".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_help);
    }

    #[test]
    fn test_parse_args_version() {
        let args = vec!["ss".to_string(), "--version".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_version);
    }

    #[test]
    fn test_socket_state_from_str() {
        assert_eq!(SocketState::from_str("ESTABLISHED"), SocketState::Established);
        assert_eq!(SocketState::from_str("ESTAB"), SocketState::Established);
        assert_eq!(SocketState::from_str("LISTEN"), SocketState::Listen);
        assert_eq!(SocketState::from_str("TIME_WAIT"), SocketState::TimeWait);
        assert_eq!(SocketState::from_str("01"), SocketState::Established);
        assert_eq!(SocketState::from_str("0A"), SocketState::Listen);
        assert_eq!(SocketState::from_str("UNKNOWN"), SocketState::Unknown);
    }

    #[test]
    fn test_socket_state_as_str() {
        assert_eq!(SocketState::Established.as_str(), "ESTAB");
        assert_eq!(SocketState::Listen.as_str(), "LISTEN");
        assert_eq!(SocketState::TimeWait.as_str(), "TIME-WAIT");
    }

    #[test]
    fn test_socket_state_long_str() {
        assert_eq!(SocketState::Established.long_str(), "ESTABLISHED");
        assert_eq!(SocketState::Listen.long_str(), "LISTEN");
    }

    #[test]
    fn test_socket_proto_as_str() {
        assert_eq!(SocketProto::Tcp.as_str(), "tcp");
        assert_eq!(SocketProto::Udp.as_str(), "udp");
        assert_eq!(SocketProto::Unix.as_str(), "u_str");
    }

    #[test]
    fn test_parse_hex_ip_v4() {
        // 127.0.0.1 = 0100007F (little-endian hex)
        assert_eq!(parse_hex_ip("0100007F"), "127.0.0.1");
    }

    #[test]
    fn test_parse_hex_ip_v4_zero() {
        assert_eq!(parse_hex_ip("00000000"), "0.0.0.0");
    }

    #[test]
    fn test_parse_hex_port() {
        assert_eq!(parse_hex_port("0050"), 80);
        assert_eq!(parse_hex_port("0016"), 22);
        assert_eq!(parse_hex_port("01BB"), 443);
        assert_eq!(parse_hex_port("0000"), 0);
    }

    #[test]
    fn test_resolve_port() {
        assert_eq!(resolve_port(22, "tcp"), "ssh");
        assert_eq!(resolve_port(80, "tcp"), "http");
        assert_eq!(resolve_port(443, "tcp"), "https");
        assert_eq!(resolve_port(12345, "tcp"), "12345");
    }

    #[test]
    fn test_format_addr_port_numeric() {
        assert_eq!(
            format_addr_port("192.168.1.1", 80, true, "tcp"),
            "192.168.1.1:80"
        );
    }

    #[test]
    fn test_format_addr_port_service() {
        assert_eq!(
            format_addr_port("192.168.1.1", 80, false, "tcp"),
            "192.168.1.1:http"
        );
    }

    #[test]
    fn test_format_addr_port_wildcard() {
        assert_eq!(format_addr_port("0.0.0.0", 22, true, "tcp"), "*:22");
        assert_eq!(format_addr_port("", 22, true, "tcp"), "*:22");
    }

    #[test]
    fn test_run_ss_empty() {
        let cfg = Config {
            show_tcp: true,
            no_header: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_ss(&cfg, &mut buf).unwrap();
        // Just verify no crash
    }

    #[test]
    fn test_run_ss_with_header() {
        let cfg = Config {
            show_tcp: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_ss(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Netid") || output.is_empty());
    }

    #[test]
    fn test_run_ss_summary() {
        let cfg = Config {
            show_summary: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_ss(&cfg, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Total") || output.contains("TCP"));
    }

    #[test]
    fn test_run_sockstat() {
        let cfg = Config {
            personality: Personality::Sockstat,
            show_tcp: true,
            show_all: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_sockstat(&cfg, &mut buf).unwrap();
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert!(!cfg.show_tcp);
        assert!(!cfg.show_udp);
        assert!(cfg.show_numeric);
        assert!(!cfg.show_all);
    }

    #[test]
    fn test_parse_args_extended() {
        let args = vec!["ss".to_string(), "-e".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_extended);
    }

    #[test]
    fn test_parse_args_memory() {
        let args = vec!["ss".to_string(), "-m".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_memory);
    }

    #[test]
    fn test_parse_args_no_header() {
        let args = vec!["ss".to_string(), "-H".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.no_header);
    }

    // -----------------------------------------------------------------------
    // Kernel read-path fallback decoders (SYS_TCP_LIST / LISTENER_LIST).
    // -----------------------------------------------------------------------

    #[test]
    fn test_kernel_tcp_state_mapping() {
        assert_eq!(kernel_tcp_state(0), SocketState::Close);
        assert_eq!(kernel_tcp_state(1), SocketState::Listen);
        assert_eq!(kernel_tcp_state(2), SocketState::SynSent);
        assert_eq!(kernel_tcp_state(3), SocketState::SynRecv);
        assert_eq!(kernel_tcp_state(4), SocketState::Established);
        assert_eq!(kernel_tcp_state(7), SocketState::TimeWait);
        assert_eq!(kernel_tcp_state(9), SocketState::LastAck);
        assert_eq!(kernel_tcp_state(10), SocketState::Unknown);
        assert_eq!(kernel_tcp_state(255), SocketState::Unknown);
    }

    #[test]
    fn test_parse_tcp_list_records() {
        // 10.0.2.15:443 -> 10.0.2.2:54321, Established (4),
        // rx_buffered=0x010203, tx_buffered=0x040506.
        let rec: [u8; 20] = [
            10, 0, 2, 15, // local IP (network-order octets)
            0x01, 0xBB, // local port 443 (BE)
            10, 0, 2, 2, // remote IP
            0xD4, 0x31, // remote port 54321 (BE)
            4, // state = Established
            0x03, 0x02, 0x01, // rx_buffered u24 LE
            0x06, 0x05, 0x04, // tx_buffered u24 LE
            0x01, // flags — ignored
        ];
        let socks = parse_tcp_list_records(&rec);
        assert_eq!(socks.len(), 1);
        let s = &socks[0];
        assert_eq!(s.proto, SocketProto::Tcp);
        assert_eq!(s.local_addr, "10.0.2.15");
        assert_eq!(s.local_port, 443);
        assert_eq!(s.remote_addr, "10.0.2.2");
        assert_eq!(s.remote_port, 54321);
        assert_eq!(s.state, SocketState::Established);
        assert_eq!(s.recv_q, 0x01_0203);
        assert_eq!(s.send_q, 0x04_0506);
    }

    #[test]
    fn test_parse_tcp_list_records_partial_ignored() {
        // 20 valid bytes + 7 trailing partial bytes that must be ignored.
        let mut buf = vec![0u8; 20];
        buf[0] = 127;
        buf[3] = 1;
        buf[4] = 0x00; // port 80 BE
        buf[5] = 0x50;
        buf[12] = 4; // Established
        buf.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]);
        let socks = parse_tcp_list_records(&buf);
        assert_eq!(socks.len(), 1);
        assert_eq!(socks[0].local_addr, "127.0.0.1");
        assert_eq!(socks[0].local_port, 80);
    }

    #[test]
    fn test_parse_tcp_listener_records() {
        let buf: [u8; 8] = [
            0x00, 0x16, 1, 5, // port 22, backlog 1/5
            0x1F, 0x90, 0, 10, // port 8080, backlog 0/10
        ];
        let socks = parse_tcp_listener_records(&buf);
        assert_eq!(socks.len(), 2);
        assert_eq!(socks[0].proto, SocketProto::Tcp);
        assert_eq!(socks[0].local_port, 22);
        assert_eq!(socks[0].state, SocketState::Listen);
        assert_eq!(socks[0].local_addr, "0.0.0.0");
        assert_eq!(socks[1].local_port, 8080);
        assert_eq!(socks[1].state, SocketState::Listen);
    }

    #[test]
    fn test_parse_tcp_records_empty() {
        assert!(parse_tcp_list_records(&[]).is_empty());
        assert!(parse_tcp_listener_records(&[]).is_empty());
    }
}
