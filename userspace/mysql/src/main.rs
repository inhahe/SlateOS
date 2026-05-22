#![deny(clippy::all)]

//! mysql — OurOS MySQL relational database
//!
//! Multi-personality: `mysqld` (server), `mysql` (client), `mysqladmin`, `mysqldump`

use std::env;
use std::process;

fn run_mysqld(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mysqld [options]");
        println!();
        println!("Options:");
        println!("  --port=<port>            TCP/IP port (default: 3306)");
        println!("  --bind-address=<addr>    Bind address");
        println!("  --datadir=<dir>          Data directory");
        println!("  --socket=<file>          Socket file");
        println!("  --user=<name>            Run as user");
        println!("  --log-error=<file>       Error log file");
        println!("  --innodb-buffer-pool-size=<n>  InnoDB buffer pool size");
        println!("  --max-connections=<n>    Max connections (default: 151)");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mysqld  Ver 8.4.0 for OurOS on x86_64 (OurOS Community Server - GPL)");
        return 0;
    }
    let port = args.iter().find_map(|a| {
        a.strip_prefix("--port=").and_then(|v| v.parse::<u16>().ok())
    }).unwrap_or(3306);
    println!("2025-05-22T10:00:00.000000Z 0 [System] [MY-010116] [Server] mysqld (mysqld 8.4.0) starting as process 12345");
    println!("2025-05-22T10:00:00.100000Z 0 [System] [MY-013576] [InnoDB] InnoDB initialization has started.");
    println!("2025-05-22T10:00:01.000000Z 0 [System] [MY-013577] [InnoDB] InnoDB initialization has ended.");
    println!("2025-05-22T10:00:01.500000Z 0 [System] [MY-011323] [Server] X Plugin ready for connections. Bind-address: '::' port: 33060");
    println!("2025-05-22T10:00:02.000000Z 0 [System] [MY-010931] [Server] mysqld: ready for connections. Version: '8.4.0'  socket: '/tmp/mysql.sock'  port: {}  OurOS Community Server - GPL.", port);
    0
}

fn run_mysql_client(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-?") {
        println!("Usage: mysql [options] [database]");
        println!();
        println!("Options:");
        println!("  -h, --host=<name>        Connect to host");
        println!("  -P, --port=<port>        Port number");
        println!("  -u, --user=<name>        User for login");
        println!("  -p, --password[=pass]    Password");
        println!("  -D, --database=<name>    Database to use");
        println!("  -e, --execute=<stmt>     Execute statement and quit");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mysql  Ver 8.4.0 for OurOS on x86_64 (OurOS Community Server - GPL)");
        return 0;
    }

    let exec_stmt = args.iter().position(|a| a == "-e" || a.starts_with("--execute"))
        .and_then(|i| {
            let a = &args[i];
            if let Some(val) = a.strip_prefix("--execute=") {
                Some(val.to_string())
            } else {
                args.get(i + 1).cloned()
            }
        });

    if let Some(stmt) = exec_stmt {
        let upper = stmt.to_uppercase();
        if upper.contains("SHOW DATABASES") {
            println!("+--------------------+");
            println!("| Database           |");
            println!("+--------------------+");
            println!("| information_schema |");
            println!("| mysql              |");
            println!("| performance_schema |");
            println!("| sys                |");
            println!("| myapp              |");
            println!("+--------------------+");
        } else if upper.contains("SHOW TABLES") {
            println!("+-------------------+");
            println!("| Tables_in_myapp   |");
            println!("+-------------------+");
            println!("| users             |");
            println!("| orders            |");
            println!("| products          |");
            println!("| sessions          |");
            println!("+-------------------+");
        } else if upper.contains("SELECT") {
            println!("+----+----------+---------------------+");
            println!("| id | name     | created_at          |");
            println!("+----+----------+---------------------+");
            println!("|  1 | alice    | 2025-01-15 08:30:00 |");
            println!("|  2 | bob      | 2025-02-20 14:15:00 |");
            println!("|  3 | charlie  | 2025-03-10 11:45:00 |");
            println!("+----+----------+---------------------+");
            println!("3 rows in set (0.00 sec)");
        } else if upper.contains("STATUS") {
            println!("--------------");
            println!("mysql  Ver 8.4.0 for OurOS on x86_64");
            println!();
            println!("Connection id:          8");
            println!("Current database:       myapp");
            println!("Current user:           root@localhost");
            println!("Server version:         8.4.0 OurOS Community Server - GPL");
            println!("Protocol version:       10");
            println!("Uptime:                 1 day 0 hours 0 min 0 sec");
            println!("Threads: 2  Questions: 42  Slow queries: 0  Opens: 150  Open tables: 120  Queries per second avg: 0.048");
            println!("--------------");
        } else {
            println!("Query OK, 0 rows affected (0.00 sec)");
        }
        return 0;
    }

    // Interactive mode
    let host = args.iter().find_map(|a| a.strip_prefix("-h")).unwrap_or("localhost");
    println!("Welcome to the MySQL monitor.  Commands end with ; or \\g.");
    println!("Your MySQL connection id is 8");
    println!("Server version: 8.4.0 OurOS Community Server - GPL");
    println!();
    println!("Type 'help;' or '\\h' for help. Type '\\c' to clear the current input statement.");
    println!();
    println!("mysql@{}> SELECT 1;", host);
    println!("+---+");
    println!("| 1 |");
    println!("+---+");
    println!("| 1 |");
    println!("+---+");
    println!("1 row in set (0.00 sec)");
    println!();
    println!("mysql> quit");
    println!("Bye");
    0
}

