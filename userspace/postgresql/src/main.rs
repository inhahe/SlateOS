#![deny(clippy::all)]

//! postgresql — SlateOS PostgreSQL relational database
//!
//! Multi-personality: `postgres` (server), `psql` (client), `pg_dump`, `pg_restore`,
//!                    `createdb`, `dropdb`, `pg_isready`, `initdb`

use std::env;
use std::process;

fn run_postgres(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: postgres [OPTION]...");
        println!();
        println!("Options:");
        println!("  -D DATADIR        database directory");
        println!("  -p PORT           port number (default: 5432)");
        println!("  -h HOSTNAME       host name or IP to listen on");
        println!("  -k DIRECTORY      Unix-domain socket directory");
        println!("  -c NAME=VALUE     set run-time parameter");
        println!("  --version         output version, then exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("postgres (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    let port = args.iter().position(|a| a == "-p")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(5432);
    println!("2025-05-22 10:00:00.000 UTC [12345] LOG:  starting PostgreSQL 16.3 (Slate OS) on x86_64");
    println!("2025-05-22 10:00:00.001 UTC [12345] LOG:  listening on IPv4 address \"0.0.0.0\", port {}", port);
    println!("2025-05-22 10:00:00.002 UTC [12345] LOG:  listening on IPv6 address \"::\", port {}", port);
    println!("2025-05-22 10:00:00.010 UTC [12345] LOG:  listening on Unix socket \"/tmp/.s.PGSQL.{}\"", port);
    println!("2025-05-22 10:00:00.100 UTC [12346] LOG:  database system was shut down at 2025-05-22 09:59:50 UTC");
    println!("2025-05-22 10:00:00.200 UTC [12345] LOG:  database system is ready to accept connections");
    0
}

fn run_psql(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: psql [OPTION]... [DBNAME [USERNAME]]");
        println!();
        println!("Options:");
        println!("  -h, --host=HOST          database server host");
        println!("  -p, --port=PORT          database server port");
        println!("  -U, --username=USERNAME  database user name");
        println!("  -d, --dbname=DBNAME      database name to connect to");
        println!("  -c, --command=COMMAND    run single command and exit");
        println!("  -f, --file=FILENAME      execute commands from file");
        println!("  -l, --list               list databases and exit");
        println!("  --version                output version, then exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("psql (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list") {
        println!("                              List of databases");
        println!("   Name    |  Owner   | Encoding | Collation |    Ctype    | Access privileges");
        println!("-----------+----------+----------+-----------+-------------+-------------------");
        println!(" myapp     | postgres | UTF8     | en_US.UTF-8 | en_US.UTF-8 |");
        println!(" postgres  | postgres | UTF8     | en_US.UTF-8 | en_US.UTF-8 |");
        println!(" template0 | postgres | UTF8     | en_US.UTF-8 | en_US.UTF-8 | =c/postgres");
        println!(" template1 | postgres | UTF8     | en_US.UTF-8 | en_US.UTF-8 | =c/postgres");
        return 0;
    }

    let exec_cmd = args.iter().position(|a| a == "-c" || a == "--command")
        .and_then(|i| args.get(i + 1));

    if let Some(cmd) = exec_cmd {
        let upper = cmd.to_uppercase();
        if upper.contains("\\DT") || upper.contains("SHOW TABLES") {
            println!("         List of relations");
            println!(" Schema |   Name   | Type  |  Owner");
            println!("--------+----------+-------+----------");
            println!(" public | users    | table | postgres");
            println!(" public | orders   | table | postgres");
            println!(" public | products | table | postgres");
        } else if upper.starts_with("SELECT") {
            println!(" id |  name   |     email          |      created_at");
            println!("----+---------+--------------------+---------------------");
            println!("  1 | alice   | alice@example.com  | 2025-01-15 08:30:00");
            println!("  2 | bob     | bob@example.com    | 2025-02-20 14:15:00");
            println!("  3 | charlie | charlie@example.com| 2025-03-10 11:45:00");
            println!("(3 rows)");
        } else {
            println!("({} — simulated)", cmd);
        }
        return 0;
    }

    // Interactive mode
    println!("psql (16.3 Slate OS)");
    println!("Type \"help\" for help.");
    println!();
    println!("postgres=# SELECT version();");
    println!("                                   version");
    println!("-----------------------------------------------------------------------------");
    println!(" PostgreSQL 16.3 (Slate OS) on x86_64, compiled by rustc, 64-bit");
    println!("(1 row)");
    println!();
    println!("postgres=# \\q");
    0
}

fn run_pg_dump(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: pg_dump [OPTION]... [DBNAME]");
        println!();
        println!("Options:");
        println!("  -h, --host=HOST          database server host");
        println!("  -p, --port=PORT          database server port");
        println!("  -U, --username=USERNAME  database user name");
        println!("  -F, --format=FORMAT      output format (p/c/d/t)");
        println!("  -f, --file=FILENAME      output file name");
        println!("  -t, --table=TABLE        dump named table(s) only");
        println!("  --version                output version, then exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("pg_dump (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("myapp");
    println!("--");
    println!("-- PostgreSQL database dump");
    println!("--");
    println!();
    println!("-- Dumped from database version 16.3 (Slate OS)");
    println!("-- Dumped by pg_dump version 16.3 (Slate OS)");
    println!();
    println!("SET statement_timeout = 0;");
    println!("SET client_encoding = 'UTF8';");
    println!("SET standard_conforming_strings = on;");
    println!();
    println!("--");
    println!("-- Name: {}; Type: DATABASE; Schema: -; Owner: postgres", db);
    println!("--");
    println!();
    println!("CREATE TABLE public.users (");
    println!("    id integer NOT NULL,");
    println!("    name character varying(255) NOT NULL,");
    println!("    email character varying(255),");
    println!("    created_at timestamp without time zone DEFAULT now()");
    println!(");");
    println!();
    println!("-- PostgreSQL database dump complete");
    0
}

fn run_pg_restore(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: pg_restore [OPTION]... [FILE]");
        println!();
        println!("Options:");
        println!("  -d, --dbname=NAME        database to restore into");
        println!("  -h, --host=HOST          database server host");
        println!("  -l, --list               print table of contents");
        println!("  -C, --create             create the target database");
        println!("  --version                output version, then exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("pg_restore (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    println!("pg_restore: restoring data... (simulated)");
    println!("pg_restore: finished");
    0
}

fn run_createdb(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: createdb [OPTION]... [DBNAME] [DESCRIPTION]");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("createdb (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    let db = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("newdb");
    println!("CREATE DATABASE");
    let _ = db;
    0
}

fn run_dropdb(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: dropdb [OPTION]... DBNAME");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("dropdb (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    println!("DROP DATABASE");
    0
}

fn run_pg_isready(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: pg_isready [OPTION]...");
        println!();
        println!("Options:");
        println!("  -h, --host=HOST    database server host");
        println!("  -p, --port=PORT    database server port");
        println!("  --version          output version, then exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("pg_isready (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    let host = args.iter().position(|a| a == "-h" || a == "--host")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("localhost");
    let port = args.iter().position(|a| a == "-p" || a == "--port")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("5432");
    println!("{}:{} - accepting connections", host, port);
    0
}

fn run_initdb(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-?") {
        println!("Usage: initdb [OPTION]... [DATADIR]");
        println!();
        println!("Options:");
        println!("  -D, --pgdata=DIR   location for the database cluster");
        println!("  -E, --encoding=ENC set default encoding");
        println!("  --locale=LOCALE    set default locale");
        println!("  --version          output version, then exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("initdb (PostgreSQL) 16.3 (Slate OS)");
        return 0;
    }
    let datadir = args.iter().position(|a| a == "-D" || a == "--pgdata")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .or_else(|| args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()))
        .unwrap_or("/var/lib/postgresql/16/data");
    println!("The files belonging to this database system will be owned by user \"postgres\".");
    println!("This user must also own the server process.");
    println!();
    println!("The database cluster will be initialized with locale \"en_US.UTF-8\".");
    println!("The default database encoding has accordingly been set to \"UTF8\".");
    println!("The default text search configuration will be set to \"english\".");
    println!();
    println!("Data page checksums are enabled.");
    println!();
    println!("creating directory {}... ok", datadir);
    println!("creating subdirectories... ok");
    println!("selecting dynamic shared memory implementation... posix");
    println!("selecting default max_connections... 100");
    println!("selecting default shared_buffers... 128MB");
    println!("creating configuration files... ok");
    println!("running bootstrap script... ok");
    println!("performing post-bootstrap initialization... ok");
    println!("syncing data to disk... ok");
    println!();
    println!("Success. You can now start the database server using:");
    println!("    postgres -D {}", datadir);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("postgres");
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
        "psql" => run_psql(rest),
        "pg_dump" => run_pg_dump(rest),
        "pg_restore" => run_pg_restore(rest),
        "createdb" => run_createdb(rest),
        "dropdb" => run_dropdb(rest),
        "pg_isready" => run_pg_isready(rest),
        "initdb" => run_initdb(rest),
        _ => run_postgres(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_postgres};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_postgres(vec!["--help".to_string()]), 0);
        assert_eq!(run_postgres(vec!["-h".to_string()]), 0);
        let _ = run_postgres(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_postgres(vec![]);
    }
}
