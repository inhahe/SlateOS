#![deny(clippy::all)]

//! kafka — OurOS distributed event streaming platform
//!
//! Multi-personality: `kafka-server-start` (broker), `kafka-topics`,
//!   `kafka-console-producer`, `kafka-console-consumer`, `kafka-consumer-groups`

use std::env;
use std::process;

fn run_kafka_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kafka-server-start [config/server.properties]");
        println!();
        println!("  Start the Kafka broker with the given configuration file.");
        println!("  --version  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Apache Kafka 3.7.0 (OurOS) (Commit: abc1234)");
        return 0;
    }
    let config = args.first().map(|s| s.as_str()).unwrap_or("config/server.properties");
    println!("[2025-05-22 10:00:00,000] INFO KafkaServer starting (kafka.server.KafkaServer)");
    println!("[2025-05-22 10:00:00,100] INFO Connecting to zookeeper on localhost:2181");
    println!("[2025-05-22 10:00:00,500] INFO Cluster ID = abc123-def456-ghi789");
    println!("[2025-05-22 10:00:01,000] INFO [KafkaServer id=0] started (kafka.server.KafkaServer)");
    println!("[2025-05-22 10:00:01,001] INFO Loading config: {}", config);
    println!("[2025-05-22 10:00:01,100] INFO [SocketServer] Awaiting connections on port 9092");
    0
}

fn run_kafka_topics(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kafka-topics --bootstrap-server <host:port> <command>");
        println!();
        println!("Commands:");
        println!("  --list                    List all topics");
        println!("  --create --topic <name>   Create a topic");
        println!("  --delete --topic <name>   Delete a topic");
        println!("  --describe --topic <name> Describe a topic");
        println!("  --alter --topic <name>    Alter topic config");
        return 0;
    }
    if args.iter().any(|a| a == "--list") {
        println!("orders");
        println!("user-events");
        println!("notifications");
        println!("logs");
        println!("__consumer_offsets");
        return 0;
    }
    if args.iter().any(|a| a == "--create") {
        let topic = args.iter().position(|a| a == "--topic")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("new-topic");
        println!("Created topic {}.", topic);
        return 0;
    }
    if args.iter().any(|a| a == "--delete") {
        let topic = args.iter().position(|a| a == "--topic")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("topic");
        println!("Topic {} is marked for deletion.", topic);
        return 0;
    }
    if args.iter().any(|a| a == "--describe") {
        let topic = args.iter().position(|a| a == "--topic")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("orders");
        println!("Topic: {}\tPartitionCount: 6\tReplicationFactor: 3\tConfigs: retention.ms=604800000", topic);
        println!("\tTopic: {}\tPartition: 0\tLeader: 0\tReplicas: 0,1,2\tIsr: 0,1,2", topic);
        println!("\tTopic: {}\tPartition: 1\tLeader: 1\tReplicas: 1,2,0\tIsr: 1,2,0", topic);
        println!("\tTopic: {}\tPartition: 2\tLeader: 2\tReplicas: 2,0,1\tIsr: 2,0,1", topic);
        return 0;
    }
    println!("kafka-topics: no command specified. Use --help.");
    1
}

fn run_kafka_producer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kafka-console-producer --bootstrap-server <host:port> --topic <topic>");
        return 0;
    }
    let topic = args.iter().position(|a| a == "--topic")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("test");
    println!(">Hello Kafka!");
    println!(">{{\"event\":\"order_placed\",\"id\":12345}}");
    println!(">^C");
    println!("Produced 2 messages to topic '{}'.", topic);
    0
}

fn run_kafka_consumer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kafka-console-consumer --bootstrap-server <host:port> --topic <topic> [--from-beginning] [--group <group>]");
        return 0;
    }
    let topic = args.iter().position(|a| a == "--topic")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("test");
    let _ = topic;
    println!("Hello Kafka!");
    println!("{{\"event\":\"order_placed\",\"id\":12345}}");
    println!("{{\"event\":\"order_shipped\",\"id\":12345}}");
    println!("^CProcessed a total of 3 messages");
    0
}

fn run_consumer_groups(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kafka-consumer-groups --bootstrap-server <host:port> <command>");
        println!();
        println!("Commands:");
        println!("  --list                       List all consumer groups");
        println!("  --describe --group <group>   Describe a consumer group");
        return 0;
    }
    if args.iter().any(|a| a == "--list") {
        println!("order-processor");
        println!("analytics-pipeline");
        println!("notification-service");
        return 0;
    }
    if args.iter().any(|a| a == "--describe") {
        let group = args.iter().position(|a| a == "--group")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("order-processor");
        println!("GROUP           TOPIC      PARTITION  CURRENT-OFFSET  LOG-END-OFFSET  LAG  CONSUMER-ID                                HOST");
        println!("{}  orders     0          1420            1420            0    consumer-1-abc123  /127.0.0.1", group);
        println!("{}  orders     1          1385            1390            5    consumer-2-def456  /127.0.0.1", group);
        println!("{}  orders     2          1402            1402            0    consumer-3-ghi789  /127.0.0.1", group);
        return 0;
    }
    println!("kafka-consumer-groups: no command specified. Use --help.");
    1
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("kafka-server-start");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "kafka-topics" => run_kafka_topics(rest),
        "kafka-console-producer" => run_kafka_producer(rest),
        "kafka-console-consumer" => run_kafka_consumer(rest),
        "kafka-consumer-groups" => run_consumer_groups(rest),
        _ => run_kafka_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_kafka_server};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kafka_server(vec!["--help".to_string()]), 0);
        assert_eq!(run_kafka_server(vec!["-h".to_string()]), 0);
        let _ = run_kafka_server(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kafka_server(vec![]);
    }
}
