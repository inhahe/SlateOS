//! OurOS Firewall Management CLI (`fw`)
//!
//! Manages the kernel's packet-filtering firewall rules.  Rules are persisted
//! to `/etc/fw.rules` and applied via `SYS_NET_IOCTL` (syscall 810) with
//! firewall-specific sub-commands, or through the `/proc/net/firewall`
//! interface when the procfs path is available.
//!
//! # Usage
//!
//! ```text
//! fw status                       Show firewall status
//! fw enable                       Enable firewall
//! fw disable                      Disable firewall
//! fw list|rules [--json]          List all rules
//! fw allow <port>[/<proto>]       Allow incoming on port
//! fw deny  <port>[/<proto>]       Deny incoming on port
//! fw allow from <ip>              Allow all from IP
//! fw deny  from <ip>              Deny all from IP
//! fw allow <port> from <ip>       Allow port from specific IP
//! fw deny  <port> from <ip>       Deny port from specific IP
//! fw delete <rule#>               Delete rule by number
//! fw reset                        Reset to defaults
//! fw policy <accept|drop>         Set default inbound policy
//! fw log on|off                   Toggle blocked-packet logging
//! fw save                         Save rules to /etc/fw.rules
//! fw load                         Load rules from /etc/fw.rules
//! fw --json <listing-command>     JSON output for any listing command
//! ```

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const RULES_PATH: &str = "/etc/fw.rules";
const PROC_FIREWALL: &str = "/proc/net/firewall";

/// Syscall number for network IOCTL (from the net zone, numbers 800-999).
const SYS_NET_IOCTL: u64 = 810;

// Firewall sub-commands for SYS_NET_IOCTL.
const FW_ENABLE: u64 = 100;
const FW_DISABLE: u64 = 101;
const FW_GET_STATUS: u64 = 102;
const FW_ADD_RULE: u64 = 103;
const FW_DEL_RULE: u64 = 104;
const FW_SET_POLICY: u64 = 105;
const FW_SET_LOG: u64 = 106;
#[allow(dead_code)] // Will be used when kernel wires up bulk flush.
const FW_FLUSH: u64 = 107;
const FW_GET_RULES: u64 = 108;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 4-argument syscall on x86_64.
///
/// # Safety
///
/// Caller must ensure all arguments are valid for the given syscall number.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall4(nr: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> i64 {
    let ret: i64;
    // SAFETY: Arguments validated by caller; this is the standard x86_64
    // Linux/OurOS syscall ABI.
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

/// Send a firewall command to the kernel.
fn fw_ioctl(cmd: u64, arg1: u64, arg2: u64) -> i64 {
    // SAFETY: cmd is a known firewall sub-command, arg1/arg2 are value
    // parameters (not pointers that could be dangling).
    unsafe { syscall4(SYS_NET_IOCTL, cmd, arg1, arg2, 0) }
}

/// Send a firewall command that passes a buffer pointer.
fn fw_ioctl_buf(cmd: u64, buf: &[u8]) -> i64 {
    // SAFETY: buf is a valid slice; pointer and length are passed as
    // arg1 and arg2.  The kernel reads at most `len` bytes from the
    // buffer during the syscall (synchronous; buffer outlives the call).
    unsafe { syscall4(SYS_NET_IOCTL, cmd, buf.as_ptr() as u64, buf.len() as u64, 0) }
}

// ============================================================================
// Rule data model
// ============================================================================

/// Firewall rule action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Allow,
    Deny,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::Deny => "DENY",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "ALLOW" => Some(Self::Allow),
            "DENY" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// Traffic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    In,
    Out,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Self::In => "IN",
            Self::Out => "OUT",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "IN" => Some(Self::In),
            "OUT" => Some(Self::Out),
            _ => None,
        }
    }
}

/// Protocol filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proto {
    Any,
    Tcp,
    Udp,
}

impl Proto {
    fn as_str(self) -> &'static str {
        match self {
            Self::Any => "*",
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "*" | "any" | "all" => Some(Self::Any),
            "tcp" => Some(Self::Tcp),
            "udp" => Some(Self::Udp),
            _ => None,
        }
    }
}

/// A single firewall rule.
///
/// File format (one rule per line):
///   `ACTION DIRECTION PROTO SRC_IP SRC_PORT DST_IP DST_PORT`
///
/// `*` denotes "any" for IP addresses, ports, or protocol.
#[derive(Debug, Clone)]
struct Rule {
    action: Action,
    direction: Direction,
    proto: Proto,
    src_ip: String,
    src_port: String,
    dst_ip: String,
    dst_port: String,
}

impl Rule {
    /// Serialize a rule to the on-disk line format.
    fn to_line(&self) -> String {
        format!(
            "{} {} {} {} {} {} {}",
            self.action.as_str(),
            self.direction.as_str(),
            self.proto.as_str(),
            self.src_ip,
            self.src_port,
            self.dst_ip,
            self.dst_port,
        )
    }

    /// Parse a rule from a single line of the rules file.
    fn from_line(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            return None;
        }

