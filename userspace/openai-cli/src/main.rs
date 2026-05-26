#![deny(clippy::all)]
//! openai-cli — personality CLI for OpenAI, the lab that turned LLMs into
//! a consumer product.
//!
//! Founded December 2015 in San Francisco as a non-profit by Sam Altman,
//! Greg Brockman, Elon Musk, Ilya Sutskever, Wojciech Zaremba, John
//! Schulman and others with a $1B pledged. Restructured into a "capped-
//! profit" company in 2019. Shipped GPT-3 (2020), ChatGPT (Nov 30 2022),
//! GPT-4 (Mar 2023), GPT-4o (May 2024), o1 reasoning models (Sep 2024),
//! and the dalle/sora image and video generators. Microsoft is the
//! dominant investor with $13B+ committed and an exclusive Azure
//! infrastructure deal.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — OpenAI LLM personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Altman/Brockman, GPT family lineage");
    println!("    models        GPT-4o, o1, o3, DALL-E, Sora, Whisper");
    println!("    api           Chat completions, responses, assistants");
    println!("    chatgpt       Consumer product, Plus/Team/Enterprise");
    println!("    safety        Alignment, red team, model spec");
    println!("    governance    Capped-profit, board, MSFT relationship");
    println!("    pricing       Per-token + ChatGPT subscription tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("openai-cli 0.1.0 (frontier-LLM personality build)"); }

fn run_about() {
    println!("OpenAI, L.P. / OpenAI, Inc.");
    println!("  Founded:   December 11, 2015, San Francisco.");
    println!("  Founders:  Sam Altman (CEO), Greg Brockman (President),");
    println!("             Elon Musk (departed 2018), Ilya Sutskever (Chief");
    println!("             Scientist, departed 2024), John Schulman, others.");
    println!("  Structure: Capped-profit subsidiary of a non-profit parent");
    println!("             (since 2019 restructure).");
    println!("  Investor:  Microsoft, $13B+ committed; exclusive Azure cloud.");
    println!("  Watershed: ChatGPT, launched Nov 30 2022, hit 100M MAU in 2 months —");
    println!("             fastest consumer-product adoption curve on record.");
    println!("  Valuation: Reported at $157B as of Oct 2024 funding round.");
}

fn run_models() {
    println!("Model family:");
    println!("  GPT-4o          omni-modal frontier model (text/voice/vision).");
    println!("  GPT-4o mini     cost-optimised tier, 60%+ cheaper.");
    println!("  o1 / o1-mini    chain-of-thought reasoning models, Sep 2024.");
    println!("  o3 / o3-mini    next-gen reasoning, announced Dec 2024.");
    println!("  GPT-4 Turbo     long-context text model, 128K tokens.");
    println!("  DALL-E 3        text-to-image.");
    println!("  Sora            text-to-video, limited release.");
    println!("  Whisper         speech-to-text (open weights).");
    println!("  text-embedding-3 retrieval embeddings, large + small variants.");
}

fn run_api() {
    println!("API surface:");
    println!("  /v1/chat/completions   the workhorse: messages in, message out.");
    println!("  /v1/responses          newer multi-turn API with tool calls.");
    println!("  /v1/assistants         stateful assistants + threads + runs.");
    println!("  /v1/images/generations DALL-E.");
    println!("  /v1/audio/transcriptions Whisper STT.");
    println!("  /v1/audio/speech       TTS.");
    println!("  /v1/embeddings         vectors for retrieval.");
    println!("  /v1/fine_tuning/jobs   custom-trained variants of base models.");
    println!("  /v1/batch              50% discount, 24h async turnaround.");
}

fn run_chatgpt() {
    println!("ChatGPT — the consumer product.");
    println!("  Free       GPT-4o mini + limited GPT-4o; basic features.");
    println!("  Plus       $20/mo, faster GPT-4o, image+voice, GPTs marketplace.");
    println!("  Team       $25/user/mo, admin console, shared GPTs.");
    println!("  Enterprise per-seat custom, SSO/SAML, no training on your data,");
    println!("             unlimited GPT-4o, longer context, audit logs.");
    println!("  Pro        $200/mo, unlimited o1, advanced voice, exclusive tier.");
    println!("  GPTs       user-created custom assistants; share via link.");
}

fn run_safety() {
    println!("Safety and alignment.");
    println!("  RLHF + RLAIF training; preference modelling at scale.");
    println!("  Red teaming with internal and external contractors.");
    println!("  Model spec: public document of intended behaviour.");
    println!("  Preparedness framework: pre-release dangerous-capability evals");
    println!("  for cybersecurity, CBRN uplift, autonomy, persuasion.");
    println!("  Usage policies + automated content moderation API.");
}

fn run_governance() {
    println!("Governance — unusual structure.");
    println!("  Non-profit parent (OpenAI, Inc.) governs the for-profit sub.");
    println!("  Board overhaul Nov 2023 after Altman firing/reinstatement.");
    println!("  Capped-profit: outside investor returns capped (originally 100x).");
    println!("  Mission charter: 'safe and beneficial AGI'.");
    println!("  Microsoft has board observer + significant commercial rights.");
    println!("  Public restructure to traditional for-profit reportedly underway.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  API per-token, per-model. GPT-4o ~$2.5/M in, $10/M out;");
    println!("  GPT-4o mini ~$0.15/M in, $0.60/M out; o1 ~$15/M in, $60/M out.");
    println!("  Batch API gives 50% discount for async 24h jobs.");
    println!("  Cached input tokens are discounted ~50%.");
    println!("  ChatGPT subscription: $0 / $20 / $25 seat / $200 / custom Ent.");
    println!("  Sora and advanced features gated to higher tiers.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Effectively every Fortune 500 has at least a pilot.");
    println!("  Microsoft Copilot stack (M365, GitHub, Bing) runs on OpenAI.");
    println!("  Khan Academy 'Khanmigo', Stripe, Salesforce Einstein, Duolingo Max,");
    println!("  Klarna assistant, Morgan Stanley wealth advisors — all built");
    println!("  on the API.");
    println!("  300M+ weekly ChatGPT users (reported Dec 2024).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "openai-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "models" => run_models(),
        "api" => run_api(),
        "chatgpt" => run_chatgpt(),
        "safety" => run_safety(),
        "governance" => run_governance(),
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
        run_api();
        run_chatgpt();
        run_safety();
        run_governance();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("openai-cli");
        print_version();
    }
}
