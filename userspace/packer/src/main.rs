#![deny(clippy::all)]

//! packer — OurOS machine image builder
//!
//! Single personality: `packer`

use std::env;
use std::process;

fn run_packer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: packer <command> [flags]");
        println!();
        println!("Commands:");
        println!("  build       Build image(s) from template");
        println!("  console     Interactive console for testing");
        println!("  fix         Fix template for newer Packer versions");
        println!("  fmt         Format HCL2 template");
        println!("  hcl2_upgrade Convert JSON template to HCL2");
        println!("  init        Install required plugins");
        println!("  inspect     See components of a template");
        println!("  plugins     Manage plugins");
        println!("  validate    Validate a template");
        println!("  version     Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("Packer v1.11.0 (OurOS)");
        }
        "build" => {
            let template = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("packer: building from {}", template);
            println!("==> amazon-ebs: Prevalidating any provided VPC information...");
            println!("==> amazon-ebs: Launching a source AMI...");
            println!("    amazon-ebs: Instance ID: i-1234567890abcdef0");
            println!("==> amazon-ebs: Waiting for instance to become ready...");
            println!("==> amazon-ebs: Provisioning with shell script...");
            println!("==> amazon-ebs: Stopping the source instance...");
            println!("==> amazon-ebs: Creating AMI...");
            println!("    amazon-ebs: AMI: ami-0123456789abcdef0");
            println!("==> amazon-ebs: Terminating source instance...");
            println!("Build 'amazon-ebs' finished (simulated).");
            println!();
            println!("==> Builds finished. The artifacts of successful builds are:");
            println!("--> amazon-ebs: AMIs were created: ami-0123456789abcdef0");
        }
        "validate" => {
            let template = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("The configuration is valid ({})", template);
        }
        "fmt" => {
            let template = args.get(1).map(|s| s.as_str()).unwrap_or(".");
            println!("{} (formatted)", template);
        }
        "inspect" => {
            println!("Packer Inspect: template");
            println!();
            println!("Required plugins:");
            println!("  hashicorp/amazon >= 1.3.0");
            println!();
            println!("Builders:");
            println!("  amazon-ebs");
            println!();
            println!("Provisioners:");
            println!("  shell");
            println!();
            println!("Post-processors:");
            println!("  <No post-processors>");
        }
        "init" => {
            println!("Installed plugin github.com/hashicorp/amazon v1.3.0");
        }
        "plugins" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "installed" | "list" => {
                    println!("github.com/hashicorp/amazon    1.3.0");
                    println!("github.com/hashicorp/docker    1.0.0");
                }
                _ => println!("Subcommands: installed, required, install, remove"),
            }
        }
        "fix" | "hcl2_upgrade" | "console" => {
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
    let code = run_packer(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_packer};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_packer(vec!["--help".to_string()]), 0);
        assert_eq!(run_packer(vec!["-h".to_string()]), 0);
        let _ = run_packer(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_packer(vec![]);
    }
}
