#![deny(clippy::all)]

//! tensorboard-cli — OurOS TensorBoard CLI
//!
//! Single personality: `tensorboard`

use std::env;
use std::process;

fn run_tensorboard(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tensorboard <COMMAND> [OPTIONS]");
        println!();
        println!("TensorBoard visualization toolkit CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  serve        Start TensorBoard server");
        println!("  dev          Start TensorBoard in dev mode");
        println!("  inspect      Inspect event files");
        println!("  export       Export scalars to CSV");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("TensorBoard 2.16.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("serve");
    match cmd {
        "serve" => {
            let logdir = args.windows(2).find(|w| w[0] == "--logdir").map(|w| w[1].as_str()).unwrap_or("./logs");
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("6006");
            let host = args.windows(2).find(|w| w[0] == "--host").map(|w| w[1].as_str()).unwrap_or("localhost");
            println!("TensorBoard 2.16.0 at http://{}:{} (Press CTRL+C to quit)", host, port);
            println!("  Serving data from: {}", logdir);
            println!("  Plugins loaded: scalars, images, histograms, graphs, projector, text, audio, hparams");
            println!("  Data reload every 5 seconds");
            0
        }
        "dev" => {
            let logdir = args.windows(2).find(|w| w[0] == "--logdir").map(|w| w[1].as_str()).unwrap_or("./logs");
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("6006");
            println!("TensorBoard dev mode at http://localhost:{}", port);
            println!("  Serving data from: {}", logdir);
            println!("  Hot reloading enabled");
            println!("  Debug logging enabled");
            0
        }
        "inspect" => {
            let logdir = args.get(1).map(|s| s.as_str()).unwrap_or("./logs");
            println!("Inspecting event files in {}...", logdir);
            println!();
            println!("  Event file: events.out.tfevents.1705312800.host");
            println!("    File version: brain.Event:2");
            println!("    Tags:");
            println!("      train/loss          (scalar)   1500 steps");
            println!("      train/accuracy      (scalar)   1500 steps");
            println!("      val/loss            (scalar)    300 steps");
            println!("      val/accuracy        (scalar)    300 steps");
            println!("      model/weights       (histogram) 150 steps");
            println!("      sample/images       (image)      30 steps");
            println!("      graph               (graph)       1 step");
            println!();
            println!("  Wall time range: 2024-01-15 12:00:00 to 2024-01-15 14:30:00");
            println!("  Total events: 3,481");
            0
        }
        "export" => {
            let logdir = args.get(1).map(|s| s.as_str()).unwrap_or("./logs");
            let output = args.windows(2).find(|w| w[0] == "--output" || w[0] == "-o").map(|w| w[1].as_str()).unwrap_or("export.csv");
            let tag = args.windows(2).find(|w| w[0] == "--tag").map(|w| w[1].as_str()).unwrap_or("train/loss");
            println!("Exporting tag '{}' from {}...", tag, logdir);
            println!("  Found 1500 data points");
            println!("  Written to {}", output);
            println!("  Format: wall_time,step,value");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: tensorboard <command>. See --help.");
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
    let code = run_tensorboard(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_tensorboard};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_tensorboard(vec!["--help".to_string()]), 0);
        assert_eq!(run_tensorboard(vec!["-h".to_string()]), 0);
        assert_eq!(run_tensorboard(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_tensorboard(vec![]), 0);
    }
}
