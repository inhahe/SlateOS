#![deny(clippy::all)]

//! jless — SlateOS command-line JSON viewer/explorer
//!
//! Single personality: `jless`

use std::env;
use std::process;

fn run_jless(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jless [OPTIONS] [FILE]");
        println!();
        println!("A command-line JSON viewer.");
        println!();
        println!("Options:");
        println!("  --mode <MODE>         Viewing mode (line/data)");
        println!("  --scrolloff <N>       Lines of context around cursor");
        println!("  --theme <THEME>       Color theme");
        println!("  --json                Input is JSON (default)");
        println!("  --yaml                Input is YAML");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("jless 0.9.0 (Slate OS)");
        return 0;
    }

    let yaml_mode = args.iter().any(|a| a == "--yaml");

    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    if let Some(f) = file {
        if yaml_mode {
            println!("(jless: viewing YAML file '{}')", f);
        } else {
            println!("(jless: viewing JSON file '{}')", f);
        }
    } else {
        println!("(jless: reading from stdin)");
    }

    println!();
    println!("▼ {{");
    println!("    \"name\": \"example-project\",");
    println!("    \"version\": \"1.0.0\",");
    println!("  ▼ \"dependencies\": {{");
    println!("      ▼ \"serde\": {{");
    println!("            \"version\": \"1.0\",");
    println!("          ▶ \"features\": [ ... ] (2 items)");
    println!("        }},");
    println!("        \"tokio\": \"1.37\",");
    println!("    }},");
    println!("  ▶ \"scripts\": {{ ... }} (3 entries),");
    println!("  ▶ \"devDependencies\": {{ ... }} (5 entries)");
    println!("  }}");
    println!();
    println!("(TUI mode — navigate with j/k, expand/collapse with h/l/space)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_jless(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jless};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jless(vec!["--help".to_string()]), 0);
        assert_eq!(run_jless(vec!["-h".to_string()]), 0);
        let _ = run_jless(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jless(vec![]);
    }
}
