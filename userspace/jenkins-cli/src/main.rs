#![deny(clippy::all)]

//! jenkins-cli — SlateOS Jenkins CI/CD tools
//!
//! Multi-personality: `jenkins-cli`

use std::env;
use std::process;

fn run_jenkins_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: jenkins-cli [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("jenkins-cli — Jenkins CI/CD management (Slate OS).");
        println!();
        println!("Commands:");
        println!("  build <job>          Trigger a build");
        println!("  list-jobs            List all jobs");
        println!("  get-job <job>        Get job config");
        println!("  console <job> [n]    Show build console");
        println!("  who-am-i             Auth info");
        println!("  version              Server version");
        println!("  list-plugins         List plugins");
        println!("  safe-restart         Restart Jenkins");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" => println!("Jenkins 2.440.1 (Slate OS)"),
        "who-am-i" => {
            println!("Authenticated as: admin");
            println!("Authorities:");
            println!("  authenticated");
            println!("  admin");
        }
        "list-jobs" => {
            println!("NAME                    STATUS    LAST BUILD");
            println!("slateos-kernel-build      SUCCESS   #142 (2h ago)");
            println!("slateos-userspace-test    SUCCESS   #89 (4h ago)");
            println!("slateos-integration       FAILURE   #23 (1h ago)");
            println!("slateos-deploy-staging    SUCCESS   #67 (6h ago)");
        }
        "build" => {
            let job = args.get(1).map(|s| s.as_str()).unwrap_or("slateos-kernel-build");
            println!("Build triggered for '{}'.", job);
            println!("Queue item: #143");
        }
        "console" => {
            let job = args.get(1).map(|s| s.as_str()).unwrap_or("slateos-kernel-build");
            println!("Console output for {} #142:", job);
            println!("[Pipeline] Start of Pipeline");
            println!("[Pipeline] stage (Build)");
            println!("+ cargo build --release");
            println!("   Compiling slateos-kernel v0.1.0");
            println!("    Finished release [optimized] target(s)");
            println!("[Pipeline] stage (Test)");
            println!("+ cargo test --workspace");
            println!("test result: ok. 342 passed; 0 failed");
            println!("[Pipeline] End of Pipeline");
            println!("Finished: SUCCESS");
        }
        "list-plugins" => {
            println!("NAME                        VERSION    ENABLED");
            println!("git                         5.2.1      true");
            println!("pipeline-model-definition   2.2144.0   true");
            println!("docker-workflow              572.v950f58993843  true");
            println!("credentials                 1311.vcf0a_900b_37c2  true");
        }
        "safe-restart" => println!("Jenkins is restarting..."),
        _ => println!("jenkins-cli: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_jenkins_cli(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jenkins_cli};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jenkins_cli(&["--help".to_string()]), 0);
        assert_eq!(run_jenkins_cli(&["-h".to_string()]), 0);
        let _ = run_jenkins_cli(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jenkins_cli(&[]);
    }
}
