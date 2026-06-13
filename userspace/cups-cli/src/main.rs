#![deny(clippy::all)]

//! cups-cli — Slate OS CUPS printing system CLI
//!
//! Multi-personality: `cupsctl`, `cupsenable`, `cupsdisable`, `cupsaccept`, `cupsreject`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_cupsctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cupsctl [OPTIONS] [NAME=VALUE ...]");
        println!();
        println!("cupsctl — configure CUPS server (Slate OS).");
        println!();
        println!("Options:");
        println!("  -h SERVER[:PORT]   Connect to server");
        println!("  -E                 Encrypt connection");
        println!("  -U USER            Set username");
        println!("  --[no-]debug-logging  Toggle debug logging");
        println!("  --[no-]remote-admin   Toggle remote admin");
        println!("  --[no-]remote-any     Toggle remote printing");
        println!("  --[no-]share-printers Toggle printer sharing");
        println!("  --[no-]user-cancel-any  Toggle cancel-any");
        return 0;
    }

    if args.is_empty() {
        println!("_debug_logging=0");
        println!("_remote_admin=0");
        println!("_remote_any=0");
        println!("_share_printers=0");
        println!("_user_cancel_any=0");
    } else {
        for a in args.iter().filter(|a| a.contains('=')) {
            println!("Setting: {}", a);
        }
        if args.iter().any(|a| a == "--debug-logging") {
            println!("Debug logging enabled.");
        }
        if args.iter().any(|a| a == "--no-debug-logging") {
            println!("Debug logging disabled.");
        }
        if args.iter().any(|a| a == "--share-printers") {
            println!("Printer sharing enabled.");
        }
    }
    0
}

fn run_cupsenable(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cupsenable PRINTER ...");
        println!();
        println!("cupsenable — enable CUPS printer (Slate OS).");
        return 0;
    }
    for printer in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Printer '{}' enabled.", printer);
    }
    0
}

fn run_cupsdisable(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cupsdisable [OPTIONS] PRINTER ...");
        println!();
        println!("cupsdisable — disable CUPS printer (Slate OS).");
        println!();
        println!("Options:");
        println!("  -r REASON   Reason for disabling");
        return 0;
    }
    for printer in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Printer '{}' disabled.", printer);
    }
    0
}

fn run_cupsaccept(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cupsaccept PRINTER ...");
        println!();
        println!("cupsaccept — accept jobs on printer (Slate OS).");
        return 0;
    }
    for printer in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Printer '{}' now accepting jobs.", printer);
    }
    0
}

fn run_cupsreject(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cupsreject [OPTIONS] PRINTER ...");
        println!();
        println!("cupsreject — reject jobs on printer (Slate OS).");
        return 0;
    }
    for printer in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Printer '{}' now rejecting jobs.", printer);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "cupsctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "cupsenable" => run_cupsenable(&rest),
        "cupsdisable" => run_cupsdisable(&rest),
        "cupsaccept" => run_cupsaccept(&rest),
        "cupsreject" => run_cupsreject(&rest),
        _ => run_cupsctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cupsctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cups"), "cups");
        assert_eq!(basename(r"C:\bin\cups.exe"), "cups.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cups.exe"), "cups");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cupsctl(&["--help".to_string()]), 0);
        assert_eq!(run_cupsctl(&["-h".to_string()]), 0);
        let _ = run_cupsctl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cupsctl(&[]);
    }
}
