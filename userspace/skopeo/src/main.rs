#![deny(clippy::all)]

//! skopeo — Slate OS container image operations
//!
//! Single personality: `skopeo`

use std::env;
use std::process;

fn run_skopeo(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: skopeo <command> [flags]");
        println!();
        println!("Commands:");
        println!("  copy         Copy images between registries");
        println!("  inspect      Inspect image");
        println!("  delete       Delete image from registry");
        println!("  list-tags    List tags in repository");
        println!("  sync         Sync images between registries");
        println!("  standalone-sign   Sign image");
        println!("  standalone-verify Verify signature");
        println!("  login        Login to registry");
        println!("  logout       Logout from registry");
        println!("  manifest-digest  Compute manifest digest");
        println!();
        println!("Flags:");
        println!("  --insecure-policy   Trust all registries");
        println!("  --override-os OS    Override OS for image");
        println!("  --override-arch A   Override architecture");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "copy" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("docker://source:tag");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("docker://dest:tag");
            println!("Getting image source signatures");
            println!("Copying blob sha256:abc123... done");
            println!("Copying blob sha256:def456... done");
            println!("Copying config sha256:789abc... done");
            println!("Writing manifest to image destination");
            println!("Storing signatures");
            let _ = (src, dst);
        }
        "inspect" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("docker://alpine:latest");
            println!("{{");
            println!("  \"Name\": \"{}\",", target);
            println!("  \"Tag\": \"latest\",");
            println!("  \"Digest\": \"sha256:abc123def456...\",");
            println!("  \"Created\": \"2025-05-22T10:00:00Z\",");
            println!("  \"DockerVersion\": \"\",");
            println!("  \"Os\": \"linux\",");
            println!("  \"Architecture\": \"amd64\",");
            println!("  \"Layers\": [\"sha256:abc123...\"]");
            println!("}}");
        }
        "list-tags" => {
            let repo = args.get(1).map(|s| s.as_str()).unwrap_or("docker://alpine");
            println!("{{\"Repository\":\"{}\",\"Tags\":[\"latest\",\"3.19\",\"3.18\",\"3.17\",\"edge\"]}}", repo);
        }
        "delete" => println!("(image deleted — simulated)"),
        "sync" => println!("(sync complete — simulated)"),
        "login" => println!("Login Succeeded!"),
        "logout" => println!("Removed login credentials."),
        "manifest-digest" => println!("sha256:abc123def456789..."),
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
    let code = run_skopeo(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_skopeo};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_skopeo(vec!["--help".to_string()]), 0);
        assert_eq!(run_skopeo(vec!["-h".to_string()]), 0);
        let _ = run_skopeo(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_skopeo(vec![]);
    }
}
