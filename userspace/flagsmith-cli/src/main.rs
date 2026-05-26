#![deny(clippy::all)]
//! flagsmith-cli — personality CLI for Flagsmith, the open-source feature
//! flag and remote configuration service.
//!
//! Founded 2018 by Ben Rometsch and Kyle Johnson in London. Originally
//! branded "Bullet Train" before renaming to Flagsmith in 2020. Operated by
//! Solid State Group as a SaaS product alongside agency work; later spun
//! into its own venture. License: BSD-3 server + SDKs (with an Enterprise
//! commercial feature set). Strong story for self-hosting on Kubernetes
//! including airgap installs for regulated industries.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Flagsmith feature flags + remote config personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Bullet Train -> Flagsmith, London, BSD-3");
    println!("    flags         Flags and remote-config values");
    println!("    identities    Per-user overrides");
    println!("    segments      Saved cohorts + traits");
    println!("    architecture  Django + Postgres + Redis core");
    println!("    selfhost      Open source vs Cloud vs Enterprise");
    println!("    integrations  Slack, Datadog, Mixpanel, Amplitude, ...");
    println!("    pricing       Free OSS, Cloud, Enterprise");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("flagsmith-cli 0.1.0 (Bullet-Train-lineage personality build)"); }

fn run_about() {
    println!("Flagsmith Ltd.");
    println!("  Founded:  2018, London, UK.");
    println!("  Founders: Ben Rometsch (CEO), Kyle Johnson.");
    println!("  Origin:   Initially branded 'Bullet Train' as part of");
    println!("            Solid State Group (a UK product studio).");
    println!("  Rebrand:  Flagsmith, 2020.");
    println!("  License:  BSD-3 for server + SDKs.");
    println!("  Source:   github.com/Flagsmith/flagsmith");
    println!("  Stars:    ~5,000+ on GitHub.");
}

fn run_flags() {
    println!("Flags + remote configuration:");
    println!("  Boolean        on/off.");
    println!("  String         e.g. CSS theme name, copy variant.");
    println!("  Integer        e.g. rate limit, page size.");
    println!("  JSON-as-string for structured configuration.");
    println!("Each flag has per-environment defaults plus overrides.");
    println!("Flags can be associated with multi-variate values for A/B.");
}

fn run_identities() {
    println!("Identities — Flagsmith's per-user override model.");
    println!("  Identities are first-class objects with traits (attributes).");
    println!("  Operators can override a flag value for one identity directly");
    println!("  in the UI (great for opt-in beta testing of a single user).");
    println!("  Identities flow through SDK calls; same identity gets same value.");
}

fn run_segments() {
    println!("Segments — saved cohorts.");
    println!("  Defined by trait rules: equals, contains, gt, lt, regex,");
    println!("                          percentage_split, modulo, semver_*.");
    println!("  Reusable across flags within a project.");
    println!("  Environment-scoped or project-scoped overrides.");
}

fn run_architecture() {
    println!("Architecture:");
    println!("  Core API     Python / Django.");
    println!("  Database     Postgres (Aurora in Cloud).");
    println!("  Cache        Redis.");
    println!("  Frontend     React admin UI.");
    println!("  Edge         optional Edge API (DynamoDB-backed) for fast SDK reads.");
    println!("  Real-time    SSE channels for SDK live updates.");
    println!("Deploy as Docker Compose, Kubernetes (official Helm chart),");
    println!("or via Cloud.");
}

fn run_selfhost() {
    println!("Editions:");
    println!("  Open Source       BSD-3 server, self-host without limit.");
    println!("  Cloud Start-Up    free tier with generous request quota.");
    println!("  Cloud Scale-Up    per-seat + API request bands.");
    println!("  Cloud Enterprise  SSO, audit, advanced approvals,");
    println!("                    dedicated cluster option.");
    println!("  On-prem Enterprise self-host with vendor support + airgap.");
}

fn run_integrations() {
    println!("Integrations:");
    println!("  Slack, Microsoft Teams — flag change notifications");
    println!("  Datadog, New Relic, Grafana — flag-change events for correlation");
    println!("  Mixpanel, Amplitude, Heap, Segment — event analytics");
    println!("  GitHub, GitLab, Bitbucket — code reference scanning");
    println!("  Jira — flag-to-ticket linking");
    println!("  AWS, GCP, Azure — secret store integrations");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Open Source    free forever, self-host, BSD-3.");
    println!("  Start-Up Cloud free tier with monthly API quota.");
    println!("  Scale-Up Cloud per-seat + request band.");
    println!("  Enterprise     custom contract for SSO, audit, on-prem,");
    println!("                 airgap deploys, dedicated success engineering.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "flagsmith-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "flags" => run_flags(),
        "identities" => run_identities(),
        "segments" => run_segments(),
        "architecture" => run_architecture(),
        "selfhost" => run_selfhost(),
        "integrations" => run_integrations(),
        "pricing" => run_pricing(),
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
        run_identities();
        run_segments();
        run_architecture();
        run_selfhost();
        run_integrations();
        run_pricing();
    }

    #[test]
    fn help_and_version() {
        print_help("flagsmith-cli");
        print_version();
    }
}
