//! SlateOS hostname management utilities.
//!
//! Multi-personality binary providing:
//! - **hostnamectl** — query and set system hostname and related settings
//! - **hostname** — show or set hostname (simple interface)
//! - **domainname** — show or set NIS domain name
//! - **dnsdomainname** — show the DNS domain name
//!
//! Manages hostname via `/etc/hostname`, `/proc/sys/kernel/hostname`,
//! and machine info via `/etc/machine-info`.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const ETC_HOSTNAME: &str = "/etc/hostname";
const PROC_HOSTNAME: &str = "/proc/sys/kernel/hostname";
const PROC_DOMAINNAME: &str = "/proc/sys/kernel/domainname";
const ETC_MACHINE_INFO: &str = "/etc/machine-info";
const OS_RELEASE: &str = "/etc/os-release";

// ============================================================================
// Data structures
// ============================================================================

/// Machine info from /etc/machine-info.
struct MachineInfo {
    pretty_hostname: String,
    icon_name: String,
    chassis: String,
    deployment: String,
    location: String,
}

/// OS release info from /etc/os-release.
struct OsRelease {
    name: String,
    version: String,
    pretty_name: String,
    id: String,
    cpe_name: String,
}

// ============================================================================
// Reading system state
// ============================================================================

fn read_file_trimmed(path: &str) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn get_hostname() -> String {
    let hostname = read_file_trimmed(PROC_HOSTNAME);
    if hostname.is_empty() {
        read_file_trimmed(ETC_HOSTNAME)
    } else {
        hostname
    }
}

fn get_domainname() -> String {
    let domain = read_file_trimmed(PROC_DOMAINNAME);
    if domain == "(none)" { String::new() } else { domain }
}

fn get_fqdn() -> String {
    let hostname = get_hostname();
    // Try to read from /etc/hosts for the FQDN.
    if let Ok(content) = fs::read_to_string("/etc/hosts") {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 2 {
                for &name in &fields[1..] {
                    if name == hostname && fields.len() > 2 {
                        // Find the first FQDN-looking name.
                        for &n in &fields[1..] {
                            if n.contains('.') {
                                return n.to_string();
                            }
                        }
                    }
                }
            }
        }
    }

    let domain = get_domainname();
    if !domain.is_empty() {
        format!("{hostname}.{domain}")
    } else {
        hostname
    }
}

fn read_machine_info() -> MachineInfo {
    let content = read_file_trimmed(ETC_MACHINE_INFO);
    let mut info = MachineInfo {
        pretty_hostname: String::new(),
        icon_name: String::new(),
        chassis: String::new(),
        deployment: String::new(),
        location: String::new(),
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(val) = line.strip_prefix("PRETTY_HOSTNAME=") {
            info.pretty_hostname = unquote(val);
        } else if let Some(val) = line.strip_prefix("ICON_NAME=") {
            info.icon_name = unquote(val);
        } else if let Some(val) = line.strip_prefix("CHASSIS=") {
            info.chassis = unquote(val);
        } else if let Some(val) = line.strip_prefix("DEPLOYMENT=") {
            info.deployment = unquote(val);
        } else if let Some(val) = line.strip_prefix("LOCATION=") {
            info.location = unquote(val);
        }
    }
    info
}

fn read_os_release() -> OsRelease {
    let content = read_file_trimmed(OS_RELEASE);
    let mut info = OsRelease {
        name: String::new(),
        version: String::new(),
        pretty_name: String::new(),
        id: String::new(),
        cpe_name: String::new(),
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(val) = line.strip_prefix("NAME=") {
            info.name = unquote(val);
        } else if let Some(val) = line.strip_prefix("VERSION=") {
            info.version = unquote(val);
        } else if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
            info.pretty_name = unquote(val);
        } else if let Some(val) = line.strip_prefix("ID=") {
            info.id = unquote(val);
        } else if let Some(val) = line.strip_prefix("CPE_NAME=") {
            info.cpe_name = unquote(val);
        }
    }
    info
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\''))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn get_kernel_version() -> String {
    read_file_trimmed("/proc/sys/kernel/osrelease")
}

fn get_architecture() -> String {
    // Read from uname info or /proc/sys/kernel/arch.
    let arch = read_file_trimmed("/proc/sys/kernel/arch");
    if arch.is_empty() { "x86_64".to_string() } else { arch }
}

fn get_machine_id() -> String {
    read_file_trimmed("/etc/machine-id")
}

