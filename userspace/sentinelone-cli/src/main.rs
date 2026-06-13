#![deny(clippy::all)]

//! sentinelone-cli — SlateOS SentinelOne Singularity XDR
//!
//! Single personality: `sentinelone`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_s1(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sentinelone [OPTIONS] [SUBCMD]");
        println!("SentinelOne Singularity Platform (SlateOS) — AI-powered XDR");
        println!();
        println!("Options:");
        println!("  --console URL          Management console URL");
        println!("  --api-token TOKEN      API token");
        println!("  agents list            List Sentinel agents");
        println!("  threats mitigate ID    Mitigate threat");
        println!("  --dv                   Deep Visibility (threat hunting)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SentinelOne Sentinel Agent 24.1.5 (SlateOS)"); return 0; }
    println!("SentinelOne Singularity Platform (SlateOS)");
    println!("  Modules: Endpoint (EPP/EDR), Cloud, Identity, Data (Singularity DataLake)");
    println!("  Architecture: autonomous AI agent on every endpoint (works offline)");
    println!("  Storyline: ATT&CK-mapped behavioral storylines, 1-click remediation");
    println!("  Ranger: passive network discovery + rogue device identification");
    println!("  Purple AI: GenAI security analyst (OpenAI + custom models)");
    println!("  Singularity Marketplace: 1-click integrations (SIEM, ticketing, threat intel)");
    println!("  Acquired Attivo (deception), Krebs (Scuba data security)");
    println!("  Vigilance MDR: managed detection/response service");
    println!("  License: per-endpoint subscription, tiered (Core/Control/Complete/Commercial)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sentinelone".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_s1(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_s1};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sentinelone"), "sentinelone");
        assert_eq!(basename(r"C:\bin\sentinelone.exe"), "sentinelone.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sentinelone.exe"), "sentinelone");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_s1(&["--help".to_string()], "sentinelone"), 0);
        assert_eq!(run_s1(&["-h".to_string()], "sentinelone"), 0);
        let _ = run_s1(&["--version".to_string()], "sentinelone");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_s1(&[], "sentinelone");
    }
}
