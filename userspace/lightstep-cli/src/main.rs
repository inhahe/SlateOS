#![deny(clippy::all)]

//! lightstep-cli — SlateOS Lightstep / ServiceNow Cloud Observability (Ben Sigelman's OpenTracing co.)
//!
//! Single personality: `lightstep`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ls(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: lightstep [OPTIONS]");
        println!("Lightstep / ServiceNow Cloud Observability (SlateOS) — Distributed tracing pioneer");
        println!();
        println!("Options:");
        println!("  --traces               Trace view");
        println!("  --change-intelligence  Detect anomalies before deploys vs after");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ServiceNow Cloud Observability (formerly Lightstep) 2024 (SlateOS)"); return 0; }
    println!("ServiceNow Cloud Observability (formerly Lightstep) 2024 (SlateOS)");
    println!("  Vendor: ServiceNow Inc. (NYSE:NOW) — acquired Lightstep May 2021 (~$300M, undisclosed)");
    println!("          rebranded 'ServiceNow Cloud Observability' 2023");
    println!("  Founders: Ben Sigelman + Daniel Spoonhower + Spencer Rugaber (San Francisco, 2015)");
    println!("           Ben Sigelman — co-creator of Google Dapper (the 2010 paper that founded distributed tracing)");
    println!("           also co-created OpenTracing (2016), OpenCensus, OpenTelemetry merger (2019)");
    println!("  History: built on Sigelman's Dapper experience — first commercial tracing at scale");
    println!("          championed open standards (OpenTracing → OpenTelemetry)");
    println!("          acquired by ServiceNow May 2021 for integration with ITSM");
    println!("          decline in mindshare post-acquisition (sales focus shifted to ServiceNow customers)");
    println!("  Pricing: Free tier (3 users, 30-day retention)");
    println!("          Pro/Enterprise — usage-based on span volume");
    println!("          enterprise pricing for ServiceNow-bundled deals");
    println!("  Killer feature — Change Intelligence:");
    println!("    correlate deploys/feature flag flips to latency/error changes");
    println!("    auto-flag 'release X regressed checkout p99'");
    println!("  Killer feature — Satellite architecture:");
    println!("    intelligent satellite agents do tail-sampling near the source");
    println!("    keep 100% of error+slow traces, sample boring ones");
    println!("    enabled 'see ALL the errors, even at petabyte trace volumes'");
    println!("  Features:");
    println!("    - OpenTelemetry-native (since Lightstep was a co-founder of OTel)");
    println!("    - Trace search with full-text + structured filters");
    println!("    - Notebooks (Jupyter-style mixed query + commentary)");
    println!("    - SLO management + error-budget burn alerting");
    println!("    - Workflows (post-incident retrospective tooling)");
    println!("  Strategy: ServiceNow positions observability as part of broader 'AIOps + ITOM' suite");
    println!("           goal: unified IT ops platform from monitoring → incident → CMDB → ticket");
    println!("  Cultural impact (independent of present): Ben Sigelman shaped the entire OpenTelemetry standard");
    println!("                                            most modern APM SDKs trace lineage to OpenTracing/Lightstep");
    println!("  Differentiator: distributed-tracing-first design + true OTel native + tail-sampling at scale");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "lightstep".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ls(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ls};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/lightstep"), "lightstep");
        assert_eq!(basename(r"C:\bin\lightstep.exe"), "lightstep.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("lightstep.exe"), "lightstep");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ls(&["--help".to_string()], "lightstep"), 0);
        assert_eq!(run_ls(&["-h".to_string()], "lightstep"), 0);
        let _ = run_ls(&["--version".to_string()], "lightstep");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ls(&[], "lightstep");
    }
}
