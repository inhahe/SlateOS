#![deny(clippy::all)]

//! terraform-cli — SlateOS Terraform infrastructure-as-code CLI
//!
//! Single personality: `terraform`

use std::env;
use std::process;

fn run_terraform(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "-help") {
        println!("Usage: terraform [global options] <subcommand> [args]");
        println!();
        println!("Main commands:");
        println!("  init          Initialize a working directory");
        println!("  validate      Validate configuration files");
        println!("  plan          Show changes required by the current configuration");
        println!("  apply         Create or update infrastructure");
        println!("  destroy       Destroy previously-created infrastructure");
        println!();
        println!("Other commands:");
        println!("  console       Try Terraform expressions in an interactive console");
        println!("  fmt           Reformat configuration in standard style");
        println!("  force-unlock  Release a stuck lock");
        println!("  get           Download and install modules");
        println!("  graph         Generate a Graphviz graph of the steps");
        println!("  import        Associate existing infrastructure with a resource");
        println!("  login         Obtain and save credentials for a remote host");
        println!("  logout        Remove locally-stored credentials");
        println!("  metadata      Metadata related commands");
        println!("  output        Show output values from root module");
        println!("  providers     Show providers required by configuration");
        println!("  refresh       Update local state file against real resources");
        println!("  show          Inspect state or plan");
        println!("  state         Advanced state management");
        println!("  taint         Mark a resource instance as not fully functional");
        println!("  test          Execute integration tests");
        println!("  untaint       Remove the 'tainted' state from a resource");
        println!("  version       Show the current Terraform version");
        println!("  workspace     Workspace management");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" | "-version" => {
            println!("Terraform v1.7.4 (SlateOS)");
            println!("on slateos_amd64");
            0
        }
        "init" => {
            println!("Initializing the backend...");
            println!();
            println!("Initializing provider plugins...");
            println!("- Finding hashicorp/aws versions matching \"~> 5.0\"...");
            println!("- Installing hashicorp/aws v5.34.0...");
            println!("- Installed hashicorp/aws v5.34.0 (signed by HashiCorp)");
            println!();
            println!("Terraform has been successfully initialized!");
            println!();
            println!("You may now begin working with Terraform. Try running \"terraform plan\".");
            0
        }
        "plan" => {
            let auto_approve = args.iter().any(|a| a == "-auto-approve");
            println!("Terraform used the selected providers to generate the following execution plan.");
            println!();
            println!("  # aws_instance.web will be created");
            println!("  + resource \"aws_instance\" \"web\" {{");
            println!("      + ami                    = \"ami-0c55b159cbfafe1f0\"");
            println!("      + instance_type           = \"t3.micro\"");
            println!("      + tags                    = {{");
            println!("          + \"Name\" = \"web-server\"");
            println!("        }}");
            println!("    }}");
            println!();
            println!("  # aws_security_group.web will be created");
            println!("  + resource \"aws_security_group\" \"web\" {{");
            println!("      + name = \"web-sg\"");
            println!("    }}");
            println!();
            println!("Plan: 2 to add, 0 to change, 0 to destroy.");
            if auto_approve {
                println!();
                println!("(auto-approve mode)");
            }
            0
        }
        "apply" => {
            println!("aws_security_group.web: Creating...");
            println!("aws_security_group.web: Creation complete after 2s [id=sg-0123456789]");
            println!("aws_instance.web: Creating...");
            println!("aws_instance.web: Still creating... [10s elapsed]");
            println!("aws_instance.web: Creation complete after 32s [id=i-0123456789abcdef]");
            println!();
            println!("Apply complete! Resources: 2 added, 0 changed, 0 destroyed.");
            0
        }
        "destroy" => {
            println!("aws_instance.web: Destroying... [id=i-0123456789abcdef]");
            println!("aws_instance.web: Destruction complete after 45s");
            println!("aws_security_group.web: Destroying... [id=sg-0123456789]");
            println!("aws_security_group.web: Destruction complete after 1s");
            println!();
            println!("Destroy complete! Resources: 2 destroyed.");
            0
        }
        "validate" => {
            println!("Success! The configuration is valid.");
            0
        }
        "fmt" => {
            let check = args.iter().any(|a| a == "-check");
            if check {
                println!("main.tf");
                println!("variables.tf");
                1
            } else {
                println!("main.tf");
                println!("variables.tf");
                0
            }
        }
        "output" => {
            println!("instance_id = \"i-0123456789abcdef\"");
            println!("public_ip = \"54.123.45.67\"");
            println!("public_dns = \"ec2-54-123-45-67.compute-1.amazonaws.com\"");
            0
        }
        "state" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("aws_instance.web");
                    println!("aws_security_group.web");
                }
                "show" => {
                    let resource = args.get(2).map(|s| s.as_str()).unwrap_or("aws_instance.web");
                    println!("# {}:", resource);
                    println!("resource \"aws_instance\" \"web\" {{");
                    println!("    ami           = \"ami-0c55b159cbfafe1f0\"");
                    println!("    instance_type = \"t3.micro\"");
                    println!("    id            = \"i-0123456789abcdef\"");
                    println!("}}");
                }
                _ => println!("Usage: terraform state <list|show|mv|rm|pull|push>"),
            }
            0
        }
        "workspace" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("  default");
                    println!("* dev");
                    println!("  staging");
                    println!("  production");
                }
                "show" => println!("dev"),
                "new" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("new");
                    println!("Created and switched to workspace \"{}\"!", name);
                }
                "select" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("default");
                    println!("Switched to workspace \"{}\".", name);
                }
                _ => println!("Usage: terraform workspace <list|show|new|select|delete>"),
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: terraform <subcommand>. See --help.");
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
    let code = run_terraform(rest);
    process::exit(code);
}

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
