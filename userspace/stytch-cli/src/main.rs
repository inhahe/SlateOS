#![deny(clippy::all)]
//! stytch-cli — OurOS Stytch developer-first auth platform personality CLI.

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn print_help(prog: &str) {
    println!("{prog} — Stytch developer-first authentication platform.");
    println!();
    println!("USAGE:");
    println!("    {prog} <subcommand> [args...]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    about           Stytch's story, founders, and funding");
    println!("    products        Consumer Auth, B2B SaaS Auth, Connected Apps, Fraud & Risk");
    println!("    primitives      Composable auth primitives — the Stytch philosophy");
    println!("    methods         Magic Links, OTP, WebAuthn/Passkeys, OAuth, SAML, SSO");
    println!("    sdks            Backend SDKs, frontend SDKs, headless components");
    println!("    b2b             Organizations, RBAC, SSO, SCIM, JIT provisioning");
    println!("    pricing         Free tier and growth plans");
    println!("    customers       Notable Stytch deployments");
    println!("    differentiator  Why API-first beats components-first for some teams");
    println!("    critique        Honest tradeoffs of choosing Stytch");
    println!("    help            Show this help");
    println!("    version         Show version");
}

fn print_about() {
    println!("Stytch — auth infrastructure built for builders.");
    println!();
    println!("Founded 2020 in San Francisco by Reed McGinley-Stempel (CEO) and");
    println!("Julianna Lamb (CTO), both veterans of Plaid where they built the");
    println!("identity verification platform for financial-services onboarding.");
    println!("They left Plaid believing that auth was still stuck in a 2012 mental");
    println!("model — username/password, session cookies, server-rendered login");
    println!("forms — while the rest of the stack had moved on to APIs, JAMstack,");
    println!("React, edge functions, and mobile-first. Stytch's thesis: rebuild");
    println!("auth from the ground up as composable primitives, not as a black-box");
    println!("hosted page you embed in an iframe.");
    println!();
    println!("Funding: $124M+ across Seed (2020), Series A ($30M Apr 2021, led by");
    println!("Thrive Capital), and Series B ($90M Mar 2022 at ~$1B valuation, led");
    println!("by Coatue with Thrive, Benchmark, Index Ventures participating).");
    println!("The Series B unicorn pricing put Stytch on the same trajectory as");
    println!("Auth0's mid-2010s growth — but with a fundamentally different");
    println!("product approach.");
    println!();
    println!("Headcount ~150. Headquartered in SF SoMa with remote engineering.");
    println!("Customer count crossed 10,000+ developers signed up by 2024, with");
    println!("hundreds of paying customers ranging from solo founders on the free");
    println!("tier to public companies running production auth on Stytch APIs.");
}

fn print_products() {
    println!("Stytch product surfaces:");
    println!();
    println!("• B2C (Consumer Authentication)");
    println!("    Auth for end-user-facing apps. Email magic links, SMS/WhatsApp/email");
    println!("    OTP, embeddable Passkeys (WebAuthn), OAuth (Google, Apple, Microsoft,");
    println!("    Facebook, GitHub, Discord, Slack, Coinbase, Twitch, etc.), session");
    println!("    management. Powers consumer apps that want passwordless-first UX.");
    println!();
    println!("• B2B SaaS Authentication");
    println!("    Multi-tenant auth designed for B2B SaaS from day one. Organizations");
    println!("    as first-class objects, member invitations, JIT provisioning,");
    println!("    organization-scoped SSO (SAML + OIDC), SCIM directory sync, RBAC");
    println!("    with custom roles and resources, just-in-time member creation.");
    println!("    Competes directly with WorkOS and Clerk's B2B mode.");
    println!();
    println!("• Connected Apps");
    println!("    Stytch as identity provider for your own customers. Turn your B2B");
    println!("    SaaS into an OAuth provider so third-party apps and AI agents can");
    println!("    log into your platform on behalf of users. Critical for the");
    println!("    emerging AI-agent-as-user economy.");
    println!();
    println!("• Fraud & Risk (Stytch Strong CAPTCHA, Device Fingerprinting)");
    println!("    Acquired SuperAGI device fingerprinting tech. Distinguishes humans");
    println!("    from bots without CAPTCHAs, blocks credential stuffing, scrapes,");
    println!("    fake account creation. Sold standalone or bundled with auth.");
    println!();
    println!("• M2M (Machine-to-Machine)");
    println!("    OAuth 2.0 client credentials for service-to-service auth, API");
    println!("    token management, scoped permissions for backend integrations.");
}

