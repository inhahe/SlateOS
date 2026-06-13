#![deny(clippy::all)]

//! docker — Slate OS container management tool
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `docker` (default) — container management CLI
//! - `dockerd` — Docker daemon
//! - `docker-compose` — multi-container orchestration

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct _Container {
    id: String,
    image: String,
    command: String,
    status: String,
    name: String,
    _ports: String,
}

fn _sample_containers() -> Vec<_Container> {
    vec![
        _Container { id: "a1b2c3d4e5f6".to_string(), image: "nginx:latest".to_string(),
            command: "nginx -g 'daemon off;'".to_string(), status: "Up 2 hours".to_string(),
            name: "web-server".to_string(), _ports: "0.0.0.0:80->80/tcp".to_string() },
        _Container { id: "f6e5d4c3b2a1".to_string(), image: "postgres:16".to_string(),
            command: "postgres".to_string(), status: "Up 2 hours".to_string(),
            name: "database".to_string(), _ports: "5432/tcp".to_string() },
        _Container { id: "1234abcd5678".to_string(), image: "redis:7".to_string(),
            command: "redis-server".to_string(), status: "Exited (0) 1 hour ago".to_string(),
            name: "cache".to_string(), _ports: "".to_string() },
    ]
}

#[derive(Clone, Debug)]
struct _Image {
    repository: String,
    tag: String,
    id: String,
    size: String,
}

