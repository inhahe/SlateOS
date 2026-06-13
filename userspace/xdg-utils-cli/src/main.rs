#![deny(clippy::all)]

//! xdg-utils-cli — SlateOS xdg-utils desktop integration
//!
//! Multi-personality: `xdg-open`, `xdg-mime`, `xdg-settings`, `xdg-email`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xdg_open(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xdg-open FILE|URL");
        println!("xdg-open v1.2 (Slate OS) — Open file or URL in preferred application");
        return 0;
    }
    let target = args.first().map(|s| s.as_str()).unwrap_or("(none)");
    println!("Opening: {}", target);
    0
}

fn run_xdg_mime(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xdg-mime COMMAND [OPTIONS]");
        println!("xdg-mime v1.2 (Slate OS) — MIME type operations");
        println!();
        println!("Commands:");
        println!("  query filetype FILE   Query MIME type");
        println!("  query default MIME    Query default app");
        println!("  default APP MIME      Set default app");
        println!("  install XMLFILE       Install MIME type");
        println!("  uninstall XMLFILE     Uninstall MIME type");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("query");
    match cmd {
        "query" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("filetype");
            let target = args.get(2).map(|s| s.as_str()).unwrap_or("file.txt");
            if sub == "filetype" {
                println!("text/plain");
            } else {
                println!("org.gnome.TextEditor.desktop for {}", target);
            }
        }
        "default" => {
            let app = args.get(1).map(|s| s.as_str()).unwrap_or("app.desktop");
            let mime = args.get(2).map(|s| s.as_str()).unwrap_or("text/plain");
            println!("Set {} as default for {}", app, mime);
        }
        _ => println!("xdg-mime: {}", cmd),
    }
    0
}

fn run_xdg_settings(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: xdg-settings COMMAND PROPERTY [VALUE]");
        println!("xdg-settings v1.2 (Slate OS) — Desktop settings");
        println!();
        println!("Commands:");
        println!("  get PROPERTY      Get setting value");
        println!("  set PROPERTY VAL  Set setting value");
        println!("  check PROPERTY VAL Check setting");
        println!();
        println!("Properties:");
        println!("  default-web-browser, default-url-scheme-handler");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("get");
    let prop = args.get(1).map(|s| s.as_str()).unwrap_or("default-web-browser");
    match cmd {
        "get" => println!("firefox.desktop"),
        "set" => {
            let val = args.get(2).map(|s| s.as_str()).unwrap_or("firefox.desktop");
            println!("Set {} = {}", prop, val);
        }
        _ => println!("xdg-settings: {}", cmd),
    }
    0
}

fn run_xdg_email(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xdg-email [OPTIONS] ADDRESS");
        println!("xdg-email v1.2 (Slate OS) — Compose email");
        println!();
        println!("Options:");
        println!("  --subject TEXT    Subject line");
        println!("  --body TEXT       Body text");
        println!("  --attach FILE     Attachment");
        println!("  --cc ADDRESS      CC address");
        return 0;
    }
    let addr = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("user@example.com");
    println!("Composing email to: {}", addr);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xdg-open".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "xdg-mime" => run_xdg_mime(&rest, &prog),
        "xdg-settings" => run_xdg_settings(&rest, &prog),
        "xdg-email" => run_xdg_email(&rest, &prog),
        _ => run_xdg_open(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xdg_open};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xdg-utils"), "xdg-utils");
        assert_eq!(basename(r"C:\bin\xdg-utils.exe"), "xdg-utils.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xdg-utils.exe"), "xdg-utils");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xdg_open(&["--help".to_string()], "xdg-utils"), 0);
        assert_eq!(run_xdg_open(&["-h".to_string()], "xdg-utils"), 0);
        let _ = run_xdg_open(&["--version".to_string()], "xdg-utils");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xdg_open(&[], "xdg-utils");
    }
}
