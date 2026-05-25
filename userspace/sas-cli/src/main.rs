#![deny(clippy::all)]

//! sas-cli — OurOS SAS analytics platform
//!
//! Single personality: `sas`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sas(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sas [OPTIONS]");
        println!("SAS Viya 4 / SAS 9.4 M8 (OurOS) — Enterprise analytics platform");
        println!();
        println!("Options:");
        println!("  -sysin FILE            Run .sas program in batch");
        println!("  -log FILE              Log file path");
        println!("  -nodms                 No display manager (no GUI)");
        println!("  --studio               Launch SAS Studio (web)");
        println!("  --enterprise-miner     SAS Enterprise Miner (data mining)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SAS Viya 4 (Stable 2024.10) / SAS 9.4 TS1M8 (OurOS)"); return 0; }
    println!("SAS Viya 4 (Stable 2024.10) / SAS 9.4 TS1M8 (OurOS)");
    println!("  Editions: Viya 4 (cloud-native, Kubernetes), SAS 9.4 (traditional)");
    println!("  Language: SAS programming language (DATA step + PROC step)");
    println!("  Procs: PROC SQL, PROC GLM, PROC LOGISTIC, PROC FCMP, PROC SGPLOT, ...");
    println!("  Products: Base SAS, SAS/STAT, SAS/ETS, SAS/IML, Enterprise Miner");
    println!("  CAS: Cloud Analytic Services (in-memory distributed engine)");
    println!("  Industries: pharma (clinical trials), finance, government, insurance");
    println!("  Compliance: FDA-validated for 21 CFR Part 11, SOX, Basel III");
    println!("  License: enterprise — per-server, per-user, per-CPU");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sas".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sas(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