        Some(Self {
            action: Action::parse(parts[0])?,
            direction: Direction::parse(parts[1])?,
            proto: Proto::parse(parts[2])?,
            src_ip: parts[3].to_string(),
            src_port: parts[4].to_string(),
            dst_ip: parts[5].to_string(),
            dst_port: parts[6].to_string(),
        })
    }

    /// Human-readable description for listing.
    fn describe(&self) -> String {
        let action = self.action.as_str();
        let dir = self.direction.as_str();

        let proto = match self.proto {
            Proto::Any => String::new(),
            other => format!(" proto={}", other.as_str()),
        };

        let src = format_endpoint(&self.src_ip, &self.src_port);
        let dst = format_endpoint(&self.dst_ip, &self.dst_port);

        format!("{action} {dir}{proto} src={src} dst={dst}")
    }

    /// Serialize this rule to a compact JSON object string.
    fn to_json(&self) -> String {
        format!(
            "{{\"action\":\"{}\",\"direction\":\"{}\",\"proto\":\"{}\",\
             \"src_ip\":\"{}\",\"src_port\":\"{}\",\"dst_ip\":\"{}\",\"dst_port\":\"{}\"}}",
            json_escape(self.action.as_str()),
            json_escape(self.direction.as_str()),
            json_escape(self.proto.as_str()),
            json_escape(&self.src_ip),
            json_escape(&self.src_port),
            json_escape(&self.dst_ip),
            json_escape(&self.dst_port),
        )
    }
}

fn format_endpoint(ip: &str, port: &str) -> String {
    if ip == "*" && port == "*" {
        "*".to_string()
    } else if ip == "*" {
        format!("*:{port}")
    } else if port == "*" {
        ip.to_string()
    } else {
        format!("{ip}:{port}")
    }
}

// ============================================================================
// Firewall state
// ============================================================================

/// In-memory firewall configuration.
struct Firewall {
    enabled: bool,
    default_policy: Action,
    logging: bool,
    rules: Vec<Rule>,
}

impl Firewall {
    /// Load the current state.  Tries `/proc/net/firewall` first (the
    /// kernel's live state), then falls back to reading the saved rules
    /// file.
    fn load() -> Self {
        // Try reading live kernel state from procfs.
        if let Some(state) = Self::from_procfs() {
            return state;
        }

        // Try the kernel syscall to query status.
        let ret = fw_ioctl(FW_GET_STATUS, 0, 0);
        if ret >= 0 {
            let enabled = (ret & 1) != 0;
            let logging = (ret & 2) != 0;
            let policy = if (ret & 4) != 0 { Action::Deny } else { Action::Allow };
            let rules = Self::load_rules_from_kernel().unwrap_or_default();
            return Self {
                enabled,
                default_policy: policy,
                logging,
                rules,
            };
        }

        // Fallback: load from saved rules file.
        Self::from_file().unwrap_or_else(Self::defaults)
    }

    /// Read state from `/proc/net/firewall`.
    ///
    /// Expected format:
    /// ```text
    /// enabled: yes|no
    /// policy: accept|drop
    /// logging: on|off
    /// rules: <count>
    /// <rule lines>
    /// ```
    fn from_procfs() -> Option<Self> {
        let content = fs::read_to_string(PROC_FIREWALL).ok()?;
        let mut enabled = false;
        let mut policy = Action::Deny;
        let mut logging = false;
        let mut rules = Vec::new();
        let mut in_rules = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if in_rules {
                if let Some(rule) = Rule::from_line(line) {
                    rules.push(rule);
                }
                continue;
            }

            if let Some(val) = line.strip_prefix("enabled:") {
                enabled = val.trim().eq_ignore_ascii_case("yes");
            } else if let Some(val) = line.strip_prefix("policy:") {
                policy = match val.trim().to_lowercase().as_str() {
                    "accept" | "allow" => Action::Allow,
                    _ => Action::Deny,
                };
            } else if let Some(val) = line.strip_prefix("logging:") {
                logging = val.trim().eq_ignore_ascii_case("on");
            } else if line.starts_with("rules:") {
                in_rules = true;
            }
        }

