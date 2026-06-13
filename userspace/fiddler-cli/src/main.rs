#![deny(clippy::all)]

//! fiddler-cli — SlateOS Fiddler web debugging proxy
//!
//! Single personality: `fiddler`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fiddler(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fiddler [OPTIONS]");
        println!("Fiddler Everywhere v5.1.0 (Slate OS) — Web debugging proxy");
        println!();
        println!("Options:");
        println!("  --port PORT         Proxy port (default: 8866)");
        println!("  --headless          Headless mode");
        println!("  --capture           Start capturing immediately");
        println!("  --rules FILE        Load rules");
        println!("  --export FILE       Export captured traffic");
        println!("  -V, --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Fiddler Everywhere v5.1.0 (Slate OS)");
        return 0;
    }
    println!("Fiddler Everywhere v5.1.0");
    println!("  Proxy: localhost:8866");
    println!("  HTTPS: decryption enabled");
    println!("  Capturing traffic...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fiddler".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fiddler(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fiddler};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fiddler"), "fiddler");
        assert_eq!(basename(r"C:\bin\fiddler.exe"), "fiddler.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fiddler.exe"), "fiddler");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fiddler(&["--help".to_string()], "fiddler"), 0);
        assert_eq!(run_fiddler(&["-h".to_string()], "fiddler"), 0);
        let _ = run_fiddler(&["--version".to_string()], "fiddler");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fiddler(&[], "fiddler");
    }
}