fn print_primitives() {
    println!("Stytch's design philosophy: composable primitives, not components.");
    println!();
    println!("Where Clerk and Auth0 ship pre-built UI components and a hosted");
    println!("login page, Stytch ships APIs and headless SDKs. The thesis: every");
    println!("serious product team wants pixel-perfect control over their auth");
    println!("UX. The login screen is often the first impression of your brand;");
    println!("handing it to a vendor's component library is a tradeoff worth");
    println!("rejecting if you have any design budget.");
    println!();
    println!("What Stytch provides:");
    println!("  • REST API + gRPC for every auth operation");
    println!("  • Idiomatic backend SDKs (Node, Python, Go, Ruby, Java, .NET)");
    println!("  • Frontend SDKs (vanilla JS, React, React Native, iOS, Android)");
    println!("  • Headless React components — logic without styling");
    println!("  • Optional pre-built UI (Stytch B2B UI components) for teams that");
    println!("    want a fast start and customize later");
    println!();
    println!("What you build yourself:");
    println!("  • The login screen (or use the headless components)");
    println!("  • The signup flow ordering");
    println!("  • The session-handoff between your frontend and backend");
    println!("  • Brand and copy and animations");
    println!();
    println!("The contract: Stytch handles the cryptographic plumbing, token");
    println!("issuance, OAuth dances, WebAuthn challenges, SAML assertion");
    println!("parsing, session storage. You handle the UI and product logic.");
}

fn print_methods() {
    println!("Authentication methods supported by Stytch:");
    println!();
    println!("• Email Magic Links");
    println!("    Click-to-login. Stytch generates a signed one-time URL, emails");
    println!("    it via your branded sender (or Stytch's). Token-based, anti-");
    println!("    replay, configurable TTL. The default UX for B2C in 2025.");
    println!();
    println!("• OTP (One-Time Passcodes)");
    println!("    Email, SMS, WhatsApp delivery. 6-digit numeric by default,");
    println!("    configurable length and alphabet. Rate-limited and replay-");
    println!("    protected at the API layer.");
    println!();
    println!("• Passkeys (WebAuthn / FIDO2)");
    println!("    Platform authenticators (Touch ID, Face ID, Windows Hello) and");
    println!("    roaming keys (YubiKey). Cross-device sync via iCloud Keychain");
    println!("    or Google Password Manager. The phishing-resistant future of");
    println!("    consumer auth.");
    println!();
    println!("• OAuth Social Login");
    println!("    30+ providers: Google, Apple, Microsoft, Facebook, GitHub,");
    println!("    Discord, Slack, Twitch, Twitter/X, LinkedIn, Coinbase, Yahoo,");
    println!("    Snapchat, Spotify, TikTok, Figma, ClassLink, Salesforce, etc.");
    println!("    PKCE-enforced, automatic token refresh, profile sync.");
    println!();
    println!("• SAML SSO (B2B)");
    println!("    SP-initiated and IdP-initiated flows, encrypted assertions,");
    println!("    Okta/Azure AD/Google Workspace/OneLogin/Ping/JumpCloud tested.");
    println!();
    println!("• OIDC SSO (B2B)");
    println!("    Modern alternative to SAML for B2B SSO. Faster setup, JSON");
    println!("    over JWT, native to most modern IdPs.");
    println!();
    println!("• SCIM 2.0 (B2B)");
    println!("    Directory provisioning. Members created/updated/deactivated");
    println!("    automatically when IT admins update their IdP.");
    println!();
    println!("• TOTP");
    println!("    Authenticator-app second factors. Backup codes generated and");
    println!("    delivered as a recovery mechanism.");
    println!();
    println!("• Passwords (with breach detection)");
    println!("    For teams that need passwords: zxcvbn strength check, HIBP");
    println!("    leak check, Argon2id hashing. Stytch encourages moving away.");
    println!();
    println!("• Crypto Wallets");
    println!("    Sign-in-with-Ethereum (EIP-4361), Solana wallets, Sui. For");
    println!("    Web3 apps without re-inventing wallet auth.");
}