fn _sample_images() -> Vec<_Image> {
    vec![
        _Image { repository: "nginx".to_string(), tag: "latest".to_string(), id: "abc123def456".to_string(), size: "187MB".to_string() },
        _Image { repository: "postgres".to_string(), tag: "16".to_string(), id: "def456abc789".to_string(), size: "425MB".to_string() },
        _Image { repository: "redis".to_string(), tag: "7".to_string(), id: "789abc123def".to_string(), size: "138MB".to_string() },
        _Image { repository: "node".to_string(), tag: "22-slim".to_string(), id: "456def789abc".to_string(), size: "245MB".to_string() },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_docker(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: docker [OPTIONS] COMMAND");
            println!();
            println!("Management Commands:");
            println!("  container   Manage containers");
            println!("  image       Manage images");
            println!("  network     Manage networks");
            println!("  volume      Manage volumes");
            println!("  system      Manage Docker");
            println!();
            println!("Commands:");
            println!("  run         Create and run a container");
            println!("  exec        Execute command in container");
            println!("  ps          List containers");
            println!("  build       Build an image from Dockerfile");
            println!("  pull        Download an image");
            println!("  push        Upload an image");
            println!("  images      List images");
            println!("  logs        Fetch container logs");
            println!("  stop        Stop containers");
            println!("  start       Start containers");
            println!("  restart     Restart containers");
            println!("  rm          Remove containers");
            println!("  rmi         Remove images");
            println!("  inspect     Return low-level info");
            println!("  tag         Create a tag");
            println!("  login       Log in to a registry");
            println!("  logout      Log out from a registry");
            println!("  cp          Copy files between container and host");
            println!("  stats       Display container resource usage");
            println!("  top         Display running processes");
            println!("  --version   Show version");
            0
        }
        "--version" | "version" => {
            println!("Docker version 26.0.0, build abcdef0 (Slate OS)");
            0
        }
        "run" => {
            let detach = cmd_args.iter().any(|a| a == "-d" || a == "--detach");
            let image = cmd_args.iter().rfind(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("ubuntu:latest");
            if detach {
                println!("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2");
            } else {
                println!("Unable to find image '{}' locally", image);
                println!("latest: Pulling from library/{}", image.split(':').next().unwrap_or(image));
                println!("Digest: sha256:abcdef1234567890...");
                println!("Status: Downloaded newer image for {}", image);
                println!("(container started — simulated)");
            }
            0
        }
        "ps" => {
            let show_all = cmd_args.iter().any(|a| a == "-a" || a == "--all");
            let containers = _sample_containers();
            println!("CONTAINER ID   IMAGE           COMMAND                  STATUS              NAMES");
            for c in &containers {
                if !show_all && c.status.starts_with("Exited") { continue; }
                println!("{:<14} {:<15} {:?}{:<5} {:<19} {}",
                    c.id, c.image,
                    if c.command.len() > 20 { &c.command[..20] } else { &c.command },
                    "", c.status, c.name);
            }
            0
        }
        "images" => {
            let images = _sample_images();
            println!("REPOSITORY   TAG        IMAGE ID       SIZE");
            for img in &images {
                println!("{:<12} {:<10} {:<14} {}", img.repository, img.tag, img.id, img.size);
            }
            0
        }
        "build" => {
            let tag = cmd_args.iter().position(|a| a == "-t" || a == "--tag")
                .and_then(|i| cmd_args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("myapp:latest");
            println!("[+] Building {}", tag);
            println!(" => [internal] load build definition from Dockerfile");
            println!(" => [1/4] FROM ubuntu:22.04");
            println!(" => [2/4] RUN apt-get update");
            println!(" => [3/4] COPY . /app");
            println!(" => [4/4] RUN make install");
            println!(" => exporting to image");
            println!(" => => naming to docker.io/library/{}", tag);
            0
        }
        "pull" => {
            let image = cmd_args.first().map(|s| s.as_str()).unwrap_or("ubuntu:latest");
            println!("Using default tag: latest");
            println!("latest: Pulling from library/{}", image.split(':').next().unwrap_or(image));
            println!("a1b2c3d4: Pull complete");
            println!("e5f6a7b8: Pull complete");
            println!("Digest: sha256:abcdef1234567890...");
            println!("Status: Downloaded newer image for {}", image);
            0
        }
        "stop" => {
            for name in &cmd_args {
                if !name.starts_with('-') { println!("{}", name); }
            }
            0
        }
        "start" | "restart" => {
            for name in &cmd_args {
                if !name.starts_with('-') { println!("{}", name); }
            }
            0
        }
        "rm" => {
            for name in &cmd_args {
                if !name.starts_with('-') { println!("{}", name); }
            }
            0
        }
        "rmi" => {
            for name in &cmd_args {
                if !name.starts_with('-') {
                    println!("Untagged: {}", name);
                    println!("Deleted: sha256:abc123... (simulated)");
                }
            }
            0
        }
        "logs" => {
            let name = cmd_args.first().map(|s| s.as_str()).unwrap_or("container");
            println!("[{}] 2025-05-22T10:00:00Z Starting application...", name);
            println!("[{}] 2025-05-22T10:00:01Z Listening on port 8080", name);
            println!("[{}] 2025-05-22T10:00:05Z GET / 200 0.5ms", name);
            0
        }
        "exec" => {
            println!("(executing command in container — simulated)");
            0
        }
        "inspect" => {
            let target = cmd_args.first().map(|s| s.as_str()).unwrap_or("container");
            println!("[{{");
            println!("  \"Id\": \"a1b2c3d4...\",");
            println!("  \"Name\": \"/{}\",", target);
            println!("  \"State\": {{\"Status\": \"running\", \"Running\": true}},");
            println!("  \"Image\": \"sha256:abc123...\"");
            println!("}}]");
            0
        }
        "stats" => {
            println!("CONTAINER ID   NAME         CPU %   MEM USAGE / LIMIT     MEM %   NET I/O          BLOCK I/O");
            println!("a1b2c3d4e5f6   web-server   0.15%   25.5MiB / 2GiB        1.24%   1.2kB / 648B     0B / 0B");
            println!("f6e5d4c3b2a1   database     1.20%   128MiB / 2GiB         6.25%   856B / 432B      4.1MB / 12.3MB");
            0
        }
        "top" => {
            println!("UID    PID    PPID   CMD");
            println!("root   1      0      nginx: master process");
            println!("www    15     1      nginx: worker process");
            0
        }
        "network" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" | "list" => {
                    println!("NETWORK ID     NAME      DRIVER    SCOPE");
                    println!("abc123def456   bridge    bridge    local");
                    println!("def456abc789   host      host      local");
                    println!("789abc123def   none      null      local");
                }
                "create" => println!("Network created (simulated)"),
                "rm" => println!("Network removed (simulated)"),
                _ => println!("network {}: (simulated)", sub),
            }
            0
        }
        "volume" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("ls");
            match sub {
                "ls" | "list" => {
                    println!("DRIVER    VOLUME NAME");
                    println!("local     pgdata");
                    println!("local     redis-data");
                }
                "create" => println!("Volume created (simulated)"),
                "rm" => println!("Volume removed (simulated)"),
                "prune" => println!("Deleted 2 volumes. Reclaimed 1.2GB (simulated)"),
                _ => println!("volume {}: (simulated)", sub),
            }
            0
        }
        "system" => {
            let sub = cmd_args.first().map(|s| s.as_str()).unwrap_or("info");
            match sub {
                "info" => {
                    println!("Client: Docker Engine - Slate OS");
                    println!(" Version:    26.0.0");
                    println!(" OS/Arch:    slateos/amd64");
                    println!("Server:");
                    println!(" Containers: 3 (2 running, 1 stopped)");
                    println!(" Images:     4");
                    println!(" Storage Driver: overlay2");
                }
                "prune" => {
                    println!("Deleted Containers: 1");
                    println!("Deleted Images: 2");
                    println!("Total reclaimed space: 512MB (simulated)");
                }
                "df" => {
                    println!("TYPE            TOTAL   ACTIVE  SIZE      RECLAIMABLE");
                    println!("Images          4       2       995.0MB   550.0MB (55%)");
                    println!("Containers      3       2       12.5MB    0B (0%)");
                    println!("Local Volumes   2       2       256.0MB   0B (0%)");
                }
                _ => println!("system {}: (simulated)", sub),
            }
            0
        }
        "login" => { println!("Login Succeeded (simulated)"); 0 }
        "logout" => { println!("Removing login credentials (simulated)"); 0 }
        "tag" => { println!("Tagged (simulated)"); 0 }
        "push" => {
            let image = cmd_args.first().map(|s| s.as_str()).unwrap_or("myapp:latest");
            println!("The push refers to repository [docker.io/library/{}]", image);
            println!("a1b2c3d4: Pushed");
            println!("{}: digest: sha256:abcdef... size: 1234", image);
            0
        }
        other => { eprintln!("docker: '{}' is not a docker command.", other); 1 }
    }
}

