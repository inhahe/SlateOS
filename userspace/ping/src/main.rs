//! SlateOS ICMP Ping Utility
//!
//! Sends ICMP echo requests to a host and displays round-trip times.
//! Supports hostname resolution via `SYS_DNS_RESOLVE`, configurable
//! intervals, timeouts, payload sizes, flood mode, quiet mode, and
//! JSON output.
//!
//! # Usage
//!
//! ```text
//! ping <host>                   Continuously ping host
//! ping -c 5 <host>             Send 5 pings then stop
//! ping -i 0.5 <host>           Ping every 0.5 seconds
//! ping -W 2 <host>             2-second reply timeout
//! ping -s 120 <host>           120-byte payload
//! ping -q <host>               Quiet mode (summary only)
//! ping -f <host>               Flood mode (fast as possible)
//! ping -n <host>               Numeric only, skip reverse DNS
//! ping --json <host>           JSON output per reply
//! ```

use std::env;
use std::io::{self, Write};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Syscall numbers
// ============================================================================

/// Send an ICMP echo request.
/// arg1 = ip_addr (u32, network byte order),
/// arg2 = sequence number,
/// arg3 = payload size in bytes.
/// Returns 0 on success, negative on error.
const SYS_ICMP_PING: u64 = 830;

/// Wait for an ICMP echo reply.
/// arg1 = timeout in milliseconds.
/// Returns RTT in microseconds on success, negative on error.
const SYS_ICMP_PING_WAIT: u64 = 831;

/// Resolve a hostname to an IPv4 address.
/// arg1 = pointer to hostname string,
/// arg2 = hostname length,
/// arg3 = pointer to u32 result (network byte order).
/// Returns 0 on success, negative on error.
const SYS_DNS_RESOLVE: u64 = 820;

// ============================================================================
// Syscall interface
// ============================================================================

/// Issue a 3-argument syscall via the x86_64 `syscall` instruction.
///
/// # Safety
///
/// The caller must ensure:
/// - `nr` is a valid syscall number.
/// - Arguments are valid for the specific syscall (e.g., pointers must be
///   readable/writable as required, sizes must be accurate).
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid for the given syscall.
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

// ============================================================================
// Syscall wrappers
// ============================================================================

/// Send an ICMP echo request to `ip_addr` with the given sequence number
/// and payload size.
fn icmp_ping(ip_addr: u32, seq: u16, payload_size: u16) -> Result<(), i64> {
    // SAFETY: SYS_ICMP_PING takes three scalar arguments; no pointer
    // dereferences occur in userspace.
    let ret = unsafe {
        syscall3(
            SYS_ICMP_PING,
            u64::from(ip_addr),
            u64::from(seq),
            u64::from(payload_size),
        )
    };
    if ret < 0 { Err(ret) } else { Ok(()) }
}

/// Wait for an ICMP echo reply with the given timeout (in milliseconds).
/// Returns the round-trip time in microseconds on success.
fn icmp_ping_wait(timeout_ms: u64) -> Result<u64, i64> {
    // SAFETY: SYS_ICMP_PING_WAIT takes one scalar argument; no pointer
    // dereferences occur in userspace.
    let ret = unsafe { syscall3(SYS_ICMP_PING_WAIT, timeout_ms, 0, 0) };
    if ret < 0 {
        Err(ret)
    } else {
        Ok(ret as u64)
    }
}

/// Resolve a hostname to an IPv4 address (network byte order u32).
fn dns_resolve(hostname: &str) -> Result<u32, i64> {
    let mut result_ip: u32 = 0;
    let name_ptr = hostname.as_ptr() as u64;
    let name_len = hostname.len() as u64;
    let result_ptr = &mut result_ip as *mut u32 as u64;

    // SAFETY: name_ptr points to a valid UTF-8 string of length name_len,
    // and result_ptr points to a stack-allocated u32 with sufficient lifetime.
    let ret = unsafe { syscall3(SYS_DNS_RESOLVE, name_ptr, name_len, result_ptr) };
    if ret < 0 { Err(ret) } else { Ok(result_ip) }
}

