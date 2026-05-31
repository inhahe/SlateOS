#![deny(clippy::all)]

//! chrony — OurOS NTP implementation
//!
//! Multi-personality binary for time synchronization.
//! Detected via argv[0]:
//!
//! - `chronyd` (default) — chrony NTP daemon
//! - `chronyc` — chrony command-line client

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _CHRONY_CONF: &str = "/etc/chrony.conf";
const _CHRONY_DRIFT: &str = "/var/lib/chrony/drift";
const _CHRONY_KEYS: &str = "/etc/chrony.keys";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct NtpSource {
    name: String,
    mode: SourceMode,
    state: SourceState,
    stratum: u32,
    poll_interval: u32,
    reach: u16,
    last_rx_secs: u32,
    offset_us: f64,
    _delay_us: f64,
    _jitter_us: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SourceMode {
    Server,
    Peer,
    Refclock,
}

impl std::fmt::Display for SourceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Server => write!(f, "^"),
            Self::Peer => write!(f, "="),
            Self::Refclock => write!(f, "#"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SourceState {
    Synced,
    Reachable,
    _Unreachable,
    _Selected,
    _Combined,
    _NotCombined,
}

impl SourceState {
    fn marker(self) -> char {
        match self {
            Self::Synced => '*',
            Self::Reachable => '+',
            Self::_Unreachable => '?',
            Self::_Selected => '#',
            Self::_Combined => '-',
            Self::_NotCombined => 'x',
        }
    }
}

#[derive(Clone, Debug)]
struct TrackingInfo {
    reference: String,
    stratum: u32,
    ref_time_utc: String,
    system_time_offset: f64,
    last_offset: f64,
    rms_offset: f64,
    frequency: f64,
    _residual_freq: f64,
    _skew: f64,
    root_delay: f64,
    root_dispersion: f64,
    _update_interval: f64,
    _leap_status: LeapStatus,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LeapStatus {
    Normal,
    _InsertSecond,
    _DeleteSecond,
    _NotSynchronized,
}

impl std::fmt::Display for LeapStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::_InsertSecond => write!(f, "Insert second"),
            Self::_DeleteSecond => write!(f, "Delete second"),
            Self::_NotSynchronized => write!(f, "Not synchronised"),
        }
    }
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_sources() -> Vec<NtpSource> {
    vec![
        NtpSource {
            name: "time.cloudflare.com".to_string(),
            mode: SourceMode::Server,
            state: SourceState::Synced,
            stratum: 3, poll_interval: 6, reach: 377, last_rx_secs: 12,
            offset_us: -234.5, _delay_us: 15800.0, _jitter_us: 123.4,
        },
        NtpSource {
            name: "ntp.ubuntu.com".to_string(),
            mode: SourceMode::Server,
            state: SourceState::Reachable,
            stratum: 2, poll_interval: 6, reach: 377, last_rx_secs: 45,
            offset_us: 567.8, _delay_us: 32100.0, _jitter_us: 456.7,
        },
        NtpSource {
            name: "pool.ntp.org".to_string(),
            mode: SourceMode::Server,
            state: SourceState::Reachable,
            stratum: 2, poll_interval: 7, reach: 377, last_rx_secs: 120,
            offset_us: -89.1, _delay_us: 28500.0, _jitter_us: 234.5,
        },
    ]
}

fn read_tracking() -> TrackingInfo {
    TrackingInfo {
        reference: "time.cloudflare.com (162.159.200.123)".to_string(),
        stratum: 4,
        ref_time_utc: "Thu May 22 05:30:00 2025".to_string(),
        system_time_offset: 0.000234,
        last_offset: -0.000234,
        rms_offset: 0.000567,
        frequency: -12.345,
        _residual_freq: 0.001,
        _skew: 0.123,
        root_delay: 0.015800,
        root_dispersion: 0.000890,
        _update_interval: 64.0,
        _leap_status: LeapStatus::Normal,
    }
}

// ── chronyd personality ───────────────────────────────────────────────

fn run_chronyd(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "run".to_string());

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: chronyd [OPTIONS]");
            println!();
            println!("Chrony NTP daemon.");
            println!();
            println!("Options:");
            println!("  -f FILE     Configuration file (default: {})", _CHRONY_CONF);
            println!("  -d          Don't daemonize (run in foreground)");
            println!("  -q          Set clock once and exit");
            println!("  -Q          Print offset and exit (no set)");
            println!("  -r          Reload config");
            println!("  --version   Show version");
            0
        }
        "--version" | "-V" => { println!("chronyd 0.1.0 (OurOS)"); 0 }
        "-q" => {
            println!("chronyd: clock set mode");
            println!("  Querying time.cloudflare.com...");
            println!("  System clock offset: -0.000234 seconds");
            println!("  Clock adjusted");
            0
        }
        "-Q" => {
            println!("chronyd: query mode (no clock set)");
            println!("  System clock offset: -0.000234 seconds");
            0
        }
        _ => {
            let foreground = args.iter().any(|a| a == "-d");
            println!("chronyd: starting NTP daemon");
            println!("  Config: {}", _CHRONY_CONF);
            println!("  Mode: {}", if foreground { "foreground" } else { "daemon" });
            println!();
            let sources = read_sources();
            println!("  Loaded {} NTP sources", sources.len());
            for s in &sources {
                println!("    {} {}", s.mode, s.name);
            }
            println!();
            println!("chronyd: listening for NTP requests (simulated)");
            0
        }
    }
}

