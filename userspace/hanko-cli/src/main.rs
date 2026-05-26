#![deny(clippy::all)]
//! hanko-cli — personality CLI for Hanko, the passkeys-first OSS auth
//! platform.
//!
//! Founded 2020 in Heidelberg, Germany by Felix Magedanz. Hanko's
//! positioning is the bluntest in the auth market: passwords are obsolete,
//! and the product is built around WebAuthn / passkeys as the primary
//! login factor, with email-OTP fallback for first-time enrol and
//! recovery. The flagship hanko-elements package is a set of web
//! components (`<hanko-auth>`, `<hanko-profile>`) that drop a complete
//! passkey-first login UI into any web page with a single import.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Hanko passkeys-first OSS auth personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Magedanz, Heidelberg, passkeys-first thesis");
    println!("    passkeys      WebAuthn / FIDO2 as primary factor");
    println!("    elements      <hanko-auth> web components");
    println!("    backend       Self-host the Hanko backend in Go");
    println!("    cloud         Hanko Cloud managed service");
    println!("    sdks          Web, mobile, backend integrations");
    println!("    pricing       OSS free + Cloud tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("hanko-cli 0.1.0 (passkeys-first personality build)"); }

fn run_about() {
    println!("Hanko GmbH.");
    println!("  Founded:    2020, Heidelberg, Germany.");
    println!("  Founder:    Felix Magedanz (CEO).");
    println!("  Funding:    Pre-seed + seed from EU angels and small funds.");
    println!("  Identity:   'The future is passwordless. Build for that future");
    println!("              today.'");
    println!("  Stance:     Passkeys are the primary factor, not an opt-in.");
    println!("  Licence:    AGPL-3.0 backend; MIT for hanko-elements UI lib.");
    println!("  GitHub:     hanko/hanko, hanko/hanko-elements.");
}

fn run_passkeys() {
    println!("Passkeys — first-class, not bolted on.");
    println!("  WebAuthn / FIDO2 spec compliant.");
    println!("  Hybrid transport: cross-device passkey sync via QR + Bluetooth.");
    println!("  Platform authenticators: Touch ID, Windows Hello, Android.");
    println!("  Roaming authenticators: YubiKey, Titan, SoloKey.");
    println!("  Fallback enrol: one-time code via email; never a password.");
    println!("  Conditional UI: browsers offer the passkey from autofill.");
}

fn run_elements() {
    println!("hanko-elements — drop-in web components.");
    println!("  <hanko-auth>     full login/registration flow as a custom element.");
    println!("  <hanko-profile>  account settings: list passkeys, rename, revoke.");
    println!("  Pure Web Components — work with React, Vue, Svelte, Solid,");
    println!("  vanilla HTML alike.");
    println!("  Themable via CSS custom properties; no fork required.");
    println!("  Total size on the wire: small enough to ship critical-path.");
}

fn run_backend() {
    println!("Hanko backend — Go service.");
    println!("  Stateless API: REST + JSON, easy to scale horizontally.");
    println!("  Storage: PostgreSQL.");
    println!("  Sessions: JWT signed with rotating keys.");
    println!("  Webhooks: user.created, user.login, passkey.added.");
    println!("  Identity Providers: social OIDC (Google, Apple, Microsoft, GitHub).");
    println!("  Self-host: docker compose stack ships with sane defaults.");
}

fn run_cloud() {
    println!("Hanko Cloud — managed.");
    println!("  EU-hosted by default (good fit for GDPR-strict deployments).");
    println!("  Same APIs as the OSS backend; trivial migration both ways.");
    println!("  Adds: custom domains, analytics, audit log retention.");
    println!("  SLAs at paid tiers.");
}

fn run_sdks() {
    println!("SDK matrix:");
    println!("  Web        hanko-elements (web components) + hanko-frontend-sdk.");
    println!("  Mobile     iOS Swift, Android Kotlin SDKs for native passkey UX.");
    println!("  Backend    Go (first-class), Node, Python, Java starters.");
    println!("  Frameworks Examples for Next.js, Nuxt, SvelteKit, Remix.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Self-host       OSS (AGPL backend, MIT UI), free forever.");
    println!("  Cloud Free      generous tier, EU-hosted.");
    println!("  Cloud Pro       per-MAU, custom domain, analytics, support.");
    println!("  Enterprise      custom contracts, SLAs, BAA, dedicated infra.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  EU-focused B2C apps with GDPR + data-residency requirements.");
    println!("  Developer-tool startups wanting passkey-only UX from day one.");
    println!("  Public references from German SaaS + ecommerce companies.");
    println!("  Heavy adoption inside the WebAuthn / passkeys community.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "hanko-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "passkeys" => run_passkeys(),
        "elements" => run_elements(),
        "backend" => run_backend(),
        "cloud" => run_cloud(),
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
        run_passkeys();
        run_elements();
        run_backend();
        run_cloud();
        run_sdks();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("hanko-cli");
        print_version();
    }
}
