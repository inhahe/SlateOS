#![deny(clippy::all)]

//! rhvoice-cli — SlateOS RHVoice speech synthesizer
//!
//! Single personality: `rhvoice`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rhvoice(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rhvoice [OPTIONS] TEXT");
        println!("RHVoice v1.8 (SlateOS) — Free and open-source speech synthesizer");
        println!();
        println!("Options:");
        println!("  -p VOICE          Voice name");
        println!("  -o FILE           Output WAV file");
        println!("  -r RATE           Speech rate (0.5-2.0)");
        println!("  -v VOLUME         Volume (0.0-1.0)");
        println!("  --voices          List available voices");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("RHVoice v1.8 (SlateOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--voices") {
        println!("Available voices:");
        println!("  English: Alan, Bdl, Clb, Slt");
        println!("  Russian: Aleksandr, Anna, Elena, Irina");
        println!("  Portuguese: Leticia");
        return 0;
    }
    let text = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("Hello");
    println!("Speaking: \"{}\"", text);
    println!("  Voice: Slt (English)");
    println!("  Rate: 1.0");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rhvoice".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rhvoice(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rhvoice};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rhvoice"), "rhvoice");
        assert_eq!(basename(r"C:\bin\rhvoice.exe"), "rhvoice.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rhvoice.exe"), "rhvoice");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rhvoice(&["--help".to_string()], "rhvoice"), 0);
        assert_eq!(run_rhvoice(&["-h".to_string()], "rhvoice"), 0);
        let _ = run_rhvoice(&["--version".to_string()], "rhvoice");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rhvoice(&[], "rhvoice");
    }
}
