#![deny(clippy::all)]
//! unleash-cli — personality CLI for Unleash, the open-source feature toggle
//! service born inside FINN.no in Norway.
//!
//! Unleash started as an internal project at FINN.no (Norway's largest
//! classifieds site) around 2014, primary author Ivar Conradi Osthus. Open
//! sourced and grew an external community before being spun out as
//! "Bricks Software AS" trading as Unleash in 2019, headquartered in Oslo.
//! Apache 2.0 server + SDKs. Strong adoption inside finance + government
//! that need data residency and self-hosted feature management.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Unleash OSS feature toggle service personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         FINN.no origins, Bricks Software, Apache 2.0");
    println!("    toggles       The five feature toggle categories");
    println!("    strategies    Activation strategies, default and custom");
    println!("    architecture  Server + SDK proxy + edge");
    println!("    sdks          SDK languages");
    println!("    selfhost      Self-host vs Pro vs Enterprise");
    println!("    governance    Approvals, change requests, environments");
    println!("    customers     Finance and gov customers");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("unleash-cli 0.1.0 (FINN.no-lineage personality build)"); }

fn run_about() {
    println!("Unleash (Bricks Software AS)");
    println!("  Origin:     Internal at FINN.no ~2014.");
    println!("  Primary author: Ivar Conradi Osthus.");
    println!("  Open source release: 2014-2015.");
    println!("  Company:    Bricks Software AS, founded 2019, Oslo, Norway.");
    println!("  License:    Apache 2.0 (server + SDKs).");
    println!("  Source:     github.com/Unleash/unleash");
    println!("  Stars:      ~12,000+ on GitHub.");
}

fn run_toggles() {
    println!("Feature toggle categories — Pete Hodgson's classic taxonomy,");
    println!("which Unleash adopted in its docs:");
    println!("  Release toggles      ship dark code, flip on later");
    println!("  Experiment toggles   A/B and multivariate tests");
    println!("  Ops toggles          kill switches for ops emergencies");
    println!("  Permission toggles   per-user/group feature access");
    println!("  Stickiness control   keep users on a consistent variation");
    println!("All five sit on the same primitive: an Unleash toggle.");
}

fn run_strategies() {
    println!("Activation strategies:");
    println!("  Default              on/off, no constraints");
    println!("  UserIDs              specific user IDs");
    println!("  IPs                  by source IP / CIDR");
    println!("  Hostnames            by hostname");
    println!("  Gradual Rollout      percentage with sticky hash");
    println!("  Flexible Rollout     percentage + sub-context groups");
    println!("Custom strategies are implemented as a server plugin or in the SDK.");
    println!("Constraints can be layered on any strategy (custom attributes).");
}

fn run_architecture() {
    println!("Architecture:");
    println!("  Unleash server      Node.js + Postgres, central admin UI + API.");
    println!("  Unleash Edge        a high-performance Rust proxy that caches");
    println!("                      and serves toggles to client-side SDKs.");
    println!("  Unleash Proxy       lighter Node.js proxy for client SDKs.");
    println!("  Server SDKs poll/stream toggles and evaluate locally.");
    println!("  Postgres is the only required dependency.");
}

fn run_sdks() {
    println!("SDK matrix:");
    println!("  Server     Node, Java, Go, .NET, Python, Ruby, PHP, Rust.");
    println!("  Client     JavaScript browser, React, Vue, Svelte.");
    println!("  Mobile     iOS (Swift), Android (Kotlin), Flutter (via proxy).");
    println!("  All SDKs implement the same wire spec; community-maintained");
    println!("  ports cover Erlang, Elixir, Clojure, ClojureScript.");
}

fn run_selfhost() {
    println!("Editions:");
    println!("  Open Source       Apache 2.0, self-host, every flag feature");
    println!("                    needed for production.");
    println!("  Pro (Cloud)       managed, environments + variants + SSO.");
    println!("  Enterprise        self-host or managed; change requests,");
    println!("                    custom roles, audit, scheduled releases,");
    println!("                    SCIM, dedicated support.");
}

fn run_governance() {
    println!("Governance features (Enterprise):");
    println!("  Environments         dev/preprod/prod with separate API tokens.");
    println!("  Project hierarchy    team-scoped flag namespaces.");
    println!("  Change requests      4-eyes approval workflow before a flag");
    println!("                       change goes live in production.");
    println!("  Scheduled changes    release a flag at a future timestamp.");
    println!("  Audit log            every change attributed to a user.");
    println!("These features are what wins Unleash its banking + gov customers.");
}

fn run_customers() {
    println!("Selected customers / known adopters:");
    println!("  Norwegian government services (NAV, Skatteetaten via Norge)");
    println!("  DNB, the largest Norwegian bank");
    println!("  Vy (formerly NSB), Norwegian state rail");
    println!("  PostNord, Nordic postal service");
    println!("  Generali, ING, Deutsche Bank (community references)");
    println!("  Lufthansa, Mercedes-Benz.io");
    println!("Strong pattern: data-sovereignty-sensitive European enterprises");
    println!("preferring an Apache-2.0 + self-host story over US SaaS.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "unleash-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "toggles" => run_toggles(),
        "strategies" => run_strategies(),
        "architecture" => run_architecture(),
        "sdks" => run_sdks(),
        "selfhost" => run_selfhost(),
        "governance" => run_governance(),
        "customers" => run_customers(),
        "help" | "--help" | "-h" => print_help(&prog),
        "version" | "--version" | "-V" => print_version(),
        other => {
            println!("unknown command: {other}");
            print_help(&prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_separators() {
        assert_eq!(basename("/a/b/c"), "c");
        assert_eq!(basename("a\\b\\c"), "c");
        assert_eq!(basename("only"), "only");
    }

    #[test]
    fn strip_ext_drops_exe() {
        assert_eq!(strip_ext("foo.exe"), "foo");
        assert_eq!(strip_ext("foo"), "foo");
    }

    #[test]
    fn smoke_runs() {
        run_about();
        run_toggles();
        run_strategies();
        run_architecture();
        run_sdks();
        run_selfhost();
        run_governance();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("unleash-cli");
        print_version();
    }
}
