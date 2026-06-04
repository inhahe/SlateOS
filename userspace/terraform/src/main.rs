#![deny(clippy::all)]

//! terraform — OurOS infrastructure as code tool
//!
//! Single personality: `terraform`

use std::env;
use std::process;

// ── Main logic ────────────────────────────────────────────────────────

fn run_terraform(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" | "-help" => {
            println!("Usage: terraform [global options] <subcommand> [args]");
            println!();
            println!("Main commands:");
            println!("  init          Prepare your working directory");
            println!("  validate      Check whether the configuration is valid");
            println!("  plan          Show changes required by the current configuration");
            println!("  apply         Create or update infrastructure");
            println!("  destroy       Destroy previously-created infrastructure");
            println!();
            println!("Other commands:");
            println!("  console       Try Terraform expressions");
            println!("  fmt           Reformat your configuration");
            println!("  force-unlock  Release a stuck lock");
            println!("  get           Install or upgrade remote modules");
            println!("  graph         Generate a Graphviz graph");
            println!("  import        Associate existing infra with Terraform resource");
            println!("  login         Obtain and save API token");
            println!("  logout        Remove locally-stored credentials");
            println!("  output        Show output values from state");
            println!("  providers     Show the providers required");
            println!("  refresh       Update the state to match remote systems");
            println!("  show          Inspect Terraform state or plan");
            println!("  state         Advanced state management");
            println!("  taint         Mark a resource for recreation");
            println!("  untaint       Remove the taint from a resource");
            println!("  workspace     Workspace management");
            println!("  --version     Show version");
            0
        }
        "--version" | "-version" | "version" => {
            println!("Terraform v1.8.0 (OurOS)");
            println!("on ouros_amd64");
            0
        }
        "init" => {
            println!("Initializing the backend...");
            println!();
            println!("Initializing provider plugins...");
            println!("- Finding hashicorp/aws versions matching \"~> 5.0\"...");
            println!("- Installing hashicorp/aws v5.40.0...");
            println!("- Installed hashicorp/aws v5.40.0 (signed by HashiCorp)");
            println!();
            println!("Terraform has been successfully initialized!");
            println!();
            println!("You may now begin working with Terraform. Try running \"terraform plan\".");
            0
        }
        "validate" => {
            println!("Success! The configuration is valid.");
            0
        }
        "plan" => {
            let destroy = cmd_args.iter().any(|a| a == "-destroy");
            println!("Terraform used the selected providers to generate the following execution plan.");
            println!();
            if destroy {
                println!("Plan: 0 to add, 0 to change, 3 to destroy.");
            } else {
                println!("  # aws_instance.web will be created");
                println!("  + resource \"aws_instance\" \"web\" {{");
                println!("      + ami                    = \"ami-0123456789abcdef0\"");
                println!("      + instance_type          = \"t3.micro\"");
                println!("      + tags                   = {{");
                println!("          + \"Name\" = \"web-server\"");
                println!("        }}");
                println!("    }}");
                println!();
                println!("Plan: 1 to add, 0 to change, 0 to destroy.");
            }
            0
        }
        "apply" => {
            let auto_approve = cmd_args.iter().any(|a| a == "-auto-approve");
            if !auto_approve {
                println!("Do you want to perform these actions?");
                println!("  Terraform will perform the actions described above.");
                println!("  Only 'yes' will be accepted to approve.");
                println!("  Enter a value: yes");
            }
            println!();
            println!("aws_instance.web: Creating...");
            println!("aws_instance.web: Still creating... [10s elapsed]");
            println!("aws_instance.web: Creation complete after 15s [id=i-1234567890abcdef0]");
            println!();
            println!("Apply complete! Resources: 1 added, 0 changed, 0 destroyed.");
            0
        }
        "destroy" => {
            println!("aws_instance.web: Destroying... [id=i-1234567890abcdef0]");
            println!("aws_instance.web: Destruction complete after 30s");
            println!();
            println!("Destroy complete! Resources: 1 destroyed.");
            0
        }
        "fmt" => {
            let check = cmd_args.iter().any(|a| a == "-check");
            if check {
                println!("main.tf");
                println!("(files would be reformatted)");
            } else {
                println!("main.tf");
            }
            0
        }
        "output" => {
            let name = cmd_args.first().map(|s| s.as_str());
            match name {
                Some(n) => println!("\"{}\" = \"value-123\" (simulated)", n),
                None => {
                    println!("instance_ip = \"10.0.1.50\"");
                    println!("instance_id = \"i-1234567890abcdef0\"");
                }
            }
            0
        }
        "state" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("aws_instance.web");
                    println!("aws_security_group.web_sg");
                }
                "show" => {
                    let resource = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("aws_instance.web");
                    println!("# {}:", resource);
                    println!("resource \"aws_instance\" \"web\" {{");
                    println!("    ami           = \"ami-0123456789abcdef0\"");
                    println!("    instance_type = \"t3.micro\"");
                    println!("    id            = \"i-1234567890abcdef0\"");
                    println!("}}");
                }
                "rm" => println!("Removed from state (simulated)"),
                "mv" => println!("Moved in state (simulated)"),
                "pull" => println!("(pulling state — simulated)"),
                "push" => println!("(pushing state — simulated)"),
                _ => println!("state {}: (simulated)", sub),
            }
            0
        }
        "workspace" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  default");
                    println!("* production");
                    println!("  staging");
                }
                "new" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("workspace");
                    println!("Created and switched to workspace \"{}\"!", name);
                }
                "select" => {
                    let name = cmd_args.get(1).map(|s| s.as_str()).unwrap_or("default");
                    println!("Switched to workspace \"{}\".", name);
                }
                "delete" => println!("Deleted workspace (simulated)"),
                _ => println!("workspace {}: (simulated)", sub),
            }
            0
        }
        "graph" => {
            println!("digraph {{");
            println!("  \"aws_instance.web\" -> \"aws_security_group.web_sg\"");
            println!("}}");
            0
        }
        "providers" => {
            println!("Providers required by configuration:");
            println!(".");
            println!("└── provider[registry.terraform.io/hashicorp/aws] ~> 5.0");
            0
        }
        "show" => { println!("(showing state/plan — simulated)"); 0 }
        "import" => { println!("Import successful (simulated)"); 0 }
        "refresh" => { println!("aws_instance.web: Refreshing state... [id=i-1234567890abcdef0]"); 0 }
        "taint" => { println!("Resource has been marked as tainted (simulated)"); 0 }
        "untaint" => { println!("Resource has been untainted (simulated)"); 0 }
        "console" => { println!("(interactive console — simulated)"); 0 }
        "get" => { println!("Downloading modules... done (simulated)"); 0 }
        other => { eprintln!("terraform: unknown command \"{}\"", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_terraform(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{run_terraform};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_terraform(vec!["--help".to_string()]), 0);
        assert_eq!(run_terraform(vec!["-h".to_string()]), 0);
        let _ = run_terraform(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_terraform(vec![]);
    }
}
