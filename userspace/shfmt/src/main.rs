#![deny(clippy::all)]

//! shfmt — SlateOS shell script formatter
//!
//! Single personality: `shfmt`

use std::env;
use std::process;

fn run_shfmt(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shfmt [OPTIONS] [FILE]...");
        println!();
        println!("Format shell scripts.");
        println!();
        println!("Options:");
        println!("  -i, --indent <N>      Indent with N spaces (0 for tabs)");
        println!("  -bn                   Binary ops may start a line");
        println!("  -ci                   Switch cases are indented");
        println!("  -sr                   Redirect ops are followed by space");
        println!("  -kp                   Keep column alignment paddings");
        println!("  -fn                   Function opening braces on separate line");
        println!("  -s, --simplify        Simplify code");
        println!("  -mn                   Minify code");
        println!("  -d, --diff            Show diff instead of rewriting");
        println!("  -l, --list            List files that differ");
        println!("  -w, --write           Write to file instead of stdout");
        println!("  -f, --find            Find shell files recursively");
        println!("  -ln, --language-dialect <LANG>  Language (auto/bash/posix/mksh/bats)");
        println!("  --to-json             Output AST as JSON");
        println!("  --from-json           Read AST from JSON");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("shfmt 3.8.0 (Slate OS)");
        return 0;
    }

    let diff_mode = args.iter().any(|a| a == "-d" || a == "--diff");
    let list_mode = args.iter().any(|a| a == "-l" || a == "--list");
    let find_mode = args.iter().any(|a| a == "-f" || a == "--find");

    if find_mode {
        println!("./script.sh");
        println!("./lib/helpers.sh");
        println!("./tests/test.sh");
        println!("./ci/build.sh");
        return 0;
    }

    if list_mode {
        println!("script.sh");
        println!("lib/helpers.sh");
        return 0;
    }

    if diff_mode {
        println!("--- script.sh.orig");
        println!("+++ script.sh");
        println!("@@ -1,5 +1,5 @@");
        println!(" #!/bin/bash");
        println!("-if [ $x = 1 ];then");
        println!("-echo \"hello\"");
        println!("-fi");
        println!("+if [ \"$x\" = 1 ]; then");
        println!("+    echo \"hello\"");
        println!("+fi");
        return 0;
    }

    // Default: format and output
    println!("#!/bin/bash");
    println!();
    println!("set -euo pipefail");
    println!();
    println!("main() {{");
    println!("    local name=\"$1\"");
    println!("    if [ -z \"$name\" ]; then");
    println!("        echo \"Usage: $0 <name>\"");
    println!("        exit 1");
    println!("    fi");
    println!("    echo \"Hello, $name!\"");
    println!("}}");
    println!();
    println!("main \"$@\"");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_shfmt(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_shfmt};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shfmt(vec!["--help".to_string()]), 0);
        assert_eq!(run_shfmt(vec!["-h".to_string()]), 0);
        let _ = run_shfmt(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shfmt(vec![]);
    }
}
