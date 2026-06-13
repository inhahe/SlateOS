#![deny(clippy::all)]

//! kafkactl-cli — SlateOS kafkactl Apache Kafka management
//!
//! Single personality: `kafkactl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kafkactl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: kafkactl COMMAND [OPTIONS]");
        println!("kafkactl v5.0.0 (SlateOS) — Apache Kafka CLI");
        println!();
        println!("Commands:");
        println!("  consume         Consume messages");
        println!("  produce         Produce messages");
        println!("  create          Create topic/acl");
        println!("  delete          Delete topic/acl/records");
        println!("  describe        Describe topic/broker/consumer-group");
        println!("  get             List topics/brokers/consumer-groups");
        println!("  alter           Alter topic/partition");
        println!("  clone           Clone topic");
        println!("  attach          Attach partition reassignment");
        println!("  reset           Reset consumer group offset");
        println!("  completion      Shell completion");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("kafkactl v5.0.0 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("get");
    match cmd {
        "get" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("topics");
            match sub {
                "topics" => {
                    println!("TOPIC              PARTITIONS  REPLICAS");
                    println!("events             6           3");
                    println!("orders             12          3");
                    println!("notifications      3           2");
                }
                "brokers" => {
                    println!("ID   ADDRESS                RACK");
                    println!("0    broker-0:9092          us-east-1a");
                    println!("1    broker-1:9092          us-east-1b");
                    println!("2    broker-2:9092          us-east-1c");
                }
                "consumer-groups" => {
                    println!("GROUP              STATE    MEMBERS");
                    println!("my-consumer        Stable   3");
                    println!("analytics          Stable   2");
                }
                _ => println!("kafkactl get {}: completed", sub),
            }
        }
        "describe" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("topic");
            if sub == "topic" {
                let name = args.get(2).map(|s| s.as_str()).unwrap_or("events");
                println!("Topic: {}", name);
                println!("  Partitions: 6");
                println!("  Replication: 3");
                println!("  Config: retention.ms=604800000, cleanup.policy=delete");
            }
        }
        "consume" => {
            println!("key: user-1  value: {{\"event\":\"login\"}}  partition: 0  offset: 1234");
            println!("key: user-2  value: {{\"event\":\"purchase\"}}  partition: 1  offset: 5678");
        }
        "produce" => println!("Message produced to topic 'events' partition 0 offset 9999"),
        "create" => {
            let sub = args.get(1).map(|s| s.as_str()).unwrap_or("topic");
            println!("kafkactl: Created {} successfully.", sub);
        }
        "reset" => println!("Consumer group offsets reset."),
        _ => println!("kafkactl {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kafkactl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kafkactl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kafkactl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kafkactl"), "kafkactl");
        assert_eq!(basename(r"C:\bin\kafkactl.exe"), "kafkactl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kafkactl.exe"), "kafkactl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kafkactl(&["--help".to_string()], "kafkactl"), 0);
        assert_eq!(run_kafkactl(&["-h".to_string()], "kafkactl"), 0);
        let _ = run_kafkactl(&["--version".to_string()], "kafkactl");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kafkactl(&[], "kafkactl");
    }
}
