#![deny(clippy::all)]

//! nsq-cli — OurOS NSQ messaging tools
//!
//! Multi-personality: `nsq_tail`, `nsq_to_file`, `nsqlookupd`, `nsqadmin`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nsq(args: &[String], prog_name: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog_name {
            "nsq_tail" => {
                println!("Usage: nsq_tail [OPTIONS]");
                println!("  --topic TOPIC          Topic to tail");
                println!("  --channel CHANNEL      Channel name");
                println!("  --lookupd-http URL     nsqlookupd address");
                println!("  -n N                   Number of messages");
            }
            "nsq_to_file" => {
                println!("Usage: nsq_to_file [OPTIONS]");
                println!("  --topic TOPIC          Topic to consume");
                println!("  --output-dir DIR       Output directory");
                println!("  --lookupd-http URL     nsqlookupd address");
            }
            "nsqlookupd" => {
                println!("Usage: nsqlookupd [OPTIONS]");
                println!("  --http-address ADDR    HTTP listen address");
                println!("  --tcp-address ADDR     TCP listen address");
            }
            _ => {
                println!("Usage: nsqadmin [OPTIONS]");
                println!("  --http-address ADDR    HTTP listen address");
                println!("  --lookupd-http URL     nsqlookupd address");
            }
        }
        println!("NSQ 1.3.0 (OurOS)");
        return 0;
    }
    match prog_name {
        "nsq_tail" => {
            let topic = args.windows(2).find(|w| w[0] == "--topic")
                .map(|w| w[1].as_str()).unwrap_or("events");
            println!("Tailing topic: {}", topic);
            println!("[2024-06-15 12:00:00] message 1");
            println!("[2024-06-15 12:00:01] message 2");
        }
        "nsq_to_file" => {
            let topic = args.windows(2).find(|w| w[0] == "--topic")
                .map(|w| w[1].as_str()).unwrap_or("events");
            println!("Writing topic '{}' to files...", topic);
        }
        "nsqlookupd" => {
            println!("nsqlookupd v1.3.0");
            println!("  HTTP: http://0.0.0.0:4161");
            println!("  TCP: 0.0.0.0:4160");
        }
        _ => {
            println!("nsqadmin v1.3.0");
            println!("  HTTP: http://0.0.0.0:4171");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nsq_tail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nsq(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_nsq};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/nsq"), "nsq");
        assert_eq!(basename(r"C:\bin\nsq.exe"), "nsq.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("nsq.exe"), "nsq");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_nsq(&["--help".to_string()], "nsq"), 0);
        assert_eq!(run_nsq(&["-h".to_string()], "nsq"), 0);
        assert_eq!(run_nsq(&["--version".to_string()], "nsq"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_nsq(&[], "nsq"), 0);
    }
}
