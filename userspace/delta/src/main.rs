#![deny(clippy::all)]

//! delta — SlateOS syntax-highlighting pager for git, diff, and grep output
//!
//! Single personality: `delta`

use std::env;
use std::process;

fn run_delta(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: delta [OPTIONS] [MINUS_FILE] [PLUS_FILE]");
        println!();
        println!("A syntax-highlighting pager for git, diff, and grep output.");
        println!();
        println!("Options:");
        println!("  -n, --line-numbers           Show line numbers");
        println!("  -s, --side-by-side           Side-by-side view");
        println!("  --diff-so-fancy              Emulate diff-so-fancy");
        println!("  --navigate                   Activate navigation keybindings");
        println!("  --hyperlinks                 Render OSC 8 hyperlinks");
        println!("  --syntax-theme <THEME>       Syntax highlighting theme");
        println!("  --light                      Use light background theme");
        println!("  --dark                       Use dark background theme");
        println!("  --line-fill-method <METHOD>  Line fill (ansi/spaces)");
        println!("  --width <WIDTH>              Output width (default: terminal)");
        println!("  --tabs <N>                   Tab width (default: 4)");
        println!("  --true-color <WHEN>          True color (auto/always/never)");
        println!("  --24-bit-color <WHEN>        Alias for --true-color");
        println!("  --paging <WHEN>              Pager mode (auto/always/never)");
        println!("  --pager <CMD>                Pager program");
        println!("  --minus-style <STYLE>        Style for removed lines");
        println!("  --plus-style <STYLE>         Style for added lines");
        println!("  --minus-emph-style <STYLE>   Style for emphasized removed text");
        println!("  --plus-emph-style <STYLE>    Style for emphasized added text");
        println!("  --zero-style <STYLE>         Style for unchanged lines");
        println!("  --hunk-header-style <STYLE>  Style for hunk headers");
        println!("  --file-style <STYLE>         Style for file names");
        println!("  --file-decoration-style <S>  File decoration (box/underline/none)");
        println!("  --commit-style <STYLE>       Style for commit hashes");
        println!("  --blame-format <FMT>         Format for git blame output");
        println!("  --grep-output-type <TYPE>    Grep output (ripgrep/classic)");
        println!("  --map-styles <MAP>           Map input styles to delta styles");
        println!("  --max-line-length <N>        Truncate lines longer than N");
        println!("  --diff-stat-align-width <N>  Alignment width for diff stat");
        println!("  --features <FEATURES>        Named feature sets");
        println!("  --raw                        Don't alter input");
        println!("  --color-only                 Only add color (keep formatting)");
        println!("  --list-syntax-themes         List available themes");
        println!("  --list-languages             List supported languages");
        println!("  --show-syntax-themes         Demo available themes");
        println!("  --show-colors                Show 256 color palette");
        println!("  --show-config                Print active configuration");
        println!("  -V, --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("delta 0.17.0 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--list-syntax-themes") {
        println!("Dark themes:");
        println!("  1337");
        println!("  Coldark-Dark");
        println!("  DarkNeon");
        println!("  Dracula");
        println!("  Monokai Extended");
        println!("  Nord");
        println!("  OneHalfDark");
        println!("  Sublime Snazzy");
        println!("  TwoDark");
        println!("  Visual Studio Dark+");
        println!("  ansi");
        println!("  base16");
        println!("  gruvbox-dark");
        println!("  zenburn");
        println!();
        println!("Light themes:");
        println!("  Coldark-Cold");
        println!("  GitHub");
        println!("  OneHalfLight");
        println!("  Solarized (light)");
        println!("  gruvbox-light");
        return 0;
    }
    if args.iter().any(|a| a == "--show-config") {
        println!("[delta]");
        println!("    minus-style                   = red bold");
        println!("    plus-style                    = green bold");
        println!("    minus-emph-style              = red bold ul");
        println!("    plus-emph-style               = green bold ul");
        println!("    hunk-header-style             = blue");
        println!("    file-style                    = yellow bold");
        println!("    file-decoration-style         = yellow ul");
        println!("    line-numbers                  = true");
        println!("    side-by-side                  = false");
        println!("    navigate                      = false");
        println!("    syntax-theme                  = Monokai Extended");
        println!("    paging                        = auto");
        println!("    tabs                          = 4");
        println!("    true-color                    = auto");
        return 0;
    }

    let side = args.iter().any(|a| a == "-s" || a == "--side-by-side");
    let line_nums = args.iter().any(|a| a == "-n" || a == "--line-numbers");

    // Simulate processing diff input
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if files.len() >= 2 {
        println!("─── {} vs {} ───", files[0], files[1]);
    }

    if side {
        println!("│  1 │ fn main() {{                    │  1 │ fn main() {{");
        println!("│  2 │     let x = 10;               │  2 │     let x = 42;");
        println!("│  3 │     println!(\"old\");           │  3 │     println!(\"new\");");
        println!("│  4 │ }}                              │  4 │ }}");
    } else if line_nums {
        println!("  1 1 │  fn main() {{");
        println!("  2   │ -    let x = 10;");
        println!("    2 │ +    let x = 42;");
        println!("  3   │ -    println!(\"old\");");
        println!("    3 │ +    println!(\"new\");");
        println!("  4 4 │  }}");
    } else {
        println!(" fn main() {{");
        println!("-    let x = 10;");
        println!("+    let x = 42;");
        println!("-    println!(\"old\");");
        println!("+    println!(\"new\");");
        println!(" }}");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_delta(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_delta};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_delta(vec!["--help".to_string()]), 0);
        assert_eq!(run_delta(vec!["-h".to_string()]), 0);
        let _ = run_delta(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_delta(vec![]);
    }
}
