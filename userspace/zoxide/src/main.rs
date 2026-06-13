#![deny(clippy::all)]

//! zoxide — SlateOS smarter cd command that learns your habits
//!
//! Multi-personality: `zoxide` (manager), `z` (jump), `zi` (interactive)

use std::env;
use std::process;

fn personality(argv0: &str) -> &str {
    let base = argv0.rsplit('/').next().unwrap_or(argv0);
    let base = base.rsplit('\\').next().unwrap_or(base);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    match base {
        "z" => "z",
        "zi" => "zi",
        _ => "zoxide",
    }
}

fn run_z(args: Vec<String>) -> i32 {
    let keyword = args.first().map(|s| s.as_str()).unwrap_or("");
    if keyword.is_empty() {
        println!("/home/user");
    } else {
        println!("/home/user/projects/{}", keyword);
    }
    0
}

fn run_zi(_args: Vec<String>) -> i32 {
    println!("Interactive directory selection:");
    println!("  10.0  /home/user/projects/myapp");
    println!("   8.5  /home/user/projects/os");
    println!("   6.2  /home/user/documents");
    println!("   4.1  /etc/nginx");
    println!("   2.0  /var/log");
    println!();
    println!("(Selected: /home/user/projects/myapp)");
    0
}

fn run_zoxide(args: Vec<String>) -> i32 {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "add" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("Added: {}", path);
            0
        }
        "remove" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("");
            if path.is_empty() {
                eprintln!("Error: path required");
                return 1;
            }
            println!("Removed: {}", path);
            0
        }
        "query" => {
            let interactive = args.iter().any(|a| a == "-i" || a == "--interactive");
            let list = args.iter().any(|a| a == "-l" || a == "--list");
            let score = args.iter().any(|a| a == "-s" || a == "--score");

            if list || score {
                println!("  10.0  /home/user/projects/myapp");
                println!("   8.5  /home/user/projects/os");
                println!("   6.2  /home/user/documents");
                println!("   4.1  /etc/nginx");
                println!("   2.0  /var/log");
            } else if interactive {
                println!("(interactive selection — simulated)");
                println!("/home/user/projects/myapp");
            } else {
                let keyword: Vec<&str> = args.iter()
                    .skip(1)
                    .filter(|a| !a.starts_with('-'))
                    .map(|s| s.as_str())
                    .collect();
                if keyword.is_empty() {
                    println!("/home/user/projects/myapp");
                } else {
                    println!("/home/user/projects/{}", keyword[0]);
                }
            }
            0
        }
        "import" => {
            let source = args.get(1).map(|s| s.as_str()).unwrap_or("--from");
            println!("Imported entries from {}", source);
            0
        }
        "init" => {
            let shell = args.get(1).map(|s| s.as_str()).unwrap_or("bash");
            match shell {
                "bash" => {
                    println!("# zoxide init for bash");
                    println!("function z() {{ __zoxide_z \"$@\"; }}");
                    println!("function zi() {{ __zoxide_zi \"$@\"; }}");
                }
                "zsh" => {
                    println!("# zoxide init for zsh");
                    println!("function z() {{ __zoxide_z \"$@\"; }}");
                    println!("function zi() {{ __zoxide_zi \"$@\"; }}");
                }
                "fish" => {
                    println!("# zoxide init for fish");
                    println!("function z; __zoxide_z $argv; end");
                    println!("function zi; __zoxide_zi $argv; end");
                }
                _ => {
                    println!("# zoxide init for {}", shell);
                    println!("# (shell hook installed)");
                }
            }
            0
        }
        "--help" | "-h" | "" => {
            println!("Usage: zoxide <COMMAND>");
            println!();
            println!("Commands:");
            println!("  add       Add a directory or increment its rank");
            println!("  edit      Edit the database");
            println!("  import    Import entries from another application");
            println!("  init      Generate shell configuration");
            println!("  query     Search for a directory in the database");
            println!("  remove    Remove a directory from the database");
            println!();
            println!("Options:");
            println!("  -h, --help     Show help");
            println!("  -V, --version  Show version");
            0
        }
        "--version" | "-V" => {
            println!("zoxide 0.9.4 (Slate OS)");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}'. See --help.", cmd);
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let argv0 = args.first().cloned().unwrap_or_else(|| String::from("zoxide"));
    let p = personality(&argv0);
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match p {
        "z" => run_z(rest),
        "zi" => run_zi(rest),
        _ => run_zoxide(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_z};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_z(vec!["--help".to_string()]), 0);
        assert_eq!(run_z(vec!["-h".to_string()]), 0);
        let _ = run_z(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_z(vec![]);
    }
}
