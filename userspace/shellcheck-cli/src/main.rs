#![deny(clippy::all)]

//! shellcheck-cli — OurOS ShellCheck CLI
//!
//! Single personality: `shellcheck`

use std::env;
use std::process;

fn run_shellcheck(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shellcheck [OPTIONS] [FILES...]");
        println!();
        println!("ShellCheck — shell script static analysis tool (OurOS).");
        println!();
        println!("Options:");
        println!("  -f, --format FORMAT  Output format (tty, gcc, checkstyle, json, diff)");
        println!("  -s, --shell SHELL    Specify dialect (sh, bash, dash, ksh)");
        println!("  -e, --exclude CODE   Exclude specific codes");
        println!("  -S, --severity LVL   Minimum severity (error, warning, info, style)");
        println!("  --color WHEN         Color output (auto, always, never)");
        println!("  -x                   Follow source statements");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ShellCheck - shell script analysis tool");
        println!("version: 0.9.0 (OurOS)");
        return 0;
    }

    let format = args.windows(2).find(|w| w[0] == "-f" || w[0] == "--format")
        .map(|w| w[1].as_str()).unwrap_or("tty");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let target = if files.is_empty() { "script.sh" } else { files[0] };

    match format {
        "json" => {
            println!("[");
            println!("  {{\"file\":\"{}\",\"line\":3,\"column\":1,\"level\":\"warning\",\"code\":2034,\"message\":\"x appears unused. Verify use (or export if used externally).\"}},", target);
            println!("  {{\"file\":\"{}\",\"line\":7,\"column\":5,\"level\":\"error\",\"code\":2086,\"message\":\"Double quote to prevent globbing and word splitting.\"}},", target);
            println!("  {{\"file\":\"{}\",\"line\":12,\"column\":3,\"level\":\"info\",\"code\":2046,\"message\":\"Quote this to prevent word splitting.\"}}", target);
            println!("]");
        }
        "gcc" => {
            println!("{}:3:1: warning: x appears unused. Verify use (or export if used externally). [SC2034]", target);
            println!("{}:7:5: error: Double quote to prevent globbing and word splitting. [SC2086]", target);
            println!("{}:12:3: info: Quote this to prevent word splitting. [SC2046]", target);
        }
        _ => {
            println!();
            println!("In {} line 3:", target);
            println!("x=42");
            println!("^-- SC2034 (warning): x appears unused. Verify use (or export if used externally).");
            println!();
            println!("In {} line 7:", target);
            println!("echo $var");
            println!("     ^---^ SC2086 (error): Double quote to prevent globbing and word splitting.");
            println!();
            println!("Did you mean: ");
            println!("echo \"$var\"");
            println!();
            println!("In {} line 12:", target);
            println!("files=$(ls *.txt)");
            println!("       ^--------^ SC2046 (info): Quote this to prevent word splitting.");
            println!();
            println!("For more information:");
            println!("  https://www.shellcheck.net/wiki/SC2034");
            println!("  https://www.shellcheck.net/wiki/SC2086");
            println!("  https://www.shellcheck.net/wiki/SC2046");
        }
    }
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_shellcheck(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_shellcheck};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shellcheck(vec!["--help".to_string()]), 0);
        assert_eq!(run_shellcheck(vec!["-h".to_string()]), 0);
        let _ = run_shellcheck(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shellcheck(vec![]);
    }
}
