//! OurOS Hostname Utility — Display and Set the System Hostname
//!
//! Reads or writes the system hostname via `/proc/sys/kernel/hostname` and
//! `/etc/hostname`. Supports FQDN resolution via `/etc/resolv.conf`, IP
//! address lookup, and `argv[0]`-based `domainname` mode for NIS domain
//! management.
//!
//! # Usage
//!
//! ```text
//! hostname                       Display current hostname
//! hostname <name>                Set hostname
//! hostname -f                    Display fully qualified domain name
//! hostname -d                    Display domain part only
//! hostname -s                    Display short hostname (up to first dot)
//! hostname -i                    Display IP address(es) for hostname
//! hostname -I                    Display all host IP addresses
//! hostname -F <file>             Set hostname from file
//! hostname -b                    Set hostname only if currently empty
//! hostname -V                    Display version
//! ```
//!
//! When invoked as `domainname` (via argv[0]), shows or sets the NIS domain
//! name from `/proc/sys/kernel/domainname`.

use std::env;
use std::fs;
use std::path::Path;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Live kernel hostname (preferred source for reading).
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";

/// Persistent hostname file (fallback for reading, always written on set).
const ETC_HOSTNAME: &str = "/etc/hostname";

/// NIS domain name path, used when invoked as `domainname`.
const PROC_DOMAINNAME: &str = "/proc/sys/kernel/domainname";

/// Resolver config, used for FQDN / domain extraction.
const RESOLV_CONF: &str = "/etc/resolv.conf";

/// Directory containing per-interface network info.
const SYS_NET_DIR: &str = "/sys/class/net";

/// Proc file listing interface addresses.
const PROC_IF_INET: &str = "/proc/net/if_inet";

/// Maximum total hostname length per RFC 1123.
const MAX_HOSTNAME_LEN: usize = 253;

/// Maximum label length (between dots) per RFC 1123.
const MAX_LABEL_LEN: usize = 63;

// ============================================================================
// Action enum
// ============================================================================

/// Parsed command-line action.
enum Action {
    /// Display the current hostname.
    Show,
    /// Display the fully qualified domain name.
    ShowFqdn,
    /// Display just the domain portion.
    ShowDomain,
    /// Display the short hostname (up to first dot).
    ShowShort,
    /// Display IP address(es) for the hostname.
    ShowIp,
    /// Display all IP addresses on the host.
    ShowAllIp,
    /// Set the hostname to the given value.
    Set { name: String },
    /// Set the hostname from a file.
    SetFromFile { path: String },
    /// Set hostname only if currently empty (boot mode).
    BootSet { name: String },
    /// Display version.
    Version,
    /// Show usage help.
    Help,
    /// Show or set NIS domain name (domainname mode).
    DomainMode { new_name: Option<String> },
}

// ============================================================================
// Reading helpers
// ============================================================================

/// Read a single-line value from a file, trimmed. Returns empty string on
/// missing/unreadable files rather than erroring, since callers often fall
/// back to another source.
fn read_trimmed(path: &str) -> Result<String, String> {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("cannot read {path}: {e}"))
}

/// Read the current hostname. Tries the live proc file first, then the
/// persistent etc file.
fn read_hostname() -> Result<String, String> {
    // Try live kernel value first.
    if let Ok(name) = read_trimmed(PROC_HOSTNAME) {
        if !name.is_empty() {
            return Ok(name);
        }
    }

    // Fall back to /etc/hostname.
    if let Ok(name) = read_trimmed(ETC_HOSTNAME) {
        if !name.is_empty() {
            return Ok(name);
        }
    }

    Err("unable to determine hostname: neither /proc/sys/kernel/hostname nor /etc/hostname are readable".to_string())
}

/// Parse the domain from `/etc/resolv.conf`.
///
/// Looks for `domain <name>` first, then falls back to the first entry in
/// `search <name1> <name2> ...`. Returns `None` if no domain is configured.
fn read_domain() -> Option<String> {
    let content = fs::read_to_string(RESOLV_CONF).ok()?;

    let mut search_domain: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();

        // Skip comments and empty lines.
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // "domain" directive takes priority.
        if let Some(rest) = line.strip_prefix("domain") {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }

        // "search" directive is the fallback — take the first entry.
        if search_domain.is_none() {
            if let Some(rest) = line.strip_prefix("search") {
                let rest = rest.trim();
                if let Some(first) = rest.split_whitespace().next() {
                    search_domain = Some(first.to_string());
                }
            }
        }
    }

    search_domain
}

