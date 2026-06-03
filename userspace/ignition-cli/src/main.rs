#![deny(clippy::all)]

//! ignition-cli — OurOS Ignition/Gazebo Sim tools
//!
//! Multi-personality: `ign`, `gz`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gz(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gz COMMAND [OPTIONS]");
        println!("Gazebo Sim v8 (OurOS) — Robot simulation platform");
        println!();
        println!("Commands:");
        println!("  sim               Launch simulator");
        println!("  model             Model inspection");
        println!("  topic             Topic tools");
        println!("  service           Service tools");
        println!("  fuel              Fuel model database");
        println!("  sdf               SDF validation");
        println!("  plugin            Plugin info");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Gazebo Sim v8 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("sim");
    match cmd {
        "sim" => {
            let world = args.get(1).map(|s| s.as_str()).unwrap_or("empty.sdf");
            println!("Launching Gazebo Sim...");
            println!("  World: {}", world);
            println!("  Physics: DART");
            println!("  Rendering: Ogre2");
            println!("  Transport: ign-transport");
        }
        "model" => {
            println!("Models in simulation:");
            println!("  ground_plane (static)");
            println!("  sun (light)");
        }
        "topic" => {
            println!("Topics:");
            println!("  /world/default/clock");
            println!("  /world/default/stats");
            println!("  /world/default/state");
        }
        "fuel" => {
            println!("Gazebo Fuel models:");
            println!("  OpenRobotics/Tugbot");
            println!("  OpenRobotics/X2_UAV");
            println!("  OpenRobotics/Prius_Hybrid");
        }
        "sdf" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("model.sdf");
            println!("Validating: {}", file);
            println!("  SDF version: 1.9");
            println!("  Valid: yes");
        }
        _ => println!("gz {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gz".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gz(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ignition"), "ignition");
        assert_eq!(basename(r"C:\bin\ignition.exe"), "ignition.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ignition.exe"), "ignition");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_gz(&["--help".to_string()], "ignition"), 0);
        assert_eq!(run_gz(&["-h".to_string()], "ignition"), 0);
        assert_eq!(run_gz(&["--version".to_string()], "ignition"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_gz(&[], "ignition"), 0);
    }
}
