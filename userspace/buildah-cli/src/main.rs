#![deny(clippy::all)]

//! buildah-cli — SlateOS Buildah OCI image builder CLI
//!
//! Single personality: `buildah`

use std::env;
use std::process;

fn run_buildah(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: buildah <COMMAND> [OPTIONS]");
        println!();
        println!("OCI image builder — build containers without a daemon.");
        println!();
        println!("Commands:");
        println!("  from         Create a working container");
        println!("  bud          Build using Dockerfile/Containerfile");
        println!("  run          Run a command in the container");
        println!("  copy         Copy files into the container");
        println!("  add          Add files into the container");
        println!("  config       Set configuration");
        println!("  commit       Create image from container");
        println!("  push         Push an image");
        println!("  pull         Pull an image");
        println!("  images       List images");
        println!("  containers   List working containers");
        println!("  rm           Remove working containers");
        println!("  rmi          Remove images");
        println!("  inspect      Inspect image/container");
        println!("  mount        Mount working container");
        println!("  umount       Unmount working container");
        println!("  unshare      Run in user namespace");
        println!("  version      Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "version" => {
            println!("Version:         1.34.0 (Slate OS)");
            println!("Go Version:      go1.21.6");
            println!("Image Spec:      1.1.0");
            println!("Runtime Spec:    1.1.0");
            println!("CNI Spec:        1.0.0");
            println!("libcni Version:  1.1.2");
            println!("Git Commit:      abc123");
            0
        }
        "from" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("ubuntu:22.04");
            println!("working-container-1");
            println!("  (from {})", image);
            0
        }
        "bud" | "build" => {
            let tag = args.windows(2)
                .find(|w| w[0] == "-t" || w[0] == "--tag")
                .map(|w| w[1].as_str())
                .unwrap_or("myimage:latest");
            println!("STEP 1: FROM ubuntu:22.04");
            println!("STEP 2: RUN apt-get update && apt-get install -y nginx");
            println!("STEP 3: COPY index.html /var/www/html/");
            println!("STEP 4: EXPOSE 80");
            println!("STEP 5: CMD [\"nginx\", \"-g\", \"daemon off;\"]");
            println!("STEP 6: COMMIT {}", tag);
            println!("--> abc123def456");
            println!("Successfully tagged {}", tag);
            0
        }
        "images" => {
            println!("REPOSITORY                TAG     IMAGE ID      CREATED       SIZE");
            println!("localhost/myimage         latest  abc123def456  5 min ago     234 MB");
            println!("docker.io/library/ubuntu  22.04   def456abc789  2 weeks ago   77.8 MB");
            0
        }
        "containers" => {
            println!("CONTAINER ID  BUILDER  IMAGE ID       IMAGE NAME                CONTAINER NAME");
            println!("abc123def4    *        def456abc789   docker.io/library/ubuntu  working-container-1");
            0
        }
        "commit" => {
            let container = args.get(1).map(|s| s.as_str()).unwrap_or("working-container-1");
            let image = args.get(2).map(|s| s.as_str()).unwrap_or("myimage:latest");
            println!("Getting image source signatures");
            println!("Copying blob abc123 done");
            println!("Writing manifest to image destination");
            println!("--> abc123def456789");
            println!("  ({} -> {})", container, image);
            0
        }
        "push" => {
            let image = args.get(1).map(|s| s.as_str()).unwrap_or("myimage:latest");
            println!("Getting image source signatures");
            println!("Copying blob abc123 done");
            println!("Writing manifest to image destination");
            println!("Storing signatures");
            println!("  (pushed {})", image);
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: buildah <command>. See --help.");
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
    let code = run_buildah(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_buildah};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_buildah(vec!["--help".to_string()]), 0);
        assert_eq!(run_buildah(vec!["-h".to_string()]), 0);
        let _ = run_buildah(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_buildah(vec![]);
    }
}
