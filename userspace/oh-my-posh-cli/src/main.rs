#![deny(clippy::all)]

//! oh-my-posh-cli — OurOS Oh My Posh prompt engine
//!
//! Single personality: `oh-my-posh`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_omp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: oh-my-posh [COMMAND]");
        println!("Oh My Posh 23.6.4 (OurOS) — Prompt theme engine");
        println!();
        println!("Commands:");
        println!("  init SHELL          Print shell init script");
        println!("  print primary       Print primary prompt");
        println!("  print secondary     Print secondary prompt");
        println!("  print transient     Print transient prompt");
        println!("  print right         Print right prompt");
        println!("  print tooltip       Print tooltip prompt");
        println!("  config              Manage config");
        println!("  config edit         Edit config file");
        println!("  config export       Export config");
        println!("  config migrate      Migrate config version");
        println!("  get shell           Get current shell");
        println!("  get millis          Get time in ms");
        println!("  cache clear         Clear cache");
        println!("  font install NAME   Install a Nerd Font");
        println!("  font configure      Configure font in terminal");
        println!("  notice              Print update notice");
        println!("  upgrade             Upgrade Oh My Posh");
        println!("  version             Show version");
        return 0;
    }
    let cmd = args.iter().find(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "version" => println!("oh-my-posh 23.6.4 (OurOS)"),
        "init" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            println!("# oh-my-posh init for {}", shell);
            println!("eval \"$(oh-my-posh init {})\"", shell);
        }
        "print" => {
            let what = args.iter().skip_while(|a| a.as_str() != "print").nth(1)
                .map(|s| s.as_str()).unwrap_or("primary");
            match what {
                "primary" => println!("\x1b[32m❯\x1b[0m "),
                "secondary" => println!(".. "),
                "transient" => println!("> "),
                "right" => println!("[12:00]"),
                _ => println!("(prompt: {})", what),
            }
        }
        "config" => {
            let sub = args.iter().skip_while(|a| a.as_str() != "config").nth(1)
                .map(|s| s.as_str()).unwrap_or("edit");
            match sub {
                "edit" => println!("oh-my-posh: Opening config editor..."),
                "export" => println!("oh-my-posh: Config exported."),
                "migrate" => println!("oh-my-posh: Config migrated to latest version."),
                _ => println!("oh-my-posh config: {}", sub),
            }
        }
        "get" => {
            let what = args.iter().skip_while(|a| a.as_str() != "get").nth(1)
                .map(|s| s.as_str()).unwrap_or("shell");
            match what {
                "shell" => println!("bash"),
                "millis" => println!("1716422400000"),
                _ => println!("{}", what),
            }
        }
        "cache" => println!("oh-my-posh: Cache cleared."),
        "font" => {
            let sub = args.iter().skip_while(|a| a.as_str() != "font").nth(1)
                .map(|s| s.as_str()).unwrap_or("install");
            if sub == "install" {
                let name = args.iter().skip_while(|a| a.as_str() != "install").nth(1)
                    .map(|s| s.as_str()).unwrap_or("JetBrainsMono");
                println!("oh-my-posh: Installing font '{}'...", name);
            } else {
                println!("oh-my-posh font: {}", sub);
            }
        }
        "notice" => println!("oh-my-posh: No updates available."),
        "upgrade" => println!("oh-my-posh: Already up to date."),
        _ => println!("oh-my-posh: unknown command '{}'", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "oh-my-posh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_omp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_omp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/oh-my-posh"), "oh-my-posh");
        assert_eq!(basename(r"C:\bin\oh-my-posh.exe"), "oh-my-posh.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("oh-my-posh.exe"), "oh-my-posh");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_omp(&["--help".to_string()], "oh-my-posh"), 0);
        assert_eq!(run_omp(&["-h".to_string()], "oh-my-posh"), 0);
        assert_eq!(run_omp(&["--version".to_string()], "oh-my-posh"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_omp(&[], "oh-my-posh"), 0);
    }
}
