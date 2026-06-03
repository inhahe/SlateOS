#![deny(clippy::all)]

//! trendmicro-cli — OurOS Trend Micro Maximum Security
//!
//! Single personality: `trendmicro`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: trendmicro [OPTIONS]");
        println!("Trend Micro Maximum Security 17.8 (OurOS) — Consumer + enterprise security");
        println!();
        println!("Options:");
        println!("  --scan TYPE            quick/full/custom");
        println!("  --pay-guard            Pay Guard secure browser");
        println!("  --vault                Vault encrypted folder");
        println!("  --vision-one           Trend Vision One XDR platform (enterprise)");
        println!("  --deep-security        Deep Security (server/cloud workload)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Trend Micro Maximum Security 17.8.1308 (OurOS)"); return 0; }
    println!("Trend Micro Maximum Security 17.8.1308 (OurOS)");
    println!("  Origin: Japan/US, founded 1988; Tokyo Stock Exchange listed");
    println!("  Consumer: AntiVirus+, Internet Security, Maximum Security, Premium Security Suite");
    println!("  Mobile: Trend Micro Mobile Security (Android/iOS)");
    println!("  Mac: Trend Micro Antivirus for Mac, ID Safe");
    println!("  Business: Trend Vision One (XDR), Apex One (endpoint), Deep Security");
    println!("  Cloud: Cloud One (workload, container, file storage, application, conformity)");
    println!("  Network: TippingPoint IPS, Deep Discovery (APT detection), Smart Protection Network");
    println!("  Engines: Smart Scan (cloud lookups), behavior monitoring, ML, sandbox analyzer");
    println!("  Features: AV, web threat protection, Pay Guard, parental, Vault, password mgr");
    println!("  License: annual subscription (consumer) + enterprise per-seat/per-VM");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "trendmicro".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/trendmicro"), "trendmicro");
        assert_eq!(basename(r"C:\bin\trendmicro.exe"), "trendmicro.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("trendmicro.exe"), "trendmicro");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tm(&["--help".to_string()], "trendmicro"), 0);
        assert_eq!(run_tm(&["-h".to_string()], "trendmicro"), 0);
        assert_eq!(run_tm(&["--version".to_string()], "trendmicro"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tm(&[], "trendmicro"), 0);
    }
}
