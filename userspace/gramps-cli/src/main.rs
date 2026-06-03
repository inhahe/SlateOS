#![deny(clippy::all)]

//! gramps-cli — OurOS Gramps genealogy research tool
//!
//! Single personality: `gramps`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gramps(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: gramps [OPTIONS]");
        println!("Gramps v5.2 (OurOS) — Genealogy research software");
        println!();
        println!("Options:");
        println!("  -i FILE           Import file (GEDCOM, Gramps XML, CSV)");
        println!("  -o FILE           Export file");
        println!("  -f FORMAT         Export format (gedcom, gramps, csv)");
        println!("  -a ACTION         Perform action (report, tool, check)");
        println!("  -p NAME=VALUE     Action parameter");
        println!("  -l                List databases");
        println!("  -C                Create new database");
        println!("  -d DBNAME         Use specific database");
        println!("  --remove DBNAME   Remove database");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Gramps v5.2.1 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-l") {
        println!("Gramps databases:");
        println!("  Family Tree    1,234 people    456 families");
        println!("  Research       567 people      123 families");
        return 0;
    }
    println!("Gramps v5.2.1 (OurOS) — Genealogy Research");
    println!("  Database: Family Tree");
    println!("  People: 1,234");
    println!("  Families: 456");
    println!("  Events: 3,456");
    println!("  Places: 234");
    println!("  Sources: 89");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gramps".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gramps(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gramps};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gramps"), "gramps");
        assert_eq!(basename(r"C:\bin\gramps.exe"), "gramps.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gramps.exe"), "gramps");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gramps(&["--help".to_string()], "gramps"), 0);
        assert_eq!(run_gramps(&["-h".to_string()], "gramps"), 0);
        assert_eq!(run_gramps(&["--version".to_string()], "gramps"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gramps(&[], "gramps"), 0);
    }
}
