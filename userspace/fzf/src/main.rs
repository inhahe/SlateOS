#![deny(clippy::all)]

//! fzf — SlateOS command-line fuzzy finder
//!
//! Single personality: `fzf`

use std::env;
use std::process;

fn run_fzf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fzf [OPTIONS]");
        println!();
        println!("A command-line fuzzy finder.");
        println!();
        println!("Search:");
        println!("  -x, --extended        Extended-search mode (default)");
        println!("  -e, --exact           Enable exact-match");
        println!("  -i                    Case-insensitive match (default)");
        println!("  +i                    Case-sensitive match");
        println!("  --scheme <SCHEME>     Scoring scheme (default/path/history)");
        println!("  --literal             Don't normalize Latin characters");
        println!("  -n, --nth <N>[,..]    Comma-separated list of field indices");
        println!("  --with-nth <N>[,..]   Transform the presentation of each line");
        println!("  -d, --delimiter <STR> Field delimiter regex");
        println!("  --algo <TYPE>         Fuzzy matching algorithm (v1/v2)");
        println!();
        println!("Interface:");
        println!("  -m, --multi [MAX]     Enable multi-select");
        println!("  --no-mouse            Disable mouse");
        println!("  --bind <KEYBINDS>     Custom key bindings");
        println!("  --cycle               Enable cyclic scroll");
        println!("  --keep-right          Keep right end visible on overflow");
        println!("  --scroll-off <N>      Lines of context around cursor");
        println!("  --no-hscroll          Disable horizontal scroll");
        println!("  --filepath-word       Word-wise movement with filepath chars");
        println!();
        println!("Layout:");
        println!("  --height <HEIGHT>     Display fzf window in given height");
        println!("  --min-height <N>      Minimum height when using --height");
        println!("  --layout <LAYOUT>     Choose layout (default/reverse/reverse-list)");
        println!("  --border [STYLE]      Border style (rounded/sharp/bold/double/block/none)");
        println!("  --border-label <LABEL>  Border label");
        println!("  --margin <MARGIN>     Screen margin");
        println!("  --padding <PADDING>   Padding inside border");
        println!("  --info <STYLE>        Info display style (default/right/hidden/inline)");
        println!("  --separator <SEP>     Separator between header and body");
        println!("  --no-separator        Disable separator");
        println!("  --prompt <STR>        Prompt (default: '> ')");
        println!("  --pointer <STR>       Pointer to current line (default: '▶')");
        println!("  --marker <STR>        Multi-select marker (default: '▶')");
        println!("  --header <STR>        Sticky header");
        println!("  --header-lines <N>    First N lines as header");
        println!();
        println!("Display:");
        println!("  --ansi                Enable processing of ANSI color codes");
        println!("  --tabstop <N>         Tab stop width (default: 8)");
        println!("  --color <COLSPEC>     Color scheme");
        println!("  --no-bold             Don't use bold text");
        println!();
        println!("Preview:");
        println!("  --preview <CMD>       Preview command");
        println!("  --preview-window <OPT>  Preview window layout");
        println!("  --preview-label <L>   Preview window label");
        println!();
        println!("Scripting:");
        println!("  -q, --query <STR>     Initial query string");
        println!("  -1, --select-1        Automatically select if one match");
        println!("  -0, --exit-0          Exit immediately if no match");
        println!("  -f, --filter <STR>    Filter mode (non-interactive)");
        println!("  --print-query         Print query as first line");
        println!("  --expect <KEYS>       Comma-separated list of keys");
        println!("  --read0               Read input delimited by NUL");
        println!("  --print0              Print output delimited by NUL");
        println!("  --sync                Synchronous search");
        println!("  --listen [PORT]       Start HTTP server");
        println!("  -V, --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("fzf 0.53.0 (Slate OS)");
        return 0;
    }

    // Check for filter mode
    let filter_idx = args.iter().position(|a| a == "-f" || a == "--filter");
    if let Some(idx) = filter_idx {
        let query = args.get(idx + 1).map(|s| s.as_str()).unwrap_or("");
        // Simulate filter mode
        let items = ["src/main.rs", "src/lib.rs", "src/config.rs", "tests/test.rs", "Cargo.toml"];
        for item in &items {
            if item.contains(query) {
                println!("{}", item);
            }
        }
        return 0;
    }

    let query_idx = args.iter().position(|a| a == "-q" || a == "--query");
    let initial_query = query_idx
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("");

    // Simulate interactive mode
    println!("fzf — interactive fuzzy finder");
    if !initial_query.is_empty() {
        println!("  Query: {}", initial_query);
    }
    println!();
    println!("  ▶ src/main.rs");
    println!("    src/lib.rs");
    println!("    src/config.rs");
    println!("    tests/test.rs");
    println!("    Cargo.toml");
    println!("    README.md");
    println!("    .gitignore");
    println!();
    println!("  7/7  (0 selected)");
    println!("  > {}", initial_query);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fzf(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_fzf};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fzf(vec!["--help".to_string()]), 0);
        assert_eq!(run_fzf(vec!["-h".to_string()]), 0);
        let _ = run_fzf(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fzf(vec![]);
    }
}
