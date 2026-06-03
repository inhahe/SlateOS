#![deny(clippy::all)]

//! qlik-cli — OurOS Qlik Sense / QlikView analytics
//!
//! Single personality: `qlik`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qlik(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qlik [OPTIONS] [APP]");
        println!("Qlik Sense November 2024 / QlikView 12.90 (OurOS) — Associative analytics");
        println!();
        println!("Options:");
        println!("  --app FILE             Open .qvf (Sense) or .qvw (QlikView)");
        println!("  --cloud TENANT         Qlik Cloud Analytics tenant");
        println!("  --talend               Talend Data Integration (acquired)");
        println!("  --automl               Qlik AutoML predictive analytics");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Qlik Sense Enterprise November 2024 (OurOS)"); return 0; }
    println!("Qlik Sense Enterprise November 2024 (OurOS)");
    println!("  Products: Sense (modern), QlikView (classic), Cloud, Talend, Stitch");
    println!("  Engine: Associative in-memory engine — explores all data relationships");
    println!("  Storage: QVD (Qlik Data) columnar files for fast extraction");
    println!("  Scripting: Qlik script (load script), set analysis, AAI integration");
    println!("  Format: .qvf (Sense app), .qvw (QlikView document), .qvd (data)");
    println!("  AI: Insight Advisor (NLP), AutoML, predictive analytics");
    println!("  Data integration: Talend (acquired 2023), real-time CDC via Replicate");
    println!("  License: Analyzer/Professional users, capacity-based cloud SKUs");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qlik".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qlik(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qlik};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qlik"), "qlik");
        assert_eq!(basename(r"C:\bin\qlik.exe"), "qlik.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qlik.exe"), "qlik");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_qlik(&["--help".to_string()], "qlik"), 0);
        assert_eq!(run_qlik(&["-h".to_string()], "qlik"), 0);
        assert_eq!(run_qlik(&["--version".to_string()], "qlik"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_qlik(&[], "qlik"), 0);
    }
}
