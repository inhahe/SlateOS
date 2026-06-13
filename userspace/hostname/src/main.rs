//! SlateOS Hostname Utility — Display and Set the System Hostname
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
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
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
    if let Ok(name) = read_trimmed(PROC_HOSTNAME)
        && !name.is_empty() {
            return Ok(name);
        }

    // Fall back to /etc/hostname.
    if let Ok(name) = read_trimmed(ETC_HOSTNAME)
        && !name.is_empty() {
            return Ok(name);
        }

    Err("unable to determine hostname: neither /proc/sys/kernel/hostname nor /etc/hostname are readable".to_string())
}

/// Parse the domain from `/etc/resolv.conf`.
///
/// Looks for `domain <name>` first, then falls back to the first entry in
/// `search <name1> <name2> ...`. Returns `None` if no domain is configured.
fn read_domain() -> Option<String> {
    let content = fs::read_to_string(RESOLV_CONF).ok()?;
    parse_resolv_conf(&content)
}

/// Pure parser for `/etc/resolv.conf` content. Extracted from `read_domain`
/// so it can be unit-tested without touching the filesystem.
///
/// Priority: `domain <name>` directive wins over `search <name1> ...`. If
/// only `search` is present, the first entry is returned.
fn parse_resolv_conf(content: &str) -> Option<String> {
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
        if search_domain.is_none()
            && let Some(rest) = line.strip_prefix("search") {
                let rest = rest.trim();
                if let Some(first) = rest.split_whitespace().next() {
                    search_domain = Some(first.to_string());
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

    pick_first_meaningful_line(&content)
        .ok_or_else(|| format!("no hostname found in {path} (file is empty or all comments)"))
}

/// Pick the first non-empty, non-comment line from `content`. Trims each line.
/// Extracted from `read_hostname_from_file` so it can be unit-tested without
/// touching the filesystem.
fn pick_first_meaningful_line(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            return Some(line.to_string());
        }
    }
    None
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
///
/// Returns `Err` with a user-facing message on usage errors (unknown flag,
/// missing operand). The caller is responsible for printing the message and
/// exiting with a nonzero status.
fn parse_args(args: &[String], domainname_mode: bool) -> Result<Action, String> {
    // domainname mode: bare invocation shows domain, bare arg sets it.
    if domainname_mode {
        if args.len() <= 1 {
            return Ok(Action::DomainMode { new_name: None });
        }
        let arg = args[1].as_str();
        if arg == "-V" || arg == "--version" {
            return Ok(Action::Version);
        }
        if arg == "-h" || arg == "--help" {
            return Ok(Action::Help);
        }
        return Ok(Action::DomainMode {
            new_name: Some(args[1].clone()),
        });
    }

    if args.len() <= 1 {
        return Ok(Action::Show);
    }

    let mut i = 1;
    let mut boot_mode = false;

    while i < args.len() {
        let arg = args[i].as_str();

        match arg {
            "-h" | "--help" | "help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "-f" | "--fqdn" | "--long" => return Ok(Action::ShowFqdn),
            "-d" | "--domain" => return Ok(Action::ShowDomain),
            "-s" | "--short" => return Ok(Action::ShowShort),
            "-i" | "--ip-address" => return Ok(Action::ShowIp),
            "-I" | "--all-ip-addresses" => return Ok(Action::ShowAllIp),

            "-F" | "--file" => {
                if i + 1 >= args.len() {
                    return Err("-F/--file requires a filename argument".to_string());
                }
                i += 1;
                return Ok(Action::SetFromFile { path: args[i].clone() });
            }

            "-b" | "--boot" => {
                boot_mode = true;
                i += 1;
                continue;
            }

            _ => {
                // Bare argument: set hostname.
                if arg.starts_with('-') {
                    return Err(format!(
                        "unknown option: {arg}\nTry 'hostname --help' for more information."
                    ));
                }

                if boot_mode {
                    return Ok(Action::BootSet { name: arg.to_string() });
                }
                return Ok(Action::Set { name: arg.to_string() });
            }
        }
    }

    // Only modifier flags (-b) with no operand — show current hostname.
    if boot_mode {
        return Err("-b/--boot requires a hostname argument".to_string());
    }

    Ok(Action::Show)
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage(domainname_mode: bool) {
    if domainname_mode {
        println!("SlateOS Domain Name Utility v{VERSION}");
        println!();
        println!("Display or set the NIS/YP domain name.");
        println!();
        println!("USAGE:");
        println!("  domainname                    Display current NIS domain name");
        println!("  domainname <name>             Set NIS domain name");
        println!("  domainname -V                 Display version");
        return;
    }

    println!("SlateOS Hostname Utility v{VERSION}");
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

    let action = match parse_args(&args, domainname_mode) {
        Ok(a) => a,
        Err(msg) => {
            let prog = if domainname_mode { "domainname" } else { "hostname" };
            eprintln!("{prog}: {msg}");
            process::exit(1);
        }
    };

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
            println!("{name} (SlateOS) {VERSION}");
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    // ---------------- validate_hostname ----------------

    #[test]
    fn validate_hostname_accepts_simple_name() {
        assert!(validate_hostname("host").is_ok());
        assert!(validate_hostname("my-server").is_ok());
        assert!(validate_hostname("a").is_ok());
        assert!(validate_hostname("host123").is_ok());
    }

    #[test]
    fn validate_hostname_accepts_dotted_fqdn() {
        assert!(validate_hostname("host.example.com").is_ok());
        assert!(validate_hostname("a.b.c.d.e").is_ok());
    }

    #[test]
    fn validate_hostname_rejects_empty() {
        let err = validate_hostname("").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn validate_hostname_rejects_too_long() {
        let s = "a".repeat(MAX_HOSTNAME_LEN + 1);
        let err = validate_hostname(&s).unwrap_err();
        assert!(err.contains("too long"));
    }

    #[test]
    fn validate_hostname_accepts_max_length() {
        // Max length built from 63-char labels separated by dots, total 253.
        // 63 + 1 + 63 + 1 + 63 + 1 + 61 = 253
        let s = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(s.len(), MAX_HOSTNAME_LEN);
        assert!(validate_hostname(&s).is_ok());
    }

    #[test]
    fn validate_hostname_rejects_empty_label_leading_dot() {
        let err = validate_hostname(".host").unwrap_err();
        assert!(err.contains("empty label"));
    }

    #[test]
    fn validate_hostname_rejects_empty_label_trailing_dot() {
        let err = validate_hostname("host.").unwrap_err();
        assert!(err.contains("empty label"));
    }

    #[test]
    fn validate_hostname_rejects_empty_label_double_dot() {
        let err = validate_hostname("a..b").unwrap_err();
        assert!(err.contains("empty label"));
    }

    #[test]
    fn validate_hostname_rejects_label_too_long() {
        let s = "a".repeat(MAX_LABEL_LEN + 1);
        let err = validate_hostname(&s).unwrap_err();
        assert!(err.contains("too long"));
    }

    #[test]
    fn validate_hostname_rejects_leading_hyphen() {
        let err = validate_hostname("-host").unwrap_err();
        assert!(err.contains("hyphen"));
    }

    #[test]
    fn validate_hostname_rejects_trailing_hyphen() {
        let err = validate_hostname("host-").unwrap_err();
        assert!(err.contains("hyphen"));
    }

    #[test]
    fn validate_hostname_rejects_hyphen_in_label() {
        // Mid-label hyphens are fine — but leading/trailing per-label is not.
        assert!(validate_hostname("a-b.c-d.e").is_ok());
        assert!(validate_hostname("foo.-bar.baz").is_err());
        assert!(validate_hostname("foo.bar-.baz").is_err());
    }

    #[test]
    fn validate_hostname_rejects_invalid_character() {
        assert!(validate_hostname("host_name").is_err());
        assert!(validate_hostname("host name").is_err());
        assert!(validate_hostname("host!").is_err());
        assert!(validate_hostname("h@st").is_err());
    }

    #[test]
    fn validate_hostname_rejects_non_ascii() {
        // RFC 952/1123 hostnames are ASCII alphanumeric + hyphen.
        assert!(validate_hostname("hôst").is_err());
        assert!(validate_hostname("ホスト").is_err());
    }

    // ---------------- parse_args (hostname mode) ----------------

    #[test]
    fn parse_args_no_args_returns_show() {
        let a = parse_args(&args(&["hostname"]), false).unwrap();
        assert_eq!(a, Action::Show);
    }

    #[test]
    fn parse_args_help_variants() {
        for flag in &["-h", "--help", "help"] {
            let a = parse_args(&args(&["hostname", flag]), false).unwrap();
            assert_eq!(a, Action::Help);
        }
    }

    #[test]
    fn parse_args_version_variants() {
        for flag in &["-V", "--version"] {
            let a = parse_args(&args(&["hostname", flag]), false).unwrap();
            assert_eq!(a, Action::Version);
        }
    }

    #[test]
    fn parse_args_fqdn_variants() {
        for flag in &["-f", "--fqdn", "--long"] {
            let a = parse_args(&args(&["hostname", flag]), false).unwrap();
            assert_eq!(a, Action::ShowFqdn);
        }
    }

    #[test]
    fn parse_args_show_domain() {
        let a = parse_args(&args(&["hostname", "-d"]), false).unwrap();
        assert_eq!(a, Action::ShowDomain);
        let a = parse_args(&args(&["hostname", "--domain"]), false).unwrap();
        assert_eq!(a, Action::ShowDomain);
    }

    #[test]
    fn parse_args_show_short() {
        let a = parse_args(&args(&["hostname", "-s"]), false).unwrap();
        assert_eq!(a, Action::ShowShort);
        let a = parse_args(&args(&["hostname", "--short"]), false).unwrap();
        assert_eq!(a, Action::ShowShort);
    }

    #[test]
    fn parse_args_show_ip() {
        let a = parse_args(&args(&["hostname", "-i"]), false).unwrap();
        assert_eq!(a, Action::ShowIp);
        let a = parse_args(&args(&["hostname", "--ip-address"]), false).unwrap();
        assert_eq!(a, Action::ShowIp);
    }

    #[test]
    fn parse_args_show_all_ip() {
        let a = parse_args(&args(&["hostname", "-I"]), false).unwrap();
        assert_eq!(a, Action::ShowAllIp);
        let a = parse_args(&args(&["hostname", "--all-ip-addresses"]), false).unwrap();
        assert_eq!(a, Action::ShowAllIp);
    }

    #[test]
    fn parse_args_set_bare_name() {
        let a = parse_args(&args(&["hostname", "myhost"]), false).unwrap();
        assert_eq!(a, Action::Set { name: "myhost".to_string() });
    }

    #[test]
    fn parse_args_set_from_file_short() {
        let a = parse_args(&args(&["hostname", "-F", "/etc/hostname"]), false).unwrap();
        assert_eq!(
            a,
            Action::SetFromFile {
                path: "/etc/hostname".to_string()
            }
        );
    }

    #[test]
    fn parse_args_set_from_file_long() {
        let a = parse_args(&args(&["hostname", "--file", "/tmp/hn"]), false).unwrap();
        assert_eq!(
            a,
            Action::SetFromFile {
                path: "/tmp/hn".to_string()
            }
        );
    }

    #[test]
    fn parse_args_set_from_file_missing_arg() {
        let err = parse_args(&args(&["hostname", "-F"]), false).unwrap_err();
        assert!(err.contains("-F/--file"));
    }

    #[test]
    fn parse_args_boot_mode_with_name() {
        let a = parse_args(&args(&["hostname", "-b", "myhost"]), false).unwrap();
        assert_eq!(a, Action::BootSet { name: "myhost".to_string() });
        let a = parse_args(&args(&["hostname", "--boot", "myhost"]), false).unwrap();
        assert_eq!(a, Action::BootSet { name: "myhost".to_string() });
    }

    #[test]
    fn parse_args_boot_mode_without_name_errors() {
        let err = parse_args(&args(&["hostname", "-b"]), false).unwrap_err();
        assert!(err.contains("-b/--boot"));
    }

    #[test]
    fn parse_args_unknown_option_errors() {
        let err = parse_args(&args(&["hostname", "-z"]), false).unwrap_err();
        assert!(err.contains("unknown option"));
        assert!(err.contains("-z"));
    }

    #[test]
    fn parse_args_unknown_long_option_errors() {
        let err = parse_args(&args(&["hostname", "--bogus"]), false).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_args_first_action_wins() {
        // Once an action-producing flag is consumed, parsing returns. The
        // second argument is irrelevant.
        let a = parse_args(&args(&["hostname", "-f", "ignored"]), false).unwrap();
        assert_eq!(a, Action::ShowFqdn);
    }

    // ---------------- parse_args (domainname mode) ----------------

    #[test]
    fn parse_args_domainname_no_args_shows() {
        let a = parse_args(&args(&["domainname"]), true).unwrap();
        assert_eq!(a, Action::DomainMode { new_name: None });
    }

    #[test]
    fn parse_args_domainname_with_arg_sets() {
        let a = parse_args(&args(&["domainname", "example.com"]), true).unwrap();
        assert_eq!(
            a,
            Action::DomainMode {
                new_name: Some("example.com".to_string())
            }
        );
    }

    #[test]
    fn parse_args_domainname_version() {
        let a = parse_args(&args(&["domainname", "-V"]), true).unwrap();
        assert_eq!(a, Action::Version);
        let a = parse_args(&args(&["domainname", "--version"]), true).unwrap();
        assert_eq!(a, Action::Version);
    }

    #[test]
    fn parse_args_domainname_help() {
        let a = parse_args(&args(&["domainname", "-h"]), true).unwrap();
        assert_eq!(a, Action::Help);
        let a = parse_args(&args(&["domainname", "--help"]), true).unwrap();
        assert_eq!(a, Action::Help);
    }

    // ---------------- parse_resolv_conf ----------------

    #[test]
    fn parse_resolv_conf_empty_returns_none() {
        assert_eq!(parse_resolv_conf(""), None);
    }

    #[test]
    fn parse_resolv_conf_only_comments_returns_none() {
        let c = "# a comment\n; another\n   \n";
        assert_eq!(parse_resolv_conf(c), None);
    }

    #[test]
    fn parse_resolv_conf_domain_directive() {
        let c = "nameserver 1.1.1.1\ndomain example.com\n";
        assert_eq!(parse_resolv_conf(c), Some("example.com".to_string()));
    }

    #[test]
    fn parse_resolv_conf_search_first_entry() {
        let c = "search foo.com bar.com baz.com\n";
        assert_eq!(parse_resolv_conf(c), Some("foo.com".to_string()));
    }

    #[test]
    fn parse_resolv_conf_domain_beats_search() {
        // Even if search comes first, a later "domain" directive takes
        // priority (returns immediately on encountering it).
        let c = "search alpha.com beta.com\ndomain example.com\n";
        assert_eq!(parse_resolv_conf(c), Some("example.com".to_string()));
    }

    #[test]
    fn parse_resolv_conf_first_domain_wins() {
        let c = "domain first.com\ndomain second.com\n";
        assert_eq!(parse_resolv_conf(c), Some("first.com".to_string()));
    }

    #[test]
    fn parse_resolv_conf_skips_blank_and_comment_lines() {
        let c = "\n# leading comment\n; semi comment\n\ndomain example.com\n";
        assert_eq!(parse_resolv_conf(c), Some("example.com".to_string()));
    }

    #[test]
    fn parse_resolv_conf_empty_domain_directive_falls_through() {
        // "domain" with no argument should not return; falls through.
        let c = "domain   \nsearch fallback.com\n";
        assert_eq!(parse_resolv_conf(c), Some("fallback.com".to_string()));
    }

    #[test]
    fn parse_resolv_conf_trims_whitespace() {
        let c = "   domain    example.com   \n";
        assert_eq!(parse_resolv_conf(c), Some("example.com".to_string()));
    }

    // ---------------- pick_first_meaningful_line ----------------

    #[test]
    fn pick_first_meaningful_line_basic() {
        assert_eq!(
            pick_first_meaningful_line("hello\n"),
            Some("hello".to_string())
        );
    }

    #[test]
    fn pick_first_meaningful_line_skips_blank() {
        assert_eq!(
            pick_first_meaningful_line("\n\n  \nhello\n"),
            Some("hello".to_string())
        );
    }

    #[test]
    fn pick_first_meaningful_line_skips_comments() {
        assert_eq!(
            pick_first_meaningful_line("# a comment\n# another\nactual\n"),
            Some("actual".to_string())
        );
    }

    #[test]
    fn pick_first_meaningful_line_trims_whitespace() {
        assert_eq!(
            pick_first_meaningful_line("   spaced   \n"),
            Some("spaced".to_string())
        );
    }

    #[test]
    fn pick_first_meaningful_line_first_wins() {
        assert_eq!(
            pick_first_meaningful_line("first\nsecond\nthird\n"),
            Some("first".to_string())
        );
    }

    #[test]
    fn pick_first_meaningful_line_empty_returns_none() {
        assert_eq!(pick_first_meaningful_line(""), None);
        assert_eq!(pick_first_meaningful_line("\n\n\n"), None);
        assert_eq!(pick_first_meaningful_line("# only comments\n# more\n"), None);
    }

    // ---------------- parse_proc_if_inet ----------------

    #[test]
    fn parse_proc_if_inet_empty_returns_empty() {
        assert!(parse_proc_if_inet("").is_empty());
    }

    #[test]
    fn parse_proc_if_inet_skips_loopback_v4() {
        let c = "lo 127.0.0.1 255.0.0.0 UP\n";
        assert!(parse_proc_if_inet(c).is_empty());
    }

    #[test]
    fn parse_proc_if_inet_skips_loopback_v6() {
        let c = "lo ::1 - UP\n";
        assert!(parse_proc_if_inet(c).is_empty());
    }

    #[test]
    fn parse_proc_if_inet_skips_127_subnet() {
        let c = "lo 127.0.0.42 255.0.0.0 UP\n";
        assert!(parse_proc_if_inet(c).is_empty());
    }

    #[test]
    fn parse_proc_if_inet_extracts_non_loopback() {
        let c = "eth0 192.168.1.5 255.255.255.0 UP\n";
        assert_eq!(parse_proc_if_inet(c), vec!["192.168.1.5".to_string()]);
    }

    #[test]
    fn parse_proc_if_inet_handles_multiple_interfaces() {
        let c = "\
lo 127.0.0.1 255.0.0.0 UP
eth0 192.168.1.5 255.255.255.0 UP
wlan0 10.0.0.42 255.255.0.0 UP
";
        assert_eq!(
            parse_proc_if_inet(c),
            vec!["192.168.1.5".to_string(), "10.0.0.42".to_string()]
        );
    }

    #[test]
    fn parse_proc_if_inet_skips_comments_and_blank_lines() {
        let c = "\
# header
\
eth0 1.2.3.4 255.255.255.0 UP

# another
";
        assert_eq!(parse_proc_if_inet(c), vec!["1.2.3.4".to_string()]);
    }

    #[test]
    fn parse_proc_if_inet_skips_lines_without_address() {
        // Only an interface name on the line — no address column.
        let c = "eth0\n";
        assert!(parse_proc_if_inet(c).is_empty());
    }
}
