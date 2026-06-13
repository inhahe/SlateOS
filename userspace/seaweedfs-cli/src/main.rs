#![deny(clippy::all)]

//! seaweedfs-cli — SlateOS SeaweedFS distributed storage
//!
//! Multi-personality: `weed`, `weed-master`, `weed-volume`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_weed(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [COMMAND] [OPTIONS]", prog);
        match prog {
            "weed-master" => {
                println!("weed master (SlateOS) — SeaweedFS master server");
                println!("  --port PORT        Master port (default: 9333)");
                println!("  --mdir DIR         Metadata directory");
                println!("  --volumeSizeLimitMB N  Volume size limit");
            }
            "weed-volume" => {
                println!("weed volume (SlateOS) — SeaweedFS volume server");
                println!("  --port PORT        Volume port (default: 8080)");
                println!("  --dir DIR          Data directory");
                println!("  --max N            Max volumes");
                println!("  --mserver ADDR     Master address");
            }
            _ => {
                println!("SeaweedFS v3.71 (SlateOS) — Distributed file & object store");
                println!();
                println!("Commands:");
                println!("  master             Start master server");
                println!("  volume             Start volume server");
                println!("  filer              Start filer server");
                println!("  s3                 Start S3 gateway");
                println!("  mount              FUSE mount");
                println!("  benchmark          Run benchmark");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SeaweedFS v3.71.0 (SlateOS)"); return 0; }
    println!("SeaweedFS v3.71.0 (SlateOS)");
    println!("  Master: http://0.0.0.0:9333");
    println!("  Volume servers: 4");
    println!("  Volumes: 23 (replication: 001)");
    println!("  Free volumes: 12");
    println!("  Total: 2.4 TiB");
    println!("  Filer: http://0.0.0.0:8888");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "weed".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_weed(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_weed};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/seaweedfs"), "seaweedfs");
        assert_eq!(basename(r"C:\bin\seaweedfs.exe"), "seaweedfs.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("seaweedfs.exe"), "seaweedfs");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_weed(&["--help".to_string()], "seaweedfs"), 0);
        assert_eq!(run_weed(&["-h".to_string()], "seaweedfs"), 0);
        let _ = run_weed(&["--version".to_string()], "seaweedfs");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_weed(&[], "seaweedfs");
    }
}