// ── chronyc personality ───────────────────────────────────────────────

fn run_chronyc(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "tracking".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: chronyc [COMMAND]");
            println!();
            println!("Chrony command-line client.");
            println!();
            println!("Commands:");
            println!("  tracking        Show system clock sync status (default)");
            println!("  sources [-v]    Show NTP sources");
            println!("  sourcestats     Show source statistics");
            println!("  ntpdata         Show NTP data for sources");
            println!("  activity        Show active/inactive sources");
            println!("  serverstats     Show NTP server stats");
            println!("  clients         Show NTP clients");
            println!("  makestep        Force clock step");
            println!("  burst <on|off>  Enable/disable burst mode");
            println!("  offline/online  Take sources offline/online");
            println!("  version         Show version");
            0
        }
        "version" | "--version" | "-V" => { println!("chronyc 0.1.0 (OurOS)"); 0 }
        "tracking" => chronyc_tracking(),
        "sources" => chronyc_sources(cmd_args.iter().any(|a| a == "-v")),
        "sourcestats" => chronyc_sourcestats(),
        "ntpdata" => chronyc_ntpdata(),
        "activity" => chronyc_activity(),
        "serverstats" => chronyc_serverstats(),
        "clients" => chronyc_clients(),
        "makestep" => {
            println!("200 OK");
            println!("chronyc: clock stepped by -0.000234 seconds");
            0
        }
        other => {
            eprintln!("chronyc: unknown command '{}'", other);
            1
        }
    }
}

fn chronyc_tracking() -> i32 {
    let t = read_tracking();
    println!("Reference ID    : 162.159.200.123 ({})", t.reference);
    println!("Stratum         : {}", t.stratum);
    println!("Ref time (UTC)  : {}", t.ref_time_utc);
    println!("System time     : {:.6} seconds fast of NTP time", t.system_time_offset);
    println!("Last offset     : {:+.6} seconds", t.last_offset);
    println!("RMS offset      : {:.6} seconds", t.rms_offset);
    println!("Frequency       : {:.3} ppm slow", t.frequency);
    println!("Residual freq   : {:.3} ppm", t._residual_freq);
    println!("Skew            : {:.3} ppm", t._skew);
    println!("Root delay      : {:.6} seconds", t.root_delay);
    println!("Root dispersion : {:.6} seconds", t.root_dispersion);
    println!("Update interval : {:.1} seconds", t._update_interval);
    println!("Leap status     : {}", t._leap_status);
    0
}

