#![deny(clippy::all)]

//! instana-cli — SlateOS IBM Instana (APM + observability, IBM subsidiary since 2020)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_instana(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: instana [OPTIONS]");
        println!("IBM Instana (SlateOS) — automatic full-stack APM + observability");
        println!();
        println!("Options:");
        println!("  --apm                  Application Performance Monitoring (auto-discovery)");
        println!("  --infrastructure       Infrastructure monitoring");
        println!("  --kubernetes           Kubernetes + container observability");
        println!("  --traces               Distributed tracing (every request, no sampling)");
        println!("  --website-monitoring   End-user / RUM monitoring");
        println!("  --aiops                Watson AIOps integration");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IBM Instana 2024 (SlateOS)"); return 0; }
    println!("IBM Instana 2024 (SlateOS) — Enterprise Observability");
    println!("  Vendor: IBM Corporation, via Instana (Solingen Germany + Chicago — IBM subsidiary since 2020)");
    println!("  Founders: Mirko Novakovic + Pavlo Baron + Pete Abrams + Fabian Lange, 2015");
    println!("          founded in Solingen, Germany (technical depth + DACH enterprise focus)");
    println!("          'Instana' name = 'instant' + Latin '-ana' (instant analytics)");
    println!("          Mirko Novakovic: long-time CEO + ex-Codecentric founder (German consulting house)");
    println!("          Differentiator from day one: 'automatic discovery + zero-config APM' for microservices");
    println!("  Acquisition: IBM bought Instana Nov 2020 for ~$500M");
    println!("              Strategic for IBM hybrid cloud / OpenShift / Watson AIOps platform");
    println!("              Integrated with IBM Cloud Pak for Watson AIOps");
    println!("              Mirko Novakovic stayed as IBM observability leader (left 2023)");
    println!("              Now part of IBM Software 'Automation' portfolio");
    println!("  Strategic position: 'automatic APM — zero-config, full-stack, every-request tracing':");
    println!("                    pitch: 'observability without instrumentation drudgery — automatic + complete'");
    println!("                    target: enterprise + IBM Cloud Pak customers (hybrid cloud)");
    println!("                    primary competitor: Dynatrace, Datadog APM, New Relic, AppDynamics (Splunk)");
    println!("                    secondary: Honeycomb, Lightstep (acq Splunk), Elastic APM");
    println!("                    Instana's wedge: automatic discovery + every-request tracing + IBM ecosystem distribution");
    println!("                    'No sampling' = full fidelity (vs Datadog's stochastic sampling)");
    println!("  Pricing:");
    println!("    Self-hosted on-prem: $50K-$1M+/yr");
    println!("    SaaS (Instana Cloud): per-host based, $50-200/host/month");
    println!("    Enterprise IBM Cloud Pak bundle: included in larger Watson AIOps deals");
    println!("    typically competitive with Dynatrace + Datadog APM for similar host counts");
    println!("  Product portfolio:");
    println!("    1. APM (the flagship):");
    println!("       - Auto-discovery: agent finds all running services + libraries automatically");
    println!("       - 'No-touch' instrumentation for Java, .NET, Node.js, Python, Go, Ruby, PHP");
    println!("       - Every-request tracing (no sampling)");
    println!("       - Distributed traces + service dependency map auto-built");
    println!("    2. Infrastructure Monitoring:");
    println!("       - Servers, VMs, containers, Kubernetes, cloud services");
    println!("       - Auto-detects all running tech stack components");
    println!("    3. Kubernetes Observability:");
    println!("       - Auto-discovers pods, deployments, services, namespaces");
    println!("       - Container performance + dependencies");
    println!("       - Helm-based agent deployment");
    println!("    4. Distributed Tracing:");
    println!("       - End-to-end traces across microservices");
    println!("       - OpenTelemetry + OpenTracing + Instana-native ingestion");
    println!("       - 'Calls' visualization (vs Datadog's flame graphs)");
    println!("    5. Website Monitoring (RUM / End-User Monitoring):");
    println!("       - JavaScript beacon for browser/mobile RUM");
    println!("       - Core Web Vitals + Apdex");
    println!("       - Synthetic monitoring (HTTP + browser automation)");
    println!("    6. AI-driven Analytics ('Stan' AI assistant):");
    println!("       - Automatic incident root-cause analysis");
    println!("       - 'Stan' = Instana's AI chatbot interface");
    println!("       - Integration with Watson AIOps for enterprise SRE");
    println!("    7. Custom Events + Alerts:");
    println!("       - Event-driven anomaly detection");
    println!("       - Integration with PagerDuty, Slack, ServiceNow, MS Teams");
    println!("    8. Unbounded Analytics:");
    println!("       - Query historical trace data with arbitrary filters");
    println!("       - No pre-aggregation = full fidelity queries");
    println!("  Auto-discovery (the differentiator):");
    println!("    - Instana agent identifies all running tech stack components automatically");
    println!("    - Java agent attaches via JVMTI without config changes");
    println!("    - 250+ sensor types (Java, .NET, Node, Python, Ruby, PHP, Go, Erlang, Crystal, etc.)");
    println!("    - 'Plug it in and it works' demo a key sales mechanic");
    println!("  Every-request tracing (the architectural bet):");
    println!("    - No sampling = every request fully traced");
    println!("    - Contrast Datadog APM (head-based sampling)");
    println!("    - Heavier ingestion cost but better fidelity for long-tail debugging");
    println!("    - Compresses + tiered storage to manage costs");
    println!("  Integrations:");
    println!("    - Cloud: AWS, Azure, GCP, IBM Cloud, Alibaba Cloud");
    println!("    - Container: Kubernetes (deep), OpenShift, Docker, ECS, Fargate, EKS");
    println!("    - IBM stack: WebSphere, MQ, DataPower, Cloud Pak, Watson AIOps");
    println!("    - DBs: Oracle, DB2, SQL Server, PostgreSQL, MySQL, MongoDB, Cassandra, Redis");
    println!("    - Messaging: Kafka, RabbitMQ, ActiveMQ, IBM MQ, AMQP");
    println!("    - Tracing: OpenTelemetry, OpenTracing, Jaeger, Zipkin");
    println!("    - Notifications: PagerDuty, Opsgenie, Slack, Teams, ServiceNow, Splunk");
    println!("  Instana CLI usage:");
    println!("    instana login --tenant my-org --region us-east");
    println!("    instana agent install --target kubernetes --namespace instana-agent");
    println!("    instana application list --filter env=production");
    println!("    instana trace search --service checkout --duration '>1s' --from -1h");
    println!("    instana event create --severity critical --description 'Deployment of v2.0'");
    println!("    instana dashboard import --file my-dashboard.json");
    println!("    instana stan chat --query 'why is the checkout service slow today?'");
    println!("  Customers (~5,000+):");
    println!("    - IBM customer base + standalone");
    println!("    - Heavy in: financial services, retail, manufacturing (DACH heritage)");
    println!("    - Sky, Allianz, Deutsche Bank, ING, Daimler, BMW");
    println!("    - U.S. federal: limited (vs Splunk/Dynatrace)");
    println!("    - International: extremely strong in Germany + DACH region");
    println!("    - sweet spot: enterprise hybrid-cloud + IBM Cloud Pak shops");
    println!("  Critique: post-IBM acquisition innovation pace concerns");
    println!("           bundled into Watson AIOps = sometimes hard to buy standalone");
    println!("           Datadog + Dynatrace marketing budgets dominate top-of-funnel");
    println!("           every-request tracing = higher cost than head-sampled competitors");
    println!("           less brand awareness outside IBM-aligned shops");
    println!("           OpenTelemetry-native but slower to add OTel features vs Honeycomb/Lightstep");
    println!("           'Stan' AI assistant features behind Datadog Bits AI + Dynatrace Davis CoPilot");
    println!("           DACH regional concentration limits NAm enterprise wins");
    println!("  Differentiator: automatic discovery (zero-config APM) + every-request tracing (no sampling) + 250+ auto-detected sensor types + IBM Cloud Pak / Watson AIOps integration + 'Stan' AI assistant + strong DACH enterprise footprint (Sky, Allianz, Deutsche Bank, ING) + Mirko Novakovic's technical credibility — the German-founded APM platform that auto-discovers your stack and traces every request, distributed by IBM to its global enterprise base");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "instana".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_instana(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_instana};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/instana"), "instana");
        assert_eq!(basename(r"C:\bin\instana.exe"), "instana.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("instana.exe"), "instana");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_instana(&["--help".to_string()], "instana"), 0);
        assert_eq!(run_instana(&["-h".to_string()], "instana"), 0);
        let _ = run_instana(&["--version".to_string()], "instana");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_instana(&[], "instana");
    }
}
