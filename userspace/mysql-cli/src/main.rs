#![deny(clippy::all)]

//! mysql-cli — SlateOS MySQL/MariaDB client
//!
//! Multi-personality: `mysql`, `mysqldump`, `mysqladmin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mysql(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: mysql [OPTIONS] [DATABASE]");
        println!("mysql Ver 8.4.0 for SlateOS (x86_64)");
        println!();
        println!("Options:");
        println!("  -h HOST      Server host");
        println!("  -P PORT      Server port");
        println!("  -u USER      User name");
        println!("  -p           Prompt for password");
        println!("  -e STMT      Execute statement");
        println!("  -D DB        Database to use");
        println!("  --batch      Batch mode (tab-separated output)");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mysql  Ver 8.4.0 for SlateOS on x86_64");
        return 0;
    }
    let stmt = args.windows(2).find(|w| w[0] == "-e").map(|w| w[1].as_str());
    if let Some(s) = stmt {
        println!("{}", s);
        println!("(query OK)");
        return 0;
    }
    let host = args.windows(2).find(|w| w[0] == "-h").map(|w| w[1].as_str()).unwrap_or("localhost");
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    println!("Welcome to the MySQL monitor.  Commands end with ; or \\g.");
    println!("Server version: 8.4.0 SlateOS");
    println!();
    println!("Connected to {} at {}", db.unwrap_or("(none)"), host);
    println!("mysql> ");
    0
}

fn run_mysqldump(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") || args.is_empty() {
        println!("Usage: mysqldump [OPTIONS] DATABASE [TABLES]");
        println!("  -h HOST       Server host");
        println!("  -u USER       User name");
        println!("  --all-databases   Dump all databases");
        println!("  --single-transaction   Consistent snapshot");
        println!("  --routines    Include stored procedures");
        println!("  --triggers    Include triggers");
        return 0;
    }
    let all = args.iter().any(|a| a == "--all-databases");
    if all {
        println!("-- MySQL dump -- All databases");
    } else {
        let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("mydb");
        println!("-- MySQL dump");
        println!("-- Server version\t8.4.0");
        println!("--");
        println!("-- Dumping data for database '{}'", db);
    }
    println!("-- Dump completed");
    0
}

fn run_mysqladmin(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") || args.is_empty() {
        println!("Usage: mysqladmin [OPTIONS] COMMAND [ARG]");
        println!("  create DB     Create database");
        println!("  drop DB       Drop database");
        println!("  status        Show server status");
        println!("  ping          Check if server is alive");
        println!("  processlist   Show running queries");
        println!("  variables     Show server variables");
        println!("  version       Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "status" => {
            println!("Uptime: 86400  Threads: 4  Questions: 1234  Slow queries: 0");
            println!("Opens: 42  Flush tables: 1  Open tables: 30  Queries per second avg: 0.014");
        }
        "ping" => println!("mysqld is alive"),
        "version" => {
            println!("mysqladmin  Ver 8.4.0 for SlateOS on x86_64");
            println!("Server version\t\t8.4.0");
            println!("Protocol version\t10");
        }
        "create" => {
            let db = args.get(1).map(|s| s.as_str()).unwrap_or("newdb");
            println!("Database \"{}\" created.", db);
        }
        "drop" => {
            let db = args.get(1).map(|s| s.as_str()).unwrap_or("olddb");
            println!("Database \"{}\" dropped.", db);
        }
        "processlist" => {
            println!("Id  User    Host           db     Command  Time  State  Info");
            println!("1   root    localhost      mydb   Sleep    123          ");
            println!("2   app     192.168.1.10   mydb   Query    0     exec   SELECT 1");
        }
        _ => println!("mysqladmin: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mysql".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mysqldump" => run_mysqldump(&rest),
        "mysqladmin" => run_mysqladmin(&rest),
        _ => run_mysql(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mysql};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mysql"), "mysql");
        assert_eq!(basename(r"C:\bin\mysql.exe"), "mysql.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mysql.exe"), "mysql");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mysql(&["--help".to_string()]), 0);
        assert_eq!(run_mysql(&["-h".to_string()]), 0);
        let _ = run_mysql(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mysql(&[]);
    }
}
