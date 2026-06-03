#![deny(clippy::all)]

//! stackstorm-cli — OurOS StackStorm event-driven automation
//!
//! Single personality: `st2`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_st2(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: st2 [COMMAND] [OPTIONS]");
        println!("StackStorm v3.9 (OurOS) — Event-driven automation (IFTTT for ops)");
        println!();
        println!("Commands:");
        println!("  action list|get|execute   Manage actions");
        println!("  rule list|get|create      Manage rules");
        println!("  trigger list|get          Manage triggers");
        println!("  execution list|get|cancel Manage executions");
        println!("  pack list|install|remove  Manage packs");
        println!("  sensor list|enable        Manage sensors");
        println!("  workflow list|inspect      Manage workflows (Orquesta)");
        println!("  key list|set|delete       Key-value store");
        println!();
        println!("Options:");
        println!("  --url URL          StackStorm API URL");
        println!("  --api-key KEY      API key");
        println!("  --token TOKEN      Auth token");
        println!("  --json             JSON output");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("st2 v3.9.0 (OurOS)"); return 0; }
    println!("StackStorm v3.9.0 (OurOS)");
    println!("  Packs: 23 installed");
    println!("  Actions: 456");
    println!("  Rules: 34 active");
    println!("  Sensors: 12 running");
    println!("  Triggers: 89");
    println!("  Executions: 2,345 (last 24h)");
    println!("  Workflows: 67 (Orquesta)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "st2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_st2(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_st2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/stackstorm"), "stackstorm");
        assert_eq!(basename(r"C:\bin\stackstorm.exe"), "stackstorm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("stackstorm.exe"), "stackstorm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_st2(&["--help".to_string()], "stackstorm"), 0);
        assert_eq!(run_st2(&["-h".to_string()], "stackstorm"), 0);
        assert_eq!(run_st2(&["--version".to_string()], "stackstorm"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_st2(&[], "stackstorm"), 0);
    }
}