fn chronyc_sources(verbose: bool) -> i32 {
    let sources = read_sources();
    if verbose {
        println!("  .-- Source mode  '^' = server, '=' = peer, '#' = local clock");
        println!(" / .- Source state '*' = current best, '+' = combined");
        println!("| /   Reach = octal register of responses");
        println!("||");
    }
    println!("MS Name/IP address         Stratum Poll Reach LastRx Last sample");
    println!("===============================================================================");

    for s in &sources {
        println!("{}{} {:<25} {:>5} {:>4} {:>5} {:>6} {:>+8.1}us",
            s.mode, s.state.marker(), s.name,
            s.stratum, s.poll_interval, s.reach, s.last_rx_secs,
            s.offset_us);
    }
    0
}

fn chronyc_sourcestats() -> i32 {
    let sources = read_sources();
    println!("Name/IP Address            NP  NR  Span  Frequency  Freq Skew  Offset  Std Dev");
    println!("===============================================================================");

    for s in &sources {
        println!("{:<25} {:>4} {:>3} {:>5} {:>+10.3} {:>10.3} {:>+8.1}us {:>7.1}us",
            s.name, 8, 4, "255m", -12.345, 0.123, s.offset_us, s._jitter_us);
    }
    0
}

fn chronyc_ntpdata() -> i32 {
    let sources = read_sources();
    for s in &sources {
        println!("Remote address  : {} ({})", s.name, s.name);
        println!("Remote port     : 123");
        println!("Local address   : 0.0.0.0 (0.0.0.0)");
        println!("Leap status     : Normal");
        println!("Version         : 4");
        println!("Mode            : Server");
        println!("Stratum         : {}", s.stratum);
        println!("Poll interval   : {} ({}s)", s.poll_interval, 1 << s.poll_interval);
        println!("Offset          : {:+.6} seconds", s.offset_us / 1_000_000.0);
        println!();
    }
    0
}

fn chronyc_activity() -> i32 {
    let sources = read_sources();
    let online = sources.len();
    println!("{} sources online", online);
    println!("0 sources offline");
    println!("0 sources doing burst (return to online)");
    println!("0 sources doing burst (return to offline)");
    println!("0 sources with unknown address");
    0
}

fn chronyc_serverstats() -> i32 {
    println!("NTP packets received       : 12345");
    println!("NTP packets dropped        : 0");
    println!("Command packets received   : 678");
    println!("Command packets dropped    : 0");
    println!("Client log records dropped : 0");
    0
}

fn chronyc_clients() -> i32 {
    println!("Hostname                      NTP   Drop Int IntL Last     Cmd   Drop Int  Last");
    println!("===============================================================================");
    println!("localhost                        10      0   6    -     5       5      0  10    15");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("chronyd");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "chronyc" => run_chronyc(rest),
        _ => run_chronyd(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sources() {
        let sources = read_sources();
        assert_eq!(sources.len(), 3);
        assert!(sources.iter().any(|s| s.state == SourceState::Synced));
    }

    #[test]
    fn test_tracking() {
        let t = read_tracking();
        assert_eq!(t.stratum, 4);
        assert!(t.system_time_offset.abs() < 1.0);
    }

    #[test]
    fn test_source_mode_display() {
        assert_eq!(format!("{}", SourceMode::Server), "^");
        assert_eq!(format!("{}", SourceMode::Peer), "=");
    }

    #[test]
    fn test_source_state_marker() {
        assert_eq!(SourceState::Synced.marker(), '*');
        assert_eq!(SourceState::Reachable.marker(), '+');
        assert_eq!(SourceState::_Unreachable.marker(), '?');
    }

    #[test]
    fn test_leap_status_display() {
        assert_eq!(format!("{}", LeapStatus::Normal), "Normal");
        assert_eq!(format!("{}", LeapStatus::_NotSynchronized), "Not synchronised");
    }

    #[test]
    fn test_all_sources_reachable() {
        let sources = read_sources();
        for s in &sources {
            assert!(s.reach > 0);
        }
    }
}
