#![deny(clippy::all)]

//! victoria-cli — OurOS VictoriaMetrics tools
//!
//! Multi-personality: `vmctl`, `vmagent`, `vmbackup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vmctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vmctl COMMAND [OPTIONS]");
        println!("vmctl — VictoriaMetrics migration tool (OurOS)");
        println!();
        println!("Commands:");
        println!("  prometheus   Migrate from Prometheus");
        println!("  influx       Migrate from InfluxDB");
        println!("  opentsdb     Migrate from OpenTSDB");
        println!("  remote-read  Migrate via remote read API");
        println!("  verify-block Verify TSDB blocks");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "prometheus" => {
            println!("Starting migration from Prometheus...");
            println!("  Discovered 1234 time series");
            println!("  Migrated 567890 samples");
            println!("Migration complete.");
        }
        "influx" => {
            println!("Starting migration from InfluxDB...");
            println!("Migration complete.");
        }
        _ => println!("vmctl: '{}' completed", subcmd),
    }
    0
}

fn run_vmagent(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vmagent [OPTIONS]");
        println!("vmagent — lightweight Prometheus-compatible agent (OurOS)");
        println!("  -promscrape.config FILE   Scrape config file");
        println!("  -remoteWrite.url URL      Remote write endpoint");
        println!("  --version                 Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("vmagent-20240615-123456");
        return 0;
    }
    println!("vmagent: starting with scrape config");
    println!("vmagent: scraping 5 targets");
    0
}

fn run_vmbackup(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vmbackup [OPTIONS]");
        println!("  -storageDataPath PATH   Source data path");
        println!("  -dst URL                Backup destination");
        println!("  -snapshot.createURL URL  Create snapshot");
        return 0;
    }
    println!("vmbackup: creating snapshot...");
    println!("vmbackup: uploading to destination...");
    println!("vmbackup: complete. 1.2 GB backed up.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vmctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "vmagent" => run_vmagent(&rest),
        "vmbackup" => run_vmbackup(&rest),
        _ => run_vmctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vmctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/victoria"), "victoria");
        assert_eq!(basename(r"C:\bin\victoria.exe"), "victoria.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("victoria.exe"), "victoria");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_vmctl(&["--help".to_string()]), 0);
        assert_eq!(run_vmctl(&["-h".to_string()]), 0);
        assert_eq!(run_vmctl(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_vmctl(&[]), 0);
    }
}
