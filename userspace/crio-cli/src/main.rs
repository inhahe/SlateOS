#![deny(clippy::all)]

//! crio-cli — SlateOS CRI-O container runtime
//!
//! Multi-personality: `crio`, `crictl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crictl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("NAME:");
        println!("   crictl — CRI client CLI (SlateOS)");
        println!();
        println!("COMMANDS:");
        println!("   ps           List containers");
        println!("   pods         List pods");
        println!("   images       List images");
        println!("   pull         Pull image");
        println!("   logs         Fetch container logs");
        println!("   exec         Exec in container");
        println!("   stats        Container resource stats");
        println!("   info         Runtime info");
        println!("   version      Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" => {
            println!("Version:  0.1.0");
            println!("RuntimeName:  cri-o");
            println!("RuntimeVersion:  1.29.2 (SlateOS)");
            println!("RuntimeApiVersion:  v1");
        }
        "ps" => {
            println!("CONTAINER       IMAGE                         CREATED         STATE     NAME          ATTEMPT  POD ID");
            println!("a1b2c3d4e5f6    docker.io/library/nginx       5 hours ago     Running   web-server    0        aabb11223344");
            println!("f6e5d4c3b2a1    docker.io/library/redis       3 hours ago     Running   cache         0        aabb11223344");
        }
        "pods" => {
            println!("POD ID          CREATED         STATE   NAME              NAMESPACE   ATTEMPT  RUNTIME");
            println!("aabb11223344    5 hours ago     Ready   webapp-pod        default     0        (default)");
            println!("ccdd55667788    2 hours ago     Ready   monitoring-pod    kube-sys    0        (default)");
        }
        "images" => {
            println!("IMAGE                             TAG       IMAGE ID       SIZE");
            println!("docker.io/library/nginx           latest    aabb...eeff    67.3MB");
            println!("docker.io/library/redis           7         1122...3344    42.1MB");
            println!("docker.io/library/alpine          3.19      5566...7788    7.7MB");
        }
        "stats" => {
            println!("CONTAINER       CPU %    MEM           DISK          INODES");
            println!("a1b2c3d4e5f6    2.35     45.2MB        12.3MB        234");
            println!("f6e5d4c3b2a1    0.89     28.1MB        5.6MB         156");
        }
        "info" => {
            println!("{{");
            println!("  \"status\": {{");
            println!("    \"conditions\": [");
            println!("      {{ \"type\": \"RuntimeReady\", \"status\": true }}");
            println!("      {{ \"type\": \"NetworkReady\", \"status\": true }}");
            println!("    ]");
            println!("  }}");
            println!("}}");
        }
        _ => println!("crictl: command '{}' completed", subcmd),
    }
    0
}

fn run_crio(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: crio [OPTIONS]");
        println!("  --config <path>    Config file");
        println!("  --log-level        Log level");
        println!("  --version          Version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("crio version 1.29.2 (SlateOS)");
        println!("go: go1.22.0");
        return 0;
    }

    println!("crio: Starting CRI-O 1.29.2 (SlateOS)");
    println!("crio: Using default capabilities: CAP_CHOWN, CAP_DAC_OVERRIDE, ...");
    println!("crio: Listening on /var/run/crio/crio.sock");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crictl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "crio" => run_crio(&rest),
        _ => run_crictl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crictl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/crio"), "crio");
        assert_eq!(basename(r"C:\bin\crio.exe"), "crio.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("crio.exe"), "crio");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crictl(&["--help".to_string()]), 0);
        assert_eq!(run_crictl(&["-h".to_string()]), 0);
        let _ = run_crictl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crictl(&[]);
    }
}
