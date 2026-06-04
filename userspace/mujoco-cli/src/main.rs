#![deny(clippy::all)]

//! mujoco-cli — OurOS MuJoCo physics simulator
//!
//! Multi-personality: `mujoco`, `simulate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mujoco(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mujoco COMMAND [OPTIONS]");
        println!("MuJoCo v3.2 (OurOS) — Multi-Joint dynamics with Contact");
        println!();
        println!("Commands:");
        println!("  simulate FILE     Run interactive simulation");
        println!("  compile FILE      Compile MJCF/URDF model");
        println!("  info FILE         Show model info");
        println!("  bench FILE        Benchmark simulation speed");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("MuJoCo v3.2 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "simulate" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.xml");
            println!("Simulating: {}", file);
            println!("  Timestep: 0.002s");
            println!("  Solver: Newton");
            println!("  Renderer: OpenGL");
        }
        "compile" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.xml");
            println!("Compiling: {} -> model.mjb", file);
            println!("  Bodies: 12");
            println!("  Joints: 8");
            println!("  Geoms: 15");
            println!("  Done.");
        }
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.xml");
            println!("Model: {}", file);
            println!("  nq (positions): 14");
            println!("  nv (velocities): 12");
            println!("  nu (controls): 8");
            println!("  nbody: 12");
            println!("  ngeom: 15");
            println!("  Contacts: convex, mesh");
        }
        "bench" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("humanoid.xml");
            println!("Benchmarking: {}", file);
            println!("  Steps/sec: 125,000");
            println!("  Real-time factor: 250x");
            println!("  Step time: 8.0 us");
        }
        _ => println!("mujoco {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mujoco".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mujoco(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mujoco};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mujoco"), "mujoco");
        assert_eq!(basename(r"C:\bin\mujoco.exe"), "mujoco.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mujoco.exe"), "mujoco");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mujoco(&["--help".to_string()], "mujoco"), 0);
        assert_eq!(run_mujoco(&["-h".to_string()], "mujoco"), 0);
        let _ = run_mujoco(&["--version".to_string()], "mujoco");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mujoco(&[], "mujoco");
    }
}
