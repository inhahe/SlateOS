//! ntpd / ntpdate / sntp -- NTP time synchronization for OurOS.
//!
//! Multi-personality binary: behaves as `ntpdate` or `sntp` when invoked under
//! those names (one-shot query + set), or as `ntpd` (continuous daemon) when
//! invoked as anything else.
//!
//! Implements the NTP client protocol per RFC 5905 (NTPv4) over UDP port 123.

use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::net::UdpSocket;
use std::process;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Seconds between NTP epoch (1900-01-01) and Unix epoch (1970-01-01).
const NTP_UNIX_DELTA: u64 = 2_208_988_800;

/// NTP packet size (48 bytes, RFC 5905 section 7.3).
const NTP_PACKET_LEN: usize = 48;

/// Default UDP port for NTP.
const NTP_PORT: u16 = 123;

/// Default number of samples per server in ntpdate mode.
const DEFAULT_SAMPLES: u32 = 4;

/// Default query timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 5;

/// Step threshold: step the clock if absolute offset exceeds 128 ms.
const STEP_THRESHOLD_US: i64 = 128_000;

/// Panic threshold: refuse to set time if absolute offset exceeds 1000 seconds.
const PANIC_THRESHOLD_US: i64 = 1_000_000_000;

/// Minimum poll interval in seconds (2^6 = 64).
const MIN_POLL_INTERVAL: u32 = 64;

/// Maximum poll interval in seconds (2^10 = 1024).
const MAX_POLL_INTERVAL: u32 = 1024;

/// Default NTP servers used when none are specified.
const DEFAULT_SERVERS: &[&str] = &[
    "pool.ntp.org",
    "time.google.com",
    "time.cloudflare.com",
];

/// Default configuration file path.
const DEFAULT_CONFIG_PATH: &str = "/etc/ntp.conf";

/// Default drift file path.
const DEFAULT_DRIFT_PATH: &str = "/var/lib/ntp/drift";

// ---------------------------------------------------------------------------
// NTP Packet
// ---------------------------------------------------------------------------

/// NTP packet (48 bytes) per RFC 5905.
///
/// Layout:
/// ```text
///  0               1               2               3
///  0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |LI | VN  |Mode | Stratum     | Poll          | Precision       |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Root Delay (32 bits)                          |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Root Dispersion (32 bits)                     |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Reference ID (32 bits)                        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Reference Timestamp (64 bits)                 |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Origin Timestamp (64 bits)                    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Receive Timestamp (64 bits)                   |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                   Transmit Timestamp (64 bits)                  |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[derive(Clone, Debug)]
struct NtpPacket {
    /// Leap Indicator (2 bits).
    li: u8,
    /// Version Number (3 bits).
    vn: u8,
    /// Mode (3 bits): 3 = client, 4 = server.
    mode: u8,
    /// Stratum (0 = unspecified/KoD, 1 = primary, 2-15 = secondary).
    stratum: u8,
    /// Poll interval (log2 seconds).
    poll: i8,
    /// Precision (log2 seconds).
    precision: i8,
    /// Root delay (NTP short format, 32 bits).
    root_delay: u32,
    /// Root dispersion (NTP short format, 32 bits).
    root_dispersion: u32,
    /// Reference ID (4 bytes: stratum-1 = ASCII, else IP).
    ref_id: [u8; 4],
    /// Reference timestamp.
    ref_ts: NtpTimestamp,
    /// Origin timestamp (T1 in the client, copied from transmit of request).
    origin_ts: NtpTimestamp,
    /// Receive timestamp (T2 at the server).
    receive_ts: NtpTimestamp,
    /// Transmit timestamp (T3 at the server).
    transmit_ts: NtpTimestamp,
}

/// 64-bit NTP timestamp: seconds since 1900-01-01 (32 bits) + fraction (32 bits).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct NtpTimestamp {
    seconds: u32,
    fraction: u32,
}

impl NtpTimestamp {
    /// Create from the two 32-bit halves.
    fn new(seconds: u32, fraction: u32) -> Self {
        Self { seconds, fraction }
    }

    /// Create from a Unix timestamp (seconds + microsecond fraction).
    fn from_unix(unix_secs: u64, micro_frac: u32) -> Self {
        let ntp_secs = unix_secs.saturating_add(NTP_UNIX_DELTA);
        // Clamp to u32 range (NTP era 0 wraps at 2036).
        let seconds = if ntp_secs > u64::from(u32::MAX) {
            u32::MAX
        } else {
            ntp_secs as u32
        };
        // Convert microseconds to NTP fraction: frac = micro * 2^32 / 1_000_000.
        let fraction = ((u64::from(micro_frac) << 32) / 1_000_000) as u32;
        Self { seconds, fraction }
    }

    /// Convert to Unix seconds (returns None if before Unix epoch).
    fn to_unix_secs(self) -> Option<u64> {
        let ntp = u64::from(self.seconds);
        if ntp < NTP_UNIX_DELTA {
            return None;
        }
        Some(ntp - NTP_UNIX_DELTA)
    }

    /// Convert to a floating-point seconds value for arithmetic.
    fn to_f64(self) -> f64 {
        f64::from(self.seconds) + f64::from(self.fraction) / 4_294_967_296.0
    }

    /// Build from a floating-point seconds value.
    fn from_f64(val: f64) -> Self {
        let seconds = val as u32;
        let fraction = ((val - f64::from(seconds)) * 4_294_967_296.0) as u32;
        Self { seconds, fraction }
    }

    /// True if this is a zero timestamp.
    fn is_zero(self) -> bool {
        self.seconds == 0 && self.fraction == 0
    }
}

impl NtpPacket {
    /// Create a client-mode request packet (NTPv4, mode 3).
    fn new_client_request(transmit_ts: NtpTimestamp) -> Self {
        Self {
            li: 0,
            vn: 4,
            mode: 3,
            stratum: 0,
            poll: 0,
            precision: 0,
            root_delay: 0,
            root_dispersion: 0,
            ref_id: [0; 4],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts,
        }
    }

    /// Serialize to a 48-byte buffer.
    fn to_bytes(&self) -> [u8; NTP_PACKET_LEN] {
        let mut buf = [0u8; NTP_PACKET_LEN];
        // Byte 0: LI (2) | VN (3) | Mode (3).
        buf[0] = (self.li << 6) | (self.vn << 3) | self.mode;
        buf[1] = self.stratum;
        buf[2] = self.poll as u8;
        buf[3] = self.precision as u8;
        buf[4..8].copy_from_slice(&self.root_delay.to_be_bytes());
        buf[8..12].copy_from_slice(&self.root_dispersion.to_be_bytes());
        buf[12..16].copy_from_slice(&self.ref_id);
        Self::write_ts(&mut buf[16..24], &self.ref_ts);
        Self::write_ts(&mut buf[24..32], &self.origin_ts);
        Self::write_ts(&mut buf[32..40], &self.receive_ts);
        Self::write_ts(&mut buf[40..48], &self.transmit_ts);
        buf
    }

    /// Parse a 48-byte buffer into an NTP packet.
    fn from_bytes(buf: &[u8; NTP_PACKET_LEN]) -> Self {
        let flags = buf[0];
        Self {
            li: (flags >> 6) & 0x03,
            vn: (flags >> 3) & 0x07,
            mode: flags & 0x07,
            stratum: buf[1],
            poll: buf[2] as i8,
            precision: buf[3] as i8,
            root_delay: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            root_dispersion: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            ref_id: [buf[12], buf[13], buf[14], buf[15]],
            ref_ts: Self::read_ts(&buf[16..24]),
            origin_ts: Self::read_ts(&buf[24..32]),
            receive_ts: Self::read_ts(&buf[32..40]),
            transmit_ts: Self::read_ts(&buf[40..48]),
        }
    }

    /// Read an NTP timestamp from an 8-byte slice.
    fn read_ts(data: &[u8]) -> NtpTimestamp {
        let seconds = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let fraction = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        NtpTimestamp { seconds, fraction }
    }