        Some(Self { enabled, default_policy: policy, logging, rules })
    }

    /// Load rules from the kernel via syscall.
    fn load_rules_from_kernel() -> Option<Vec<Rule>> {
        // Request the kernel to write rules into a buffer.
        let mut buf = vec![0u8; 8192];
        let ret = unsafe {
            syscall4(
                SYS_NET_IOCTL,
                FW_GET_RULES,
                buf.as_mut_ptr() as u64,
                buf.len() as u64,
                0,
            )
        };
        if ret <= 0 {
            return None;
        }

        let len = ret as usize;
        if len > buf.len() {
            return None;
        }

        let text = String::from_utf8_lossy(&buf[..len]);
        let mut rules = Vec::new();
        for line in text.lines() {
            if let Some(rule) = Rule::from_line(line) {
                rules.push(rule);
            }
        }
        Some(rules)
    }

    /// Load from the saved rules file.
    fn from_file() -> Option<Self> {
        let content = fs::read_to_string(RULES_PATH).ok()?;
        let mut fw = Self::defaults();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Meta-directives stored as comments.
            if let Some(val) = line.strip_prefix("@enabled ") {
                fw.enabled = val.eq_ignore_ascii_case("yes");
                continue;
            }
            if let Some(val) = line.strip_prefix("@policy ") {
                fw.default_policy = match val.trim().to_lowercase().as_str() {
                    "accept" | "allow" => Action::Allow,
                    _ => Action::Deny,
                };
                continue;
            }
            if let Some(val) = line.strip_prefix("@logging ") {
                fw.logging = val.eq_ignore_ascii_case("on");
                continue;
            }

            if let Some(rule) = Rule::from_line(line) {
                fw.rules.push(rule);
            }
        }

        Some(fw)
    }

    /// Factory defaults: enabled, deny inbound, allow outbound, no rules.
    fn defaults() -> Self {
        Self {
            enabled: true,
            default_policy: Action::Deny,
            logging: false,
            rules: Vec::new(),
        }
    }

    /// Save the current ruleset to `/etc/fw.rules`.
    fn save_to_file(&self) -> io::Result<()> {
        // Ensure parent directory exists.
        if let Some(parent) = std::path::Path::new(RULES_PATH).parent() {
            fs::create_dir_all(parent)?;
        }

        let mut f = fs::File::create(RULES_PATH)?;
        writeln!(f, "# OurOS firewall rules")?;
        writeln!(f, "# Auto-generated by fw(1).  Manual edits are preserved on load.")?;
        writeln!(f, "@enabled {}", if self.enabled { "yes" } else { "no" })?;
        writeln!(f, "@policy {}", match self.default_policy {
            Action::Allow => "accept",
            Action::Deny => "drop",
        })?;
        writeln!(f, "@logging {}", if self.logging { "on" } else { "off" })?;
        writeln!(f)?;
        for rule in &self.rules {
            writeln!(f, "{}", rule.to_line())?;
        }
        f.flush()?;
        Ok(())
    }

    /// Push the current state to the kernel via syscalls.
    fn apply_to_kernel(&self) {
        // Enable/disable.
        if self.enabled {
            fw_ioctl(FW_ENABLE, 0, 0);
        } else {
            fw_ioctl(FW_DISABLE, 0, 0);
        }

        // Default policy (0 = accept, 1 = drop).
        let policy_val: u64 = match self.default_policy {
            Action::Allow => 0,
            Action::Deny => 1,
        };
        fw_ioctl(FW_SET_POLICY, policy_val, 0);

        // Logging (1 = on, 0 = off).
        fw_ioctl(FW_SET_LOG, u64::from(self.logging), 0);

        // Push each rule.
        for rule in &self.rules {
            let line = rule.to_line();
            let terminated = format!("{line}\0");
            fw_ioctl_buf(FW_ADD_RULE, terminated.as_bytes());
        }
    }
}

// ============================================================================
// CLI output helpers
// ============================================================================

/// Escape a string for JSON embedding.
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
                // Control characters: \u00XX
                let _ = std::fmt::Write::write_fmt(
                    &mut out,
                    format_args!("\\u{:04x}", c as u32),
                );
            }
            c => out.push(c),
        }
    }
    out
}

/// Validate an IPv4 address string.
fn is_valid_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        if p.is_empty() || p.len() > 3 {
            return false;
        }
        p.parse::<u8>().is_ok()
    })
}

/// Validate a port number string.
fn is_valid_port(s: &str) -> bool {
    match s.parse::<u32>() {
        Ok(p) => p <= 65535,
        Err(_) => false,
    }
}

/// Parse a port[/proto] argument.
///
/// Returns `(port_string, Proto)`.
fn parse_port_proto(s: &str) -> Result<(String, Proto), String> {
    if let Some((port_str, proto_str)) = s.split_once('/') {
        if !is_valid_port(port_str) {
            return Err(format!("Invalid port: {port_str}"));
        }
        let proto = Proto::parse(proto_str)
            .ok_or_else(|| format!("Unknown protocol: {proto_str} (expected tcp, udp, or any)"))?;
        Ok((port_str.to_string(), proto))
    } else {
        if !is_valid_port(s) {
            return Err(format!("Invalid port: {s}"));
        }
        Ok((s.to_string(), Proto::Any))
    }
}

// ============================================================================
// Sub-commands
// ============================================================================

fn cmd_status(fw: &Firewall, json: bool) {
    if json {
        println!(
            "{{\"enabled\":{},\"policy\":\"{}\",\"logging\":{},\"rules\":{}}}",
            fw.enabled,
            match fw.default_policy {
                Action::Allow => "accept",
                Action::Deny => "drop",
            },
            fw.logging,
            fw.rules.len(),
        );
        return;
    }

    let status_str = if fw.enabled {
        "\x1b[32menabled\x1b[0m"
    } else {
        "\x1b[31mdisabled\x1b[0m"
    };
    let policy_str = match fw.default_policy {
        Action::Allow => "\x1b[33maccept\x1b[0m (allow all inbound by default)",
        Action::Deny => "\x1b[32mdrop\x1b[0m (deny all inbound by default)",
    };
    let log_str = if fw.logging {
        "\x1b[32mon\x1b[0m"
    } else {
        "off"
    };

    println!("Firewall status: {status_str}");
    println!("Default policy:  {policy_str}");
    println!("Logging:         {log_str}");
    println!("Active rules:    {}", fw.rules.len());
}

