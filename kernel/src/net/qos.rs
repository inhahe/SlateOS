//! QoS — Quality of Service and traffic classification.
//!
//! Provides traffic classification, prioritization, and basic
//! rate limiting for network flows.
//!
//! ## Features
//!
//! - Traffic classification by protocol, port, DSCP
//! - Priority queuing (8 priority levels)
//! - Per-class packet/byte counters
//! - DSCP marking (DiffServ Code Point)
//! - Token bucket rate limiter
//! - Bandwidth monitoring per class
//!
//! ## Traffic classes
//!
//! | Priority | Class           | Example traffic              |
//! |----------|-----------------|------------------------------|
//! | 7        | Network Control | routing protocols, ICMP      |
//! | 6        | Voice           | VoIP, SIP                    |
//! | 5        | Video           | streaming, video calls       |
//! | 4        | Interactive     | SSH, telnet, gaming          |
//! | 3        | Critical        | database, HTTPS              |
//! | 2        | Excellent       | HTTP, email                  |
//! | 1        | Background      | backups, updates             |
//! | 0        | Best Effort     | default, unclassified        |

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of QoS priority levels.
const NUM_PRIORITIES: usize = 8;

/// Maximum number of classification rules.
const MAX_RULES: usize = 32;

/// Maximum number of rate limiters.
const MAX_RATE_LIMITERS: usize = 8;

/// Token bucket refill interval (nanoseconds) — 100ms.
const TOKEN_REFILL_INTERVAL_NS: u64 = 100_000_000;

// ---------------------------------------------------------------------------
// DSCP values
// ---------------------------------------------------------------------------

/// DiffServ Code Point values (6 bits, in upper byte of TOS field).
#[allow(dead_code)] // Public API.
pub mod dscp {
    /// Default / Best Effort.
    pub const BE: u8 = 0;
    /// Assured Forwarding class 1, low drop.
    pub const AF11: u8 = 10;
    /// Assured Forwarding class 1, medium drop.
    pub const AF12: u8 = 12;
    /// Assured Forwarding class 1, high drop.
    pub const AF13: u8 = 14;
    /// Assured Forwarding class 2, low drop.
    pub const AF21: u8 = 18;
    /// Assured Forwarding class 2, medium drop.
    pub const AF22: u8 = 20;
    /// Assured Forwarding class 2, high drop.
    pub const AF23: u8 = 22;
    /// Assured Forwarding class 3, low drop.
    pub const AF31: u8 = 26;
    /// Assured Forwarding class 3, medium drop.
    pub const AF32: u8 = 28;
    /// Assured Forwarding class 3, high drop.
    pub const AF33: u8 = 30;
    /// Assured Forwarding class 4, low drop.
    pub const AF41: u8 = 34;
    /// Assured Forwarding class 4, medium drop.
    pub const AF42: u8 = 36;
    /// Assured Forwarding class 4, high drop.
    pub const AF43: u8 = 38;
    /// Expedited Forwarding (low latency, low jitter).
    pub const EF: u8 = 46;
    /// Class Selector 6 (network control).
    pub const CS6: u8 = 48;
    /// Class Selector 7 (network control).
    pub const CS7: u8 = 56;

    /// Get DSCP name.
    pub fn name(dscp: u8) -> &'static str {
        match dscp {
            0 => "BE (Best Effort)",
            10 => "AF11",
            12 => "AF12",
            14 => "AF13",
            18 => "AF21",
            20 => "AF22",
            22 => "AF23",
            26 => "AF31",
            28 => "AF32",
            30 => "AF33",
            34 => "AF41",
            36 => "AF42",
            38 => "AF43",
            46 => "EF (Expedited Forwarding)",
            48 => "CS6 (Network Control)",
            56 => "CS7 (Network Control)",
            _ => "Unknown",
        }
    }
}

// ---------------------------------------------------------------------------
// Traffic class
// ---------------------------------------------------------------------------

/// Traffic class with counters.
#[derive(Debug)]
struct TrafficClass {
    /// Human-readable name.
    name: &'static str,
    /// Total packets processed.
    packets: AtomicU64,
    /// Total bytes processed.
    bytes: AtomicU64,
    /// Packets dropped (rate-limited or deprioritized).
    drops: AtomicU64,
}

