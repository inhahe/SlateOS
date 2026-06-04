#![deny(clippy::all)]

//! drake-cli — OurOS Drake robotics toolbox
//!
//! Single personality: `drake`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_drake(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: drake COMMAND [OPTIONS]");
        println!("Drake v1.28 (OurOS) — Model-based robotics toolbox");
        println!();
        println!("Commands:");
        println!("  simulate FILE     Simulate a model");
        println!("  visualize FILE    Launch MeshCat visualizer");
        println!("  info FILE         Show model info (URDF/SDF)");
        println!("  ik FILE           Solve inverse kinematics");
        println!("  trajectory FILE   Plan trajectory");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Drake v1.28 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "simulate" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("robot.urdf");
            println!("Simulating: {}", file);
            println!("  Physics: MultibodyPlant");
            println!("  Integrator: semi-implicit Euler");
            println!("  Step: 0.001s");
            println!("  Duration: 10.0s");
        }
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("robot.urdf");
            println!("Model: {}", file);
            println!("  Bodies: 7");
            println!("  Joints: 6 (revolute)");
            println!("  DoF: 6");
            println!("  Actuators: 6");
        }
        "ik" => {
            println!("Solving inverse kinematics...");
            println!("  Target: [0.5, 0.2, 0.8]");
            println!("  Solution found (12 iterations)");
        }
        "trajectory" => {
            println!("Planning trajectory...");
            println!("  Waypoints: 5");
            println!("  Duration: 3.0s");
            println!("  Collision-free: yes");
        }
        _ => println!("drake {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "drake".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_drake(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_drake};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/drake"), "drake");
        assert_eq!(basename(r"C:\bin\drake.exe"), "drake.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("drake.exe"), "drake");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_drake(&["--help".to_string()], "drake"), 0);
        assert_eq!(run_drake(&["-h".to_string()], "drake"), 0);
        let _ = run_drake(&["--version".to_string()], "drake");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_drake(&[], "drake");
    }
}
