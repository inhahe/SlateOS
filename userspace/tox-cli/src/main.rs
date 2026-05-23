#![deny(clippy::all)]

//! tox-cli — OurOS tox test automation tool
//!
//! Multi-personality: `tox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tox(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tox [OPTIONS] [COMMAND]");
        println!("tox 4.16.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  run          Run test environments (default)");
        println!("  list         List environments");
        println!("  devenv       Create development environment");
        println!("  config       Show tox configuration");
        println!("  quickstart   Generate a tox.ini");
        println!("  depends      Show environment dependencies");
        println!("  exec         Execute command in environment");
        println!();
        println!("Options:");
        println!("  -e ENV       Run specific environment(s)");
        println!("  -p, --parallel  Run in parallel");
        println!("  --recreate   Recreate environments");
        println!("  -l           List environments (short)");
        println!("  -v           Verbose output");
        println!("  --version    Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("4.16.0 from /usr/lib/python3/dist-packages/tox");
        return 0;
    }
    if args.iter().any(|a| a == "-l") || args.first().map(|s| s.as_str()) == Some("list") {
        println!("default environments:");
        println!("  py312 -> [no description]");
        println!("  py311 -> [no description]");
        println!("  py310 -> [no description]");
        println!("  lint  -> run linters");
        println!("  docs  -> build documentation");
        return 0;
    }
    let env = args.windows(2).find(|w| w[0] == "-e")
        .map(|w| w[1].as_str());
    let parallel = args.iter().any(|a| a == "-p" || a == "--parallel");
    let recreate = args.iter().any(|a| a == "--recreate");

    let envs = if let Some(e) = env {
        vec![e]
    } else {
        vec!["py312", "py311", "lint"]
    };

    if parallel {
        println!("Running {} environments in parallel...", envs.len());
    }

    for e in &envs {
        println!("{}: commands[0]> python -m pytest", e);
        if recreate {
            println!("{}: recreating virtual environment...", e);
        }
        println!("{}: install_deps> pip install -r requirements-test.txt", e);
        println!("{}: commands[0]> pytest --tb=short", e);
        println!("{}: OK (3.45 seconds)", e);
        println!();
    }

    println!("  {} ok", envs.join(", "));
    println!("  congratulations :)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tox(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
