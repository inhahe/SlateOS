#![deny(clippy::all)]

//! sqlite3 — SlateOS SQLite command-line shell
//!
//! Single personality: `sqlite3`

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct _TableInfo {
    name: String,
    columns: Vec<ColumnInfo>,
    _row_count: u64,
}

#[derive(Clone, Debug)]
struct ColumnInfo {
    name: String,
    col_type: String,
    _notnull: bool,
    _pk: bool,
}

fn sample_tables() -> Vec<_TableInfo> {
    vec![
        _TableInfo {
            name: "users".to_string(),
            columns: vec![
                ColumnInfo { name: "id".to_string(), col_type: "INTEGER".to_string(), _notnull: true, _pk: true },
                ColumnInfo { name: "name".to_string(), col_type: "TEXT".to_string(), _notnull: true, _pk: false },
                ColumnInfo { name: "email".to_string(), col_type: "TEXT".to_string(), _notnull: false, _pk: false },
                ColumnInfo { name: "created_at".to_string(), col_type: "DATETIME".to_string(), _notnull: false, _pk: false },
            ],
            _row_count: 42,
        },
        _TableInfo {
            name: "posts".to_string(),
            columns: vec![
                ColumnInfo { name: "id".to_string(), col_type: "INTEGER".to_string(), _notnull: true, _pk: true },
                ColumnInfo { name: "user_id".to_string(), col_type: "INTEGER".to_string(), _notnull: true, _pk: false },
                ColumnInfo { name: "title".to_string(), col_type: "TEXT".to_string(), _notnull: true, _pk: false },
                ColumnInfo { name: "body".to_string(), col_type: "TEXT".to_string(), _notnull: false, _pk: false },
            ],
            _row_count: 156,
        },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_sqlite3(args: Vec<String>) -> i32 {
    let mut db_path: Option<String> = None;
    let mut commands: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-help" | "-h" => {
                println!("Usage: sqlite3 [OPTIONS] [FILENAME [SQL]]");
                println!();
                println!("SQLite command-line shell.");
                println!();
                println!("Options:");
                println!("  -batch          Force batch mode");
                println!("  -init FILE      Read/process FILE");
                println!("  -header         Turn headers on");
                println!("  -noheader       Turn headers off");
                println!("  -separator SEP  Set column separator [default: |]");
                println!("  -csv            Set output mode to CSV");
                println!("  -json           Set output mode to JSON");
                println!("  -column         Set output mode to columns");
                println!("  -line           Set output mode to line");
                println!("  -cmd COMMAND    Run COMMAND before reading stdin");
                println!("  -version        Show version");
                return 0;
            }
            "-version" | "--version" => {
                println!("3.45.0 2025-05-22 (SlateOS)");
                return 0;
            }
            "-cmd" => {
                i += 1;
                if i < args.len() { commands.push(args[i].clone()); }
            }
            s if !s.starts_with('-') && db_path.is_none() => {
                db_path = Some(s.to_string());
            }
            s if !s.starts_with('-') => {
                commands.push(s.to_string());
            }
            _ => {} // ignore other flags in simulated mode
        }
        i += 1;
    }

    let db = db_path.as_deref().unwrap_or(":memory:");

    if commands.is_empty() {
        // Interactive mode
        println!("SQLite version 3.45.0 (SlateOS)");
        println!("Enter \".help\" for usage hints.");
        if db == ":memory:" {
            println!("Connected to a transient in-memory database.");
        } else {
            println!("Connected to database: {}", db);
        }
        println!("sqlite> .tables");
        run_dot_tables();
        println!("sqlite> SELECT * FROM users LIMIT 3;");
        run_sample_select();
        println!("sqlite> .quit");
        return 0;
    }

    // Batch mode — execute commands
    for cmd in &commands {
        let lower = cmd.trim().to_ascii_lowercase();
        if lower.starts_with(".tables") {
            run_dot_tables();
        } else if lower.starts_with(".schema") {
            run_dot_schema();
        } else if lower.starts_with(".databases") {
            println!("main: {} r/w", db);
        } else if lower.starts_with(".dump") {
            run_dot_dump();
        } else if lower.contains("select") {
            run_sample_select();
        } else {
            println!("(executed: {} — simulated)", cmd);
        }
    }
    0
}

fn run_dot_tables() {
    let tables = sample_tables();
    for t in &tables {
        print!("{}  ", t.name);
    }
    println!();
}

fn run_dot_schema() {
    let tables = sample_tables();
    for t in &tables {
        print!("CREATE TABLE {} (", t.name);
        for (i, c) in t.columns.iter().enumerate() {
            if i > 0 { print!(", "); }
            print!("{} {}", c.name, c.col_type);
            if c._pk { print!(" PRIMARY KEY"); }
            if c._notnull && !c._pk { print!(" NOT NULL"); }
        }
        println!(");");
    }
}

fn run_sample_select() {
    println!("1|Alice|alice@example.com|2025-01-15");
    println!("2|Bob|bob@example.com|2025-02-20");
    println!("3|Charlie|charlie@example.com|2025-03-10");
}

fn run_dot_dump() {
    println!("BEGIN TRANSACTION;");
    run_dot_schema();
    println!("INSERT INTO users VALUES(1,'Alice','alice@example.com','2025-01-15');");
    println!("INSERT INTO users VALUES(2,'Bob','bob@example.com','2025-02-20');");
    println!("COMMIT;");
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqlite3(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_tables() {
        let tables = sample_tables();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].name, "users");
        assert_eq!(tables[0].columns.len(), 4);
    }

    #[test]
    fn test_pk_column() {
        let tables = sample_tables();
        assert!(tables[0].columns[0]._pk);
        assert!(!tables[0].columns[1]._pk);
    }
}
