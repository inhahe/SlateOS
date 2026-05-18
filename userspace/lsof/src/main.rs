//! OurOS `lsof` — list open files utility.
//!
//! Reads `/proc/<pid>/fd/`, `/proc/<pid>/fdinfo/`, `/proc/<pid>/stat`,
//! and `/proc/<pid>/status` to enumerate open file descriptors across
//! all (or filtered) processes. Supports network connection display via
//! `/proc/net/tcp` and `/proc/net/udp`.
//!
//! # Usage
//!
//! ```text
//! lsof                         List all open files for all processes
//! lsof -p <pid>                Show files for specific PID
//! lsof -u <user>               Show files for specific user (UID or name)
//! lsof -c <name>               Show files for processes matching name
//! lsof +D <dir>                Show files under directory (path prefix match)
//! lsof -i [proto[@host]:port]  Show network connections
//! lsof -n                      No hostname resolution (numeric only)
//! lsof -P                      No port name resolution (numeric ports)
//! lsof -t                      Terse mode (PIDs only, for piping)
//! lsof --json                  JSON output
//! lsof -r <secs>               Repeat mode
//! ```

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;
use std::thread;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

/// Version string.
const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

/// Information about a single open file descriptor.
struct OpenFile {
    /// Process command name.
    command: String,
    /// Process ID.
    pid: u32,
    /// Owner user (UID as string, or resolved name).
    user: String,
    /// File descriptor column: "cwd", "rtd", "txt", "mem", or "Nr/w/u".
    fd_col: String,
    /// File type: REG, DIR, CHR, BLK, FIFO, SOCK, IPv4, IPv6, unknown.
    file_type: String,
    /// Device major,minor as "maj,min".
    device: String,
    /// Size or offset string.
    size_off: String,
    /// Inode number (as string, or blank).
    node: String,
    /// File name / path / socket description.
    name: String,
}

/// Parsed network connection from /proc/net/tcp or /proc/net/udp.
struct NetEntry {
    protocol: String,
    local_addr: String,
    local_port: u16,
    remote_addr: String,
    remote_port: u16,
    state: String,
    inode: u64,
}

/// Filter configuration from CLI arguments.
struct Config {
    filter_pid: Option<u32>,
    filter_user: Option<String>,
    filter_command: Option<String>,
    filter_dir: Option<String>,
    filter_net: Option<NetFilter>,
    no_hostname: bool,
    no_portname: bool,
    terse: bool,
    json_output: bool,
    repeat_secs: Option<u64>,
}

/// Network filter parsed from `-i` argument.
struct NetFilter {
    protocol: Option<String>,
    host: Option<String>,
    port: Option<u16>,
}

// ============================================================================
// /proc readers
// ============================================================================

/// Read a file into a trimmed string, returning None on any error.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Enumerate all numeric PIDs from /proc.
fn enumerate_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(pid) = name.parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }
    pids.sort_unstable();
    pids
}

/// Read the command name from /proc/<pid>/stat.
/// The comm field is enclosed in parentheses and may contain spaces.
fn read_command(pid: u32) -> Option<String> {
    let content = read_file(&format!("/proc/{pid}/stat"))?;
    let start = content.find('(')?;
    let end = content.rfind(')')?;
    content.get(start + 1..end).map(|s| s.to_string())
}

/// Read the UID from /proc/<pid>/status.
fn read_uid(pid: u32) -> Option<u32> {
    let content = read_file(&format!("/proc/{pid}/status"))?;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("Uid:") {
            return val.trim().split_whitespace().next().and_then(|s| s.parse().ok());
        }
    }
    None
}

/// Resolve a UID to a username by reading /etc/passwd.
/// Falls back to the numeric UID string if resolution fails.
fn uid_to_name(uid: u32) -> String {
    if let Some(content) = read_file("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 {
                if let Ok(file_uid) = fields[2].parse::<u32>() {
                    if file_uid == uid {
                        return fields[0].to_string();
                    }
                }
            }
        }
    }
    uid.to_string()
}

/// Resolve a username to a UID by reading /etc/passwd.
/// Available for future use (e.g., resolving user filters to UIDs for
/// permission-based lookups).
#[allow(dead_code)]
fn name_to_uid(name: &str) -> Option<u32> {
    let content = read_file("/etc/passwd")?;
    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == name {
            return fields[2].parse().ok();
        }
    }
    None
}

/// Read the symlink target for a given fd path.
fn read_fd_link(path: &str) -> Option<String> {
    fs::read_link(path)
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
}

