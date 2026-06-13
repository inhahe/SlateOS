#![deny(clippy::all)]

//! singer-cli — Slate OS Singer tap/target runner
//!
//! Multi-personality: `tap`, `target`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_singer(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        if prog.starts_with("target") {
            println!("Usage: target-<name> --config CONFIG [--state STATE]");
            println!("Singer Target — Data loader");
            println!();
            println!("Options:");
            println!("  --config FILE   Configuration file");
            println!("  --state FILE    State file for bookmarking");
        } else {
            println!("Usage: tap-<name> --config CONFIG [--catalog CATALOG] [--state STATE]");
            println!("Singer Tap — Data extractor");
            println!();
            println!("Options:");
            println!("  --config FILE    Configuration file");
            println!("  --catalog FILE   Catalog file (stream selection)");
            println!("  --state FILE     State file for incremental sync");
            println!("  --discover       Output catalog (discovery mode)");
        }
        return 0;
    }
    let discover = args.iter().any(|a| a == "--discover");

    if prog.starts_with("target") {
        println!("{{\"type\": \"STATE\", \"value\": {{\"position\": 1000}}}}");
        println!("INFO: 1000 records loaded to destination");
    } else if discover {
        println!("{{");
        println!("  \"streams\": [");
        println!("    {{");
        println!("      \"tap_stream_id\": \"users\",");
        println!("      \"stream\": \"users\",");
        println!("      \"schema\": {{");
        println!("        \"type\": \"object\",");
        println!("        \"properties\": {{");
        println!("          \"id\": {{\"type\": \"integer\"}},");
        println!("          \"name\": {{\"type\": \"string\"}},");
        println!("          \"email\": {{\"type\": \"string\"}}");
        println!("        }}");
        println!("      }}");
        println!("    }}");
        println!("  ]");
        println!("}}");
    } else {
        println!("{{\"type\": \"SCHEMA\", \"stream\": \"users\", \"schema\": {{\"type\": \"object\"}}, \"key_properties\": [\"id\"]}}");
        println!("{{\"type\": \"RECORD\", \"stream\": \"users\", \"record\": {{\"id\": 1, \"name\": \"Alice\", \"email\": \"alice@example.com\"}}}}");
        println!("{{\"type\": \"RECORD\", \"stream\": \"users\", \"record\": {{\"id\": 2, \"name\": \"Bob\", \"email\": \"bob@example.com\"}}}}");
        println!("{{\"type\": \"STATE\", \"value\": {{\"position\": 2}}}}");
        println!("INFO: 2 records synced from stream 'users'");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tap".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_singer(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_singer};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/singer"), "singer");
        assert_eq!(basename(r"C:\bin\singer.exe"), "singer.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("singer.exe"), "singer");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_singer(&["--help".to_string()], "singer"), 0);
        assert_eq!(run_singer(&["-h".to_string()], "singer"), 0);
        let _ = run_singer(&["--version".to_string()], "singer");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_singer(&[], "singer");
    }
}