// ============================================================================
// IP address parsing and formatting
// ============================================================================

/// Parse a dotted-decimal IPv4 address string into a u32 in network byte
/// order (big-endian). Returns `None` if the string is malformed.
fn parse_ipv4(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }

    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        let val: u16 = part.parse().ok()?;
        if val > 255 {
            return None;
        }
        octets[i] = val as u8;
    }

    // Network byte order: most significant octet first.
    Some(u32::from_be_bytes(octets))
}

/// Format a u32 IP address (network byte order) as a dotted-decimal string.
fn format_ipv4(ip: u32) -> String {
    let octets = ip.to_be_bytes();
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

/// Returns true if the string looks like a dotted-decimal IPv4 address
/// (four groups of digits separated by dots).
fn is_ipv4_address(s: &str) -> bool {
    parse_ipv4(s).is_some()
}

// ============================================================================
// Syscall error descriptions
// ============================================================================

/// Map a negative syscall return value to a human-readable message.
fn syscall_error_msg(code: i64) -> &'static str {
    match code {
        -1 => "operation not permitted",
        -2 => "no such host",
        -11 => "resource temporarily unavailable",
        -13 => "permission denied",
        -22 => "invalid argument",
        -99 => "cannot assign requested address",
        -101 => "network is unreachable",
        -110 => "connection timed out",
        -111 => "connection refused",
        -113 => "no route to host",
        _ => "unknown error",
    }
}

// ============================================================================
// Statistics tracker
// ============================================================================

struct PingStats {
    transmitted: u64,
    received: u64,
    rtt_min_us: u64,
    rtt_max_us: u64,
    rtt_sum_us: u64,
    /// Sum of squared RTT values (for standard deviation calculation).
    rtt_sum_sq_us: u128,
    start_time: Instant,
}

impl PingStats {
    fn new() -> Self {
        Self {
            transmitted: 0,
            received: 0,
            rtt_min_us: u64::MAX,
            rtt_max_us: 0,
            rtt_sum_us: 0,
            rtt_sum_sq_us: 0,
            start_time: Instant::now(),
        }
    }

    fn record_sent(&mut self) {
        self.transmitted = self.transmitted.saturating_add(1);
    }

    fn record_received(&mut self, rtt_us: u64) {
        self.received = self.received.saturating_add(1);
        if rtt_us < self.rtt_min_us {
            self.rtt_min_us = rtt_us;
        }
        if rtt_us > self.rtt_max_us {
            self.rtt_max_us = rtt_us;
        }
        self.rtt_sum_us = self.rtt_sum_us.saturating_add(rtt_us);
        self.rtt_sum_sq_us = self.rtt_sum_sq_us.saturating_add(u128::from(rtt_us) * u128::from(rtt_us));
    }

    fn loss_percent(&self) -> f64 {
        if self.transmitted == 0 {
            return 0.0;
        }
        let lost = self.transmitted.saturating_sub(self.received);
        (lost as f64 / self.transmitted as f64) * 100.0
    }

    fn rtt_avg_us(&self) -> f64 {
        if self.received == 0 {
            return 0.0;
        }
        self.rtt_sum_us as f64 / self.received as f64
    }

    /// Standard deviation of RTT in microseconds, computed via
    /// sqrt(E[x^2] - (E[x])^2).
    fn rtt_mdev_us(&self) -> f64 {
        if self.received < 2 {
            return 0.0;
        }
        let n = self.received as f64;
        let mean = self.rtt_avg_us();
        let mean_sq = self.rtt_sum_sq_us as f64 / n;
        let variance = mean_sq - (mean * mean);
        // Clamp to zero in case of floating-point imprecision.
        if variance < 0.0 { 0.0 } else { variance.sqrt() }
    }

    fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }
}

// ============================================================================
// CLI options
// ============================================================================

