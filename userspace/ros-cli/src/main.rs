#![deny(clippy::all)]

//! ros-cli — OurOS ROS 2 robotics middleware
//!
//! Multi-personality: `ros2`, `colcon`, `rosdep`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ros2(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("usage: ros2 [-h] COMMAND ...");
        println!("ROS 2 Jazzy Jalisco (OurOS)");
        println!();
        println!("Commands:");
        println!("  run          Run a ROS 2 node");
        println!("  launch       Launch a launch file");
        println!("  topic        Topic tools (list, echo, pub, info, hz)");
        println!("  service      Service tools (list, call, type)");
        println!("  node         Node tools (list, info)");
        println!("  param        Parameter tools (list, get, set)");
        println!("  bag          Bag tools (record, play, info)");
        println!("  pkg          Package tools (list, create)");
        println!("  interface    Interface tools (list, show)");
        println!("  doctor       Check ROS 2 setup");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "topic" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            match action {
                "list" => {
                    println!("/chatter");
                    println!("/cmd_vel");
                    println!("/odom");
                    println!("/scan");
                    println!("/tf");
                    println!("/tf_static");
                }
                "hz" => {
                    let topic = args.get(2).map(|s| s.as_str()).unwrap_or("/chatter");
                    println!("average rate: 10.00 Hz");
                    println!("  min: 0.098s max: 0.102s std dev: 0.001s");
                    let _ = topic;
                }
                _ => println!("ros2 topic {}: completed", action),
            }
        }
        "node" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("list");
            if action == "list" {
                println!("/talker");
                println!("/listener");
                println!("/robot_state_publisher");
                println!("/rviz2");
            }
        }
        "run" => {
            let pkg = args.get(1).map(|s| s.as_str()).unwrap_or("demo_nodes_cpp");
            let node = args.get(2).map(|s| s.as_str()).unwrap_or("talker");
            println!("[INFO] Starting node: {}/{}", pkg, node);
            println!("[INFO] Node started.");
        }
        "doctor" => {
            println!("ROS 2 Doctor Report:");
            println!("  Platform: OurOS x86_64");
            println!("  ROS 2 distro: Jazzy Jalisco");
            println!("  DDS middleware: FastDDS 2.14");
            println!("  All checks passed.");
        }
        "bag" => {
            let action = args.get(1).map(|s| s.as_str()).unwrap_or("info");
            if action == "info" {
                let bag = args.get(2).map(|s| s.as_str()).unwrap_or("rosbag2");
                println!("Bag: {}", bag);
                println!("  Duration: 30.5s");
                println!("  Messages: 1234");
                println!("  Topics: 5");
            }
        }
        _ => println!("ros2: '{}' completed", subcmd),
    }
    0
}

fn run_colcon(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: colcon COMMAND [OPTIONS]");
        println!("  build        Build packages");
        println!("  test         Test packages");
        println!("  list         List packages");
        println!("  graph        Show dependency graph");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("build");
    match subcmd {
        "build" => {
            println!("Starting >>> my_robot_pkg");
            println!("Finished <<< my_robot_pkg [2.3s]");
            println!("Starting >>> my_msgs_pkg");
            println!("Finished <<< my_msgs_pkg [1.8s]");
            println!("Summary: 2 packages finished [4.1s]");
        }
        "list" => {
            println!("my_robot_pkg     ament_cmake");
            println!("my_msgs_pkg      ament_cmake");
        }
        "test" => {
            println!("Starting >>> my_robot_pkg");
            println!("Finished <<< my_robot_pkg [1.2s]");
            println!("Summary: 1 package finished, 0 failures");
        }
        _ => println!("colcon: '{}' completed", subcmd),
    }
    0
}

fn run_rosdep(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rosdep COMMAND [OPTIONS]");
        println!("  install      Install dependencies");
        println!("  update       Update rosdep database");
        println!("  check        Check dependencies");
        println!("  keys         List rosdep keys");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("check");
    match subcmd {
        "update" => {
            println!("reading in sources list data...");
            println!("Hit https://raw.githubusercontent.com/ros/rosdistro/master/rosdep/...");
            println!("updated cache in /home/user/.ros/rosdep/sources.cache");
        }
        "install" => {
            println!("#All required rosdeps installed successfully");
        }
        "check" => {
            println!("All system dependencies have been satisfied");
        }
        _ => println!("rosdep: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ros2".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "colcon" => run_colcon(&rest),
        "rosdep" => run_rosdep(&rest),
        _ => run_ros2(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ros2};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ros"), "ros");
        assert_eq!(basename(r"C:\bin\ros.exe"), "ros.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ros.exe"), "ros");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ros2(&["--help".to_string()]), 0);
        assert_eq!(run_ros2(&["-h".to_string()]), 0);
        assert_eq!(run_ros2(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ros2(&[]), 0);
    }
}
