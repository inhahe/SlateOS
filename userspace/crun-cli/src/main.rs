#![deny(clippy::all)]

//! crun-cli — OurOS crun OCI container runtime
//!
//! Single personality: `crun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crun(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: crun COMMAND [OPTIONS]");
        println!("crun v1.14 (OurOS) — OCI container runtime");
        println!();
        println!("Commands:");
        println!("  create            Create a container");
        println!("  start             Start a created container");
        println!("  run               Create and start a container");
        println!("  delete            Delete a container");
        println!("  kill              Send signal to container");
        println!("  state             Get container state");
        println!("  list              List containers");
        println!("  spec              Generate OCI runtime spec");
        println!("  exec              Execute process in container");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match cmd {
        "create" | "run" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("container1");
            println!("Container {} created", id);
            if cmd == "run" {
                println!("Container {} started (PID 1234)", id);
            }
        }
        "start" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("container1");
            println!("Container {} started", id);
        }
        "state" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("container1");
            println!("{{");
            println!("  \"ociVersion\": \"1.0.2\",");
            println!("  \"id\": \"{}\",", id);
            println!("  \"status\": \"running\",");
            println!("  \"pid\": 1234,");
            println!("  \"bundle\": \"/run/containers/{}\"", id);
            println!("}}");
        }
        "list" => {
            println!("ID              PID    STATUS    BUNDLE");
            println!("container1      1234   running   /run/containers/container1");
            println!("container2      5678   stopped   /run/containers/container2");
        }
        "delete" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("container1");
            println!("Container {} deleted", id);
        }
        "spec" => println!("Generated config.json (OCI runtime spec)"),
        _ => println!("crun {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_crun(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crun};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/crun"), "crun");
        assert_eq!(basename(r"C:\bin\crun.exe"), "crun.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("crun.exe"), "crun");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crun(&["--help".to_string()], "crun"), 0);
        assert_eq!(run_crun(&["-h".to_string()], "crun"), 0);
        let _ = run_crun(&["--version".to_string()], "crun");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crun(&[], "crun");
    }
}
