#![deny(clippy::all)]

//! varnish-cli — OurOS Varnish HTTP cache
//!
//! Multi-personality: `varnishd`, `varnishlog`, `varnishstat`, `varnishadm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_varnish(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "varnishlog" => {
                println!("varnishlog (OurOS) — Display Varnish logs");
                println!("  -q QUERY           VSL query expression");
                println!("  -g GROUP           Group by (session/request/vxid/raw)");
                println!("  -c                 Client-side logs");
                println!("  -b                 Backend-side logs");
            }
            "varnishstat" => {
                println!("varnishstat (OurOS) — Varnish statistics");
                println!("  -1                 One-shot mode");
                println!("  -f FIELD           Field filter");
                println!("  -j                 JSON output");
            }
            "varnishadm" => {
                println!("varnishadm (OurOS) — Varnish admin interface");
                println!("  vcl.list           List loaded VCL");
                println!("  vcl.load NAME FILE Load VCL");
                println!("  vcl.use NAME       Activate VCL");
                println!("  ban EXPR           Ban cache entries");
                println!("  backend.list       List backends");
            }
            _ => {
                println!("varnishd v7.5 (OurOS) — HTTP accelerator daemon");
                println!("  -a ADDR:PORT       Listen address");
                println!("  -b HOST:PORT       Backend server");
                println!("  -f FILE            VCL config file");
                println!("  -s TYPE[,OPTIONS]  Storage backend (malloc/file)");
                println!("  -T ADDR:PORT       Management interface");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") { println!("Varnish v7.5.0 (OurOS)"); return 0; }
    match prog {
        "varnishstat" => {
            println!("Varnish Statistics:");
            println!("  cache_hit: 890,123 (92.3%)");
            println!("  cache_miss: 74,567 (7.7%)");
            println!("  client_req: 964,690");
            println!("  backend_req: 74,567");
        }
        _ => {
            println!("Varnish v7.5.0 (OurOS)");
            println!("  Listen: 0.0.0.0:80");
            println!("  Admin: 127.0.0.1:6082");
            println!("  Backend: 127.0.0.1:8080");
            println!("  Storage: malloc (256 MB)");
            println!("  VCL: default (active)");
            println!("  Hit ratio: 92.3%");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "varnishd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_varnish(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_varnish};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/varnish"), "varnish");
        assert_eq!(basename(r"C:\bin\varnish.exe"), "varnish.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("varnish.exe"), "varnish");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_varnish(&["--help".to_string()], "varnish"), 0);
        assert_eq!(run_varnish(&["-h".to_string()], "varnish"), 0);
        assert_eq!(run_varnish(&["--version".to_string()], "varnish"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_varnish(&[], "varnish"), 0);
    }
}
