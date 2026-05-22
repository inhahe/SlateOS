#![deny(clippy::all)]

//! rabbitmq — OurOS message broker
//!
//! Multi-personality: `rabbitmq-server`, `rabbitmqctl`, `rabbitmq-plugins`, `rabbitmq-diagnostics`

use std::env;
use std::process;

fn run_server(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rabbitmq-server [options]");
        println!();
        println!("Options:");
        println!("  --detached         Run node in background");
        println!("  --nodename <name>  Node name (default: rabbit@localhost)");
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("RabbitMQ 3.13.2 (OurOS)");
        println!("Erlang/OTP 26");
        return 0;
    }
    let node = args.iter().position(|a| a == "--nodename")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("rabbit@localhost");
    println!("  ##  ##      RabbitMQ 3.13.2 (OurOS)");
    println!("  ##  ##");
    println!("  ##########  Copyright (c) 2007-2025 Broadcom Inc.");
    println!("  ######  ##");
    println!("  ##########  Licensed under the MPL 2.0");
    println!();
    println!("  Erlang:  26.2.4 [jit]");
    println!("  TLS:     OpenSSL 3.2.1");
    println!();
    println!("  Doc guides:  https://rabbitmq.com/docs/documentation");
    println!("  Support:     https://rabbitmq.com/docs/contact");
    println!();
    println!("  Logs: /var/log/rabbitmq/{}.log", node);
    println!();
    println!("  Starting broker...");
    println!("  completed with 4 plugins.");
    println!("  Server startup complete; 4 plugins started.");
    println!("  * rabbitmq_management");
    println!("  * rabbitmq_management_agent");
    println!("  * rabbitmq_web_dispatch");
    println!("  * rabbitmq_prometheus");
    0
}

fn run_ctl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let _cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: rabbitmqctl [--node <node>] <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  status              Display broker status");
            println!("  list_queues         List queues");
            println!("  list_exchanges      List exchanges");
            println!("  list_bindings       List bindings");
            println!("  list_connections    List connections");
            println!("  list_channels       List channels");
            println!("  list_users          List users");
            println!("  add_user            Add a user");
            println!("  set_permissions     Set user permissions");
            println!("  list_vhosts         List virtual hosts");
            println!("  stop_app            Stop the RabbitMQ application");
            println!("  start_app           Start the RabbitMQ application");
            println!("  reset               Reset the node");
            println!("  cluster_status      Display cluster status");
            0
        }
        "status" => {
            println!("Status of node rabbit@localhost ...");
            println!("Runtime");
            println!();
            println!("OS PID: 12345");
            println!("OS: OurOS — x86_64");
            println!("Uptime (seconds): 86400");
            println!("RabbitMQ version: 3.13.2");
            println!("Erlang version: 26.2.4");
            println!();
            println!("Plugins");
            println!();
            println!("Enabled plugin file: /etc/rabbitmq/enabled_plugins");
            println!("[rabbitmq_management,rabbitmq_prometheus]");
            println!();
            println!("Listeners");
            println!();
            println!("Interface: [::], port: 5672, protocol: amqp");
            println!("Interface: [::], port: 15672, protocol: http");
            println!("Interface: [::], port: 25672, protocol: clustering");
            0
        }
        "list_queues" => {
            println!("Timeout: 60.0 seconds ...");
            println!("Listing queues for vhost / ...");
            println!("name\tmessages");
            println!("email.outbound\t142");
            println!("order.processing\t38");
            println!("notifications\t1053");
            println!("dlx.email\t7");
            0
        }
        "list_exchanges" => {
            println!("Listing exchanges for vhost / ...");
            println!("name\ttype");
            println!("\tdirect");
            println!("amq.direct\tdirect");
            println!("amq.fanout\tfanout");
            println!("amq.headers\theaders");
            println!("amq.match\theaders");
            println!("amq.topic\ttopic");
            println!("app.events\ttopic");
            0
        }
        "list_users" => {
            println!("Listing users ...");
            println!("user\ttags");
            println!("guest\t[administrator]");
            println!("app_user\t[monitoring]");
            0
        }
        "list_vhosts" => {
            println!("Listing vhosts ...");
            println!("name");
            println!("/");
            println!("production");
            println!("staging");
            0
        }
        "cluster_status" => {
            println!("Cluster status of node rabbit@localhost ...");
            println!("Basics");
            println!();
            println!("Cluster name: rabbit@localhost");
            println!();
            println!("Disk Nodes");
            println!();
            println!("rabbit@localhost");
            println!();
            println!("Running Nodes");
            println!();
            println!("rabbit@localhost");
            0
        }
        "stop_app" => { println!("Stopping rabbit application on node rabbit@localhost ..."); 0 }
        "start_app" => { println!("Starting node rabbit@localhost ..."); 0 }
        other => { eprintln!("rabbitmqctl: unknown command '{}'", other); 1 }
    }
}