/// Build the fully qualified domain name from hostname + resolv.conf domain.
fn build_fqdn() -> Result<String, String> {
    let hostname = read_hostname()?;

    // If the hostname already contains a dot, it may already be an FQDN.
    if hostname.contains('.') {
        return Ok(hostname);
    }

    // Append domain from resolv.conf if available.
    if let Some(domain) = read_domain() {
        return Ok(format!("{hostname}.{domain}"));
    }

    // No domain configured — return the bare hostname.
    Ok(hostname)
}

// ============================================================================
// IP address helpers
// ============================================================================

/// Collect IP addresses associated with the hostname.
///
/// Tries `/proc/net/if_inet` first (our OS-specific format), then falls back
/// to scanning `/sys/class/net/*/address`.
fn get_hostname_ips() -> Vec<String> {
    // Try our proc interface first.
    if let Ok(content) = fs::read_to_string(PROC_IF_INET) {
        let ips = parse_proc_if_inet(&content);
        if !ips.is_empty() {
            return ips;
        }
    }

    // Fall back to sysfs scan.
    get_all_ips()
}

/// Collect all IP addresses on all interfaces.
///
/// Reads from `/proc/net/if_inet` first, then scans `/sys/class/net/`.
fn get_all_ips() -> Vec<String> {
    // Try /proc/net/if_inet first.
    if let Ok(content) = fs::read_to_string(PROC_IF_INET) {
        let ips = parse_proc_if_inet(&content);
        if !ips.is_empty() {
            return ips;
        }
    }

    // Scan sysfs.
    scan_sysfs_addresses()
}

/// Parse `/proc/net/if_inet` for IP addresses.
///
/// Expected format (one entry per line):
/// ```text
/// <interface> <address> <netmask> <flags>
/// ```
/// We extract the address column, skipping loopback (127.x.x.x and ::1).
fn parse_proc_if_inet(content: &str) -> Vec<String> {
    let mut ips = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split_whitespace();
        let _iface = fields.next(); // interface name
        if let Some(addr) = fields.next() {
            // Skip loopback addresses.
            if addr == "127.0.0.1" || addr == "::1" || addr.starts_with("127.") {
                continue;
            }
            ips.push(addr.to_string());
        }
    }

    ips
}

/// Scan `/sys/class/net/*/address` for interface addresses.
///
/// These are typically MAC addresses, but on our OS may contain IP info
/// depending on the network driver implementation. Skips loopback and
/// all-zero addresses.
fn scan_sysfs_addresses() -> Vec<String> {
    let mut addrs = Vec::new();
    let net_dir = Path::new(SYS_NET_DIR);

    let entries = match fs::read_dir(net_dir) {
        Ok(e) => e,
        Err(_) => return addrs,
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let iface_name = entry.file_name();
        let iface_str = iface_name.to_string_lossy();

        // Skip loopback.
        if iface_str == "lo" {
            continue;
        }

        let addr_path = entry.path().join("address");
        if let Ok(addr) = fs::read_to_string(&addr_path) {
            let addr = addr.trim();
            // Skip empty or all-zero addresses.
            if !addr.is_empty() && addr != "00:00:00:00:00:00" {
                addrs.push(addr.to_string());
            }
        }
    }

    addrs
}

// ============================================================================
// Hostname validation
// ============================================================================

/// Validate a hostname per RFC 952 / RFC 1123.
///
/// Rules:
/// - Total length 1..=253 characters.
/// - Split on `.` into labels, each 1..=63 characters.
/// - Labels contain only ASCII alphanumeric characters and hyphens.
/// - Labels must not start or end with a hyphen.
fn validate_hostname(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("hostname must not be empty".to_string());
    }

    if name.len() > MAX_HOSTNAME_LEN {
        return Err(format!(
            "hostname too long ({} chars, max {MAX_HOSTNAME_LEN})",
            name.len()
        ));
    }

    for label in name.split('.') {
        if label.is_empty() {
            return Err("hostname contains an empty label (double dot or leading/trailing dot)".to_string());
        }

        if label.len() > MAX_LABEL_LEN {
            return Err(format!(
                "label '{}' too long ({} chars, max {MAX_LABEL_LEN})",
                label,
                label.len()
            ));
        }

        if label.starts_with('-') || label.ends_with('-') {
            return Err(format!("label '{label}' must not start or end with a hyphen"));
        }

        for ch in label.chars() {
            if !ch.is_ascii_alphanumeric() && ch != '-' {
                return Err(format!(
                    "invalid character '{ch}' in label '{label}' (only alphanumeric and hyphens allowed)"
                ));
            }
        }
    }

    Ok(())
}

// ============================================================================
// Hostname setting
// ============================================================================