/// Parse fdinfo for a file descriptor to get position and flags.
fn read_fdinfo(pid: u32, fd: u32) -> (String, u32) {
    let path = format!("/proc/{pid}/fdinfo/{fd}");
    let content = match read_file(&path) {
        Some(c) => c,
        None => return ("0t0".to_string(), 0),
    };

    let mut pos: u64 = 0;
    let mut flags: u32 = 0;

    for line in content.lines() {
        if let Some(val) = line.strip_prefix("pos:") {
            pos = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("flags:") {
            flags = u32::from_str_radix(val.trim(), 8).unwrap_or(0);
        }
    }

    let size_off = format!("0t{pos}");
    (size_off, flags)
}

/// Determine the access mode character from octal flags.
/// O_RDONLY=0, O_WRONLY=1, O_RDWR=2.
fn access_mode(flags: u32) -> char {
    match flags & 0o3 {
        0 => 'r',
        1 => 'w',
        2 => 'u', // read+write
        _ => 'u',
    }
}

// ============================================================================
// File type classification
// ============================================================================

/// Classify a file descriptor symlink target into a type string and cleaned name.
fn classify_fd_target(target: &str) -> (String, String) {
    if let Some(rest) = target.strip_prefix("socket:[") {
        let inode = rest.strip_suffix(']').unwrap_or(rest);
        ("SOCK".to_string(), format!("socket:[{inode}]"))
    } else if let Some(rest) = target.strip_prefix("pipe:[") {
        let inode = rest.strip_suffix(']').unwrap_or(rest);
        ("FIFO".to_string(), format!("pipe:[{inode}]"))
    } else if target.starts_with("anon_inode:") {
        ("a_inode".to_string(), target.to_string())
    } else if target.starts_with("/dev/") {
        // Determine if it is a character or block device.
        // We check the file type via stat metadata.
        let dev_type = classify_dev_path(target);
        (dev_type, target.to_string())
    } else {
        // Regular file or directory -- determine from metadata.
        let file_type = classify_path(target);
        (file_type, target.to_string())
    }
}

/// Classify a /dev/ path as CHR or BLK based on metadata.
fn classify_dev_path(path: &str) -> String {
    match fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                "DIR".to_string()
            } else {
                // On our OS, /dev entries are typically character devices.
                // Without platform-specific stat bits, default to CHR.
                "CHR".to_string()
            }
        }
        Err(_) => "CHR".to_string(),
    }
}

/// Classify a filesystem path as REG, DIR, etc. based on metadata.
fn classify_path(path: &str) -> String {
    match fs::metadata(path) {
        Ok(meta) => {
            if meta.is_dir() {
                "DIR".to_string()
            } else if meta.is_file() {
                "REG".to_string()
            } else if meta.is_symlink() {
                "LINK".to_string()
            } else {
                "REG".to_string()
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

/// Extract the inode number from a socket:[] or pipe:[] target.
fn extract_inode_from_target(target: &str) -> Option<u64> {
    if let Some(rest) = target.strip_prefix("socket:[") {
        rest.strip_suffix(']').and_then(|s| s.parse().ok())
    } else if let Some(rest) = target.strip_prefix("pipe:[") {
        rest.strip_suffix(']').and_then(|s| s.parse().ok())
    } else {
        None
    }
}

/// Read device major:minor from the path's metadata.
/// Returns "maj,min" string. Falls back to "0,0" on error.
fn read_device(path: &str) -> String {
    // On our custom OS, we cannot rely on platform-specific stat extensions.
    // Attempt to read from a hypothetical /proc interface or return a placeholder.
    // For /dev/ paths, try to parse the device number from /sys or return a
    // reasonable default.
    match fs::metadata(path) {
        Ok(meta) => {
            let len = meta.len();
            if len > 0 {
                // Provide the size in the device column as a fallback
                // since true dev_t requires OS-specific APIs.
                "0,0".to_string()
            } else {
                "0,0".to_string()
            }
        }
        Err(_) => "0,0".to_string(),
    }
}

/// Read the file size from metadata.
fn read_size(path: &str) -> Option<u64> {
    fs::metadata(path).ok().map(|m| m.len())
}

/// Read the inode number. On our OS, we attempt to read it from metadata.
/// Falls back to empty string.
fn read_inode(path: &str) -> String {
    // Standard Rust metadata does not expose inodes portably.
    // On unix-like systems, we could use std::os::unix::fs::MetadataExt,
    // but our custom target may not support it. Return a placeholder.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(path) {
            return meta.ino().to_string();
        }
    }
    // Fallback for non-unix or error cases.
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    String::new()
}

// ============================================================================
// Network connection parsing
// ============================================================================

/// Parse a hex-encoded IPv4 address from /proc/net/tcp format.
/// Format: `AABBCCDD:PORT` where IP bytes are in host (little-endian) order.
fn parse_ipv4_addr(hex: &str) -> Option<(String, u16)> {
    let mut parts = hex.split(':');
    let ip_hex = parts.next()?;
    let port_hex = parts.next()?;

    if ip_hex.len() != 8 {
        return None;
    }

    let ip_val = u32::from_str_radix(ip_hex, 16).ok()?;
    let octets = [
        (ip_val & 0xFF) as u8,
        ((ip_val >> 8) & 0xFF) as u8,
        ((ip_val >> 16) & 0xFF) as u8,
        ((ip_val >> 24) & 0xFF) as u8,
    ];
    let addr_str = format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]);
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    Some((addr_str, port))
}

/// Parse a hex-encoded IPv6 address from /proc/net/tcp6 format.
fn parse_ipv6_addr(hex: &str) -> Option<(String, u16)> {
    let mut parts = hex.split(':');
    let ip_hex = parts.next()?;
    let port_hex = parts.next()?;

    if ip_hex.len() != 32 {
        return None;
    }

    let mut octets = [0u8; 16];
    for word_idx in 0..4 {
        let start = word_idx * 8;
        let word = u32::from_str_radix(ip_hex.get(start..start + 8)?, 16).ok()?;
        let base = word_idx * 4;
        octets[base] = (word & 0xFF) as u8;
        octets[base + 1] = ((word >> 8) & 0xFF) as u8;
        octets[base + 2] = ((word >> 16) & 0xFF) as u8;
        octets[base + 3] = ((word >> 24) & 0xFF) as u8;
    }

    let addr = std::net::Ipv6Addr::from(octets);
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    Some((format!("{addr}"), port))
}

