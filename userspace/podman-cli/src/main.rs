#![deny(clippy::all)]

//! podman-cli — Slate OS Podman rootless container CLI
//!
//! Single personality: `podman`

use std::env;
use std::process;

fn run_podman(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: podman [OPTIONS] <COMMAND>");
        println!();
        println!("Manage pods, containers, and images (rootless, daemonless).");
        println!();
        println!("Commands:");
        println!("  run, start, stop, rm, exec, logs, ps, inspect, stats");
        println!("  build, pull, push, images, rmi, tag");
        println!("  pod create/start/stop/rm/ps/inspect");
        println!("  volume create/ls/rm/inspect");
        println!("  network create/ls/rm/inspect/connect");
        println!("  machine init/start/stop/rm/ls/ssh");
        println!("  generate kube/systemd/spec");
        println!("  play kube");
        println!("  system info/prune/df/connection");
        println!("  compose — Compose support");
        println!("  secret create/ls/rm/inspect");
        println!("  version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Client:       Podman Engine");
            println!("Version:      5.0.0 (Slate OS)");
            println!("API Version:  5.0.0");
            println!("Go Version:   go1.21.6");
            println!("Built:        Thu Jan 15 2024");
            println!("OS/Arch:      slateos/amd64");
            0
        }
        "ps" => {
            println!("CONTAINER ID  IMAGE                 COMMAND     CREATED      STATUS       PORTS                  NAMES");
            println!("abc123def4    docker.io/nginx:1.25  nginx -g…   2 hours ago  Up 2 hours   0.0.0.0:80->80/tcp     web");
            println!("def456abc7    docker.io/redis:7.2   redis-s…    3 hours ago  Up 3 hours   6379/tcp               cache");
            0
        }
        "images" => {
            println!("REPOSITORY                TAG     IMAGE ID      CREATED       SIZE");
            println!("docker.io/library/nginx   1.25    abc123def456  2 weeks ago   192 MB");
            println!("docker.io/library/redis   7.2     def456abc789  3 weeks ago   138 MB");
            0
        }
        "pod" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ps");
            match sub {
                "ps" => {
                    println!("POD ID       NAME        STATUS   CREATED       INFRA ID     # OF CONTAINERS");
                    println!("abc123def4   web-pod     Running  2 hours ago   def456abc7   3");
                }
                "create" => {
                    let name = args.windows(2)
                        .find(|w| w[0] == "--name")
                        .map(|w| w[1].as_str())
                        .unwrap_or("my-pod");
                    println!("Pod {} created.", name);
                }
                _ => println!("Usage: podman pod <create|start|stop|rm|ps|inspect>"),
            }
            0
        }
        "machine" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" | "list" => {
                    println!("NAME                    VM TYPE   CREATED       LAST UP       CPUS  MEMORY   DISK SIZE");
                    println!("podman-machine-default  qemu      1 week ago    2 hours ago   2     2.048GB  100GB");
                }
                "init" => println!("Machine init complete"),
                "start" => println!("Machine started successfully"),
                "stop" => println!("Machine stopped successfully"),
                _ => println!("Usage: podman machine <init|start|stop|rm|ls|ssh|inspect>"),
            }
            0
        }
        "system" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            if sub == "info" {
                println!("host:");
                println!("  os: Slate OS");
                println!("  arch: amd64");
                println!("  cpus: 4");
                println!("  memTotal: 8589934592");
                println!("  rootless: true");
                println!("store:");
                println!("  graphDriverName: overlay");
                println!("  imageStore:");
                println!("    number: 5");
                println!("  containerStore:");
                println!("    number: 3");
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: podman <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_podman(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_podman};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_podman(vec!["--help".to_string()]), 0);
        assert_eq!(run_podman(vec!["-h".to_string()]), 0);
        let _ = run_podman(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_podman(vec![]);
    }
}
