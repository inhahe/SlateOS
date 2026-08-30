#![deny(clippy::all)]
//! fusionauth-cli — personality CLI for FusionAuth, the on-prem-friendly
//! customer identity platform.
//!
//! Founded 2017 by Brian Pontarelli in Broomfield, Colorado, as a spin-out
//! from Inversoft (his earlier company, behind CleanSpeak content moderation
//! and Passport — the Java auth library that became FusionAuth's core).
//! FusionAuth's distinctive position: identity software you can download
//! and run on your own infrastructure, including fully air-gapped, with
//! per-instance licensing instead of per-MAU SaaS pricing.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — FusionAuth self-hostable identity personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Pontarelli, Inversoft lineage, Colorado");
    println!("    deploy        Self-host, air-gap, Docker, kubernetes");
    println!("    tenants       Multi-tenant identity model");
    println!("    apps          Per-application OIDC/SAML config");
    println!("    lambdas       Server-side JS hooks");
    println!("    themes        Per-tenant themable login UIs");
    println!("    pricing       Per-instance licence, not per-MAU");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("fusionauth-cli 0.1.0 (self-host personality build)"); }

fn run_about() {
    println!("FusionAuth (Inversoft, Inc.)");
    println!("  Founded:   2017 — spun out from Inversoft's Passport library.");
    println!("  Founder:   Brian Pontarelli (CEO), previously Inversoft + CleanSpeak.");
    println!("  HQ:        Broomfield / Denver area, Colorado.");
    println!("  Funding:   Bootstrapped from Inversoft revenue; no VC.");
    println!("  Identity:  'Auth that you can download'. On-prem first.");
    println!("  Heritage:  Passport (the Java auth library) became");
    println!("             FusionAuth's core — 10+ years of production use");
    println!("             before the rebrand.");
}

fn run_deploy() {
    println!("Deployment topology — self-host first.");
    println!("  Download: tar.gz, deb, rpm, docker image, kubernetes helm chart.");
    println!("  Database: PostgreSQL or MySQL — bring your own.");
    println!("  Search:   Elasticsearch/OpenSearch (optional, for advanced search).");
    println!("  Air-gap:  Fully supported. No call-home, no licence ping needed");
    println!("            for the free Community edition.");
    println!("  Cloud:    FusionAuth Cloud (managed) is an option, not the only one.");
    println!("  HA:       Stateless app nodes; scale horizontally behind LB.");
}

fn run_tenants() {
    println!("Multi-tenant identity model.");
    println!("  Single FusionAuth instance can host many tenants.");
    println!("  Each tenant: isolated users, apps, themes, email templates.");
    println!("  Tenants share underlying DB but are logically isolated.");
    println!("  Use case: agency hosts auth for many client SaaS apps from one box.");
}

fn run_apps() {
    println!("Applications and grants.");
    println!("  An 'application' = an OAuth2/OIDC client.");
    println!("  Per-app: client_id, client_secret, redirect URIs, scopes, roles.");
    println!("  SAML 2.0 service-provider endpoints also per-app.");
    println!("  Registration concept: a user 'registers' for an application;");
    println!("  a tenant user without registrations cannot access that app.");
}

fn run_lambdas() {
    println!("FusionAuth Lambdas — server-side JS hooks.");
    println!("  Lambdas run inside FusionAuth at well-defined extension points:");
    println!("  - JWT populate (add claims at token mint time)");
    println!("  - SAML response populate");
    println!("  - User reconcile from social/SAML/OIDC IdP");
    println!("  - LDAP search transform");
    println!("  Pure JS (Nashorn / GraalJS), sandboxed, no native calls.");
}

fn run_themes() {
    println!("Themes — per-tenant or per-app login UI.");
    println!("  FreeMarker templates with CSS/JS assets.");
    println!("  Override individual pages: login, register, 2FA, password reset.");
    println!("  Themes are versioned; preview and roll back from admin UI.");
    println!("  Reactor (Pro) edition adds advanced theme management.");
}

fn run_pricing() {
    println!("Pricing model — instance-based, not per-MAU.");
    println!("  Community   free forever, self-hosted, all core features.");
    println!("  Starter     paid, per-instance, adds advanced features.");
    println!("  Essentials  per-instance, adds threat detection, breached pw.");
    println!("  Enterprise  per-instance, adds SCIM, advanced lambdas, SLA.");
    println!("Notable: no MAU caps in the self-hosted Community edition.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Regulated industries: defence contractors, healthcare,");
    println!("  financial services that need air-gapped identity.");
    println!("  Indie devs and small teams using the free Community edition");
    println!("  for greenfield SaaS without infra cost.");
    println!("  Public case studies on fusionauth.io.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fusionauth-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "deploy" => run_deploy(),
        "tenants" => run_tenants(),
        "apps" => run_apps(),
        "lambdas" => run_lambdas(),
        "themes" => run_themes(),
        "pricing" => run_pricing(),
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
        run_deploy();
        run_tenants();
        run_apps();
        run_lambdas();
        run_themes();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("fusionauth-cli");
        print_version();
    }
}