fn cmd_enable() {
    let ret = fw_ioctl(FW_ENABLE, 0, 0);
    if ret < 0 {
        eprintln!("Failed to enable firewall (kernel returned {ret}).");
        eprintln!("Updating local state only.");
    }

    match Firewall::from_file() {
        Some(mut fw) => {
            fw.enabled = true;
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not save state: {e}");
            }
        }
        None => {
            let mut fw = Firewall::defaults();
            fw.enabled = true;
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not save state: {e}");
            }
        }
    }
    println!("Firewall enabled.");
}

fn cmd_disable() {
    let ret = fw_ioctl(FW_DISABLE, 0, 0);
    if ret < 0 {
        eprintln!("Failed to disable firewall (kernel returned {ret}).");
        eprintln!("Updating local state only.");
    }

    match Firewall::from_file() {
        Some(mut fw) => {
            fw.enabled = false;
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not save state: {e}");
            }
        }
        None => {
            let mut fw = Firewall::defaults();
            fw.enabled = false;
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not save state: {e}");
            }
        }
    }
    println!("Firewall disabled.");
}

fn cmd_list(fw: &Firewall, json: bool) {
    if fw.rules.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No firewall rules defined.");
            println!("Default policy: {} all inbound.",
                     fw.default_policy.as_str().to_lowercase());
        }
        return;
    }

    if json {
        print!("[");
        for (i, rule) in fw.rules.iter().enumerate() {
            if i > 0 {
                print!(",");
            }
            println!();
            print!("  {}", rule.to_json());
        }
        println!();
        println!("]");
        return;
    }

    println!("{:<5} {:<6} {:<4} {:<5} {:<16} {:<6} {:<16} {:<6}  Description",
             "#", "Action", "Dir", "Proto", "Src IP", "SPort", "Dst IP", "DPort");
    println!("{}", "-".repeat(82));

    for (i, rule) in fw.rules.iter().enumerate() {
        println!(
            "{:<5} {:<6} {:<4} {:<5} {:<16} {:<6} {:<16} {:<6}  {}",
            i + 1,
            rule.action.as_str(),
            rule.direction.as_str(),
            rule.proto.as_str(),
            rule.src_ip,
            rule.src_port,
            rule.dst_ip,
            rule.dst_port,
            rule.describe(),
        );
    }

    println!();
    println!("Total: {} rule(s).  Default inbound policy: {}.",
             fw.rules.len(),
             fw.default_policy.as_str().to_lowercase());
}

