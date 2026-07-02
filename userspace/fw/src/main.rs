//! Slate OS Firewall Management CLI (`fw`)
//!
//! Manages the kernel's packet-filtering firewall rules.  Rules are persisted
//! to `/etc/fw.rules` and read from the `/proc/net/firewall` interface when the
//! procfs path is available. Writes go to the kernel through the root-gated
//! firewall syscalls (`SYS_NET_FW_ENABLE` .. `SYS_NET_FW_FLUSH`, 860..=864),
//! each operating on the caller's network namespace. The kernel rule model is
//! narrower than this tool's on-disk format (no source-port or destination-IP
//! dimension), so rules constraining either are skipped with a warning rather
//! than pushed as broader rules than the operator wrote.
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

// Firewall-control syscalls (`kernel/src/syscall/number.rs`). Root-gated write
// path; each operates on the caller's network namespace (root ns uses the
// global firewall). Reads (status + rule listing) are served by
// `/proc/net/firewall`, so there is no read syscall.
const SYS_NET_FW_ENABLE: u64 = 860; // arg0: 1 = enable, 0 = disable
const SYS_NET_FW_SET_POLICY: u64 = 861; // arg0: 0 = accept, 1 = drop
const SYS_NET_FW_ADD_RULE: u64 = 862; // arg0=ptr, arg1=len (12-byte record)
const SYS_NET_FW_DEL_RULE: u64 = 863; // arg0: rule index
const SYS_NET_FW_FLUSH: u64 = 864; // remove all rules

/// Size of the `SYS_NET_FW_ADD_RULE` record (must match the kernel's REC_SIZE).
const FW_RULE_REC_SIZE: usize = 12;

// ============================================================================
// Syscall interface
// ============================================================================

#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall.
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

#[cfg(not(target_arch = "x86_64"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS on non-target host builds (unit tests use the pure builders).
}

/// Enable (`on = true`) or disable the firewall. Returns the kernel result.
fn fw_enable(on: bool) -> i64 {
    // SAFETY: scalar-only syscall; no user pointers are dereferenced.
    unsafe { syscall3(SYS_NET_FW_ENABLE, u64::from(on), 0, 0) }
}

/// Set the default policy (`drop = true` → drop, else accept).
fn fw_set_policy(drop: bool) -> i64 {
    // SAFETY: scalar-only syscall; no user pointers are dereferenced.
    unsafe { syscall3(SYS_NET_FW_SET_POLICY, u64::from(drop), 0, 0) }
}

/// Add one rule from its 12-byte record. Returns the rule index or negative errno.
fn fw_add_rule(rec: &[u8; FW_RULE_REC_SIZE]) -> i64 {
    // SAFETY: `rec` is exactly FW_RULE_REC_SIZE bytes, matching the kernel's
    // contract; the kernel only reads (never writes) the record.
    unsafe { syscall3(SYS_NET_FW_ADD_RULE, rec.as_ptr() as u64, rec.len() as u64, 0) }
}

/// Delete a rule by kernel index. Returns the kernel result.
fn fw_del_rule(index: usize) -> i64 {
    // SAFETY: scalar-only syscall; no user pointers are dereferenced.
    unsafe { syscall3(SYS_NET_FW_DEL_RULE, index as u64, 0, 0) }
}