    /// Write an NTP timestamp into an 8-byte slice.
    fn write_ts(dest: &mut [u8], ts: &NtpTimestamp) {
        dest[0..4].copy_from_slice(&ts.seconds.to_be_bytes());
        dest[4..8].copy_from_slice(&ts.fraction.to_be_bytes());
    }

    /// True if the server response indicates Kiss-of-Death.
    ///
    /// KoD: stratum == 0 and ref_id is an ASCII code like "DENY", "RATE", etc.
    fn is_kod(&self) -> bool {
        self.stratum == 0 && self.ref_id.iter().all(|&b| b.is_ascii_graphic() || b == b' ')
    }

    /// Return the KoD code as a string (if this is a KoD packet).
    fn kod_code(&self) -> Option<String> {
        if self.is_kod() {
            let code: String = self
                .ref_id
                .iter()
                .map(|&b| if b.is_ascii_graphic() { b as char } else { ' ' })
                .collect();
            Some(code.trim().to_string())
        } else {
            None
        }
    }

    /// Validate that this is a plausible server response.
    fn validate_response(&self) -> Result<(), String> {
        if self.mode != 4 {
            return Err(format!("unexpected mode {} (expected 4 = server)", self.mode));
        }
        if self.stratum > 15 && self.stratum != 0 {
            return Err(format!("invalid stratum: {}", self.stratum));
        }
        if let Some(code) = self.kod_code() {
            return Err(format!("Kiss-of-Death from server: {code}"));
        }
        if self.transmit_ts.is_zero() {
            return Err("server transmit timestamp is zero".to_string());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Clock offset / delay calculation
// ---------------------------------------------------------------------------

/// Result of a single NTP query exchange.
#[derive(Clone, Debug)]
struct NtpSample {
    /// Clock offset in microseconds: ((T2 - T1) + (T3 - T4)) / 2.
    offset_us: i64,
    /// Round-trip delay in microseconds: (T4 - T1) - (T3 - T2).
    delay_us: i64,
    /// Server stratum.
    stratum: u8,
}

/// Compute clock offset and round-trip delay from the four NTP timestamps.
///
/// - T1: client transmit time (origin timestamp in request).
/// - T2: server receive time.
/// - T3: server transmit time.
/// - T4: client receive time.
///
/// offset = ((T2 - T1) + (T3 - T4)) / 2
/// delay  = (T4 - T1) - (T3 - T2)
fn compute_offset_delay(
    t1: NtpTimestamp,
    t2: NtpTimestamp,
    t3: NtpTimestamp,
    t4: NtpTimestamp,
) -> NtpSample {
    let t1f = t1.to_f64();
    let t2f = t2.to_f64();
    let t3f = t3.to_f64();
    let t4f = t4.to_f64();

    let offset = ((t2f - t1f) + (t3f - t4f)) / 2.0;
    let delay = (t4f - t1f) - (t3f - t2f);

    // Convert to microseconds.
    let offset_us = (offset * 1_000_000.0) as i64;
    let delay_us = (delay * 1_000_000.0) as i64;

    NtpSample {
        offset_us,
        delay_us,
        stratum: 0, // filled in by caller
    }
}

// ---------------------------------------------------------------------------
// Clock filter (select best sample from multiple queries)
// ---------------------------------------------------------------------------

/// Select the best sample from a set using a simple median-of-offsets,
/// minimum-delay heuristic.
fn clock_filter(samples: &mut [NtpSample]) -> Option<NtpSample> {
    if samples.is_empty() {
        return None;
    }
    if samples.len() == 1 {
        return Some(samples[0].clone());
    }

    // Sort by delay ascending -- the sample with the least network asymmetry
    // is most likely to have the most accurate offset.
    samples.sort_by(|a, b| a.delay_us.cmp(&b.delay_us));

    // Take the sample with the minimum delay.
    Some(samples[0].clone())
}

/// Compute the jitter (RMS of offset differences from the mean) in microseconds.
fn compute_jitter(samples: &[NtpSample]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mean: f64 =
        samples.iter().map(|s| s.offset_us as f64).sum::<f64>() / samples.len() as f64;
    let variance: f64 = samples
        .iter()
        .map(|s| {
            let diff = s.offset_us as f64 - mean;
            diff * diff
        })
        .sum::<f64>()
        / samples.len() as f64;
    variance.sqrt()
}

// ---------------------------------------------------------------------------
// Date / Time helpers
// ---------------------------------------------------------------------------

/// Simple date-time structure (no external crate dependencies).
#[derive(Clone, Debug, PartialEq, Eq)]
struct DateTime {
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second,
        )
    }
}

/// Returns `true` if `year` is a leap year under the Gregorian calendar.
fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Number of days in a given month (1-based) for a given year.
fn days_in_month(year: u32, month: u32) -> Option<u32> {
    match month {
        1 => Some(31),
        2 => Some(if is_leap_year(year) { 29 } else { 28 }),
        3 => Some(31),
        4 => Some(30),
        5 => Some(31),
        6 => Some(30),
        7 => Some(31),
        8 => Some(31),
        9 => Some(30),
        10 => Some(31),
        11 => Some(30),
        12 => Some(31),
        _ => None,
    }
}

/// Day of year (1-based) for a given date.
fn day_of_year(year: u32, month: u32, day: u32) -> Option<u32> {
    if month < 1 || month > 12 {
        return None;
    }
    let mut doy: u32 = 0;
    for m in 1..month {
        doy = doy.checked_add(days_in_month(year, m)?)?;
    }
    doy.checked_add(day)
}

/// Convert a `DateTime` (assumed UTC) to Unix timestamp (seconds since
/// 1970-01-01 00:00:00 UTC). Only valid for years >= 1970.
fn datetime_to_unix(dt: &DateTime) -> Result<u64, String> {
    if dt.year < 1970 {
        return Err("year before Unix epoch (1970)".into());
    }
    let mut days: u64 = 0;
    for y in 1970..dt.year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    for m in 1..dt.month {
        days += u64::from(
            days_in_month(dt.year, m).ok_or_else(|| format!("invalid month: {m}"))?,
        );
    }
    days += u64::from(dt.day.saturating_sub(1));

    let secs = days
        .checked_mul(86400)
        .and_then(|s| s.checked_add(u64::from(dt.hour) * 3600))
        .and_then(|s| s.checked_add(u64::from(dt.minute) * 60))
        .and_then(|s| s.checked_add(u64::from(dt.second)))
        .ok_or_else(|| "timestamp overflow".to_string())?;

    Ok(secs)
}

/// Convert a Unix timestamp to a `DateTime` (UTC).
fn unix_to_datetime(mut secs: u64) -> DateTime {
    let second = (secs % 60) as u32;
    secs /= 60;
    let minute = (secs % 60) as u32;
    secs /= 60;
    let hour = (secs % 24) as u32;
    let mut days = secs / 24;

    let mut year: u32 = 1970;
    loop {
        let year_days: u64 = if is_leap_year(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    let mut month: u32 = 1;
    loop {
        let md = days_in_month(year, month).unwrap_or(30);
        if days < u64::from(md) {
            break;
        }
        days -= u64::from(md);
        month += 1;
    }

    let day = days as u32 + 1;
    DateTime { year, month, day, hour, minute, second }
}

/// Format a Unix timestamp as ISO 8601 (e.g. "2026-05-18T14:30:00Z").
fn format_iso8601(unix_secs: u64) -> String {
    let dt = unix_to_datetime(unix_secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second,
    )
}

/// Format a Unix timestamp in a human-readable style.
fn format_human(unix_secs: u64) -> String {
    let dt = unix_to_datetime(unix_secs);
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mi = dt.month.saturating_sub(1).min(11) as usize;
    format!(
        "{:02} {} {:04} {:02}:{:02}:{:02}",
        dt.day, MONTHS[mi], dt.year, dt.hour, dt.minute, dt.second,
    )
}

// ---------------------------------------------------------------------------
// System time helpers
// ---------------------------------------------------------------------------

/// Read the system (kernel) clock as a Unix timestamp via `/proc/time`.
fn read_system_time() -> Result<u64, String> {
    let content = fs::read_to_string("/proc/time")
        .map_err(|e| format!("/proc/time: {e}"))?;
    let trimmed = content.trim();
    let secs_str = trimmed.split('.').next().unwrap_or(trimmed);
    secs_str
        .parse::<u64>()
        .map_err(|e| format!("/proc/time parse error: {e}"))
}

/// Set the system clock to a given Unix timestamp.
fn write_system_time(unix_secs: u64) -> Result<(), String> {
    let path = "/proc/time";
    fs::write(path, format!("{unix_secs}").as_bytes())
        .map_err(|e| format!("{path}: {e}"))
}

// ---------------------------------------------------------------------------
// NTP network query
// ---------------------------------------------------------------------------

/// Perform a single NTP exchange with a server and return the sample.
fn ntp_query_one(
    server: &str,
    timeout_secs: u64,
    use_unpriv_port: bool,
    debug: bool,
) -> Result<NtpSample, String> {
    let addr = format!("{server}:{NTP_PORT}");
    let bind_addr = if use_unpriv_port { "0.0.0.0:0" } else { "0.0.0.0:0" };

    let socket =
        UdpSocket::bind(bind_addr).map_err(|e| format!("bind UDP: {e}"))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(timeout_secs)))
        .map_err(|e| format!("set timeout: {e}"))?;

    // T1: client transmit timestamp.
    let t1_unix = read_system_time().unwrap_or(0);
    let t1 = NtpTimestamp::from_unix(t1_unix, 0);

    let request = NtpPacket::new_client_request(t1);
    let pkt_bytes = request.to_bytes();

    if debug {
        eprintln!(
            "ntpd: sending {} bytes to {addr} (T1 = {}.{})",
            pkt_bytes.len(),
            t1.seconds,
            t1.fraction,
        );
    }

    socket
        .send_to(&pkt_bytes, &addr)
        .map_err(|e| format!("send to {addr}: {e}"))?;

    let mut buf = [0u8; NTP_PACKET_LEN];
    let (n, _src) = socket
        .recv_from(&mut buf)
        .map_err(|e| format!("recv from {addr}: {e}"))?;
    if n < NTP_PACKET_LEN {
        return Err(format!("NTP response too short: {n} bytes"));
    }

    // T4: client receive timestamp.
    let t4_unix = read_system_time().unwrap_or(t1_unix);
    let t4 = NtpTimestamp::from_unix(t4_unix, 0);

    let response = NtpPacket::from_bytes(&buf);

    if debug {
        eprintln!(
            "ntpd: response: li={} vn={} mode={} stratum={} ref_id={:?}",
            response.li, response.vn, response.mode, response.stratum, response.ref_id,
        );
        eprintln!(
            "ntpd: T2={}.{} T3={}.{}",
            response.receive_ts.seconds,
            response.receive_ts.fraction,
            response.transmit_ts.seconds,
            response.transmit_ts.fraction,
        );
    }

    response.validate_response()?;

    let t2 = response.receive_ts;
    let t3 = response.transmit_ts;

    let mut sample = compute_offset_delay(t1, t2, t3, t4);
    sample.stratum = response.stratum;

    if debug {
        eprintln!(
            "ntpd: offset={} us, delay={} us, stratum={}",
            sample.offset_us, sample.delay_us, sample.stratum,
        );
    }

    Ok(sample)
}

/// Query a server multiple times and return all samples.
fn ntp_query_multi(
    server: &str,
    count: u32,
    timeout_secs: u64,
    use_unpriv_port: bool,
    debug: bool,
) -> Vec<NtpSample> {
    let mut samples = Vec::new();
    for i in 0..count {
        match ntp_query_one(server, timeout_secs, use_unpriv_port, debug) {
            Ok(s) => samples.push(s),
            Err(e) => {
                if debug {
                    eprintln!("ntpd: query {}/{count} to {server} failed: {e}", i + 1);
                }
            }
        }
        // Brief pause between queries to avoid rate limiting.
        if i + 1 < count {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    samples
}

// ---------------------------------------------------------------------------
// Configuration file parsing
// ---------------------------------------------------------------------------

/// A parsed configuration directive.
#[derive(Clone, Debug, PartialEq)]
enum ConfigDirective {
    /// `server <hostname> [options]`
    Server(String),
    /// `pool <hostname> [options]`
    Pool(String),
    /// `restrict <address> <flags...>`
    Restrict { address: String, flags: Vec<String> },
    /// `driftfile <path>`
    DriftFile(String),
    /// `logfile <path>`
    LogFile(String),
}

/// Parse an ntp.conf file into a list of directives.
fn parse_config(content: &str) -> Vec<ConfigDirective> {
    let mut directives = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip empty lines and comments.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let keyword = match parts.next() {
            Some(k) => k,
            None => continue,
        };
        match keyword {
            "server" => {
                if let Some(host) = parts.next() {
                    directives.push(ConfigDirective::Server(host.to_string()));
                }
            }
            "pool" => {
                if let Some(host) = parts.next() {
                    directives.push(ConfigDirective::Pool(host.to_string()));
                }
            }
            "restrict" => {
                if let Some(addr) = parts.next() {
                    let flags: Vec<String> = parts.map(|s| s.to_string()).collect();
                    directives.push(ConfigDirective::Restrict {
                        address: addr.to_string(),
                        flags,
                    });
                }
            }
            "driftfile" => {
                if let Some(path) = parts.next() {
                    directives.push(ConfigDirective::DriftFile(path.to_string()));
                }
            }
            "logfile" => {
                if let Some(path) = parts.next() {
                    directives.push(ConfigDirective::LogFile(path.to_string()));
                }
            }
            _ => {
                // Unknown directive -- silently ignore for forward compatibility.
            }
        }
    }
    directives
}

/// Extract server hostnames from a parsed config.
fn servers_from_config(directives: &[ConfigDirective]) -> Vec<String> {
    let mut servers = Vec::new();
    for d in directives {
        match d {
            ConfigDirective::Server(h) | ConfigDirective::Pool(h) => {
                servers.push(h.clone());
            }
            _ => {}
        }
    }
    servers
}

/// Extract the drift file path from config (or use default).
fn driftfile_from_config(directives: &[ConfigDirective]) -> String {
    for d in directives {
        if let ConfigDirective::DriftFile(p) = d {
            return p.clone();
        }
    }
    DEFAULT_DRIFT_PATH.to_string()
}

/// Extract the log file path from config (if specified).
fn logfile_from_config(directives: &[ConfigDirective]) -> Option<String> {
    for d in directives {
        if let ConfigDirective::LogFile(p) = d {
            return Some(p.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Drift file
// ---------------------------------------------------------------------------

/// Read the frequency drift value from a drift file (PPM as f64).
fn read_drift(path: &str) -> Option<f64> {
    let content = fs::read_to_string(path).ok()?;
    content.trim().parse::<f64>().ok()
}

/// Write the frequency drift value to a drift file.
fn write_drift(path: &str, ppm: f64) -> Result<(), String> {
    // Ensure parent directory exists.
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, format!("{ppm:.6}\n").as_bytes())
        .map_err(|e| format!("{path}: {e}"))
}

// ---------------------------------------------------------------------------
// Clock discipline (proportional + integral)
// ---------------------------------------------------------------------------

/// Simple PI controller state for clock discipline.
struct ClockDiscipline {
    /// Accumulated integral term (frequency correction in PPM).
    freq_ppm: f64,
    /// Last offset for proportional term.
    last_offset_us: i64,
    /// Current poll interval in seconds.
    poll_interval: u32,
    /// Number of good updates received.
    update_count: u64,
}

impl ClockDiscipline {
    fn new(initial_freq_ppm: f64) -> Self {
        Self {
            freq_ppm: initial_freq_ppm,
            last_offset_us: 0,
            poll_interval: MIN_POLL_INTERVAL,
            update_count: 0,
        }
    }

    /// Process a new offset sample and return the time adjustment to apply.
    ///
    /// Returns (step: bool, correction_us: i64).
    /// - step=true means apply the full offset immediately.
    /// - step=false means slew gradually.
    fn update(&mut self, offset_us: i64, allow_step: bool) -> (bool, i64) {
        let abs_offset = offset_us.unsigned_abs() as i64;

        // Panic threshold: refuse.
        if abs_offset > PANIC_THRESHOLD_US {
            return (false, 0);
        }

        // Step threshold: step if large and allowed.
        if abs_offset > STEP_THRESHOLD_US && allow_step {
            self.last_offset_us = 0;
            self.update_count = self.update_count.saturating_add(1);
            return (true, offset_us);
        }

        // Proportional + Integral.
        // P gain: 1/4 of the offset.
        let p_correction = offset_us / 4;
        // I gain: accumulate frequency error.
        self.freq_ppm += (offset_us as f64) * 0.001;
        let i_correction = self.freq_ppm as i64;

        let correction = p_correction.saturating_add(i_correction);
        self.last_offset_us = offset_us;
        self.update_count = self.update_count.saturating_add(1);

        // Adjust poll interval based on stability.
        self.adjust_poll(abs_offset);

        (false, correction)
    }

    /// Adjust the poll interval: increase when stable, decrease when drifting.
    fn adjust_poll(&mut self, abs_offset: i64) {
        if abs_offset < STEP_THRESHOLD_US / 4 && self.update_count > 4 {
            // Stable: increase poll.
            if self.poll_interval < MAX_POLL_INTERVAL {
                self.poll_interval = self.poll_interval.saturating_mul(2).min(MAX_POLL_INTERVAL);
            }
        } else if abs_offset > STEP_THRESHOLD_US / 2 {
            // Drifting: decrease poll.
            if self.poll_interval > MIN_POLL_INTERVAL {
                self.poll_interval = self.poll_interval.saturating_div(2).max(MIN_POLL_INTERVAL);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Status display (ntpq -p style)
// ---------------------------------------------------------------------------

/// Format a status line for a peer (ntpq -p style).
fn format_peer_status(
    server: &str,
    stratum: u8,
    reach: u8,
    delay_us: i64,
    offset_us: i64,
    jitter_us: f64,
) -> String {
    let delay_ms = delay_us as f64 / 1000.0;
    let offset_ms = offset_us as f64 / 1000.0;
    let jitter_ms = jitter_us / 1000.0;
    format!(
        "*{:<24} .{:.<5}. {:>2} u {:>4} {:>3} {:>8.3} {:>9.3} {:>8.3}",
        server,
        "",
        stratum,
        "-",
        reach,
        delay_ms,
        offset_ms,
        jitter_ms,
    )
}

/// Print the ntpq -p style header.
fn print_peer_header() {
    println!(
        "{:<25} {:>7} {:>2} {:>1} {:>4} {:>3} {:>8} {:>9} {:>8}",
        "remote", "refid", "st", "t", "when", "reach", "delay", "offset", "jitter",
    );
    println!("{}", "=".repeat(76));
}

// ---------------------------------------------------------------------------
// Mode: ntpdate / sntp (one-shot query)
// ---------------------------------------------------------------------------

struct NtpdateOpts {
    servers: Vec<String>,
    query_only: bool,
    num_samples: u32,
    timeout_secs: u64,
    debug: bool,
    use_unpriv_port: bool,
    use_syslog: bool,
    force_step: bool,
}

fn run_ntpdate(opts: &NtpdateOpts) -> Result<(), String> {
    let servers = if opts.servers.is_empty() {
        DEFAULT_SERVERS.iter().map(|s| (*s).to_string()).collect()
    } else {
        opts.servers.clone()
    };

    let mut best_sample: Option<NtpSample> = None;
    let mut best_server: Option<String> = None;

    for server in &servers {
        if opts.debug {
            eprintln!("ntpdate: querying {server} ({} samples)...", opts.num_samples);
        }

        let mut samples = ntp_query_multi(
            server,
            opts.num_samples,
            opts.timeout_secs,
            opts.use_unpriv_port,
            opts.debug,
        );

        if samples.is_empty() {
            if !opts.use_syslog {
                eprintln!("ntpdate: no response from {server}");
            }
            continue;
        }

        if let Some(filtered) = clock_filter(&mut samples) {
            let replace = match &best_sample {
                None => true,
                Some(prev) => filtered.delay_us < prev.delay_us,
            };
            if replace {
                best_sample = Some(filtered);
                best_server = Some(server.clone());
            }
        }
    }

    let sample = best_sample.ok_or("no usable responses from any server")?;
    let server = best_server.unwrap_or_default();

    let offset_ms = sample.offset_us as f64 / 1000.0;
    let delay_ms = sample.delay_us as f64 / 1000.0;

    let output = |msg: &str| {
        if opts.use_syslog {
            // Write to syslog via /dev/log or /var/log/syslog.
            let _ = fs::OpenOptions::new()
                .append(true)
                .open("/dev/log")
                .and_then(|mut f| writeln!(f, "ntpdate: {msg}"));
        } else {
            println!("{msg}");
        }
    };

    if opts.query_only {
        let now = read_system_time().unwrap_or(0);
        output(&format!(
            "server {server}, stratum {}, offset {offset_ms:.6} ms, delay {delay_ms:.6} ms",
            sample.stratum,
        ));
        output(&format!("  {} (UTC)", format_iso8601(now)));
        return Ok(());
    }

    // Apply the time correction.
    let abs_offset = sample.offset_us.unsigned_abs() as i64;

    if abs_offset > PANIC_THRESHOLD_US {
        return Err(format!(
            "offset {offset_ms:.3} ms exceeds panic threshold; refusing to set time",
        ));
    }

    let now = read_system_time().unwrap_or(0);

    if opts.force_step || abs_offset > STEP_THRESHOLD_US {
        // Step: set the clock immediately.
        let new_secs = if sample.offset_us >= 0 {
            now.saturating_add((sample.offset_us / 1_000_000) as u64)
        } else {
            now.saturating_sub((sample.offset_us.unsigned_abs() / 1_000_000) as u64)
        };
        write_system_time(new_secs)?;
        output(&format!(
            "step time server {server} offset {offset_ms:.6} ms",
        ));
    } else {
        // Slew: adjust gradually (for now, still a step since we lack adjtime).
        let new_secs = if sample.offset_us >= 0 {
            now.saturating_add((sample.offset_us / 1_000_000) as u64)
        } else {
            now.saturating_sub((sample.offset_us.unsigned_abs() / 1_000_000) as u64)
        };
        write_system_time(new_secs)?;
        output(&format!(
            "adjust time server {server} offset {offset_ms:.6} ms",
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Mode: ntpd (daemon)
// ---------------------------------------------------------------------------

struct NtpdOpts {
    config_path: String,
    allow_first_step: bool,
    no_daemonize: bool,
    set_and_exit: bool,
    debug: bool,
}

fn run_ntpd(opts: &NtpdOpts) -> Result<(), String> {
    // Load configuration.
    let config_content = fs::read_to_string(&opts.config_path).unwrap_or_default();
    let directives = parse_config(&config_content);

    let servers = servers_from_config(&directives);
    let servers = if servers.is_empty() {
        DEFAULT_SERVERS.iter().map(|s| (*s).to_string()).collect()
    } else {
        servers
    };

    let drift_path = driftfile_from_config(&directives);
    let _log_path = logfile_from_config(&directives);

    if opts.debug {
        eprintln!("ntpd: config from {}", opts.config_path);
        eprintln!("ntpd: servers: {servers:?}");
        eprintln!("ntpd: drift file: {drift_path}");
    }

    // Load saved drift.
    let initial_drift = read_drift(&drift_path).unwrap_or(0.0);
    if opts.debug {
        eprintln!("ntpd: initial drift: {initial_drift:.6} PPM");
    }

    let mut discipline = ClockDiscipline::new(initial_drift);
    let mut first_update = true;

    // Main loop.
    loop {
        let mut all_samples: Vec<NtpSample> = Vec::new();
        let mut best_server = String::new();

        for server in &servers {
            let mut samples = ntp_query_multi(server, 4, DEFAULT_TIMEOUT_SECS, false, opts.debug);
            if let Some(filtered) = clock_filter(&mut samples) {
                let replace = match all_samples.first() {
                    None => true,
                    Some(prev) => filtered.delay_us < prev.delay_us,
                };
                if replace {
                    best_server = server.clone();
                }
                all_samples.push(filtered);
            }
        }

        if all_samples.is_empty() {
            if opts.debug {
                eprintln!("ntpd: no usable responses this cycle");
            }
            std::thread::sleep(Duration::from_secs(u64::from(discipline.poll_interval)));
            continue;
        }

        let jitter = compute_jitter(&all_samples);

        // Use the sample from the best (lowest-delay) server.
        all_samples.sort_by(|a, b| a.delay_us.cmp(&b.delay_us));
        let best = &all_samples[0];

        if opts.debug {
            eprintln!(
                "ntpd: best server={best_server} offset={} us delay={} us stratum={} jitter={:.0} us",
                best.offset_us, best.delay_us, best.stratum, jitter,
            );
        }

        // Apply clock discipline.
        let allow_step = first_update && opts.allow_first_step;
        let (step, correction) = discipline.update(best.offset_us, allow_step);

        if step {
            let now = read_system_time().unwrap_or(0);
            let new_secs = if correction >= 0 {
                now.saturating_add((correction / 1_000_000) as u64)
            } else {
                now.saturating_sub((correction.unsigned_abs() / 1_000_000) as u64)
            };
            let _ = write_system_time(new_secs);
            if opts.debug {
                eprintln!("ntpd: stepped clock by {} us", correction);
            }
        } else if correction != 0 {
            let now = read_system_time().unwrap_or(0);
            let new_secs = if correction >= 0 {
                now.saturating_add((correction / 1_000_000) as u64)
            } else {
                now.saturating_sub((correction.unsigned_abs() / 1_000_000) as u64)
            };
            let _ = write_system_time(new_secs);
            if opts.debug {
                eprintln!("ntpd: slew correction {} us", correction);
            }
        }

        first_update = false;

        // Save drift.
        let _ = write_drift(&drift_path, discipline.freq_ppm);

        // Print status.
        if opts.debug || opts.no_daemonize {
            print_peer_header();
            for (server, sample) in servers.iter().zip(all_samples.iter()) {
                println!(
                    "{}",
                    format_peer_status(
                        server,
                        sample.stratum,
                        0xFF,
                        sample.delay_us,
                        sample.offset_us,
                        jitter,
                    ),
                );
            }
            println!();
        }

        if opts.set_and_exit {
            if opts.debug {
                eprintln!("ntpd: -q mode, exiting after first update");
            }
            break;
        }

        // Sleep until next poll.
        if opts.debug {
            eprintln!("ntpd: sleeping {} s until next poll", discipline.poll_interval);
        }
        std::thread::sleep(Duration::from_secs(u64::from(discipline.poll_interval)));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Argv[0] personality detection
// ---------------------------------------------------------------------------

/// Determine the binary personality from argv[0].
fn detect_personality() -> &'static str {
    let argv0 = env::args().next().unwrap_or_default();
    let basename = argv0.rsplit('/').next().unwrap_or(&argv0);
    let basename = basename.rsplit('\\').next().unwrap_or(basename);
    // Strip .exe suffix for Windows/testing.
    let basename = basename.strip_suffix(".exe").unwrap_or(basename);
    match basename {
        "ntpdate" => "ntpdate",
        "sntp" => "sntp",
        _ => "ntpd",
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

fn parse_ntpdate_args(args: &[String]) -> Result<NtpdateOpts, String> {
    let mut opts = NtpdateOpts {
        servers: Vec::new(),
        query_only: false,
        num_samples: DEFAULT_SAMPLES,
        timeout_secs: DEFAULT_TIMEOUT_SECS,
        debug: false,
        use_unpriv_port: false,
        use_syslog: false,
        force_step: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-q" => opts.query_only = true,
            "-p" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or("-p requires an argument")?;
                opts.num_samples = val
                    .parse()
                    .map_err(|_| format!("-p: invalid number: {val}"))?;
            }
            "-t" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or("-t requires an argument")?;
                opts.timeout_secs = val
                    .parse()
                    .map_err(|_| format!("-t: invalid number: {val}"))?;
            }
            "-d" => opts.debug = true,
            "-u" => opts.use_unpriv_port = true,
            "-s" => opts.use_syslog = true,
            "-b" => opts.force_step = true,
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            server => {
                opts.servers.push(server.to_string());
            }
        }
        i += 1;
    }

    Ok(opts)
}

fn parse_ntpd_args(args: &[String]) -> Result<NtpdOpts, String> {
    let mut opts = NtpdOpts {
        config_path: DEFAULT_CONFIG_PATH.to_string(),
        allow_first_step: false,
        no_daemonize: false,
        set_and_exit: false,
        debug: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-g" => opts.allow_first_step = true,
            "-n" => opts.no_daemonize = true,
            "-q" => opts.set_and_exit = true,
            "-d" => {
                opts.debug = true;
                opts.no_daemonize = true; // -d implies -n
            }
            "-c" => {
                i += 1;
                opts.config_path = args
                    .get(i)
                    .ok_or("-c requires a config file path")?
                    .clone();
            }
            "--help" | "-h" => {
                print_ntpd_usage();
                process::exit(0);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                // ntpd doesn't take positional args
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ---------------------------------------------------------------------------
// Usage messages
// ---------------------------------------------------------------------------

fn print_ntpdate_usage() {
    let usage = "\
ntpdate - NTP time synchronization (one-shot)

Usage:
  ntpdate [OPTIONS] [server ...]

Options:
  -q            Query only (don't set clock)
  -p N          Number of samples per server (default 4)
  -t SECS       Query timeout in seconds (default 5)
  -d            Debug/verbose mode
  -u            Use unprivileged port
  -s            Log to syslog instead of stdout
  -b            Force step (immediate set) instead of slew

Default servers: pool.ntp.org, time.google.com, time.cloudflare.com

Examples:
  ntpdate                     Query default servers and set clock
  ntpdate -q pool.ntp.org     Query only, don't set clock
  ntpdate -p 8 time.google.com  8 samples from time.google.com";
    println!("{usage}");
}

fn print_ntpd_usage() {
    let usage = "\
ntpd - NTP daemon for time synchronization

Usage:
  ntpd [OPTIONS]

Options:
  -g            Allow first large time step
  -n            Don't fork/daemonize
  -q            Set time and exit
  -d            Debug mode (implies -n)
  -c FILE       Configuration file (default /etc/ntp.conf)
  -h, --help    Show this help message

Configuration directives (in /etc/ntp.conf):
  server <hostname>           Add an NTP server
  pool <hostname>             Add an NTP pool
  restrict <addr> <flags>     Access restrictions
  driftfile <path>            Frequency drift file
  logfile <path>              Log file path

Examples:
  ntpd -g -n                  Run in foreground, allow first step
  ntpd -d                     Debug mode
  ntpd -q                     Set time once and exit";
    println!("{usage}");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let personality = detect_personality();
    let args: Vec<String> = env::args().skip(1).collect();

    // Check for --help in any mode.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match personality {
            "ntpdate" | "sntp" => print_ntpdate_usage(),
            _ => print_ntpd_usage(),
        }
        process::exit(0);
    }

    let result = match personality {
        "ntpdate" | "sntp" => {
            match parse_ntpdate_args(&args) {
                Ok(opts) => run_ntpdate(&opts),
                Err(e) => {
                    eprintln!("{personality}: {e}");
                    print_ntpdate_usage();
                    process::exit(1);
                }
            }
        }
        _ => {
            match parse_ntpd_args(&args) {
                Ok(opts) => run_ntpd(&opts),
                Err(e) => {
                    eprintln!("ntpd: {e}");
                    print_ntpd_usage();
                    process::exit(1);
                }
            }
        }
    };

    if let Err(e) = result {
        eprintln!("{personality}: {e}");
        process::exit(1);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- NTP Packet building ------------------------------------------------

    #[test]
    fn test_client_request_fields() {
        let ts = NtpTimestamp::new(100, 200);
        let pkt = NtpPacket::new_client_request(ts);
        assert_eq!(pkt.li, 0);
        assert_eq!(pkt.vn, 4);
        assert_eq!(pkt.mode, 3);
        assert_eq!(pkt.transmit_ts, ts);
    }

    #[test]
    fn test_client_request_byte0() {
        let pkt = NtpPacket::new_client_request(NtpTimestamp::default());
        let bytes = pkt.to_bytes();
        // LI=0, VN=4, Mode=3 => 0b00_100_011 = 0x23
        assert_eq!(bytes[0], 0x23);
    }

    #[test]
    fn test_packet_roundtrip() {
        let ts = NtpTimestamp::new(3_900_000_000, 0x8000_0000);
        let pkt = NtpPacket {
            li: 0,
            vn: 4,
            mode: 4,
            stratum: 2,
            poll: 6,
            precision: -20,
            root_delay: 0x0100,
            root_dispersion: 0x0200,
            ref_id: [10, 0, 0, 1],
            ref_ts: NtpTimestamp::new(1000, 2000),
            origin_ts: NtpTimestamp::new(3000, 4000),
            receive_ts: NtpTimestamp::new(5000, 6000),
            transmit_ts: ts,
        };
        let bytes = pkt.to_bytes();
        let parsed = NtpPacket::from_bytes(&bytes);
        assert_eq!(parsed.li, pkt.li);
        assert_eq!(parsed.vn, pkt.vn);
        assert_eq!(parsed.mode, pkt.mode);
        assert_eq!(parsed.stratum, pkt.stratum);
        assert_eq!(parsed.poll, pkt.poll);
        assert_eq!(parsed.precision, pkt.precision);
        assert_eq!(parsed.root_delay, pkt.root_delay);
        assert_eq!(parsed.root_dispersion, pkt.root_dispersion);
        assert_eq!(parsed.ref_id, pkt.ref_id);
        assert_eq!(parsed.ref_ts, pkt.ref_ts);
        assert_eq!(parsed.origin_ts, pkt.origin_ts);
        assert_eq!(parsed.receive_ts, pkt.receive_ts);
        assert_eq!(parsed.transmit_ts, pkt.transmit_ts);
    }

    #[test]
    fn test_packet_all_zeros() {
        let bytes = [0u8; NTP_PACKET_LEN];
        let pkt = NtpPacket::from_bytes(&bytes);
        assert_eq!(pkt.li, 0);
        assert_eq!(pkt.vn, 0);
        assert_eq!(pkt.mode, 0);
        assert_eq!(pkt.stratum, 0);
        assert!(pkt.transmit_ts.is_zero());
    }

    #[test]
    fn test_packet_max_values() {
        let pkt = NtpPacket {
            li: 3,
            vn: 7,
            mode: 7,
            stratum: 255,
            poll: 127,
            precision: -128,
            root_delay: u32::MAX,
            root_dispersion: u32::MAX,
            ref_id: [0xFF; 4],
            ref_ts: NtpTimestamp::new(u32::MAX, u32::MAX),
            origin_ts: NtpTimestamp::new(u32::MAX, u32::MAX),
            receive_ts: NtpTimestamp::new(u32::MAX, u32::MAX),
            transmit_ts: NtpTimestamp::new(u32::MAX, u32::MAX),
        };
        let bytes = pkt.to_bytes();
        let parsed = NtpPacket::from_bytes(&bytes);
        assert_eq!(parsed.li, 3);
        assert_eq!(parsed.vn, 7);
        assert_eq!(parsed.mode, 7);
        assert_eq!(parsed.stratum, 255);
    }

    // -- NTP Timestamp conversion -------------------------------------------

    #[test]
    fn test_ntp_ts_from_unix_epoch() {
        let ts = NtpTimestamp::from_unix(0, 0);
        assert_eq!(ts.seconds, NTP_UNIX_DELTA as u32);
        assert_eq!(ts.fraction, 0);
    }

    #[test]
    fn test_ntp_ts_to_unix_epoch() {
        let ts = NtpTimestamp::new(NTP_UNIX_DELTA as u32, 0);
        assert_eq!(ts.to_unix_secs(), Some(0));
    }

    #[test]
    fn test_ntp_ts_before_unix_epoch() {
        let ts = NtpTimestamp::new(100, 0);
        assert_eq!(ts.to_unix_secs(), None);
    }

    #[test]
    fn test_ntp_ts_known_value() {
        // 2026-01-01 00:00:00 UTC = Unix 1767225600
        let unix_secs: u64 = 1_767_225_600;
        let ts = NtpTimestamp::from_unix(unix_secs, 0);
        assert_eq!(ts.to_unix_secs(), Some(unix_secs));
    }

    #[test]
    fn test_ntp_ts_fraction_half() {
        // 500000 microseconds = 0.5 seconds => fraction should be ~0x80000000
        let ts = NtpTimestamp::from_unix(0, 500_000);
        // Allow small rounding error.
        let diff = (ts.fraction as i64 - 0x8000_0000_i64).unsigned_abs();
        assert!(diff < 5000, "fraction {:#x} not close to 0x80000000", ts.fraction);
    }

    #[test]
    fn test_ntp_ts_f64_roundtrip() {
        let ts = NtpTimestamp::new(3_800_000_000, 0x4000_0000);
        let f = ts.to_f64();
        let rt = NtpTimestamp::from_f64(f);
        assert_eq!(rt.seconds, ts.seconds);
        // Fraction may have small rounding.
        let diff = (rt.fraction as i64 - ts.fraction as i64).unsigned_abs();
        assert!(diff < 2, "fraction drift too large: {diff}");
    }

    #[test]
    fn test_ntp_ts_zero() {
        let ts = NtpTimestamp::default();
        assert!(ts.is_zero());
    }

    #[test]
    fn test_ntp_ts_not_zero() {
        let ts = NtpTimestamp::new(0, 1);
        assert!(!ts.is_zero());
    }

    // -- Offset calculation -------------------------------------------------

    #[test]
    fn test_offset_symmetric() {
        // If T2 - T1 = T3 - T4 = delta, offset = delta.
        let t1 = NtpTimestamp::new(1000, 0);
        let t2 = NtpTimestamp::new(1001, 0);
        let t3 = NtpTimestamp::new(1001, 0);
        let t4 = NtpTimestamp::new(1000, 0);
        let s = compute_offset_delay(t1, t2, t3, t4);
        // offset = ((1) + (1)) / 2 = 1 second = 1_000_000 us
        assert!((s.offset_us - 1_000_000).unsigned_abs() < 100);
    }

    #[test]
    fn test_offset_zero() {
        // Clocks perfectly synchronized, zero delay.
        let t = NtpTimestamp::new(1000, 0);
        let s = compute_offset_delay(t, t, t, t);
        assert_eq!(s.offset_us, 0);
        assert_eq!(s.delay_us, 0);
    }

    #[test]
    fn test_delay_calculation() {
        // T1=100, T2=101, T3=102, T4=105
        // delay = (T4 - T1) - (T3 - T2) = 5 - 1 = 4 seconds
        let t1 = NtpTimestamp::new(100, 0);
        let t2 = NtpTimestamp::new(101, 0);
        let t3 = NtpTimestamp::new(102, 0);
        let t4 = NtpTimestamp::new(105, 0);
        let s = compute_offset_delay(t1, t2, t3, t4);
        assert!((s.delay_us - 4_000_000).unsigned_abs() < 100);
    }

    #[test]
    fn test_negative_offset() {
        // Client clock is ahead: T2 < T1 and T3 < T4.
        let t1 = NtpTimestamp::new(1001, 0);
        let t2 = NtpTimestamp::new(1000, 0);
        let t3 = NtpTimestamp::new(1000, 0);
        let t4 = NtpTimestamp::new(1001, 0);
        let s = compute_offset_delay(t1, t2, t3, t4);
        assert!((s.offset_us + 1_000_000).unsigned_abs() < 100);
    }

    #[test]
    fn test_offset_with_asymmetric_delay() {
        // T1=100, T2=103, T3=103, T4=104
        // offset = ((3) + (-1)) / 2 = 1 second
        // delay = (4) - (0) = 4 seconds
        let t1 = NtpTimestamp::new(100, 0);
        let t2 = NtpTimestamp::new(103, 0);
        let t3 = NtpTimestamp::new(103, 0);
        let t4 = NtpTimestamp::new(104, 0);
        let s = compute_offset_delay(t1, t2, t3, t4);
        assert!((s.offset_us - 1_000_000).unsigned_abs() < 100);
        assert!((s.delay_us - 4_000_000).unsigned_abs() < 100);
    }

    // -- Clock filter -------------------------------------------------------

    #[test]
    fn test_clock_filter_empty() {
        let mut samples: Vec<NtpSample> = Vec::new();
        assert!(clock_filter(&mut samples).is_none());
    }

    #[test]
    fn test_clock_filter_single() {
        let s = NtpSample { offset_us: 500, delay_us: 100, stratum: 2 };
        let mut samples = vec![s.clone()];
        let best = clock_filter(&mut samples).unwrap();
        assert_eq!(best.offset_us, 500);
    }

    #[test]
    fn test_clock_filter_picks_min_delay() {
        let mut samples = vec![
            NtpSample { offset_us: 500, delay_us: 300, stratum: 2 },
            NtpSample { offset_us: 400, delay_us: 100, stratum: 2 },
            NtpSample { offset_us: 600, delay_us: 200, stratum: 2 },
        ];
        let best = clock_filter(&mut samples).unwrap();
        assert_eq!(best.delay_us, 100);
        assert_eq!(best.offset_us, 400);
    }

    // -- Stratum validation -------------------------------------------------

    #[test]
    fn test_stratum_valid() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 2, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0, ref_id: [10, 0, 0, 1],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(pkt.validate_response().is_ok());
    }

    #[test]
    fn test_stratum_invalid_16() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 16, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0, ref_id: [10, 0, 0, 1],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(pkt.validate_response().is_err());
    }

    #[test]
    fn test_stratum_1_primary() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 1, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0,
            ref_id: [b'G', b'P', b'S', 0],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(pkt.validate_response().is_ok());
        assert_eq!(pkt.stratum, 1);
    }

    // -- KoD detection ------------------------------------------------------

    #[test]
    fn test_kod_deny() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 0, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0,
            ref_id: [b'D', b'E', b'N', b'Y'],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(pkt.is_kod());
        assert_eq!(pkt.kod_code(), Some("DENY".to_string()));
    }

    #[test]
    fn test_kod_rate() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 0, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0,
            ref_id: [b'R', b'A', b'T', b'E'],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(pkt.is_kod());
        assert_eq!(pkt.kod_code(), Some("RATE".to_string()));
        // Should fail validation.
        assert!(pkt.validate_response().is_err());
    }

    #[test]
    fn test_not_kod_normal_server() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 2, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0,
            ref_id: [192, 168, 1, 1],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(!pkt.is_kod());
    }

    // -- Config file parsing ------------------------------------------------

    #[test]
    fn test_parse_config_server() {
        let conf = "server pool.ntp.org iburst\nserver time.google.com\n";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 2);
        assert_eq!(dirs[0], ConfigDirective::Server("pool.ntp.org".to_string()));
        assert_eq!(dirs[1], ConfigDirective::Server("time.google.com".to_string()));
    }

    #[test]
    fn test_parse_config_pool() {
        let conf = "pool 0.pool.ntp.org iburst\n";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], ConfigDirective::Pool("0.pool.ntp.org".to_string()));
    }

    #[test]
    fn test_parse_config_restrict() {
        let conf = "restrict default kod nomodify notrap nopeer\n";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 1);
        match &dirs[0] {
            ConfigDirective::Restrict { address, flags } => {
                assert_eq!(address, "default");
                assert_eq!(flags, &["kod", "nomodify", "notrap", "nopeer"]);
            }
            _ => panic!("expected Restrict"),
        }
    }

    #[test]
    fn test_parse_config_driftfile() {
        let conf = "driftfile /var/lib/ntp/drift\n";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], ConfigDirective::DriftFile("/var/lib/ntp/drift".to_string()));
    }

    #[test]
    fn test_parse_config_logfile() {
        let conf = "logfile /var/log/ntp.log\n";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], ConfigDirective::LogFile("/var/log/ntp.log".to_string()));
    }

    #[test]
    fn test_parse_config_comments_and_blank() {
        let conf = "# This is a comment\n\n  \nserver pool.ntp.org\n";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], ConfigDirective::Server("pool.ntp.org".to_string()));
    }

