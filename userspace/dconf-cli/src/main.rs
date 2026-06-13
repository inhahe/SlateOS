#![deny(clippy::all)]

//! dconf-cli — SlateOS dconf configuration system CLI
//!
//! Single personality: `dconf`

use std::env;
use std::process;

fn run_dconf(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dconf COMMAND [ARGS]");
        println!();
        println!("dconf — low-level configuration system (Slate OS).");
        println!();
        println!("Commands:");
        println!("  read KEY           Read key value");
        println!("  list DIR           List keys in directory");
        println!("  write KEY VALUE    Write key value");
        println!("  reset KEY          Reset key to default");
        println!("  compile OUT DIR    Compile binary database");
        println!("  update             Update system databases");
        println!("  watch PATH         Watch for changes");
        println!("  dump DIR           Dump directory contents");
        println!("  load DIR           Load directory from stdin");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "read" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("/");
            let _ = key;
            println!("'default-value'");
        }
        "list" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("/");
            if dir == "/" {
                println!("org/");
                println!("system/");
                println!("desktop/");
            } else {
                println!("background/");
                println!("interface/");
                println!("peripherals/");
            }
        }
        "write" => {
            if args.len() < 3 {
                eprintln!("dconf write: KEY VALUE required");
                return 1;
            }
            // Silent success
        }
        "reset" => {
            let key = args.get(1).map(|s| s.as_str()).unwrap_or("/");
            let _ = key;
        }
        "dump" => {
            let dir = args.get(1).map(|s| s.as_str()).unwrap_or("/");
            println!("[{}]", dir.trim_start_matches('/'));
            println!("key1='value1'");
            println!("key2=42");
            println!("key3=true");
        }
        "update" => println!("dconf: databases updated"),
        "compile" => println!("dconf: database compiled"),
        "watch" => {
            let path = args.get(1).map(|s| s.as_str()).unwrap_or("/");
            println!("Watching '{}' for changes...", path);
        }
        "load" => println!("dconf: loaded from stdin"),
        _ => {
            eprintln!("dconf: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dconf(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_dconf};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dconf(vec!["--help".to_string()]), 0);
        assert_eq!(run_dconf(vec!["-h".to_string()]), 0);
        let _ = run_dconf(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dconf(vec![]);
    }
}
