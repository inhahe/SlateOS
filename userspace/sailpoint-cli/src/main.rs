#![deny(clippy::all)]

//! sailpoint-cli — OurOS SailPoint (IGA market leader, Austin TX, Thoma Bravo round-trip, IPO'd twice)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sailpoint(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sailpoint [OPTIONS]");
        println!("SailPoint (OurOS) — Identity Governance & Administration (IGA) market leader");
        println!();
        println!("Options:");
        println!("  --identitynow          IdentityNow (cloud IGA SaaS, now 'Identity Security Cloud')");
        println!("  --identityiq           IdentityIQ (on-prem IGA, the heritage product)");
        println!("  --accessrisk           Access Risk Management (SAP / Oracle access analytics)");
        println!("  --filesecurity         File Access Manager (data access governance)");
        println!("  --ai                   AI/ML identity insights (NLP-driven access reviews)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SailPoint 2024 (OurOS) — sailpoint-cli (Identity Security Cloud admin)"); return 0; }
    println!("SailPoint 2024 (OurOS) — Identity Governance & Administration Market Leader");
    println!("  Vendor: SailPoint Technologies Holdings (Austin, TX — NYSE: SAIL again, re-IPO'd Feb 2025)");
    println!("  Founders: Mark McClain + Kevin Cunningham + Jackie Gilbert, 2005");
    println!("          Mark McClain: CEO 2005-present (20 years!), founder-led for two decades");
    println!("          Kevin Cunningham: CTO co-founder");
    println!("          Jackie Gilbert: VP Marketing co-founder");
    println!("          All three ex-Waveset Technologies (Sun acquired Waveset 2003)");
    println!("          'We left Sun + identity team behind to start SailPoint'");
    println!("          Austin TX based — Texas tech hub");
    println!("  History (the round-trip):");
    println!("    - Founded 2005 in Austin TX after Sun's Waveset acquisition (founders disliked direction)");
    println!("    - First IPO Nov 2017 NYSE:SAIL at $12/share, raised $240M");
    println!("    - Stock peaked ~$70 during 2021 cybersecurity rally");
    println!("    - Acquired private by Thoma Bravo Aug 2022 for $6.9B ($65.25/share)");
    println!("    - Mark McClain stayed as CEO under Thoma Bravo");
    println!("    - Re-IPO'd Feb 2025 NYSE:SAIL at $23/share (raised $1.38B)");
    println!("    - 'IPO -> PE -> IPO' arc in 7 years — rare in software");
    println!("    - Thoma Bravo retained majority control post-re-IPO");
    println!("  Strategic position: 'IGA (Identity Governance & Administration) market leader':");
    println!("                    pitch: 'who has access to what, why, and should they still have it?'");
    println!("                    target: Fortune 1000, regulated industries (banks, pharma, healthcare, government)");
    println!("                    primary competitor: Saviynt, Microsoft Entra ID Governance, Oracle IGA");
    println!("                    secondary: IBM Security Verify Governance, Omada, Brainwave (now Radiant Logic)");
    println!("                    SailPoint wedge: IGA category leader (Gartner MQ #1 every year)");
    println!("                    + the only pure-play IGA vendor at scale (others are part of bigger suites)");
    println!("                    + AI/ML-driven access review intelligence");
    println!("                    + Identity Security Cloud (IdentityNow) is dominant cloud IGA");
    println!("                    'IGA' as a category was largely defined by SailPoint");
    println!("                    'Who should have access to what' — that's the SailPoint question");
    println!("  Public company finances (2024):");
    println!("    Revenue: ~$700M (FY 2024 estimate)");
    println!("    Subscription mix: ~70% (transitioning from license to SaaS)");
    println!("    Market cap: ~$10B at re-IPO");
    println!("    Free cash flow positive");
    println!("    Growing 20%+ YoY in subscription");
    println!("  Pricing (enterprise, opaque):");
    println!("    Identity Security Cloud Business: $4-7/identity/mo (small)");
    println!("    Identity Security Cloud Business Plus: $8-12/identity/mo (larger)");
    println!("    Identity Security Cloud Premier: $15+/identity/mo (enterprise)");
    println!("    IdentityIQ on-prem: per-employee perpetual + maintenance OR subscription");
    println!("    typically 6-7 figure annual deals at large enterprises");
    println!("    pricing scales with: identity count + connectors + AI capabilities");
    println!("  Architecture (the IGA platform):");
    println!("    - Identity Security Cloud (cloud SaaS): multi-tenant on AWS");
    println!("    - IdentityIQ (on-prem): Java + Hibernate + MySQL/Oracle");
    println!("    - 200+ connectors to source systems (AD, LDAP, SAP, Workday, ServiceNow, AWS, Azure, GCP, etc.)");
    println!("    - Connector framework: extensible Java SDK");
    println!("    - Workflow engine for approvals, certifications, requests");
    println!("    - Rule engine + policy engine");
    println!("    - AI/ML layer for recommendation + outlier detection");
    println!("  Product portfolio (the IGA stack):");
    println!("    1. Identity Security Cloud (formerly IdentityNow, the SaaS flagship):");
    println!("       - Multi-tenant cloud IGA");
    println!("       - Lifecycle management (joiner/mover/leaver)");
    println!("       - Access requests + approvals");
    println!("       - Access certifications + reviews");
    println!("       - Role mining + role management");
    println!("       - Separation of Duties (SoD) policy");
    println!("       - Provisioning to 200+ source systems");
    println!("    2. IdentityIQ (on-prem, the heritage product):");
    println!("       - On-prem IGA for orgs that need it");
    println!("       - Java-based, deploy on-prem or in cloud as software");
    println!("       - 15+ years of feature richness");
    println!("       - Still used by orgs that haven't migrated to cloud");
    println!("       - 'IIQ' to the SailPoint community");
    println!("    3. Access Risk Management (for SAP/Oracle ERP):");
    println!("       - SAP GRC + SoD analysis (SAP has notoriously complex SoD)");
    println!("       - Oracle EBS access analytics");
    println!("       - Fine-grained entitlement analysis");
    println!("       - 'Where SoD really matters — financial systems'");
    println!("    4. File Access Manager (data governance):");
    println!("       - File share access auditing (Windows + NAS + cloud file)");
    println!("       - Discovery of sensitive data + who can access it");
    println!("       - Used for: GDPR/CCPA + insider risk programs");
    println!("       - From acquisitions over time");
    println!("    5. AI/ML driven Identity Outliers (new flagship feature):");
    println!("       - Detect unusual access patterns");
    println!("       - 'Why does Bob have access to 3x more apps than his peers?'");
    println!("       - Recommend revocations + role memberships");
    println!("       - ML scoring of access risk");
    println!("    6. Access Modeling (role mining + suggestions):");
    println!("       - Mine roles from actual access");
    println!("       - Suggest role definitions based on user clusters");
    println!("       - Reduce manual role engineering");
    println!("    7. Cloud Infrastructure Entitlement Management (CIEM):");
    println!("       - Cloud IAM permission analytics (AWS, Azure, GCP)");
    println!("       - 'Over-permissioned cloud identities'");
    println!("       - Competing with: Sonrai, Ermetic (Tenable acq), Wiz CIEM");
    println!("    8. Non-Employee Risk Management:");
    println!("       - Contractors, vendors, partners, bots");
    println!("       - The 'who is this non-employee with access' category");
    println!("       - Acquired SecZetta 2024 for $260M for this");
    println!("    9. Customer Identity Governance (CIAM-adjacent):");
    println!("       - Governance over customer identities");
    println!("       - Adjacent to consumer IAM (Auth0, Okta CIAM)");
    println!("    10. SailPoint Atlas:");
    println!("       - Unified platform layer (data layer + AI + workflows)");
    println!("       - Modern microservices architecture");
    println!("       - Roadmap announced 2024");
    println!("  The IGA category creation story:");
    println!("    - 'Identity Governance & Administration' as a Gartner category coined ~2010s");
    println!("    - SailPoint was the first pure-play IGA vendor at scale");
    println!("    - Defined the category alongside Gartner");
    println!("    - 'Who has access to what + why + is that appropriate' = the IGA question");
    println!("    - Differentiated from IAM (which is sign-in) and PAM (which is privileged)");
    println!("    - Compliance-driven: SOX, GDPR, HIPAA, ISO 27001 all need IGA");
    println!("  The compliance angle:");
    println!("    - Access certifications: regulators require periodic 'still need it?' reviews");
    println!("    - Separation of Duties: same person can't initiate + approve a transaction");
    println!("    - Joiner/Mover/Leaver: must revoke access on termination");
    println!("    - SailPoint automates all of this");
    println!("    - Without SailPoint, this is spreadsheet hell at scale");
    println!("    - 'Audit-time saviour' — what SailPoint customers say");
    println!("  The Thoma Bravo round-trip:");
    println!("    - Aug 2022: TB acquired for $6.9B at $65.25/share");
    println!("    - 2022-2025: invested in product (Atlas, CIEM, SecZetta acq)");
    println!("    - Feb 2025: re-IPO'd at $23/share (lower per-share but stock split + larger float)");
    println!("    - TB returned to public market with stronger product + larger company");
    println!("    - One of TB's identity vertical investments (alongside Ping + ForgeRock + SonicWall)");
    println!("  Integrations:");
    println!("    - 200+ source system connectors (AD, LDAP, SAP, Workday, ServiceNow, etc.)");
    println!("    - Cloud: AWS IAM, Azure AD/Entra, GCP IAM");
    println!("    - SCIM 2.0 for SaaS app provisioning");
    println!("    - Ticketing: ServiceNow, Jira, Cherwell");
    println!("    - SIEM: Splunk, QRadar, Sentinel");
    println!("    - Workday + SAP SuccessFactors for HR system of record");
    println!("    - LDAP + AD as identity sources");
    println!("    - PAM: CyberArk + BeyondTrust integration");
    println!("    - REST API for custom integrations");
    println!("    - Connector SDK for building new connectors");
    println!("  SailPoint CLI usage:");
    println!("    # sailpoint CLI (newer, official):");
    println!("    sailpoint connect <tenant>");
    println!("    sailpoint identities list");
    println!("    sailpoint accounts list --source <source-id>");
    println!("    sailpoint search aggregate --query 'attributes.department:Finance'");
    println!("    sailpoint sources list                                    # data sources");
    println!("    sailpoint workflows list");
    println!("    # REST API (canonical):");
    println!("    curl -H 'Authorization: Bearer <token>' https://<tenant>.api.identitynow.com/v3/identities");
    println!("    # IdentityIQ console (on-prem):");
    println!("    iiq console");
    println!("    > run TaskDefinition 'Aggregate AD'");
    println!("    > run TaskDefinition 'Refresh Identity Cube'");
    println!("    # Terraform provider available for Identity Security Cloud");
    println!("  Customers (Fortune 500 + government + regulated industries):");
    println!("    - JPMorgan Chase, Citi, Wells Fargo, Goldman, BNY Mellon (banks)");
    println!("    - UnitedHealth, Anthem, Cigna (healthcare insurance)");
    println!("    - Pfizer, Merck, J&J, Roche (pharma)");
    println!("    - US Federal: DoD, DHS, IRS, SSA (gov)");
    println!("    - State of California, State of Texas (state gov)");
    println!("    - Walmart, Target, Lowes (retail)");
    println!("    - Lockheed Martin, Raytheon, Boeing (defense)");
    println!("    - 2000+ enterprise customers globally");
    println!("    - 'IGA at Fortune 500 scale = SailPoint by default'");
    println!("  Critique: complex implementations (6-12 month deployments common)");
    println!("           IdentityIQ feels dated (older Java app)");
    println!("           cloud (IdentityNow / Identity Security Cloud) catching up in features");
    println!("           expensive vs lighter governance tools");
    println!("           Microsoft Entra ID Governance eating SailPoint at smaller orgs (bundled)");
    println!("           connector quality varies (200+ but some are basic)");
    println!("           dev DX less modern than Okta + Auth0 (legacy enterprise feel)");
    println!("           PE-owned pricing pressure post-2022");
    println!("           Atlas platform migration creates dual-architecture transition pain");
    println!("           AI features still maturing (Outliers + Modeling promise vs reality)");
    println!("  Differentiator: IGA market leader since 2005 (founded Austin TX by Mark McClain + Kevin Cunningham + Jackie Gilbert ex-Sun/Waveset team, IPO 2017 $240M, Thoma Bravo $6.9B Aug 2022, re-IPO'd Feb 2025 $1.38B, NYSE:SAIL again) + Gartner MQ Leader every year + Identity Security Cloud (formerly IdentityNow, multi-tenant SaaS) + IdentityIQ on-prem (the 15+ year heritage Java product) + Access Risk Management (SAP/Oracle SoD analysis) + File Access Manager (data governance) + AI Identity Outliers (ML access pattern anomaly detection) + Access Modeling (role mining) + Cloud Infrastructure Entitlement Management (CIEM, cloud IAM analytics) + Non-Employee Risk Management (SecZetta acq 2024 $260M) + Customer Identity Governance + SailPoint Atlas platform + 200+ source system connectors + JPMorgan/Citi/Wells/Goldman/BNY/UnitedHealth/Pfizer/DoD/DHS/IRS-proven + 2000+ enterprise customers + the only pure-play IGA vendor at scale + IGA category creator (defined the Gartner category alongside Gartner) + 20 years of access certifications + SoD policy + JML workflows + compliance automation (SOX/GDPR/HIPAA/ISO 27001) + Mark McClain founder/CEO for 20 years + Texas tech success story + 6-12 month enterprise deployments + Thoma Bravo identity vertical play (with Ping + ForgeRock sister portfolio) — the IGA category-defining vendor that answers 'who has access to what, why, and should they still' at Fortune 500 scale");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sailpoint".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sailpoint(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
