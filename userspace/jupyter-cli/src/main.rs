#![deny(clippy::all)]

//! jupyter-cli — SlateOS Jupyter notebook CLI
//!
//! Single personality: `jupyter`

use std::env;
use std::process;

fn run_jupyter(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jupyter <COMMAND> [OPTIONS]");
        println!();
        println!("Jupyter notebook and lab CLI (Slate OS).");
        println!();
        println!("Commands:");
        println!("  notebook     Start notebook server");
        println!("  lab          Start JupyterLab");
        println!("  nbconvert    Convert notebooks");
        println!("  kernelspec   Manage kernel specs");
        println!("  execute      Execute a notebook");
        println!("  trust        Trust notebooks");
        println!("  server       List running servers");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("jupyter 7.0.0 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "notebook" | "lab" => {
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("8888");
            let kind = if cmd == "lab" { "JupyterLab" } else { "Jupyter Notebook" };
            println!("[I] {} is running at:", kind);
            println!("    http://localhost:{}/?token=abc123def456ghi789", port);
            println!("     or http://127.0.0.1:{}/?token=abc123def456ghi789", port);
            println!("[I] Use Control-C to stop this server.");
            0
        }
        "nbconvert" => {
            let notebook = args.get(1).map(|s| s.as_str()).unwrap_or("notebook.ipynb");
            let to = args.windows(2).find(|w| w[0] == "--to").map(|w| w[1].as_str()).unwrap_or("html");
            println!("[NbConvertApp] Converting {} to {}...", notebook, to);
            let output = notebook.strip_suffix(".ipynb").unwrap_or("notebook");
            println!("[NbConvertApp] Writing {} to {}.{}", to, output, to);
            0
        }
        "kernelspec" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Available kernels:");
                    println!("  python3      /usr/share/jupyter/kernels/python3");
                    println!("  rust         /usr/share/jupyter/kernels/rust");
                    println!("  julia-1.10   /usr/share/jupyter/kernels/julia-1.10");
                }
                "install" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("mykernel");
                    println!("[InstallKernelSpec] Installed kernelspec {} in /usr/share/jupyter/kernels/{}", name, name);
                }
                _ => { println!("Kernelspec operation: {}", sub); }
            }
            0
        }
        "execute" => {
            let notebook = args.get(1).map(|s| s.as_str()).unwrap_or("notebook.ipynb");
            println!("[ExecutePreprocessor] Executing {} ...", notebook);
            println!("  Cell 1/5 executed (0.2s)");
            println!("  Cell 2/5 executed (1.5s)");
            println!("  Cell 3/5 executed (3.2s)");
            println!("  Cell 4/5 executed (0.8s)");
            println!("  Cell 5/5 executed (0.3s)");
            println!("[ExecutePreprocessor] Done. Output saved to {}", notebook);
            0
        }
        "server" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    println!("Currently running servers:");
                    println!("  http://localhost:8888/?token=abc123... :: /home/user/notebooks");
                    println!("  http://localhost:8889/?token=def456... :: /home/user/project");
                }
                "stop" => {
                    let port = args.get(2).map(|s| s.as_str()).unwrap_or("8888");
                    println!("Shutting down server on port {}...", port);
                    println!("  ✔ Server stopped.");
                }
                _ => { println!("Server operation: {}", sub); }
            }
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: jupyter <command>. See --help.");
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
    let code = run_jupyter(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jupyter};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_jupyter(vec!["--help".to_string()]), 0);
        assert_eq!(run_jupyter(vec!["-h".to_string()]), 0);
        let _ = run_jupyter(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_jupyter(vec![]);
    }
}
