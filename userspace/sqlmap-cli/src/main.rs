#![deny(clippy::all)]

//! sqlmap-cli — Slate OS sqlmap CLI
//!
//! Single personality: `sqlmap`

use std::env;
use std::process;

fn run_sqlmap(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-hh") {
        println!("Usage: sqlmap [OPTIONS]");
        println!();
        println!("sqlmap — SQL injection detection and exploitation (Slate OS).");
        println!();
        println!("Target:");
        println!("  -u URL, --url=URL      Target URL");
        println!("  -r FILE                Load HTTP request from file");
        println!("  -g DORK                Google dork");
        println!();
        println!("Request:");
        println!("  --data=DATA            POST data");
        println!("  --cookie=COOKIE        Cookie header");
        println!("  --random-agent         Random User-Agent");
        println!("  --proxy=PROXY          Use proxy");
        println!("  --tor                  Use Tor");
        println!();
        println!("Injection:");
        println!("  -p PARAM               Testable parameter");
        println!("  --dbms=DBMS            Target DBMS");
        println!("  --technique=TECH       Injection techniques (BEUSTQ)");
        println!("  --level=LEVEL          Level (1-5)");
        println!("  --risk=RISK            Risk (1-3)");
        println!();
        println!("Enumeration:");
        println!("  --dbs                  List databases");
        println!("  --tables               List tables");
        println!("  --columns              List columns");
        println!("  --dump                 Dump table data");
        println!("  --dump-all             Dump everything");
        println!("  -D DB                  Target database");
        println!("  -T TABLE               Target table");
        println!("  --batch                Non-interactive mode");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sqlmap/1.8#stable (Slate OS)");
        return 0;
    }

    let url = args.windows(2).find(|w| w[0] == "-u" || w[0] == "--url")
        .map(|w| w[1].as_str()).unwrap_or("http://example.com/page?id=1");
    let dbs = args.iter().any(|a| a == "--dbs");
    let tables = args.iter().any(|a| a == "--tables");
    let dump = args.iter().any(|a| a == "--dump");

    println!("        ___");
    println!("       __H__");
    println!(" ___ ___[.]_____ ___ ___  {{1.8#stable}}");
    println!("|_ -| . [.]     | .'| . |");
    println!("|___|_  [']_|_|_|__,|  _|");
    println!("      |_|V...       |_|  https://sqlmap.org (Slate OS)");
    println!();
    println!("[*] starting @ 12:00:00");
    println!();
    println!("[12:00:01] [INFO] testing connection to the target URL");
    println!("[12:00:01] [INFO] checking if the target is protected by some kind of WAF/IPS");
    println!("[12:00:02] [INFO] testing if the target URL content is stable");
    println!("[12:00:02] [INFO] target URL content is stable");
    println!("[12:00:03] [INFO] testing if GET parameter 'id' is dynamic");
    println!("[12:00:03] [INFO] GET parameter 'id' appears to be dynamic");
    println!("[12:00:04] [INFO] heuristic (basic) test shows that GET parameter 'id' might be injectable");
    println!("[12:00:05] [INFO] GET parameter 'id' is 'MySQL >= 5.0 AND error-based' injectable");

    if dbs {
        println!("[12:00:06] [INFO] fetching database names");
        println!("available databases [3]:");
        println!("[*] information_schema");
        println!("[*] mysql");
        println!("[*] webapp_db");
    } else if tables {
        println!("[12:00:06] [INFO] fetching tables");
        println!("Database: webapp_db");
        println!("[3 tables]");
        println!("+---------+");
        println!("| users   |");
        println!("| orders  |");
        println!("| config  |");
        println!("+---------+");
    } else if dump {
        println!("[12:00:06] [INFO] dumping table");
        println!("Database: webapp_db");
        println!("Table: users");
        println!("[3 entries]");
        println!("+----+----------+----------+");
        println!("| id | username | password |");
        println!("+----+----------+----------+");
        println!("| 1  | admin    | ***      |");
        println!("| 2  | user1    | ***      |");
        println!("| 3  | user2    | ***      |");
        println!("+----+----------+----------+");
    }

    println!();
    let _ = url;
    println!("[*] ending @ 12:00:10");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqlmap(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_sqlmap};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sqlmap(vec!["--help".to_string()]), 0);
        assert_eq!(run_sqlmap(vec!["-h".to_string()]), 0);
        let _ = run_sqlmap(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sqlmap(vec![]);
    }
}