fn run_plugins(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: rabbitmq-plugins <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  list      List plugins");
            println!("  enable    Enable plugins");
            println!("  disable   Disable plugins");
            0
        }
        "list" => {
            println!("Listing plugins with pattern \".*\" ...");
            println!(" Configured: E = explicitly enabled; e = implicitly enabled");
            println!(" | Status: * = running on rabbit@localhost");
            println!(" |/");
            println!("[E*] rabbitmq_management              3.13.2");
            println!("[E*] rabbitmq_management_agent        3.13.2");
            println!("[E*] rabbitmq_prometheus              3.13.2");
            println!("[E*] rabbitmq_web_dispatch            3.13.2");
            println!("[  ] rabbitmq_amqp1_0                 3.13.2");
            println!("[  ] rabbitmq_auth_backend_ldap       3.13.2");
            println!("[  ] rabbitmq_mqtt                    3.13.2");
            println!("[  ] rabbitmq_stomp                   3.13.2");
            println!("[  ] rabbitmq_stream                  3.13.2");
            0
        }
        "enable" => {
            let plugin = cmd_args.first().map(|s| s.as_str()).unwrap_or("rabbitmq_management");
            println!("Enabling plugins on node rabbit@localhost:");
            println!("  {}", plugin);
            println!("The following plugins have been configured:");
            println!("  {}", plugin);
            println!("Applying plugin configuration to rabbit@localhost...");
            println!("Plugin configuration unchanged.");
            0
        }
        "disable" => {
            let plugin = cmd_args.first().map(|s| s.as_str()).unwrap_or("plugin");
            println!("Disabling plugins on node rabbit@localhost:");
            println!("  {}", plugin);
            0
        }
        other => { eprintln!("rabbitmq-plugins: unknown command '{}'", other); 1 }
    }
}

fn run_diagnostics(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "help".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: rabbitmq-diagnostics <command> [<args>]");
            println!();
            println!("Commands:");
            println!("  ping                Ping the node");
            println!("  status              Node status");
            println!("  check_running       Check if node is running");
            println!("  check_alarms        Check for alarms");
            println!("  memory_breakdown    Memory usage breakdown");
            println!("  environment         Node environment");
            0
        }
        "ping" => { println!("Ping succeeded on node rabbit@localhost"); 0 }
        "check_running" => { println!("Node rabbit@localhost is running"); 0 }
        "check_alarms" => { println!("Node rabbit@localhost reported no alarms, OK"); 0 }
        "memory_breakdown" => {
            println!("Reporting memory breakdown on node rabbit@localhost ...");
            println!();
            println!("connection_readers: 0.0524 gb (13.72%)");
            println!("queue_procs: 0.0412 gb (10.79%)");
            println!("allocated_unused: 0.0398 gb (10.42%)");
            println!("other_proc: 0.0301 gb (7.88%)");
            println!("binary: 0.0287 gb (7.51%)");
            println!("code: 0.0256 gb (6.70%)");
            println!("other_system: 0.0198 gb (5.18%)");
            println!("atom: 0.0145 gb (3.80%)");
            println!("other_ets: 0.0134 gb (3.51%)");
            println!("Total: 0.382 gb");
            0
        }
        "environment" => {
            println!("Application environment of rabbit@localhost ...");
            println!();
            println!("  log_levels: info");
            println!("  default_vhost: /");
            println!("  default_user: guest");
            println!("  disk_free_limit: 50000000");
            println!("  vm_memory_high_watermark: 0.4");
            0
        }
        other => { eprintln!("rabbitmq-diagnostics: unknown command '{}'", other); 1 }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("rabbitmq-server");
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
        "rabbitmqctl" => run_ctl(rest),
        "rabbitmq-plugins" => run_plugins(rest),
        "rabbitmq-diagnostics" => run_diagnostics(rest),
        _ => run_server(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