fn print_sdks() {
    println!("Stytch SDK matrix:");
    println!();
    println!("Backend SDKs (server-side, holds the secret API key):");
    println!("  • Node.js / TypeScript — official, most-used");
    println!("  • Python — official, asyncio-first");
    println!("  • Go — official, idiomatic context.Context throughout");
    println!("  • Ruby — official");
    println!("  • Java — official, Spring-friendly");
    println!("  • .NET / C# — official");
    println!();
    println!("Frontend SDKs (browser/native, uses publishable token):");
    println!("  • JavaScript (vanilla) — works in any browser app");
    println!("  • React — hooks + headless components");
    println!("  • React Native — iOS + Android via JSI bridge");
    println!("  • iOS (Swift) — native SDK, Combine + async/await");
    println!("  • Android (Kotlin) — coroutine-first");
    println!();
    println!("UI Components:");
    println!("  • Headless React components — logic only, you bring the styles");
    println!("  • B2B UI Components — fully-styled but white-label-able drop-in");
    println!("    organization switcher, member management, SSO discovery");
    println!();
    println!("Code generation:");
    println!("  • OpenAPI spec published, generates clients in any language");
    println!("  • Postman collection maintained officially");
}

fn print_b2b() {
    println!("Stytch B2B SaaS Authentication — multi-tenant auth done right.");
    println!();
    println!("The model: Organizations are first-class entities. A user is a");
    println!("Member of one or more Organizations. Every auth event is scoped");
    println!("to an Organization. SSO configurations, RBAC policies, session");
    println!("policies, allowed auth methods — all per-Organization.");
    println!();
    println!("Core B2B primitives:");
    println!("  • Organizations: tenant boundary, branding, settings, SSO config");
    println!("  • Members: users inside Organizations, with roles and statuses");
    println!("  • Invitations: pending memberships sent via email");
    println!("  • Roles: built-in (admin, member) + unlimited custom roles");
    println!("  • Resources: RBAC objects with actions (e.g., 'documents:read')");
    println!("  • JIT Provisioning: auto-create members on first SSO login");
    println!("  • Discovery: 'what orgs can this email log into?' for IdP-init");
    println!("  • Just-in-Time Membership: invite-by-domain, anyone @acme.com");
    println!("  • Step-up Authentication: require MFA for sensitive operations");
    println!();
    println!("B2B-specific authentication flows:");
    println!("  • Organization-scoped SSO (each tenant configures their own IdP)");
    println!("  • Magic links scoped to a specific Organization");
    println!("  • OAuth identity linking across Organizations");
    println!("  • SCIM-managed Organizations where IT controls memberships");
    println!();
    println!("Why this matters: building B2B auth on a B2C primitive (single");
    println!("global user pool) requires inventing all of this yourself. Stytch");
    println!("B2B and competitors (WorkOS, Clerk B2B) sell exactly the");
    println!("infrastructure that you'd otherwise spend a year recreating.");
}