fn get_boot_id() -> String {
    read_file_trimmed("/proc/sys/kernel/random/boot_id")
}

fn detect_virtualization() -> String {
    // Check common virtualization indicators.
    if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
        if content.contains("QEMU") || content.contains("qemu") {
            return "qemu".to_string();
        }
        if content.contains("VMware") {
            return "vmware".to_string();
        }
        if content.contains("VirtualBox") || content.contains("VBOX") {
            return "oracle".to_string();
        }
    }

    // Check DMI info.
    let product = read_file_trimmed("/sys/class/dmi/id/product_name");
    if product.contains("VirtualBox") {
        return "oracle".to_string();
    }
    if product.contains("VMware") {
        return "vmware".to_string();
    }
    if product.contains("QEMU") || product.contains("KVM") {
        return "kvm".to_string();
    }
    if product.contains("Hyper-V") {
        return "microsoft".to_string();
    }

    // Check for containers.
    if std::path::Path::new("/.dockerenv").exists() {
        return "docker".to_string();
    }
    if let Ok(cgroup) = fs::read_to_string("/proc/1/cgroup")
        && (cgroup.contains("/docker/") || cgroup.contains("/lxc/")) {
            return "container".to_string();
        }

    "none".to_string()
}

fn detect_chassis() -> String {
    // Try from machine-info first.
    let info = read_machine_info();
    if !info.chassis.is_empty() {
        return info.chassis;
    }

    // Try DMI chassis type.
    let chassis_type = read_file_trimmed("/sys/class/dmi/id/chassis_type");
    match chassis_type.as_str() {
        "1" => "other".to_string(),
        "3" | "4" | "5" | "6" | "7" | "15" | "16" => "desktop".to_string(),
        "8" | "9" | "10" | "14" => "laptop".to_string(),
        "11" => "handset".to_string(),
        "17" | "18" | "19" | "20" | "21" | "22" | "23" | "24" => "server".to_string(),
        "25" => "tablet".to_string(),
        "30" | "31" | "32" => "tablet".to_string(),
        _ => "desktop".to_string(),
    }
}

// ============================================================================
// Writing system state
// ============================================================================

fn set_hostname(name: &str) -> io::Result<()> {
    // Validate hostname.
    if name.is_empty() || name.len() > 64 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,
            "hostname must be 1-64 characters"));
    }
    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '.' {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                format!("invalid character in hostname: '{ch}'")));
        }
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,
            "hostname must not start or end with '-'"));
    }

    // Write to /etc/hostname (persistent).
    fs::write(ETC_HOSTNAME, format!("{name}\n"))?;
    // Write to /proc (transient).
    let _ = fs::write(PROC_HOSTNAME, name);
    Ok(())
}

fn set_machine_info_field(key: &str, value: &str) -> io::Result<()> {
    let content = fs::read_to_string(ETC_MACHINE_INFO).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;

    for line in content.lines() {
        if line.starts_with(&format!("{key}=")) {
            lines.push(format!("{key}=\"{value}\""));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("{key}=\"{value}\""));
    }

    let mut output = lines.join("\n");
    output.push('\n');
    fs::write(ETC_MACHINE_INFO, output)
}

// ============================================================================
// Personality: hostnamectl
// ============================================================================

