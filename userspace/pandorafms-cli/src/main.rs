#![deny(clippy::all)]

//! pandorafms-cli — OurOS Pandora FMS monitoring
//!
//! Multi-personality: `pandora_server`, `pandora_agent`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pandora(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "pandora_agent" => {
                println!("pandora_agent (OurOS) — Pandora FMS monitoring agent");
                println!("  --config FILE      Agent config file");
                println!("  --server HOST      Server address");
                println!("  --group NAME       Agent group");
                println!("  --interval SEC     Collection interval");
                println!("  --daemon           Run as daemon");
            }
            _ => {
                println!("pandora_server (OurOS) — Pandora FMS monitoring server");
                println!("  --config FILE      Server config file");
                println!("  --start            Start server");
                println!("  --stop             Stop server");
                println!("  --status           Show server status");
                println!("  --restart          Restart server");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Pandora FMS v777 (OurOS)"); return 0; }
    match prog {
        "pandora_agent" => {
            println!("Pandora FMS Agent v777 (OurOS)");
            println!("  Server: pandora.example.com");
            println!("  Modules: 23 active");
            println!("  Interval: 300s");
            println!("  Group: Servers");
            println!("  Status: running");
        }
        _ => {
            println!("Pandora FMS Server v777 (OurOS)");
            println!("  Agents: 56 reporting");
            println!("  Modules: 1,234");
            println!("  Alerts: 12 fired (last 24h)");
            println!("  Events: 4,567");
            println!("  Policies: 8 active");
            println!("  Console: http://0.0.0.0/pandora_console");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pandora_server".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pandora(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pandora};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pandorafms"), "pandorafms");
        assert_eq!(basename(r"C:\bin\pandorafms.exe"), "pandorafms.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pandorafms.exe"), "pandorafms");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pandora(&["--help".to_string()], "pandorafms"), 0);
        assert_eq!(run_pandora(&["-h".to_string()], "pandorafms"), 0);
        assert_eq!(run_pandora(&["--version".to_string()], "pandorafms"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pandora(&[], "pandorafms"), 0);
    }
}
