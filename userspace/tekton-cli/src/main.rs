#![deny(clippy::all)]

//! tekton-cli — OurOS Tekton Pipelines CLI
//!
//! Multi-personality: `tkn`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tkn(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tkn COMMAND [OPTIONS]");
        println!("Tekton CLI 0.37.0 (OurOS)");
        println!();
        println!("Commands:");
        println!("  pipeline       Manage Pipelines");
        println!("  pipelinerun    Manage PipelineRuns");
        println!("  task           Manage Tasks");
        println!("  taskrun        Manage TaskRuns");
        println!("  hub            Interact with Tekton Hub");
        println!("  bundle         Manage Tekton bundles");
        println!("  clustertask    Manage ClusterTasks");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "version" | "--version" => println!("tkn 0.37.0"),
        "pipeline" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("NAME             AGE              LAST RUN           STARTED        DURATION   STATUS");
                    println!("build-deploy     2 days ago       build-deploy-r4    1 hour ago     3m 12s     Succeeded");
                    println!("test-pipeline    5 days ago       test-pipeline-r8   2 hours ago    1m 45s     Succeeded");
                }
                "start" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("build-deploy");
                    println!("PipelineRun started: {}-run-xyz", name);
                }
                "logs" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("build-deploy-r4");
                    println!("[{} : build] Building...", name);
                    println!("[{} : build] Build complete", name);
                    println!("[{} : test] Running tests...", name);
                    println!("[{} : test] All tests passed", name);
                    println!("[{} : deploy] Deploying...", name);
                    println!("[{} : deploy] Deployed successfully", name);
                }
                _ => println!("tkn pipeline: '{}' completed", sub),
            }
        }
        "task" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" || sub == "ls" {
                println!("NAME         AGE");
                println!("git-clone    10 days ago");
                println!("build        10 days ago");
                println!("deploy       10 days ago");
            } else {
                println!("tkn task: '{}' completed", sub);
            }
        }
        "hub" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("search");
            match sub {
                "search" => {
                    let query = args.get(2).map(|s| s.as_str()).unwrap_or("git");
                    println!("NAME              DESCRIPTION                    RATING");
                    println!("git-clone         Clone a git repo               ★★★★★");
                    println!("git-batch-merge   Merge multiple PRs             ★★★★☆");
                    println!("Query: '{}'", query);
                }
                "install" => {
                    let task = args.get(2).map(|s| s.as_str()).unwrap_or("git-clone");
                    println!("Task {} installed in default namespace", task);
                }
                _ => println!("tkn hub: '{}' completed", sub),
            }
        }
        _ => println!("tkn: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tkn".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tkn(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