    #[test]
    fn test_parse_config_mixed() {
        let conf = "\
server pool.ntp.org iburst
pool 0.pool.ntp.org
restrict default kod nomodify
driftfile /var/lib/ntp/drift
logfile /var/log/ntp.log
";
        let dirs = parse_config(conf);
        assert_eq!(dirs.len(), 5);
    }

    #[test]
    fn test_servers_from_config() {
        let conf = "server a.ntp.org\npool b.ntp.org\nrestrict default\n";
        let dirs = parse_config(conf);
        let servers = servers_from_config(&dirs);
        assert_eq!(servers, vec!["a.ntp.org", "b.ntp.org"]);
    }

    #[test]
    fn test_driftfile_from_config_present() {
        let conf = "driftfile /custom/drift\n";
        let dirs = parse_config(conf);
        assert_eq!(driftfile_from_config(&dirs), "/custom/drift");
    }

    #[test]
    fn test_driftfile_from_config_default() {
        let dirs: Vec<ConfigDirective> = Vec::new();
        assert_eq!(driftfile_from_config(&dirs), DEFAULT_DRIFT_PATH);
    }

    // -- Poll interval adjustment -------------------------------------------

    #[test]
    fn test_poll_increases_when_stable() {
        let mut disc = ClockDiscipline::new(0.0);
        disc.update_count = 10;
        disc.poll_interval = MIN_POLL_INTERVAL;
        // Small offset => stable.
        disc.adjust_poll(1000);
        assert!(disc.poll_interval > MIN_POLL_INTERVAL);
    }

