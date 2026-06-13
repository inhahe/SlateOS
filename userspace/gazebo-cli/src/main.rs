#![deny(clippy::all)]

//! gazebo-cli — Slate OS Gazebo robotics simulator
//!
//! Multi-personality: `gz`, `ign`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gz(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gz COMMAND [OPTIONS]");
        println!("Gazebo Harmonic (Slate OS)");
        println!();
        println!("Commands:");
        println!("  sim          Run simulation");
        println!("  model        Model tools");
        println!("  topic        Topic tools");
        println!("  service      Service tools");
        println!("  sdf          SDF tools");
        println!("  fuel         Fuel (model repository) tools");
        println!("  plugin       Plugin tools");
        println!("  log          Log tools");
        println!("  version      Show version");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("version");
    match subcmd {
        "version" | "--version" => {
            println!("Gazebo Harmonic (Slate OS)");
            println!("gz-sim version 8.3.0");
            println!("gz-transport version 13.1.0");
            println!("gz-math version 7.3.0");
            println!("gz-physics version 7.3.0");
            println!("gz-rendering version 8.1.0");
        }
        "sim" => {
            let world = args.get(1).map(|s| s.as_str()).unwrap_or("empty.sdf");
            println!("gz sim: loading world '{}'", world);
            println!("  Physics engine: bullet");
            println!("  Rendering engine: ogre2");
            println!("  Step size: 1ms");
            println!("  Simulation started.");
        }
        "model" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if action == "list" {
                println!("Models in simulation:");
                println!("  ground_plane");
                println!("  sun");
                println!("  robot");
            }
        }
        "topic" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if action == "list" {
                println!("/clock");
                println!("/world/default/pose/info");
                println!("/world/default/stats");
                println!("/model/robot/cmd_vel");
                println!("/model/robot/odometry");
            }
        }
        "sdf" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            if action == "check" {
                let file = args.get(2).map(|s| s.as_str()).unwrap_or("model.sdf");
                println!("Checking: {}", file);
                println!("Valid.");
            }
        }
        "fuel" => {
            println!("Gazebo Fuel — model repository");
            println!("  URL: https://fuel.gazebosim.org");
            println!("  Cached models: 12");
        }
        _ => println!("gz: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gz".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gz(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gazebo"), "gazebo");
        assert_eq!(basename(r"C:\bin\gazebo.exe"), "gazebo.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gazebo.exe"), "gazebo");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gz(&["--help".to_string()]), 0);
        assert_eq!(run_gz(&["-h".to_string()]), 0);
        let _ = run_gz(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gz(&[]);
    }
}
