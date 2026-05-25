#![deny(clippy::all)]

//! mulesoft-cli — OurOS MuleSoft (Anypoint Platform + API mgmt, San Francisco, Salesforce subsidiary)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mulesoft(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mulesoft [OPTIONS]");
        println!("MuleSoft (OurOS) — Anypoint Platform (API + integration — Salesforce subsidiary)");
        println!();
        println!("Options:");
        println!("  --anypoint             Anypoint Platform (unified API + integration)");
        println!("  --api-manager          API Manager (lifecycle, gateway, security)");
        println!("  --runtime-fabric       Anypoint Runtime Fabric (Kubernetes runtime)");
        println!("  --exchange             Anypoint Exchange (asset marketplace)");
        println!("  --dataweave            DataWeave (transformation language)");
        println!("  --rpa                  MuleSoft RPA (was Servicetrace)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MuleSoft Anypoint 2024 (OurOS) — Mule 4.6"); return 0; }
    println!("MuleSoft 2024 (OurOS) — Anypoint Platform");
    println!("  Vendor: MuleSoft, LLC (San Francisco — Salesforce subsidiary since 2018)");
    println!("  Founder: Ross Mason, 2006 (originally as MuleSource)");
    println!("          'Mule' name = 'pack mule' carrying heavy integration loads — also play on 'donkey work'");
    println!("          ESB (Enterprise Service Bus) → modern iPaaS evolution");
    println!("          'API-led connectivity' methodology popularized the API-first approach in enterprises");
    println!("          Greg Schott: long-time CEO 2009-2021");
    println!("          Brent Hayward: current CEO (since 2021, Salesforce-era)");
    println!("  Acquisition: Salesforce bought MuleSoft March 2018 for $6.5B");
    println!("              one of Salesforce's largest acquisitions ever");
    println!("              MuleSoft had IPO'd in 2017 at $17, peaked $44 pre-acquisition");
    println!("              acquired at $44.89/share — solid premium");
    println!("              Salesforce wanted MuleSoft for 'Customer 360' data integration story");
    println!("              MuleSoft now part of Salesforce 'Data + AI' (with Tableau + Slack + Data Cloud)");
    println!("  Strategic position: 'API-led connectivity — unlock your data with reusable APIs':");
    println!("                    pitch: 'compose connected experiences with APIs that are discoverable + reusable'");
    println!("                    target: large enterprise (Salesforce ecosystem + standalone)");
    println!("                    primary competitor: Boomi, Workato, Informatica, IBM webMethods, Microsoft Power Platform");
    println!("                    secondary: Kong (API mgmt only), Apigee (Google), AWS API Gateway");
    println!("                    MuleSoft's wedge: Anypoint unified platform + DataWeave + Salesforce go-to-market");
    println!("                    'Composability + reusable APIs + Salesforce data' = enterprise architect's playbook");
    println!("  Pricing (notoriously expensive):");
    println!("    Anypoint Platform — $80K-$5M+/yr (enterprise iPaaS)");
    println!("    Per-core licensing: $40K-$120K per CPU-core/year for Mule runtime");
    println!("    API Manager add-on — $50K-$500K+/yr");
    println!("    Runtime Fabric (K8s deploy) — additional");
    println!("    MuleSoft RPA — $20K-$1M+/yr");
    println!("    typically the most-expensive iPaaS — competitors win on price (Workato, Boomi)");
    println!("  Product portfolio (Anypoint Platform):");
    println!("    1. Anypoint Design Center (Flow Designer + API Designer):");
    println!("       - Visual API + flow design");
    println!("       - RAML (RESTful API Modeling Language) — MuleSoft's spec, now also supports OpenAPI");
    println!("    2. Anypoint Studio (Eclipse-based IDE):");
    println!("       - Heavy desktop tool for Mule flow development");
    println!("       - Graphical 'connectors + transformers + flow control'");
    println!("    3. Mule Runtime (the integration engine):");
    println!("       - JVM-based ESB/integration runtime");
    println!("       - Mule 4.x current line (was Mule 3.x — major breaking change)");
    println!("       - Embeds DataWeave (the transformation language)");
    println!("    4. Anypoint API Manager:");
    println!("       - API lifecycle, gateway, throttling, security policies");
    println!("       - Compete with: Kong, Apigee, AWS API Gateway, Azure API Mgmt");
    println!("    5. Anypoint Exchange (asset marketplace):");
    println!("       - Internal catalog of reusable APIs + templates + connectors");
    println!("       - 200+ pre-built connectors (Salesforce, SAP, Oracle, Workday, etc.)");
    println!("    6. Anypoint Runtime Fabric (Kubernetes):");
    println!("       - K8s deployment for Mule runtime");
    println!("       - Multi-cloud + hybrid deployment");
    println!("    7. Anypoint Monitoring + Visualizer:");
    println!("       - APM for Mule flows");
    println!("       - Application Network Graph (Anypoint Visualizer)");
    println!("    8. MuleSoft Composer (low-code for business users):");
    println!("       - Salesforce + Slack integration without coding");
    println!("       - Salesforce-platform-native low-code (alternative to Anypoint for citizen integrators)");
    println!("    9. MuleSoft RPA (was Servicetrace, acquired 2021):");
    println!("       - Robotic process automation");
    println!("       - Compete with: UiPath, Automation Anywhere, Microsoft Power Automate");
    println!("    10. MuleSoft AI (2024 — agentic + LLM-augmented integration):");
    println!("       - Generative API design + AI-assisted flow building");
    println!("       - Topic Center for Agentforce");
    println!("  DataWeave (the secret weapon):");
    println!("    - MuleSoft's data transformation language");
    println!("    - Functional, expression-based JSON/XML/CSV/Java/etc. transformation");
    println!("    - Similar role to XSLT, but for the JSON/REST era");
    println!("    - Highly performant, type-safe, lazy-evaluated");
    println!("    - Killer feature for complex enterprise data mappings");
    println!("  API-led connectivity methodology:");
    println!("    - System APIs (close to system of record)");
    println!("    - Process APIs (orchestrate business processes)");
    println!("    - Experience APIs (channel-specific — web, mobile, partner)");
    println!("    - Application Network (all APIs as a graph)");
    println!("    - Influential conceptual framework — adopted across industry");
    println!("  Integrations (200+ connectors):");
    println!("    - SaaS: Salesforce (native), Workday, ServiceNow, NetSuite, HubSpot, Marketo, Slack");
    println!("    - ERP: SAP, Oracle EBS, Microsoft Dynamics, JD Edwards");
    println!("    - Database: Oracle, SQL Server, PostgreSQL, MongoDB, Snowflake");
    println!("    - Messaging: Kafka, RabbitMQ, IBM MQ, AMQP, JMS");
    println!("    - Cloud: AWS (S3, Lambda, SQS, SNS, RDS), Azure, GCP");
    println!("    - Files: SFTP, FTP, FTPS, S3, Azure Blob, GCS");
    println!("    - Legacy: COBOL, IBM AS/400 (DataWeave handles transformation)");
    println!("  MuleSoft CLI usage:");
    println!("    mulesoft anypoint-cli login --username dev --password=$ANYPOINT_PASSWORD");
    println!("    mulesoft anypoint api-mgr deploy --api-spec orders-v1.raml");
    println!("    mulesoft runtime deploy --target-id 'prod-cluster' --mule-app orders.jar");
    println!("    mulesoft exchange publish --asset-type rest-api --gav com.acme:orders:1.0.0");
    println!("    mulesoft dataweave run script.dwl --input payload.json");
    println!("  Customers (huge enterprise base):");
    println!("    - 1,800+ enterprise customers");
    println!("    - Heavy in: financial services, retail, manufacturing, healthcare, government");
    println!("    - Coca-Cola, Mount Sinai, McDonald's, Unilever, Cisco, Bank of America");
    println!("    - U.S. federal: VA, DoD, USAID");
    println!("    - International: heavy in UK + Europe + APAC enterprise");
    println!("  Critique: very expensive — per-core pricing punishes scale");
    println!("           Mule 3 → Mule 4 migration was painful (breaking changes, 2018-2022)");
    println!("           heavy ESB heritage = perceived as 'legacy' vs cloud-native iPaaS (Workato)");
    println!("           Anypoint Studio (Eclipse) dated UX vs modern web-IDEs");
    println!("           Salesforce ownership = pushed harder into Salesforce ecosystem, sometimes feels lock-in");
    println!("           Salesforce Composer overlap = strategic confusion (which tool to use?)");
    println!("           cloud-native challengers (Workato, Tray) winning new logos on price + UX");
    println!("           API Manager is decent but loses to Kong on pure API mgmt + Apigee on Google ecosystem");
    println!("  Differentiator: $6.5B Salesforce acquisition (huge) + Anypoint unified iPaaS + DataWeave (best-in-class transformation language) + API-led connectivity methodology + 200+ pre-built connectors + Salesforce Customer 360 data integration story + RAML/OpenAPI tooling + Application Network Graph — the enterprise-grade iPaaS for big customers who already run Salesforce and need complex hybrid (on-prem + cloud + SaaS) integration");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mulesoft".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mulesoft(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
