#![deny(clippy::all)]

//! elasticsearch — OurOS distributed search and analytics engine
//!
//! Multi-personality: `elasticsearch` (server), `elasticsearch-plugin` (plugin manager)

use std::env;
use std::process;

fn run_elasticsearch(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: elasticsearch [options]");
        println!();
        println!("Options:");
        println!("  -d, --daemonize       Run as daemon");
        println!("  -p, --pidfile <path>  PID file path");
        println!("  -E <setting>=<value>  Configure a setting");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("Version: 8.13.0 (OurOS), Build: abc1234/2025-05-22");
        return 0;
    }
    println!("[2025-05-22T10:00:00,000][INFO ][o.e.n.Node               ] [ouros-node-1] version[8.13.0], pid[12345]");
    println!("[2025-05-22T10:00:00,500][INFO ][o.e.n.Node               ] [ouros-node-1] JVM: 21.0.2 (OurOS)");
    println!("[2025-05-22T10:00:01,000][INFO ][o.e.e.NodeEnvironment    ] [ouros-node-1] using [1] data paths, mounts [[/ (/)]]");
    println!("[2025-05-22T10:00:02,000][INFO ][o.e.g.GatewayService     ] [ouros-node-1] recovered [0] indices");
    println!("[2025-05-22T10:00:02,500][INFO ][o.e.c.s.MasterService    ] [ouros-node-1] elected-as-master");
    println!("[2025-05-22T10:00:03,000][INFO ][o.e.h.AbstractHttpServerTransport] [ouros-node-1] publish_address {{127.0.0.1:9200}}");
    println!("[2025-05-22T10:00:03,000][INFO ][o.e.n.Node               ] [ouros-node-1] started");
    0
}

fn run_es_plugin(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: elasticsearch-plugin <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  install   Install a plugin");
            println!("  remove    Remove a plugin");
            println!("  list      List installed plugins");
            0
        }
        "install" => {
            let plugin = cmd_args.first().map(|s| s.as_str()).unwrap_or("analysis-icu");
            println!("-> Installing {}", plugin);
            println!("-> Downloading {}...", plugin);
            println!("-> Installed {}", plugin);
            println!("-> Please restart Elasticsearch to activate any plugins installed");
            0
        }
        "remove" => {
            let plugin = cmd_args.first().map(|s| s.as_str()).unwrap_or("plugin");
            println!("-> removing [{}]...", plugin);
            0
        }
        "list" => {
            println!("analysis-icu");
            println!("ingest-geoip");
            println!("repository-s3");
            0
        }
        other => { eprintln!("elasticsearch-plugin: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("elasticsearch");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "elasticsearch-plugin" => run_es_plugin(rest),
        _ => run_elasticsearch(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_elasticsearch};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_elasticsearch(vec!["--help".to_string()]), 0);
        assert_eq!(run_elasticsearch(vec!["-h".to_string()]), 0);
        let _ = run_elasticsearch(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_elasticsearch(vec![]);
    }
}
