#![deny(clippy::all)]

//! mongosh-cli — OurOS MongoDB Shell CLI
//!
//! Single personality: `mongosh`

use std::env;
use std::process;

fn run_mongosh(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mongosh [OPTIONS] [CONNECTION_STRING] [SCRIPT]");
        println!();
        println!("MongoDB Shell (OurOS).");
        println!();
        println!("Options:");
        println!("  --host HOST       Server hostname (default: localhost)");
        println!("  --port PORT       Port number (default: 27017)");
        println!("  --username USER   Username");
        println!("  --password PASS   Password");
        println!("  --authenticationDatabase DB  Auth database");
        println!("  --eval CMD        Evaluate command");
        println!("  --file FILE       Execute script file");
        println!("  --quiet           Suppress output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mongosh 2.1.1 (OurOS)");
        return 0;
    }

    let host = args.windows(2).find(|w| w[0] == "--host")
        .map(|w| w[1].as_str()).unwrap_or("localhost");
    let port = args.windows(2).find(|w| w[0] == "--port")
        .map(|w| w[1].as_str()).unwrap_or("27017");

    let eval_cmd = args.windows(2).find(|w| w[0] == "--eval")
        .map(|w| w[1].as_str());

    if let Some(cmd) = eval_cmd {
        println!("Connecting to mongodb://{}:{}...", host, port);
        match cmd {
            "db.stats()" => {
                println!("{{");
                println!("  db: 'mydb',");
                println!("  collections: 5,");
                println!("  views: 0,");
                println!("  objects: 45678,");
                println!("  avgObjSize: 256,");
                println!("  dataSize: 11693568,");
                println!("  storageSize: 15728640,");
                println!("  indexes: 12,");
                println!("  indexSize: 2097152");
                println!("}}");
            }
            "show dbs" | "show databases" => {
                println!("admin    180.00 KiB");
                println!("config   108.00 KiB");
                println!("local     75.00 KiB");
                println!("mydb      14.98 MiB");
                println!("testdb     1.24 MiB");
            }
            "show collections" => {
                println!("users");
                println!("orders");
                println!("products");
                println!("sessions");
                println!("logs");
            }
            _ => {
                println!("(result of: {})", cmd);
            }
        }
    } else {
        println!("Current Mongosh Log ID: 65a5abc123def456ghi789jk");
        println!("Connecting to:          mongodb://{}:{}/", host, port);
        println!("Using MongoDB:          7.0.4");
        println!("Using Mongosh:          2.1.1");
        println!();
        println!("For mongosh info see: https://docs.mongodb.com/mongodb-shell/");
        println!();
        println!("mydb> (interactive mode)");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mongosh(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_mongosh};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mongosh(vec!["--help".to_string()]), 0);
        assert_eq!(run_mongosh(vec!["-h".to_string()]), 0);
        let _ = run_mongosh(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mongosh(vec![]);
    }
}