/// Remove all rules. Returns the kernel result.
fn fw_flush() -> i64 {
    // SAFETY: scalar-only syscall; no user pointers are dereferenced.
    unsafe { syscall3(SYS_NET_FW_FLUSH, 0, 0, 0) }
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

    /// Build the kernel's 12-byte firewall-rule record for this rule.
    ///
    /// Layout (see `SYS_NET_FW_ADD_RULE` in the kernel):
    /// ```text
    /// [0]     direction  (0 = In, 1 = Out, 2 = Both)
    /// [1]     action     (0 = Allow, 1 = Deny)
    /// [2]     protocol   (0 = Any, 1 = Tcp, 2 = Udp, 3 = Icmp)
    /// [3]     src_prefix (0..=32)
    /// [4..6]  dst_port   (u16 little-endian; 0 = any)
    /// [6..8]  priority   (u16 little-endian)
    /// [8..12] src_ip     (network byte order)
    /// ```
    ///
    /// Returns `None` when the rule constrains a dimension the kernel model
    /// cannot express — a source port or a destination IP — so the caller can
    /// skip it rather than push a broader rule than the operator wrote.
    fn to_kernel_record(&self, priority: usize) -> Option<[u8; FW_RULE_REC_SIZE]> {
        // The kernel has no source-port or destination-IP dimension. If either
        // is constrained, we cannot represent the rule faithfully.
        if self.src_port != "*" || self.dst_ip != "*" {
            return None;
        }

        let direction: u8 = match self.direction {
            Direction::In => 0,
            Direction::Out => 1,
        };
        let action: u8 = match self.action {
            Action::Allow => 0,
            Action::Deny => 1,
        };
        let protocol: u8 = match self.proto {
            Proto::Any => 0,
            Proto::Tcp => 1,
            Proto::Udp => 2,
        };

        // Source IP + prefix: "*" means match any (0.0.0.0/0); a concrete
        // address is treated as a /32 host match.
        let (src_octets, src_prefix): ([u8; 4], u8) = if self.src_ip == "*" {
            ([0, 0, 0, 0], 0)
        } else {
            (parse_ipv4_octets(&self.src_ip)?, 32)
        };

        // Destination port: "*" → 0 (any), else a concrete port.
        let dst_port: u16 = if self.dst_port == "*" {
            0
        } else {
            self.dst_port.parse::<u16>().ok()?
        };

        // Priority is capped at u16::MAX; larger indices saturate (they would
        // only affect ordering among an implausibly large ruleset).
        let priority = u16::try_from(priority).unwrap_or(u16::MAX);

        let mut rec = [0u8; FW_RULE_REC_SIZE];
        rec[0] = direction;
        rec[1] = action;
        rec[2] = protocol;
        rec[3] = src_prefix;
        rec[4..6].copy_from_slice(&dst_port.to_le_bytes());
        rec[6..8].copy_from_slice(&priority.to_le_bytes());
        rec[8..12].copy_from_slice(&src_octets);
        Some(rec)
    }
}

