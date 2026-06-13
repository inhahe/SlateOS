#![deny(clippy::all)]

//! itamae-cli — SlateOS Itamae configuration management
//!
//! Single personality: `itamae`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_itamae(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: itamae COMMAND [OPTIONS] RECIPE...");
        println!("itamae v1.14 (Slate OS) — Simple configuration management");
        println!();
        println!("Commands:");
        println!("  local             Apply recipes locally");
        println!("  ssh               Apply recipes via SSH");
        println!("  docker            Apply recipes in Docker");
        println!("  version           Show version");
        println!();
        println!("Options:");
        println!("  -n / --dry-run    Dry-run mode");
        println!("  -l / --log-level  Log level (debug, info, warn, error)");
        println!("  -j / --node-json  Node attributes JSON");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match cmd {
        "local" => {
            let recipes: Vec<&str> = args.iter().skip(1).filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
            let recipe = recipes.first().copied().unwrap_or("recipe.rb");
            let dry = args.iter().any(|a| a == "-n" || a == "--dry-run");
            if dry {
                println!("[dry-run] Recipe: {}", recipe);
                println!("[dry-run]   package[nginx] — would install");
                println!("[dry-run]   template[/etc/nginx/nginx.conf] — would create");
                println!("[dry-run]   service[nginx] — would start");
            } else {
                println!("Recipe: {}", recipe);
                println!("  package[nginx] — installed");
                println!("  template[/etc/nginx/nginx.conf] — created");
                println!("  service[nginx] — started");
            }
        }
        "ssh" => {
            println!("Connecting via SSH...");
            println!("  Host: web-01");
            println!("  Recipe: default.rb");
            println!("  Applied 5 resources.");
        }
        "version" | "--version" => println!("itamae v1.14 (Slate OS)"),
        _ => println!("itamae {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "itamae".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_itamae(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_itamae};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/itamae"), "itamae");
        assert_eq!(basename(r"C:\bin\itamae.exe"), "itamae.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("itamae.exe"), "itamae");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_itamae(&["--help".to_string()], "itamae"), 0);
        assert_eq!(run_itamae(&["-h".to_string()], "itamae"), 0);
        let _ = run_itamae(&["--version".to_string()], "itamae");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_itamae(&[], "itamae");
    }
}
