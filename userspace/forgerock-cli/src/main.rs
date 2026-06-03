#![deny(clippy::all)]

//! forgerock-cli — OurOS ForgeRock (Sun OpenAM heritage, Vancouver WA/Bristol UK, merged with Ping 2023)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_forgerock(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: forgerock [OPTIONS]");
        println!("ForgeRock (OurOS) — identity platform (now part of Ping Identity, Thoma Bravo)");
        println!();
        println!("Options:");
        println!("  --identitycloud        ForgeRock Identity Cloud (SaaS, integrating with PingOne)");
        println!("  --am                   Access Management (descendant of Sun OpenAM)");
        println!("  --idm                  Identity Management (descendant of Sun OpenIDM)");
        println!("  --ds                   Directory Services (descendant of Sun OpenDJ)");
        println!("  --autonomous           Autonomous Identity (ML access reviews)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("ForgeRock 2024 (OurOS) — forgerock-cli (Identity Cloud + AM admin)"); return 0; }
    println!("ForgeRock 2024 (OurOS) — Identity Platform (Sun OpenAM Heritage, Now Ping Identity)");
    println!("  Vendor: ForgeRock Inc. (San Francisco, CA + Bristol, UK — now part of Ping Identity / Thoma Bravo)");
    println!("  Founders: Lasse Andresen + Jamie Nelson + Hermann Wittmann + others, 2010");
    println!("          Lasse Andresen: Norwegian, ex-Sun Microsystems identity engineer");
    println!("          Jamie Nelson: ex-Sun identity engineer");
    println!("          Founded when Oracle acquired Sun (Apr 2009) + dropped Sun's open identity stack");
    println!("          'When Oracle killed Sun's open-source identity stack, ForgeRock forked it'");
    println!("          The forks: OpenAM, OpenIDM, OpenDJ, OpenIG");
    println!("          Built the company around continuing + commercializing the open Sun stack");
    println!("          Bristol UK + Norway + San Francisco distributed engineering");
    println!("          IPO Sep 2021 NYSE:FORG at $25/share (raised ~$275M)");
    println!("          Acquired private by Thoma Bravo Aug 2023 for $2.3B");
    println!("          Merging with Ping Identity (also Thoma Bravo) into single identity entity");
    println!("          Andre Durand (Ping founder) leading combined entity");
    println!("  History (the Sun-fork story):");
    println!("    - 2003: Sun acquired Waveset (identity management)");
    println!("    - 2005: Sun developed OpenAM (open-source access management)");
    println!("    - 2009: Oracle acquires Sun ($7.4B) — identity engineers worry");
    println!("    - 2010: Oracle slows/stops Sun's open-source identity commits");
    println!("    - 2010: ForgeRock founded by ex-Sun engineers — forks OpenAM");
    println!("    - 2011-2020: Decade of building commercial product on Sun heritage");
    println!("    - 2018: ForgeRock Identity Cloud SaaS launched");
    println!("    - 2021: IPO NYSE:FORG");
    println!("    - 2023: Thoma Bravo acquisition + Ping merger announced");
    println!("    - 'ForgeRock' name = forging a new path from the rock-solid Sun identity heritage");
    println!("  Strategic position (pre-merger, now subsumed):");
    println!("                    pitch (then): 'next-gen identity built on proven Sun open source heritage'");
    println!("                    target: digital identity at scale (CIAM, IoT, employee, partner)");
    println!("                    primary competitor: Okta, Ping Identity, Microsoft Entra, ForgeRock-now-Ping");
    println!("                    secondary: Auth0, SAP IDS, Salesforce Identity");
    println!("                    ForgeRock wedge: CIAM scale + identity tree visual flows + AI/ML insights");
    println!("                    + the only platform built from open-source identity heritage");
    println!("                    + intelligent authentication trees (drag-and-drop flow builder)");
    println!("                    + Norway/UK engineering + global open-source roots");
    println!("                    Now: 'ForgeRock products being integrated into Ping Identity portfolio'");
    println!("                    Post-merger combined entity: 'largest enterprise IDaaS outside Okta + Microsoft'");
    println!("  Pricing (was, pre-merger):");
    println!("    Identity Cloud: ~$1-10/identity/year (CIAM tier-based)");
    println!("    AM/IDM/DS on-prem: per-host or per-identity subscription");
    println!("    typically 6-7 figure annual deals at enterprise scale");
    println!("    pricing now being unified with PingOne pricing post-merger");
    println!("  Architecture (the open Sun heritage stack):");
    println!("    - Access Management (AM): SSO + auth + adaptive (Java, OpenAM descendant)");
    println!("    - Identity Management (IDM): provisioning + sync (Java, OpenIDM descendant)");
    println!("    - Directory Services (DS): LDAP-compliant directory (Java, OpenDJ descendant)");
    println!("    - Identity Gateway (IG): API security + reverse proxy (OpenIG descendant)");
    println!("    - Identity Cloud: SaaS hosting all components");
    println!("    - Autonomous Identity: ML layer over the platform");
    println!("    - All Java-based, deployed on-prem or as SaaS");
    println!("    - Common Audit Framework, Common REST framework");
    println!("  Product portfolio (the four-component platform):");
    println!("    1. Access Management (AM):");
    println!("       - SSO, MFA, federation (SAML, OIDC, OAuth, WS-Fed)");
    println!("       - Adaptive auth with context (device, location, risk score)");
    println!("       - Intelligent Authentication Trees (drag-and-drop flow builder)");
    println!("       - Sun OpenAM descendant — 20+ years of access management heritage");
    println!("       - Used for: customer SSO at scale (CIAM)");
    println!("    2. Identity Management (IDM):");
    println!("       - User provisioning, sync, lifecycle");
    println!("       - 80+ connectors (AD, LDAP, SaaS apps, mainframe)");
    println!("       - JavaScript-based workflows");
    println!("       - 'Reconciles' identities across systems");
    println!("       - Sun OpenIDM descendant");
    println!("    3. Directory Services (DS):");
    println!("       - LDAP-compliant directory (LDAPv3 + extensions)");
    println!("       - Scales to billions of identities");
    println!("       - Replication: multi-master + global");
    println!("       - Sun OpenDJ descendant (originally Sun DSEE)");
    println!("       - Used for: large consumer directories (telcos, governments)");
    println!("    4. Identity Gateway (IG):");
    println!("       - API gateway with identity awareness");
    println!("       - Reverse proxy for legacy app SSO");
    println!("       - Token translation (SAML to OIDC, etc.)");
    println!("       - Sun OpenIG descendant");
    println!("    5. ForgeRock Identity Cloud (SaaS, 2018+):");
    println!("       - Multi-tenant cloud hosting AM + IDM + DS");
    println!("       - Pay per identity model");
    println!("       - Lower ops burden than on-prem");
    println!("       - The flagship product of recent years");
    println!("       - Being integrated with PingOne post-merger");
    println!("    6. Autonomous Identity (AI/ML):");
    println!("       - ML-driven access analytics");
    println!("       - Recommends role memberships + revocations");
    println!("       - Detects anomalous access patterns");
    println!("       - Competes with: SailPoint AI Outliers, Saviynt");
    println!("    7. Intelligent Authentication Trees (the differentiator):");
    println!("       - Visual flow builder for auth journeys");
    println!("       - Drag-and-drop nodes: 'Username? -> Password? -> MFA? -> Risk score? -> Allow/Deny'");
    println!("       - Conditional branching, A/B testing, progressive profiling");
    println!("       - One of the most flexible auth journey builders on the market");
    println!("       - 'Trees' became a buzzword in identity orchestration");
    println!("    8. Identity Edge for IoT:");
    println!("       - Identity for connected devices (IoT, cars, industrial)");
    println!("       - Lightweight protocols (CoAP, MQTT integration)");
    println!("       - Provisioning + authentication for billions of devices");
    println!("    9. ForgeRock Authenticator (mobile MFA):");
    println!("       - Push, OTP, biometric MFA app");
    println!("       - Embedded in customer mobile apps via SDK");
    println!("    10. Identity Cloud Express (smaller-tier SaaS):");
    println!("       - Lower-cost SaaS tier for smaller customers");
    println!("       - Faster onboarding");
    println!("  The Sun heritage (the open-source roots):");
    println!("    - Sun Microsystems had a strong identity team in early 2000s");
    println!("    - OpenSSO (later OpenAM) was THE open SAML federation server");
    println!("    - When Oracle acquired Sun 2009 + slowed Sun's open identity development, ex-Sun engineers feared the projects would die");
    println!("    - ForgeRock forked and commercialized: 'we'll keep the open identity stack alive'");
    println!("    - For 13 years, ForgeRock = commercial steward of Sun open identity heritage");
    println!("    - Massive customer install base from Sun era — telcos, governments, banks");
    println!("    - 'When you have a Sun identity install, you eventually become a ForgeRock customer'");
    println!("  The intelligent authentication trees (the killer feature):");
    println!("    - Visual flow builder for any auth journey");
    println!("    - 'Login -> Risk score -> if risky, MFA; if very risky, deny'");
    println!("    - 'Register -> capture name -> email verify -> show ToS -> activate'");
    println!("    - One of the first identity orchestration tools that designers loved");
    println!("    - Now imitated by: Okta Workflows, Auth0 Forms + Actions, PingOne DaVinci");
    println!("  The merger with Ping:");
    println!("    - Aug 2023: Thoma Bravo announced ForgeRock merger into Ping Identity");
    println!("    - Combined entity has Ping branding, products integrated 2024-2026");
    println!("    - Customers being migrated: ForgeRock Identity Cloud → PingOne");
    println!("    - AM/IDM/DS → integrating with PingFederate/PingAccess/PingDirectory");
    println!("    - Andre Durand (Ping founder) returned as CEO of combined entity");
    println!("    - Net effect: largest pure-play IDaaS outside Okta + Microsoft");
    println!("  Integrations:");
    println!("    - ForgeRock CLI (frodo CLI, community + official)");
    println!("    - REST API for everything");
    println!("    - Terraform provider for Identity Cloud");
    println!("    - 80+ connectors for IDM");
    println!("    - SAML 2.0, OIDC, OAuth 2.0, WS-Federation");
    println!("    - LDAP v3 client compatibility");
    println!("    - Mobile SDKs (iOS, Android, Cordova, React Native)");
    println!("    - JavaScript + Java + .NET SDKs");
    println!("    - WebAuthn / FIDO2 support");
    println!("    - SIEM integration (Splunk, QRadar, Sentinel)");
    println!("    - Salesforce, ServiceNow, Workday, SAP integration");
    println!("  ForgeRock CLI usage:");
    println!("    # frodo CLI (open-source, popular):");
    println!("    frodo conn add my-tenant https://openam.example.com/am admin <password>");
    println!("    frodo journey list my-tenant                            # auth trees");
    println!("    frodo journey export my-tenant my-tree                  # export tree as JSON");
    println!("    frodo journey import my-tenant my-tree --file tree.json");
    println!("    frodo realm list");
    println!("    frodo app list                                          # OAuth clients");
    println!("    # AM CLI:");
    println!("    amster install --config /opt/ds/config");
    println!("    amster run --file setup.txt");
    println!("    # Identity Cloud REST:");
    println!("    curl -H 'X-Requested-With: forgerock-cli' \\");
    println!("         https://<tenant>.forgeblocks.com/openidm/managed/user?_queryFilter=true");
    println!("    # Migration to PingOne (post-merger):");
    println!("    # ForgeRock migration assistant tool available");
    println!("  Customers (CIAM + government + telcos):");
    println!("    - BBC, Toyota, Geico, Comcast, T-Mobile (consumer identity at scale)");
    println!("    - Pearson, McGraw-Hill (education)");
    println!("    - Norwegian government (BankID, MinID — Norwegian origin)");
    println!("    - Royal Bank of Canada, Lloyds (banks)");
    println!("    - Telia, Telenor, Vodafone (telcos — Sun heritage)");
    println!("    - US Federal: VA, DoD components (FedRAMP authorized)");
    println!("    - 1300+ enterprise customers globally");
    println!("    - Strong in EU (German, Nordic, UK) — UK + Norway HQs");
    println!("  Critique: now subsumed into Ping Identity — uncertain product future");
    println!("           Java-heavy stack feels dated next to cloud-native Auth0/Clerk/Stytch");
    println!("           4-product complexity (AM + IDM + DS + IG) = steep learning curve");
    println!("           on-prem deployments tedious to operate");
    println!("           Identity Cloud was catching up but merger pause is real");
    println!("           pricing per-identity expensive vs newer dev-first auth");
    println!("           migration to PingOne creates uncertainty for existing customers");
    println!("           Identity Gateway less popular than dedicated API gateways");
    println!("           Autonomous Identity less mature than SailPoint AI Outliers");
    println!("  Differentiator: Sun OpenAM/OpenIDM/OpenDJ/OpenIG heritage (founded 2010 by ex-Sun engineers Lasse Andresen + Jamie Nelson when Oracle slowed Sun's open identity development after $7.4B Sun acq, ForgeRock forked + commercialized to keep the open identity stack alive) + 4-component platform (Access Management for SSO/MFA/federation, Identity Management for provisioning/sync, Directory Services for LDAP-scale directories, Identity Gateway for API/legacy proxy) + Identity Cloud SaaS (2018+) + Autonomous Identity (ML access analytics) + Intelligent Authentication Trees (drag-and-drop visual auth journey builder, the killer feature imitated by Okta Workflows + Auth0 Actions + PingOne DaVinci) + Identity Edge for IoT (billions of devices) + ForgeRock Authenticator mobile MFA + IPO Sep 2021 NYSE:FORG $275M + Thoma Bravo $2.3B Aug 2023 acquisition + merger with Ping Identity announced (Andre Durand returned as CEO of combined entity, ForgeRock products integrating into Ping portfolio) + BBC/Toyota/Geico/Comcast/T-Mobile/RBC/Lloyds/Telia/Telenor/Vodafone-proven + Norwegian government BankID + 1300+ enterprise customers + 80+ IDM connectors + Java-based + frodo open-source CLI + LDAPv3 directory scale to billions of identities + UK + Norway engineering heritage + the bridge from Sun's open identity legacy to modern cloud IDaaS — now subsumed into Ping Identity but still the visual auth orchestration leader and the commercial steward of Sun's identity heritage");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "forgerock".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_forgerock(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_forgerock};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/forgerock"), "forgerock");
        assert_eq!(basename(r"C:\bin\forgerock.exe"), "forgerock.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("forgerock.exe"), "forgerock");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_forgerock(&["--help".to_string()], "forgerock"), 0);
        assert_eq!(run_forgerock(&["-h".to_string()], "forgerock"), 0);
        assert_eq!(run_forgerock(&["--version".to_string()], "forgerock"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_forgerock(&[], "forgerock"), 0);
    }
}
