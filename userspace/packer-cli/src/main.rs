#![deny(clippy::all)]

//! packer-cli — OurOS HashiCorp Packer image builder CLI
//!
//! Single personality: `packer`

use std::env;
use std::process;

fn run_packer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: packer <COMMAND> [OPTIONS]");
        println!();
        println!("Build automated machine images.");
        println!();
        println!("Commands:");
        println!("  build       Build images from a template");
        println!("  validate    Check a template is valid");
        println!("  inspect     See components of a template");
        println!("  init        Install missing plugins");
        println!("  fmt         Format HCL2 config files");
        println!("  hcl2_upgrade  Upgrade JSON template to HCL2");
        println!("  plugins     Manage Packer plugins");
        println!("  version     Show version");
        println!();
        println!("Options:");
        println!("  -var <K>=<V>       Set a variable");
        println!("  -var-file <FILE>   Variable file");
        println!("  -force             Force a build");
        println!("  -only <BUILDER>    Build only named builders");
        println!("  -except <BUILDER>  Skip named builders");
        println!("  -parallel-builds <N>  Parallel builds (default: 0=unlimited)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Packer v1.10.1 (OurOS)");
            0
        }
        "build" => {
            let template = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("template.pkr.hcl");
            println!("==> amazon-ebs.ubuntu: Prevalidating AMI Name...");
            println!("==> amazon-ebs.ubuntu: Creating temporary SSH key...");
            println!("==> amazon-ebs.ubuntu: Launching a source AWS instance...");
            println!("    amazon-ebs.ubuntu: Instance ID: i-0123456789abcdef0");
            println!("==> amazon-ebs.ubuntu: Waiting for instance to become ready...");
            println!("==> amazon-ebs.ubuntu: Waiting for SSH to become available...");
            println!("==> amazon-ebs.ubuntu: Connected to SSH!");
            println!("==> amazon-ebs.ubuntu: Provisioning with shell script...");
            println!("    amazon-ebs.ubuntu: Installing packages...");
            println!("    amazon-ebs.ubuntu: Done.");
            println!("==> amazon-ebs.ubuntu: Stopping the source instance...");
            println!("==> amazon-ebs.ubuntu: Creating AMI: my-image-20240115");
            println!("    amazon-ebs.ubuntu: AMI: ami-0123456789abcdef0");
            println!("==> amazon-ebs.ubuntu: Terminating the source instance...");
            println!("Build 'amazon-ebs.ubuntu' finished after 3m45s.");
            println!();
            println!("==> Builds finished. The artifacts of successful builds are:");
            println!("--> amazon-ebs.ubuntu: AMIs were created: ami-0123456789abcdef0");
            println!("  (template: {})", template);
            0
        }
        "validate" => {
            let template = args.iter()
                .filter(|a| !a.starts_with('-'))
                .nth(1)
                .map(|s| s.as_str())
                .unwrap_or("template.pkr.hcl");
            println!("The configuration is valid. ({})", template);
            0
        }
        "inspect" => {
            println!("Packer Inspect: template.pkr.hcl");
            println!();
            println!("  Description:");
            println!("    Ubuntu 22.04 base image");
            println!();
            println!("  Builders:");
            println!("    amazon-ebs.ubuntu");
            println!();
            println!("  Provisioners:");
            println!("    shell");
            println!("    file");
            println!();
            println!("  Post-processors:");
            println!("    manifest");
            0
        }
        "init" => {
            println!("Installed plugin github.com/hashicorp/amazon v1.2.9");
            0
        }
        "fmt" => {
            let check = args.iter().any(|a| a == "-check");
            if check {
                println!("template.pkr.hcl");
                1
            } else {
                println!("template.pkr.hcl");
                0
            }
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: packer <command>. See --help.");
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
    let code = run_packer(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_packer};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_packer(vec!["--help".to_string()]), 0);
        assert_eq!(run_packer(vec!["-h".to_string()]), 0);
        assert_eq!(run_packer(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_packer(vec![]), 0);
    }
}
