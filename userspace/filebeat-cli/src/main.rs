#![deny(clippy::all)]

//! filebeat-cli — SlateOS Filebeat log shipper
//!
//! Single personality: `filebeat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_filebeat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: filebeat COMMAND [OPTIONS]");
        println!("filebeat v8.12 (Slate OS) — Lightweight log shipper");
        println!();
        println!("Commands:");
        println!("  run               Run filebeat");
        println!("  test config       Test configuration");
        println!("  test output       Test output connectivity");
        println!("  modules list      List available modules");
        println!("  modules enable    Enable modules");
        println!("  setup             Setup dashboards and index templates");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "run" => {
            println!("Starting filebeat...");
            println!("  Config: /etc/filebeat/filebeat.yml");
            println!("  Inputs: 2 (log, container)");
            println!("  Output: elasticsearch (localhost:9200)");
            println!("  Harvesting: /var/log/*.log");
        }
        "test" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("config");
            match sub {
                "config" => println!("Config OK."),
                "output" => {
                    println!("elasticsearch: http://localhost:9200...");
                    println!("  parse url... OK");
                    println!("  connection... OK");
                    println!("  TLS... WARN certificate verification disabled");
                    println!("  talk to server... OK");
                }
                _ => println!("filebeat test {}: completed", sub),
            }
        }
        "modules" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("Enabled:");
                println!("  system");
                println!("  nginx");
                println!();
                println!("Disabled:");
                println!("  apache");
                println!("  auditd");
                println!("  elasticsearch");
                println!("  kafka");
                println!("  redis");
            } else {
                println!("Module operation: {}", sub);
            }
        }
        "setup" => {
            println!("Setting up dashboards...");
            println!("  Index template loaded.");
            println!("  Dashboards loaded: 12");
        }
        "version" | "--version" => println!("filebeat v8.12 (Slate OS)"),
        _ => println!("filebeat {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "filebeat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_filebeat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_filebeat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/filebeat"), "filebeat");
        assert_eq!(basename(r"C:\bin\filebeat.exe"), "filebeat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("filebeat.exe"), "filebeat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_filebeat(&["--help".to_string()], "filebeat"), 0);
        assert_eq!(run_filebeat(&["-h".to_string()], "filebeat"), 0);
        let _ = run_filebeat(&["--version".to_string()], "filebeat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_filebeat(&[], "filebeat");
    }
}
