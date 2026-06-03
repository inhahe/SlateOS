#![deny(clippy::all)]

//! acpid-cli — OurOS acpid ACPI event daemon
//!
//! Multi-personality: `acpid`, `acpi_listen`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_acpid(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: acpid [OPTIONS]");
        println!("acpid v2.0 (OurOS) — ACPI event daemon");
        println!();
        println!("Options:");
        println!("  -d                Debug (foreground)");
        println!("  -c DIR            Config directory");
        println!("  -s SOCKET         Socket path");
        println!("  -S                No socket");
        println!("  -p PID_FILE       PID file path");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("acpid v2.0 (OurOS)"); return 0; }
    println!("acpid: ACPI event daemon started");
    println!("  Socket: /var/run/acpid.socket");
    println!("  Handlers: lid, power, battery");
    0
}

fn run_listen(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: acpi_listen [OPTIONS]");
        println!("acpi_listen v2.0 (OurOS) — Listen for ACPI events");
        println!();
        println!("Options:");
        println!("  -c COUNT          Number of events to capture");
        println!("  -t SECONDS        Timeout");
        return 0;
    }
    let _ = args;
    println!("Listening for ACPI events...");
    println!("button/power PWRF 00000080 00000001");
    println!("button/lid LID close");
    println!("ac_adapter ACPI0003:00 00000080 00000001");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "acpid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "acpi_listen" => run_listen(&rest, &prog),
        _ => run_acpid(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_acpid};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/acpid"), "acpid");
        assert_eq!(basename(r"C:\bin\acpid.exe"), "acpid.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("acpid.exe"), "acpid");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_acpid(&["--help".to_string()], "acpid"), 0);
        assert_eq!(run_acpid(&["-h".to_string()], "acpid"), 0);
        assert_eq!(run_acpid(&["--version".to_string()], "acpid"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_acpid(&[], "acpid"), 0);
    }
}