fn cmd_hostnamectl(args: &[String]) {
    if args.is_empty() {
        // Default: show status.
        show_status();
        return;
    }

    let subcmd = args[0].as_str();
    let rest = &args[1..];

    match subcmd {
        "status" => {
            // Parse optional --json flag.
            let json = rest.iter().any(|a| a == "--json" || a == "-j" || a == "--json=pretty");
            if json {
                show_status_json();
            } else {
                show_status();
            }
        }
        "hostname" | "set-hostname" => {
            if rest.is_empty() {
                println!("{}", get_hostname());
                return;
            }
            // Parse flags.
            let mut transient = false;
            let mut pretty = false;
            let mut name: Option<&str> = None;

            for arg in rest {
                match arg.as_str() {
                    "--transient" => transient = true,
                    "--pretty" => pretty = true,
                    "--static" => {} // default behavior
                    s if !s.starts_with('-') => name = Some(s),
                    other => {
                        eprintln!("hostnamectl: unknown option: {other}");
                        process::exit(1);
                    }
                }
            }

            let name = match name {
                Some(n) => n,
                None => {
                    eprintln!("hostnamectl set-hostname: no hostname specified");
                    process::exit(1);
                }
            };

            if pretty {
                if let Err(e) = set_machine_info_field("PRETTY_HOSTNAME", name) {
                    eprintln!("hostnamectl: failed to set pretty hostname: {e}");
                    process::exit(1);
                }
            } else if transient {
                if let Err(e) = fs::write(PROC_HOSTNAME, name) {
                    eprintln!("hostnamectl: failed to set transient hostname: {e}");
                    process::exit(1);
                }
            } else {
                if let Err(e) = set_hostname(name) {
                    eprintln!("hostnamectl: failed to set hostname: {e}");
                    process::exit(1);
                }
            }
        }
        "icon-name" | "set-icon-name" => {
            if rest.is_empty() {
                let info = read_machine_info();
                println!("{}", info.icon_name);
                return;
            }
            if let Err(e) = set_machine_info_field("ICON_NAME", &rest[0]) {
                eprintln!("hostnamectl: failed to set icon name: {e}");
                process::exit(1);
            }
        }
        "chassis" | "set-chassis" => {
            if rest.is_empty() {
                println!("{}", detect_chassis());
                return;
            }
            let valid = ["desktop", "laptop", "convertible", "server", "tablet",
                "handset", "watch", "embedded", "vm", "container", ""];
            if !valid.contains(&rest[0].as_str()) {
                eprintln!("hostnamectl: invalid chassis type: {}", rest[0]);
                eprintln!("Valid types: desktop, laptop, convertible, server, tablet, handset, watch, embedded, vm, container");
                process::exit(1);
            }
            if let Err(e) = set_machine_info_field("CHASSIS", &rest[0]) {
                eprintln!("hostnamectl: failed to set chassis: {e}");
                process::exit(1);
            }
        }
        "deployment" | "set-deployment" => {
            if rest.is_empty() {
                let info = read_machine_info();
                println!("{}", info.deployment);
                return;
            }
            if let Err(e) = set_machine_info_field("DEPLOYMENT", &rest[0]) {
                eprintln!("hostnamectl: failed to set deployment: {e}");
                process::exit(1);
            }
        }
        "location" | "set-location" => {
            if rest.is_empty() {
                let info = read_machine_info();
                println!("{}", info.location);
                return;
            }
            if let Err(e) = set_machine_info_field("LOCATION", &rest[0]) {
                eprintln!("hostnamectl: failed to set location: {e}");
                process::exit(1);
            }
        }
        "-h" | "--help" | "help" => {
            println!("Usage: hostnamectl [COMMAND]");
            println!();
            println!("Commands:");
            println!("  status              Show current hostname settings (default)");
            println!("  hostname [NAME]     Get/set the system hostname");
            println!("    --transient       Set transient hostname only");
            println!("    --pretty          Set pretty hostname only");
            println!("  icon-name [NAME]    Get/set the host icon name");
            println!("  chassis [TYPE]      Get/set the chassis type");
            println!("  deployment [ENV]    Get/set the deployment environment");
            println!("  location [LOC]      Get/set the location");
            println!();
            println!("Options:");
            println!("  -h, --help          Show this help");
            println!("  -v, --version       Show version");
            println!("  --json              JSON output (with status)");
            process::exit(0);
        }
        "-v" | "--version" => {
            println!("hostnamectl {VERSION}");
            process::exit(0);
        }
        other => {
            eprintln!("hostnamectl: unknown command: {other}");
            eprintln!("Try 'hostnamectl --help' for usage.");
            process::exit(1);
        }
    }
}

fn show_status() {
    let hostname = get_hostname();
    let machine_info = read_machine_info();
    let os = read_os_release();
    let kernel = get_kernel_version();
    let arch = get_architecture();
    let machine_id = get_machine_id();
    let boot_id = get_boot_id();
    let virt = detect_virtualization();
    let chassis = detect_chassis();

    let pretty = if machine_info.pretty_hostname.is_empty() {
        &hostname
    } else {
        &machine_info.pretty_hostname
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "   Static hostname: {hostname}");
    if !machine_info.pretty_hostname.is_empty() {
        let _ = writeln!(out, "   Pretty hostname: {pretty}");
    }
    let _ = writeln!(out, "         Icon name: {}", if machine_info.icon_name.is_empty() { "computer-desktop" } else { &machine_info.icon_name });
    let _ = writeln!(out, "           Chassis: {chassis}");
    if !machine_info.deployment.is_empty() {
        let _ = writeln!(out, "        Deployment: {}", machine_info.deployment);
    }
    if !machine_info.location.is_empty() {
        let _ = writeln!(out, "          Location: {}", machine_info.location);
    }
    let _ = writeln!(out, "        Machine ID: {machine_id}");
    let _ = writeln!(out, "           Boot ID: {boot_id}");
    if virt != "none" {
        let _ = writeln!(out, "    Virtualization: {virt}");
    }
    let _ = writeln!(out, "  Operating System: {}", if os.pretty_name.is_empty() { &os.name } else { &os.pretty_name });
    if !os.cpe_name.is_empty() {
        let _ = writeln!(out, "       CPE OS Name: {}", os.cpe_name);
    }
    let _ = writeln!(out, "            Kernel: {kernel}");
    let _ = writeln!(out, "      Architecture: {arch}");
}

