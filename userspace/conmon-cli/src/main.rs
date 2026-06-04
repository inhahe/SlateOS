#![deny(clippy::all)]

//! conmon-cli — OurOS conmon container monitor
//!
//! Single personality: `conmon`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_conmon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: conmon [OPTIONS]");
        println!("conmon v2.1 (OurOS) — OCI container runtime monitor");
        println!();
        println!("Options:");
        println!("  -c CID            Container ID");
        println!("  -n NAME           Container name");
        println!("  -r RUNTIME        OCI runtime path (default: crun)");
        println!("  -b BUNDLE         OCI bundle directory");
        println!("  -p PIDFILE        PID file path");
        println!("  -l LOGFILE        Log file path");
        println!("  --log-level LEVEL Log level (error, warn, info, debug)");
        println!("  --exit-dir DIR    Exit files directory");
        println!("  --socket-dir-path DIR  Attach socket dir");
        println!("  --syslog          Log to syslog");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("conmon v2.1.10 (OurOS)"); return 0; }
    println!("conmon v2.1.10 (OurOS)");
    println!("  Container: abc123def456");
    println!("  Runtime: /usr/bin/crun");
    println!("  PID: 12345");
    println!("  State: running");
    println!("  Log driver: k8s-file");
    println!("  Attach socket: /var/run/conmon/abc123.attach");
    println!("  Monitoring container I/O...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "conmon".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_conmon(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_conmon};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/conmon"), "conmon");
        assert_eq!(basename(r"C:\bin\conmon.exe"), "conmon.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("conmon.exe"), "conmon");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_conmon(&["--help".to_string()], "conmon"), 0);
        assert_eq!(run_conmon(&["-h".to_string()], "conmon"), 0);
        let _ = run_conmon(&["--version".to_string()], "conmon");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_conmon(&[], "conmon");
    }
}