impl TrafficClass {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            packets: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            drops: AtomicU64::new(0),
        }
    }
}

/// Traffic class table (indexed by priority 0-7).
static CLASSES: [TrafficClass; NUM_PRIORITIES] = [
    TrafficClass::new("Best Effort"),
    TrafficClass::new("Background"),
    TrafficClass::new("Excellent Effort"),
    TrafficClass::new("Critical"),
    TrafficClass::new("Interactive"),
    TrafficClass::new("Video"),
    TrafficClass::new("Voice"),
    TrafficClass::new("Network Control"),
];

// ---------------------------------------------------------------------------
// Classification rules
// ---------------------------------------------------------------------------

/// Match criteria for a classification rule.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Public API.
pub enum ClassifyMatch {
    /// Match by IP protocol number (TCP=6, UDP=17, ICMP=1).
    Protocol(u8),
    /// Match by destination port.
    DstPort(u16),
    /// Match by source port.
    SrcPort(u16),
    /// Match by DSCP value.
    Dscp(u8),
    /// Match all traffic.
    All,
}

/// A classification rule: match → assign priority.
#[derive(Debug, Clone, Copy)]
struct ClassifyRule {
    active: bool,
    match_on: ClassifyMatch,
    /// Priority to assign (0-7).
    priority: u8,
}

impl ClassifyRule {
    const fn empty() -> Self {
        Self {
            active: false,
            match_on: ClassifyMatch::All,
            priority: 0,
        }
    }
}

/// Classification rule table.
static RULES: Mutex<[ClassifyRule; MAX_RULES]> = Mutex::new([const { ClassifyRule::empty() }; MAX_RULES]);

/// Whether QoS classification is enabled.
static QOS_ENABLED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Token bucket rate limiter
// ---------------------------------------------------------------------------

/// Token bucket state for rate limiting.
#[derive(Debug)]
struct TokenBucket {
    active: bool,
    /// Priority class this limiter applies to.
    priority: u8,
    /// Maximum tokens (bytes).
    burst_size: u64,
    /// Current tokens available.
    tokens: u64,
    /// Token refill rate (bytes per second).
    rate_bps: u64,
    /// Last refill timestamp (ns).
    last_refill_ns: u64,
    /// Total bytes passed.
    bytes_passed: u64,
    /// Total bytes dropped.
    bytes_dropped: u64,
}

impl TokenBucket {
    const fn empty() -> Self {
        Self {
            active: false,
            priority: 0,
            burst_size: 0,
            tokens: 0,
            rate_bps: 0,
            last_refill_ns: 0,
            bytes_passed: 0,
            bytes_dropped: 0,
        }
    }

    /// Refill tokens based on elapsed time.
    fn refill(&mut self, now_ns: u64) {
        if self.last_refill_ns == 0 {
            self.last_refill_ns = now_ns;
            return;
        }

        let elapsed_ns = now_ns.saturating_sub(self.last_refill_ns);
        if elapsed_ns < TOKEN_REFILL_INTERVAL_NS {
            return;
        }

        // tokens_to_add = rate_bps * elapsed_ns / 1_000_000_000.
        let new_tokens = self.rate_bps
            .saturating_mul(elapsed_ns)
            .checked_div(1_000_000_000)
            .unwrap_or(0);

        self.tokens = self.tokens.saturating_add(new_tokens).min(self.burst_size);
        self.last_refill_ns = now_ns;
    }

    /// Try to consume tokens. Returns true if allowed.
    fn consume(&mut self, bytes: u64, now_ns: u64) -> bool {
        self.refill(now_ns);
        if self.tokens >= bytes {
            self.tokens = self.tokens.saturating_sub(bytes);
            self.bytes_passed = self.bytes_passed.saturating_add(bytes);
            true
        } else {
            self.bytes_dropped = self.bytes_dropped.saturating_add(bytes);
            false
        }
    }
}

/// Rate limiter table.
static RATE_LIMITERS: Mutex<[TokenBucket; MAX_RATE_LIMITERS]> = Mutex::new(
    [const { TokenBucket::empty() }; MAX_RATE_LIMITERS]
);

// ---------------------------------------------------------------------------
// Public API — Classification
// ---------------------------------------------------------------------------