/// Set the system hostname. Writes to both the live proc file and the
/// persistent /etc/hostname.
///
/// The persistent file is written atomically (write temp, rename) to avoid
/// partial writes on crash.
fn set_hostname(name: &str) -> Result<(), String> {
    validate_hostname(name)?;

    // Write to the live kernel parameter.
    if Path::new(PROC_HOSTNAME).exists() {
        fs::write(PROC_HOSTNAME, name)
            .map_err(|e| format!("cannot write {PROC_HOSTNAME}: {e} (are you root?)"))?;
    }

    // Write to /etc/hostname atomically: write a temp file in the same
    // directory, then rename. This avoids a half-written file if the system
    // crashes mid-write.
    let etc_dir = Path::new(ETC_HOSTNAME)
        .parent()
        .unwrap_or_else(|| Path::new("/etc"));
    let tmp_path = etc_dir.join(".hostname.tmp");

    // Content: hostname followed by a single newline (POSIX convention).
    let content = format!("{name}\n");

    fs::write(&tmp_path, &content)
        .map_err(|e| format!("cannot write {}: {e}", tmp_path.display()))?;

    fs::rename(&tmp_path, ETC_HOSTNAME)
        .map_err(|e| {
            // Best-effort cleanup of the temp file.
            let _ = fs::remove_file(&tmp_path);
            format!("cannot rename {} to {ETC_HOSTNAME}: {e}", tmp_path.display())
        })?;

    Ok(())
}

/// Read a hostname from a file (for -F / --file).
///
/// Reads the first non-empty, non-comment line from the file.
fn read_hostname_from_file(path: &str) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {path}: {e}"))?;

    for line in content.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            return Ok(line.to_string());
        }
    }

    Err(format!("no hostname found in {path} (file is empty or all comments)"))
}

// ============================================================================
// Domainname mode
// ============================================================================

/// Show the NIS domain name.
fn show_domainname() -> Result<(), String> {
    let name = read_trimmed(PROC_DOMAINNAME)?;
    if name.is_empty() || name == "(none)" {
        println!("(none)");
    } else {
        println!("{name}");
    }
    Ok(())
}

