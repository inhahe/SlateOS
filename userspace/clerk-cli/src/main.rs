#![deny(clippy::all)]

//! clerk-cli — Slate OS Clerk (modern React-first auth + user mgmt, SF, founded 2019)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_clerk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: clerk [OPTIONS]");
        println!("Clerk (Slate OS) — modern React-first auth + user management for full-stack apps");
        println!();
        println!("Options:");
        println!("  --components           Pre-built React components (SignIn, SignUp, UserButton, OrgSwitcher)");
        println!("  --organizations        Multi-tenant orgs + roles + invitations built-in");
        println!("  --b2b-saas             B2B SaaS feature set (orgs, SSO/SAML, RBAC)");
        println!("  --jwt                  JWT session tokens + verification helpers");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Clerk 2024 (Slate OS) — clerk CLI (next-gen auth)"); return 0; }
    println!("Clerk 2024 (Slate OS) — Modern React-First Auth + User Management Platform");
    println!("  Vendor: Clerk, Inc. (San Francisco, CA — private)");
    println!("  Founders: Colin Sidoti + Braden Sidoti + Brent Pinkerton, 2019");
    println!("          Colin Sidoti: CEO, ex-Aurelius (startup), Y Combinator W20 batch");
    println!("          Braden Sidoti: ex-Square engineering (brothers)");
    println!("          Brent Pinkerton: CTO co-founder");
    println!("          Founded in NYC, moved HQ to San Francisco");
    println!("          'Frustrated with Auth0 complexity, wanted React-native auth'");
    println!("          'Same auth UX as Notion / Linear / Stripe — but as a service'");
    println!("          Bootstrapped early, Y Combinator Winter 2020");
    println!("  Funding:");
    println!("         Total raised: ~$55M+");
    println!("         YC W20 + seed");
    println!("         Series A May 2022: $15M (Madrona)");
    println!("         Series B Feb 2024: $30M (CRV) at ~$300M valuation");
    println!("         Strong revenue growth trajectory");
    println!("         Smaller raise than Auth0 era — more capital efficient");
    println!("  Strategic position: 'React-first auth + user management built for modern web apps':");
    println!("                    pitch: 'auth that looks like Notion / Linear / Stripe — drop in, looks great'");
    println!("                    target: React/Next.js + Remix + Astro + Svelte developers, SaaS startups, B2B SaaS");
    println!("                    primary competitor: Auth0 (Okta), Supabase Auth, Stytch, Firebase Auth, NextAuth.js");
    println!("                    secondary: WorkOS (B2B), AuthKit (WorkOS), Kinde, FusionAuth");
    println!("                    Clerk wedge: best-in-class React component library (looks beautiful out of the box)");
    println!("                    + B2B-native (organizations + roles + invitations + SSO built-in)");
    println!("                    + Next.js + Remix first-class");
    println!("                    + drop-in <SignIn /> component vs assembling Lego pieces (Stytch style)");
    println!("                    Notion + Linear-quality UI default");
    println!("  Pricing (modern SaaS, transparent):");
    println!("    Clerk Free: free up to 10,000 MAU (Monthly Active Users)");
    println!("    Clerk Pro: $25/mo + $0.02/MAU beyond 10,000");
    println!("    Clerk Enhanced Auth: $100/mo + $0.05/MAU (SAML, custom domains)");
    println!("    Clerk Enterprise: custom pricing for SSO, SCIM, compliance");
    println!("    notably generous free tier (Auth0's free tier is much smaller)");
    println!("    'B2B SaaS' add-on for organizations features");
    println!("    'Like Stripe pricing' — clear MAU-based");
    println!("  Architecture (modern dev-first auth):");
    println!("    - Hosted SaaS multi-tenant");
    println!("    - React + Next.js + Remix SDKs (canonical)");
    println!("    - JS, Python, Ruby, Go, Express backends");
    println!("    - JWT session tokens (verifiable in your backend)");
    println!("    - JWKS endpoint for token verification");
    println!("    - Webhooks for sync to your DB");
    println!("    - Server actions (Next.js) + Edge runtime support");
    println!("    - Pre-built React components: <SignIn />, <UserButton />, etc.");
    println!("    - Tailwind-compatible styling");
    println!("  Product portfolio:");
    println!("    1. Pre-built React Components (the killer feature):");
    println!("       - <SignIn />, <SignUp />, <UserButton />, <UserProfile />");
    println!("       - <OrganizationSwitcher />, <OrganizationProfile />");
    println!("       - <SignedIn>, <SignedOut> conditional rendering");
    println!("       - Drop in, looks beautiful (Notion-quality)");
    println!("       - Themeable with appearance prop");
    println!("       - Mobile + desktop responsive out of the box");
    println!("    2. Authentication methods (all the modern ones):");
    println!("       - Email + password");
    println!("       - Email magic links (passwordless email)");
    println!("       - SMS OTP");
    println!("       - Email OTP");
    println!("       - OAuth (Google, GitHub, Discord, Apple, Microsoft, Slack, LinkedIn, Twitter/X, 30+ providers)");
    println!("       - Passkeys (WebAuthn/FIDO2) since 2023");
    println!("       - SAML SSO (Enterprise tier)");
    println!("    3. Multi-Factor Authentication:");
    println!("       - TOTP (Google Authenticator, Authy)");
    println!("       - SMS OTP");
    println!("       - Backup codes");
    println!("       - Passkeys as MFA");
    println!("    4. Organizations (B2B SaaS multi-tenancy):");
    println!("       - Built-in orgs/teams/workspaces");
    println!("       - Member roles + permissions");
    println!("       - Invitations + role-based access");
    println!("       - Org-level settings + branding");
    println!("       - 'Like Slack workspaces' built into your app");
    println!("    5. User Management UI:");
    println!("       - Clerk Dashboard: admin UI for users, orgs, sessions");
    println!("       - End-user <UserProfile /> component (lets users self-serve)");
    println!("    6. Sessions + JWT tokens:");
    println!("       - JWT session tokens issued by Clerk");
    println!("       - JWKS endpoint for backend verification");
    println!("       - Custom claims from organization context");
    println!("       - Session revocation + active session listing");
    println!("    7. Webhooks:");
    println!("       - Real-time sync to your DB");
    println!("       - user.created, user.updated, organization.created events");
    println!("       - Svix-powered (now Svix used widely in webhook industry)");
    println!("    8. Backend SDKs:");
    println!("       - clerkClient (Node) for server-side API calls");
    println!("       - Python (clerk-backend-api), Go, Ruby SDKs");
    println!("       - Middleware: Next.js middleware, Express middleware");
    println!("    9. Custom Sign-in Flows + Elements:");
    println!("       - For fully custom UI (without pre-built components)");
    println!("       - <SignIn.Step name='start'> primitives");
    println!("       - Compose your own sign-in UI with Clerk's logic");
    println!("    10. Production-grade compliance:");
    println!("       - SOC 2 Type II");
    println!("       - HIPAA compliance available");
    println!("       - GDPR + CCPA compliant");
    println!("       - SSO/SAML for enterprise customers");
    println!("       - SCIM 2.0 provisioning");
    println!("  The React-first design (the differentiator):");
    println!("    - Auth0 + Okta + Stytch = API-first (you build the UI)");
    println!("    - Clerk = component-first (UI built, you just drop it in)");
    println!("    - <ClerkProvider> wraps app");
    println!("    - <SignIn /> renders entire sign-in flow");
    println!("    - useUser() / useAuth() hooks give you user data");
    println!("    - For Next.js: auth() server function + middleware");
    println!("    - 'Auth in 5 minutes' for React/Next.js devs");
    println!("    - Trade-off: less flexibility if you want exotic UI");
    println!("  The Next.js partnership:");
    println!("    - Clerk + Vercel partnership 2022-2024");
    println!("    - Featured in Next.js docs as recommended auth");
    println!("    - Co-marketing at Vercel events");
    println!("    - Works seamlessly with Next.js App Router + middleware");
    println!("    - 'If you're on Next.js, Clerk is the default auth recommendation'");
    println!("  The organizations B2B SaaS angle:");
    println!("    - Most modern SaaS = multi-tenant with orgs/teams");
    println!("    - Auth0 + Okta require building orgs yourself");
    println!("    - Clerk has orgs as first-class: invitations, roles, permissions all built in");
    println!("    - Org Switcher component just works");
    println!("    - Major selling point for B2B SaaS startups");
    println!("    - 'Stripe gives you billing, Clerk gives you orgs'");
    println!("  Integrations:");
    println!("    - React, Next.js, Remix, Astro, Svelte, Vue, Nuxt, Expo first-class");
    println!("    - Backend SDKs: Node, Python, Go, Ruby");
    println!("    - 30+ OAuth providers (Google, GitHub, etc.)");
    println!("    - SAML 2.0 SSO + SCIM 2.0 (Enterprise tier)");
    println!("    - Supabase + Convex + PlanetScale integrations");
    println!("    - Stripe integration for paid orgs");
    println!("    - Webhooks via Svix");
    println!("    - Vercel + Cloudflare Workers integration");
    println!("    - Apollo + Hasura GraphQL integrations");
    println!("  Clerk CLI usage:");
    println!("    # Clerk CLI is mostly via dashboard + npx for boilerplate:");
    println!("    npx create-next-app@latest --example with-clerk-auth my-app");
    println!("    # Add to existing Next.js app:");
    println!("    npm install @clerk/nextjs");
    println!("    # Then in next.config or middleware:");
    println!("    # import {{ clerkMiddleware }} from '@clerk/nextjs/server';");
    println!("    # export default clerkMiddleware();");
    println!("    # Drop a SignIn component:");
    println!("    # import {{ SignIn }} from '@clerk/nextjs';");
    println!("    # export default function Page() {{ return <SignIn />; }}");
    println!("    # Backend API for admin tasks:");
    println!("    curl -H 'Authorization: Bearer <secret_key>' \\");
    println!("         https://api.clerk.com/v1/users");
    println!("    # User mgmt + org mgmt via Clerk Dashboard:");
    println!("    # https://dashboard.clerk.com");
    println!("    # Webhooks endpoint setup in dashboard");
    println!("  Customers (modern web apps + B2B SaaS):");
    println!("    - Cal.com (scheduling app, OSS)");
    println!("    - Drata (compliance automation)");
    println!("    - Inkeep, Resend, Tigris, Sequence");
    println!("    - Various YC startups");
    println!("    - Notion clones + Linear clones + AI startups");
    println!("    - Growing list of B2B SaaS at $5M+ ARR");
    println!("    - 'Default for Next.js + B2B SaaS startups'");
    println!("  Critique: opinionated component design (limits exotic UI)");
    println!("           pricing scales with MAU (gets expensive at scale, but transparent)");
    println!("           less mature than Auth0 for niche enterprise needs");
    println!("           SAML SSO + SCIM require higher-tier plan");
    println!("           backend SDK feature coverage varies by language");
    println!("           React-first means less natural for non-React backends");
    println!("           component theming has limits vs full custom UI");
    println!("           young company (founded 2019) = less battle-tested at extreme scale");
    println!("           vendor lock-in to Clerk's data model (orgs, sessions, etc.)");
    println!("           supports many providers but lacks Apigee-style custom OAuth flows");
    println!("  Differentiator: modern React-first auth + user management built for Next.js + Remix + Astro era (founded 2019 by Colin Sidoti + Braden Sidoti + Brent Pinkerton, YC W20, $55M+ raised including Madrona + CRV, ~$300M valuation Feb 2024) + pre-built React components that look beautiful out of the box (<SignIn /> + <SignUp /> + <UserButton /> + <UserProfile /> + <OrganizationSwitcher />, drop-in vs assembling primitives) + Organizations (first-class B2B SaaS multi-tenancy with invitations + roles + permissions + branding) + 30+ OAuth providers + email + SMS + magic links + TOTP + Passkeys + SAML SSO + SCIM 2.0 + Vercel/Next.js partnership (featured in Next.js docs as recommended auth) + JWT sessions + JWKS verification + webhooks via Svix + Clerk Dashboard admin UI + middleware for Next.js App Router + Svelte/Astro/Vue/Nuxt/Remix/Expo SDKs + Cal.com/Drata/Resend-proven + generous free tier (10K MAU free) + Pro $25/mo + transparent $0.02/MAU pricing + SOC 2 Type II + HIPAA + GDPR compliance + Custom Sign-in Elements (compose your own UI with Clerk logic) + Stripe-integration patterns + 'auth in 5 minutes for React/Next.js devs' + Notion/Linear/Stripe-quality UI default + the auth SaaS that emerged as Auth0 fatigue grew among modern dev teams — the most React-friendly auth + user management platform with the best out-of-the-box UI in the category");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "clerk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_clerk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_clerk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/clerk"), "clerk");
        assert_eq!(basename(r"C:\bin\clerk.exe"), "clerk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("clerk.exe"), "clerk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_clerk(&["--help".to_string()], "clerk"), 0);
        assert_eq!(run_clerk(&["-h".to_string()], "clerk"), 0);
        let _ = run_clerk(&["--version".to_string()], "clerk");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_clerk(&[], "clerk");
    }
}
