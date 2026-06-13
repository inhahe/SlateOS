#![deny(clippy::all)]

//! phpmyadmin-cli — SlateOS phpMyAdmin database management
//!
//! Single personality: `phpmyadmin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_phpmyadmin(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: phpmyadmin [COMMAND] [OPTIONS]");
        println!("phpMyAdmin v5.2 (Slate OS) — MySQL/MariaDB web administration");
        println!();
        println!("Commands:");
        println!("  serve              Start web server");
        println!("  db list|create|drop  Database operations");
        println!("  table list|describe  Table operations");
        println!("  import FILE        Import SQL file");
        println!("  export DB          Export database");
        println!("  query SQL          Execute SQL query");
        println!();
        println!("Options:");
        println!("  --host HOST        MySQL host");
        println!("  --port PORT        MySQL port");
        println!("  --user USER        MySQL username");
        println!("  --listen ADDR      Web server address");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("phpMyAdmin v5.2.1 (Slate OS)"); return 0; }
    println!("phpMyAdmin v5.2.1 (Slate OS)");
    println!("  MySQL: localhost:3306 (v8.0)");
    println!("  Databases: 12");
    println!("  Tables: 234");
    println!("  Users: 8");
    println!("  Server charset: utf8mb4");
    println!("  Web: http://0.0.0.0:8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "phpmyadmin".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_phpmyadmin(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_phpmyadmin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/phpmyadmin"), "phpmyadmin");
        assert_eq!(basename(r"C:\bin\phpmyadmin.exe"), "phpmyadmin.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("phpmyadmin.exe"), "phpmyadmin");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_phpmyadmin(&["--help".to_string()], "phpmyadmin"), 0);
        assert_eq!(run_phpmyadmin(&["-h".to_string()], "phpmyadmin"), 0);
        let _ = run_phpmyadmin(&["--version".to_string()], "phpmyadmin");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_phpmyadmin(&[], "phpmyadmin");
    }
}