/// TCP state code to human-readable string.
fn tcp_state_str(hex: &str) -> String {
    let val = u8::from_str_radix(hex.trim(), 16).unwrap_or(0);
    match val {
        1 => "ESTABLISHED".to_string(),
        2 => "SYN_SENT".to_string(),
        3 => "SYN_RECV".to_string(),
        4 => "FIN_WAIT1".to_string(),
        5 => "FIN_WAIT2".to_string(),
        6 => "TIME_WAIT".to_string(),
        7 => "CLOSE".to_string(),
        8 => "CLOSE_WAIT".to_string(),
        9 => "LAST_ACK".to_string(),
        10 => "LISTEN".to_string(),
        11 => "CLOSING".to_string(),
        _ => format!("UNKNOWN({val})"),
    }
}

/// Parse /proc/net/tcp or /proc/net/udp into NetEntry records.
fn parse_proc_net_file(path: &str, protocol: &str, is_v6: bool) -> Vec<NetEntry> {
    let content = match read_file(path) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();

    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }

        let (local_addr, local_port) = if is_v6 {
            match parse_ipv6_addr(fields[1]) {
                Some(v) => v,
                None => continue,
            }
        } else {
            match parse_ipv4_addr(fields[1]) {
                Some(v) => v,
                None => continue,
            }
        };

        let (remote_addr, remote_port) = if is_v6 {
            match parse_ipv6_addr(fields[2]) {
                Some(v) => v,
                None => continue,
            }
        } else {
            match parse_ipv4_addr(fields[2]) {
                Some(v) => v,
                None => continue,
            }
        };

        let state = if protocol.starts_with("tcp") {
            tcp_state_str(fields[3])
        } else {
            String::new()
        };

        let inode = fields.get(9).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);

        entries.push(NetEntry {
            protocol: protocol.to_string(),
            local_addr,
            local_port,
            remote_addr,
            remote_port,
            state,
            inode,
        });
    }

    entries
}

/// Build inode -> NetEntry map from all /proc/net/* files.
fn build_net_inode_map() -> HashMap<u64, NetEntry> {
    let mut map = HashMap::new();

    let sources = [
        ("/proc/net/tcp", "tcp", false),
        ("/proc/net/tcp6", "tcp6", true),
        ("/proc/net/udp", "udp", false),
        ("/proc/net/udp6", "udp6", true),
    ];

    for (path, proto, is_v6) in &sources {
        for entry in parse_proc_net_file(path, proto, *is_v6) {
            if entry.inode != 0 {
                map.insert(entry.inode, entry);
            }
        }
    }

    map
}

// ============================================================================
// Port/service name resolution
// ============================================================================

/// Map common port numbers to service names.
fn port_to_service(port: u16) -> Option<&'static str> {
    match port {
        20 => Some("ftp-data"),
        21 => Some("ftp"),
        22 => Some("ssh"),
        23 => Some("telnet"),
        25 => Some("smtp"),
        53 => Some("domain"),
        67 => Some("bootps"),
        68 => Some("bootpc"),
        80 => Some("http"),
        110 => Some("pop3"),
        123 => Some("ntp"),
        143 => Some("imap"),
        161 => Some("snmp"),
        389 => Some("ldap"),
        443 => Some("https"),
        465 => Some("smtps"),
        514 => Some("syslog"),
        587 => Some("submission"),
        993 => Some("imaps"),
        995 => Some("pop3s"),
        1433 => Some("ms-sql"),
        3306 => Some("mysql"),
        3389 => Some("rdp"),
        5432 => Some("postgresql"),
        5900 => Some("vnc"),
        6379 => Some("redis"),
        8080 => Some("http-alt"),
        8443 => Some("https-alt"),
        _ => None,
    }
}

/// Format a port number, optionally resolving to a service name.
fn format_port(port: u16, resolve: bool) -> String {
    if resolve {
        if let Some(name) = port_to_service(port) {
            return name.to_string();
        }
    }
    port.to_string()
}

/// Format a network connection for the NAME column.
fn format_net_name(entry: &NetEntry, no_hostname: bool, no_portname: bool) -> String {
    let resolve_port = !no_portname;
    let local_port_str = format_port(entry.local_port, resolve_port);
    let remote_port_str = format_port(entry.remote_port, resolve_port);

    let local_addr = if !no_hostname && entry.local_addr == "0.0.0.0" {
        "*".to_string()
    } else if !no_hostname && entry.local_addr == "::" {
        "*".to_string()
    } else {
        entry.local_addr.clone()
    };

    let remote_addr = if !no_hostname && entry.remote_addr == "0.0.0.0" {
        "*".to_string()
    } else if !no_hostname && entry.remote_addr == "::" {
        "*".to_string()
    } else {
        entry.remote_addr.clone()
    };

    let local = format!("{local_addr}:{local_port_str}");
    let remote = format!("{remote_addr}:{remote_port_str}");

    if entry.state.is_empty() {
        // UDP -- no state
        format!("{local}->{remote}")
    } else {
        format!("{local}->{remote} ({state})", state = entry.state)
    }
}

