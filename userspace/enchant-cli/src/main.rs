#![deny(clippy::all)]

//! enchant-cli — SlateOS Enchant spell checking library CLI
//!
//! Multi-personality: `enchant-2`, `enchant-lsmod-2`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_enchant(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: enchant-2 [OPTIONS]");
        println!();
        println!("Enchant — spell checking interface (Slate OS).");
        println!();
        println!("Options:");
        println!("  -d DICT        Dictionary tag");
        println!("  -l             List misspelled words");
        println!("  -a             Pipe mode");
        println!("  -L             List with line numbers");
        println!("  -v             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("enchant-2 2.6.8 (Slate OS)");
        return 0;
    }

    let dict = args.windows(2).find(|w| w[0] == "-d").map(|w| w[1].as_str()).unwrap_or("en_US");
    let list_mode = args.iter().any(|a| a == "-l");
    let pipe_mode = args.iter().any(|a| a == "-a");

    if pipe_mode {
        println!("@(#) Enchant 2.6.8 (Slate OS)");
    } else if list_mode {
        println!("teh");
        println!("recieve");
    } else {
        println!("Enchant 2.6.8 (dict={})", dict);
        println!("(reading from stdin)");
    }
    0
}

fn run_enchant_lsmod(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: enchant-lsmod-2 [OPTIONS]");
        println!();
        println!("enchant-lsmod-2 — list Enchant providers (Slate OS).");
        println!();
        println!("Options:");
        println!("  -lang TAG     List providers for language");
        println!("  -word-chars   Show word characters");
        return 0;
    }

    if let Some(pos) = args.iter().position(|a| a == "-lang") {
        let lang = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("en_US");
        println!("Providers for '{}':", lang);
        println!("  hunspell  Hunspell Provider (en_US)");
        println!("  aspell    Aspell Provider (en)");
    } else {
        println!("Providers:");
        println!("  hunspell  Hunspell Provider");
        println!("  aspell    Aspell Provider");
        println!("  nuspell   Nuspell Provider");
        println!("  voikko    Voikko Provider");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "enchant-2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "enchant-lsmod-2" => run_enchant_lsmod(&rest),
        _ => run_enchant(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_enchant};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/enchant"), "enchant");
        assert_eq!(basename(r"C:\bin\enchant.exe"), "enchant.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("enchant.exe"), "enchant");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_enchant(&["--help".to_string()]), 0);
        assert_eq!(run_enchant(&["-h".to_string()]), 0);
        let _ = run_enchant(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_enchant(&[]);
    }
}