struct Options {
    host: String,
    count: Option<u64>,
    interval_ms: u64,
    timeout_ms: u64,
    payload_size: u16,
    quiet: bool,
    flood: bool,
    numeric: bool,
    ttl: Option<u32>,
    json: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            host: String::new(),
            count: None,
            interval_ms: 1000,
            timeout_ms: 5000,
            payload_size: 56,
            quiet: false,
            flood: false,
            numeric: false,
            ttl: None,
            json: false,
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

fn print_usage() {
    eprintln!("Usage: ping [OPTIONS] <host>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -c <count>      Stop after <count> pings");
    eprintln!("  -i <interval>   Seconds between pings (default: 1.0)");
    eprintln!("  -W <timeout>    Reply timeout in seconds (default: 5)");
    eprintln!("  -s <size>       Payload size in bytes (default: 56)");
    eprintln!("  -q              Quiet mode (summary only)");
    eprintln!("  -f              Flood mode (send as fast as possible)");
    eprintln!("  -n              Numeric only, skip reverse DNS");
    eprintln!("  -4              Force IPv4 (default)");
    eprintln!("  -t <ttl>        Set TTL (informational)");
    eprintln!("  --json          JSON output for each reply");
    eprintln!("  -h, --help      Display this help message");
}

fn parse_args() -> Result<Options, String> {
    let argv: Vec<String> = env::args().collect();
    let mut opts = Options::default();
    let mut positionals: Vec<String> = Vec::new();

    if argv.len() < 2 {
        return Err("missing host operand".to_string());
    }

    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                process::exit(0);
            }
            "-c" => {
                i += 1;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-c requires a count value".to_string())?;
                let count: u64 = val
                    .parse()
                    .map_err(|_| format!("invalid count: '{val}'"))?;
                if count == 0 {
                    return Err("count must be greater than 0".to_string());
                }
                opts.count = Some(count);
            }
            "-i" => {
                i += 1;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-i requires an interval value".to_string())?;
                let secs: f64 = val
                    .parse()
                    .map_err(|_| format!("invalid interval: '{val}'"))?;
                if secs < 0.0 {
                    return Err("interval must be non-negative".to_string());
                }
                opts.interval_ms = (secs * 1000.0) as u64;
            }
            "-W" => {
                i += 1;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-W requires a timeout value".to_string())?;
                let secs: f64 = val
                    .parse()
                    .map_err(|_| format!("invalid timeout: '{val}'"))?;
                if secs <= 0.0 {
                    return Err("timeout must be positive".to_string());
                }
                opts.timeout_ms = (secs * 1000.0) as u64;
            }
            "-s" => {
                i += 1;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-s requires a size value".to_string())?;
                let size: u16 = val
                    .parse()
                    .map_err(|_| format!("invalid payload size: '{val}'"))?;
                opts.payload_size = size;
            }
            "-t" => {
                i += 1;
                let val = argv
                    .get(i)
                    .ok_or_else(|| "-t requires a TTL value".to_string())?;
                let ttl: u32 = val
                    .parse()
                    .map_err(|_| format!("invalid TTL: '{val}'"))?;
                if ttl == 0 || ttl > 255 {
                    return Err("TTL must be between 1 and 255".to_string());
                }
                opts.ttl = Some(ttl);
            }
            "-q" => opts.quiet = true,
            "-f" => opts.flood = true,
            "-n" => opts.numeric = true,
            "-4" => { /* IPv4 is the default; accept and ignore. */ }
            "--json" => opts.json = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown option: '{other}'"));
            }
            _ => {
                positionals.push(arg.clone());
            }
        }
        i += 1;
    }

    if positionals.is_empty() {
        return Err("missing host operand".to_string());
    }
    if positionals.len() > 1 {
        return Err("too many positional arguments".to_string());
    }
    opts.host = positionals.into_iter().next().unwrap_or_default();

    // Flood mode overrides interval to 0.
    if opts.flood {
        opts.interval_ms = 0;
    }

    Ok(opts)
}

