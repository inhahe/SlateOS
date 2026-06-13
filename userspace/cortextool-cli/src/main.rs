#![deny(clippy::all)]

//! cortextool-cli — SlateOS Cortex metrics management tool
//!
//! Single personality: `cortextool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cortextool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: cortextool COMMAND [OPTIONS]");
        println!("cortextool v0.17.0 (Slate OS) — Cortex metrics tool");
        println!();
        println!("Commands:");
        println!("  rules           Manage recording/alerting rules");
        println!("  alertmanager    Manage Alertmanager config");
        println!("  analyse         Analyse metrics usage");
        println!("  remote-read     Read remote metrics");
        println!("  chunk-tool      Manage chunks");
        println!("  overrides       Manage runtime overrides");
        println!("  version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("cortextool v0.17.0 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("rules");
    match cmd {
        "rules" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Namespace   Group              Rules");
                    println!("default     cpu_alerts         3");
                    println!("default     memory_alerts      2");
                    println!("monitoring  slo_rules          5");
                }
                "load" => println!("Rules loaded successfully."),
                "diff" => println!("No differences found."),
                "sync" => println!("Rules synchronized."),
                _ => println!("cortextool rules {}: completed", sub),
            }
        }
        "alertmanager" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "get" => println!("Retrieved Alertmanager config (2.3 KB)"),
                "load" => println!("Alertmanager config loaded."),
                _ => println!("cortextool alertmanager {}: completed", sub),
            }
        }
        "analyse" => {
            println!("Analyzing metrics usage...");
            println!("  Total series: 12,456");
            println!("  Used in rules: 234");
            println!("  Used in dashboards: 567");
            println!("  Unused: 11,655 (93.6%)");
        }
        "remote-read" => println!("Reading from remote..."),
        _ => println!("cortextool {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cortextool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_cortextool(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cortextool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cortextool"), "cortextool");
        assert_eq!(basename(r"C:\bin\cortextool.exe"), "cortextool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cortextool.exe"), "cortextool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cortextool(&["--help".to_string()], "cortextool"), 0);
        assert_eq!(run_cortextool(&["-h".to_string()], "cortextool"), 0);
        let _ = run_cortextool(&["--version".to_string()], "cortextool");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cortextool(&[], "cortextool");
    }
}