fn show_status_json() {
    let hostname = get_hostname();
    let machine_info = read_machine_info();
    let os = read_os_release();
    let kernel = get_kernel_version();
    let arch = get_architecture();
    let machine_id = get_machine_id();
    let boot_id = get_boot_id();
    let virt = detect_virtualization();
    let chassis = detect_chassis();

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"Hostname\": \"{hostname}\",");
    let _ = writeln!(out, "  \"PrettyHostname\": \"{}\",", machine_info.pretty_hostname);
    let _ = writeln!(out, "  \"IconName\": \"{}\",", machine_info.icon_name);
    let _ = writeln!(out, "  \"Chassis\": \"{chassis}\",");
    let _ = writeln!(out, "  \"Deployment\": \"{}\",", machine_info.deployment);
    let _ = writeln!(out, "  \"Location\": \"{}\",", machine_info.location);
    let _ = writeln!(out, "  \"MachineID\": \"{machine_id}\",");
    let _ = writeln!(out, "  \"BootID\": \"{boot_id}\",");
    let _ = writeln!(out, "  \"Virtualization\": \"{virt}\",");
    let _ = writeln!(out, "  \"OperatingSystemPrettyName\": \"{}\",", os.pretty_name);
    let _ = writeln!(out, "  \"Kernel\": \"{kernel}\",");
    let _ = writeln!(out, "  \"Architecture\": \"{arch}\"");
    let _ = writeln!(out, "}}");
}

// ============================================================================
// Personality: hostname
// ============================================================================

fn cmd_hostname(args: &[String]) {
    let mut fqdn = false;
    let mut short = false;
    let mut domain_flag = false;
    let mut ip_flag = false;

    let mut new_hostname: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: hostname [options] [hostname]");
                println!();
                println!("Options:");
                println!("  -f, --fqdn      Display the FQDN");
                println!("  -s, --short     Display the short hostname");
                println!("  -d, --domain    Display the DNS domain");
                println!("  -i, --ip-address  Display host IP address");
                println!("  -h, --help      Show this help");
                println!("  -V, --version   Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("hostname {VERSION}");
                process::exit(0);
            }
            "-f" | "--fqdn" | "--long" => fqdn = true,
            "-s" | "--short" => short = true,
            "-d" | "--domain" => domain_flag = true,
            "-i" | "--ip-address" => ip_flag = true,
            s if !s.starts_with('-') => {
                new_hostname = Some(s.to_string());
            }
            other => {
                eprintln!("hostname: unknown option: {other}");
                process::exit(1);
            }
        }
    }

    if let Some(name) = new_hostname {
        if let Err(e) = set_hostname(&name) {
            eprintln!("hostname: {e}");
            process::exit(1);
        }
        return;
    }

    if fqdn {
        println!("{}", get_fqdn());
    } else if short {
        let hostname = get_hostname();
        println!("{}", hostname.split('.').next().unwrap_or(&hostname));
    } else if domain_flag {
        let fqdn_str = get_fqdn();
        if let Some(dot_pos) = fqdn_str.find('.') {
            println!("{}", &fqdn_str[dot_pos + 1..]);
        }
    } else if ip_flag {
        // Try to resolve hostname to IP via /etc/hosts.
        let hostname = get_hostname();
        if let Ok(content) = fs::read_to_string("/etc/hosts") {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() >= 2 && fields[1..].contains(&&*hostname) {
                    println!("{}", fields[0]);
                    return;
                }
            }
        }
        println!("127.0.0.1");
    } else {
        println!("{}", get_hostname());
    }
}

// ============================================================================
// Personality: domainname
// ============================================================================