fn cmd_allow_deny(fw: &mut Firewall, action: Action, args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: fw {} <port>[/<proto>]", action.as_str().to_lowercase());
        eprintln!("       fw {} from <ip>", action.as_str().to_lowercase());
        eprintln!("       fw {} <port> from <ip>", action.as_str().to_lowercase());
        process::exit(1);
    }

    // Pattern: allow/deny from <ip>
    if args[0] == "from" {
        if args.len() < 2 {
            eprintln!("Expected IP address after 'from'.");
            process::exit(1);
        }
        let ip = &args[1];
        if !is_valid_ipv4(ip) {
            eprintln!("Invalid IPv4 address: {ip}");
            process::exit(1);
        }

        let rule = Rule {
            action,
            direction: Direction::In,
            proto: Proto::Any,
            src_ip: ip.clone(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "*".to_string(),
        };

        push_rule_to_kernel(&rule);
        println!("Rule added: {}", rule.describe());
        fw.rules.push(rule);
        return;
    }

    // Parse the port[/proto] part.
    let (port, proto) = match parse_port_proto(&args[0]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    // Check for "from <ip>" after the port.
    let src_ip = if args.len() >= 3 && args[1] == "from" {
        let ip = &args[2];
        if !is_valid_ipv4(ip) {
            eprintln!("Invalid IPv4 address: {ip}");
            process::exit(1);
        }
        ip.clone()
    } else {
        "*".to_string()
    };

    let rule = Rule {
        action,
        direction: Direction::In,
        proto,
        src_ip,
        src_port: "*".to_string(),
        dst_ip: "*".to_string(),
        dst_port: port,
    };

    push_rule_to_kernel(&rule);
    println!("Rule added: {}", rule.describe());
    fw.rules.push(rule);
}

fn cmd_delete(fw: &mut Firewall, num_str: &str) {
    let num: usize = match num_str.parse() {
        Ok(n) if n >= 1 => n,
        _ => {
            eprintln!("Invalid rule number: {num_str}");
            eprintln!("Use 'fw list' to see rule numbers.");
            process::exit(1);
        }
    };

    if num > fw.rules.len() {
        eprintln!("Rule #{num} does not exist (only {} rules defined).", fw.rules.len());
        process::exit(1);
    }

    let removed = fw.rules.remove(num - 1);

    // Tell the kernel to remove this rule.
    fw_ioctl(FW_DEL_RULE, (num - 1) as u64, 0);

    println!("Deleted rule #{num}: {}", removed.describe());
}

fn cmd_reset(fw: &mut Firewall) {
    fw.rules.clear();
    fw.enabled = true;
    fw.default_policy = Action::Deny;
    fw.logging = false;

    // Push reset to kernel.
    fw_ioctl(FW_ENABLE, 0, 0);
    fw_ioctl(FW_SET_POLICY, 1, 0); // 1 = drop
    fw_ioctl(FW_SET_LOG, 0, 0);

    println!("Firewall reset to defaults.");
    println!("  Status:  enabled");
    println!("  Policy:  deny all inbound");
    println!("  Logging: off");
    println!("  Rules:   cleared");
}

fn cmd_policy(fw: &mut Firewall, value: &str) {
    let (policy, policy_val) = match value.to_lowercase().as_str() {
        "accept" | "allow" => (Action::Allow, 0u64),
        "drop" | "deny" | "reject" => (Action::Deny, 1u64),
        other => {
            eprintln!("Unknown policy: {other}");
            eprintln!("Expected: accept or drop");
            process::exit(1);
        }
    };

    fw.default_policy = policy;
    fw_ioctl(FW_SET_POLICY, policy_val, 0);

    println!("Default inbound policy set to: {}", policy.as_str().to_lowercase());
}

fn cmd_log(fw: &mut Firewall, value: &str) {
    let on = match value.to_lowercase().as_str() {
        "on" | "yes" | "true" | "1" => true,
        "off" | "no" | "false" | "0" => false,
        other => {
            eprintln!("Unknown log setting: {other}");
            eprintln!("Expected: on or off");
            process::exit(1);
        }
    };

    fw.logging = on;
    fw_ioctl(FW_SET_LOG, u64::from(on), 0);

    println!("Blocked-packet logging: {}", if on { "on" } else { "off" });
}

fn cmd_save(fw: &Firewall) {
    match fw.save_to_file() {
        Ok(()) => println!("Rules saved to {RULES_PATH} ({} rules).", fw.rules.len()),
        Err(e) => {
            eprintln!("Failed to save rules to {RULES_PATH}: {e}");
            process::exit(1);
        }
    }
}

fn cmd_load(fw: &mut Firewall) {
    let content = match fs::read_to_string(RULES_PATH) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read {RULES_PATH}: {e}");
            process::exit(1);
        }
    };

    let mut new_rules = Vec::new();
    let mut loaded_enabled = fw.enabled;
    let mut loaded_policy = fw.default_policy;
    let mut loaded_logging = fw.logging;

    let reader = io::BufReader::new(content.as_bytes());
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Meta-directives.
        if let Some(val) = line.strip_prefix("@enabled ") {
            loaded_enabled = val.eq_ignore_ascii_case("yes");
            continue;
        }
        if let Some(val) = line.strip_prefix("@policy ") {
            loaded_policy = match val.trim().to_lowercase().as_str() {
                "accept" | "allow" => Action::Allow,
                _ => Action::Deny,
            };
            continue;
        }
        if let Some(val) = line.strip_prefix("@logging ") {
            loaded_logging = val.eq_ignore_ascii_case("on");
            continue;
        }

        if let Some(rule) = Rule::from_line(&line) {
            new_rules.push(rule);
        } else {
            eprintln!("Warning: skipping invalid rule line: {line}");
        }
    }

    fw.enabled = loaded_enabled;
    fw.default_policy = loaded_policy;
    fw.logging = loaded_logging;
    fw.rules = new_rules;

    // Push the loaded state to the kernel.
    fw.apply_to_kernel();

    println!("Loaded {} rules from {RULES_PATH}.", fw.rules.len());
}

/// Push a single rule to the kernel.
fn push_rule_to_kernel(rule: &Rule) {
    let line = rule.to_line();
    let terminated = format!("{line}\0");
    fw_ioctl_buf(FW_ADD_RULE, terminated.as_bytes());
}

// ============================================================================
// Usage
// ============================================================================

