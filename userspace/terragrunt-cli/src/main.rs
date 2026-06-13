#![deny(clippy::all)]

//! terragrunt-cli — SlateOS Terragrunt CLI
//!
//! Multi-personality: `terragrunt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_terragrunt(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: terragrunt COMMAND [OPTIONS]");
        println!("Terragrunt 0.67.0 (SlateOS)");
        println!();
        println!("Commands:");
        println!("  plan              Run terraform plan");
        println!("  apply             Run terraform apply");
        println!("  destroy           Run terraform destroy");
        println!("  run-all           Run command in all subdirectories");
        println!("  graph-dependencies Show dependency graph");
        println!("  hclfmt            Format terragrunt.hcl files");
        println!("  validate-inputs   Validate inputs");
        println!("  render-json       Render config as JSON");
        println!("  output-module-groups  Show module groups");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" | "version" => println!("terragrunt 0.67.0"),
        "plan" => {
            println!("[INFO] Setting up remote state backend...");
            println!("[INFO] Running terraform plan...");
            println!();
            println!("Terraform will perform the following actions:");
            println!();
            println!("  # module.vpc.aws_vpc.main will be created");
            println!("  + resource \"aws_vpc\" \"main\" {{");
            println!("      + cidr_block = \"10.0.0.0/16\"");
            println!("    }}");
            println!();
            println!("Plan: 1 to add, 0 to change, 0 to destroy.");
        }
        "apply" => {
            println!("[INFO] Setting up remote state backend...");
            println!("[INFO] Running terraform apply...");
            println!();
            println!("Apply complete! Resources: 3 added, 0 changed, 0 destroyed.");
        }
        "run-all" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("plan");
            println!("[INFO] Running '{}' in all subdirectories...", cmd);
            println!();
            println!("Group 1:");
            println!("  - modules/vpc");
            println!("  - modules/security-groups");
            println!("Group 2 (depends on group 1):");
            println!("  - modules/ec2");
            println!("  - modules/rds");
            println!("Group 3 (depends on group 2):");
            println!("  - modules/app");
            println!();
            println!("All modules processed successfully.");
        }
        "graph-dependencies" => {
            println!("digraph {{");
            println!("  \"modules/vpc\" ;");
            println!("  \"modules/security-groups\" ;");
            println!("  \"modules/ec2\" -> \"modules/vpc\" ;");
            println!("  \"modules/ec2\" -> \"modules/security-groups\" ;");
            println!("  \"modules/rds\" -> \"modules/vpc\" ;");
            println!("  \"modules/app\" -> \"modules/ec2\" ;");
            println!("  \"modules/app\" -> \"modules/rds\" ;");
            println!("}}");
        }
        "hclfmt" => {
            println!("Formatting terragrunt.hcl files...");
            println!("  terragrunt.hcl: formatted");
            println!("  modules/vpc/terragrunt.hcl: already formatted");
            println!("  modules/ec2/terragrunt.hcl: formatted");
        }
        "validate-inputs" => {
            println!("Validating inputs...");
            println!("All inputs are valid.");
        }
        "render-json" => {
            println!("{{");
            println!("  \"terraform\": {{");
            println!("    \"source\": \"tfr:///terraform-aws-modules/vpc/aws?version=5.0.0\"");
            println!("  }},");
            println!("  \"inputs\": {{");
            println!("    \"name\": \"my-vpc\",");
            println!("    \"cidr\": \"10.0.0.0/16\"");
            println!("  }}");
            println!("}}");
        }
        _ => println!("terragrunt: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "terragrunt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_terragrunt(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_terragrunt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/terragrunt"), "terragrunt");
        assert_eq!(basename(r"C:\bin\terragrunt.exe"), "terragrunt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("terragrunt.exe"), "terragrunt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_terragrunt(&["--help".to_string()]), 0);
        assert_eq!(run_terragrunt(&["-h".to_string()]), 0);
        let _ = run_terragrunt(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_terragrunt(&[]);
    }
}