fn print_pricing() {
    println!("Stytch pricing (USD, 2025 list pricing):");
    println!();
    println!("Consumer Auth:");
    println!("  • Free — up to 10,000 MAUs (monthly active users)");
    println!("      All auth methods, unlimited orgs (B2C: just users), email/SMS");
    println!("      bundled with reasonable rate limits, community support");
    println!();
    println!("  • Growth — usage-based starting after 10K MAU");
    println!("      $0.05/MAU 10K-25K, declining tiers above");
    println!("      Optional adds: SMS at carrier cost passthrough, WhatsApp");
    println!();
    println!("  • Enterprise — custom");
    println!("      Volume discounts, SLA, dedicated support, deployment regions");
    println!();
    println!("B2B SaaS Auth:");
    println!("  • Free — up to 25 Organizations, 1,000 monthly active Members");
    println!("  • Growth — $249/mo, includes 50 Organizations + 5K MAMs (Members)");
    println!("  • Enterprise — custom, with SCIM, advanced RBAC, audit logs");
    println!();
    println!("Add-on products:");
    println!("  • Device Fingerprinting — $0.0006-$0.002/check tiered");
    println!("  • Strong CAPTCHA — per-check pricing");
    println!("  • Connected Apps — usage-based");
    println!();
    println!("Honest take: Stytch's free tier is generous enough that hobbyists");
    println!("and small SaaS can ship to production without paying. The B2B");
    println!("$249/mo entry is competitive with WorkOS and substantially below");
    println!("Auth0's B2B add-on pricing.");
}

fn print_customers() {
    println!("Notable Stytch customers (public references):");
    println!();
    println!("  • Replicate (ML model hosting) — B2B auth + SSO");
    println!("  • Curated.com — consumer auth via magic links");
    println!("  • Decagon (AI customer support) — B2B auth + SSO + RBAC");
    println!("  • Brilliant (education) — passwordless consumer auth");
    println!("  • Rye (commerce API) — B2B auth for merchants");
    println!("  • Notable Health — magic-link auth for HIPAA workflows");
    println!("  • Hex Technologies (data notebooks) — B2B SSO + RBAC");
    println!("  • Convex (backend platform) — auth for developer dashboard");
    println!("  • Mentava (children's reading) — passwordless family auth");
    println!("  • Truebill / Rocket Money — consumer auth (legacy migration)");
    println!();
    println!("Pattern: dev-tools companies, B2B SaaS with technical buyers,");
    println!("AI-first startups that need M2M + Connected Apps, and consumer");
    println!("apps where the founders care about login UX as a brand surface.");
}

fn print_differentiator() {
    println!("Why teams choose Stytch over alternatives:");
    println!();
    println!("vs. Auth0 (Okta):");
    println!("  • Auth0 is API-first too but priced for the enterprise");
    println!("  • Stytch's free tier is dramatically more generous");
    println!("  • Stytch's docs are noticeably more dev-friendly");
    println!("  • Stytch's B2B is native; Auth0 grafts orgs onto a B2C model");
    println!("  • Auth0 has more enterprise checkboxes (Universal Login, Actions");
    println!("    Hooks, Rules engine); Stytch counts that complexity as a bug");
    println!();
    println!("vs. Clerk:");
    println!("  • Clerk leads with pre-built React components; Stytch leads");
    println!("    with APIs and headless primitives");
    println!("  • Clerk excels for Next.js + Vercel teams that want auth done");
    println!("    in 10 minutes with default UI; Stytch wins for teams that");
    println!("    care about pixel-perfect custom auth UX");
    println!("  • Both have strong B2B offerings; Stytch's API surface is");
    println!("    arguably more composable, Clerk's UI surface is more polished");
    println!();
    println!("vs. WorkOS:");
    println!("  • WorkOS is laser-focused on B2B enterprise features (SSO,");
    println!("    SCIM, audit logs, directory sync)");
    println!("  • Stytch covers all that plus consumer auth methods");
    println!("  • WorkOS sometimes used in addition to a B2C auth (Stytch +");
    println!("    WorkOS combo for early-stage going up-market)");
    println!("  • Stytch's bet: you'll outgrow needing two vendors");
    println!();
    println!("vs. Firebase Auth / Supabase Auth:");
    println!("  • Firebase locks you into Google; Supabase locks you to Supabase");
    println!("  • Stytch is BYO database, BYO backend, BYO frontend — pure");
    println!("    auth infrastructure with no platform lock-in");
    println!();
    println!("vs. building it yourself:");
    println!("  • You'll spend 6 months and ship a worse version");
    println!("  • Stytch's compliance (SOC 2 Type II, HIPAA, GDPR) is included");
    println!("  • Every passkey edge case has been hit and fixed already");
}

