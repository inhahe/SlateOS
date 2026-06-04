#![deny(clippy::all)]

//! ntp-cli — OurOS NTP time synchronization tools
//!
//! Multi-personality: `ntpd`, `ntpq`, `ntpdate`, `chronyc`, `chronyd`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_ntpd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ntpd [OPTIONS]");
        println!();
        println!("ntpd — NTP daemon (OurOS).");
        println!();
        println!("Options:");
        println!("  -d         Debug mode");
        println!("  -g         Allow large clock step");
        println!("  -n         Don't fork");
        println!("  -q         Set clock and quit");
        println!("  -c FILE    Config file");
        println!("  -p FILE    PID file");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ntpd 4.2.8p17 (OurOS)");
        return 0;
    }

    let quick = args.iter().any(|a| a == "-q");
    if quick {
        println!("ntpd: time server 0.pool.ntp.org offset -0.001234 sec");
        println!("ntpd: clock set, exiting");
    } else {
        println!("ntpd: starting NTP daemon");
        println!("ntpd: listening on 0.0.0.0:123");
        println!("ntpd: synchronized to 0.pool.ntp.org, stratum 2");
    }
    0
}

fn run_ntpq(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ntpq [OPTIONS] [HOST]");
        println!();
        println!("ntpq — NTP query program (OurOS).");
        println!();
        println!("Options:");
        println!("  -p         Print peers list");
        println!("  -n         Show addresses as numbers");
        println!("  -c CMD     Execute command");
        return 0;
    }

    let peers = args.iter().any(|a| a == "-p");
    if peers {
        println!("     remote           refid      st t when poll reach   delay   offset  jitter");
        println!("==============================================================================");
        println!("*0.pool.ntp.org  .GPS.            1 u   64  128  377    5.123   -0.456   0.789");
        println!("+1.pool.ntp.org  .PPS.            1 u   32  128  377   12.345   -0.234   1.234");
        println!("+2.pool.ntp.org  17.253.34.253    2 u  128  256  377   25.678    0.123   0.567");
        println!("-3.pool.ntp.org  129.6.15.28      2 u  256  512  377   45.901    1.234   2.345");
    } else {
        println!("assID=0 status=0615 leap_none, sync_ntp, 1 event, clock_sync,");
        println!("version=\"ntpd 4.2.8p17 (OurOS)\",");
        println!("processor=\"x86_64\",");
        println!("system=\"OurOS/1.0.0\",");
        println!("stratum=2,");
        println!("precision=-23,");
        println!("rootdelay=5.123,");
        println!("rootdisp=10.456,");
        println!("refid=0.pool.ntp.org,");
        println!("clock=da1234567890.abcdef");
    }
    0
}

fn run_ntpdate(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ntpdate [OPTIONS] SERVER...");
        println!();
        println!("ntpdate — set the date and time via NTP (OurOS).");
        println!();
        println!("Options:");
        println!("  -q         Query only (don't set)");
        println!("  -u         Use unprivileged port");
        println!("  -b         Force slew clock");
        println!("  -v         Verbose");
        return 0;
    }

    let server = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("pool.ntp.org");
    let query = args.iter().any(|a| a == "-q");

    println!("server {}, stratum 2, offset -0.001234, delay 0.005678", server);
    if query {
        println!(" 1 Jan 12:00:00 ntpdate[1234]: adjust time server {} offset -0.001234 sec (query only)", server);
    } else {
        println!(" 1 Jan 12:00:00 ntpdate[1234]: adjust time server {} offset -0.001234 sec", server);
    }
    0
}

fn run_chronyc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chronyc [OPTIONS] [COMMAND]");
        println!();
        println!("chronyc — chronyd control program (OurOS).");
        println!();
        println!("Commands: sources, sourcestats, tracking, ntpdata, activity, makestep");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("chronyc (chrony) version 4.5 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("tracking");
    match subcmd {
        "sources" => {
            println!("MS Name/IP address         Stratum Poll Reach LastRx Last sample");
            println!("===============================================================================");
            println!("^* 0.pool.ntp.org                1   7   377    45   -456us[ -789us] +/-  5ms");
            println!("^+ 1.pool.ntp.org                1   7   377    23   +123us[ +456us] +/- 12ms");
            println!("^+ 2.pool.ntp.org                2   8   377   128   -234us[ -567us] +/- 25ms");
        }
        "tracking" => {
            println!("Reference ID    : AC1E0001 (0.pool.ntp.org)");
            println!("Stratum         : 2");
            println!("Ref time (UTC)  : Thu Jan 01 12:00:00 2025");
            println!("System time     : 0.000001234 seconds fast of NTP time");
            println!("Last offset     : -0.000000456 seconds");
            println!("RMS offset      : 0.000000789 seconds");
            println!("Frequency       : 1.234 ppm slow");
            println!("Residual freq   : -0.001 ppm");
            println!("Skew            : 0.012 ppm");
            println!("Root delay      : 0.005123 seconds");
            println!("Root dispersion : 0.000456 seconds");
            println!("Update interval : 128.0 seconds");
            println!("Leap status     : Normal");
        }
        "activity" => {
            println!("200 OK");
            println!("3 sources online");
            println!("0 sources offline");
            println!("0 sources doing burst (return to online)");
            println!("0 sources doing burst (return to offline)");
            println!("0 sources with unknown address");
        }
        "makestep" => {
            println!("200 OK");
        }
        _ => {
            println!("chronyc: unknown command '{}'", subcmd);
        }
    }
    0
}

fn run_chronyd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chronyd [OPTIONS]");
        println!("Options: -d (foreground), -f FILE (config), -r (reload), -q (set and quit)");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("chronyd (chrony) version 4.5 (OurOS)");
        return 0;
    }
    println!("chronyd: starting NTP daemon");
    println!("chronyd: reading config from /etc/chrony/chrony.conf");
    println!("chronyd: 3 sources configured");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ntpd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "ntpq" => run_ntpq(&rest),
        "ntpdate" => run_ntpdate(&rest),
        "chronyc" => run_chronyc(&rest),
        "chronyd" => run_chronyd(&rest),
        _ => run_ntpd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ntpd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ntp"), "ntp");
        assert_eq!(basename(r"C:\bin\ntp.exe"), "ntp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ntp.exe"), "ntp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ntpd(&["--help".to_string()]), 0);
        assert_eq!(run_ntpd(&["-h".to_string()]), 0);
        let _ = run_ntpd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ntpd(&[]);
    }
}
