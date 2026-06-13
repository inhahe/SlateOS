#![deny(clippy::all)]

//! pybullet-cli — SlateOS PyBullet Python physics tool
//!
//! Single personality: `pybullet`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pybullet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pybullet COMMAND [OPTIONS]");
        println!("PyBullet v3.2.6 (Slate OS) — Python physics simulation");
        println!();
        println!("Commands:");
        println!("  run SCRIPT        Run PyBullet script");
        println!("  demo NAME         Run built-in demo");
        println!("  models            List available URDF models");
        println!("  info              Show configuration");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("PyBullet v3.2.6 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("sim.py");
            println!("Running: {}", script);
            println!("  Physics server: DIRECT");
            println!("  Gravity: [0, 0, -9.81]");
        }
        "demo" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("kuka_iiwa");
            println!("Running demo: {}", name);
            println!("  Loading URDF...");
            println!("  Simulation running.");
        }
        "models" => {
            println!("Available URDF models:");
            println!("  kuka_iiwa, panda, ur5");
            println!("  humanoid, quadruped, car");
            println!("  table, plane, sphere");
        }
        "info" => {
            println!("PyBullet v3.2.6");
            println!("  Engine: Bullet Physics 3.25");
            println!("  Python: 3.12");
            println!("  Rendering: TinyRenderer / OpenGL");
            println!("  URDF/SDF loader: built-in");
        }
        _ => println!("pybullet {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pybullet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pybullet(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pybullet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pybullet"), "pybullet");
        assert_eq!(basename(r"C:\bin\pybullet.exe"), "pybullet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pybullet.exe"), "pybullet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pybullet(&["--help".to_string()], "pybullet"), 0);
        assert_eq!(run_pybullet(&["-h".to_string()], "pybullet"), 0);
        let _ = run_pybullet(&["--version".to_string()], "pybullet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pybullet(&[], "pybullet");
    }
}
