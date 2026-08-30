#![deny(clippy::all)]
//! frontegg-cli — personality CLI for Frontegg, the Tel Aviv user-management
//! platform aimed squarely at B2B SaaS.
//!
//! Founded 2019 by Sagi Rodin and Aviad Mizrachi in Tel Aviv. Raised a $40M
//! Series B in May 2022 led by Insight Partners. Differentiator: a built-in
//! "Admin Portal" — a hosted, embeddable, white-labelled settings UI where
//! end-customer admins of a B2B SaaS can self-serve SSO, SCIM, audit log,
//! roles, MFA, and API keys for their own organisation. Frontegg's pitch
//! is that B2B SaaS founders should not be building this admin surface
//! themselves.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Frontegg B2B user management personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Tel Aviv, B2B SaaS focus");
    println!("    adminportal   The embeddable admin portal — the moat");
    println!("    tenants       Multi-tenant by default");
    println!("    sso           Per-tenant SSO/SAML configuration self-serve");
    println!("    scim          User provisioning via SCIM 2.0");
    println!("    audit         Audit log surfaced to end customers");
    println!("    pricing       Per-MAU + per-tenant");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("frontegg-cli 0.1.0 (Admin-Portal personality build)"); }

fn run_about() {
    println!("Frontegg, Ltd.");
    println!("  Founded:    2019, Tel Aviv, Israel.");
    println!("  Founders:   Sagi Rodin (CEO), Aviad Mizrachi (CTO).");
    println!("  Funding:    $40M Series B May 2022 led by Insight Partners.");
    println!("              ~$70M total raised.");
    println!("  Pitch:      'User management platform for B2B SaaS'. Not just");
    println!("              auth — also tenant admin, SSO, SCIM, audit, all");
    println!("              wrapped in an embeddable admin portal.");
}

fn run_adminportal() {
    println!("Admin Portal — the differentiator.");
    println!("  An embeddable, white-labelled settings UI for the B2B SaaS");
    println!("  customer's own end-customer admins.");
    println!("  Pages: Users, Groups, Roles, SSO, SCIM, MFA, Audit Log, API Keys.");
    println!("  Each tenant admin self-serves their organisation's config");
    println!("  without filing tickets with the SaaS vendor.");
    println!("  Frontegg's pitch: stop reinventing this in every B2B app.");
}

fn run_tenants() {
    println!("Multi-tenant by default.");
    println!("  Every user is scoped to one or more Accounts (tenants).");
    println!("  Resources, roles, and feature toggles live per-account.");
    println!("  Per-tenant branding (logo, primary colour, custom domain).");
    println!("  Cross-tenant queries blocked at the platform level.");
}

fn run_sso() {
    println!("SSO/SAML — self-serve.");
    println!("  Per-tenant SAML 2.0 and OpenID Connect configuration.");
    println!("  Tenant admins paste their IdP metadata into the Admin Portal");
    println!("  and turn on SSO themselves — no support ticket round-trip.");
    println!("  Auto-provisioning of users on first SSO login.");
    println!("  Strict domain enforcement (only logins from my domain).");
}

fn run_scim() {
    println!("SCIM 2.0 user provisioning.");
    println!("  Each tenant gets a SCIM endpoint + bearer token.");
    println!("  IdPs (Okta, Entra ID, Google) push user/group changes to");
    println!("  Frontegg, which mirrors them into the tenant's user store.");
    println!("  Required by enterprise procurement; Frontegg ships it for free");
    println!("  on the relevant tier.");
}

fn run_audit() {
    println!("Audit log — surfaced to end customers.");
    println!("  Every user/admin action gets an audit event.");
    println!("  The Admin Portal exposes audit logs to tenant admins so they");
    println!("  can prove SOC 2 / ISO 27001 controls to their auditors.");
    println!("  Logs are filterable, paginated, exportable as CSV.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free            7,500 MAU free, basic auth + Admin Portal.");
    println!("  Pro             per-MAU + paid add-ons for SSO/SCIM/audit.");
    println!("  Scale           per-MAU + larger quotas + advanced security.");
    println!("  Enterprise      custom: dedicated tenancy, audit log retention.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Talkdesk, Snyk, Hibob, Salesloft (B2B SaaS scaleups).");
    println!("  Various Israeli + US Series-B-and-up B2B SaaS companies.");
    println!("  Frontegg's GTM is heavily into mid-market B2B founders.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "frontegg-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "adminportal" => run_adminportal(),
        "tenants" => run_tenants(),
        "sso" => run_sso(),
        "scim" => run_scim(),
        "audit" => run_audit(),
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
        run_adminportal();
        run_tenants();
        run_sso();
        run_scim();
        run_audit();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("frontegg-cli");
        print_version();
    }
}