fn run_compose(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: docker-compose [OPTIONS] COMMAND");
            println!();
            println!("Commands:");
            println!("  up        Create and start containers");
            println!("  down      Stop and remove containers");
            println!("  build     Build or rebuild services");
            println!("  ps        List containers");
            println!("  logs      View output from containers");
            println!("  exec      Execute a command in a running container");
            println!("  config    Validate and view the Compose file");
            println!("  --version Show version");
            0
        }
        "--version" | "version" => { println!("Docker Compose version v2.26.0 (Slate OS)"); 0 }
        "up" => {
            println!("[+] Running 3/3");
            println!(" ✔ Container cache       Started");
            println!(" ✔ Container database     Started");
            println!(" ✔ Container web-server   Started");
            0
        }
        "down" => {
            println!("[+] Running 3/3");
            println!(" ✔ Container web-server   Removed");
            println!(" ✔ Container database     Removed");
            println!(" ✔ Container cache        Removed");
            println!(" ✔ Network default        Removed");
            0
        }
        "ps" => {
            println!("NAME          SERVICE     STATUS    PORTS");
            println!("web-server    web         running   0.0.0.0:80->80/tcp");
            println!("database      db          running   5432/tcp");
            println!("cache         cache       exited");
            0
        }
        "logs" => { println!("Attaching to web-server, database, cache"); println!("web-server | Listening on :80"); 0 }
        "config" => { println!("name: myproject"); println!("services:"); println!("  web:"); println!("    image: nginx:latest"); 0 }
        "build" => { println!("Building web... done (simulated)"); 0 }
        other => { eprintln!("docker-compose: '{}' is not a command.", other); 1 }
    }
}

fn run_dockerd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: dockerd [OPTIONS]");
        println!();
        println!("Docker daemon.");
        println!();
        println!("Options:");
        println!("  --data-root string   Root directory of runtime state (default /var/lib/docker)");
        println!("  --debug              Enable debug mode");
        println!("  --host list          Daemon socket(s) to connect to");
        println!("  --storage-driver     Storage driver to use");
        println!("  --version            Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("Docker version 26.0.0 (Slate OS)");
        return 0;
    }

    println!("INFO[0000] Starting up (Slate OS)");
    println!("INFO[0000] containerd not running, starting managed containerd");
    println!("INFO[0001] Loading containers: start.");
    println!("INFO[0001] Daemon has completed initialization");
    println!("INFO[0001] API listen on /var/run/docker.sock");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("docker");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "docker-compose" | "docker_compose" => run_compose(rest),
        "dockerd" => run_dockerd(rest),
        _ => run_docker(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_containers() {
        let containers = _sample_containers();
        assert_eq!(containers.len(), 3);
        assert_eq!(containers[0].name, "web-server");
    }

    #[test]
    fn test_sample_images() {
        let images = _sample_images();
        assert_eq!(images.len(), 4);
        assert_eq!(images[0].repository, "nginx");
    }
}