// ============================================================================
// Global running flag for Ctrl+C handling
// ============================================================================

static RUNNING: AtomicBool = AtomicBool::new(true);

/// Install a Ctrl+C handler that clears the RUNNING flag.
fn install_signal_handler() {
    // Use a simple approach: spawn a thread that waits for Ctrl+C via
    // the libc signal mechanism. On our OS, std provides signal support
    // through the POSIX layer.
    //
    // We register an atomic flag rather than using std::process::exit
    // so the main loop can print the summary before exiting.
    #[cfg(target_family = "unix")]
    {
        // SAFETY: We are installing a signal handler for SIGINT (2).
        // The handler function only performs an atomic store, which is
        // async-signal-safe.
        unsafe {
            libc_signal(2, signal_handler as *const () as usize);
        }
    }
}

/// Minimal POSIX signal registration without depending on the `libc` crate.
///
/// # Safety
///
/// `handler` must be a valid function pointer suitable for use as a signal
/// handler (it may only call async-signal-safe functions).
#[cfg(target_family = "unix")]
unsafe fn libc_signal(signum: i32, handler: usize) {
    // SAFETY: signum (SIGINT=2) is a valid signal, and handler is a valid
    // function pointer that performs only an atomic store.
    // SYS_rt_sigaction is 13 on x86_64 Linux.
    // For simplicity, we use the old-style signal(2) via SYS_signal (not
    // available on modern Linux). Instead we call the C library's signal()
    // through an extern declaration.
    // SAFETY: signal() is a standard POSIX function. The extern block
    // declares its signature correctly: sig is a valid signal number,
    // handler is a function pointer (passed as usize).
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    // SAFETY: signal() is async-signal-safe for installation, and handler
    // points to a function that only does an atomic store.
    unsafe {
        signal(signum, handler);
    }
}

/// Signal handler: sets the RUNNING flag to false.
///
/// This function only performs an atomic store, which is async-signal-safe.
#[cfg(target_family = "unix")]
extern "C" fn signal_handler(_sig: i32) {
    RUNNING.store(false, Ordering::SeqCst);
}

// ============================================================================
// Output formatting
// ============================================================================

/// Format microseconds as a millisecond string with 3 decimal places.
fn format_rtt_ms(us: u64) -> String {
    let ms = us / 1000;
    let frac = us % 1000;
    format!("{ms}.{frac:03}")
}

/// Print a single ping reply line in standard format.
fn print_reply(
    stdout: &mut io::StdoutLock<'_>,
    ip_str: &str,
    seq: u16,
    ttl: u32,
    rtt_us: u64,
    payload_size: u16,
) {
    let total_size = u32::from(payload_size) + 8; // ICMP header is 8 bytes.
    let _ = writeln!(
        stdout,
        "{total_size} bytes from {ip_str}: icmp_seq={seq} ttl={ttl} time={} ms",
        format_rtt_ms(rtt_us),
    );
}

/// Print a single ping reply in JSON format.
fn print_reply_json(
    stdout: &mut io::StdoutLock<'_>,
    ip_str: &str,
    seq: u16,
    ttl: u32,
    rtt_us: u64,
    payload_size: u16,
) {
    let total_size = u32::from(payload_size) + 8;
    let rtt_ms_float = rtt_us as f64 / 1000.0;
    let _ = writeln!(
        stdout,
        "{{\"bytes\":{total_size},\"from\":\"{ip_str}\",\"icmp_seq\":{seq},\"ttl\":{ttl},\"time_ms\":{rtt_ms_float:.3}}}",
    );
}

/// Print a timeout line.
fn print_timeout(stdout: &mut io::StdoutLock<'_>, seq: u16) {
    let _ = writeln!(stdout, "Request timeout for icmp_seq {seq}");
}

