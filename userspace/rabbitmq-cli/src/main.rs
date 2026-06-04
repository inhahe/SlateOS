#![deny(clippy::all)]

//! rabbitmq-cli — OurOS RabbitMQ CLI
//!
//! Single personality: `rabbitmqctl`

use std::env;
use std::process;

fn run_rabbitmqctl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rabbitmqctl <COMMAND> [OPTIONS]");
        println!();
        println!("RabbitMQ management CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  status           Show node status");
        println!("  list_queues      List queues");
        println!("  list_exchanges   List exchanges");
        println!("  list_bindings    List bindings");
        println!("  list_connections List connections");
        println!("  list_channels    List channels");
        println!("  list_consumers   List consumers");
        println!("  add_user         Add a user");
        println!("  delete_user      Delete a user");
        println!("  list_users       List users");
        println!("  add_vhost        Add a vhost");
        println!("  list_vhosts      List vhosts");
        println!("  set_permissions  Set user permissions");
        println!("  cluster_status   Show cluster status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("rabbitmqctl 3.13.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "status" => {
            println!("Status of node rabbit@myhost ...");
            println!("Runtime:");
            println!("  OS PID: 12345");
            println!("  Uptime (seconds): 345600");
            println!("  RabbitMQ version: 3.13.0");
            println!("  Erlang version: 26.2.1");
            println!("Listeners:");
            println!("  amqp:  0.0.0.0:5672");
            println!("  http:  0.0.0.0:15672");
            println!("Memory:");
            println!("  Total: 256 MB");
            println!("  Connection readers: 12 MB");
            println!("  Queue procs: 45 MB");
            println!("  Mnesia: 8 MB");
            0
        }
        "list_queues" => {
            println!("Timeout: 60.0 seconds ...");
            println!("Listing queues for vhost / ...");
            println!("name                    messages  consumers");
            println!("orders.queue            1234      3");
            println!("notifications.queue     56        2");
            println!("dead-letter.queue       12        0");
            println!("payments.queue          0         4");
            0
        }
        "list_exchanges" => {
            println!("Listing exchanges for vhost / ...");
            println!("name                    type      durable  auto_delete");
            println!("                        direct    true     false");
            println!("amq.direct              direct    true     false");
            println!("amq.fanout              fanout    true     false");
            println!("amq.topic               topic     true     false");
            println!("orders.exchange         topic     true     false");
            println!("notifications.exchange  fanout    true     false");
            0
        }
        "list_connections" => {
            println!("Listing connections ...");
            println!("user         peer_host        peer_port  state    channels");
            println!("app-user     192.168.1.10     45678      running  3");
            println!("app-user     192.168.1.11     45679      running  2");
            println!("admin        192.168.1.1      45680      running  1");
            0
        }
        "list_users" => {
            println!("Listing users ...");
            println!("user          tags");
            println!("admin         [administrator]");
            println!("app-user      [monitoring]");
            println!("guest         [administrator]");
            0
        }
        "add_user" => {
            let user = args.get(1).map(|s| s.as_str()).unwrap_or("newuser");
            println!("Adding user \"{}\" ...", user);
            println!("Done.");
            0
        }
        "list_vhosts" => {
            println!("Listing vhosts ...");
            println!("name          tracing");
            println!("/             false");
            println!("/production   false");
            println!("/staging      false");
            0
        }
        "cluster_status" => {
            println!("Cluster status of node rabbit@myhost ...");
            println!("Nodes:");
            println!("  disc: rabbit@myhost");
            println!("  disc: rabbit@node2");
            println!("  disc: rabbit@node3");
            println!("Running Nodes:");
            println!("  rabbit@myhost");
            println!("  rabbit@node2");
            println!("  rabbit@node3");
            println!("Alarms: (none)");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: rabbitmqctl <command>. See --help.");
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
    let code = run_rabbitmqctl(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_rabbitmqctl};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rabbitmqctl(vec!["--help".to_string()]), 0);
        assert_eq!(run_rabbitmqctl(vec!["-h".to_string()]), 0);
        let _ = run_rabbitmqctl(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rabbitmqctl(vec![]);
    }
}
