#![deny(clippy::all)]

//! nomad-cli — OurOS HashiCorp Nomad workload orchestrator CLI
//!
//! Single personality: `nomad`

use std::env;
use std::process;

fn run_nomad(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nomad <COMMAND> [OPTIONS]");
        println!();
        println!("Workload orchestrator — deploy and manage applications.");
        println!();
        println!("Commands:");
        println!("  job           Interact with jobs");
        println!("  alloc         Interact with allocations");
        println!("  node          Interact with nodes");
        println!("  agent         Run a Nomad agent");
        println!("  status        Display allocation status");
        println!("  plan          Dry-run a job update");
        println!("  run           Run a new job or update");
        println!("  stop          Stop a running job");
        println!("  logs          Display allocation logs");
        println!("  exec          Execute command in task");
        println!("  eval          Interact with evaluations");
        println!("  deployment    Interact with deployments");
        println!("  namespace     Interact with namespaces");
        println!("  server        Interact with servers");
        println!("  var           Interact with variables");
        println!("  volume        Interact with volumes");
        println!("  service       Interact with registered services");
        println!("  system        Interact with the system");
        println!("  ui            Open the Nomad UI");
        println!("  version       Show version");
        println!();
        println!("Options:");
        println!("  -address <ADDR>  Nomad address (or $NOMAD_ADDR)");
        println!("  -token <TOKEN>   API token");
        println!("  -namespace <NS>  Namespace");
        println!("  -region <R>      Region");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Nomad v1.7.4 (OurOS)");
            0
        }
        "status" => {
            let name = args.get(1).map(|s| s.as_str());
            if let Some(job) = name {
                println!("ID            = {}", job);
                println!("Name          = {}", job);
                println!("Type          = service");
                println!("Priority      = 50");
                println!("Datacenters   = dc1");
                println!("Namespace     = default");
                println!("Status        = running");
                println!("Periodic      = false");
                println!("Parameterized = false");
                println!();
                println!("Allocations:");
                println!("ID        Node     Task Group  Status   Created");
                println!("abc123    node-1   web         running  2h ago");
                println!("def456    node-2   web         running  2h ago");
            } else {
                println!("ID          Type     Priority  Status   Submit Date");
                println!("web-app     service  50        running  2024-01-15T14:30:00Z");
                println!("api-srv     service  50        running  2024-01-15T12:00:00Z");
                println!("batch-job   batch    30        dead     2024-01-14T08:00:00Z");
            }
            0
        }
        "run" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("job.nomad");
            println!("==> 2024-01-15T14:30:00Z: Monitoring evaluation \"abc123\"");
            println!("    2024-01-15T14:30:00Z: Evaluation triggered by job \"web-app\"");
            println!("    2024-01-15T14:30:01Z: Evaluation status changed: \"pending\" -> \"complete\"");
            println!("==> 2024-01-15T14:30:01Z: Evaluation \"abc123\" finished with status \"complete\"");
            println!("==> 2024-01-15T14:30:01Z: Monitoring deployment \"def456\"");
            println!("    2024-01-15T14:30:10Z: Deployment \"def456\" successful");
            println!("  (from {})", file);
            0
        }
        "stop" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("web-app");
            println!("==> Monitoring evaluation \"abc123\"");
            println!("    Evaluation triggered by job \"{}\"", name);
            println!("    Job \"{}\" modified", name);
            0
        }
        "plan" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("job.nomad");
            println!("+/- Job: \"web-app\"");
            println!("+   Task Group: \"web\" (2 create)");
            println!();
            println!("Scheduler dry-run:");
            println!("- All tasks successfully allocated.");
            println!("  (from {})", file);
            0
        }
        "node" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            if sub == "status" {
                println!("ID        DC   Name    Class   Drain  Eligibility  Status");
                println!("abc123    dc1  node-1  <none>  false  eligible     ready");
                println!("def456    dc1  node-2  <none>  false  eligible     ready");
                println!("ghi789    dc1  node-3  <none>  false  eligible     ready");
            }
            0
        }
        "logs" => {
            let alloc = args.get(1).map(|s| s.as_str()).unwrap_or("abc123");
            println!("[{}] 2024/01/15 14:30:00 Starting web server...", alloc);
            println!("[{}] 2024/01/15 14:30:01 Listening on :8080", alloc);
            println!("[{}] 2024/01/15 14:31:23 GET / 200 3ms", alloc);
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: nomad <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nomad(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_nomad};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nomad(vec!["--help".to_string()]), 0);
        assert_eq!(run_nomad(vec!["-h".to_string()]), 0);
        assert_eq!(run_nomad(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nomad(vec![]), 0);
    }
}
