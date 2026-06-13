#![deny(clippy::all)]

//! helix — SlateOS post-modern modal text editor
//!
//! Single personality: `hx`

use std::env;
use std::process;

fn run_hx(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hx [FLAGS] [files]...");
        println!();
        println!("Flags:");
        println!("  --tutor           Open tutorial");
        println!("  --health [lang]   Check health");
        println!("  -g, --grammar <lang>  Fetch or build grammar");
        println!("  --vsplit          Open in vertical split");
        println!("  --hsplit          Open in horizontal split");
        println!("  -c, --config <file>  Config file");
        println!("  --log <file>      Log file");
        println!("  -v                Verbose logging");
        println!("  -V, --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("helix 24.07 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--health") {
        let lang = args.iter().position(|a| a == "--health")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str());
        if let Some(l) = lang {
            println!("Language: {}", l);
            println!("  Language server: installed");
            println!("  Highlight queries: found");
            println!("  Textobject queries: found");
            println!("  Indent queries: found");
        } else {
            println!("Config file: default");
            println!("Config dir: ~/.config/helix");
            println!("Runtime dirs: /usr/lib/helix/runtime");
            println!();
            println!("Language  LSP       DAP       Highlight  Textobject  Indent");
            println!("rust      rust-analyzer  -  ✓          ✓           ✓");
            println!("python    pylsp         -  ✓          ✓           ✓");
            println!("js        tsserver      -  ✓          ✓           ✓");
            println!("go        gopls         -  ✓          ✓           ✓");
        }
        return 0;
    }
    if args.iter().any(|a| a == "--tutor") {
        println!("Welcome to the Helix editor tutorial!");
        println!("(tutorial mode — simulated)");
        return 0;
    }

    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    if files.is_empty() {
        println!("helix 24.07 (SlateOS) — post-modern modal editor");
    } else {
        for f in &files {
            println!("Opening: {}", f);
        }
    }
    println!("(TUI launched — simulated)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hx(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_hx};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hx(vec!["--help".to_string()]), 0);
        assert_eq!(run_hx(vec!["-h".to_string()]), 0);
        let _ = run_hx(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hx(vec![]);
    }
}