    #[test]
    fn test_poll_decreases_when_drifting() {
        let mut disc = ClockDiscipline::new(0.0);
        disc.poll_interval = MAX_POLL_INTERVAL;
        // Large offset => drifting.
        disc.adjust_poll(STEP_THRESHOLD_US);
        assert!(disc.poll_interval < MAX_POLL_INTERVAL);
    }

    #[test]
    fn test_poll_clamped_min() {
        let mut disc = ClockDiscipline::new(0.0);
        disc.poll_interval = MIN_POLL_INTERVAL;
        disc.adjust_poll(STEP_THRESHOLD_US);
        assert_eq!(disc.poll_interval, MIN_POLL_INTERVAL);
    }

    #[test]
    fn test_poll_clamped_max() {
        let mut disc = ClockDiscipline::new(0.0);
        disc.update_count = 100;
        disc.poll_interval = MAX_POLL_INTERVAL;
        disc.adjust_poll(0);
        assert_eq!(disc.poll_interval, MAX_POLL_INTERVAL);
    }

    // -- Drift file ---------------------------------------------------------

    #[test]
    fn test_drift_read_missing() {
        assert!(read_drift("/nonexistent/drift").is_none());
    }

    #[test]
    fn test_drift_write_read_roundtrip() {
        let path = "/tmp/ntpd_test_drift";
        let ppm = -3.141_592;
        write_drift(path, ppm).unwrap();
        let read_val = read_drift(path).unwrap();
        assert!((read_val - ppm).abs() < 0.001);
        // Cleanup.
        let _ = fs::remove_file(path);
    }

