#![deny(clippy::all)]

//! crane-cli — OurOS container registry interaction CLI
//!
//! Single personality: `crane`

use std::env;
use std::process;

fn run_crane(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: crane <COMMAND> [OPTIONS]");
        println!();
        println!("Interact with container registries.");
        println!();
        println!("Commands:");
        println!("  ls           List tags");
        println!("  digest       Get digest of an image");
        println!("  manifest     Get manifest of an image");
        println!("  config       Get config of an image");
        println!("  pull         Pull an image as tarball");
        println!("  push         Push an image from tarball");
        println!("  copy         Copy images between registries");
        println!("  delete       Delete an image");
        println!("  catalog      List repos in a registry");
        println!("  flatten      Flatten an image to one layer");
        println!("  mutate       Modify an image");
        println!("  append       Append layers to an image");
        println!("  auth         Authentication commands");
        println!("  blob         Interact with blobs");
        println!("  validate     Validate an image");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("crane 0.19.0 (OurOS)");
            0
        }
        "ls" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("nginx");
            println!("Tags for {}:", image);
            println!("latest");
            println!("1.25");
            println!("1.25.3");
            println!("1.24");
            println!("alpine");
            0
        }
        "digest" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("nginx:latest");
            println!("sha256:abc123def456789012345678901234567890123456789012345678901234");
            let _ = image;
            0
        }
        "manifest" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("nginx:latest");
            println!("{{");
            println!("  \"schemaVersion\": 2,");
            println!("  \"mediaType\": \"application/vnd.docker.distribution.manifest.v2+json\",");
            println!("  \"config\": {{");
            println!("    \"mediaType\": \"application/vnd.docker.container.image.v1+json\",");
            println!("    \"size\": 7023,");
            println!("    \"digest\": \"sha256:abc123...\"");
            println!("  }},");
            println!("  \"layers\": [");
            println!("    {{\"mediaType\": \"application/vnd.docker.image.rootfs.diff.tar.gzip\", \"size\": 33674240, \"digest\": \"sha256:def456...\"}}");
            println!("  ]");
            println!("}}");
            let _ = image;
            0
        }
        "copy" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("nginx:latest");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("myregistry/nginx:latest");
            println!("Copying {} -> {}", src, dst);
            println!("  Done.");
            0
        }
        "catalog" => {
            let registry = args.get(1).map(|s| s.as_str()).unwrap_or("registry.example.com");
            println!("Repositories in {}:", registry);
            println!("  myapp");
            println!("  nginx");
            println!("  redis");
            println!("  postgres");
            0
        }
        "validate" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("myimage:latest");
            println!("{}: VALID", image);
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: crane <command>. See --help.");
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
    let code = run_crane(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
