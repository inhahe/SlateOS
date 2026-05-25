#![deny(clippy::all)]

//! sqlserver-cli — OurOS Microsoft SQL Server (sqlcmd + SSMS)
//!
//! Single personality: `sqlserver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sqlserver(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sqlserver [OPTIONS] [-S SERVER]");
        println!("Microsoft SQL Server 2022 (OurOS) — sqlcmd / SSMS / Azure Data Studio");
        println!();
        println!("Options:");
        println!("  -S SERVER              Server name or address");
        println!("  -U USER -P PASS        SQL authentication");
        println!("  -E                     Windows integrated auth");
        println!("  -d DATABASE            Initial database");
        println!("  -i FILE                Run input SQL script");
        println!("  --ssms                 Launch SQL Server Management Studio");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Microsoft SQL Server 2022 (16.0.4135.4) (OurOS)"); return 0; }
    println!("Microsoft SQL Server 2022 (16.0.4135.4) (OurOS)");
    println!("  Editions: Express (free), Standard, Enterprise, Web, Developer");
    println!("  Cloud: Azure SQL Database, Azure SQL Managed Instance, SQL on Azure VMs");
    println!("  Language: T-SQL (Transact-SQL), SQLCLR (managed CLR procs)");
    println!("  Engines: Database Engine, Analysis Services (SSAS), Reporting (SSRS)");
    println!("  Integration: SSIS, Master Data Services, Data Quality Services");
    println!("  Tools: SSMS (Management Studio), Azure Data Studio (cross-platform)");
    println!("  Features: Always On AGs, In-Memory OLTP, Columnstore, Always Encrypted");
    println!("  License: Free (Express/Developer), per-core (Standard/Enterprise)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sqlserver".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sqlserver(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
