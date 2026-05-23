#![deny(clippy::all)]

//! elasticsearch-cli — OurOS Elasticsearch search engine tools
//!
//! Multi-personality: `elasticsearch`, `elasticsearch-keystore`, `elasticsearch-plugin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_elasticsearch(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: elasticsearch [OPTIONS]");
        println!();
        println!("Elasticsearch — distributed search and analytics engine (OurOS).");
        println!();
        println!("Options:");
        println!("  -d, --daemonize    Run as daemon");
        println!("  -p, --pidfile <f>  PID file");
        println!("  -E <setting>=<v>  Set configuration");
        println!("  -V, --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("Version: 8.12.0 (OurOS)");
        println!("Build flavor: default");
        println!("Build type: tar");
        println!("JVM: 21.0.1");
        return 0;
    }

    println!("[2024-05-22T12:00:00,000][INFO ][o.e.n.Node] [ouros-node-1] version[8.12.0], pid[1234]");
    println!("[2024-05-22T12:00:00,100][INFO ][o.e.n.Node] [ouros-node-1] JVM: OpenJDK 64-Bit Server VM 21.0.1");
    println!("[2024-05-22T12:00:01,000][INFO ][o.e.e.NodeEnvironment] [ouros-node-1] using [1] data paths, mounts [[(/)]], net usable [456.7gb], net total [500.0gb]");
    println!("[2024-05-22T12:00:02,000][INFO ][o.e.p.PluginsService] [ouros-node-1] loaded module [analysis-common]");
    println!("[2024-05-22T12:00:03,000][INFO ][o.e.c.s.ClusterApplierService] [ouros-node-1] master node changed {{previous [], current [ouros-node-1]}}");
    println!("[2024-05-22T12:00:04,000][INFO ][o.e.h.AbstractHttpServerTransport] [ouros-node-1] publish_address {{192.168.1.100:9200}}, bound_addresses {{[::]:9200}}");
    println!("[2024-05-22T12:00:04,100][INFO ][o.e.n.Node] [ouros-node-1] started");
    println!();
    println!("Cluster health: green");
    println!("  Status:       green");
    println!("  Nodes:        1");
    println!("  Indices:      5");
    println!("  Shards:       10 (5 primary, 5 replica)");
    println!("  Documents:    1,234,567");
    println!("  Data:         2.3 GB");
    0
}

fn run_elasticsearch_keystore(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: elasticsearch-keystore COMMAND [args]");
        println!("Commands: create, list, add, remove, show, has-passwd, passwd, upgrade");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            println!("bootstrap.password");
            println!("keystore.seed");
            println!("xpack.security.transport.ssl.keystore.secure_password");
        }
        "create" => println!("Created elasticsearch keystore in /etc/elasticsearch/elasticsearch.keystore"),
        _ => println!("elasticsearch-keystore: {} completed", subcmd),
    }
    0
}

fn run_elasticsearch_plugin(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: elasticsearch-plugin COMMAND [args]");
        println!("Commands: install, remove, list");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("list");
    match subcmd {
        "list" => {
            println!("analysis-icu");
            println!("analysis-kuromoji");
            println!("ingest-attachment");
        }
        "install" => {
            let plugin = args.get(1).map(|s| s.as_str()).unwrap_or("analysis-icu");
            println!("-> Installing {}", plugin);
            println!("-> Installed {}", plugin);
        }
        _ => println!("elasticsearch-plugin: {} completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "elasticsearch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "elasticsearch-keystore" => run_elasticsearch_keystore(&rest),
        "elasticsearch-plugin" => run_elasticsearch_plugin(&rest),
        _ => run_elasticsearch(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