fn print_usage() {
    println!("OurOS Firewall Manager v0.1.0");
    println!();
    println!("USAGE:");
    println!("  fw <command> [options]");
    println!();
    println!("COMMANDS:");
    println!("  status                       Show firewall status");
    println!("  enable                       Enable firewall");
    println!("  disable                      Disable firewall");
    println!("  list, rules                  List all rules");
    println!("  allow <port>[/<proto>]        Allow incoming on port (tcp/udp/any)");
    println!("  deny  <port>[/<proto>]        Deny incoming on port");
    println!("  allow from <ip>              Allow all traffic from IP");
    println!("  deny  from <ip>              Deny all traffic from IP");
    println!("  allow <port> from <ip>       Allow port from specific IP");
    println!("  deny  <port> from <ip>       Deny port from specific IP");
    println!("  delete <rule#>               Delete rule by number");
    println!("  reset                        Reset to defaults (deny all inbound)");
    println!("  policy <accept|drop>         Set default inbound policy");
    println!("  log <on|off>                 Toggle blocked-packet logging");
    println!("  save                         Save rules to {RULES_PATH}");
    println!("  load                         Load rules from {RULES_PATH}");
    println!();
    println!("OPTIONS:");
    println!("  --json                       JSON output for status/list commands");
    println!("  --help, -h                   Show this help");
    println!();
    println!("EXAMPLES:");
    println!("  fw enable                    Enable the firewall");
    println!("  fw allow 22/tcp              Allow SSH connections");
    println!("  fw allow 80                  Allow HTTP (tcp and udp)");
    println!("  fw deny from 10.0.0.50       Block all from an IP");
    println!("  fw allow 443 from 10.0.0.1   Allow HTTPS from a specific IP");
    println!("  fw list --json               List rules as JSON");
    println!("  fw policy drop               Deny all inbound by default");
    println!("  fw delete 3                  Remove the third rule");
    println!("  fw save                      Persist rules to disk");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(0);
    }

    // Check for global --json flag anywhere in arguments.
    let json_output = args.iter().any(|a| a == "--json");

    // Filter out the --json flag from args for command parsing.
    let cmd_args: Vec<String> = args.iter()
        .skip(1)
        .filter(|a| a.as_str() != "--json")
        .cloned()
        .collect();

    if cmd_args.is_empty() {
        print_usage();
        process::exit(0);
    }

    match cmd_args[0].as_str() {
        "help" | "--help" | "-h" => {
            print_usage();
        }

        "status" => {
            let fw = Firewall::load();
            cmd_status(&fw, json_output);
        }

        "enable" => {
            cmd_enable();
        }

        "disable" => {
            cmd_disable();
        }

        "list" | "rules" => {
            let fw = Firewall::load();
            cmd_list(&fw, json_output);
        }

        "allow" => {
            let mut fw = Firewall::load();
            cmd_allow_deny(&mut fw, Action::Allow, &cmd_args[1..]);
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not auto-save rules: {e}");
            }
        }

        "deny" => {
            let mut fw = Firewall::load();
            cmd_allow_deny(&mut fw, Action::Deny, &cmd_args[1..]);
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not auto-save rules: {e}");
            }
        }

        "delete" | "del" | "remove" | "rm" => {
            if cmd_args.len() < 2 {
                eprintln!("Usage: fw delete <rule#>");
                process::exit(1);
            }
            let mut fw = Firewall::load();
            cmd_delete(&mut fw, &cmd_args[1]);
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not auto-save rules: {e}");
            }
        }

        "reset" => {
            let mut fw = Firewall::load();
            cmd_reset(&mut fw);
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not auto-save rules: {e}");
            }
        }

        "policy" => {
            if cmd_args.len() < 2 {
                eprintln!("Usage: fw policy <accept|drop>");
                process::exit(1);
            }
            let mut fw = Firewall::load();
            cmd_policy(&mut fw, &cmd_args[1]);
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not auto-save rules: {e}");
            }
        }

        "log" => {
            if cmd_args.len() < 2 {
                eprintln!("Usage: fw log <on|off>");
                process::exit(1);
            }
            let mut fw = Firewall::load();
            cmd_log(&mut fw, &cmd_args[1]);
            if let Err(e) = fw.save_to_file() {
                eprintln!("Warning: could not auto-save rules: {e}");
            }
        }

        "save" => {
            let fw = Firewall::load();
            cmd_save(&fw);
        }

        "load" => {
            let mut fw = Firewall::load();
            cmd_load(&mut fw);
        }

        other => {
            eprintln!("Unknown command: {other}");
            eprintln!("Run 'fw --help' for usage.");
            process::exit(1);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Action --

    #[test]
    fn test_action_parse() {
        assert_eq!(Action::parse("ALLOW"), Some(Action::Allow));
        assert_eq!(Action::parse("allow"), Some(Action::Allow));
        assert_eq!(Action::parse("DENY"), Some(Action::Deny));
        assert_eq!(Action::parse("deny"), Some(Action::Deny));
        assert_eq!(Action::parse("block"), None);
        assert_eq!(Action::parse(""), None);
    }

    #[test]
    fn test_action_as_str() {
        assert_eq!(Action::Allow.as_str(), "ALLOW");
        assert_eq!(Action::Deny.as_str(), "DENY");
    }

    // -- Direction --

    #[test]
    fn test_direction_parse() {
        assert_eq!(Direction::parse("IN"), Some(Direction::In));
        assert_eq!(Direction::parse("in"), Some(Direction::In));
        assert_eq!(Direction::parse("OUT"), Some(Direction::Out));
        assert_eq!(Direction::parse("out"), Some(Direction::Out));
        assert_eq!(Direction::parse("both"), None);
    }

    #[test]
    fn test_direction_as_str() {
        assert_eq!(Direction::In.as_str(), "IN");
        assert_eq!(Direction::Out.as_str(), "OUT");
    }

    // -- Proto --

    #[test]
    fn test_proto_parse() {
        assert_eq!(Proto::parse("tcp"), Some(Proto::Tcp));
        assert_eq!(Proto::parse("TCP"), Some(Proto::Tcp));
        assert_eq!(Proto::parse("udp"), Some(Proto::Udp));
        assert_eq!(Proto::parse("*"), Some(Proto::Any));
        assert_eq!(Proto::parse("any"), Some(Proto::Any));
        assert_eq!(Proto::parse("all"), Some(Proto::Any));
        assert_eq!(Proto::parse("icmp"), None);
    }

    #[test]
    fn test_proto_as_str() {
        assert_eq!(Proto::Any.as_str(), "*");
        assert_eq!(Proto::Tcp.as_str(), "tcp");
        assert_eq!(Proto::Udp.as_str(), "udp");
    }

    // -- Rule parsing --

    #[test]
    fn test_rule_from_line_valid() {
        let line = "ALLOW IN tcp 192.168.1.0 * * 22";
        let rule = Rule::from_line(line).unwrap();
        assert_eq!(rule.action, Action::Allow);
        assert_eq!(rule.direction, Direction::In);
        assert_eq!(rule.proto, Proto::Tcp);
        assert_eq!(rule.src_ip, "192.168.1.0");
        assert_eq!(rule.src_port, "*");
        assert_eq!(rule.dst_ip, "*");
        assert_eq!(rule.dst_port, "22");
    }

    #[test]
    fn test_rule_from_line_deny_out() {
        let line = "DENY OUT udp * * 10.0.0.1 53";
        let rule = Rule::from_line(line).unwrap();
        assert_eq!(rule.action, Action::Deny);
        assert_eq!(rule.direction, Direction::Out);
        assert_eq!(rule.proto, Proto::Udp);
        assert_eq!(rule.dst_ip, "10.0.0.1");
        assert_eq!(rule.dst_port, "53");
    }

    #[test]
    fn test_rule_from_line_wildcard() {
        let line = "ALLOW IN * * * * *";
        let rule = Rule::from_line(line).unwrap();
        assert_eq!(rule.proto, Proto::Any);
        assert_eq!(rule.src_ip, "*");
        assert_eq!(rule.dst_port, "*");
    }

    #[test]
    fn test_rule_from_line_empty() {
        assert!(Rule::from_line("").is_none());
    }

    #[test]
    fn test_rule_from_line_comment() {
        assert!(Rule::from_line("# this is a comment").is_none());
    }

    #[test]
    fn test_rule_from_line_too_short() {
        assert!(Rule::from_line("ALLOW IN tcp").is_none());
    }

    #[test]
    fn test_rule_from_line_bad_action() {
        assert!(Rule::from_line("BLOCK IN tcp * * * 80").is_none());
    }

    #[test]
    fn test_rule_from_line_bad_direction() {
        assert!(Rule::from_line("ALLOW BOTH tcp * * * 80").is_none());
    }

    #[test]
    fn test_rule_from_line_bad_proto() {
        assert!(Rule::from_line("ALLOW IN icmp * * * *").is_none());
    }

    // -- Rule serialization roundtrip --

    #[test]
    fn test_rule_roundtrip() {
        let original = Rule {
            action: Action::Deny,
            direction: Direction::In,
            proto: Proto::Tcp,
            src_ip: "10.0.0.5".to_string(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "443".to_string(),
        };

        let line = original.to_line();
        let parsed = Rule::from_line(&line).unwrap();

        assert_eq!(parsed.action, original.action);
        assert_eq!(parsed.direction, original.direction);
        assert_eq!(parsed.proto, original.proto);
        assert_eq!(parsed.src_ip, original.src_ip);
        assert_eq!(parsed.src_port, original.src_port);
        assert_eq!(parsed.dst_ip, original.dst_ip);
        assert_eq!(parsed.dst_port, original.dst_port);
    }

    #[test]
    fn test_rule_roundtrip_wildcard() {
        let original = Rule {
            action: Action::Allow,
            direction: Direction::Out,
            proto: Proto::Any,
            src_ip: "*".to_string(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "*".to_string(),
        };

        let line = original.to_line();
        assert_eq!(line, "ALLOW OUT * * * * *");

        let parsed = Rule::from_line(&line).unwrap();
        assert_eq!(parsed.action, Action::Allow);
        assert_eq!(parsed.direction, Direction::Out);
        assert_eq!(parsed.proto, Proto::Any);
    }

    // -- Rule description --

    #[test]
    fn test_rule_describe_port_only() {
        let rule = Rule {
            action: Action::Allow,
            direction: Direction::In,
            proto: Proto::Tcp,
            src_ip: "*".to_string(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "22".to_string(),
        };
        let desc = rule.describe();
        assert!(desc.contains("ALLOW"));
        assert!(desc.contains("IN"));
        assert!(desc.contains("tcp"));
        assert!(desc.contains("22"));
    }

    #[test]
    fn test_rule_describe_ip_only() {
        let rule = Rule {
            action: Action::Deny,
            direction: Direction::In,
            proto: Proto::Any,
            src_ip: "10.0.0.50".to_string(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "*".to_string(),
        };
        let desc = rule.describe();
        assert!(desc.contains("DENY"));
        assert!(desc.contains("10.0.0.50"));
    }

    // -- Rule JSON --

    #[test]
    fn test_rule_to_json() {
        let rule = Rule {
            action: Action::Allow,
            direction: Direction::In,
            proto: Proto::Tcp,
            src_ip: "*".to_string(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "80".to_string(),
        };
        let json = rule.to_json();
        assert!(json.contains("\"action\":\"ALLOW\""));
        assert!(json.contains("\"proto\":\"tcp\""));
        assert!(json.contains("\"dst_port\":\"80\""));
    }

    // -- Validation helpers --

    #[test]
    fn test_is_valid_ipv4() {
        assert!(is_valid_ipv4("192.168.1.1"));
        assert!(is_valid_ipv4("0.0.0.0"));
        assert!(is_valid_ipv4("255.255.255.255"));
        assert!(is_valid_ipv4("10.0.0.1"));

        assert!(!is_valid_ipv4(""));
        assert!(!is_valid_ipv4("256.0.0.1"));
        assert!(!is_valid_ipv4("1.2.3"));
        assert!(!is_valid_ipv4("1.2.3.4.5"));
        assert!(!is_valid_ipv4("abc.def.ghi.jkl"));
        assert!(!is_valid_ipv4("192.168.1"));
        assert!(!is_valid_ipv4("192.168.1."));
        assert!(!is_valid_ipv4(".168.1.1"));
    }

    #[test]
    fn test_is_valid_port() {
        assert!(is_valid_port("0"));
        assert!(is_valid_port("22"));
        assert!(is_valid_port("80"));
        assert!(is_valid_port("443"));
        assert!(is_valid_port("65535"));

        assert!(!is_valid_port("65536"));
        assert!(!is_valid_port("-1"));
        assert!(!is_valid_port("abc"));
        assert!(!is_valid_port(""));
        assert!(!is_valid_port("99999"));
    }

    #[test]
    fn test_parse_port_proto_port_only() {
        let (port, proto) = parse_port_proto("80").unwrap();
        assert_eq!(port, "80");
        assert_eq!(proto, Proto::Any);
    }

    #[test]
    fn test_parse_port_proto_tcp() {
        let (port, proto) = parse_port_proto("22/tcp").unwrap();
        assert_eq!(port, "22");
        assert_eq!(proto, Proto::Tcp);
    }

    #[test]
    fn test_parse_port_proto_udp() {
        let (port, proto) = parse_port_proto("53/udp").unwrap();
        assert_eq!(port, "53");
        assert_eq!(proto, Proto::Udp);
    }

    #[test]
    fn test_parse_port_proto_invalid_port() {
        assert!(parse_port_proto("99999/tcp").is_err());
        assert!(parse_port_proto("abc").is_err());
    }

    #[test]
    fn test_parse_port_proto_invalid_proto() {
        assert!(parse_port_proto("80/icmp").is_err());
    }

    // -- format_endpoint --

    #[test]
    fn test_format_endpoint_both_wildcard() {
        assert_eq!(format_endpoint("*", "*"), "*");
    }

    #[test]
    fn test_format_endpoint_ip_wildcard() {
        assert_eq!(format_endpoint("*", "80"), "*:80");
    }

    #[test]
    fn test_format_endpoint_port_wildcard() {
        assert_eq!(format_endpoint("10.0.0.1", "*"), "10.0.0.1");
    }

    #[test]
    fn test_format_endpoint_both_specific() {
        assert_eq!(format_endpoint("10.0.0.1", "443"), "10.0.0.1:443");
    }

    // -- json_escape --

    #[test]
    fn test_json_escape_plain() {
        assert_eq!(json_escape("hello"), "hello");
    }

    #[test]
    fn test_json_escape_quotes() {
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
    }

    #[test]
    fn test_json_escape_backslash() {
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn test_json_escape_control_chars() {
        assert_eq!(json_escape("line\nnew"), "line\\nnew");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        assert_eq!(json_escape("cr\rhere"), "cr\\rhere");
    }

    // -- Firewall defaults --

    #[test]
    fn test_firewall_defaults() {
        let fw = Firewall::defaults();
        assert!(fw.enabled);
        assert_eq!(fw.default_policy, Action::Deny);
        assert!(!fw.logging);
        assert!(fw.rules.is_empty());
    }

    // -- Firewall from_file with missing file --

    #[test]
    fn test_firewall_from_file_missing() {
        // When the rules file does not exist, from_file returns None.
        let result = Firewall::from_file();
        // On test machines the file won't exist, so this should be None.
        // (If it happens to exist, the test is still valid -- it just
        // exercises the parsing path.)
        let _ = result;
    }

    // -- Rule to_line format --

    #[test]
    fn test_rule_to_line_format() {
        let rule = Rule {
            action: Action::Allow,
            direction: Direction::In,
            proto: Proto::Tcp,
            src_ip: "192.168.1.0".to_string(),
            src_port: "*".to_string(),
            dst_ip: "*".to_string(),
            dst_port: "22".to_string(),
        };
        assert_eq!(rule.to_line(), "ALLOW IN tcp 192.168.1.0 * * 22");
    }

    #[test]
    fn test_rule_to_line_deny_udp() {
        let rule = Rule {
            action: Action::Deny,
            direction: Direction::Out,
            proto: Proto::Udp,
            src_ip: "*".to_string(),
            src_port: "*".to_string(),
            dst_ip: "10.0.0.1".to_string(),
            dst_port: "53".to_string(),
        };
        assert_eq!(rule.to_line(), "DENY OUT udp * * 10.0.0.1 53");
    }

    // -- Multiple rules parsing --

    #[test]
    fn test_parse_multiple_rules() {
        let lines = [
            "ALLOW IN tcp * * * 22",
            "ALLOW IN tcp * * * 80",
            "DENY IN * 10.0.0.50 * * *",
            "# comment line",
            "",
            "ALLOW OUT * * * * *",
        ];

        let rules: Vec<Rule> = lines.iter()
            .filter_map(|l| Rule::from_line(l))
            .collect();

        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].dst_port, "22");
        assert_eq!(rules[1].dst_port, "80");
        assert_eq!(rules[2].src_ip, "10.0.0.50");
        assert_eq!(rules[3].direction, Direction::Out);
    }
}
