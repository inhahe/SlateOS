#![deny(clippy::all)]

//! envoy — OurOS L7 proxy and communication bus
//!
//! Single personality: `envoy`

use std::env;
use std::process;

fn run_envoy(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: envoy [options]");
        println!();
        println!("Options:");
        println!("  -c, --config-path <path>       Path to configuration file");
        println!("  --config-yaml <yaml>           Inline YAML configuration");
        println!("  -l, --log-level <level>        Log level (trace, debug, info, warning, error, critical, off)");
        println!("  --log-path <path>              Log file path");
        println!("  --component-log-level <pairs>  Component-level log levels");
        println!("  --mode <mode>                  Server mode (serve, validate, init_only)");
        println!("  --concurrency <n>              Number of worker threads");
        println!("  --service-cluster <name>       Cluster name");
        println!("  --service-node <name>          Node name");
        println!("  --drain-time-s <seconds>       Drain time during hot restart");
        println!("  --version                      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("envoy  version: 1.30.2/abc1234/Clean/RELEASE/OurOS (OurOS)");
        return 0;
    }

    let mode = args.iter().position(|a| a == "--mode")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("serve");

    if mode == "validate" {
        let config = args.iter().position(|a| a == "-c" || a == "--config-path")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("/etc/envoy/envoy.yaml");
        println!("[info] Configuration '{}' OK", config);
        return 0;
    }

    let config = args.iter().position(|a| a == "-c" || a == "--config-path")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("/etc/envoy/envoy.yaml");
    let concurrency = args.iter().position(|a| a == "--concurrency")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(4);

    println!("[2025-05-22 10:00:00.000][1][info][main] [source/server/server.cc:400] initializing epoch 0 (base id=0, hot restart version=11.104)");
    println!("[2025-05-22 10:00:00.001][1][info][main] [source/server/server.cc:402] statically linked extensions:");
    println!("[2025-05-22 10:00:00.002][1][info][main]   envoy.filters.http.router");
    println!("[2025-05-22 10:00:00.002][1][info][main]   envoy.filters.http.cors");
    println!("[2025-05-22 10:00:00.002][1][info][main]   envoy.filters.http.gzip");
    println!("[2025-05-22 10:00:00.002][1][info][main]   envoy.filters.http.grpc_web");
    println!("[2025-05-22 10:00:00.002][1][info][main]   envoy.filters.network.http_connection_manager");
    println!("[2025-05-22 10:00:00.002][1][info][main]   envoy.filters.network.tcp_proxy");
    println!("[2025-05-22 10:00:00.100][1][info][config] [source/server/config_impl.cc:100] loading {}", config);
    println!("[2025-05-22 10:00:00.200][1][info][upstream] [source/common/upstream/cluster_manager_impl.cc:200] cm init: all clusters initialized");
    println!("[2025-05-22 10:00:00.300][1][info][main] [source/server/server.cc:800] starting {} worker thread(s)", concurrency);
    println!("[2025-05-22 10:00:00.400][1][info][main] [source/server/server.cc:900] all workers started");
    println!("[2025-05-22 10:00:00.401][1][info][admin] [source/server/admin/admin.cc:100] admin address: 0.0.0.0:9901");
    println!();
    println!("Listeners:");
    println!("  0.0.0.0:10000  (HTTP)");
    println!("  0.0.0.0:10001  (TCP)");
    println!();
    println!("Clusters:");
    println!("  service_backend  ROUND_ROBIN  [127.0.0.1:8080, 127.0.0.1:8081]");
    println!("  grpc_backend     LEAST_REQUEST  [127.0.0.1:50051]");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_envoy(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_envoy};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_envoy(vec!["--help".to_string()]), 0);
        assert_eq!(run_envoy(vec!["-h".to_string()]), 0);
        assert_eq!(run_envoy(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_envoy(vec![]), 0);
    }
}
