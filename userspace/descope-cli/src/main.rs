#![deny(clippy::all)]
//! descope-cli — personality CLI for Descope, the drag-and-drop authentication
//! flow builder.
//!
//! Founded 2022 by Slavik Markovich, Rishi Bhargava, and Meir Wahnon. The
//! team came out of Demisto/Palo Alto Networks (Bhargava was a Demisto
//! co-founder; that company was acquired by PANW for $560M in 2019).
//! Descope's twist on the auth market: a visual flow builder (Descope
//! Flows) where auth journeys — sign-up, sign-in, step-up, MFA, password
//! reset — are designed as drag-drop graphs with conditional branches.
//! Raised a $53M Series A in Mar 2022 (notable for being a Series A at
//! that size in stealth, before product launch).

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Descope drag-drop auth flows personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Founders, Demisto lineage, mega-Series-A");
    println!("    flows         Visual auth flow builder");
    println!("    methods       Passwordless, social, SAML, OTP, passkeys");
    println!("    tenancy       Multi-tenant identity model");
    println!("    riskengine    Adaptive risk + step-up");
    println!("    sdks          Languages and frameworks supported");
    println!("    pricing       Free 7,500 MAU, paid tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("descope-cli 0.1.0 (Flows-first personality build)"); }

fn run_about() {
    println!("Descope, Inc.");
    println!("  Founded:    2022, Los Altos / Tel Aviv.");
    println!("  Founders:   Slavik Markovich (CEO), Rishi Bhargava (CMO),");
    println!("              Meir Wahnon (CRO/COO).");
    println!("  Heritage:   Bhargava + Markovich previously founded Demisto");
    println!("              (SOAR vendor) acquired by Palo Alto Networks for");
    println!("              ~$560M in 2019.");
    println!("  Funding:    $53M Series A Mar 2022 — unusually large for");
    println!("              a pre-launch identity startup.");
    println!("  Investors:  Lightspeed, GGV, Dell Tech Capital, TCV.");
}

fn run_flows() {
    println!("Descope Flows — the marquee feature.");
    println!("  Visual graph of nodes: screen, condition, action, integration.");
    println!("  Examples of nodes: 'show passwordless screen', 'validate OTP',");
    println!("  'lookup user', 'call webhook', 'enroll TOTP', 'finish login'.");
    println!("  Conditions branch on user attributes, risk score, device, etc.");
    println!("  Flows are versioned, A/B-testable, and live-editable without");
    println!("  shipping a code release on the app side.");
}

fn run_methods() {
    println!("Authentication methods:");
    println!("  Passwordless     magic link, email/SMS OTP.");
    println!("  Passkeys         WebAuthn / FIDO2 first-class.");
    println!("  Social           Google, Apple, Facebook, GitHub, MS, LinkedIn.");
    println!("  SAML / OIDC      enterprise SSO inbound.");
    println!("  SAML / OIDC out  Descope as the IdP for downstream apps.");
    println!("  TOTP, SMS, voice OTP, biometric step-up.");
    println!("  Password         supported but de-emphasised.");
}

fn run_tenancy() {
    println!("Multi-tenant identity model.");
    println!("  Tenants are first-class objects.");
    println!("  Users can belong to multiple tenants with distinct roles.");
    println!("  Per-tenant flow overrides — enterprise tenant uses SAML,");
    println!("  consumer tenant uses passkeys + social.");
    println!("  Tenant SSO and SCIM provisioning supported.");
}

fn run_riskengine() {
    println!("Risk engine + step-up.");
    println!("  Built-in signals: device fingerprint, IP reputation, velocity.");
    println!("  Plug-ins for external risk feeds (Reblaze, Cloudflare Turnstile,");
    println!("  Arkose, hCaptcha).");
    println!("  Flows can branch on risk score: low -> let through, medium ->");
    println!("  require MFA, high -> deny + send to incident handler.");
}

fn run_sdks() {
    println!("SDK matrix:");
    println!("  Web        React, Vue, Angular, vanilla JS.");
    println!("  Mobile     iOS, Android, React Native, Flutter.");
    println!("  Backend    Node, Python, Go, Java, .NET, PHP, Ruby.");
    println!("  Web Components for plain-HTML drop-in screens.");
    println!("  Most flows can run entirely on the front end via the");
    println!("  Descope client SDK + Flow renderer.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Free            7,500 MAU, all auth methods, basic Flows.");
    println!("  Pro             per-MAU, multi-tenancy, social IDPs, SAML.");
    println!("  Business        per-MAU, advanced flows, support SLAs.");
    println!("  Enterprise      custom, custom domains, audit retention,");
    println!("                  dedicated environments, SOC 2 Type II reports.");
}

fn run_customers() {
    println!("Selected customers:");
    println!("  Various B2C consumer apps using passwordless flows.");
    println!("  Mid-market B2B SaaS using multi-tenant SSO.");
    println!("  Several public Descope reference customers in fintech +");
    println!("  ed-tech (e.g. Outschool-era startups, B2B fintech).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "descope-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "flows" => run_flows(),
        "methods" => run_methods(),
        "tenancy" => run_tenancy(),
        "riskengine" => run_riskengine(),
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
        run_flows();
        run_methods();
        run_tenancy();
        run_riskengine();
        run_sdks();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("descope-cli");
        print_version();
    }
}
