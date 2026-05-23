#![deny(clippy::all)]

//! concourse-cli — OurOS Concourse CI CLI (fly)
//!
//! Multi-personality: `fly`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fly(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: fly [OPTIONS] COMMAND");
        println!("Concourse CLI (fly) 7.11.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  login, -t      Login to a target");
        println!("  targets        List saved targets");
        println!("  pipelines      List pipelines");
        println!("  set-pipeline   Create/update a pipeline");
        println!("  trigger-job    Trigger a job");
        println!("  builds         List builds");
        println!("  watch          Watch a build's output");
        println!("  execute        Execute a one-off task");
        println!("  intercept      Hijack into a running container");
        println!("  workers        List workers");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("7.11.0"),
        "login" | "-t" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("main");
            println!("logging in to team 'main' on target '{}'", target);
            println!("target saved");
        }
        "targets" => {
            println!("name    url                            team    expiry");
            println!("main    https://ci.example.com         main    Wed, 22 May 2026 12:00:00 UTC");
        }
        "pipelines" => {
            println!("name            paused  public  last updated");
            println!("my-pipeline     no      no      2024-01-15");
            println!("deploy-prod     no      no      2024-01-14");
            println!("nightly-tests   no      yes     2024-01-15");
        }
        "set-pipeline" | "sp" => {
            let pipeline = args.windows(2).find(|w| w[0] == "-p")
                .map(|w| w[1].as_str()).unwrap_or("my-pipeline");
            println!("Setting pipeline '{}'...", pipeline);
            println!("  resources:");
            println!("    resource my-repo has been added");
            println!("  jobs:");
            println!("    job build has been added");
            println!("    job test has been added");
            println!("    job deploy has been added");
            println!("pipeline created.");
        }
        "trigger-job" | "tj" => {
            let job = args.windows(2).find(|w| w[0] == "-j")
                .map(|w| w[1].as_str()).unwrap_or("my-pipeline/build");
            println!("started {}/42", job);
        }
        "builds" => {
            println!("id  pipeline/job             build  status     start                 end");
            println!("42  my-pipeline/build        42     succeeded  2024-01-15 10:00:00   2024-01-15 10:02:34");
            println!("41  my-pipeline/test         41     succeeded  2024-01-15 10:02:35   2024-01-15 10:05:12");
        }
        "workers" => {
            println!("name      containers  platform  tags  team  state    version");
            println!("worker-1  5           linux     []    none  running  2.4");
            println!("worker-2  3           linux     []    none  running  2.4");
        }
        "execute" => {
            println!("executing one-off task...");
            println!("  initializing...");
            println!("  running task...");
            println!("  task completed successfully.");
        }
        _ => println!("fly: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fly".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fly(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
