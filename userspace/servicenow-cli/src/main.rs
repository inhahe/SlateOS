#![deny(clippy::all)]

//! servicenow-cli — OurOS ServiceNow Now Platform
//!
//! Single personality: `servicenow`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_snow(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: servicenow [OPTIONS]");
        println!("ServiceNow Now Platform — Xanadu (OurOS) — IT/HR/CS workflow platform");
        println!();
        println!("Options:");
        println!("  --instance NAME        Instance name (e.g. mycompany)");
        println!("  --app APP              ITSM/ITOM/CSM/HRSD/SecOps/GRC/SPM/CMDB");
        println!("  --sdk                  ServiceNow SDK / CLI for app development");
        println!("  --studio               Now Studio web IDE");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ServiceNow Now Platform Xanadu (Q3 2024) (OurOS)"); return 0; }
    println!("ServiceNow Now Platform Xanadu (Q3 2024) (OurOS)");
    println!("  Releases: half-yearly families (Vancouver/Washington DC/Xanadu/Yokohama)");
    println!("  Workflow apps: ITSM, ITOM, ITBM, CSM, HRSD, SecOps, GRC, IRM, FSM, App Engine");
    println!("  Now Assist: GenAI assistants for ITSM/CSM/HRSD/Creator");
    println!("  Architecture: multi-instance SaaS — dedicated DB per customer");
    println!("  Language: JavaScript (server + client), GlideRecord ORM, Jelly templates");
    println!("  Mid-Server: on-premise integration agent (Java)");
    println!("  Studio: web-based scoped app development (now Workspaces)");
    println!("  Store: ServiceNow Store marketplace (certified + 3rd party apps)");
    println!("  License: per-user (fulfiller/requester) + product-specific");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "servicenow".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_snow(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_snow};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/servicenow"), "servicenow");
        assert_eq!(basename(r"C:\bin\servicenow.exe"), "servicenow.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("servicenow.exe"), "servicenow");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_snow(&["--help".to_string()], "servicenow"), 0);
        assert_eq!(run_snow(&["-h".to_string()], "servicenow"), 0);
        let _ = run_snow(&["--version".to_string()], "servicenow");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_snow(&[], "servicenow");
    }
}