/// Print a timeout line in JSON format.
fn print_timeout_json(stdout: &mut io::StdoutLock<'_>, seq: u16) {
    let _ = writeln!(
        stdout,
        "{{\"icmp_seq\":{seq},\"error\":\"timeout\"}}",
    );
}

/// Print the header line.
fn print_header(
    stdout: &mut io::StdoutLock<'_>,
    host: &str,
    ip_str: &str,
    payload_size: u16,
) {
    let _ = writeln!(
        stdout,
        "PING {host} ({ip_str}) {payload_size} data bytes",
    );
}

/// Print the summary statistics.
fn print_summary(
    stdout: &mut io::StdoutLock<'_>,
    host: &str,
    stats: &PingStats,
    json: bool,
) {
    if json {
        let loss = stats.loss_percent();
        let elapsed = stats.elapsed_ms();
        let _ = write!(
            stdout,
            "{{\"host\":\"{host}\",\"transmitted\":{},\"received\":{},\"loss_percent\":{loss:.1},\"time_ms\":{elapsed}",
            stats.transmitted,
            stats.received,
        );
        if stats.received > 0 {
            let min_ms = stats.rtt_min_us as f64 / 1000.0;
            let avg_ms = stats.rtt_avg_us() / 1000.0;
            let max_ms = stats.rtt_max_us as f64 / 1000.0;
            let mdev_ms = stats.rtt_mdev_us() / 1000.0;
            let _ = write!(
                stdout,
                ",\"rtt_min_ms\":{min_ms:.3},\"rtt_avg_ms\":{avg_ms:.3},\"rtt_max_ms\":{max_ms:.3},\"rtt_mdev_ms\":{mdev_ms:.3}",
            );
        }
        let _ = writeln!(stdout, "}}");
        return;
    }

    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "--- {host} ping statistics ---");
    let loss = stats.loss_percent();
    let elapsed = stats.elapsed_ms();
    let _ = writeln!(
        stdout,
        "{} packets transmitted, {} received, {loss:.0}% packet loss, time {elapsed}ms",
        stats.transmitted,
        stats.received,
    );

    if stats.received > 0 {
        let min_ms = format_rtt_ms(stats.rtt_min_us);
        let avg_ms_f = stats.rtt_avg_us() / 1000.0;
        let max_ms = format_rtt_ms(stats.rtt_max_us);
        let mdev_ms_f = stats.rtt_mdev_us() / 1000.0;
        let _ = writeln!(
            stdout,
            "rtt min/avg/max/mdev = {min_ms}/{avg_ms_f:.3}/{max_ms}/{mdev_ms_f:.3} ms",
        );
    }
}

// ============================================================================
// Main ping loop
// ============================================================================