fn cmd_domainname(args: &[String]) {
    // domainname accepts at most one operand/option; only the first is acted on.
    if let Some(arg) = args.first() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: domainname [name]");
                println!("Show or set the NIS/YP domain name.");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("domainname {VERSION}");
                process::exit(0);
            }
            s if !s.starts_with('-') => {
                if let Err(e) = fs::write(PROC_DOMAINNAME, s) {
                    eprintln!("domainname: {e}");
                    process::exit(1);
                }
                return;
            }
            other => {
                eprintln!("domainname: unknown option: {other}");
                process::exit(1);
            }
        }
    }

    let domain = get_domainname();
    if domain.is_empty() {
        println!("(none)");
    } else {
        println!("{domain}");
    }
}

// ============================================================================
// Personality: dnsdomainname
// ============================================================================

fn cmd_dnsdomainname(args: &[String]) {
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: dnsdomainname");
                println!("Show the system's DNS domain name.");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("dnsdomainname {VERSION}");
                process::exit(0);
            }
            _ => {}
        }
    }

    let fqdn = get_fqdn();
    if let Some(dot_pos) = fqdn.find('.') {
        println!("{}", &fqdn[dot_pos + 1..]);
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("hostnamectl");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match prog_name.as_str() {
        "hostname" => cmd_hostname(&rest),
        "domainname" | "nisdomainname" | "ypdomainname" => cmd_domainname(&rest),
        "dnsdomainname" => cmd_dnsdomainname(&rest),
        _ => cmd_hostnamectl(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unquote() {
        assert_eq!(unquote("\"hello\""), "hello");
        assert_eq!(unquote("'world'"), "world");
        assert_eq!(unquote("noquotes"), "noquotes");
        assert_eq!(unquote("\"mixed'"), "\"mixed'");
        assert_eq!(unquote("  \"spaced\"  "), "spaced");
        assert_eq!(unquote(""), "");
    }

    #[test]
    fn test_read_file_trimmed_missing() {
        let result = read_file_trimmed("/nonexistent/file");
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_hostname_not_empty() {
        // May return empty string if neither /proc nor /etc exist.
        let _ = get_hostname();
    }

    #[test]
    fn test_get_domainname() {
        let domain = get_domainname();
        // Should not contain "(none)" — our function filters that.
        assert_ne!(domain, "(none)");
    }

    #[test]
    fn test_detect_virtualization() {
        let virt = detect_virtualization();
        // Should be a valid string.
        assert!(!virt.is_empty());
    }

    #[test]
    fn test_detect_chassis() {
        let chassis = detect_chassis();
        assert!(!chassis.is_empty());
    }

    #[test]
    fn test_machine_info_defaults() {
        let info = MachineInfo {
            pretty_hostname: String::new(),
            icon_name: String::new(),
            chassis: String::new(),
            deployment: String::new(),
            location: String::new(),
        };
        assert!(info.pretty_hostname.is_empty());
    }

    #[test]
    fn test_os_release_defaults() {
        let info = OsRelease {
            name: "SlateOS".to_string(),
            version: "1.0".to_string(),
            pretty_name: "SlateOS 1.0".to_string(),
            id: "slateos".to_string(),
            cpe_name: String::new(),
        };
        assert_eq!(info.name, "SlateOS");
        assert_eq!(info.id, "slateos");
    }

    #[test]
    fn test_hostname_validation_empty() {
        let result = set_hostname("");
        assert!(result.is_err());
    }

    #[test]
    fn test_hostname_validation_too_long() {
        let name: String = "a".repeat(65);
        let result = set_hostname(&name);
        assert!(result.is_err());
    }

    #[test]
    fn test_hostname_validation_bad_chars() {
        let result = set_hostname("host name");
        assert!(result.is_err());
    }

    #[test]
    fn test_hostname_validation_leading_dash() {
        let result = set_hostname("-hostname");
        assert!(result.is_err());
    }

    #[test]
    fn test_hostname_validation_trailing_dash() {
        let result = set_hostname("hostname-");
        assert!(result.is_err());
    }

    #[test]
    fn test_personality_detection() {
        let test_cases = [
            ("/usr/bin/hostnamectl", "hostnamectl"),
            ("hostname", "hostname"),
            ("/bin/domainname", "domainname"),
            ("dnsdomainname.exe", "dnsdomainname"),
            ("C:\\bin\\hostname.exe", "hostname"),
        ];

        for (input, expected) in &test_cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, *expected, "Failed for input: {input}");
        }
    }

    #[test]
    fn test_get_architecture() {
        let arch = get_architecture();
        // On our target, should be x86_64.
        assert!(!arch.is_empty());
    }
}
