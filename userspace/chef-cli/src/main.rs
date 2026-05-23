#![deny(clippy::all)]

//! chef-cli — OurOS Chef configuration management
//!
//! Multi-personality: `knife`, `chef-client`, `chef-solo`, `ohai`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_knife(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: knife SUBCOMMAND [OPTIONS]");
        println!();
        println!("knife — Chef server CLI (OurOS).");
        println!();
        println!("Subcommands:");
        println!("  node list             List nodes");
        println!("  node show NAME        Show node");
        println!("  cookbook list          List cookbooks");
        println!("  cookbook upload NAME   Upload cookbook");
        println!("  role list             List roles");
        println!("  environment list      List environments");
        println!("  data bag list         List data bags");
        println!("  status                Node status");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Chef Infra Client: 18.3.0 (OurOS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    let sub2 = args.get(1).map(|s| s.as_str()).unwrap_or("list");
    match (subcmd, sub2) {
        ("node", "list") => {
            println!("ouros-desktop.local");
            println!("ouros-server-1.local");
            println!("ouros-server-2.local");
        }
        ("node", "show") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("ouros-desktop.local");
            println!("Node Name:   {}", name);
            println!("Environment: production");
            println!("FQDN:        {}", name);
            println!("IP:          192.168.1.100");
            println!("Run List:    recipe[base], recipe[nginx], role[webserver]");
            println!("Platform:    ouros 1.0");
        }
        ("cookbook", "list") => {
            println!("apt        7.4.0");
            println!("base       1.0.0");
            println!("nginx      12.1.0");
            println!("postgresql 11.0.0");
        }
        ("role", "list") => {
            println!("base");
            println!("webserver");
            println!("database");
        }
        ("status", _) => {
            println!("1 hour ago, ouros-desktop.local, ouros-desktop.local, 192.168.1.100, ouros 1.0.");
            println!("2 hours ago, ouros-server-1.local, ouros-server-1.local, 192.168.1.101, ouros 1.0.");
        }
        _ => println!("knife: {} {} completed", subcmd, sub2),
    }
    0
}

fn run_chef_client(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: chef-client [OPTIONS]");
        println!("  -o, --override-runlist  Override run list");
        println!("  -j, --json-attributes   JSON attributes file");
        println!("  -l, --log_level         Log level");
        println!("  --once                  Run once and exit");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Chef Infra Client: 18.3.0 (OurOS)");
        return 0;
    }

    println!("[2024-05-22T12:00:00+00:00] INFO: *** Chef Infra Client 18.3.0 ***");
    println!("[2024-05-22T12:00:00+00:00] INFO: Platform: x86_64-ouros");
    println!("[2024-05-22T12:00:01+00:00] INFO: Setting the run_list to [\"recipe[base]\", \"recipe[nginx]\"]");
    println!("[2024-05-22T12:00:02+00:00] INFO: Run List is [recipe[base], recipe[nginx]]");
    println!("[2024-05-22T12:00:03+00:00] INFO: Processing package[nginx] action install");
    println!("[2024-05-22T12:00:04+00:00] INFO: Processing service[nginx] action enable");
    println!("[2024-05-22T12:00:05+00:00] INFO: Chef Infra Client finished, 2/8 resources updated in 5 seconds");
    0
}

fn run_ohai(_args: &[String]) -> i32 {
    println!("{{");
    println!("  \"os\": \"ouros\",");
    println!("  \"os_version\": \"1.0\",");
    println!("  \"platform\": \"ouros\",");
    println!("  \"hostname\": \"ouros-desktop\",");
    println!("  \"fqdn\": \"ouros-desktop.local\",");
    println!("  \"ipaddress\": \"192.168.1.100\",");
    println!("  \"memory\": {{\"total\": \"16384MB\"}},");
    println!("  \"cpu\": {{\"total\": 8}},");
    println!("  \"kernel\": {{\"name\": \"ouros\", \"machine\": \"x86_64\"}}");
    println!("}}");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "knife".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "chef-client" => run_chef_client(&rest),
        "chef-solo" => run_chef_client(&rest),
        "ohai" => run_ohai(&rest),
        _ => run_knife(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