// ============================================================================
// Core: collect open files for a single process
// ============================================================================

/// Collect all open file entries for a given PID.
fn collect_process_files(
    pid: u32,
    net_map: &HashMap<u64, NetEntry>,
    config: &Config,
) -> Vec<OpenFile> {
    let command = match read_command(pid) {
        Some(c) => c,
        None => return Vec::new(), // Process disappeared
    };

    let uid = read_uid(pid).unwrap_or(0);
    let user = uid_to_name(uid);

    // Apply filters.
    if let Some(ref filter_cmd) = config.filter_command {
        if !command.contains(filter_cmd.as_str()) {
            return Vec::new();
        }
    }

    if let Some(ref filter_user) = config.filter_user {
        // Try matching by name or numeric UID.
        if user != *filter_user && uid.to_string() != *filter_user {
            return Vec::new();
        }
    }

    let mut files = Vec::new();

    // Special FDs: cwd, root, exe.
    if let Some(target) = read_fd_link(&format!("/proc/{pid}/cwd")) {
        let (file_type, name) = classify_fd_target(&target);
        let device = read_device(&target);
        let node = read_inode(&target);
        let size_off = read_size(&target)
            .map(|s| s.to_string())
            .unwrap_or_default();
        files.push(OpenFile {
            command: command.clone(),
            pid,
            user: user.clone(),
            fd_col: "cwd".to_string(),
            file_type,
            device,
            size_off,
            node,
            name,
        });
    }

    if let Some(target) = read_fd_link(&format!("/proc/{pid}/root")) {
        let (file_type, name) = classify_fd_target(&target);
        let device = read_device(&target);
        let node = read_inode(&target);
        let size_off = read_size(&target)
            .map(|s| s.to_string())
            .unwrap_or_default();
        files.push(OpenFile {
            command: command.clone(),
            pid,
            user: user.clone(),
            fd_col: "rtd".to_string(),
            file_type,
            device,
            size_off,
            node,
            name,
        });
    }

    if let Some(target) = read_fd_link(&format!("/proc/{pid}/exe")) {
        let (file_type, name) = classify_fd_target(&target);
        let device = read_device(&target);
        let node = read_inode(&target);
        let size_off = read_size(&target)
            .map(|s| s.to_string())
            .unwrap_or_default();
        files.push(OpenFile {
            command: command.clone(),
            pid,
            user: user.clone(),
            fd_col: "txt".to_string(),
            file_type,
            device,
            size_off,
            node,
            name,
        });
    }

    // Memory-mapped files from /proc/<pid>/maps.
    if let Some(maps_content) = read_file(&format!("/proc/{pid}/maps")) {
        let mut seen_paths: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for line in maps_content.lines() {
            // Format: addr perms offset dev inode pathname
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 6 {
                let pathname = fields[5..].join(" ");
                if !pathname.is_empty()
                    && !pathname.starts_with('[')
                    && seen_paths.insert(pathname.clone())
                {
                    let (file_type, name) = classify_fd_target(&pathname);
                    let device = if fields.len() >= 4 {
                        fields[3].replace(':', ",")
                    } else {
                        "0,0".to_string()
                    };
                    let node = if fields.len() >= 5 {
                        fields[4].to_string()
                    } else {
                        String::new()
                    };
                    files.push(OpenFile {
                        command: command.clone(),
                        pid,
                        user: user.clone(),
                        fd_col: "mem".to_string(),
                        file_type,
                        device,
                        size_off: String::new(),
                        node,
                        name,
                    });
                }
            }
        }
    }

    // Numbered file descriptors from /proc/<pid>/fd/.
    let fd_dir = format!("/proc/{pid}/fd");
    if let Ok(entries) = fs::read_dir(&fd_dir) {
        let mut fd_entries: Vec<(u32, String)> = Vec::new();

        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(fd_num) = name.parse::<u32>() {
                    let link_path = format!("{fd_dir}/{name}");
                    if let Some(target) = read_fd_link(&link_path) {
                        fd_entries.push((fd_num, target));
                    }
                }
            }
        }

        // Sort by fd number for stable output.
        fd_entries.sort_by_key(|(fd, _)| *fd);

        for (fd_num, target) in fd_entries {
            let (size_off, flags) = read_fdinfo(pid, fd_num);
            let mode = access_mode(flags);
            let fd_col = format!("{fd_num}{mode}");

            // Check if this is a socket and resolve via net_map.
            if let Some(inode) = extract_inode_from_target(&target) {
                if let Some(net_entry) = net_map.get(&inode) {
                    let net_type = if net_entry.protocol.contains('6') {
                        "IPv6"
                    } else {
                        "IPv4"
                    };
                    let net_name =
                        format_net_name(net_entry, config.no_hostname, config.no_portname);
                    let proto_upper = net_entry.protocol.to_uppercase();
                    files.push(OpenFile {
                        command: command.clone(),
                        pid,
                        user: user.clone(),
                        fd_col,
                        file_type: net_type.to_string(),
                        device: "0,0".to_string(),
                        size_off: "0t0".to_string(),
                        node: format!("{proto_upper}"),
                        name: net_name,
                    });
                    continue;
                }
            }

            let (file_type, name) = classify_fd_target(&target);
            let device = if target.starts_with('/') {
                read_device(&target)
            } else {
                "0,0".to_string()
            };
            let node = if target.starts_with('/') {
                read_inode(&target)
            } else if let Some(ino) = extract_inode_from_target(&target) {
                ino.to_string()
            } else {
                String::new()
            };

            files.push(OpenFile {
                command: command.clone(),
                pid,
                user: user.clone(),
                fd_col,
                file_type,
                device,
                size_off,
                node,
                name,
            });
        }
    }

    files
}

