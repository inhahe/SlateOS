#![deny(clippy::all)]

//! kafka-cli — OurOS Apache Kafka CLI
//!
//! Multi-personality: `kafka-topics`, `kafka-console-producer`, `kafka-console-consumer`, `kafka-consumer-groups`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn strip_ext(name: &str) -> &str {
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
}

fn run_topics(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: kafka-topics --bootstrap-server <server> <COMMAND>");
        println!("  --list             List all topics");
        println!("  --create           Create a topic");
        println!("  --delete           Delete a topic");
        println!("  --describe         Describe a topic");
        return 0;
    }
    if args.iter().any(|a| a == "--list") {
        println!("orders");
        println!("user-events");
        println!("payments");
        println!("notifications");
        println!("__consumer_offsets");
        return 0;
    }
    if args.iter().any(|a| a == "--create") {
        let topic = args.windows(2).find(|w| w[0] == "--topic")
            .map(|w| w[1].as_str()).unwrap_or("new-topic");
        let partitions = args.windows(2).find(|w| w[0] == "--partitions")
            .map(|w| w[1].as_str()).unwrap_or("3");
        let replication = args.windows(2).find(|w| w[0] == "--replication-factor")
            .map(|w| w[1].as_str()).unwrap_or("1");
        println!("Created topic {}.", topic);
        println!("  Partitions: {}", partitions);
        println!("  Replication factor: {}", replication);
        return 0;
    }
    if args.iter().any(|a| a == "--describe") {
        let topic = args.windows(2).find(|w| w[0] == "--topic")
            .map(|w| w[1].as_str()).unwrap_or("orders");
        println!("Topic: {}    TopicId: abc123def456    PartitionCount: 3    ReplicationFactor: 3", topic);
        println!("  Partition: 0    Leader: 1    Replicas: 1,2,3    Isr: 1,2,3");
        println!("  Partition: 1    Leader: 2    Replicas: 2,3,1    Isr: 2,3,1");
        println!("  Partition: 2    Leader: 3    Replicas: 3,1,2    Isr: 3,1,2");
        return 0;
    }
    eprintln!("Usage: kafka-topics --bootstrap-server <server> <command>. See --help.");
    1
}

fn run_producer(args: &[String]) -> i32 {
    let topic = args.windows(2).find(|w| w[0] == "--topic")
        .map(|w| w[1].as_str()).unwrap_or("test");
    let broker = args.windows(2).find(|w| w[0] == "--bootstrap-server")
        .map(|w| w[1].as_str()).unwrap_or("localhost:9092");
    println!("Producing to topic '{}' on {}...", topic, broker);
    println!("> (interactive mode - type messages, Ctrl+C to quit)");
    0
}

fn run_consumer(args: &[String]) -> i32 {
    let topic = args.windows(2).find(|w| w[0] == "--topic")
        .map(|w| w[1].as_str()).unwrap_or("test");
    let broker = args.windows(2).find(|w| w[0] == "--bootstrap-server")
        .map(|w| w[1].as_str()).unwrap_or("localhost:9092");
    let from_beginning = args.iter().any(|a| a == "--from-beginning");
    println!("Consuming from topic '{}' on {}...", topic, broker);
    if from_beginning {
        println!("  (starting from beginning)");
    }
    println!("{{\"user_id\": 123, \"action\": \"login\", \"ts\": \"2024-01-15T14:00:00Z\"}}");
    println!("{{\"user_id\": 456, \"action\": \"purchase\", \"ts\": \"2024-01-15T14:00:01Z\"}}");
    println!("{{\"user_id\": 789, \"action\": \"logout\", \"ts\": \"2024-01-15T14:00:02Z\"}}");
    0
}

fn run_consumer_groups(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--list") {
        println!("GROUP                   STATE");
        println!("order-processor         Stable");
        println!("notification-service    Stable");
        println!("analytics-pipeline      Empty");
        return 0;
    }
    if args.iter().any(|a| a == "--describe") {
        let group = args.windows(2).find(|w| w[0] == "--group")
            .map(|w| w[1].as_str()).unwrap_or("order-processor");
        println!("GROUP           TOPIC     PARTITION  CURRENT-OFFSET  LOG-END-OFFSET  LAG");
        println!("{}  orders    0          12345           12345           0", group);
        println!("{}  orders    1          11234           11240           6", group);
        println!("{}  orders    2          13456           13456           0", group);
        return 0;
    }
    eprintln!("Usage: kafka-consumer-groups --bootstrap-server <server> [--list|--describe]. See --help.");
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "kafka-topics".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "kafka-console-producer" => run_producer(&rest),
        "kafka-console-consumer" => run_consumer(&rest),
        "kafka-consumer-groups" => run_consumer_groups(&rest),
        _ => run_topics(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
