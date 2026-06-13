#![deny(clippy::all)]

//! workos-cli — SlateOS WorkOS (developer-friendly enterprise-readiness API: SSO, SCIM, audit logs)
//!
//! Single personality: `workos`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wos(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: workos [OPTIONS]");
        println!("WorkOS (SlateOS) — Enterprise readiness APIs for SaaS apps");
        println!();
        println!("Options:");
        println!("  --sso                  SSO (SAML/OIDC, all enterprise IdPs)");
        println!("  --directory-sync       SCIM directory sync");
        println!("  --audit-logs           Audit Logs (SIEM-format event ingest + retrieval)");
        println!("  --authkit              AuthKit (hosted auth UI)");
        println!("  --fga                  FGA (Fine-Grained Authorization, Google Zanzibar-style)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("WorkOS 2024 (SlateOS)"); return 0; }
    println!("WorkOS 2024 (SlateOS)");
    println!("  Vendor: WorkOS Inc. (San Francisco — founded 2019)");
    println!("  Founder: Michael Grinich (ex-Cofounder Nylas, MIT)");
    println!("          Grinich: prolific essayist/blog poster, advocate for 'SaaS enterprise readiness'");
    println!("  Funding: Lachy Groom, Lightspeed, Benchmark + Stripe");
    println!("          ~$80M raised, est. ~$1B valuation 2024");
    println!("  Strategy: 'Stripe for SSO' — APIs that let a SaaS startup sell into the enterprise overnight");
    println!("           NOT a full IdP — sits BETWEEN apps and customer-provided IdPs (Okta/Entra/etc.)");
    println!("  Pricing: SSO from $125/mo per connection (each customer IdP), then volume pricing");
    println!("          Directory Sync from $125/mo per connection");
    println!("          Audit Logs from $0.001/event");
    println!("          AuthKit (basic auth UI) FREE up to 1M MAU");
    println!("          FGA (Fine-Grained Authz) free preview");
    println!("  Customers: OpenAI, Vercel, Plaid, Webflow, Loom, Cursor, Anthropic Console, Perplexity");
    println!("            sweet spot: developer-tooling startups going upmarket to enterprise");
    println!("  Core products:");
    println!("    1. SSO — one API call → connect any customer SAML/OIDC IdP");
    println!("       supports: Okta, Entra ID/Azure AD, Google Workspace, OneLogin, JumpCloud, ADFS, generic SAML, generic OIDC");
    println!("    2. Directory Sync — SCIM 2.0 endpoint hosted by WorkOS → maps to your user DB via webhooks");
    println!("       supports: Azure AD, Okta, Google Workspace, OneLogin, Rippling, JumpCloud, BambooHR + others");
    println!("    3. Magic Link — passwordless email link auth");
    println!("    4. AuthKit — hosted login UI with social, password, magic link, MFA, passkeys");
    println!("    5. Organizations — multi-tenant user model with org-level connection settings");
    println!("    6. Admin Portal — pre-built UI where customer's IT admin configures their SSO/SCIM (no engineering work)");
    println!("    7. Audit Logs — pump events into WorkOS → customers query/export via API (SIEM-ready)");
    println!("    8. FGA (Fine-Grained Authorization) — Google Zanzibar-style relation tuples, $100K MAU free tier");
    println!("  Killer feature — Admin Portal:");
    println!("    your enterprise customer's IT admin gets a self-service URL");
    println!("    they configure their Okta/Entra/Google Workspace → SSO works");
    println!("    NO engineering work on your side per customer IdP");
    println!("  Competitive position vs Auth0/Okta: 'AGI-friendly' — WorkOS doesn't try to BE the IdP, it lets customers bring their own");
    println!("  Critique: per-connection pricing adds up fast at scale");
    println!("           less of a 'full identity platform' — opinionated about staying in lane");
    println!("           competitors: Stytch, Clerk, FrontEgg, Userfront, ScaleKit");
    println!("  Differentiator: best-in-class developer experience + Admin Portal removes per-customer engineering");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "workos".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wos(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wos};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/workos"), "workos");
        assert_eq!(basename(r"C:\bin\workos.exe"), "workos.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("workos.exe"), "workos");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wos(&["--help".to_string()], "workos"), 0);
        assert_eq!(run_wos(&["-h".to_string()], "workos"), 0);
        let _ = run_wos(&["--version".to_string()], "workos");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wos(&[], "workos");
    }
}
