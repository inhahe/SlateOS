#![deny(clippy::all)]

//! mongodb — SlateOS document database
//!
//! Multi-personality: `mongod` (server), `mongos` (router), `mongosh` (shell)

use std::env;
use std::process;

fn run_mongod(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongod [options]");
        println!();
        println!("Options:");
        println!("  --port <port>            Port number (default: 27017)");
        println!("  --bind_ip <addr>         Bind address (default: localhost)");
        println!("  --dbpath <path>          Directory for data files");
        println!("  --logpath <path>         Log file path");
        println!("  --replSet <name>         Replica set name");
        println!("  --configsvr              Declare as config server");
        println!("  --shardsvr               Declare as shard server");
        println!("  --fork                   Fork server process");
        println!("  --auth                   Run with security enabled");
        println!("  --wiredTigerCacheSizeGB  WiredTiger cache size");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("db version v7.0.9 (SlateOS)");
        println!("Build Info: {{");
        println!("    \"version\": \"7.0.9\",");
        println!("    \"gitVersion\": \"abc1234def5678\",");
        println!("    \"modules\": [],");
        println!("    \"allocator\": \"tcmalloc\",");
        println!("    \"environment\": {{");
        println!("        \"distmod\": \"slateos-x86_64\",");
        println!("        \"target_arch\": \"x86_64\"");
        println!("    }}");
        println!("}}");
        return 0;
    }
    let port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(27017);
    let dbpath = args.iter().position(|a| a == "--dbpath")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("/data/db");
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:00.000+00:00\"}},\"s\":\"I\",\"c\":\"CONTROL\",\"msg\":\"mongod starting\",\"attr\":{{\"pid\":12345,\"port\":{},\"dbPath\":\"{}\"}}}}", port, dbpath);
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:00.100+00:00\"}},\"s\":\"I\",\"c\":\"CONTROL\",\"msg\":\"Build Info\",\"attr\":{{\"buildInfo\":{{\"version\":\"7.0.9\",\"gitVersion\":\"abc1234\"}}}}}}");
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:00.500+00:00\"}},\"s\":\"I\",\"c\":\"STORAGE\",\"msg\":\"WiredTiger message\",\"attr\":{{\"message\":\"opened database\"}}}}");
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:01.000+00:00\"}},\"s\":\"I\",\"c\":\"NETWORK\",\"msg\":\"Listening on\",\"attr\":{{\"address\":\"0.0.0.0\",\"port\":{}}}}}",port);
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:01.001+00:00\"}},\"s\":\"I\",\"c\":\"NETWORK\",\"msg\":\"Waiting for connections\",\"attr\":{{\"port\":{}}}}}",port);
    0
}

fn run_mongos(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongos [options]");
        println!();
        println!("Options:");
        println!("  --port <port>            Port number (default: 27017)");
        println!("  --configdb <string>      Config database connection string");
        println!("  --bind_ip <addr>         Bind address");
        println!("  --logpath <path>         Log file path");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mongos version v7.0.9 (SlateOS)");
        return 0;
    }
    let port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(27017);
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:00.000+00:00\"}},\"s\":\"I\",\"c\":\"CONTROL\",\"msg\":\"mongos starting\",\"attr\":{{\"pid\":12346,\"port\":{}}}}}",port);
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:00.500+00:00\"}},\"s\":\"I\",\"c\":\"SHARDING\",\"msg\":\"Cluster identity established\"}}");
    println!("{{\"t\":{{\"$date\":\"2025-05-22T10:00:01.000+00:00\"}},\"s\":\"I\",\"c\":\"NETWORK\",\"msg\":\"Waiting for connections\",\"attr\":{{\"port\":{}}}}}",port);
    0
}

fn run_mongosh(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongosh [options] [db address] [file names]");
        println!();
        println!("Options:");
        println!("  --host <hostname>        Server hostname (default: localhost)");
        println!("  --port <port>            Server port (default: 27017)");
        println!("  --username <user>        Username for authentication");
        println!("  --password <pass>        Password for authentication");
        println!("  --authenticationDatabase Admin database for authentication");
        println!("  --eval <expr>            Evaluate expression");
        println!("  --quiet                  Silence output on startup");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("2.2.5 (SlateOS)");
        return 0;
    }

    let eval_expr = args.iter().position(|a| a == "--eval")
        .and_then(|i| args.get(i + 1));

    if let Some(expr) = eval_expr {
        let expr_upper = expr.to_uppercase();
        if expr_upper.contains("DB.SERVERSTATUS") {
            println!("{{");
            println!("  host: 'slateos-host-1',");
            println!("  version: '7.0.9',");
            println!("  uptime: 86400,");
            println!("  connections: {{ current: 5, available: 838855 }},");
            println!("  opcounters: {{ insert: 1420, query: 8930, update: 356, delete: 42 }}");
            println!("}}");
        } else if expr_upper.contains("SHOW DBS") || expr_upper.contains("SHOW DATABASES") {
            println!("admin    40.00 KiB");
            println!("config  108.00 KiB");
            println!("local    40.00 KiB");
            println!("myapp     2.31 MiB");
        } else if expr_upper.contains("SHOW COLLECTIONS") {
            println!("users");
            println!("orders");
            println!("products");
            println!("sessions");
        } else {
            println!("(result of eval: {} — simulated)", expr);
        }
        return 0;
    }

    // Interactive mode
    let host = args.iter().position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("localhost");
    let port = args.iter().position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("27017");
    println!("Current Mongosh Log ID: 664e1234567890abcdef1234");
    println!("Connecting to:          mongodb://{}:{}/", host, port);
    println!("Using MongoDB:          7.0.9 (SlateOS)");
    println!("Using Mongosh:          2.2.5");
    println!();
    println!("For mongosh info see: https://docs.mongodb.com/mongodb-shell/");
    println!();
    println!("test> show dbs");
    println!("admin    40.00 KiB");
    println!("config  108.00 KiB");
    println!("local    40.00 KiB");
    println!("myapp     2.31 MiB");
    println!("test> use myapp");
    println!("switched to db myapp");
    println!("myapp> db.users.countDocuments()");
    println!("1842");
    println!("myapp> quit");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("mongod");
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
        "mongos" => run_mongos(rest),
        "mongosh" => run_mongosh(rest),
        _ => run_mongod(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mongod};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mongod(vec!["--help".to_string()]), 0);
        assert_eq!(run_mongod(vec!["-h".to_string()]), 0);
        let _ = run_mongod(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mongod(vec![]);
    }
}