// ============================================================================
// Filtering
// ============================================================================

/// Apply directory filter: keep only files whose name starts with the given prefix.
fn apply_dir_filter(files: &mut Vec<OpenFile>, dir: &str) {
    let prefix = if dir.ends_with('/') {
        dir.to_string()
    } else {
        format!("{dir}/")
    };
    files.retain(|f| f.name.starts_with(&prefix) || f.name == dir.trim_end_matches('/'));
}

/// Apply network filter: keep only network-type entries matching the filter.
fn apply_net_filter(files: &mut Vec<OpenFile>, filter: &NetFilter) {
    files.retain(|f| {
        // Only keep network entries.
        if f.file_type != "IPv4" && f.file_type != "IPv6" && f.file_type != "SOCK" {
            return false;
        }

        // Protocol filter.
        if let Some(ref proto) = filter.protocol {
            let proto_upper = proto.to_uppercase();
            if !f.node.starts_with(&proto_upper) {
                return false;
            }
        }

        // Host filter: check if the name contains the host address.
        if let Some(ref host) = filter.host {
            if !f.name.contains(host.as_str()) {
                return false;
            }
        }

        // Port filter: check if the name contains :port.
        if let Some(port) = filter.port {
            let port_str = format!(":{port}");
            if !f.name.contains(&port_str) {
                return false;
            }
        }

        true
    });
}

// ============================================================================
// Output formatting
// ============================================================================

/// Print the standard lsof header.
fn print_header(stdout: &mut io::StdoutLock<'_>) {
    let _ = writeln!(
        stdout,
        "{:<9} {:>5}  {:<8} {:>4}  {:<6} {:>6} {:>8} {:>5} {}",
        "COMMAND", "PID", "USER", "FD", "TYPE", "DEVICE", "SIZE/OFF", "NODE", "NAME"
    );
}

/// Print a single OpenFile entry in standard format.
fn print_entry(stdout: &mut io::StdoutLock<'_>, f: &OpenFile) {
    // Truncate command to 9 characters (standard lsof behavior).
    let cmd_display: String = if f.command.len() > 9 {
        f.command.chars().take(9).collect()
    } else {
        f.command.clone()
    };

    let _ = writeln!(
        stdout,
        "{:<9} {:>5}  {:<8} {:>4}  {:<6} {:>6} {:>8} {:>5} {}",
        cmd_display, f.pid, f.user, f.fd_col, f.file_type, f.device, f.size_off, f.node, f.name
    );
}

