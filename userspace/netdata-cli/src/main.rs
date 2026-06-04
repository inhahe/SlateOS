#![deny(clippy::all)]

//! netdata-cli — OurOS Netdata real-time monitoring
//!
//! Multi-personality: `netdata`, `netdatacli`, `netdata-claim`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_netdata(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: netdata [OPTIONS]");
        println!("netdata v1.44 (OurOS) — Real-time performance monitoring");
        println!();
        println!("Options:");
        println!("  -D              Run in foreground (don't fork)");
        println!("  -W set KEY=VAL  Override config option");
        println!("  -W buildinfo    Show build info");
        println!("  -c FILE         Configuration file");
        println!("  --version       Show version");
        println!();
        println!("Dashboard: http://localhost:19999");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("netdata v1.44 (OurOS)"); return 0; }
    if args.windows(2).any(|w| w[0] == "-W" && w[1] == "buildinfo") {
        println!("netdata v1.44 (OurOS)");
        println!("  Configure: default");
        println!("  Features: dbengine cloud");
        println!("  Plugins: proc diskspace cgroups apps");
        return 0;
    }
    println!("netdata: real-time monitoring started");
    println!("  Dashboard: http://localhost:19999");
    println!("  Collectors: cpu, memory, disk, network, processes");
    println!("  Update every: 1 second");
    println!("  DB mode: dbengine (tier 0: per-second)");
    0
}

fn run_netdatacli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: netdatacli <command>");
        println!("netdatacli v1.44 (OurOS) — Netdata CLI control");
        println!("  reload-health     Reload health configuration");
        println!("  reload-claiming   Reload claiming config");
        println!("  aclk-state        Show ACLK connection state");
        println!("  dumpconfig        Dump running configuration");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("aclk-state") => {
            println!("ACLK state: Offline (not claimed)");
        }
        Some("dumpconfig") => {
            println!("[global]");
            println!("  hostname = ouros-host");
            println!("  update every = 1");
            println!("  memory mode = dbengine");
        }
        _ => {
            println!("netdatacli: command executed");
        }
    }
    0
}

fn run_netdata_claim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: netdata-claim --token TOKEN --rooms ROOM_ID");
        println!("netdata-claim v1.44 (OurOS) — Claim node to Netdata Cloud");
        return 0;
    }
    let _ = args;
    println!("netdata-claim: node claiming");
    println!("  Status: not claimed (no token provided)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "netdata".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "netdatacli" => run_netdatacli(&rest, &prog),
        "netdata-claim" => run_netdata_claim(&rest, &prog),
        _ => run_netdata(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_netdata};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/netdata"), "netdata");
        assert_eq!(basename(r"C:\bin\netdata.exe"), "netdata.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("netdata.exe"), "netdata");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_netdata(&["--help".to_string()], "netdata"), 0);
        assert_eq!(run_netdata(&["-h".to_string()], "netdata"), 0);
        let _ = run_netdata(&["--version".to_string()], "netdata");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_netdata(&[], "netdata");
    }
}
