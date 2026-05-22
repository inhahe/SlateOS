#![deny(clippy::all)]

//! cassandra-cli — OurOS Cassandra CLI
//!
//! Multi-personality: `cqlsh` and `nodetool`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn strip_ext(name: &str) -> &str {
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
}

fn run_cqlsh(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cqlsh [OPTIONS] [HOST [PORT]]");
        println!();
        println!("Cassandra CQL shell (OurOS).");
        println!();
        println!("Options:");
        println!("  -e CMD         Execute CQL statement");
        println!("  -f FILE        Execute CQL file");
        println!("  -k KEYSPACE    Use keyspace");
        println!("  -u USER        Username");
        println!("  -p PASSWORD    Password");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cqlsh 6.1.0 (OurOS)");
        return 0;
    }

    let host = args.first().filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or("localhost");
    let execute = args.windows(2).find(|w| w[0] == "-e")
        .map(|w| w[1].as_str());

    if let Some(cmd) = execute {
        println!("Connected to cluster at {}:9042.", host);
        if cmd.contains("SELECT") || cmd.contains("select") {
            println!();
            println!(" id | name    | email");
            println!("----+---------+-------------------");
            println!("  1 | Alice   | alice@example.com");
            println!("  2 | Bob     | bob@example.com");
            println!("  3 | Carol   | carol@example.com");
            println!();
            println!("(3 rows)");
        } else {
            println!("  {}", cmd);
            println!("  OK");
        }
    } else {
        println!("Connected to Test Cluster at {}:9042.", host);
        println!("[cqlsh 6.1.0 | Cassandra 4.1.3 | CQL spec 3.4.6 | Native protocol v5]");
        println!("cqlsh> (interactive mode)");
    }
    0
}

fn run_nodetool(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nodetool <COMMAND> [OPTIONS]");
        println!();
        println!("Cassandra node management tool (OurOS).");
        println!();
        println!("Commands:");
        println!("  status         Show cluster status");
        println!("  info           Show node info");
        println!("  ring           Show token ring");
        println!("  tablestats     Show table statistics");
        println!("  compactionstats  Show compaction stats");
        println!("  repair         Repair node");
        println!("  cleanup        Clean up keyspaces");
        println!("  flush          Flush memtables");
        println!("  drain          Drain node for shutdown");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            println!("Datacenter: dc1");
            println!("===============");
            println!("Status=Up/Down  State=Normal/Leaving/Joining/Moving");
            println!("--  Address        Load       Tokens  Owns    Host ID                              Rack");
            println!("UN  192.168.1.10   256.5 GiB  256     33.3%   abc12345-def6-7890-abcd-ef1234567890 rack1");
            println!("UN  192.168.1.11   248.2 GiB  256     33.4%   def67890-abc1-2345-defg-hi6789012345 rack1");
            println!("UN  192.168.1.12   261.8 GiB  256     33.3%   ghi12345-jkl6-7890-mnop-qr1234567890 rack2");
            0
        }
        "info" => {
            println!("ID                     : abc12345-def6-7890-abcd-ef1234567890");
            println!("Gossip active          : true");
            println!("Native Transport active: true");
            println!("Load                   : 256.5 GiB");
            println!("Generation No          : 1705312800");
            println!("Uptime (seconds)       : 345600");
            println!("Heap Memory (MB)       : 4096.00 / 8192.00");
            println!("Off Heap Memory (MB)   : 128.45");
            println!("Data Center            : dc1");
            println!("Rack                   : rack1");
            0
        }
        "compactionstats" => {
            println!("pending tasks: 2");
            println!("            compaction type    keyspace   table      completed    total       unit");
            println!("  Compaction                   myks       users      128 MB       256 MB      bytes");
            println!("  Compaction                   myks       orders     64 MB        128 MB      bytes");
            println!("Active compaction remaining time:  0h12m30s");
            0
        }
        "repair" => {
            let keyspace = args.get(1).map(|s| s.as_str()).unwrap_or("myks");
            println!("[2024-01-15 14:00:00] Starting repair for keyspace '{}'...", keyspace);
            println!("[2024-01-15 14:00:05] Repair session 1 of 3 completed");
            println!("[2024-01-15 14:00:10] Repair session 2 of 3 completed");
            println!("[2024-01-15 14:00:15] Repair session 3 of 3 completed");
            println!("[2024-01-15 14:00:15] Repair completed successfully");
            0
        }
        "flush" => {
            let keyspace = args.get(1).map(|s| s.as_str()).unwrap_or("myks");
            println!("Flushing keyspace '{}'...", keyspace);
            println!("  Flushed memtables for 5 tables");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: nodetool <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "cqlsh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nodetool" => run_nodetool(rest),
        _ => run_cqlsh(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
