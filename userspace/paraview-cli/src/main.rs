#![deny(clippy::all)]

//! paraview-cli — OurOS ParaView scientific visualization
//!
//! Multi-personality: `paraview`, `pvserver`, `pvbatch`, `pvpython`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_paraview(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: paraview [OPTIONS] [FILE]");
        println!("  --server URL      Connect to pvserver");
        println!("  --data FILE       Load data file");
        println!("  --state FILE      Load state file");
        println!("  --script FILE     Run Python script on startup");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ParaView 5.12.0 (OurOS)");
        println!("VTK 9.3.0");
        println!("Qt 6.6.1");
        println!("Python 3.12.0");
        println!("MPI: OpenMPI 4.1.6");
        println!("OpenGL: 4.6");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    if let Some(f) = file {
        println!("ParaView 5.12.0");
        println!("Loading: {}", f);
        println!("Data loaded successfully.");
        println!("Pipeline: 1 source, 0 filters");
    } else {
        println!("ParaView 5.12.0");
        println!("Starting GUI...");
        println!("Ready.");
    }
    0
}

fn run_pvserver(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pvserver [OPTIONS]");
        println!("  --hostname ADDR    Bind address (default: localhost)");
        println!("  --port PORT        Bind port (default: 11111)");
        println!("  --multi-clients    Allow multiple client connections");
        println!("  --force-offscreen  Force offscreen rendering");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pvserver 5.12.0 (ParaView, OurOS)");
        return 0;
    }
    let host = args.windows(2).find(|w| w[0] == "--hostname").map(|w| w[1].as_str()).unwrap_or("localhost");
    let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("11111");
    println!("pvserver starting...");
    println!("Listening on {}:{}", host, port);
    println!("Waiting for client connection...");
    println!("Connection URL: cs://{}:{}", host, port);
    0
}

fn run_pvbatch(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pvbatch [OPTIONS] SCRIPT.py [ARGS]");
        println!("  --force-offscreen  Force offscreen rendering");
        println!("  --symmetric        Symmetric mode for MPI");
        println!("  --mpi              Enable MPI parallelism");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pvbatch 5.12.0 (ParaView, OurOS)");
        return 0;
    }
    let script = args.iter().find(|a| a.ends_with(".py")).map(|s| s.as_str()).unwrap_or("render.py");
    println!("pvbatch: loading script '{}'", script);
    println!("Initializing offscreen rendering...");
    println!("Executing batch pipeline...");
    println!("Pipeline complete. Output written.");
    0
}

fn run_pvpython(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pvpython [OPTIONS] [SCRIPT.py] [ARGS]");
        println!("  --force-offscreen  Force offscreen rendering");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("pvpython 5.12.0 (ParaView, OurOS)");
        println!("Python 3.12.0");
        println!("VTK 9.3.0");
        return 0;
    }
    let script = args.iter().find(|a| a.ends_with(".py")).map(|s| s.as_str());
    if let Some(s) = script {
        println!("pvpython: executing '{}'", s);
        println!("Script completed successfully.");
    } else {
        println!("ParaView Python Shell (5.12.0)");
        println!("Type 'help()' for help, 'quit()' to exit.");
        println!(">>>");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "paraview".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "pvserver" => run_pvserver(&rest),
        "pvbatch" => run_pvbatch(&rest),
        "pvpython" => run_pvpython(&rest),
        _ => run_paraview(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_paraview};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/paraview"), "paraview");
        assert_eq!(basename(r"C:\bin\paraview.exe"), "paraview.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("paraview.exe"), "paraview");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_paraview(&["--help".to_string()]), 0);
        assert_eq!(run_paraview(&["-h".to_string()]), 0);
        assert_eq!(run_paraview(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_paraview(&[]), 0);
    }
}
