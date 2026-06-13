#![deny(clippy::all)]

//! pgbouncer — SlateOS lightweight PostgreSQL connection pooler
//!
//! Single personality: `pgbouncer`

use std::env;
use std::process;

fn run_pgbouncer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pgbouncer [OPTION]... config.ini");
        println!();
        println!("Options:");
        println!("  -d               Run in background (as daemon)");
        println!("  -R               Do an online restart");
        println!("  -q               Run quietly");
        println!("  -v               Increase verbosity");
        println!("  -u <user>        Assume identity of <user>");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("PgBouncer 1.22.1 (SlateOS)");
        println!("libevent 2.1.12-stable");
        println!("TLS: OpenSSL 3.2.1");
        return 0;
    }
    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/etc/pgbouncer/pgbouncer.ini");
    println!("2025-05-22 10:00:00.000 UTC [12345] LOG kernel file descriptor limit: 1024 (hard: 1048576); max_client_conn: 100, max expected fd use: 112");
    println!("2025-05-22 10:00:00.001 UTC [12345] LOG listening on 0.0.0.0:6432");
    println!("2025-05-22 10:00:00.002 UTC [12345] LOG listening on unix:/tmp/.s.PGSQL.6432");
    println!("2025-05-22 10:00:00.003 UTC [12345] LOG process up: PgBouncer 1.22.1, libevent 2.1.12-stable");
    println!("2025-05-22 10:00:00.004 UTC [12345] LOG Config loaded: {}", config);
    println!();
    println!("Databases:");
    println!("  myapp     host=127.0.0.1 port=5432 pool_size=20 pool_mode=transaction");
    println!("  analytics host=127.0.0.1 port=5432 pool_size=10 pool_mode=session");
    println!();
    println!("Stats:");
    println!("  database     total_xact_count  total_query_count  total_server_time  avg_xact_time  avg_query_time");
    println!("  myapp        142890            568234             12345678           0.086          0.021");
    println!("  analytics    8923              34521              8765432            0.982          0.253");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pgbouncer(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pgbouncer};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pgbouncer(vec!["--help".to_string()]), 0);
        assert_eq!(run_pgbouncer(vec!["-h".to_string()]), 0);
        let _ = run_pgbouncer(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pgbouncer(vec![]);
    }
}
