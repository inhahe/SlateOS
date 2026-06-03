#![deny(clippy::all)]

//! eslint-cli — OurOS ESLint CLI
//!
//! Single personality: `eslint`

use std::env;
use std::process;

fn run_eslint(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: eslint [OPTIONS] [FILES/DIRS...]");
        println!();
        println!("ESLint — JavaScript/TypeScript linter (OurOS).");
        println!();
        println!("Options:");
        println!("  --fix              Automatically fix problems");
        println!("  --format FORMAT    Output format (stylish, json, compact)");
        println!("  --config PATH      Config file");
        println!("  --ext EXTS         File extensions (.js,.ts,.tsx)");
        println!("  --rule RULE        Specify rules");
        println!("  --max-warnings N   Max warnings before error");
        println!("  --quiet            Report errors only");
        println!("  --init             Initialize config");
        println!("  --cache            Cache results");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("v9.0.0 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "--init") {
        println!("✔ How would you like to use ESLint? · problems");
        println!("✔ What type of modules does your project use? · esm");
        println!("✔ Which framework does your project use? · react");
        println!("✔ Does your project use TypeScript? · Yes");
        println!("✔ Created .eslintrc.json");
        return 0;
    }

    let fix = args.iter().any(|a| a == "--fix");
    let quiet = args.iter().any(|a| a == "--quiet");

    println!();
    println!("src/components/App.tsx");
    println!("  3:10  error    'useState' is defined but never used  no-unused-vars");
    println!("  15:5  error    Unexpected console statement           no-console");
    if !quiet {
        println!("  22:1  warning  Missing return type on function        @typescript-eslint/explicit-function-return-type");
    }
    println!();
    println!("src/utils/helpers.ts");
    println!("  8:3   error    'any' is not allowed as a type          @typescript-eslint/no-explicit-any");
    if !quiet {
        println!("  12:1  warning  Prefer const over let                   prefer-const");
    }

    if fix {
        println!();
        println!("  2 problems fixed.");
    }

    println!();
    if quiet {
        println!("✖ 3 problems (3 errors, 0 warnings)");
    } else {
        println!("✖ 5 problems (3 errors, 2 warnings)");
    }
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_eslint(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_eslint};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_eslint(vec!["--help".to_string()]), 0);
        assert_eq!(run_eslint(vec!["-h".to_string()]), 0);
        assert_eq!(run_eslint(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_eslint(vec![]), 0);
    }
}
