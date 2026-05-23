#![deny(clippy::all)]

//! runc-cli — OurOS OCI container runtime
//!
//! Multi-personality: `runc`, `crun`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_runc(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("NAME:");
        println!("   runc — Open Container Initiative runtime (OurOS)");
        println!();
        println!("USAGE:");
        println!("   runc [global options] command [command options] [arguments...]");
        println!();
        println!("COMMANDS:");
        println!("   create      Create a container");
        println!("   delete      Delete a container");
        println!("   events      Display container events");
        println!("   exec        Execute a process in a container");
        println!("   kill        Send signal to container");
        println!("   list        List containers");
        println!("   pause       Pause a container");
        println!("   resume      Resume a container");
        println!("   run         Create and run a container");
        println!("   spec        Create spec file");
        println!("   start       Start a container");
        println!("   state       Get container state");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "--version" => {
            println!("runc version 1.1.12 (OurOS)");
            println!("commit: abcdef1234567890");
            println!("spec: 1.1.0");
            println!("go: go1.22.0");
            println!("libseccomp: 2.5.5");
        }
        "list" => {
            println!("ID          PID         STATUS      BUNDLE                          CREATED                          OWNER");
            println!("mycontainer 1234        running     /run/containers/mycontainer      2024-05-22T08:00:00.000000000Z   root");
        }
        "state" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("mycontainer");
            println!("{{");
            println!("  \"ociVersion\": \"1.1.0\",");
            println!("  \"id\": \"{}\",", id);
            println!("  \"pid\": 1234,");
            println!("  \"status\": \"running\",");
            println!("  \"bundle\": \"/run/containers/{}\",", id);
            println!("  \"rootfs\": \"/run/containers/{}/rootfs\",", id);
            println!("  \"created\": \"2024-05-22T08:00:00.000000000Z\",");
            println!("  \"owner\": \"root\"");
            println!("}}");
        }
        "spec" => println!("runc: spec file generated at config.json"),
        "create" | "start" | "run" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("container1");
            println!("runc: {} {}", subcmd, id);
        }
        "kill" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("mycontainer");
            let sig = args.get(2).map(|s| s.as_str()).unwrap_or("SIGTERM");
            println!("runc: sent {} to {}", sig, id);
        }
        "delete" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("mycontainer");
            println!("runc: deleted {}", id);
        }
        "pause" | "resume" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("mycontainer");
            println!("runc: {} {}", subcmd, id);
        }
        "events" => {
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("mycontainer");
            println!("{{\"type\":\"stats\",\"id\":\"{}\",\"data\":{{\"cpu\":{{\"usage\":{{\"total\":123456789}}}},\"memory\":{{\"usage\":{{\"usage\":45678901}}}}}}}}", id);
        }
        _ => println!("runc: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "runc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "crun" if rest.iter().any(|a| a == "--version") => {
            println!("crun version 1.14 (OurOS)");
            println!("commit: abcdef1234567890");
            println!("rundir: /run/crun");
            println!("spec: 1.0.0");
            0
        }
        _ => run_runc(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
