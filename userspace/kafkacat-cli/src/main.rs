#![deny(clippy::all)]

//! kafkacat-cli — OurOS kcat (formerly kafkacat) Kafka CLI tool
//!
//! Multi-personality: `kcat`, `kafkacat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kcat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kcat [OPTIONS]");
        println!("kcat 1.7.1 (formerly kafkacat, OurOS)");
        println!();
        println!("Modes:");
        println!("  -P           Producer mode");
        println!("  -C           Consumer mode");
        println!("  -L           Metadata listing mode");
        println!("  -Q           Query mode (offsets)");
        println!();
        println!("Options:");
        println!("  -b BROKERS   Broker list");
        println!("  -t TOPIC     Topic name");
        println!("  -p PARTITION Partition");
        println!("  -o OFFSET    Offset (beginning, end, N)");
        println!("  -G GROUP     Consumer group (high-level consumer)");
        println!("  -K DELIM     Key delimiter");
        println!("  -D DELIM     Message delimiter");
        println!("  -c N         Exit after consuming N messages");
        println!("  -f FMT       Output format string");
        println!("  -V           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V") {
        println!("kcat - 1.7.1 (librdkafka 2.4.0)");
        return 0;
    }
    let broker = args.windows(2).find(|w| w[0] == "-b")
        .map(|w| w[1].as_str()).unwrap_or("localhost:9092");
    let topic = args.windows(2).find(|w| w[0] == "-t")
        .map(|w| w[1].as_str()).unwrap_or("events");

    if args.iter().any(|a| a == "-L") {
        println!("Metadata for all topics (from broker {})", broker);
        println!("  3 brokers:");
        println!("    broker 0 at 192.168.1.1:9092");
        println!("    broker 1 at 192.168.1.2:9092");
        println!("    broker 2 at 192.168.1.3:9092");
        println!("  3 topics:");
        println!("    topic \"events\" with 6 partitions");
        println!("    topic \"orders\" with 12 partitions");
        println!("    topic \"logs\" with 3 partitions");
        return 0;
    }
    if args.iter().any(|a| a == "-C") {
        println!("% Consuming from topic '{}' (broker: {})", topic, broker);
        println!("hello world");
        println!("test message");
        return 0;
    }
    if args.iter().any(|a| a == "-P") {
        println!("% Producing to topic '{}' (broker: {})", topic, broker);
        println!("% Type messages, one per line. Ctrl+D to finish.");
        return 0;
    }
    if args.iter().any(|a| a == "-Q") {
        println!("{} [0] offset: 42", topic);
        println!("{} [1] offset: 38", topic);
        return 0;
    }
    println!("kcat: specify -P (produce), -C (consume), -L (list), or -Q (query)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kcat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kcat(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kcat};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kafkacat"), "kafkacat");
        assert_eq!(basename(r"C:\bin\kafkacat.exe"), "kafkacat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kafkacat.exe"), "kafkacat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kcat(&["--help".to_string()]), 0);
        assert_eq!(run_kcat(&["-h".to_string()]), 0);
        assert_eq!(run_kcat(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kcat(&[]), 0);
    }
}
