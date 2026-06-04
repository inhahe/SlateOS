#![deny(clippy::all)]

//! buildah — OurOS OCI image builder
//!
//! Single personality: `buildah`

use std::env;
use std::process;

fn run_buildah(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: buildah <command> [flags]");
        println!();
        println!("Commands:");
        println!("  bud, build    Build an image from a Containerfile");
        println!("  from          Create a working container");
        println!("  run           Run a command in container");
        println!("  copy          Copy content into container");
        println!("  add           Add content to container");
        println!("  commit        Create image from container");
        println!("  config        Update image configuration");
        println!("  push          Push image to registry");
        println!("  pull          Pull image from registry");
        println!("  images        List images");
        println!("  containers    List working containers");
        println!("  rm            Remove working container");
        println!("  rmi           Remove image");
        println!("  inspect       Inspect container/image");
        println!("  mount         Mount container rootfs");
        println!("  umount        Unmount container rootfs");
        println!("  tag           Add tag to image");
        println!("  unshare       Run in user namespace");
        println!("  version       Show version");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "version" => {
            println!("Version:         1.35.4 (OurOS)");
            println!("Go Version:      go1.22");
            println!("Image Spec:      1.1.0");
            println!("Runtime Spec:    1.2.0");
            println!("CNI Spec:        1.1.0");
            println!("libcni Version:  1.2.0");
            println!("Git Commit:      (simulated)");
            println!("Built:           Thu May 22 10:00:00 2025");
            println!("OS/Arch:         ouros/amd64");
        }
        "bud" | "build" => {
            let tag = args.iter().position(|a| a == "-t" || a == "--tag")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("localhost/image:latest");
            println!("STEP 1: FROM alpine:latest");
            println!("STEP 2: RUN apk add --no-cache python3");
            println!("STEP 3: COPY . /app");
            println!("STEP 4: COMMIT {}", tag);
            println!("Successfully tagged {}", tag);
        }
        "from" => {
            let base = args.get(1).map(|s| s.as_str()).unwrap_or("alpine:latest");
            println!("{}-working-container", base.split(':').next().unwrap_or("alpine"));
        }
        "images" => {
            println!("REPOSITORY          TAG      IMAGE ID       CREATED        SIZE");
            println!("localhost/myapp     latest   abc123def456   2 hours ago    45 MB");
            println!("docker.io/alpine    3.19     fed789abc012   3 days ago     7.4 MB");
        }
        "containers" => {
            println!("CONTAINER ID   BUILDER   IMAGE ID        IMAGE NAME                CONTAINER NAME");
            println!("abc123def456   *         fed789abc012    docker.io/alpine:3.19     alpine-working-container");
        }
        "inspect" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("container");
            println!("{{\"Type\":\"\",\"FromImage\":\"\",\"Container\":\"{}\"}}", target);
        }
        "push" | "pull" | "commit" | "tag" | "rm" | "rmi" | "run" | "copy" | "add" | "config" | "mount" | "umount" => {
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
