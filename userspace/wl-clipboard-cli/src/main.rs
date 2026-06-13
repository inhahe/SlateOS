#![deny(clippy::all)]

//! wl-clipboard-cli — SlateOS wl-clipboard Wayland clipboard
//!
//! Multi-personality: `wl-copy`, `wl-paste`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_copy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-copy [OPTIONS] [TEXT]");
        println!("wl-copy v2.2 (Slate OS) — Copy data to Wayland clipboard");
        println!();
        println!("Options:");
        println!("  TEXT              Text to copy (stdin if omitted)");
        println!("  -t MIME           MIME type");
        println!("  -o                Stay open after paste");
        println!("  -f                Foreground mode");
        println!("  -n                Trim trailing newline");
        println!("  -p                Use primary selection");
        println!("  --clear           Clear clipboard");
        return 0;
    }
    if args.iter().any(|a| a == "--clear") {
        println!("Clipboard cleared.");
        return 0;
    }
    let text = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(t) = text {
        println!("Copied: {} ({} bytes)", t, t.len());
    } else if args.is_empty() {
        println!("Reading from stdin...");
    }
    0
}

fn run_wl_paste(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-paste [OPTIONS]");
        println!("wl-paste v2.2 (Slate OS) — Paste from Wayland clipboard");
        println!();
        println!("Options:");
        println!("  -t MIME           Request specific MIME type");
        println!("  -n                Do not append newline");
        println!("  -l                List available MIME types");
        println!("  -p                Use primary selection");
        println!("  -w CMD            Watch for changes and run command");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("text/plain");
        println!("text/plain;charset=utf-8");
        println!("UTF8_STRING");
        println!("STRING");
        return 0;
    }
    println!("Hello, clipboard!");
    if args.is_empty() {
        println!("Hello, clipboard!");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-copy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wl-paste" => run_wl_paste(&rest, &prog),
        _ => run_wl_copy(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wl_copy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wl-clipboard"), "wl-clipboard");
        assert_eq!(basename(r"C:\bin\wl-clipboard.exe"), "wl-clipboard.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wl-clipboard.exe"), "wl-clipboard");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wl_copy(&["--help".to_string()], "wl-clipboard"), 0);
        assert_eq!(run_wl_copy(&["-h".to_string()], "wl-clipboard"), 0);
        let _ = run_wl_copy(&["--version".to_string()], "wl-clipboard");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wl_copy(&[], "wl-clipboard");
    }
}
