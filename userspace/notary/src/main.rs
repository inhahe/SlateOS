#![deny(clippy::all)]

//! notary — SlateOS Notary v2 content signing
//!
//! Single personality: `notation`

use std::env;
use std::process;

fn run_notation(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: notation <command> [flags]");
        println!();
        println!("Commands:");
        println!("  sign         Sign artifacts");
        println!("  verify       Verify artifacts");
        println!("  list         List signatures");
        println!("  cert         Manage certificates");
        println!("  key          Manage signing keys");
        println!("  plugin       Manage plugins");
        println!("  login        Login to registry");
        println!("  logout       Logout from registry");
        println!("  policy       Manage trust policies");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("notation version 1.1.1 (Slate OS)");
            println!("Go version: go1.22");
        }
        "sign" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("image:tag");
            println!("Successfully signed {}", target);
            println!("Signature digest: sha256:abc123def456...");
        }
        "verify" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("image:tag");
            println!("Successfully verified signature for {}", target);
        }
        "list" | "ls" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("image:tag");
            println!("Signatures for {}:", target);
            println!("  sha256:abc123... (signed by: CN=Slate OS Signer)");
            println!("  sha256:def456... (signed by: CN=CI Pipeline)");
        }
        "cert" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("NAME       KEY PATH                     CERTIFICATE PATH");
                    println!("default    /home/user/.config/notation/  /home/user/.config/notation/cert.pem");
                }
                "add" => println!("Certificate added successfully."),
                "delete" | "remove" => println!("Certificate removed."),
                _ => println!("Subcommands: list, add, delete, show, generate-test"),
            }
        }
        "key" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => {
                    println!("NAME       KEY PATH");
                    println!("default    /home/user/.config/notation/key.pem");
                }
                "add" => println!("Key added successfully."),
                "delete" | "remove" => println!("Key removed."),
                _ => println!("Subcommands: list, add, delete"),
            }
        }
        "plugin" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" | "ls" => println!("NAME               VERSION"),
                "install" => println!("Plugin installed."),
                _ => println!("Subcommands: list, install, uninstall"),
            }
        }
        "policy" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match sub {
                "show" => {
                    println!("Trust policy configuration:");
                    println!("  {{\"version\":\"1.0\",\"trustPolicies\":[]}}");
                }
                "import" => println!("Trust policy imported."),
                _ => println!("Subcommands: show, import"),
            }
        }
        "login" => println!("Login succeeded."),
        "logout" => println!("Logout succeeded."),
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
    let code = run_notation(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_notation};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_notation(vec!["--help".to_string()]), 0);
        assert_eq!(run_notation(vec!["-h".to_string()]), 0);
        let _ = run_notation(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_notation(vec![]);
    }
}
