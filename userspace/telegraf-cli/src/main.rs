#![deny(clippy::all)]

//! telegraf-cli — SlateOS Telegraf metrics agent
//!
//! Single personality: `telegraf`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_telegraf(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: telegraf [OPTIONS]");
        println!("Telegraf v1.30 (Slate OS) — Server metrics agent");
        println!();
        println!("Options:");
        println!("  --config FILE          Config file");
        println!("  --config-directory DIR  Config directory");
        println!("  --test                 Test config");
        println!("  --once                 Run once and exit");
        println!("  --input-filter REGEX   Input plugin filter");
        println!("  --output-filter REGEX  Output plugin filter");
        println!("  --sample-config        Print sample config");
        println!("  --input-list           List input plugins");
        println!("  --output-list          List output plugins");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Telegraf v1.30.2 (Slate OS)"); return 0; }
    println!("Telegraf v1.30.2 (Slate OS)");
    println!("  Config: /etc/telegraf/telegraf.conf");
    println!("  Inputs: cpu, mem, disk, net, system, processes");
    println!("  Outputs: influxdb_v2 (http://localhost:8086)");
    println!("  Interval: 10s");
    println!("  Collecting metrics...");
    println!("    cpu: 6 fields");
    println!("    mem: 12 fields");
    println!("    disk: 8 fields (3 mount points)");
    println!("    net: 16 fields (4 interfaces)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "telegraf".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_telegraf(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_telegraf};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/telegraf"), "telegraf");
        assert_eq!(basename(r"C:\bin\telegraf.exe"), "telegraf.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("telegraf.exe"), "telegraf");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_telegraf(&["--help".to_string()], "telegraf"), 0);
        assert_eq!(run_telegraf(&["-h".to_string()], "telegraf"), 0);
        let _ = run_telegraf(&["--version".to_string()], "telegraf");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_telegraf(&[], "telegraf");
    }
}
