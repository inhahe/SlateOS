#![deny(clippy::all)]
//! configcat-cli — personality CLI for ConfigCat, the Hungarian feature flag
//! service.
//!
//! Founded ~2018 by Endre Toth and team in Budapest, Hungary. Bootstrapped,
//! pricing-led, deliberately simple. Differentiator: 10 SDKs maintained by
//! a small team with a long-term-stable wire protocol and a non-confusing
//! pricing page (per-seat-or-free). 99.99% SLA, EU data residency available.
//! No experimentation, no analytics — strictly feature flags + targeting.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — ConfigCat feature flag service personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about       Hungary origins, bootstrapped, scope");
    println!("    flags       Flag types and the simple targeting model");
    println!("    sdks        Cross-language SDK lineup");
    println!("    cdn         Global CDN-served config + 99.99 SLA");
    println!("    sso         Identity provider integrations");
    println!("    pricing     Per-seat tiers, never per-flag");
    println!("    eu          EU data residency");
    println!("    integrations Slack, Zapier, Datadog, etc.");
    println!("    help        Show this help");
    println!("    version     Show version");
}

fn print_version() { println!("configcat-cli 0.1.0 (Budapest-bootstrapped personality build)"); }

fn run_about() {
    println!("ConfigCat (ConfigCat Kft.)");
    println!("  Founded:    ~2018, Budapest, Hungary.");
    println!("  Co-founder: Endre Toth (current CEO).");
    println!("  Funding:    Bootstrapped, no announced VC.");
    println!("  Scope:      Strictly feature flags + targeting.");
    println!("              No experimentation, no analytics.");
    println!("  SLA:        99.99% uptime, publicly reported.");
}

fn run_flags() {
    println!("Flag types:");
    println!("  Boolean         on/off.");
    println!("  Number          numeric variations.");
    println!("  String          textual variations.");
    println!("  Targeting rules can reference user attributes plus a built-in");
    println!("  'percentage option' for staged rollouts.");
    println!("  Sticky behaviour via a hashed user-identifier.");
}

fn run_sdks() {
    println!("SDK matrix (10+ official):");
    println!("  .NET / Java / Node / JS browser / React / Angular / Vue");
    println!("  Python / Ruby / PHP / Go / Elixir / Kotlin / Swift / Dart");
    println!("Wire protocol is intentionally stable to avoid breaking SDKs.");
    println!("Open source SDKs on GitHub under configcat/.");
}

fn run_cdn() {
    println!("Global delivery:");
    println!("  Flag config served from a CDN with PoPs across continents.");
    println!("  SDKs cache the config locally and refresh on a polling cadence");
    println!("  or via push (webhook -> SDK invalidate).");
    println!("  99.99% uptime SLA, transparent status page.");
}

fn run_sso() {
    println!("SSO / Identity:");
    println!("  Google Workspace, Microsoft Entra ID (Azure AD),");
    println!("  Okta, GitHub OAuth, SAML 2.0.");
    println!("  Available from the Smart tier upward.");
}

fn run_pricing() {
    println!("Pricing model (the pitch is simplicity):");
    println!("  Free          unlimited flags, 10 team members, 2 environments.");
    println!("  Pro           per-seat, more environments, MFA, audit log.");
    println!("  Smart         per-seat, SSO, scheduling, integrations.");
    println!("  Enterprise    custom, dedicated cluster, advanced compliance.");
    println!("Never per-flag, never per-event — only seats. Predictable.");
}

fn run_eu() {
    println!("EU data residency.");
    println!("  Customers can choose an EU data centre for flag config storage");
    println!("  and request handling, separate from the global default.");
    println!("  Targets GDPR-strict customers (German/Nordic public sector,");
    println!("  finance, healthcare) that won't tolerate US data plane.");
}

fn run_integrations() {
    println!("Integrations:");
    println!("  Slack notifications on flag changes.");
    println!("  Datadog events for change correlation.");
    println!("  Trello/Jira card sync for flag-to-ticket linking.");
    println!("  Zapier and Make.com for general no-code automations.");
    println!("  GitHub/GitLab/Bitbucket code-reference scanning.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "configcat-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "flags" => run_flags(),
        "sdks" => run_sdks(),
        "cdn" => run_cdn(),
        "sso" => run_sso(),
        "pricing" => run_pricing(),
        "eu" => run_eu(),
        "integrations" => run_integrations(),
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
        run_flags();
        run_sdks();
        run_cdn();
        run_sso();
        run_pricing();
        run_eu();
        run_integrations();
    }

    #[test]
    fn help_and_version() {
        print_help("configcat-cli");
        print_version();
    }
}
