#![deny(clippy::all)]
//! cohere-cli — personality CLI for Cohere, the enterprise-focused LLM lab.
//!
//! Founded 2019 in Toronto by Aidan Gomez (CEO, co-author of "Attention is
//! All You Need" while interning at Google Brain), Nick Frosst (also ex-Google
//! Brain, son of Moshe Safdie), and Ivan Zhang. Cohere's distinctive market
//! position: while other frontier labs chase consumer ChatGPT-like products,
//! Cohere sells exclusively to enterprises — no consumer chatbot, no ads,
//! no end-user product. Models tuned for RAG, multilingual workloads, and
//! deployment inside private VPCs. Raised $500M Series D at a $5.5B
//! valuation (Jul 2024) led by PSP Investments + Cisco + Fujitsu + Nvidia.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Cohere enterprise-LLM personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Gomez+Frosst+Zhang, Toronto, enterprise-only");
    println!("    models        Command, Embed, Rerank, Aya multilingual");
    println!("    rag           Retrieval + grounded generation primitives");
    println!("    deploy        Private VPC, on-prem, OCI/AWS/Azure/GCP");
    println!("    multilingual  Aya 23 / 101-language coverage");
    println!("    forai         For AI non-profit research lab");
    println!("    pricing       Per-token enterprise contracts");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("cohere-cli 0.1.0 (enterprise-only personality build)"); }

fn run_about() {
    println!("Cohere Inc.");
    println!("  Founded:    2019, Toronto, Canada.");
    println!("  Founders:   Aidan Gomez (CEO, co-author Transformer paper);");
    println!("              Nick Frosst (ex-Google Brain, Geoff Hinton mentee);");
    println!("              Ivan Zhang.");
    println!("  Position:   Enterprise-only LLM provider. No consumer product.");
    println!("  Funding:    $500M Series D Jul 2024 at $5.5B post.");
    println!("              Investors: PSP, Cisco, Fujitsu, AMD, Salesforce,");
    println!("              Nvidia, Inovia, Index, Tiger, Radical, Oracle.");
    println!("  HQ shifts:  Toronto roots; SF, NYC, London offices.");
}

fn run_models() {
    println!("Model family:");
    println!("  Command R+       104B generation model, RAG-optimised, long-context.");
    println!("  Command R        35B mid-tier R-family generation model.");
    println!("  Command (legacy) earlier flagship, still in service.");
    println!("  Embed v3         multilingual retrieval embeddings.");
    println!("  Rerank v3        late-stage re-ranker for hybrid search pipelines.");
    println!("  Aya 23           multilingual generation, 23 languages.");
    println!("  Aya Expanse      next-gen multilingual, frontier-class non-English.");
}

fn run_rag() {
    println!("RAG primitives — Cohere's signature.");
    println!("  Embed: turn corpus into dense vectors for vector DB.");
    println!("  Rerank: take top-100 from any retriever, reorder by relevance.");
    println!("  Command R/R+: generation models with first-class");
    println!("  document grounding — citations point back to source chunks.");
    println!("  Chat API supports {{documents: [...]}} parameter natively.");
    println!("  Output includes citations field with span ranges + doc ids.");
}

fn run_deploy() {
    println!("Deployment topology — enterprise-first.");
    println!("  Cohere API direct (cohere.com).");
    println!("  Private deployments: VPC-isolated on AWS, Azure, GCP, OCI.");
    println!("  On-prem: full air-gapped deployments for regulated sectors.");
    println!("  No-train guarantee: your data is never used to train base models.");
    println!("  Fine-tuning: customer-specific model copies, isolated.");
}

fn run_multilingual() {
    println!("Multilingual focus — Aya project.");
    println!("  Aya 23: 23-language generation model, Apache 2.0 release.");
    println!("  Aya Expanse: frontier-class on 23 languages including Arabic,");
    println!("  Chinese, French, German, Hindi, Japanese, Russian.");
    println!("  Aya Dataset: 513M-instance multilingual instruction dataset,");
    println!("  contributed by ~3000 volunteers across 65 languages.");
    println!("  Differentiator: dedicated to non-English first, not retrofitted.");
}

fn run_forai() {
    println!("Cohere For AI — research lab.");
    println!("  Non-profit research lab inside Cohere.");
    println!("  Open papers, open weights for Aya project, open datasets.");
    println!("  Scholars program: residency for early-career researchers,");
    println!("  especially from underrepresented regions.");
    println!("  Mission: 'machine learning research with the world, not for it'.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  API per-token, per-model. Command R+ ~$2.5/M in, $10/M out;");
    println!("  Command R ~$0.15/M in, $0.60/M out; Embed ~$0.10/M tokens;");
    println!("  Rerank ~$2 per 1K searches.");
    println!("  Enterprise: committed-use contracts, private deployments,");
    println!("  custom fine-tunes — quoted per-engagement.");
    println!("  Free trial keys available for evaluation, low rate limit.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Oracle (deep partnership; Cohere models embedded in OCI),");
    println!("  Fujitsu (Japanese-market enterprise LLM partnership),");
    println!("  Notion AI (RAG), Salesforce Einstein, RBC, TD Bank,");
    println!("  Bell Canada, McKinsey QuantumBlack.");
    println!("  Heavy adoption in banks, telcos, and Asian/EU enterprises that");
    println!("  need a non-US, non-Chinese sovereign LLM vendor.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "cohere-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "models" => run_models(),
        "rag" => run_rag(),
        "deploy" => run_deploy(),
        "multilingual" => run_multilingual(),
        "forai" => run_forai(),
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
        run_rag();
        run_deploy();
        run_multilingual();
        run_forai();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("cohere-cli");
        print_version();
    }
}
