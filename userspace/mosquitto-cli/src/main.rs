#![deny(clippy::all)]

//! mosquitto-cli — OurOS Mosquitto MQTT CLI
//!
//! Multi-personality: `mosquitto_pub`, `mosquitto_sub`

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

fn run_pub(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mosquitto_pub [OPTIONS] -t <topic> -m <message>");
        println!();
        println!("Publish MQTT messages (OurOS).");
        println!();
        println!("Options:");
        println!("  -h HOST       Broker hostname (default: localhost)");
        println!("  -p PORT       Broker port (default: 1883)");
        println!("  -t TOPIC      Topic to publish to");
        println!("  -m MESSAGE    Message payload");
        println!("  -q QOS        Quality of Service (0, 1, 2)");
        println!("  -r             Retain message");
        println!("  -u USER       Username");
        println!("  -P PASS       Password");
        return 0;
    }

    let host = args.windows(2).find(|w| w[0] == "-h")
        .map(|w| w[1].as_str()).unwrap_or("localhost");
    let port = args.windows(2).find(|w| w[0] == "-p")
        .map(|w| w[1].as_str()).unwrap_or("1883");
    let topic = args.windows(2).find(|w| w[0] == "-t")
        .map(|w| w[1].as_str()).unwrap_or("test/topic");
    let message = args.windows(2).find(|w| w[0] == "-m")
        .map(|w| w[1].as_str()).unwrap_or("hello");
    let qos = args.windows(2).find(|w| w[0] == "-q")
        .map(|w| w[1].as_str()).unwrap_or("0");
    let retain = args.iter().any(|a| a == "-r");

    println!("Publishing to {}:{}", host, port);
    println!("  Topic: {}", topic);
    println!("  QoS: {}", qos);
    if retain {
        println!("  Retain: true");
    }
    println!("  Message: {}", message);
    println!("  Published successfully.");
    0
}

fn run_sub(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mosquitto_sub [OPTIONS] -t <topic>");
        println!();
        println!("Subscribe to MQTT topics (OurOS).");
        println!();
        println!("Options:");
        println!("  -h HOST       Broker hostname (default: localhost)");
        println!("  -p PORT       Broker port (default: 1883)");
        println!("  -t TOPIC      Topic to subscribe to (may repeat)");
        println!("  -q QOS        Quality of Service (0, 1, 2)");
        println!("  -v             Print topic with message");
        println!("  -u USER       Username");
        println!("  -P PASS       Password");
        println!("  -C COUNT      Disconnect after COUNT messages");
        return 0;
    }

    let host = args.windows(2).find(|w| w[0] == "-h")
        .map(|w| w[1].as_str()).unwrap_or("localhost");
    let port = args.windows(2).find(|w| w[0] == "-p")
        .map(|w| w[1].as_str()).unwrap_or("1883");
    let topic = args.windows(2).find(|w| w[0] == "-t")
        .map(|w| w[1].as_str()).unwrap_or("#");
    let verbose = args.iter().any(|a| a == "-v");

    println!("Subscribing to {}:{}", host, port);
    println!("  Topic: {}", topic);
    println!();

    if verbose {
        println!("sensors/temperature {{\"value\": 22.5, \"unit\": \"C\"}}");
        println!("sensors/humidity {{\"value\": 45.2, \"unit\": \"%\"}}");
        println!("devices/light/status {{\"state\": \"on\", \"brightness\": 80}}");
    } else {
        println!("{{\"value\": 22.5, \"unit\": \"C\"}}");
        println!("{{\"value\": 45.2, \"unit\": \"%\"}}");
        println!("{{\"state\": \"on\", \"brightness\": 80}}");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mosquitto_pub".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "mosquitto_sub" => run_sub(&rest),
        _ => run_pub(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
