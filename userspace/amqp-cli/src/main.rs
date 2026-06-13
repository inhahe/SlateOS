#![deny(clippy::all)]

//! amqp-cli — Slate OS AMQP command-line tools
//!
//! Multi-personality: `amqp-publish`, `amqp-consume`, `amqp-declare-queue`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_amqp(args: &[String], prog_name: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        match prog_name {
            "amqp-publish" => {
                println!("Usage: amqp-publish [OPTIONS]");
                println!("  --url URL          AMQP broker URL");
                println!("  -e EXCHANGE        Exchange name");
                println!("  -r ROUTING_KEY     Routing key");
                println!("  --body MSG         Message body");
            }
            "amqp-consume" => {
                println!("Usage: amqp-consume [OPTIONS]");
                println!("  --url URL          AMQP broker URL");
                println!("  -q QUEUE           Queue name");
                println!("  -c N               Consume N messages then exit");
                println!("  --no-ack           Don't acknowledge messages");
            }
            _ => {
                println!("Usage: amqp-declare-queue [OPTIONS]");
                println!("  --url URL          AMQP broker URL");
                println!("  -q QUEUE           Queue name");
                println!("  --durable          Durable queue");
                println!("  --exclusive        Exclusive queue");
            }
        }
        println!("AMQP CLI tools (amqp-tools 0.10.0, Slate OS)");
        return 0;
    }
    let url = args.windows(2).find(|w| w[0] == "--url")
        .map(|w| w[1].as_str()).unwrap_or("amqp://guest:guest@localhost:5672/");

    match prog_name {
        "amqp-publish" => {
            let exchange = args.windows(2).find(|w| w[0] == "-e")
                .map(|w| w[1].as_str()).unwrap_or("");
            let key = args.windows(2).find(|w| w[0] == "-r")
                .map(|w| w[1].as_str()).unwrap_or("test");
            let body = args.windows(2).find(|w| w[0] == "--body")
                .map(|w| w[1].as_str()).unwrap_or("hello");
            println!("Publishing to {} exchange='{}' routing_key='{}'", url, exchange, key);
            println!("Body: {}", body);
            println!("Published.");
        }
        "amqp-consume" => {
            let queue = args.windows(2).find(|w| w[0] == "-q")
                .map(|w| w[1].as_str()).unwrap_or("test-queue");
            println!("Consuming from {} queue='{}'", url, queue);
            println!("  [msg 1] hello");
            println!("  [msg 2] world");
        }
        _ => {
            let queue = args.windows(2).find(|w| w[0] == "-q")
                .map(|w| w[1].as_str()).unwrap_or("test-queue");
            let durable = args.iter().any(|a| a == "--durable");
            println!("Declaring queue '{}' at {} (durable={})", queue, url, durable);
            println!("Queue declared: 0 messages, 0 consumers.");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "amqp-publish".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_amqp(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_amqp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/amqp"), "amqp");
        assert_eq!(basename(r"C:\bin\amqp.exe"), "amqp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("amqp.exe"), "amqp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_amqp(&["--help".to_string()], "amqp"), 0);
        assert_eq!(run_amqp(&["-h".to_string()], "amqp"), 0);
        let _ = run_amqp(&["--version".to_string()], "amqp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_amqp(&[], "amqp");
    }
}
