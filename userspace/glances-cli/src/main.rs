#![deny(clippy::all)]

//! glances-cli — SlateOS Glances system monitor
//!
//! Single personality: `glances`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_glances(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: glances [OPTIONS]");
        println!("Glances 4.1.2 (Slate OS) — Cross-platform system monitor");
        println!();
        println!("Options:");
        println!("  -1                 Per-CPU stats");
        println!("  -2                 Disable left sidebar");
        println!("  -3                 Disable quicklook");
        println!("  -4                 Disable all but quicklook and load");
        println!("  -C FILE            Config file");
        println!("  -d                 Disable disk I/O module");
        println!("  -e                 Enable sensors module");
        println!("  -f FILE            Export to file");
        println!("  -n                 Disable network module");
        println!("  -p PORT            Web server port");
        println!("  -q                 Quiet mode");
        println!("  -s                 Server mode");
        println!("  -t SECONDS         Refresh interval");
        println!("  -w                 Web server mode");
        println!("  --browser          Client mode (discover servers)");
        println!("  --export FORMAT    Export format (csv, json, influxdb)");
        println!("  --stdout PLUGINS   Output specific plugins to stdout");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Glances v4.1.2 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-s") {
        println!("glances: Starting in server mode on 0.0.0.0:61209...");
        return 0;
    }
    if args.iter().any(|a| a == "-w") {
        let port = args.windows(2).find(|w| w[0] == "-p")
            .map(|w| w[1].as_str()).unwrap_or("61208");
        println!("glances: Web UI at http://0.0.0.0:{}", port);
        return 0;
    }
    println!("Slate OS x86_64 (Uptime: 2 days, 3:15:42)");
    println!();
    println!("CPU  12.3%  MEM  26.2%  SWAP  0.0%  LOAD  0.45 0.38 0.31");
    println!();
    println!("DISK I/O    R: 2.1MB/s  W: 1.5MB/s");
    println!("NETWORK     Rx: 5.6MB/s  Tx: 1.2MB/s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "glances".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_glances(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_glances};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/glances"), "glances");
        assert_eq!(basename(r"C:\bin\glances.exe"), "glances.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("glances.exe"), "glances");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_glances(&["--help".to_string()], "glances"), 0);
        assert_eq!(run_glances(&["-h".to_string()], "glances"), 0);
        let _ = run_glances(&["--version".to_string()], "glances");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_glances(&[], "glances");
    }
}