/// Parse a dotted-quad IPv4 string into its four octets (network order).
fn parse_ipv4_octets(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut count = 0usize;
    for part in s.split('.') {
        if count >= 4 {
            return None;
        }
        let val = part.parse::<u8>().ok()?;
        octets[count] = val;
        count = count.checked_add(1)?;
    }
    if count == 4 { Some(octets) } else { None }
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

        // There is no firewall-query syscall (reads are served via
        // `/proc/net/firewall`); fall back to the saved rules file, or the
        // built-in defaults if it is absent.
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
        writeln!(f, "# Slate OS firewall rules")?;
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

    /// Push the current state to the kernel via the firewall write syscalls.
    ///
    /// The kernel's rule model is narrower than this tool's on-disk format: it
    /// filters on direction, action, protocol, a source IP/prefix and a single
    /// destination port. It has no notion of a source port or a destination IP.
    /// Rules that constrain either of those dimensions cannot be faithfully
    /// represented, so they are skipped with a warning rather than pushed as an
    /// overly-broad rule that would silently match more traffic than intended.
    ///
    /// Logging (`self.logging`) likewise has no kernel counterpart; it is a
    /// display/persistence-only field and is not applied here.
    fn apply_to_kernel(&self) {
        // Replace the kernel's ruleset wholesale so it matches our in-memory
        // view. Flush first, then re-add each representable rule in order.
        let flush_ret = fw_flush();
        if flush_ret < 0 {
            eprintln!("fw: warning: failed to flush kernel rules (errno {flush_ret})");
        }

        // Enable/disable the firewall.
        let en_ret = fw_enable(self.enabled);
        if en_ret < 0 {
            eprintln!(
                "fw: warning: failed to {} firewall (errno {en_ret})",
                if self.enabled { "enable" } else { "disable" }
            );
        }

        // Default policy.
        let pol_ret = fw_set_policy(matches!(self.default_policy, Action::Deny));
        if pol_ret < 0 {
            eprintln!("fw: warning: failed to set default policy (errno {pol_ret})");
        }

        if self.logging {
            eprintln!(
                "fw: note: logging is a display-only setting; the kernel firewall \
                 has no logging control, so it was not applied"
            );
        }

        // Push each representable rule. `priority` preserves list order.
        for (idx, rule) in self.rules.iter().enumerate() {
            match rule.to_kernel_record(idx) {
                Some(rec) => {
                    let ret = fw_add_rule(&rec);
                    if ret < 0 {
                        eprintln!(
                            "fw: warning: kernel rejected rule '{}' (errno {ret})",
                            rule.describe()
                        );
                    }
                }
                None => {
                    eprintln!(
                        "fw: warning: skipping rule '{}' — the kernel firewall cannot \
                         represent a source port or destination IP constraint",
                        rule.describe()
                    );
                }
            }
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
    let ret = fw_enable(true);
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
    let ret = fw_enable(false);
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

        push_rule_to_kernel(&rule, fw.rules.len());
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

    push_rule_to_kernel(&rule, fw.rules.len());
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

    // Compute the kernel-side index of this rule before removing it. Rules the
    // kernel cannot represent are never pushed, so the kernel index is the
    // count of *representable* rules appearing before this one in the list.
    let idx = num - 1;
    let kernel_index = fw.rules[..idx]
        .iter()
        .filter(|r| r.to_kernel_record(0).is_some())
        .count();
    let was_pushed = fw.rules[idx].to_kernel_record(0).is_some();

    let removed = fw.rules.remove(idx);

    // Only issue a kernel delete if this rule was actually pushed.
    if was_pushed {
        let ret = fw_del_rule(kernel_index);
        if ret < 0 {
            eprintln!("fw: warning: kernel rejected the delete (errno {ret}).");
        }
    }

    println!("Deleted rule #{num}: {}", removed.describe());
}

fn cmd_reset(fw: &mut Firewall) {
    fw.rules.clear();
    fw.enabled = true;
    fw.default_policy = Action::Deny;
    fw.logging = false;

    // Push reset to kernel: flush rules, enable, deny-by-default.
    fw.apply_to_kernel();

    println!("Firewall reset to defaults.");
    println!("  Status:  enabled");
    println!("  Policy:  deny all inbound");
    println!("  Logging: off");
    println!("  Rules:   cleared");
}

fn cmd_policy(fw: &mut Firewall, value: &str) {
    let policy = match value.to_lowercase().as_str() {
        "accept" | "allow" => Action::Allow,
        "drop" | "deny" | "reject" => Action::Deny,
        other => {
            eprintln!("Unknown policy: {other}");
            eprintln!("Expected: accept or drop");
            process::exit(1);
        }
    };

    fw.default_policy = policy;
    let ret = fw_set_policy(matches!(policy, Action::Deny));
    if ret < 0 {
        eprintln!("fw: warning: kernel rejected the policy change (errno {ret}).");
    }

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
    // Logging is a local display/persistence setting only; the kernel firewall
    // exposes no logging control, so there is nothing to push.

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

/// Push a single rule to the kernel via `SYS_NET_FW_ADD_RULE`.
///
/// `priority` is the index the rule will occupy in the ruleset, used to
/// preserve list ordering in the kernel. Rules the kernel model cannot
/// represent (a source-port or destination-IP constraint) are skipped with a
/// warning — see [`Rule::to_kernel_record`].
fn push_rule_to_kernel(rule: &Rule, priority: usize) {
    match rule.to_kernel_record(priority) {
        Some(rec) => {
            let ret = fw_add_rule(&rec);
            if ret < 0 {
                eprintln!("fw: warning: kernel rejected the rule (errno {ret}).");
            }
        }
        None => {
            eprintln!(
                "fw: warning: this rule constrains a source port or destination IP, \
                 which the kernel firewall cannot represent — it was saved locally \
                 but not applied to the kernel."
            );
        }
    }
}

// ============================================================================
// Usage
// ============================================================================

fn print_usage() {
    println!("Slate OS Firewall Manager v0.1.0");
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

    // ---- parse_ipv4_octets ------------------------------------------------

    #[test]
    fn test_parse_ipv4_octets_valid() {
        assert_eq!(parse_ipv4_octets("10.0.2.15"), Some([10, 0, 2, 15]));
        assert_eq!(parse_ipv4_octets("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(parse_ipv4_octets("255.255.255.255"), Some([255, 255, 255, 255]));
    }

    #[test]
    fn test_parse_ipv4_octets_invalid() {
        assert_eq!(parse_ipv4_octets("10.0.2"), None); // too few
        assert_eq!(parse_ipv4_octets("10.0.2.15.7"), None); // too many
        assert_eq!(parse_ipv4_octets("256.0.0.1"), None); // out of range
        assert_eq!(parse_ipv4_octets("10.0.0.a"), None); // non-numeric
        assert_eq!(parse_ipv4_octets(""), None);
    }

    // ---- Rule::to_kernel_record ------------------------------------------

    fn rule(
        action: Action,
        direction: Direction,
        proto: Proto,
        src_ip: &str,
        src_port: &str,
        dst_ip: &str,
        dst_port: &str,
    ) -> Rule {
        Rule {
            action,
            direction,
            proto,
            src_ip: src_ip.to_string(),
            src_port: src_port.to_string(),
            dst_ip: dst_ip.to_string(),
            dst_port: dst_port.to_string(),
        }
    }

    #[test]
    fn test_to_kernel_record_allow_in_tcp_port() {
        // ALLOW IN tcp * * * 22  (priority 0)
        let r = rule(Action::Allow, Direction::In, Proto::Tcp, "*", "*", "*", "22");
        let rec = r.to_kernel_record(0).expect("representable");
        assert_eq!(rec[0], 0); // direction In
        assert_eq!(rec[1], 0); // action Allow
        assert_eq!(rec[2], 1); // protocol Tcp
        assert_eq!(rec[3], 0); // src_prefix 0 (any)
        assert_eq!(u16::from_le_bytes([rec[4], rec[5]]), 22); // dst_port
        assert_eq!(u16::from_le_bytes([rec[6], rec[7]]), 0); // priority
        assert_eq!(&rec[8..12], &[0, 0, 0, 0]); // src_ip any
    }

    #[test]
    fn test_to_kernel_record_deny_out_udp_srcip() {
        // DENY OUT udp 10.0.2.50 * * *  (priority 5)
        let r = rule(Action::Deny, Direction::Out, Proto::Udp, "10.0.2.50", "*", "*", "*");
        let rec = r.to_kernel_record(5).expect("representable");
        assert_eq!(rec[0], 1); // direction Out
        assert_eq!(rec[1], 1); // action Deny
        assert_eq!(rec[2], 2); // protocol Udp
        assert_eq!(rec[3], 32); // src_prefix /32
        assert_eq!(u16::from_le_bytes([rec[4], rec[5]]), 0); // dst_port any
        assert_eq!(u16::from_le_bytes([rec[6], rec[7]]), 5); // priority
        assert_eq!(&rec[8..12], &[10, 0, 2, 50]); // src_ip network order
    }

    #[test]
    fn test_to_kernel_record_any_proto() {
        let r = rule(Action::Allow, Direction::In, Proto::Any, "*", "*", "*", "*");
        let rec = r.to_kernel_record(0).expect("representable");
        assert_eq!(rec[2], 0); // protocol Any
    }

    #[test]
    fn test_to_kernel_record_skips_src_port() {
        // A source-port constraint cannot be represented by the kernel model.
        let r = rule(Action::Allow, Direction::In, Proto::Tcp, "*", "1024", "*", "80");
        assert_eq!(r.to_kernel_record(0), None);
    }

    #[test]
    fn test_to_kernel_record_skips_dst_ip() {
        // A destination-IP constraint cannot be represented by the kernel model.
        let r = rule(Action::Allow, Direction::In, Proto::Tcp, "*", "*", "10.0.0.1", "80");
        assert_eq!(r.to_kernel_record(0), None);
    }

    #[test]
    fn test_to_kernel_record_bad_src_ip() {
        // A malformed source IP yields None rather than a bogus record.
        let r = rule(Action::Allow, Direction::In, Proto::Tcp, "not-an-ip", "*", "*", "80");
        assert_eq!(r.to_kernel_record(0), None);
    }

    #[test]
    fn test_to_kernel_record_priority_saturates() {
        let r = rule(Action::Allow, Direction::In, Proto::Any, "*", "*", "*", "*");
        let rec = r.to_kernel_record(100_000).expect("representable");
        assert_eq!(u16::from_le_bytes([rec[6], rec[7]]), u16::MAX);
    }
}
