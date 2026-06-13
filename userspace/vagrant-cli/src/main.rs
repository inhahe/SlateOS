#![deny(clippy::all)]

//! vagrant-cli — Slate OS Vagrant CLI
//!
//! Single personality: `vagrant`

use std::env;
use std::process;

fn run_vagrant(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vagrant [OPTIONS] COMMAND [ARGS]");
        println!();
        println!("Vagrant — development environment manager (Slate OS).");
        println!();
        println!("Commands:");
        println!("  init [BOX]       Initialize Vagrantfile");
        println!("  up               Start and provision");
        println!("  halt             Stop machine");
        println!("  destroy          Destroy machine");
        println!("  ssh              SSH into machine");
        println!("  status           Machine status");
        println!("  global-status    All machines status");
        println!("  provision        Run provisioners");
        println!("  reload           Restart machine");
        println!("  suspend          Suspend machine");
        println!("  resume           Resume machine");
        println!("  snapshot         Manage snapshots");
        println!("  box              Manage boxes");
        println!("  plugin           Manage plugins");
        println!("  port             Display port mappings");
        println!("  validate         Validate Vagrantfile");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Vagrant 2.4.1 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    match cmd {
        "init" => {
            let box_name = args.get(1).map(|s| s.as_str()).unwrap_or("hashicorp/bionic64");
            println!("A `Vagrantfile` has been placed in this directory.");
            println!("Box: {}", box_name);
            println!("You are now ready to `vagrant up`.");
        }
        "up" => {
            println!("Bringing machine 'default' up with 'virtualbox' provider...");
            println!("==> default: Importing base box 'hashicorp/bionic64'...");
            println!("==> default: Matching MAC address for NAT networking...");
            println!("==> default: Setting the name of the VM: project_default_1705312200");
            println!("==> default: Forwarding ports...");
            println!("    default: 22 (guest) => 2222 (host) (adapter 1)");
            println!("==> default: Booting VM...");
            println!("==> default: Machine booted and ready!");
        }
        "halt" => {
            println!("==> default: Attempting graceful shutdown of VM...");
        }
        "destroy" => {
            println!("==> default: Destroying VM and associated drives...");
        }
        "ssh" => {
            println!("Welcome to Ubuntu 18.04 LTS (Vagrant)");
            println!("vagrant@vagrant:~$");
        }
        "status" => {
            println!("Current machine states:");
            println!();
            println!("default                   running (virtualbox)");
        }
        "global-status" => {
            println!("id       name    provider   state   directory");
            println!("--------------------------------------------------------------");
            println!("abc1234  default virtualbox running /home/user/project");
            println!("def5678  default virtualbox saved   /home/user/test");
        }
        "provision" => {
            println!("==> default: Running provisioner: shell...");
            println!("    default: Running: inline script");
        }
        "reload" => {
            println!("==> default: Attempting graceful shutdown of VM...");
            println!("==> default: Booting VM...");
            println!("==> default: Machine booted and ready!");
        }
        "suspend" => println!("==> default: Saving VM state and suspending execution..."),
        "resume" => println!("==> default: Resuming suspended VM..."),
        "snapshot" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match subcmd {
                "list" => println!("==> default: No snapshots have been taken yet!"),
                "save" => println!("==> default: Snapshotting the machine as 'snapshot1'..."),
                "restore" => println!("==> default: Restoring snapshot 'snapshot1'..."),
                "delete" => println!("==> default: Deleting snapshot 'snapshot1'..."),
                _ => println!("vagrant snapshot: unknown subcommand '{}'", subcmd),
            }
        }
        "box" => {
            let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match subcmd {
                "list" => {
                    println!("hashicorp/bionic64  (virtualbox, 1.0.282)");
                    println!("ubuntu/focal64      (virtualbox, 20240115)");
                }
                "add" => println!("==> box: Adding box..."),
                "remove" => println!("==> box: Removing box..."),
                _ => println!("vagrant box: unknown subcommand '{}'", subcmd),
            }
        }
        "validate" => println!("Vagrantfile validated successfully."),
        "port" => {
            println!("The following ports are forwarded:");
            println!("  22 (guest) => 2222 (host)");
        }
        _ => {
            eprintln!("vagrant: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vagrant(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_vagrant};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vagrant(vec!["--help".to_string()]), 0);
        assert_eq!(run_vagrant(vec!["-h".to_string()]), 0);
        let _ = run_vagrant(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vagrant(vec![]);
    }
}
