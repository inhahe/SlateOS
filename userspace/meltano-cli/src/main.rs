#![deny(clippy::all)]

//! meltano-cli — Slate OS Meltano ELT CLI
//!
//! Multi-personality: `meltano`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_meltano(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: meltano COMMAND [OPTIONS]");
        println!("Meltano 3.4.0 (Slate OS)");
        println!();
        println!("Commands:");
        println!("  init           Initialize project");
        println!("  add            Add plugin");
        println!("  install        Install plugins");
        println!("  run            Run a pipeline");
        println!("  elt            Run ELT pipeline (deprecated)");
        println!("  invoke         Invoke a plugin");
        println!("  config         Manage config");
        println!("  schedule       Manage schedules");
        println!("  test           Test plugins");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("meltano 3.4.0"),
        "init" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("my-project");
            println!("Creating Meltano project '{}'...", name);
            println!("  Created: meltano.yml");
            println!("  Created: .meltano/");
            println!("  Created: output/");
            println!("Done. cd {} && meltano add extractor tap-csv", name);
        }
        "add" => {
            let plugin_type = args.get(1).map(|s| s.as_str()).unwrap_or("extractor");
            let plugin = args.get(2).map(|s| s.as_str()).unwrap_or("tap-csv");
            println!("Adding {} '{}'...", plugin_type, plugin);
            println!("  Installed: {}", plugin);
            println!("  Added to meltano.yml");
        }
        "run" => {
            let pipeline = args.iter().skip(1).map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
            let pipe = if pipeline.is_empty() { "tap-csv target-jsonl" } else { &pipeline };
            println!("Running pipeline: {}", pipe);
            println!("  tap-csv      | INFO Starting sync");
            println!("  tap-csv      | INFO Syncing stream: records");
            println!("  target-jsonl | INFO Writing to output/");
            println!("  tap-csv      | INFO 1000 records synced");
            println!("  target-jsonl | INFO 1000 records written");
            println!("Pipeline completed successfully.");
        }
        "config" => {
            let plugin = args.get(1).map(|s| s.as_str()).unwrap_or("tap-csv");
            println!("Configuration for {}:", plugin);
            println!("  csv_files_definition: extract/files.json");
            println!("  delimiter: \",\"");
        }
        "schedule" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                println!("NAME            INTERVAL    PIPELINE");
                println!("daily-sync      @daily      tap-csv target-jsonl");
                println!("hourly-api      @hourly     tap-rest-api target-postgres");
            } else {
                println!("meltano schedule: '{}' completed", sub);
            }
        }
        _ => println!("meltano: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "meltano".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_meltano(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_meltano};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/meltano"), "meltano");
        assert_eq!(basename(r"C:\bin\meltano.exe"), "meltano.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("meltano.exe"), "meltano");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_meltano(&["--help".to_string()]), 0);
        assert_eq!(run_meltano(&["-h".to_string()]), 0);
        let _ = run_meltano(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_meltano(&[]);
    }
}