fn print_critique() {
    println!("Honest tradeoffs of choosing Stytch:");
    println!();
    println!("• Smaller ecosystem than Auth0/Clerk. Fewer third-party tutorials,");
    println!("  Stack Overflow answers, and prebuilt integrations. You're more");
    println!("  often a first-mover for an unusual config.");
    println!();
    println!("• Headless-first philosophy means more work for teams that wanted");
    println!("  a drop-in login page. The B2B UI Components help but aren't as");
    println!("  polished as Clerk's component library.");
    println!();
    println!("• Documentation has historically been good for getting started but");
    println!("  thinner on advanced edge cases (multi-region deployments,");
    println!("  custom session-token JWT signing, legacy migration playbooks).");
    println!();
    println!("• Pricing is transparent but the B2B 'per Organization' model can");
    println!("  surprise you if you have many small tenants (one customer = one");
    println!("  Org, even if they have 2 members). Cost crosses Auth0 at scale.");
    println!();
    println!("• Stytch has less brand recognition with security/compliance buyers");
    println!("  at large enterprises. Procurement may insist on Okta/Ping/Auth0");
    println!("  because they've heard the name. Mitigated by Stytch's SOC 2 +");
    println!("  HIPAA + GDPR posture, but cultural inertia is real.");
    println!();
    println!("• Connected Apps is newer (2024) and the docs reflect that. If");
    println!("  you're building an OAuth IdP for AI agents to log into, expect");
    println!("  to file support tickets and influence the product direction.");
    println!();
    println!("• No on-prem deployment. Stytch is SaaS-only. Air-gapped");
    println!("  enterprises and FedRAMP-heavy buyers need alternative vendors.");
}

fn run_stytch(args: &[String], prog: &str) -> i32 {
    if args.is_empty() {
        print_help(prog);
        return 0;
    }
    match args[0].as_str() {
        "help" | "--help" | "-h" => { print_help(prog); 0 }
        "version" | "--version" | "-V" => {
            println!("{prog} 0.1.0 (OurOS personality CLI)");
            0
        }
        "about" => { print_about(); 0 }
        "products" => { print_products(); 0 }
        "primitives" => { print_primitives(); 0 }
        "methods" => { print_methods(); 0 }
        "sdks" => { print_sdks(); 0 }
        "b2b" => { print_b2b(); 0 }
        "pricing" => { print_pricing(); 0 }
        "customers" => { print_customers(); 0 }
        "differentiator" | "diff" => { print_differentiator(); 0 }
        "critique" => { print_critique(); 0 }
        other => {
            eprintln!("{prog}: unknown subcommand '{other}'");
            eprintln!("Try '{prog} help' for usage.");
            2
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "stytch".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(run_stytch(&rest, &prog));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basename() {
        assert_eq!(basename("/usr/bin/stytch"), "stytch");
        assert_eq!(basename("stytch"), "stytch");
        assert_eq!(basename("C:\\bin\\stytch.exe"), "stytch.exe");
    }

    #[test]
    fn test_strip_ext() {
        assert_eq!(strip_ext("stytch.exe"), "stytch");
        assert_eq!(strip_ext("stytch"), "stytch");
    }

    #[test]
    fn test_help_runs() {
        let _ = run_stytch(&[], "stytch");
        assert_eq!(run_stytch(&["help".to_string()], "stytch"), 0);
    }

    #[test]
    fn test_unknown_subcommand() {
        assert_eq!(run_stytch(&["nonsense".to_string()], "stytch"), 2);
    }
}
