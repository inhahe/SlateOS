#![deny(clippy::all)]

//! pulumi-cli — OurOS Pulumi infrastructure as code
//!
//! Multi-personality: `pulumi`

use std::env;
use std::process;

fn run_pulumi(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pulumi [FLAGS] COMMAND [ARGS]");
        println!();
        println!("pulumi — infrastructure as code (OurOS).");
        println!();
        println!("Commands:");
        println!("  new            Create new project");
        println!("  up             Deploy changes");
        println!("  preview        Preview changes");
        println!("  destroy        Destroy resources");
        println!("  stack          Manage stacks");
        println!("  config         Manage config");
        println!("  import         Import resources");
        println!("  refresh        Refresh state");
        println!("  whoami         Show current user");
        println!("  version        Show version");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => println!("v3.105.0 (OurOS)"),
        "whoami" => {
            println!("User: admin");
            println!("Backend URL: https://app.pulumi.com/admin");
        }
        "preview" => {
            println!("Previewing update (dev):");
            println!();
            println!("     Type                 Name             Plan");
            println!(" +   pulumi:pulumi:Stack  ouros-infra-dev  create");
            println!(" +   ├─ aws:ec2:Instance  web-server       create");
            println!(" +   ├─ aws:ec2:SecurityGroup  web-sg      create");
            println!(" +   └─ aws:s3:Bucket     data-bucket      create");
            println!();
            println!("Resources:");
            println!("    + 4 to create");
        }
        "up" => {
            println!("Updating (dev):");
            println!();
            println!("     Type                 Name             Status");
            println!(" +   pulumi:pulumi:Stack  ouros-infra-dev  created (3s)");
            println!(" +   ├─ aws:ec2:SecurityGroup  web-sg      created (2s)");
            println!(" +   ├─ aws:ec2:Instance  web-server       created (25s)");
            println!(" +   └─ aws:s3:Bucket     data-bucket      created (3s)");
            println!();
            println!("Outputs:");
            println!("    publicIp : \"203.0.113.10\"");
            println!("    bucketName: \"data-bucket-abc1234\"");
            println!();
            println!("Resources:");
            println!("    + 4 created");
            println!();
            println!("Duration: 33s");
        }
        "destroy" => {
            println!("Destroying (dev):");
            println!();
            println!("     Type                 Name             Status");
            println!(" -   pulumi:pulumi:Stack  ouros-infra-dev  deleted");
            println!(" -   ├─ aws:ec2:Instance  web-server       deleted (15s)");
            println!(" -   ├─ aws:ec2:SecurityGroup  web-sg      deleted (2s)");
            println!(" -   └─ aws:s3:Bucket     data-bucket      deleted (3s)");
            println!();
            println!("Resources:");
            println!("    - 4 deleted");
            println!();
            println!("Duration: 22s");
        }
        "stack" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            match cmd {
                "ls" => {
                    println!("NAME   LAST UPDATE  RESOURCE COUNT  URL");
                    println!("dev    2h ago       4               https://app.pulumi.com/admin/ouros-infra/dev");
                    println!("staging 1d ago      4               https://app.pulumi.com/admin/ouros-infra/staging");
                    println!("prod   3d ago       8               https://app.pulumi.com/admin/ouros-infra/prod");
                }
                "select" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("dev");
                    println!("Set stack to '{}'", name);
                }
                _ => println!("pulumi stack {} completed", cmd),
            }
        }
        "config" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("ls");
            if cmd == "ls" {
                println!("KEY             VALUE");
                println!("aws:region      us-east-1");
                println!("instanceType    t3.medium");
            } else {
                println!("pulumi config {} completed", cmd);
            }
        }
        "refresh" => {
            println!("Refreshing (dev):");
            println!("Resources:");
            println!("    4 unchanged");
            println!("Duration: 5s");
        }
        _ => println!("pulumi: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pulumi(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_pulumi};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_pulumi(&["--help".to_string()]), 0);
        assert_eq!(run_pulumi(&["-h".to_string()]), 0);
        assert_eq!(run_pulumi(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_pulumi(&[]), 0);
    }
}