fn run() -> Result<(), String> {
    let opts = parse_args()?;

    // Resolve host to IP address.
    let ip_addr: u32;
    let ip_str: String;

    if is_ipv4_address(&opts.host) {
        ip_addr = parse_ipv4(&opts.host)
            .ok_or_else(|| format!("invalid IP address: '{}'", opts.host))?;
        ip_str = opts.host.clone();
    } else {
        // Hostname: resolve via DNS syscall.
        ip_addr = dns_resolve(&opts.host).map_err(|e| {
            format!(
                "cannot resolve '{}': {} (error {})",
                opts.host,
                syscall_error_msg(e),
                e
            )
        })?;
        ip_str = format_ipv4(ip_addr);
    }

    // Default TTL to display (we may not be able to control it via syscall).
    let display_ttl = opts.ttl.unwrap_or(64);

    if opts.ttl.is_some() {
        // Informational: our syscall may not support setting TTL.
        // We note this but do not error out.
        let _ = writeln!(
            io::stderr(),
            "ping: warning: TTL setting is informational only; the kernel may not honor it"
        );
    }

    install_signal_handler();

    let stdout_handle = io::stdout();
    let mut stdout = stdout_handle.lock();
    let mut stats = PingStats::new();

    if !opts.quiet && !opts.json {
        print_header(&mut stdout, &opts.host, &ip_str, opts.payload_size);
    }

    let mut seq: u16 = 1;

    while RUNNING.load(Ordering::SeqCst) {
        // Check count limit.
        if let Some(max) = opts.count
            && stats.transmitted >= max {
                break;
            }

        // Send ICMP echo request.
        stats.record_sent();
        if let Err(e) = icmp_ping(ip_addr, seq, opts.payload_size) {
            if !opts.quiet {
                if opts.json {
                    let _ = writeln!(
                        stdout,
                        "{{\"icmp_seq\":{seq},\"error\":\"{} (error {e})\"}}",
                        syscall_error_msg(e),
                    );
                } else {
                    let _ = writeln!(
                        stdout,
                        "From {ip_str}: icmp_seq={seq} {}: error {e}",
                        syscall_error_msg(e),
                    );
                }
            }
            let _ = stdout.flush();
            seq = seq.wrapping_add(1);

            // Sleep before next attempt unless flood mode.
            if opts.interval_ms > 0 {
                sleep_interruptible(opts.interval_ms);
            }
            continue;
        }

        // Wait for reply.
        match icmp_ping_wait(opts.timeout_ms) {
            Ok(rtt_us) => {
                stats.record_received(rtt_us);
                if opts.flood {
                    // Flood mode: print backspace (erase the dot for sent).
                    let _ = write!(stdout, "\x08");
                } else if !opts.quiet {
                    if opts.json {
                        print_reply_json(
                            &mut stdout,
                            &ip_str,
                            seq,
                            display_ttl,
                            rtt_us,
                            opts.payload_size,
                        );
                    } else {
                        print_reply(
                            &mut stdout,
                            &ip_str,
                            seq,
                            display_ttl,
                            rtt_us,
                            opts.payload_size,
                        );
                    }
                }
            }
            Err(_) => {
                // Timeout or error waiting for reply.
                if opts.flood {
                    // In flood mode, no output for lost packets.
                } else if !opts.quiet {
                    if opts.json {
                        print_timeout_json(&mut stdout, seq);
                    } else {
                        print_timeout(&mut stdout, seq);
                    }
                }
            }
        }

        if opts.flood {
            // Flood mode: print a dot for each sent packet.
            let _ = write!(stdout, ".");
        }

        let _ = stdout.flush();
        seq = seq.wrapping_add(1);

        // Sleep between pings unless flood mode or we have reached the count.
        if opts.interval_ms > 0 {
            if let Some(max) = opts.count
                && stats.transmitted >= max {
                    break;
                }
            sleep_interruptible(opts.interval_ms);
        }
    }

    if opts.flood {
        let _ = writeln!(stdout);
    }

    // Print summary.
    print_summary(&mut stdout, &opts.host, &stats, opts.json);
    let _ = stdout.flush();

    // Exit with non-zero status if any packets were lost.
    if stats.received == 0 && stats.transmitted > 0 {
        process::exit(1);
    }
    if stats.received < stats.transmitted {
        process::exit(1);
    }

    Ok(())
}