    // -- Calendar helpers ---------------------------------------------------

    #[test]
    fn test_leap_year_cases() {
        assert!(is_leap_year(2000));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(1900));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(1600));
        assert!(!is_leap_year(2100));
    }

    #[test]
    fn test_days_in_month_all() {
        let expected = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for (i, &e) in expected.iter().enumerate() {
            assert_eq!(days_in_month(2023, i as u32 + 1), Some(e));
        }
    }

    #[test]
    fn test_days_in_month_feb_leap() {
        assert_eq!(days_in_month(2024, 2), Some(29));
        assert_eq!(days_in_month(2023, 2), Some(28));
    }

    #[test]
    fn test_days_in_month_invalid() {
        assert_eq!(days_in_month(2023, 0), None);
        assert_eq!(days_in_month(2023, 13), None);
    }

    #[test]
    fn test_day_of_year_jan1() {
        assert_eq!(day_of_year(2023, 1, 1), Some(1));
    }

    #[test]
    fn test_day_of_year_dec31() {
        assert_eq!(day_of_year(2023, 12, 31), Some(365));
        assert_eq!(day_of_year(2024, 12, 31), Some(366));
    }

    #[test]
    fn test_day_of_year_invalid_month() {
        assert_eq!(day_of_year(2023, 0, 1), None);
        assert_eq!(day_of_year(2023, 13, 1), None);
    }

    // -- Timestamp formatting -----------------------------------------------

    #[test]
    fn test_format_iso8601_epoch() {
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_format_iso8601_known() {
        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(format_iso8601(946_684_800), "2000-01-01T00:00:00Z");
    }

    #[test]
    fn test_format_human() {
        let s = format_human(0);
        assert!(s.contains("Jan"));
        assert!(s.contains("1970"));
    }

    // -- DateTime roundtrip -------------------------------------------------

    #[test]
    fn test_unix_epoch_roundtrip() {
        let dt = DateTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 };
        assert_eq!(datetime_to_unix(&dt).unwrap(), 0);
    }

    #[test]
    fn test_datetime_roundtrip_various() {
        let cases = [
            DateTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 },
            DateTime { year: 2000, month: 2, day: 29, hour: 23, minute: 59, second: 59 },
            DateTime { year: 2024, month: 12, day: 31, hour: 12, minute: 0, second: 0 },
            DateTime { year: 2038, month: 1, day: 19, hour: 3, minute: 14, second: 7 },
        ];
        for dt in &cases {
            let ts = datetime_to_unix(dt).unwrap();
            let rt = unix_to_datetime(ts);
            assert_eq!(&rt, dt, "round-trip failed for {dt}");
        }
    }

    #[test]
    fn test_datetime_before_epoch() {
        let dt = DateTime { year: 1969, month: 12, day: 31, hour: 23, minute: 59, second: 59 };
        assert!(datetime_to_unix(&dt).is_err());
    }

    // -- Default servers ----------------------------------------------------

    #[test]
    fn test_default_servers_non_empty() {
        assert!(!DEFAULT_SERVERS.is_empty());
    }

    #[test]
    fn test_default_servers_contain_pool() {
        assert!(DEFAULT_SERVERS.iter().any(|s| s.contains("pool.ntp.org")));
    }

    #[test]
    fn test_default_servers_contain_google() {
        assert!(DEFAULT_SERVERS.iter().any(|s| s.contains("google")));
    }

    // -- Edge cases: epoch rollover, high jitter ----------------------------

    #[test]
    fn test_ntp_ts_near_u32_max() {
        // NTP timestamp near rollover (2036 era boundary).
        let ts = NtpTimestamp::new(u32::MAX - 10, 0);
        let f = ts.to_f64();
        let rt = NtpTimestamp::from_f64(f);
        assert_eq!(rt.seconds, u32::MAX - 10);
    }

    #[test]
    fn test_ntp_ts_from_unix_large() {
        // Unix timestamp that would push past NTP u32 range.
        let ts = NtpTimestamp::from_unix(u64::from(u32::MAX), 0);
        // Should clamp to u32::MAX.
        assert_eq!(ts.seconds, u32::MAX);
    }

    #[test]
    fn test_jitter_single_sample() {
        let samples = [NtpSample { offset_us: 100, delay_us: 50, stratum: 2 }];
        assert_eq!(compute_jitter(&samples), 0.0);
    }

    #[test]
    fn test_jitter_identical_samples() {
        let samples = [
            NtpSample { offset_us: 100, delay_us: 50, stratum: 2 },
            NtpSample { offset_us: 100, delay_us: 50, stratum: 2 },
            NtpSample { offset_us: 100, delay_us: 50, stratum: 2 },
        ];
        assert_eq!(compute_jitter(&samples), 0.0);
    }

    #[test]
    fn test_jitter_high_variance() {
        let samples = [
            NtpSample { offset_us: 0, delay_us: 50, stratum: 2 },
            NtpSample { offset_us: 1_000_000, delay_us: 50, stratum: 2 },
        ];
        let j = compute_jitter(&samples);
        assert!(j > 400_000.0, "jitter should be high: {j}");
    }

    // -- Personality detection ----------------------------------------------

    #[test]
    fn test_personality_defaults_ntpd() {
        // When running as "ntpd" the detect_personality reads env::args() which
        // during test is the test binary. It should fall through to "ntpd".
        let p = detect_personality();
        // It might be "ntpd" or the test runner name, which maps to "ntpd".
        assert_eq!(p, "ntpd");
    }

    // -- Argument parsing ---------------------------------------------------

    #[test]
    fn test_parse_ntpdate_query_only() {
        let args = vec!["-q".to_string(), "pool.ntp.org".to_string()];
        let opts = parse_ntpdate_args(&args).unwrap();
        assert!(opts.query_only);
        assert_eq!(opts.servers, vec!["pool.ntp.org"]);
    }

    #[test]
    fn test_parse_ntpdate_samples() {
        let args = vec!["-p".to_string(), "8".to_string()];
        let opts = parse_ntpdate_args(&args).unwrap();
        assert_eq!(opts.num_samples, 8);
    }

    #[test]
    fn test_parse_ntpdate_debug_and_step() {
        let args = vec!["-d".to_string(), "-b".to_string()];
        let opts = parse_ntpdate_args(&args).unwrap();
        assert!(opts.debug);
        assert!(opts.force_step);
    }

    #[test]
    fn test_parse_ntpd_debug_implies_nofork() {
        let args = vec!["-d".to_string()];
        let opts = parse_ntpd_args(&args).unwrap();
        assert!(opts.debug);
        assert!(opts.no_daemonize);
    }

    #[test]
    fn test_parse_ntpd_set_and_exit() {
        let args = vec!["-q".to_string(), "-g".to_string()];
        let opts = parse_ntpd_args(&args).unwrap();
        assert!(opts.set_and_exit);
        assert!(opts.allow_first_step);
    }

    #[test]
    fn test_parse_ntpd_config_path() {
        let args = vec!["-c".to_string(), "/custom/ntp.conf".to_string()];
        let opts = parse_ntpd_args(&args).unwrap();
        assert_eq!(opts.config_path, "/custom/ntp.conf");
    }

    // -- Discipline step vs slew -------------------------------------------

    #[test]
    fn test_discipline_step_large_offset() {
        let mut disc = ClockDiscipline::new(0.0);
        let (step, correction) = disc.update(500_000, true);
        assert!(step);
        assert_eq!(correction, 500_000);
    }

    #[test]
    fn test_discipline_slew_small_offset() {
        let mut disc = ClockDiscipline::new(0.0);
        let (step, correction) = disc.update(10_000, true);
        assert!(!step);
        // Should be proportional: ~10_000/4 = 2_500.
        assert!(correction != 0);
    }

    #[test]
    fn test_discipline_panic_threshold() {
        let mut disc = ClockDiscipline::new(0.0);
        let (step, correction) = disc.update(PANIC_THRESHOLD_US + 1, true);
        assert!(!step);
        assert_eq!(correction, 0);
    }

    #[test]
    fn test_discipline_no_step_when_disallowed() {
        let mut disc = ClockDiscipline::new(0.0);
        let (step, _correction) = disc.update(500_000, false);
        // Even though offset > threshold, step is disallowed.
        assert!(!step);
    }

    // -- Validate response --------------------------------------------------

    #[test]
    fn test_validate_wrong_mode() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 3, stratum: 2, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0, ref_id: [0; 4],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::new(3_900_000_000, 0),
        };
        assert!(pkt.validate_response().is_err());
    }

    #[test]
    fn test_validate_zero_transmit() {
        let pkt = NtpPacket {
            li: 0, vn: 4, mode: 4, stratum: 2, poll: 6, precision: -20,
            root_delay: 0, root_dispersion: 0, ref_id: [0; 4],
            ref_ts: NtpTimestamp::default(),
            origin_ts: NtpTimestamp::default(),
            receive_ts: NtpTimestamp::default(),
            transmit_ts: NtpTimestamp::default(),
        };
        assert!(pkt.validate_response().is_err());
    }
}
