#![deny(clippy::all)]
//! propelauth-cli — personality CLI for PropelAuth, the indie B2B auth
//! product opinionated around the multi-organisation user model.
//!
//! Founded 2021 by Andrew Israel (ex-Foursquare engineer). PropelAuth was
//! built explicitly for B2B SaaS where every end-user belongs to one or
//! more customer organisations with their own roles and SSO config.
//! Bootstrapped / lightly-funded; the company runs as a small team with
//! detailed engineering blog posts as primary developer marketing. Pricing
//! is a flat free tier (up to 10K MAU) followed by per-MAU bands, with
//! SSO/SAML, SCIM, and audit log all included on the paid tiers (not
//! priced separately as 'enterprise add-ons').

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — PropelAuth B2B-first auth personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Andrew Israel, ex-Foursquare, B2B focus");
    println!("    orgs          Multi-org model first-class");
    println!("    portals       Hosted pages (login + org settings)");
    println!("    rbac          Roles and permissions API");
    println!("    sso           SAML/OIDC included on paid tiers");
    println!("    apikeys       Per-org API key issuance");
    println!("    pricing       Flat free + per-MAU, no SSO tax");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("propelauth-cli 0.1.0 (no-SSO-tax personality build)"); }

fn run_about() {
    println!("PropelAuth (PropelAuth, Inc.)");
    println!("  Founded:   2021.");
    println!("  Founder:   Andrew Israel (CEO), ex-Foursquare engineer.");
    println!("  Funding:   Bootstrapped / lightly-funded; small team.");
    println!("  Pitch:     'B2B authentication, not B2C with B2B bolted on'.");
    println!("  Anti-tax:  Includes SSO/SAML, SCIM, audit on Pro tier — not");
    println!("             priced separately as the typical Auth0/Okta");
    println!("             'enterprise add-on'.");
    println!("  Marketing: Detailed engineering blog at propelauth.com/blog");
    println!("             is the primary inbound channel.");
}

fn run_orgs() {
    println!("Multi-org model — the central design choice.");
    println!("  Every user belongs to N organisations.");
    println!("  Org has: name, custom URL slug, members, roles, SSO config.");
    println!("  Roles default to Owner / Admin / Member but are customisable.");
    println!("  All APIs return data scoped by org context, not just user.");
    println!("  Org switching is a first-class hosted UI affordance.");
}

fn run_portals() {
    println!("Hosted portals — both end-user-facing and admin-facing.");
    println!("  Login portal: sign-up, login, password reset, MFA enrol.");
    println!("  Org settings portal: invite members, change roles, configure");
    println!("  SSO, view audit log, generate API keys.");
    println!("  Both are white-labelled with the SaaS customer's branding.");
    println!("  Customers can embed via iframe or redirect to hosted URLs.");
}

fn run_rbac() {
    println!("Role-based access control.");
    println!("  Roles defined at the project level (e.g. Owner, Admin, Member).");
    println!("  Permissions can be associated with roles for fine-grained checks.");
    println!("  Backend SDKs expose helpers like .has_permission(user, perm, org).");
    println!("  Custom roles per organisation are supported via Pro tier.");
}

fn run_sso() {
    println!("SSO/SAML — included on paid tiers.");
    println!("  Per-org SAML 2.0 setup; org admin can self-serve metadata.");
    println!("  OIDC also supported.");
    println!("  IdPs verified against: Okta, Entra ID, Google, OneLogin, JumpCloud.");
    println!("  Auto-create users on first SSO login.");
}

fn run_apikeys() {
    println!("Per-org API keys.");
    println!("  Each organisation can mint API keys scoped to that org's resources.");
    println!("  Useful for B2B SaaS exposing APIs to its enterprise customers'");
    println!("  CI/CD pipelines without leaking cross-tenant data.");
    println!("  Issuance, rotation, revocation, audit all in the hosted portal.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free        up to 10,000 MAU, all features except SSO/SCIM.");
    println!("  Pro         per-MAU; includes SSO/SAML, SCIM, audit log,");
    println!("              custom roles, custom domains.");
    println!("  Enterprise  custom; HIPAA, BAA, dedicated infra.");
    println!("Notable: SSO and SCIM are NOT separate add-ons.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Indie + early-stage B2B SaaS that need SSO without the");
    println!("  Auth0 enterprise tier price tag.");
    println!("  Various Y Combinator + Hacker News dev-tool startups.");
    println!("  Several public case studies on propelauth.com.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "propelauth-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "orgs" => run_orgs(),
        "portals" => run_portals(),
        "rbac" => run_rbac(),
        "sso" => run_sso(),
        "apikeys" => run_apikeys(),
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
        run_orgs();
        run_portals();
        run_rbac();
        run_sso();
        run_apikeys();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("propelauth-cli");
        print_version();
    }
}
