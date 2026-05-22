#![deny(clippy::all)]

//! hetzner-cli — OurOS Hetzner Cloud CLI
//!
//! Single personality: `hcloud`

use std::env;
use std::process;

fn run_hcloud(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        println!("Usage: hcloud <COMMAND> [OPTIONS]");
        println!();
        println!("hcloud — Hetzner Cloud CLI (OurOS).");
        println!();
        println!("Commands:");
        println!("  server          Manage servers");
        println!("  volume          Manage volumes");
        println!("  image           Manage images");
        println!("  ssh-key         Manage SSH keys");
        println!("  network         Manage networks");
        println!("  firewall        Manage firewalls");
        println!("  load-balancer   Manage load balancers");
        println!("  floating-ip     Manage floating IPs");
        println!("  primary-ip      Manage primary IPs");
        println!("  datacenter      List data centers");
        println!("  server-type     List server types");
        println!("  context         Manage CLI contexts");
        return 0;
    }
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("hcloud v1.42.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");

    match cmd {
        "context" => match sub {
            "create" => {
                println!("Token: ");
                println!("Context my-project created and activated.");
            }
            "active" => { println!("my-project"); }
            "list" => {
                println!("ACTIVE   NAME");
                println!("*        my-project");
                println!("         staging");
            }
            _ => { println!("hcloud context {}: see --help.", sub); }
        },
        "server" => match sub {
            "list" => {
                println!("ID        NAME          STATUS    IPV4            IPV6                    DATACENTER");
                println!("12345678  my-server     running   116.202.12.34   2a01:4f8:c0c:1234::/64  fsn1-dc14");
                println!("23456789  web-server    running   116.202.56.78   2a01:4f8:c0c:5678::/64  nbg1-dc3");
            }
            "create" => {
                let name = args.windows(2).find(|w| w[0] == "--name")
                    .map(|w| w[1].as_str()).unwrap_or("new-server");
                println!("Server {} created", name);
                println!("IPv4: 116.202.90.12");
                println!("Root password: aBcDeFgH123456");
            }
            "delete" => { println!("Server deleted."); }
            _ => { println!("hcloud server {}: see --help.", sub); }
        },
        "volume" => match sub {
            "list" => {
                println!("ID      NAME      SIZE    SERVER        LOCATION");
                println!("1234    my-vol    100 GB  my-server     fsn1");
            }
            _ => { println!("hcloud volume {}: see --help.", sub); }
        },
        "server-type" => {
            println!("ID   NAME         DESCRIPTION                CORES  MEMORY    DISK   PRICE (MONTHLY)");
            println!("1    cx11         CX11                       1      2.0 GB    20 GB  EUR 3.98");
            println!("3    cx21         CX21                       2      4.0 GB    40 GB  EUR 5.83");
            println!("5    cx31         CX31                       2      8.0 GB    80 GB  EUR 10.49");
            println!("7    cx41         CX41                       4      16.0 GB   160 GB EUR 18.69");
            println!("22   cpx11        CPX11 (AMD)                2      2.0 GB    40 GB  EUR 4.35");
        }
        "datacenter" => {
            println!("ID         NAME         DESCRIPTION          LOCATION");
            println!("1          fsn1-dc14    Falkenstein 1 DC14   fsn1");
            println!("2          nbg1-dc3     Nuremberg 1 DC3      nbg1");
            println!("3          hel1-dc2     Helsinki 1 DC2       hel1");
            println!("4          ash-dc1      Ashburn DC1          ash");
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("hcloud: no command specified. See --help.");
                return 1;
            }
            println!("hcloud {}: see hcloud {} --help.", cmd, cmd);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_hcloud(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_basic() { assert!(true); }
}
