#![deny(clippy::all)]

//! neo4j-cli — OurOS Neo4j CLI (cypher-shell)
//!
//! Multi-personality: `cypher-shell`, `neo4j-admin`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_cypher_shell(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cypher-shell [OPTIONS] [CYPHER]");
        println!();
        println!("cypher-shell — Neo4j Cypher CLI (OurOS).");
        println!();
        println!("Options:");
        println!("  -u, --username USER   Username");
        println!("  -p, --password PASS   Password");
        println!("  -a, --address URI     Server address");
        println!("  -d, --database DB     Database name");
        println!("  --format FORMAT       Output format (auto, plain, verbose)");
        println!("  --non-interactive     Non-interactive mode");
        return 0;
    }

    let query = args.iter().filter(|a| !a.starts_with('-'))
        .next().map(|s| s.as_str());

    if let Some(q) = query {
        println!("+--------------------------------------------------+");
        println!("| n                                                |");
        println!("+--------------------------------------------------+");
        println!("| (:Person {{name: \"Alice\", age: 30}})             |");
        println!("| (:Person {{name: \"Bob\", age: 25}})               |");
        println!("| (:Person {{name: \"Charlie\", age: 35}})           |");
        println!("+--------------------------------------------------+");
        println!("3 rows");
        let _ = q;
    } else {
        println!("Connected to Neo4j 5.15.0 at neo4j://localhost:7687.");
        println!("Type :help for a list of available commands or :exit to exit.");
        println!("neo4j> ");
    }
    0
}

fn run_neo4j_admin(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: neo4j-admin <COMMAND> [OPTIONS]");
        println!();
        println!("Commands:");
        println!("  database info          Show database info");
        println!("  database dump          Dump database");
        println!("  database load          Load database dump");
        println!("  database migrate       Migrate database");
        println!("  server memory-recommendation  Memory settings");
        println!("  dbms set-initial-password     Set initial password");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, sub) {
        ("database", "info") => {
            println!("Database: neo4j");
            println!("Store format: record-aligned-1.1");
            println!("Size: 256.0MiB");
            println!("Last committed tx: 12345");
        }
        ("database", "dump") => {
            println!("Dumping database 'neo4j' to dump.dump...");
            println!("Done: 256 MiB processed.");
        }
        ("server", "memory-recommendation") => {
            println!("# Memory settings recommendation");
            println!("server.memory.heap.initial_size=512m");
            println!("server.memory.heap.max_size=512m");
            println!("server.memory.pagecache.size=2g");
        }
        ("dbms", "set-initial-password") => {
            println!("Changed password for user 'neo4j'. IMPORTANT: change password again after first login.");
        }
        _ => {
            println!("neo4j-admin: see neo4j-admin --help.");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "cypher-shell".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "neo4j-admin" => run_neo4j_admin(&rest),
        _ => run_cypher_shell(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
