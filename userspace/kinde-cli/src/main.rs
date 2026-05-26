#![deny(clippy::all)]
//! kinde-cli — personality CLI for Kinde, the Australian "business OS for
//! startups" centred on authentication.
//!
//! Founded 2021 by Ryan Dawson and team in Melbourne, Australia. Dawson
//! previously founded and exited a Melbourne ecommerce SaaS (Receive a
//! Click? Actually, he founded Receival Group / had multiple SaaS exits).
//! Kinde is unusual in framing itself as a 'business operating system' —
//! auth is the first product, with billing, feature flags, and customer
//! analytics positioned as adjacent modules. Generous free tier (10,500
//! MAU) targeting indie founders pre-product-market-fit.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Kinde 'business OS for startups' personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Melbourne, framing");
    println!("    auth          Sign-up/sign-in flows and methods");
    println!("    orgs          Multi-org model for B2B SaaS");
    println!("    billing       Billing module on top of auth");
    println!("    flags         Feature flags + permissions tied to plan");
    println!("    sdks          Languages and frameworks supported");
    println!("    pricing       10,500 MAU free, then per-MAU tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("kinde-cli 0.1.0 (Melbourne 'business OS' personality build)"); }

fn run_about() {
    println!("Kinde (Kinde Pty Ltd)");
    println!("  Founded:   2021, Melbourne, Australia.");
    println!("  Founder:   Ryan Dawson (CEO), previously SaaS founder/operator.");
    println!("  Pitch:     'Business OS for startups' — auth as the first");
    println!("             pillar, with billing + flags + analytics as");
    println!("             integrated modules built on the same identity store.");
    println!("  Audience:  Indie founders, small B2B SaaS, fast-moving startups");
    println!("             that want one place for identity + plan management.");
}

fn run_auth() {
    println!("Auth surface:");
    println!("  Email + password, magic link, social (Google/GitHub/Apple/MS).");
    println!("  Passkeys (WebAuthn) on the modern path.");
    println!("  MFA: TOTP and SMS.");
    println!("  Custom domains for the auth screen.");
    println!("  Drop-in hosted UI or fully embeddable components.");
}

fn run_orgs() {
    println!("Orgs (organisations).");
    println!("  B2B-shaped multi-tenancy: a user belongs to N orgs.");
    println!("  Each org has its own users, roles, permissions, branding.");
    println!("  Per-org SSO/SAML for enterprise customers (paid tier).");
    println!("  Switch org via dropdown in the hosted UI.");
}

fn run_billing() {
    println!("Billing — the differentiating module.");
    println!("  Plans + meters defined in Kinde, synced to Stripe.");
    println!("  Subscription state lives next to the user identity, so an");
    println!("  app can ask Kinde 'is this user on the Pro plan?' as part of");
    println!("  the same auth/permissions check.");
    println!("  Sells against the typical 'wire Stripe + Auth0 + DB myself' path.");
}

fn run_flags() {
    println!("Feature flags + permissions.");
    println!("  Permissions belong to roles; roles belong to plan tiers.");
    println!("  A feature flag can gate on plan tier directly.");
    println!("  No external feature-flag vendor needed for the simple cases.");
    println!("  Heavier experimentation customers still use LaunchDarkly/Statsig.");
}

fn run_sdks() {
    println!("SDK matrix:");
    println!("  JavaScript + React + Vue + Angular + Next.js + Nuxt + SvelteKit.");
    println!("  Node, Python, PHP, Go, .NET, Ruby, Java, Elixir.");
    println!("  iOS Swift, Android Kotlin, React Native, Flutter.");
    println!("  All SDKs are OpenID Connect compliant under the hood.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free        ~10,500 MAU, unlimited apps, MFA included.");
    println!("  Plus        per-MAU above free band; multi-org, custom domain.");
    println!("  Scale       per-MAU; SSO/SAML for B2B; advanced security.");
    println!("  Enterprise  custom; SLAs, dedicated success engineering.");
    println!("Free tier is deliberately wide to win indie founders early.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Indie founders + early-stage YC alumni + ANZ startups.");
    println!("  Various Vercel/Netlify Jamstack apps using Kinde as drop-in.");
    println!("  Public list at kinde.com/customers when published.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "kinde-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "auth" => run_auth(),
        "orgs" => run_orgs(),
        "billing" => run_billing(),
        "flags" => run_flags(),
        "sdks" => run_sdks(),
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
        run_auth();
        run_orgs();
        run_billing();
        run_flags();
        run_sdks();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("kinde-cli");
        print_version();
    }
}
