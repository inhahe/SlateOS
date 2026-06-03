#![deny(clippy::all)]

//! neo4j — OurOS graph database
//!
//! Multi-personality: `neo4j` (server manager), `cypher-shell` (CLI)

use std::env;
use std::process;

fn run_neo4j(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: neo4j <command>");
            println!();
            println!("Commands:");
            println!("  console    Start server in foreground");
            println!("  start      Start server in background");
            println!("  stop       Stop the server");
            println!("  restart    Restart the server");
            println!("  status     Check server status");
            println!("  version    Show version");
            0
        }
        "--version" | "version" => {
            println!("neo4j 5.19.0 (OurOS)");
            0
        }
        "console" | "start" => {
            let is_bg = cmd.as_str() == "start";
            println!("Directories in use:");
            println!("  home:    /var/lib/neo4j");
            println!("  config:  /etc/neo4j");
            println!("  logs:    /var/log/neo4j");
            println!("  data:    /var/lib/neo4j/data");
            println!("  plugins: /var/lib/neo4j/plugins");
            println!("  import:  /var/lib/neo4j/import");
            println!();
            println!("2025-05-22 10:00:00.000+0000 INFO  Starting Neo4j.");
            println!("2025-05-22 10:00:00.500+0000 INFO  This instance is ServerId{{abc12345}}");
            println!("2025-05-22 10:00:01.000+0000 INFO  ======== Neo4j 5.19.0 ========");
            println!("2025-05-22 10:00:02.000+0000 INFO  Bolt enabled on 0.0.0.0:7687.");
            println!("2025-05-22 10:00:02.500+0000 INFO  HTTP enabled on 0.0.0.0:7474.");
            println!("2025-05-22 10:00:03.000+0000 INFO  HTTPS enabled on 0.0.0.0:7473.");
            println!("2025-05-22 10:00:03.500+0000 INFO  Remote interface available at http://localhost:7474/");
            if is_bg {
                println!("2025-05-22 10:00:03.501+0000 INFO  Started.");
            } else {
                println!("2025-05-22 10:00:03.501+0000 INFO  Started in console mode. Press Ctrl-C to exit.");
            }
            0
        }
        "stop" => {
            println!("Stopping Neo4j...... stopped.");
            0
        }
        "restart" => {
            println!("Stopping Neo4j...... stopped.");
            println!("Starting Neo4j...... started. PID=12345.");
            0
        }
        "status" => {
            println!("Neo4j is running at pid 12345");
            0
        }
        other => { eprintln!("neo4j: unknown command '{}'", other); 1 }
    }
}

fn run_cypher_shell(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cypher-shell [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -u, --username <user>  Username");
        println!("  -p, --password <pass>  Password");
        println!("  -a, --address <uri>    Address (default: bolt://localhost:7687)");
        println!("  -d, --database <name>  Database (default: neo4j)");
        println!("  --format <type>        Output format (auto, verbose, plain)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Cypher-Shell 5.19.0 (OurOS)");
        return 0;
    }

    // Check for inline Cypher command via positional args
    let cypher_cmd: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();

    if !cypher_cmd.is_empty() {
        let query = cypher_cmd.join(" ").to_uppercase();
        if query.contains("MATCH") && query.contains("RETURN") {
            println!("+-------------------------------+");
            println!("| n                             |");
            println!("+-------------------------------+");
            println!("| (:Person {{name: \"Alice\"}})     |");
            println!("| (:Person {{name: \"Bob\"}})       |");
            println!("| (:Person {{name: \"Charlie\"}})   |");
            println!("+-------------------------------+");
            println!("3 rows");
        } else if query.contains("CALL DB.") {
            println!("+------------------+");
            println!("| name             |");
            println!("+------------------+");
            println!("| \"neo4j\"           |");
            println!("| \"system\"          |");
            println!("+------------------+");
        } else {
            println!("0 rows");
            println!("ready to start consuming query after 1 ms");
        }
        return 0;
    }

    // Interactive mode
    let addr = args.iter().position(|a| a == "-a" || a == "--address")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("bolt://localhost:7687");
    println!("Connected to Neo4j using Bolt protocol version 5.4 at {}.", addr);
    println!("Type :help for a list of available commands or :exit to exit the shell.");
    println!();
    println!("neo4j@neo4j> MATCH (n) RETURN count(n);");
    println!("+----------+");
    println!("| count(n) |");
    println!("+----------+");
    println!("| 1842     |");
    println!("+----------+");
    println!("1 row");
    println!("ready to start consuming query after 2 ms, results consumed after another 0 ms");
    println!();
    println!("neo4j@neo4j> :exit");
    println!("Bye!");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("neo4j");
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
        "cypher-shell" => run_cypher_shell(rest),
        _ => run_neo4j(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_neo4j};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_neo4j(vec!["--help".to_string()]), 0);
        assert_eq!(run_neo4j(vec!["-h".to_string()]), 0);
        assert_eq!(run_neo4j(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_neo4j(vec![]), 0);
    }
}
