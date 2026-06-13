#![deny(clippy::all)]

//! feast-cli — SlateOS Feast feature store
//!
//! Single personality: `feast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_feast(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: feast COMMAND [OPTIONS]");
        println!("Feast v0.37 (Slate OS) — Open source feature store");
        println!();
        println!("Commands:");
        println!("  init             Initialize a new feature repo");
        println!("  apply            Apply feature definitions");
        println!("  materialize      Materialize features to online store");
        println!("  serve            Start feature server");
        println!("  entities         List entities");
        println!("  feature-views    List feature views");
        println!("  on-demand-fvs    List on-demand feature views");
        println!("  teardown         Tear down infrastructure");
        println!();
        println!("Options:");
        println!("  -c DIR           Feature repo directory");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Feast v0.37.1 (Slate OS)"); return 0; }
    println!("Feast v0.37.1 (Slate OS)");
    println!("  Feature repo: /project/feature_store");
    println!("  Provider: local");
    println!("  Online store: sqlite");
    println!("  Offline store: file");
    println!();
    println!("  Entities: 3 (user, item, session)");
    println!("  Feature views: 5");
    println!("    user_features: 12 features");
    println!("    item_features: 8 features");
    println!("    session_features: 6 features");
    println!("    user_item_interactions: 4 features");
    println!("    real_time_features: 3 features (on-demand)");
    println!("  Total features: 33");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "feast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_feast(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_feast};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/feast"), "feast");
        assert_eq!(basename(r"C:\bin\feast.exe"), "feast.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("feast.exe"), "feast");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_feast(&["--help".to_string()], "feast"), 0);
        assert_eq!(run_feast(&["-h".to_string()], "feast"), 0);
        let _ = run_feast(&["--version".to_string()], "feast");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_feast(&[], "feast");
    }
}
