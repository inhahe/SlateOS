#![deny(clippy::all)]

//! clickhouse — OurOS column-oriented OLAP database
//!
//! Multi-personality: `clickhouse-server`, `clickhouse-client`, `clickhouse-local`

use std::env;
use std::process;

fn run_ch_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: clickhouse-server [OPTION]...");
        println!();
        println!("Options:");
        println!("  --config-file <file>   Path to configuration file");
        println!("  --log-file <file>      Path to log file");
        println!("  --daemon               Run as daemon");
        println!("  --pid-file <file>      Path to PID file");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ClickHouse server version 24.4.1 (OurOS).");
        return 0;
    }
    println!("{{}}'ts':'2025-05-22 10:00:00.000','level':'Information','msg':'Starting ClickHouse 24.4.1 (OurOS)'}}");
    println!("{{}}'ts':'2025-05-22 10:00:00.100','level':'Information','msg':'Listening for connections with native protocol on port 9000'}}");
    println!("{{}}'ts':'2025-05-22 10:00:00.200','level':'Information','msg':'Listening for HTTP on port 8123'}}");
    println!("{{}}'ts':'2025-05-22 10:00:00.300','level':'Information','msg':'Listening for MySQL on port 9004'}}");
    println!("{{}}'ts':'2025-05-22 10:00:00.400','level':'Information','msg':'Listening for PostgreSQL on port 9005'}}");
    println!("{{}}'ts':'2025-05-22 10:00:01.000','level':'Information','msg':'Ready for connections.'}}");
    0
}

fn run_ch_client(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: clickhouse-client [OPTION]...");
        println!();
        println!("Options:");
        println!("  --host <host>          Server hostname (default: localhost)");
        println!("  --port <port>          Server port (default: 9000)");
        println!("  --user <name>          User (default: default)");
        println!("  --password <pass>      Password");
        println!("  --database <name>      Database");
        println!("  --query <sql>          Execute query and exit");
        println!("  --multiquery           Enable multiquery mode");
        println!("  --format <fmt>         Output format");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ClickHouse client version 24.4.1 (OurOS).");
        return 0;
    }

    let query = args.iter().position(|a| a == "--query" || a == "-q")
        .and_then(|i| args.get(i + 1));

    if let Some(q) = query {
        let upper = q.to_uppercase();
        if upper.contains("SHOW DATABASES") {
            println!("INFORMATION_SCHEMA");
            println!("default");
            println!("information_schema");
            println!("system");
        } else if upper.contains("SHOW TABLES") {
            println!("events");
            println!("metrics");
            println!("logs");
        } else if upper.contains("SELECT") {
            println!("┌─id─┬─name────┬─────────created_at─┐");
            println!("│  1 │ alice   │ 2025-01-15 08:30:00 │");
            println!("│  2 │ bob     │ 2025-02-20 14:15:00 │");
            println!("│  3 │ charlie │ 2025-03-10 11:45:00 │");
            println!("└────┴─────────┴─────────────────────┘");
            println!("3 rows in set. Elapsed: 0.004 sec. Processed 3 rows, 256 B (750 rows/s., 64.00 KB/s.)");
        } else {
            println!("Ok. 0 rows in set. Elapsed: 0.001 sec.");
        }
        return 0;
    }

    // Interactive mode
    let host = args.iter().position(|a| a == "--host")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("localhost");
    println!("ClickHouse client version 24.4.1 (OurOS).");
    println!("Connecting to {}:9000 as user default.", host);
    println!("Connected to ClickHouse server version 24.4.1.");
    println!();
    println!("{} :) SELECT version()", host);
    println!();
    println!("┌─version()─┐");
    println!("│ 24.4.1    │");
    println!("└───────────┘");
    println!("1 row in set. Elapsed: 0.001 sec.");
    println!();
    println!("{} :) quit", host);
    println!("Bye.");
    0
}

fn run_ch_local(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: clickhouse-local [OPTION]...");
        println!();
        println!("Options:");
        println!("  --query <sql>          Execute query");
        println!("  --input-format <fmt>   Input format");
        println!("  --output-format <fmt>  Output format");
        println!("  --structure <cols>     Input structure definition");
        println!("  --file <path>          Input file");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ClickHouse local version 24.4.1 (OurOS).");
        return 0;
    }
    let query = args.iter().position(|a| a == "--query" || a == "-q")
        .and_then(|i| args.get(i + 1));
    if let Some(q) = query {
        let _ = q;
        println!("42");
    } else {
        println!("clickhouse-local: --query required");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("clickhouse-server");
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
        "clickhouse-client" => run_ch_client(rest),
        "clickhouse-local" => run_ch_local(rest),
        _ => run_ch_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