/// Print in terse mode: just unique PIDs, one per line.
fn print_terse(stdout: &mut io::StdoutLock<'_>, files: &[OpenFile]) {
    let mut seen_pids: Vec<u32> = Vec::new();
    for f in files {
        if !seen_pids.contains(&f.pid) {
            seen_pids.push(f.pid);
            let _ = writeln!(stdout, "{}", f.pid);
        }
    }
}

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                // Use a buffer to format the unicode escape without direct
                // allocation in the format string.
                let code = c as u32;
                out.push_str("\\u");
                // Manual hex formatting for 4-digit code point.
                for shift in &[12, 8, 4, 0] {
                    let nibble = (code >> shift) & 0xF;
                    let hex_char = if nibble < 10 {
                        (b'0' + nibble as u8) as char
                    } else {
                        (b'a' + (nibble as u8 - 10)) as char
                    };
                    out.push(hex_char);
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Print all entries as JSON.
fn print_json(stdout: &mut io::StdoutLock<'_>, files: &[OpenFile]) {
    let _ = writeln!(stdout, "[");
    for (i, f) in files.iter().enumerate() {
        let comma = if i + 1 < files.len() { "," } else { "" };
        let _ = writeln!(stdout, "  {{");
        let _ = writeln!(stdout, "    \"command\": \"{}\",", json_escape(&f.command));
        let _ = writeln!(stdout, "    \"pid\": {},", f.pid);
        let _ = writeln!(stdout, "    \"user\": \"{}\",", json_escape(&f.user));
        let _ = writeln!(stdout, "    \"fd\": \"{}\",", json_escape(&f.fd_col));
        let _ = writeln!(stdout, "    \"type\": \"{}\",", json_escape(&f.file_type));
        let _ = writeln!(stdout, "    \"device\": \"{}\",", json_escape(&f.device));
        let _ = writeln!(
            stdout,
            "    \"size_off\": \"{}\",",
            json_escape(&f.size_off)
        );
        let _ = writeln!(stdout, "    \"node\": \"{}\",", json_escape(&f.node));
        let _ = writeln!(stdout, "    \"name\": \"{}\"", json_escape(&f.name));
        let _ = writeln!(stdout, "  }}{comma}");
    }
    let _ = writeln!(stdout, "]");
}

// ============================================================================
// CLI parsing
// ============================================================================

/// Parse the `-i` argument into a NetFilter.
/// Format: `[protocol][@host][:port]`
/// Examples: `-i tcp`, `-i :80`, `-i tcp@192.168.1.1:80`, `-i` (no arg = all network)
fn parse_net_filter(arg: &str) -> NetFilter {
    if arg.is_empty() {
        return NetFilter {
            protocol: None,
            host: None,
            port: None,
        };
    }

    let mut rest = arg;
    let mut protocol = None;
    let mut host = None;
    let mut port = None;

    // Extract protocol prefix (before '@' or ':').
    if let Some(at_pos) = rest.find('@') {
        let proto_part = &rest[..at_pos];
        if !proto_part.is_empty() {
            protocol = Some(proto_part.to_string());
        }
        rest = &rest[at_pos + 1..];
    } else if let Some(colon_pos) = rest.find(':') {
        let proto_part = &rest[..colon_pos];
        // Only treat as protocol if it looks like one (alphabetic).
        if !proto_part.is_empty() && proto_part.chars().all(|c| c.is_ascii_alphabetic()) {
            protocol = Some(proto_part.to_string());
            rest = &rest[colon_pos..];
        }
    } else {
        // Just a protocol name with no host or port.
        if rest.chars().all(|c| c.is_ascii_alphabetic()) {
            return NetFilter {
                protocol: Some(rest.to_string()),
                host: None,
                port: None,
            };
        }
    }

    // Extract host@... and :port.
    if let Some(colon_pos) = rest.find(':') {
        let host_part = &rest[..colon_pos];
        if !host_part.is_empty() {
            host = Some(host_part.to_string());
        }
        let port_part = &rest[colon_pos + 1..];
        if !port_part.is_empty() {
            port = port_part.parse().ok();
        }
    } else if !rest.is_empty() {
        // Could be just a host.
        host = Some(rest.to_string());
    }

    NetFilter {
        protocol,
        host,
        port,
    }
}

fn print_usage() {
    println!("OurOS List Open Files Utility v{VERSION}");
    println!();
    println!("USAGE:");
    println!("  lsof [options]");
    println!();
    println!("OPTIONS:");
    println!("  -p <pid>                Show files for specific PID");
    println!("  -u <user>               Show files for user (UID or name)");
    println!("  -c <name>               Show files for processes matching name");
    println!("  +D <dir>                Show files under directory (path prefix)");
    println!("  -i [proto[@host]:port]  Show network connections");
    println!("  -n                      No hostname resolution");
    println!("  -P                      No port name resolution");
    println!("  -t                      Terse mode (PIDs only)");
    println!("  --json                  JSON output");
    println!("  -r <secs>               Repeat mode (refresh every N seconds)");
    println!("  -h, --help              Show this help");
    println!("  -v, --version           Show version");
    println!();
    println!("EXAMPLES:");
    println!("  lsof                    List all open files");
    println!("  lsof -p 1               Show files for PID 1");
    println!("  lsof -u root            Show files for user root");
    println!("  lsof -c sshd            Show files for sshd processes");
    println!("  lsof +D /var/log        Show files under /var/log");
    println!("  lsof -i tcp:80          Show TCP connections on port 80");
    println!("  lsof -i :22             Show all connections on port 22");
    println!("  lsof -t -i tcp          PIDs of processes with TCP connections");
    println!("  lsof -r 5               Refresh every 5 seconds");
}

fn parse_args() -> Config {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        filter_pid: None,
        filter_user: None,
        filter_command: None,
        filter_dir: None,
        filter_net: None,
        no_hostname: false,
        no_portname: false,
        terse: false,
        json_output: false,
        repeat_secs: None,
    };

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-h" | "--help" | "help" => {
                print_usage();
                process::exit(0);
            }
            "-v" | "--version" => {
                println!("lsof {VERSION}");
                process::exit(0);
            }
            "-p" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lsof: -p requires a PID argument");
                    process::exit(1);
                }
                match args[i].parse::<u32>() {
                    Ok(pid) => config.filter_pid = Some(pid),
                    Err(_) => {
                        eprintln!("lsof: invalid PID: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-u" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lsof: -u requires a user argument");
                    process::exit(1);
                }
                config.filter_user = Some(args[i].clone());
            }
            "-c" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lsof: -c requires a command name argument");
                    process::exit(1);
                }
                config.filter_command = Some(args[i].clone());
            }
            "+D" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lsof: +D requires a directory argument");
                    process::exit(1);
                }
                config.filter_dir = Some(args[i].clone());
            }
            "-i" => {
                // -i can have an optional argument.
                if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                    i += 1;
                    config.filter_net = Some(parse_net_filter(&args[i]));
                } else {
                    // No argument: show all network files.
                    config.filter_net = Some(NetFilter {
                        protocol: None,
                        host: None,
                        port: None,
                    });
                }
            }
            "-n" => {
                config.no_hostname = true;
            }
            "-P" => {
                config.no_portname = true;
            }
            "-t" => {
                config.terse = true;
            }
            "--json" => {
                config.json_output = true;
            }
            "-r" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("lsof: -r requires a seconds argument");
                    process::exit(1);
                }
                match args[i].parse::<u64>() {
                    Ok(secs) if secs > 0 => config.repeat_secs = Some(secs),
                    _ => {
                        eprintln!("lsof: invalid repeat interval: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            other => {
                eprintln!("lsof: unknown option: {other}");
                eprintln!("Try 'lsof --help' for usage.");
                process::exit(1);
            }
        }
        i += 1;
    }

    config
}

