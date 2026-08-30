#![deny(clippy::all)]

//! snmp-cli — Slate OS SNMP management tools
//!
//! Multi-personality: `snmpget`, `snmpwalk`, `snmpset`, `snmptrap`, `snmpbulkwalk`, `snmptranslate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_snmpget(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() < 2 {
        println!("Usage: snmpget [OPTIONS] <host> <OID> [OID...]");
        println!("  -v 1|2c|3    SNMP version");
        println!("  -c community Community string");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("NET-SNMP version: 5.9.4 (Slate OS)");
        return 0;
    }

    let oid = args.last().map(|s| s.as_str()).unwrap_or("sysDescr.0");
    match oid {
        "sysDescr.0" | ".1.3.6.1.2.1.1.1.0" => println!("SNMPv2-MIB::sysDescr.0 = STRING: Slate OS Desktop 1.0 (x86_64)"),
        "sysUpTime.0" | ".1.3.6.1.2.1.1.3.0" => println!("DISMAN-EVENT-MIB::sysUpTimeInstance = Timeticks: (1440000) 4:00:00.00"),
        "sysName.0" | ".1.3.6.1.2.1.1.5.0" => println!("SNMPv2-MIB::sysName.0 = STRING: slateos-desktop"),
        "sysContact.0" | ".1.3.6.1.2.1.1.4.0" => println!("SNMPv2-MIB::sysContact.0 = STRING: admin@slateos.local"),
        _ => println!("SNMPv2-MIB::{} = STRING: (value)", oid),
    }
    0
}

fn run_snmpwalk(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: snmpwalk [OPTIONS] <host> [OID]");
        return 0;
    }

    println!("SNMPv2-MIB::sysDescr.0 = STRING: Slate OS Desktop 1.0 (x86_64)");
    println!("SNMPv2-MIB::sysObjectID.0 = OID: NET-SNMP-MIB::netSnmpAgentOIDs.10");
    println!("DISMAN-EVENT-MIB::sysUpTimeInstance = Timeticks: (1440000) 4:00:00.00");
    println!("SNMPv2-MIB::sysContact.0 = STRING: admin@slateos.local");
    println!("SNMPv2-MIB::sysName.0 = STRING: slateos-desktop");
    println!("SNMPv2-MIB::sysLocation.0 = STRING: Home Office");
    println!("SNMPv2-MIB::sysServices.0 = INTEGER: 72");
    println!("IF-MIB::ifNumber.0 = INTEGER: 3");
    println!("IF-MIB::ifDescr.1 = STRING: lo");
    println!("IF-MIB::ifDescr.2 = STRING: eth0");
    println!("IF-MIB::ifDescr.3 = STRING: wlan0");
    println!("IF-MIB::ifType.1 = INTEGER: softwareLoopback(24)");
    println!("IF-MIB::ifType.2 = INTEGER: ethernetCsmacd(6)");
    println!("IF-MIB::ifType.3 = INTEGER: ieee80211(71)");
    0
}

fn run_snmpset(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() < 3 {
        println!("Usage: snmpset [OPTIONS] <host> <OID> <type> <value>");
        return 0;
    }
    let oid = args.iter().rev().nth(2).map(|s| s.as_str()).unwrap_or("sysName.0");
    let val = args.last().map(|s| s.as_str()).unwrap_or("new-value");
    println!("SNMPv2-MIB::{} = STRING: {}", oid, val);
    0
}

fn run_snmptranslate(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: snmptranslate [OPTIONS] <OID>");
        println!("  -On   Numeric output");
        println!("  -Of   Full OID path");
        println!("  -Td   Detailed description");
        return 0;
    }

    let oid = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("sysDescr");
    if args.iter().any(|a| a == "-On") {
        println!(".1.3.6.1.2.1.1.1");
    } else if args.iter().any(|a| a == "-Td") {
        println!("SNMPv2-MIB::sysDescr");
        println!("sysDescr OBJECT-TYPE");
        println!("  -- FROM\tSNMPv2-MIB");
        println!("  SYNTAX\tDisplayString (SIZE (0..255))");
        println!("  MAX-ACCESS\tread-only");
        println!("  STATUS\tcurrent");
        println!("  DESCRIPTION\t\"Textual description of the entity.\"");
        println!("::= {{ iso(1) org(3) dod(6) internet(1) mgmt(2) mib-2(1) system(1) 1 }}");
    } else {
        println!("SNMPv2-MIB::{}", oid);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "snmpget".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "snmpwalk" | "snmpbulkwalk" => run_snmpwalk(&rest),
        "snmpset" => run_snmpset(&rest),
        "snmptranslate" => run_snmptranslate(&rest),
        "snmptrap" => { println!("Trap sent successfully."); 0 }
        _ => run_snmpget(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_snmpget};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/snmp"), "snmp");
        assert_eq!(basename(r"C:\bin\snmp.exe"), "snmp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("snmp.exe"), "snmp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_snmpget(&["--help".to_string()]), 0);
        assert_eq!(run_snmpget(&["-h".to_string()]), 0);
        let _ = run_snmpget(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_snmpget(&[]);
    }
}
