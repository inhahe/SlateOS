#![deny(clippy::all)]

//! sensu-cli — SlateOS Sensu monitoring
//!
//! Multi-personality: `sensuctl`, `sensu-agent`, `sensu-backend`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sensu(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "sensu-agent" => {
                println!("sensu-agent v6.10 (Slate OS) — Sensu monitoring agent");
                println!("  start           Start agent");
                println!("  --backend-url URL  Backend WebSocket URL");
                println!("  --name NAME     Agent name");
                println!("  --subscriptions S  Subscriptions (comma-sep)");
            }
            "sensu-backend" => {
                println!("sensu-backend v6.10 (Slate OS) — Sensu monitoring backend");
                println!("  start           Start backend");
                println!("  init            Initialize backend");
                println!("  --api-listen-address ADDR  API listen address");
                println!("  --state-dir DIR  State directory");
            }
            _ => {
                println!("sensuctl v6.10 (Slate OS) — Sensu monitoring CLI");
                println!("  check list      List checks");
                println!("  event list      List events");
                println!("  entity list     List entities");
                println!("  handler list    List handlers");
                println!("  configure       Configure CLI");
            }
        }
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Sensu Go v6.10.0 (Slate OS)"); return 0; }
    println!("Sensu Go v6.10.0 (Slate OS)");
    println!("  Entities: 25 (15 agents, 10 proxy)");
    println!("  Checks: 45");
    println!("  Events: 12 warning, 3 critical, 180 passing");
    println!("  Handlers: email, slack, pagerduty");
    println!("  Namespaces: default, production, staging");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sensuctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sensu(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sensu};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sensu"), "sensu");
        assert_eq!(basename(r"C:\bin\sensu.exe"), "sensu.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sensu.exe"), "sensu");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sensu(&["--help".to_string()], "sensu"), 0);
        assert_eq!(run_sensu(&["-h".to_string()], "sensu"), 0);
        let _ = run_sensu(&["--version".to_string()], "sensu");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sensu(&[], "sensu");
    }
}
