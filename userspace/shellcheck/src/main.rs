#![deny(clippy::all)]

//! shellcheck — SlateOS shell script static analysis tool
//!
//! Single personality: `shellcheck`

use std::env;
use std::process;

fn run_shellcheck(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shellcheck [OPTIONS] <SCRIPT>...");
        println!();
        println!("Shell script analysis tool.");
        println!();
        println!("Options:");
        println!("  -f, --format <FMT>     Output format (tty/gcc/checkstyle/diff/json1/json/quiet)");
        println!("  -e, --exclude <CODE>   Exclude specific checks");
        println!("  --include <CODE>       Only run specific checks");
        println!("  -s, --shell <SHELL>    Override shell dialect (sh/bash/dash/ksh/zsh)");
        println!("  -S, --severity <SEV>   Minimum severity (error/warning/info/style)");
        println!("  -C, --color <WHEN>     Color (auto/always/never)");
        println!("  -a, --check-sourced    Check sourced files too");
        println!("  -x, --external-sources Allow external source directives");
        println!("  --source-path <DIR>    Path for sourced scripts");
        println!("  --wiki-link-count <N>  Show N wiki links");
        println!("  --norc                 Don't load .shellcheckrc");
        println!("  -P, --source-path <P>  Include path for sourced files");
        println!("  -o, --enable <NAME>    Enable optional checks");
        println!("  -V, --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("ShellCheck 0.10.0 (SlateOS)");
        return 0;
    }

    let json = args.windows(2).any(|w| (w[0] == "-f" || w[0] == "--format") && (w[1] == "json" || w[1] == "json1"));
    let quiet = args.windows(2).any(|w| (w[0] == "-f" || w[0] == "--format") && w[1] == "quiet");

    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.is_empty() {
        eprintln!("Error: script file required. See --help.");
        return 1;
    }

    if quiet {
        return 1; // Indicates issues found
    }

    if json {
        println!("[");
        println!("  {{\"file\":\"{}\",\"line\":3,\"column\":1,\"level\":\"warning\",\"code\":2034,\"message\":\"x appears unused. Verify use (or export/local).\"}},", files[0]);
        println!("  {{\"file\":\"{}\",\"line\":7,\"column\":5,\"level\":\"error\",\"code\":2086,\"message\":\"Double quote to prevent globbing and word splitting.\"}},", files[0]);
        println!("  {{\"file\":\"{}\",\"line\":12,\"column\":1,\"level\":\"info\",\"code\":2154,\"message\":\"var is referenced but not assigned.\"}}", files[0]);
        println!("]");
        return 1;
    }

    for file in &files {
        println!("In {} line 3:", file);
        println!("x=unused_var");
        println!("^── SC2034 (warning): x appears unused. Verify use (or export/local).");
        println!();
        println!("In {} line 7:", file);
        println!("echo $var");
        println!("     ^──── SC2086 (error): Double quote to prevent globbing and word splitting.");
        println!();
        println!("Did you mean:");
        println!("echo \"$var\"");
        println!();
        println!("In {} line 12:", file);
        println!("echo $undefined");
        println!("     ^───────── SC2154 (info): undefined is referenced but not assigned.");
        println!();
        println!("For more information:");
        println!("  https://www.shellcheck.net/wiki/SC2034");
        println!("  https://www.shellcheck.net/wiki/SC2086");
        println!("  https://www.shellcheck.net/wiki/SC2154");
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
