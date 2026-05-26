#![deny(clippy::all)]
//! mistral-cli — personality CLI for Mistral AI, the European frontier-lab
//! champion.
//!
//! Founded April 2023 in Paris by Arthur Mensch (CEO, ex-DeepMind), Timothée
//! Lacroix (CTO, ex-Meta FAIR), and Guillaume Lample (Chief Scientist, ex-Meta
//! FAIR — co-author of the LLaMA paper). Famous for raising a €105M seed
//! at €240M valuation with a 7-page deck and a working prototype, then a
//! €385M Series A (Dec 2023, $2B valuation) and a €600M Series B (Jun 2024,
//! $6.2B). Distinctive market position: ship open-weight models (Mistral 7B,
//! Mixtral 8x7B, Codestral) alongside closed flagship models (Large, Medium).

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Mistral AI European frontier-lab personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Mensch+Lacroix+Lample, Paris, LLaMA lineage");
    println!("    models        Large, Small, Codestral, Pixtral, open weights");
    println!("    openweight    Apache-2.0 weight releases strategy");
    println!("    api           La Plateforme + Le Chat assistant");
    println!("    eu            European data sovereignty story");
    println!("    cloud         Azure, AWS, GCP, Snowflake distribution");
    println!("    pricing       Per-token + Le Chat tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("mistral-cli 0.1.0 (open+closed-weight personality build)"); }

fn run_about() {
    println!("Mistral AI.");
    println!("  Founded:    April 2023, Paris, France.");
    println!("  Founders:   Arthur Mensch (CEO, ex-DeepMind),");
    println!("              Timothée Lacroix (CTO, ex-Meta FAIR),");
    println!("              Guillaume Lample (Chief Scientist, ex-Meta FAIR,");
    println!("              first-author of the LLaMA paper).");
    println!("  Funding:    €105M seed Jun 2023 (€240M valuation, 4 weeks old).");
    println!("              €385M Series A Dec 2023 ($2B). €600M Series B Jun 2024");
    println!("              ($6.2B). Lightspeed, a16z, Salesforce, NVIDIA, BPI.");
    println!("  Mission:    A serious European alternative to US frontier labs.");
}

fn run_models() {
    println!("Model family:");
    println!("  Mistral Large 2     124B parameters, frontier-class, closed.");
    println!("  Mistral Medium 3    cost-optimised mid-tier, closed.");
    println!("  Mistral Small 3     24B class, open-weights Apache 2.0.");
    println!("  Mistral Nemo        12B, partnership with NVIDIA, open-weights.");
    println!("  Codestral           code-specialised, 22B base + Mamba2 variant.");
    println!("  Pixtral 12B / Large vision-language models.");
    println!("  Mistral 7B          original Sep 2023 open release that built the brand.");
    println!("  Mixtral 8x7B / 8x22B Mixture-of-Experts, open-weights.");
}

fn run_openweight() {
    println!("Open-weights strategy.");
    println!("  Most small + medium models ship under Apache 2.0 (commercial OK).");
    println!("  Some research models ship under Mistral Research License.");
    println!("  Flagship 'Large' tier is closed; sold via API.");
    println!("  Effect: open weights = community + research goodwill + benchmark");
    println!("  visibility; closed flagship = revenue. Best of both.");
    println!("  Open models routinely top open-source leaderboards at release.");
}

fn run_api() {
    println!("API surface — La Plateforme.");
    println!("  /v1/chat/completions   OpenAI-compatible schema.");
    println!("  /v1/embeddings         retrieval embeddings.");
    println!("  /v1/fim/completions    fill-in-the-middle for Codestral.");
    println!("  /v1/agents             stateful agents with tools.");
    println!("  Function calling, JSON mode, vision input on Pixtral.");
    println!("  Le Chat — consumer assistant + business plan with connectors,");
    println!("  canvas, web search, image generation.");
}

fn run_eu() {
    println!("European data-sovereignty positioning.");
    println!("  Headquartered in Paris; explicit EU/AI Act alignment.");
    println!("  EU-region inference available across cloud partners.");
    println!("  Politically: visible support from the French government as");
    println!("  the European answer to US frontier labs.");
    println!("  Defense + public-sector deals: French armed forces, EU bodies.");
}

fn run_cloud() {
    println!("Cloud distribution — multi-platform.");
    println!("  Mistral La Plateforme  direct API, EU-hosted.");
    println!("  Azure AI               first non-OpenAI frontier model on Azure.");
    println!("  AWS Bedrock            Mistral models available there too.");
    println!("  Google Vertex AI       Mistral models also distributed via GCP.");
    println!("  Snowflake Cortex       in-warehouse LLM inference.");
    println!("  IBM watsonx            partnership announced 2024.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Per-token, per-model. Mistral Large ~$2/M in, $6/M out;");
    println!("  Mistral Small ~$0.2/M in, $0.6/M out; Codestral ~$0.3/M in.");
    println!("  Open-weight models: free to self-host under Apache 2.0.");
    println!("  Le Chat: Free / Pro $14.99/mo / Team / Enterprise.");
    println!("  Volume + commit discounts available for enterprise.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  BNP Paribas, Société Générale, Stellantis, French Ministry of");
    println!("  Armed Forces, Cisco, Brave Search, IBM Consulting.");
    println!("  Strong adoption among European enterprises that need on-region");
    println!("  inference and a non-US vendor.");
    println!("  Heavy use in the open-source community via the open-weight models.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "mistral-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "models" => run_models(),
        "openweight" => run_openweight(),
        "api" => run_api(),
        "eu" => run_eu(),
        "cloud" => run_cloud(),
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
        run_models();
        run_openweight();
        run_api();
        run_eu();
        run_cloud();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("mistral-cli");
        print_version();
    }
}