/// Set the NIS domain name.
fn set_domainname(name: &str) -> Result<(), String> {
    validate_hostname(name)?;
    fs::write(PROC_DOMAINNAME, name)
        .map_err(|e| format!("cannot write {PROC_DOMAINNAME}: {e} (are you root?)"))?;
    Ok(())
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parse command-line arguments into an `Action`.
///
/// When `domainname_mode` is true (argv[0] ends with "domainname"), we
/// interpret bare arguments as NIS domain operations instead of hostname
/// operations.
fn parse_args(args: &[String], domainname_mode: bool) -> Action {
    // domainname mode: bare invocation shows domain, bare arg sets it.
    if domainname_mode {
        if args.len() <= 1 {
            return Action::DomainMode { new_name: None };
        }
        let arg = args[1].as_str();
        if arg == "-V" || arg == "--version" {
            return Action::Version;
        }
        if arg == "-h" || arg == "--help" {
            return Action::Help;
        }
        return Action::DomainMode {
            new_name: Some(args[1].clone()),
        };
    }

    if args.len() <= 1 {
        return Action::Show;
    }

    let mut i = 1;
    let mut boot_mode = false;

    while i < args.len() {
        let arg = args[i].as_str();

        match arg {
            "-h" | "--help" | "help" => return Action::Help,
            "-V" | "--version" => return Action::Version,
            "-f" | "--fqdn" | "--long" => return Action::ShowFqdn,
            "-d" | "--domain" => return Action::ShowDomain,
            "-s" | "--short" => return Action::ShowShort,
            "-i" | "--ip-address" => return Action::ShowIp,
            "-I" | "--all-ip-addresses" => return Action::ShowAllIp,

            "-F" | "--file" => {
                if i + 1 >= args.len() {
                    eprintln!("hostname: -F/--file requires a filename argument");
                    process::exit(1);
                }
                i += 1;
                return Action::SetFromFile { path: args[i].clone() };
            }

            "-b" | "--boot" => {
                boot_mode = true;
                i += 1;
                continue;
            }

            _ => {
                // Bare argument: set hostname.
                if arg.starts_with('-') {
                    eprintln!("hostname: unknown option: {arg}");
                    eprintln!("Try 'hostname --help' for more information.");
                    process::exit(1);
                }

                if boot_mode {
                    return Action::BootSet { name: arg.to_string() };
                }
                return Action::Set { name: arg.to_string() };
            }
        }
    }

    // Only modifier flags (-b) with no operand — show current hostname.
    if boot_mode {
        eprintln!("hostname: -b/--boot requires a hostname argument");
        process::exit(1);
    }

    Action::Show
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage(domainname_mode: bool) {
    if domainname_mode {
        println!("OurOS Domain Name Utility v{VERSION}");
        println!();
        println!("Display or set the NIS/YP domain name.");
        println!();
        println!("USAGE:");
        println!("  domainname                    Display current NIS domain name");
        println!("  domainname <name>             Set NIS domain name");
        println!("  domainname -V                 Display version");
        return;
    }

    println!("OurOS Hostname Utility v{VERSION}");
    println!();
    println!("Display or set the system hostname.");
    println!();
    println!("USAGE:");
    println!("  hostname                       Display current hostname");
    println!("  hostname <name>                Set hostname");
    println!("  hostname -f                    Display fully qualified domain name");
    println!("  hostname -d                    Display domain part only");
    println!("  hostname -s                    Display short hostname (up to first dot)");
    println!("  hostname -i                    Display IP address(es) for hostname");
    println!("  hostname -I                    Display all host IP addresses");
    println!("  hostname -F <file>             Set hostname from file");
    println!("  hostname -b <name>             Set only if currently empty (boot mode)");
    println!("  hostname -V                    Display version");
    println!();
    println!("OPTIONS:");
    println!("  -f, --fqdn            Fully qualified domain name");
    println!("  -d, --domain           Domain part of the FQDN");
    println!("  -s, --short            Short hostname (up to first dot)");
    println!("  -i, --ip-address       IP address(es) for the hostname");
    println!("  -I, --all-ip-addresses All IP addresses on the host");
    println!("  -F, --file <file>      Read hostname from file");
    println!("  -b, --boot             Set hostname only if currently unset");
    println!("  -V, --version          Show version");
    println!("  -h, --help             Show this help");
    println!();
    println!("FILES:");
    println!("  {PROC_HOSTNAME}  Live kernel hostname");
    println!("  {ETC_HOSTNAME}              Persistent hostname");
    println!("  {RESOLV_CONF}           Domain / search configuration");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // Detect domainname mode from argv[0].
    let domainname_mode = args
        .first()
        .map(|a| {
            Path::new(a)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                == "domainname"
        })
        .unwrap_or(false);

    let action = parse_args(&args, domainname_mode);

    let exit_code = run(action, domainname_mode);
    process::exit(exit_code);
}

/// Execute the parsed action. Returns the exit code (0 = success, 1 = error).
fn run(action: Action, domainname_mode: bool) -> i32 {
    match action {
        Action::Help => {
            print_usage(domainname_mode);
            0
        }

        Action::Version => {
            let name = if domainname_mode { "domainname" } else { "hostname" };
            println!("{name} (OurOS) {VERSION}");
            0
        }

        Action::Show => {
            match read_hostname() {
                Ok(name) => {
                    println!("{name}");
                    0
                }
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::ShowFqdn => {
            match build_fqdn() {
                Ok(fqdn) => {
                    println!("{fqdn}");
                    0
                }
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::ShowDomain => {
            match build_fqdn() {
                Ok(fqdn) => {
                    // Domain is everything after the first dot.
                    if let Some(dot_pos) = fqdn.find('.') {
                        println!("{}", &fqdn[dot_pos + 1..]);
                    }
                    // No domain part — print nothing (matches Linux behavior).
                    0
                }
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::ShowShort => {
            match read_hostname() {
                Ok(name) => {
                    // Short name is everything up to the first dot.
                    let short = name.split('.').next().unwrap_or(&name);
                    println!("{short}");
                    0
                }
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::ShowIp => {
            let ips = get_hostname_ips();
            if ips.is_empty() {
                eprintln!("hostname: no IP addresses found for this host");
                1
            } else {
                println!("{}", ips.join(" "));
                0
            }
        }

        Action::ShowAllIp => {
            let ips = get_all_ips();
            if ips.is_empty() {
                eprintln!("hostname: no IP addresses found");
                1
            } else {
                println!("{}", ips.join(" "));
                0
            }
        }

        Action::Set { name } => {
            match set_hostname(&name) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::SetFromFile { path } => {
            match read_hostname_from_file(&path) {
                Ok(name) => {
                    match set_hostname(&name) {
                        Ok(()) => 0,
                        Err(e) => {
                            eprintln!("hostname: {e}");
                            1
                        }
                    }
                }
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::BootSet { name } => {
            // Only set if the current hostname is empty or unreadable.
            let current = read_hostname().unwrap_or_default();
            if !current.is_empty() {
                // Already set — success (not an error, just a no-op).
                return 0;
            }
            match set_hostname(&name) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("hostname: {e}");
                    1
                }
            }
        }

        Action::DomainMode { new_name } => {
            match new_name {
                None => {
                    match show_domainname() {
                        Ok(()) => 0,
                        Err(e) => {
                            eprintln!("domainname: {e}");
                            1
                        }
                    }
                }
                Some(name) => {
                    match set_domainname(&name) {
                        Ok(()) => 0,
                        Err(e) => {
                            eprintln!("domainname: {e}");
                            1
                        }
                    }
                }
            }
        }
    }
}
