#![deny(clippy::all)]
//! ory-cli — personality CLI for Ory, the open-source identity infrastructure
//! stack: Kratos, Hydra, Keto, Oathkeeper.
//!
//! Founded 2015 in Munich by Aeneas Rekkas and Thomas Aidan Curran. Ory's
//! distinctive design: instead of one monolithic identity product, four
//! focused, composable OSS daemons. Hydra is an OAuth2/OIDC provider with
//! zero opinion on the login UI; Kratos handles user management and
//! flows; Keto implements Google Zanzibar-style fine-grained authorisation;
//! Oathkeeper is an identity-aware reverse proxy. The Ory Network is the
//! managed cloud on top of the same OSS components.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Ory open-source identity stack personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Rekkas+Curran, Munich, OSS stack");
    println!("    hydra         OAuth2/OIDC server, BYO login UI");
    println!("    kratos        User mgmt + identity flows");
    println!("    keto          Zanzibar-style fine-grained AuthZ");
    println!("    oathkeeper    Identity-aware reverse proxy");
    println!("    network       Ory Network managed cloud");
    println!("    pricing       OSS + per-tier Network plans");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("ory-cli 0.1.0 (four-daemon OSS personality build)"); }

fn run_about() {
    println!("Ory Corp.");
    println!("  Founded:    2015, Munich, Germany.");
    println!("  Founders:   Aeneas Rekkas (CTO), Thomas Aidan Curran (CEO).");
    println!("  Funding:    Seed and Series A from European VCs; modest size.");
    println!("  Identity:   'Open source, hardened in production'.");
    println!("  OSS stack:  Hydra (OAuth2), Kratos (identity), Keto (authZ),");
    println!("              Oathkeeper (proxy). All Apache 2.0.");
    println!("  GitHub:     Tens of thousands of stars across the four repos.");
    println!("  Language:   Go (all four daemons).");
}

fn run_hydra() {
    println!("Ory Hydra — OAuth2 / OIDC provider.");
    println!("  Standards: OAuth2 (all relevant RFCs), OpenID Connect Certified.");
    println!("  Opinion:   Hydra has NO login UI. The consent + login app is");
    println!("             your problem — Hydra calls out to your UI via");
    println!("             well-defined redirect handshakes.");
    println!("  Storage:   PostgreSQL, MySQL, CockroachDB, SQLite.");
    println!("  Tokens:    Opaque or JWT access tokens, refresh, ID tokens.");
    println!("  Throughput: Designed for stateless horizontal scaling.");
}

fn run_kratos() {
    println!("Ory Kratos — identity and user management.");
    println!("  Self-service flows: registration, login, recovery, settings,");
    println!("  email/phone verification, 2FA enrolment, account linking.");
    println!("  Strategies: password, OIDC social, WebAuthn passkeys, TOTP, lookup");
    println!("  secrets, code via email/SMS.");
    println!("  Identity schemas: JSON-schema-defined, per-deployment custom.");
    println!("  Pairs naturally with Hydra to add OAuth2 on top.");
}

fn run_keto() {
    println!("Ory Keto — fine-grained authorisation.");
    println!("  Implements Google Zanzibar: relation tuples define who has");
    println!("  what permission on which object.");
    println!("  Example tuple: 'user:alice is member of group:eng' +");
    println!("                 'group:eng has editor on document:42'.");
    println!("  Queries: check, expand, list-objects, list-subjects.");
    println!("  Performance: millions of tuples, sub-millisecond check.");
}

fn run_oathkeeper() {
    println!("Ory Oathkeeper — identity-aware reverse proxy + access decision API.");
    println!("  Sits in front of your services.");
    println!("  Per-route rules: who can call, what auth method, what to do");
    println!("  with the request (mutate headers, drop, allow).");
    println!("  Authenticators: bearer JWT, OAuth2 introspection, cookie session.");
    println!("  Authorizers: deny, allow, remote JSON, Keto relation check.");
    println!("  Mutators: header, cookie, ID token, no-op.");
}

fn run_network() {
    println!("Ory Network — managed cloud version of the OSS stack.");
    println!("  Same APIs as self-hosted; the daemons run on Ory's infra.");
    println!("  Custom domains, SOC 2, global low-latency endpoints.");
    println!("  Migration story: trivial, since the OSS and Network expose");
    println!("  the same HTTP+gRPC interfaces.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  OSS         Apache 2.0, self-host the four daemons free forever.");
    println!("  Developer   Network: free tier with low usage caps.");
    println!("  Production  Network: per-MAU/per-request, includes SLAs.");
    println!("  Enterprise  custom: dedicated infra, BAA, audit log retention.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Cloud-native enterprises wanting OAuth2 without licence cost.");
    println!("  Several large fintech, telco, and gov adopters of Hydra.");
    println!("  Heavy use inside the German + EU developer scene.");
    println!("  Various open-source projects use Hydra as their IdP.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "ory-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "hydra" => run_hydra(),
        "kratos" => run_kratos(),
        "keto" => run_keto(),
        "oathkeeper" => run_oathkeeper(),
        "network" => run_network(),
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
        run_hydra();
        run_kratos();
        run_keto();
        run_oathkeeper();
        run_network();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("ory-cli");
        print_version();
    }
}