/// Enable or disable QoS classification.
#[allow(dead_code)] // Public API.
pub fn set_enabled(enabled: bool) {
    QOS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if QoS is enabled.
#[allow(dead_code)] // Public API.
pub fn is_enabled() -> bool {
    QOS_ENABLED.load(Ordering::Relaxed)
}

/// Add a classification rule.
#[allow(dead_code)] // Public API.
pub fn add_rule(match_on: ClassifyMatch, priority: u8) -> KernelResult<()> {
    if priority >= NUM_PRIORITIES as u8 {
        return Err(KernelError::InvalidArgument);
    }

    let mut rules = RULES.lock();
    for rule in rules.iter_mut() {
        if !rule.active {
            rule.active = true;
            rule.match_on = match_on;
            rule.priority = priority;
            return Ok(());
        }
    }

    Err(KernelError::OutOfMemory) // No slots.
}

/// Remove all classification rules.
#[allow(dead_code)] // Public API.
pub fn clear_rules() {
    let mut rules = RULES.lock();
    for rule in rules.iter_mut() {
        rule.active = false;
    }
}

/// Classify a packet and return its priority.
///
/// Returns priority 0-7 based on matching rules.
/// If no rule matches, returns 0 (Best Effort).
#[allow(dead_code)] // Public API.
pub fn classify(protocol: u8, src_port: u16, dst_port: u16, dscp_val: u8) -> u8 {
    if !QOS_ENABLED.load(Ordering::Relaxed) {
        return 0;
    }

    let rules = RULES.lock();
    for rule in rules.iter() {
        if !rule.active {
            continue;
        }
        let matches = match rule.match_on {
            ClassifyMatch::Protocol(p) => protocol == p,
            ClassifyMatch::DstPort(p) => dst_port == p,
            ClassifyMatch::SrcPort(p) => src_port == p,
            ClassifyMatch::Dscp(d) => dscp_val == d,
            ClassifyMatch::All => true,
        };
        if matches {
            return rule.priority;
        }
    }

    // Default classification by well-known ports.
    default_classify(protocol, dst_port)
}

/// Default classification based on well-known ports.
fn default_classify(protocol: u8, dst_port: u16) -> u8 {
    match (protocol, dst_port) {
        // ICMP — network control.
        (1, _) => 7,
        // DNS — interactive.
        (_, 53) => 4,
        // SSH — interactive.
        (_, 22) => 4,
        // HTTP/HTTPS — excellent effort.
        (_, 80) | (_, 443) => 2,
        // SMTP — background.
        (_, 25) | (_, 587) => 1,
        // NTP — network control.
        (_, 123) => 7,
        // SIP — voice.
        (_, 5060) | (_, 5061) => 6,
        // RTP range — video/voice.
        (17, p) if (16384..=32767).contains(&p) => 5,
        // Everything else.
        _ => 0,
    }
}

/// Record a packet for a given priority class.
#[allow(dead_code)] // Public API.
pub fn record_packet(priority: u8, size: usize) {
    let idx = (priority as usize).min(NUM_PRIORITIES - 1);
    CLASSES[idx].packets.fetch_add(1, Ordering::Relaxed);
    CLASSES[idx].bytes.fetch_add(size as u64, Ordering::Relaxed);
}

/// Record a dropped packet for a given priority class.
#[allow(dead_code)] // Public API.
pub fn record_drop(priority: u8) {
    let idx = (priority as usize).min(NUM_PRIORITIES - 1);
    CLASSES[idx].drops.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — Rate limiting
// ---------------------------------------------------------------------------

/// Add a rate limiter for a priority class.
///
/// `rate_bps`: bytes per second allowed.
/// `burst_bytes`: maximum burst size in bytes.
#[allow(dead_code)] // Public API.
pub fn add_rate_limit(priority: u8, rate_bps: u64, burst_bytes: u64) -> KernelResult<()> {
    if priority >= NUM_PRIORITIES as u8 {
        return Err(KernelError::InvalidArgument);
    }

    let mut limiters = RATE_LIMITERS.lock();
    for limiter in limiters.iter_mut() {
        if !limiter.active {
            limiter.active = true;
            limiter.priority = priority;
            limiter.rate_bps = rate_bps;
            limiter.burst_size = burst_bytes;
            limiter.tokens = burst_bytes;
            limiter.last_refill_ns = 0;
            limiter.bytes_passed = 0;
            limiter.bytes_dropped = 0;
            return Ok(());
        }
    }

    Err(KernelError::OutOfMemory)
}

/// Remove rate limiters for a priority class.
#[allow(dead_code)] // Public API.
pub fn remove_rate_limit(priority: u8) {
    let mut limiters = RATE_LIMITERS.lock();
    for limiter in limiters.iter_mut() {
        if limiter.active && limiter.priority == priority {
            limiter.active = false;
        }
    }
}

/// Check if a packet is allowed through the rate limiter.
///
/// Returns true if the packet should be forwarded.
#[allow(dead_code)] // Public API.
pub fn check_rate_limit(priority: u8, size: usize) -> bool {
    let now_ns = crate::hrtimer::now_ns();
    let mut limiters = RATE_LIMITERS.lock();

    for limiter in limiters.iter_mut() {
        if limiter.active && limiter.priority == priority {
            return limiter.consume(size as u64, now_ns);
        }
    }

    true // No limiter — allow.
}

// ---------------------------------------------------------------------------
// Statistics and display
// ---------------------------------------------------------------------------

/// Per-class QoS statistics.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct ClassStats {
    pub priority: u8,
    pub name: &'static str,
    pub packets: u64,
    pub bytes: u64,
    pub drops: u64,
}

/// Get statistics for all traffic classes.
#[allow(dead_code)] // Public API.
pub fn class_stats() -> Vec<ClassStats> {
    let mut result = Vec::with_capacity(NUM_PRIORITIES);
    for (i, class) in CLASSES.iter().enumerate() {
        result.push(ClassStats {
            priority: i as u8,
            name: class.name,
            packets: class.packets.load(Ordering::Relaxed),
            bytes: class.bytes.load(Ordering::Relaxed),
            drops: class.drops.load(Ordering::Relaxed),
        });
    }
    result
}

/// Rate limiter info.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API.
pub struct RateLimitInfo {
    pub priority: u8,
    pub rate_bps: u64,
    pub burst_bytes: u64,
    pub bytes_passed: u64,
    pub bytes_dropped: u64,
}

/// Get rate limiter info.
#[allow(dead_code)] // Public API.
pub fn rate_limit_info() -> Vec<RateLimitInfo> {
    let limiters = RATE_LIMITERS.lock();
    let mut result = Vec::new();
    for limiter in limiters.iter() {
        if limiter.active {
            result.push(RateLimitInfo {
                priority: limiter.priority,
                rate_bps: limiter.rate_bps,
                burst_bytes: limiter.burst_size,
                bytes_passed: limiter.bytes_passed,
                bytes_dropped: limiter.bytes_dropped,
            });
        }
    }
    result
}

/// List classification rules.
#[allow(dead_code)] // Public API.
pub fn list_rules() -> Vec<(ClassifyMatch, u8)> {
    let rules = RULES.lock();
    let mut result = Vec::new();
    for rule in rules.iter() {
        if rule.active {
            result.push((rule.match_on, rule.priority));
        }
    }
    result
}

/// Generate procfs content for `/proc/qos`.
#[allow(dead_code)] // Public API.
pub fn procfs_content() -> String {
    let enabled = is_enabled();
    let classes = class_stats();
    let limiters = rate_limit_info();

    let mut out = String::with_capacity(1024);
    out.push_str("QoS (Quality of Service)\n");
    out.push_str("========================\n\n");
    out.push_str(&format!("Status: {}\n\n", if enabled { "enabled" } else { "disabled" }));

    out.push_str("Traffic Classes:\n");
    for c in &classes {
        if c.packets > 0 || c.drops > 0 {
            out.push_str(&format!(
                "  [{}] {}: {} pkts, {} bytes, {} drops\n",
                c.priority, c.name, c.packets, c.bytes, c.drops,
            ));
        }
    }

    if !limiters.is_empty() {
        out.push_str("\nRate Limiters:\n");
        for l in &limiters {
            out.push_str(&format!(
                "  Priority {}: {} B/s (burst {}), passed {}, dropped {}\n",
                l.priority, l.rate_bps, l.burst_bytes, l.bytes_passed, l.bytes_dropped,
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run QoS self-tests.
#[allow(dead_code)] // Public API.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[qos] Running QoS self-tests...");
    let mut passed = 0u32;

    // --- Test 1: Default classification ---
    {
        // Without QoS enabled, should return 0.
        set_enabled(false);
        assert!(classify(6, 12345, 80, 0) == 0, "disabled = 0");

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 1 (disabled classification) PASSED");
    }

    // --- Test 2: Default port classification ---
    {
        set_enabled(true);
        // Default rules (no custom rules).
        let ssh = default_classify(6, 22);
        assert!(ssh == 4, "SSH = interactive");

        let http = default_classify(6, 80);
        assert!(http == 2, "HTTP = excellent effort");

        let icmp = default_classify(1, 0);
        assert!(icmp == 7, "ICMP = network control");

        let smtp = default_classify(6, 25);
        assert!(smtp == 1, "SMTP = background");

        let other = default_classify(6, 9999);
        assert!(other == 0, "unknown = best effort");

        set_enabled(false);

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 2 (default classification) PASSED");
    }

    // --- Test 3: Custom rules ---
    {
        clear_rules();
        set_enabled(true);

        // Add rule: port 8080 → priority 3.
        assert!(add_rule(ClassifyMatch::DstPort(8080), 3).is_ok(), "add rule");
        assert!(classify(6, 0, 8080, 0) == 3, "custom rule match");

        // Non-matching → default.
        assert!(classify(6, 0, 9090, 0) == 0, "non-match");

        clear_rules();
        set_enabled(false);

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 3 (custom rules) PASSED");
    }

    // --- Test 4: Invalid priority ---
    {
        assert!(add_rule(ClassifyMatch::All, 8).is_err(), "priority too high");
        assert!(add_rule(ClassifyMatch::All, 7).is_ok(), "max priority ok");
        clear_rules();

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 4 (invalid priority) PASSED");
    }

    // --- Test 5: Packet recording ---
    {
        record_packet(2, 100);
        record_packet(2, 200);
        record_drop(2);

        let stats = class_stats();
        let class2 = &stats[2];
        assert!(class2.packets >= 2, "recorded packets");
        assert!(class2.bytes >= 300, "recorded bytes");
        assert!(class2.drops >= 1, "recorded drops");

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 5 (packet recording) PASSED");
    }

    // --- Test 6: DSCP names ---
    {
        assert!(dscp::name(0).contains("Best Effort"), "BE");
        assert!(dscp::name(46).contains("Expedited"), "EF");
        assert!(dscp::name(48).contains("Control"), "CS6");
        assert!(dscp::name(99) == "Unknown", "unknown");

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 6 (DSCP names) PASSED");
    }

    // --- Test 7: Rate limiter ---
    {
        // Clean up any existing limiters for priority 5.
        remove_rate_limit(5);

        assert!(add_rate_limit(5, 1000, 5000).is_ok(), "add limiter");

        // Should allow first packet within burst.
        assert!(check_rate_limit(5, 100), "first packet allowed");

        // Get limiter info.
        let info = rate_limit_info();
        assert!(!info.is_empty(), "limiter active");

        remove_rate_limit(5);

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 7 (rate limiter) PASSED");
    }

    // --- Test 8: Rate limiter invalid ---
    {
        assert!(add_rate_limit(8, 1000, 1000).is_err(), "invalid priority");

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 8 (invalid rate limiter) PASSED");
    }

    // --- Test 9: List rules ---
    {
        clear_rules();
        // Ignore errors — test 9 verifies list_rules(); the rules are best-effort setup.
        add_rule(ClassifyMatch::DstPort(80), 2).ok();
        add_rule(ClassifyMatch::Protocol(1), 7).ok();

        let rules = list_rules();
        assert!(rules.len() == 2, "two rules");

        clear_rules();
        let rules = list_rules();
        assert!(rules.is_empty(), "cleared");

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 9 (list rules) PASSED");
    }

    // --- Test 10: Procfs content ---
    {
        let content = procfs_content();
        assert!(content.contains("QoS"), "header");
        assert!(content.contains("Status:"), "status");
        assert!(content.contains("Traffic Classes:"), "classes");

        passed = passed.saturating_add(1);
        crate::serial_println!("[qos]   test 10 (procfs content) PASSED");
    }

    crate::serial_println!("[qos] All {} self-tests PASSED", passed);
    Ok(())
}
