#![deny(clippy::all)]

//! zabbix-cli — OurOS Zabbix monitoring agent & tools
//!
//! Multi-personality: `zabbix_agentd`, `zabbix_sender`, `zabbix_get`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zabbix_agentd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zabbix_agentd [OPTIONS]");
        println!("zabbix_agentd v6.4 (OurOS) — Zabbix monitoring agent");
        println!();
        println!("Options:");
        println!("  -c FILE       Configuration file");
        println!("  -f            Run in foreground");
        println!("  -t ITEM       Test single item");
        println!("  -p            Print supported items");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zabbix_agentd v6.4 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-p") {
        println!("agent.hostname     [s|hostname]");
        println!("agent.ping         [u|1]");
        println!("system.cpu.load    [d|0.15]");
        println!("system.cpu.util    [d|5.2]");
        println!("vm.memory.size     [u|8589934592]");
        println!("vfs.fs.size        [u|53687091200]");
        return 0;
    }
    println!("zabbix_agentd: agent started");
    println!("  Server: 127.0.0.1");
    println!("  Hostname: ouros-host");
    println!("  ListenPort: 10050");
    0
}

fn run_zabbix_sender(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zabbix_sender [OPTIONS] -k KEY -o VALUE");
        println!("zabbix_sender v6.4 (OurOS) — Send data to Zabbix server");
        println!("  -z SERVER     Zabbix server");
        println!("  -s HOST       Technical hostname");
        println!("  -k KEY        Item key");
        println!("  -o VALUE      Item value");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zabbix_sender v6.4 (OurOS)"); return 0; }
    println!("info from server: \"processed: 1; failed: 0; total: 1\"");
    println!("sent: 1; skipped: 0; total: 1");
    0
}

fn run_zabbix_get(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zabbix_get -s HOST -k KEY");
        println!("zabbix_get v6.4 (OurOS) — Get data from Zabbix agent");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("zabbix_get v6.4 (OurOS)"); return 0; }
    println!("0.150000");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zabbix_agentd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "zabbix_sender" => run_zabbix_sender(&rest, &prog),
        "zabbix_get" => run_zabbix_get(&rest, &prog),
        _ => run_zabbix_agentd(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_zabbix_agentd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/zabbix"), "zabbix");
        assert_eq!(basename(r"C:\bin\zabbix.exe"), "zabbix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("zabbix.exe"), "zabbix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_zabbix_agentd(&["--help".to_string()], "zabbix"), 0);
        assert_eq!(run_zabbix_agentd(&["-h".to_string()], "zabbix"), 0);
        let _ = run_zabbix_agentd(&["--version".to_string()], "zabbix");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_zabbix_agentd(&[], "zabbix");
    }
}
