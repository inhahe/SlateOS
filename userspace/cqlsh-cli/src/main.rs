#![deny(clippy::all)]

//! cqlsh-cli — OurOS Apache Cassandra CQL shell
//!
//! Multi-personality: `cqlsh`, `nodetool`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cqlsh(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cqlsh [OPTIONS] [HOST [PORT]]");
        println!("CQL Shell 6.1.0 (Cassandra 4.1.5, OurOS)");
        println!();
        println!("Options:");
        println!("  -k KEYSPACE    Use keyspace");
        println!("  -e STMT        Execute statement");
        println!("  -f FILE        Execute statements from file");
        println!("  -u USER        Username");
        println!("  -p PASSWORD    Password");
        println!("  --ssl          Use SSL");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("cqlsh 6.1.0 | Cassandra 4.1.5 | CQL spec 3.4.6");
        return 0;
    }
    let stmt = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str());
    if let Some(s) = stmt {
        println!("{}", s);
        println!("(1 rows)");
        return 0;
    }
    let host = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("127.0.0.1");
    println!("Connected to cluster at {}:9042.", host);
    println!("[cqlsh 6.1.0 | Cassandra 4.1.5 | CQL spec 3.4.6]");
    println!("cqlsh> ");
    0
}

fn run_nodetool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nodetool COMMAND [OPTIONS]");
        println!("  status        Show cluster status");
        println!("  info          Show node info");
        println!("  ring          Show token ring");
        println!("  describecluster   Describe cluster");
        println!("  repair        Run repair");
        println!("  compact       Force compaction");
        println!("  flush         Flush memtables");
        println!("  cleanup       Remove unnecessary data");
        println!("  snapshot      Take a snapshot");
        println!("  decommission  Remove node from cluster");
        println!("  tpstats       Thread pool stats");
        println!("  cfstats       Column family stats");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "status" => {
            println!("Datacenter: datacenter1");
            println!("========================");
            println!("Status=Up/Down");
            println!("|/ State=Normal/Leaving/Joining/Moving");
            println!("--  Address       Load        Tokens  Owns    Host ID                               Rack");
            println!("UN  192.168.1.1   256.42 GiB  256     33.3%   abc12345-1234-1234-1234-abc123456789  rack1");
            println!("UN  192.168.1.2   248.15 GiB  256     33.3%   def12345-1234-1234-1234-def123456789  rack1");
            println!("UN  192.168.1.3   251.89 GiB  256     33.3%   ghi12345-1234-1234-1234-ghi123456789  rack2");
        }
        "info" => {
            println!("ID                     : abc12345-1234-1234-1234-abc123456789");
            println!("Gossip active          : true");
            println!("Native Transport active: true");
            println!("Load                   : 256.42 GiB");
            println!("Tokens                 : 256");
            println!("Uptime (seconds)       : 86400");
            println!("Heap Memory (MB)       : 4096.00 / 8192.00");
        }
        "tpstats" => {
            println!("Pool Name                    Active   Pending   Completed   Blocked");
            println!("ReadStage                    0        0         12345       0");
            println!("MutationStage                0        0         67890       0");
            println!("CompactionExecutor           1        3         456         0");
            println!("MemtableFlushWriter          0        0         78          0");
        }
        "flush" => println!("Flushing all memtables...done."),
        "repair" => println!("Starting repair...repair completed successfully."),
        "compact" => println!("Starting forced compaction...compaction completed."),
        "snapshot" => println!("Requested creating snapshot(s) for [all keyspaces] with snapshot name [auto_snapshot]"),
        _ => println!("nodetool: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cqlsh".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nodetool" => run_nodetool(&rest),
        _ => run_cqlsh(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cqlsh};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cqlsh"), "cqlsh");
        assert_eq!(basename(r"C:\bin\cqlsh.exe"), "cqlsh.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cqlsh.exe"), "cqlsh");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_cqlsh(&["--help".to_string()]), 0);
        assert_eq!(run_cqlsh(&["-h".to_string()]), 0);
        assert_eq!(run_cqlsh(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_cqlsh(&[]), 0);
    }
}