/// Sleep for `ms` milliseconds, checking the RUNNING flag periodically
/// so Ctrl+C is responsive.
fn sleep_interruptible(ms: u64) {
    // Check every 100ms so we respond to Ctrl+C within ~100ms.
    let chunk = Duration::from_millis(100);
    let total = Duration::from_millis(ms);
    let start = Instant::now();

    while start.elapsed() < total {
        if !RUNNING.load(Ordering::SeqCst) {
            return;
        }
        let remaining = total.saturating_sub(start.elapsed());
        let sleep_time = if remaining < chunk { remaining } else { chunk };
        if sleep_time.is_zero() {
            break;
        }
        thread::sleep(sleep_time);
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ping: {e}");
        print_usage();
        process::exit(2);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- IPv4 parsing ---

    #[test]
    fn parse_ipv4_simple() {
        let ip = parse_ipv4("192.168.1.1");
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), 0xC0A80101);
    }

    #[test]
    fn parse_ipv4_loopback() {
        let ip = parse_ipv4("127.0.0.1");
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), 0x7F000001);
    }

    #[test]
    fn parse_ipv4_zeros() {
        let ip = parse_ipv4("0.0.0.0");
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), 0);
    }

    #[test]
    fn parse_ipv4_broadcast() {
        let ip = parse_ipv4("255.255.255.255");
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), 0xFFFFFFFF);
    }

    #[test]
    fn parse_ipv4_invalid_too_few_parts() {
        assert!(parse_ipv4("192.168.1").is_none());
    }

    #[test]
    fn parse_ipv4_invalid_too_many_parts() {
        assert!(parse_ipv4("192.168.1.1.5").is_none());
    }

    #[test]
    fn parse_ipv4_invalid_octet_overflow() {
        assert!(parse_ipv4("256.0.0.1").is_none());
    }

    #[test]
    fn parse_ipv4_invalid_non_numeric() {
        assert!(parse_ipv4("abc.def.ghi.jkl").is_none());
    }

    #[test]
    fn parse_ipv4_empty() {
        assert!(parse_ipv4("").is_none());
    }

    // --- IPv4 formatting ---

    #[test]
    fn format_ipv4_loopback() {
        assert_eq!(format_ipv4(0x7F000001), "127.0.0.1");
    }

    #[test]
    fn format_ipv4_private() {
        assert_eq!(format_ipv4(0xC0A80101), "192.168.1.1");
    }

    #[test]
    fn format_ipv4_zeros() {
        assert_eq!(format_ipv4(0), "0.0.0.0");
    }

    #[test]
    fn format_ipv4_broadcast() {
        assert_eq!(format_ipv4(0xFFFFFFFF), "255.255.255.255");
    }

    // --- Round-trip: parse then format ---

    #[test]
    fn ipv4_roundtrip() {
        let addrs = ["10.0.0.1", "172.16.254.1", "8.8.8.8", "1.1.1.1"];
        for addr in &addrs {
            let ip = parse_ipv4(addr).unwrap();
            assert_eq!(format_ipv4(ip), *addr);
        }
    }

    // --- is_ipv4_address ---

    #[test]
    fn is_ipv4_address_valid() {
        assert!(is_ipv4_address("1.2.3.4"));
        assert!(is_ipv4_address("255.255.255.255"));
    }

    #[test]
    fn is_ipv4_address_hostname() {
        assert!(!is_ipv4_address("example.com"));
        assert!(!is_ipv4_address("localhost"));
        assert!(!is_ipv4_address(""));
    }

    // --- RTT formatting ---

    #[test]
    fn format_rtt_ms_sub_ms() {
        // 543 us = 0.543 ms
        assert_eq!(format_rtt_ms(543), "0.543");
    }

    #[test]
    fn format_rtt_ms_exact_ms() {
        // 2000 us = 2.000 ms
        assert_eq!(format_rtt_ms(2000), "2.000");
    }

    #[test]
    fn format_rtt_ms_mixed() {
        // 1234 us = 1.234 ms
        assert_eq!(format_rtt_ms(1234), "1.234");
    }

    #[test]
    fn format_rtt_ms_zero() {
        assert_eq!(format_rtt_ms(0), "0.000");
    }

    #[test]
    fn format_rtt_ms_large() {
        // 123456 us = 123.456 ms
        assert_eq!(format_rtt_ms(123456), "123.456");
    }

    // --- Statistics ---

    #[test]
    fn stats_initial_state() {
        let stats = PingStats::new();
        assert_eq!(stats.transmitted, 0);
        assert_eq!(stats.received, 0);
        assert_eq!(stats.loss_percent(), 0.0);
        assert_eq!(stats.rtt_avg_us(), 0.0);
        assert_eq!(stats.rtt_mdev_us(), 0.0);
    }

    #[test]
    fn stats_all_received() {
        let mut stats = PingStats::new();
        stats.record_sent();
        stats.record_received(1000);
        stats.record_sent();
        stats.record_received(2000);
        stats.record_sent();
        stats.record_received(3000);

        assert_eq!(stats.transmitted, 3);
        assert_eq!(stats.received, 3);
        assert_eq!(stats.loss_percent(), 0.0);
        assert_eq!(stats.rtt_min_us, 1000);
        assert_eq!(stats.rtt_max_us, 3000);
        assert_eq!(stats.rtt_avg_us(), 2000.0);
    }

    #[test]
    fn stats_partial_loss() {
        let mut stats = PingStats::new();
        stats.record_sent();
        stats.record_received(500);
        stats.record_sent();
        // Second packet lost.
        stats.record_sent();
        stats.record_received(700);

        assert_eq!(stats.transmitted, 3);
        assert_eq!(stats.received, 2);
        // 1 out of 3 lost = 33.333...%
        let loss = stats.loss_percent();
        assert!((loss - 33.333).abs() < 0.5);
    }

    #[test]
    fn stats_total_loss() {
        let mut stats = PingStats::new();
        stats.record_sent();
        stats.record_sent();
        stats.record_sent();

        assert_eq!(stats.transmitted, 3);
        assert_eq!(stats.received, 0);
        assert_eq!(stats.loss_percent(), 100.0);
        assert_eq!(stats.rtt_avg_us(), 0.0);
    }

    #[test]
    fn stats_mdev_identical_values() {
        let mut stats = PingStats::new();
        for _ in 0..5 {
            stats.record_sent();
            stats.record_received(1000);
        }
        // All identical values: standard deviation should be 0.
        assert_eq!(stats.rtt_mdev_us(), 0.0);
    }

    #[test]
    fn stats_mdev_varied_values() {
        let mut stats = PingStats::new();
        let values = [100, 200, 300, 400, 500];
        for &v in &values {
            stats.record_sent();
            stats.record_received(v);
        }
        // Mean = 300, variance = ((100-300)^2 + (200-300)^2 + ... ) / 5
        // = (40000 + 10000 + 0 + 10000 + 40000) / 5 = 20000
        // mdev = sqrt(20000) ~= 141.42
        let mdev = stats.rtt_mdev_us();
        assert!((mdev - 141.42).abs() < 1.0);
    }

    #[test]
    fn stats_single_received() {
        let mut stats = PingStats::new();
        stats.record_sent();
        stats.record_received(500);

        assert_eq!(stats.rtt_min_us, 500);
        assert_eq!(stats.rtt_max_us, 500);
        assert_eq!(stats.rtt_avg_us(), 500.0);
        // mdev with single sample should be 0.
        assert_eq!(stats.rtt_mdev_us(), 0.0);
    }

    // --- Syscall error messages ---

    #[test]
    fn syscall_error_known_codes() {
        assert_eq!(syscall_error_msg(-1), "operation not permitted");
        assert_eq!(syscall_error_msg(-110), "connection timed out");
        assert_eq!(syscall_error_msg(-113), "no route to host");
    }

    #[test]
    fn syscall_error_unknown_code() {
        assert_eq!(syscall_error_msg(-9999), "unknown error");
    }

    // --- Sleep interruptible behavior ---

    #[test]
    fn sleep_interruptible_short() {
        // Verify it returns promptly for a 0ms sleep.
        let start = Instant::now();
        sleep_interruptible(0);
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 50);
    }

    #[test]
    fn sleep_interruptible_respects_running_flag() {
        // Clear the flag, sleep should return quickly.
        RUNNING.store(false, Ordering::SeqCst);
        let start = Instant::now();
        sleep_interruptible(10000); // 10 seconds
        let elapsed = start.elapsed();
        // Should return well under 10 seconds.
        assert!(elapsed.as_millis() < 500);
        // Restore the flag for other tests.
        RUNNING.store(true, Ordering::SeqCst);
    }
}
