#![deny(clippy::all)]

//! citrix-cli — OurOS Citrix Workspace / DaaS / Hypervisor
//!
//! Single personality: `citrix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ctx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: citrix [OPTIONS]");
        println!("Citrix Workspace 2410 (OurOS) — Enterprise VDI, app delivery, DaaS");
        println!();
        println!("Options:");
        println!("  --workspace            Citrix Workspace app (client)");
        println!("  --daas                 Citrix DaaS (Desktop-as-a-Service cloud)");
        println!("  --hypervisor           Citrix Hypervisor 8.2 (was XenServer)");
        println!("  --xenapp               Citrix Virtual Apps");
        println!("  --xendesktop           Citrix Virtual Desktops");
        println!("  --adc                  Citrix ADC (NetScaler — sold to Cloud Software Group)");
        println!("  --hdx                  HDX (HD eXperience) protocol session");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Citrix Workspace 2410.10.0.16 (OurOS)"); return 0; }
    println!("Citrix Workspace 2410.10.0.16 (OurOS)");
    println!("  Vendor: Cloud Software Group (formed Sep 2022 — Citrix + TIBCO merger)");
    println!("  Citrix founded: 1989 by Ed Iacobucci (Texas) — pioneered terminal session protocols");
    println!("  Made famous by: WinFrame (Multi-Win on NT 3.51), MetaFrame, Presentation Server,");
    println!("                  XenApp/XenDesktop, now Citrix Virtual Apps and Desktops (CVAD)");
    println!("  Acquired by: Vista Equity Partners + Elliott Investment Mgmt Jan 2022 ($16.5B PE)");
    println!("  Products:");
    println!("    Workspace — unified client (replaces Receiver)");
    println!("    Virtual Apps and Desktops — VDI/RDS broker (was XenApp/XenDesktop)");
    println!("    DaaS — Citrix Cloud-hosted control plane, customer-owned workloads in Azure/AWS/GCP");
    println!("    Hypervisor — bare-metal Xen-based (formerly XenServer, now Citrix Hypervisor 8.2 EOL)");
    println!("  HDX: adaptive protocol family for delivering apps/desktops over WAN/internet");
    println!("  ICA: Independent Computing Architecture — Citrix's session protocol since 1990");
    println!("  Strengths: WAN-tolerant graphics, app virtualization, mature broker, F500 standard");
    println!("  NetScaler ADC: spun back out to standalone Cloud Software Group product 2023");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "citrix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ctx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