// ============================================================================
// Main collection and output
// ============================================================================

/// Run one pass of lsof: collect files, apply filters, output.
fn run_once(config: &Config) {
    let net_map = build_net_inode_map();

    let pids = if let Some(pid) = config.filter_pid {
        vec![pid]
    } else {
        enumerate_pids()
    };

    let mut all_files: Vec<OpenFile> = Vec::new();

    for pid in pids {
        let mut proc_files = collect_process_files(pid, &net_map, config);
        all_files.append(&mut proc_files);
    }

    // Apply directory filter.
    if let Some(ref dir) = config.filter_dir {
        apply_dir_filter(&mut all_files, dir);
    }

    // Apply network filter.
    if let Some(ref net_filter) = config.filter_net {
        apply_net_filter(&mut all_files, net_filter);
    }

    // Output.
    let stdout_handle = io::stdout();
    let mut stdout = stdout_handle.lock();

    if config.terse {
        print_terse(&mut stdout, &all_files);
    } else if config.json_output {
        print_json(&mut stdout, &all_files);
    } else {
        print_header(&mut stdout);
        for f in &all_files {
            print_entry(&mut stdout, f);
        }
    }
}

fn main() {
    let config = parse_args();

    if let Some(secs) = config.repeat_secs {
        // Repeat mode: run in a loop.
        loop {
            // Clear screen before each refresh (ANSI escape).
            print!("\x1b[2J\x1b[H");
            let _ = io::stdout().flush();
            run_once(&config);
            thread::sleep(Duration::from_secs(secs));
        }
    } else {
        run_once(&config);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ipv4_addr_loopback() {
        let result = parse_ipv4_addr("0100007F:0050");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "127.0.0.1");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_ipv4_addr_any() {
        let result = parse_ipv4_addr("00000000:0016");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "0.0.0.0");
        assert_eq!(port, 22);
    }

    #[test]
    fn test_parse_ipv4_addr_specific() {
        // 192.168.1.100 = C0.A8.01.64 -> LE hex = 6401A8C0
        let result = parse_ipv4_addr("6401A8C0:1F90");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "192.168.1.100");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_ipv4_addr_invalid() {
        assert!(parse_ipv4_addr("ZZZZZZZZ:0050").is_none());
        assert!(parse_ipv4_addr("0100007F").is_none());
        assert!(parse_ipv4_addr("").is_none());
        assert!(parse_ipv4_addr("SHORT:00").is_none());
    }

    #[test]
    fn test_parse_ipv6_addr_loopback() {
        // ::1 in /proc format
        let result = parse_ipv6_addr("00000000000000000000000001000000:0050");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "::1");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_ipv6_addr_any() {
        let result = parse_ipv6_addr("00000000000000000000000000000000:0016");
        assert!(result.is_some());
        let (addr, port) = result.unwrap();
        assert_eq!(addr, "::");
        assert_eq!(port, 22);
    }

    #[test]
    fn test_parse_ipv6_invalid() {
        assert!(parse_ipv6_addr("SHORT:0050").is_none());
        assert!(parse_ipv6_addr("").is_none());
    }

    #[test]
    fn test_tcp_state_str() {
        assert_eq!(tcp_state_str("01"), "ESTABLISHED");
        assert_eq!(tcp_state_str("0A"), "LISTEN");
        assert_eq!(tcp_state_str("06"), "TIME_WAIT");
        assert_eq!(tcp_state_str("08"), "CLOSE_WAIT");
        assert_eq!(tcp_state_str("FF"), "UNKNOWN(255)");
    }

    #[test]
    fn test_access_mode() {
        assert_eq!(access_mode(0o0), 'r');   // O_RDONLY
        assert_eq!(access_mode(0o1), 'w');   // O_WRONLY
        assert_eq!(access_mode(0o2), 'u');   // O_RDWR
        assert_eq!(access_mode(0o100), 'r'); // O_RDONLY with O_CREAT
        assert_eq!(access_mode(0o101), 'w'); // O_WRONLY with O_CREAT
        assert_eq!(access_mode(0o102), 'u'); // O_RDWR with O_CREAT
    }

    #[test]
    fn test_classify_fd_target_socket() {
        let (ft, name) = classify_fd_target("socket:[12345]");
        assert_eq!(ft, "SOCK");
        assert_eq!(name, "socket:[12345]");
    }

    #[test]
    fn test_classify_fd_target_pipe() {
        let (ft, name) = classify_fd_target("pipe:[67890]");
        assert_eq!(ft, "FIFO");
        assert_eq!(name, "pipe:[67890]");
    }

    #[test]
    fn test_classify_fd_target_anon_inode() {
        let (ft, name) = classify_fd_target("anon_inode:[eventpoll]");
        assert_eq!(ft, "a_inode");
        assert_eq!(name, "anon_inode:[eventpoll]");
    }

    #[test]
    fn test_classify_fd_target_dev() {
        let (ft, _name) = classify_fd_target("/dev/null");
        assert_eq!(ft, "CHR");
    }

    #[test]
    fn test_extract_inode_from_target() {
        assert_eq!(extract_inode_from_target("socket:[12345]"), Some(12345));
        assert_eq!(extract_inode_from_target("pipe:[67890]"), Some(67890));
        assert_eq!(extract_inode_from_target("/dev/null"), None);
        assert_eq!(extract_inode_from_target("anon_inode:[eventpoll]"), None);
    }

    #[test]
    fn test_port_to_service() {
        assert_eq!(port_to_service(22), Some("ssh"));
        assert_eq!(port_to_service(80), Some("http"));
        assert_eq!(port_to_service(443), Some("https"));
        assert_eq!(port_to_service(3306), Some("mysql"));
        assert_eq!(port_to_service(12345), None);
    }

    #[test]
    fn test_format_port() {
        assert_eq!(format_port(80, true), "http");
        assert_eq!(format_port(80, false), "80");
        assert_eq!(format_port(12345, true), "12345");
        assert_eq!(format_port(22, true), "ssh");
    }

    #[test]
    fn test_json_escape() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
        assert_eq!(json_escape("line\nnew"), "line\\nnew");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn test_parse_net_filter_empty() {
        let f = parse_net_filter("");
        assert!(f.protocol.is_none());
        assert!(f.host.is_none());
        assert!(f.port.is_none());
    }

    #[test]
    fn test_parse_net_filter_protocol_only() {
        let f = parse_net_filter("tcp");
        assert_eq!(f.protocol.as_deref(), Some("tcp"));
        assert!(f.host.is_none());
        assert!(f.port.is_none());
    }

    #[test]
    fn test_parse_net_filter_port_only() {
        let f = parse_net_filter(":80");
        assert!(f.protocol.is_none());
        assert!(f.host.is_none());
        assert_eq!(f.port, Some(80));
    }

    #[test]
    fn test_parse_net_filter_proto_port() {
        let f = parse_net_filter("tcp:443");
        assert_eq!(f.protocol.as_deref(), Some("tcp"));
        assert!(f.host.is_none());
        assert_eq!(f.port, Some(443));
    }

    #[test]
    fn test_parse_net_filter_full() {
        let f = parse_net_filter("tcp@192.168.1.1:80");
        assert_eq!(f.protocol.as_deref(), Some("tcp"));
        assert_eq!(f.host.as_deref(), Some("192.168.1.1"));
        assert_eq!(f.port, Some(80));
    }

    #[test]
    fn test_apply_dir_filter() {
        let mut files = vec![
            OpenFile {
                command: "test".into(),
                pid: 1,
                user: "root".into(),
                fd_col: "0r".into(),
                file_type: "REG".into(),
                device: "0,0".into(),
                size_off: "100".into(),
                node: "1".into(),
                name: "/var/log/syslog".into(),
            },
            OpenFile {
                command: "test".into(),
                pid: 1,
                user: "root".into(),
                fd_col: "1w".into(),
                file_type: "REG".into(),
                device: "0,0".into(),
                size_off: "200".into(),
                node: "2".into(),
                name: "/etc/config.yaml".into(),
            },
            OpenFile {
                command: "test".into(),
                pid: 1,
                user: "root".into(),
                fd_col: "2w".into(),
                file_type: "REG".into(),
                device: "0,0".into(),
                size_off: "300".into(),
                node: "3".into(),
                name: "/var/log/auth.log".into(),
            },
        ];

        apply_dir_filter(&mut files, "/var/log");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "/var/log/syslog");
        assert_eq!(files[1].name, "/var/log/auth.log");
    }

    #[test]
    fn test_apply_net_filter_protocol() {
        let mut files = vec![
            OpenFile {
                command: "httpd".into(),
                pid: 100,
                user: "root".into(),
                fd_col: "3u".into(),
                file_type: "IPv4".into(),
                device: "0,0".into(),
                size_off: "0t0".into(),
                node: "TCP".into(),
                name: "*:http->*:* (LISTEN)".into(),
            },
            OpenFile {
                command: "named".into(),
                pid: 200,
                user: "root".into(),
                fd_col: "4u".into(),
                file_type: "IPv4".into(),
                device: "0,0".into(),
                size_off: "0t0".into(),
                node: "UDP".into(),
                name: "*:53->*:*".into(),
            },
        ];

        let filter = NetFilter {
            protocol: Some("TCP".to_string()),
            host: None,
            port: None,
        };

        apply_net_filter(&mut files, &filter);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].command, "httpd");
    }

    #[test]
    fn test_format_net_name_tcp() {
        let entry = NetEntry {
            protocol: "tcp".to_string(),
            local_addr: "0.0.0.0".to_string(),
            local_port: 22,
            remote_addr: "0.0.0.0".to_string(),
            remote_port: 0,
            state: "LISTEN".to_string(),
            inode: 100,
        };

        // With hostname + port resolution.
        let name = format_net_name(&entry, false, false);
        assert!(name.contains("*:ssh"));
        assert!(name.contains("(LISTEN)"));

        // Numeric mode.
        let name_numeric = format_net_name(&entry, true, true);
        assert!(name_numeric.contains("0.0.0.0:22"));
    }

    #[test]
    fn test_format_net_name_udp() {
        let entry = NetEntry {
            protocol: "udp".to_string(),
            local_addr: "127.0.0.1".to_string(),
            local_port: 53,
            remote_addr: "0.0.0.0".to_string(),
            remote_port: 0,
            state: String::new(),
            inode: 200,
        };

        let name = format_net_name(&entry, false, false);
        // UDP has no state, so no parenthetical.
        assert!(!name.contains('('));
        assert!(name.contains("127.0.0.1:domain"));
    }
}