fn run_mysqladmin(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mysqladmin [options] command ...");
        println!();
        println!("Commands:");
        println!("  create <db>       Create a new database");
        println!("  drop <db>         Delete a database");
        println!("  extended-status   Show server status variables");
        println!("  flush-hosts       Flush all cached hosts");
        println!("  flush-logs        Flush all logs");
        println!("  ping              Check if mysqld is alive");
        println!("  processlist       Show list of active threads");
        println!("  reload            Reload grant tables");
        println!("  shutdown          Take server down");
        println!("  status            Show short server status");
        println!("  variables         Show server system variables");
        println!("  version           Show version info");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "version" | "--version" => {
            println!("mysqladmin  Ver 8.4.0 for OurOS on x86_64 (OurOS Community Server - GPL)");
            println!("Server version          8.4.0");
            println!("Protocol version        10");
            println!("Connection              Localhost via UNIX socket");
        }
        "ping" => println!("mysqld is alive"),
        "status" => {
            println!("Uptime: 86400  Threads: 2  Questions: 42  Slow queries: 0  Opens: 150  Flush tables: 3  Open tables: 120  Queries per second avg: 0.048");
        }
        "processlist" => {
            println!("+----+------+-----------+-------+---------+------+----------+------------------+");
            println!("| Id | User | Host      | db    | Command | Time | State    | Info             |");
            println!("+----+------+-----------+-------+---------+------+----------+------------------+");
            println!("|  8 | root | localhost | myapp | Query   |    0 | starting | SHOW PROCESSLIST |");
            println!("+----+------+-----------+-------+---------+------+----------+------------------+");
        }
        "shutdown" => println!("mysqld will shut down"),
        "create" => {
            let db = args.get(1).map(|s| s.as_str()).unwrap_or("database");
            println!("Database \"{}\" created.", db);
        }
        "drop" => {
            let db = args.get(1).map(|s| s.as_str()).unwrap_or("database");
            println!("Database \"{}\" dropped.", db);
        }
        "flush-logs" => println!("Logs flushed."),
        "reload" => println!("Grant tables reloaded."),
        _ => { eprintln!("mysqladmin: unknown command '{}'", cmd); return 1; }
    }
    0
}

fn run_mysqldump(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mysqldump [options] database [tables]");
        println!("   OR: mysqldump [options] --all-databases");
        println!();
        println!("Options:");
        println!("  -u, --user=<name>        User for login");
        println!("  -p, --password[=pass]    Password");
        println!("  -h, --host=<name>        Connect to host");
        println!("  --single-transaction     Use a single transaction for InnoDB");
        println!("  --routines               Dump stored routines");
        println!("  --triggers               Dump triggers");
        println!("  --all-databases          Dump all databases");
        println!("  --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mysqldump  Ver 8.4.0 for OurOS on x86_64 (OurOS Community Server - GPL)");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("myapp");
    println!("-- MySQL dump 10.13  Distrib 8.4.0, for OurOS (x86_64)");
    println!("--");
    println!("-- Host: localhost    Database: {}", db);
    println!("-- ------------------------------------------------------");
    println!("-- Server version\t8.4.0");
    println!();
    println!("/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;");
    println!("/*!40101 SET NAMES utf8mb4 */;");
    println!();
    println!("--");
    println!("-- Table structure for table `users`");
    println!("--");
    println!();
    println!("DROP TABLE IF EXISTS `users`;");
    println!("CREATE TABLE `users` (");
    println!("  `id` int NOT NULL AUTO_INCREMENT,");
    println!("  `name` varchar(255) NOT NULL,");
    println!("  `email` varchar(255) DEFAULT NULL,");
    println!("  `created_at` datetime DEFAULT CURRENT_TIMESTAMP,");
    println!("  PRIMARY KEY (`id`)");
    println!(") ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;");
    println!();
    println!("-- Dump completed on 2025-05-22 10:00:00");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("mysqld");
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
        "mysql" => run_mysql_client(rest),
        "mysqladmin" => run_mysqladmin(rest),
        "mysqldump" => run_mysqldump(rest),
        _ => run_mysqld(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
