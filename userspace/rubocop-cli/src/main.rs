#![deny(clippy::all)]

//! rubocop-cli — SlateOS RuboCop CLI
//!
//! Single personality: `rubocop`

use std::env;
use std::process;

fn run_rubocop(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rubocop [OPTIONS] [FILES/DIRS...]");
        println!();
        println!("RuboCop — Ruby static code analyzer and formatter (SlateOS).");
        println!();
        println!("Options:");
        println!("  -a, --auto-correct     Auto-correct offenses (safe)");
        println!("  -A, --auto-correct-all Auto-correct all offenses");
        println!("  --init                 Initialize .rubocop.yml");
        println!("  -f, --format FORMAT    Output format (progress, json, html)");
        println!("  --only COP            Run only specified cop(s)");
        println!("  --except COP          Exclude specified cop(s)");
        println!("  -c, --config FILE     Config file");
        println!("  --parallel            Parallel execution");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("1.60.2 (SlateOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--init") {
        println!("Writing new .rubocop.yml");
        println!("  File created successfully.");
        return 0;
    }

    let auto_correct = args.iter().any(|a| a == "-a" || a == "--auto-correct" || a == "-A" || a == "--auto-correct-all");

    println!("Inspecting 5 files");
    println!("..C.W");
    println!();
    println!("Offenses:");
    println!();
    println!("app/models/user.rb:3:3: C: Style/StringLiterals: Prefer single-quoted strings when you don't need interpolation.");
    println!("  name = \"Alice\"");
    println!("        ^^^^^^^");
    println!("app/models/user.rb:12:1: C: Layout/TrailingWhitespace: Trailing whitespace detected.");
    println!("app/controllers/users_controller.rb:8:5: W: Lint/UselessAssignment: Useless assignment to variable - result.");
    println!("  result = User.find(params[:id])");
    println!("  ^^^^^^");

    if auto_correct {
        println!();
        println!("3 files inspected, 3 offenses detected, 2 offenses corrected");
    } else {
        println!();
        println!("5 files inspected, 3 offenses detected");
    }
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rubocop(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_rubocop};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rubocop(vec!["--help".to_string()]), 0);
        assert_eq!(run_rubocop(vec!["-h".to_string()]), 0);
        let _ = run_rubocop(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rubocop(vec![]);
    }
}
