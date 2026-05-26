#![deny(clippy::all)]
//! supertokens-cli — personality CLI for SuperTokens, the open-source
//! authentication platform that competes with Auth0/Cognito at a developer
//! level.
//!
//! Founded 2020 by Rishabh Poddar and Advait Ruia. Apache 2.0 core, with a
//! managed cloud and an Enterprise tier. Distinct architecture: SDKs talk to
//! a SuperTokens Core service the customer self-hosts or rents from the
//! managed cloud. Stores users in the customer's own database (Postgres
//! or MySQL) so the user table never leaves the customer's environment.
//! Backed by Y Combinator (W20) and small seed rounds.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — SuperTokens OSS auth personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, YC W20, OSS posture");
    println!("    recipes       Email/password, passwordless, social, phone");
    println!("    architecture  Backend SDK + Frontend SDK + Core service");
    println!("    selfhost      Self-host vs Managed");
    println!("    sessions      Rotating refresh tokens with anti-theft");
    println!("    multitenant   Built-in tenant + organization model");
    println!("    pricing       Free OSS, Managed per-MAU");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("supertokens-cli 0.1.0 (Apache-2.0 auth core personality build)"); }

fn run_about() {
    println!("SuperTokens (SuperTokens Inc.)");
    println!("  Founded:  2020.");
    println!("  Founders: Rishabh Poddar (CEO), Advait Ruia (CTO).");
    println!("  YC:       W20.");
    println!("  License:  Apache 2.0 for SDKs + Core.");
    println!("  Source:   github.com/supertokens/supertokens-core");
    println!("  Pitch:    Auth that lives in your stack: SDKs + a Core service");
    println!("            that reads/writes your own database, no vendor lock.");
}

fn run_recipes() {
    println!("Recipes (auth flavours):");
    println!("  EmailPassword         classic email + password.");
    println!("  Passwordless          magic link or OTP via email/SMS.");
    println!("  ThirdParty            Google, GitHub, Apple, Facebook, etc.");
    println!("  ThirdPartyEmailPassword combo, link by email.");
    println!("  Phone Password        SMS-OTP + password.");
    println!("  Session               session-only, plug into custom auth.");
    println!("  MFA                   TOTP / WebAuthn / passcode email.");
    println!("Recipes compose: combine ThirdParty + EmailPassword + MFA.");
}

fn run_architecture() {
    println!("Architecture: three pieces.");
    println!("  Frontend SDK     React/Vue/Angular/Svelte/iOS/Android.");
    println!("                   Drops sign-in/sign-up screens into the app.");
    println!("  Backend SDK      Node, Python, Go.");
    println!("                   Runs in the customer's API server, validates");
    println!("                   sessions, calls the Core for user mgmt.");
    println!("  Core             Java service, the source of truth.");
    println!("                   Talks to Postgres/MySQL (the user table).");
    println!("Customer can run Core on their own infra OR use Managed.");
}

fn run_selfhost() {
    println!("Self-host vs Managed:");
    println!("  Self-host       free, Apache-2.0 Core, customer runs the");
    println!("                  service + chooses the DB. Full data control.");
    println!("  Managed         hosted Core, customer's DB still possible via");
    println!("                  BYO connection string, or use SuperTokens-");
    println!("                  managed Postgres.");
}

fn run_sessions() {
    println!("Sessions: rotating refresh tokens with anti-theft.");
    println!("  Short-lived access token (5 min default).");
    println!("  Refresh token rotated on every use.");
    println!("  If a stolen refresh token is reused, the chain is detected");
    println!("  and all sessions for that user are invalidated.");
    println!("  Inspired by OAuth's RFC 6819 anti-replay guidance.");
}

fn run_multitenant() {
    println!("Multi-tenancy is built in.");
    println!("  First-class Tenant concept.");
    println!("  Per-tenant connection configs and login methods.");
    println!("  Organization-style B2B mappings: user belongs to N tenants.");
    println!("  Targets B2B SaaS apps that need per-customer SSO/SAML.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Self-Hosted (OSS)     free.");
    println!("  Managed Free          generous MAU on managed cloud.");
    println!("  Managed Paid          per-MAU above free tier.");
    println!("  Multi-Tenancy add-on  flat fee for production tenants.");
    println!("  Custom Enterprise     SOC 2, dedicated infra, SLAs.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Trustana, Cherry, IoT-startups, dev-tool startups.");
    println!("  Heavy hobbyist + indie dev community on GitHub.");
    println!("  Several mid-size B2B SaaS using multi-tenancy.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "supertokens-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "recipes" => run_recipes(),
        "architecture" => run_architecture(),
        "selfhost" => run_selfhost(),
        "sessions" => run_sessions(),
        "multitenant" => run_multitenant(),
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
        run_recipes();
        run_architecture();
        run_selfhost();
        run_sessions();
        run_multitenant();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("supertokens-cli");
        print_version();
    }
}
