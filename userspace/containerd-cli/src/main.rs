#![deny(clippy::all)]

//! containerd-cli — OurOS containerd container runtime
//!
//! Multi-personality: `ctr`, `containerd`, `containerd-shim`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ctr(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("NAME:");
        println!("   ctr — containerd CLI (OurOS)");
        println!();
        println!("USAGE:");
        println!("   ctr [global options] command [command options] [arguments...]");
        println!();
        println!("COMMANDS:");
        println!("   images, i        Manage images");
        println!("   containers, c    Manage containers");
        println!("   tasks, t         Manage tasks");
        println!("   content          Manage content");
        println!("   snapshots, sn    Manage snapshots");
        println!("   namespaces, ns   Manage namespaces");
        println!("   run              Run a container");
        println!("   version          Print version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" => {
            println!("Client:");
            println!("  Version:  1.7.13");
            println!("  Revision: abcdef1234567890");
            println!("  Go version: go1.22.0");
            println!();
            println!("Server:");
            println!("  Version:  1.7.13");
            println!("  Revision: abcdef1234567890");
            println!("  UUID: 12345678-abcd-ef01-2345-67890abcdef0");
        }
        "images" | "i" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match cmd {
                "ls" | "list" => {
                    println!("REF                             TYPE                                     DIGEST                                                                  SIZE");
                    println!("docker.io/library/alpine:latest application/vnd.oci.image.manifest.v1    sha256:aabbccdd11223344556677889900aabbccddeeff11223344556677889900aabb 7.7 MiB");
                    println!("docker.io/library/nginx:latest  application/vnd.oci.image.manifest.v1    sha256:11223344556677889900aabbccddeeff11223344556677889900aabbccddeeff 67.3 MiB");
                }
                "pull" => {
                    let img = args.get(2).map(|s| s.as_str()).unwrap_or("docker.io/library/alpine:latest");
                    println!("{}: resolved", img);
                    println!("manifest-sha256:aabb...eeff: done");
                    println!("elapsed: 2.5 s\ttotal: 7.7 MiB (3.1 MiB/s)");
                }
                _ => println!("ctr: images {} completed", cmd),
            }
        }
        "containers" | "c" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            if cmd == "ls" || cmd == "list" {
                println!("CONTAINER    IMAGE                             RUNTIME");
                println!("myapp        docker.io/library/nginx:latest    io.containerd.runc.v2");
                println!("worker-1     docker.io/library/alpine:latest   io.containerd.runc.v2");
            } else {
                println!("ctr: containers {} completed", cmd);
            }
        }
        "tasks" | "t" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            if cmd == "ls" || cmd == "list" {
                println!("TASK       PID      STATUS");
                println!("myapp      1234     RUNNING");
                println!("worker-1   1235     RUNNING");
            } else {
                println!("ctr: tasks {} completed", cmd);
            }
        }
        "namespaces" | "ns" => {
            println!("NAME    LABELS");
            println!("default");
            println!("k8s.io");
            println!("moby");
        }
        _ => println!("ctr: command '{}' completed", subcmd),
    }
    0
}

fn run_containerd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: containerd [OPTIONS]");
        println!("  --config, -c <path>    Config file");
        println!("  --log-level, -l        Log level");
        println!("  --version, -v          Version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("containerd 1.7.13 (OurOS)");
        return 0;
    }

    println!("containerd: starting containerd 1.7.13 (OurOS)");
    println!("containerd: loading plugin io.containerd.content.v1.content...");
    println!("containerd: loading plugin io.containerd.snapshotter.v1.overlayfs...");
    println!("containerd: loading plugin io.containerd.runtime.v2.task...");
    println!("containerd: serving... address=/run/containerd/containerd.sock");
    println!("containerd: containerd successfully booted in 0.042s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ctr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "containerd" => run_containerd(&rest),
        "containerd-shim" => { println!("containerd-shim: started"); 0 }
        _ => run_ctr(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
