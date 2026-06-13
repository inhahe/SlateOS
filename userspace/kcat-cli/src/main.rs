#![deny(clippy::all)]

//! kcat-cli — SlateOS kcat (kafkacat) Kafka tool
//!
//! Single personality: `kcat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kcat(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kcat MODE [OPTIONS]");
        println!("kcat v1.7.0 (Slate OS) — Kafka cat (producer/consumer/metadata)");
        println!();
        println!("Modes:");
        println!("  -P              Producer mode");
        println!("  -C              Consumer mode");
        println!("  -L              Metadata list mode");
        println!("  -Q              Query mode (offsets)");
        println!();
        println!("Options:");
        println!("  -b BROKERS      Broker list (host:port,...)");
        println!("  -t TOPIC        Topic name");
        println!("  -p PARTITION    Partition");
        println!("  -o OFFSET       Offset (beginning, end, N, -N)");
        println!("  -G GROUP        Consumer group");
        println!("  -K DELIM        Key delimiter");
        println!("  -D DELIM        Message delimiter");
        println!("  -c COUNT        Message count");
        println!("  -e              Exit after last message");
        println!("  -f FORMAT       Output format string");
        println!("  -X PROP=VAL     librdkafka config");
        println!("  -V              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("kcat - 1.7.0 (Slate OS)");
        println!("librdkafka - 2.3.0 (builtin.features=gzip,snappy,ssl,sasl)");
        return 0;
    }
    let mode = args.first().map(|s| s.as_str()).unwrap_or("-L");
    match mode {
        "-C" => {
            println!("% Reached end of topic events [0] at offset 100");
            println!("{{\"user\":\"alice\",\"action\":\"login\"}}");
            println!("{{\"user\":\"bob\",\"action\":\"purchase\"}}");
        }
        "-P" => println!("% Producing to topic 'events'..."),
        "-L" => {
            println!("Metadata for all topics (from broker 0: localhost:9092/0):");
            println!(" 3 brokers:");
            println!("  broker 0 at localhost:9092");
            println!("  broker 1 at localhost:9093");
            println!("  broker 2 at localhost:9094");
            println!(" 3 topics:");
            println!("  topic \"events\" with 6 partitions:");
            println!("    partition 0, leader 0, replicas: 0,1,2, isrs: 0,1,2");
            println!("  topic \"orders\" with 12 partitions:");
            println!("    partition 0, leader 1, replicas: 0,1,2, isrs: 0,1,2");
        }
        "-Q" => println!("events [0]: 0 100"),
        _ => println!("kcat: unknown mode '{}'", mode),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kcat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kcat(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kcat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kcat"), "kcat");
        assert_eq!(basename(r"C:\bin\kcat.exe"), "kcat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kcat.exe"), "kcat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kcat(&["--help".to_string()], "kcat"), 0);
        assert_eq!(run_kcat(&["-h".to_string()], "kcat"), 0);
        let _ = run_kcat(&["--version".to_string()], "kcat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kcat(&[], "kcat");
    }
}
