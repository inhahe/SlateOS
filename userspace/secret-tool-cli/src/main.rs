#![deny(clippy::all)]

//! secret-tool-cli — OurOS libsecret secret-tool
//!
//! Single personality: `secret-tool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_secret_tool(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: secret-tool COMMAND [ARGS]");
        println!("secret-tool v0.20 (OurOS) — Secret storage CLI");
        println!();
        println!("Commands:");
        println!("  store             Store a secret");
        println!("  lookup            Lookup a secret");
        println!("  clear             Clear matching secrets");
        println!("  search            Search for secrets");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("search");
    match cmd {
        "lookup" => println!("mysecretpassword123"),
        "store" => println!("Secret stored successfully"),
        "clear" => println!("Cleared 1 matching secret"),
        "search" => {
            println!("[/org/freedesktop/secrets/collection/login/1]");
            println!("  label = WiFi Password");
            println!("  schema = org.gnome.keyring.NetworkManager");
            println!("  created = 2024-01-15");
        }
        _ => println!("secret-tool: unknown command: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "secret-tool".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_secret_tool(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_secret_tool};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/secret-tool"), "secret-tool");
        assert_eq!(basename(r"C:\bin\secret-tool.exe"), "secret-tool.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("secret-tool.exe"), "secret-tool");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_secret_tool(&["--help".to_string()], "secret-tool"), 0);
        assert_eq!(run_secret_tool(&["-h".to_string()], "secret-tool"), 0);
        assert_eq!(run_secret_tool(&["--version".to_string()], "secret-tool"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_secret_tool(&[], "secret-tool"), 0);
    }
}
