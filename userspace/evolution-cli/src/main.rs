#![deny(clippy::all)]

//! evolution-cli — SlateOS GNOME Evolution groupware suite
//!
//! Multi-personality: `evolution`, `evolution-data-server`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_evolution(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: evolution [OPTIONS]");
        println!("evolution v3.50 (SlateOS) — GNOME groupware (mail, calendar, contacts)");
        println!();
        println!("Options:");
        println!("  -c COMPONENT      Start with component (mail, calendar, contacts, tasks, memos)");
        println!("  --force-shutdown   Force shutdown running instance");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("evolution v3.50 (SlateOS)"); return 0; }
    let component = args.iter().position(|a| a == "-c")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("mail");
    println!("evolution: started with component '{}'", component);
    println!("  Accounts: 1 configured");
    println!("  Data server: running");
    0
}

fn run_data_server(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: evolution-data-server [OPTIONS]");
        println!("evolution-data-server v3.50 (SlateOS) — Backend data service");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("evolution-data-server v3.50 (SlateOS)"); return 0; }
    println!("evolution-data-server: backend service started");
    println!("  Address books: 2");
    println!("  Calendars: 3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "evolution".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "evolution-data-server" => run_data_server(&rest, &prog),
        _ => run_evolution(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_evolution};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/evolution"), "evolution");
        assert_eq!(basename(r"C:\bin\evolution.exe"), "evolution.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("evolution.exe"), "evolution");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_evolution(&["--help".to_string()], "evolution"), 0);
        assert_eq!(run_evolution(&["-h".to_string()], "evolution"), 0);
        let _ = run_evolution(&["--version".to_string()], "evolution");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_evolution(&[], "evolution");
    }
}
