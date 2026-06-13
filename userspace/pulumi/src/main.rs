#![deny(clippy::all)]

//! pulumi — Slate OS Infrastructure as Code
//!
//! Single personality: `pulumi`

use std::env;
use std::process;

fn run_pulumi(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pulumi <command> [flags]");
        println!();
        println!("Commands:");
        println!("  new           Create a new project");
        println!("  up            Create or update resources");
        println!("  preview       Preview changes");
        println!("  destroy       Destroy resources");
        println!("  refresh       Refresh resource state");
        println!("  stack         Manage stacks");
        println!("  config        Manage configuration");
        println!("  import        Import resources");
        println!("  export        Export stack state");
        println!("  login         Log in to backend");
        println!("  logout        Log out from backend");
        println!("  whoami        Show current user");
        println!("  org           Manage organizations");
        println!("  plugin        Manage plugins");
        println!("  about         Show environment info");
        println!("  version       Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => println!("v3.117.0 (Slate OS)"),
        "about" => {
            println!("CLI          ");
            println!("Version      3.117.0");
            println!("Go Version   go1.22");
            println!("Go Compiler  gc");
            println!();
            println!("Plugins");
            println!("NAME     VERSION");
            println!("aws      6.32.0");
            println!("gcp      7.21.0");
            println!("azure    5.73.0");
            println!();
            println!("Host");
            println!("OS       slateos");
            println!("Arch     x86_64");
        }
        "whoami" => println!("user@example.com"),
        "preview" | "up" => {
            let is_up = cmd == "up";
            println!("Previewing update (dev)");
            println!();
            println!("     Type                     Name              Plan");
            println!(" +   pulumi:pulumi:Stack      myproject-dev     create");
            println!(" +   aws:s3:Bucket            my-bucket         create");
            println!(" +   aws:ec2:Instance          my-server         create");
            println!();
            println!("Resources:");
            println!("    + 3 to create");
            if is_up {
                println!();
                println!("Updating (dev)");
                println!("Resources:");
                println!("    + 3 created");
                println!();
                println!("Duration: 25s");
            }
        }
        "destroy" => {
            println!("Destroying (dev)");
            println!();
            println!("     Type                     Name              Plan");
            println!(" -   aws:ec2:Instance          my-server         delete");
            println!(" -   aws:s3:Bucket            my-bucket         delete");
            println!(" -   pulumi:pulumi:Stack      myproject-dev     delete");
            println!();
            println!("Resources:");
            println!("    - 3 deleted");
        }
        "stack" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" | "list" => {
                    println!("NAME  LAST UPDATE  RESOURCE COUNT  URL");
                    println!("dev   2 hours ago  3               https://app.pulumi.com/...");
                    println!("prod  1 day ago    5               https://app.pulumi.com/...");
                }
                "select" => println!("Stack selected."),
                _ => println!("Subcommands: ls, select, init, rm, output, tag, history"),
            }
        }
        "config" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("KEY               VALUE");
                    println!("aws:region        us-east-1");
                    println!("myproject:env     dev");
                }
                _ => println!("Subcommands: list, set, get, rm, refresh"),
            }
        }
        "new" | "login" | "logout" | "refresh" | "import" | "export" | "plugin" | "org" => {
            println!("({} — simulated)", cmd);
        }
        _ => {
            eprintln!("Unknown command '{}'. Use --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pulumi(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pulumi};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pulumi(vec!["--help".to_string()]), 0);
        assert_eq!(run_pulumi(vec!["-h".to_string()]), 0);
        let _ = run_pulumi(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pulumi(vec![]);
    }
}
