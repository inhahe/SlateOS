#![deny(clippy::all)]
//! anthropic-cli — personality CLI for Anthropic, the AI-safety lab behind
//! the Claude model family.
//!
//! Founded 2021 in San Francisco by Dario Amodei (CEO, ex-OpenAI VP of
//! Research) and his sister Daniela Amodei (President), along with several
//! other senior OpenAI researchers who left to focus on what they saw as
//! a more safety-first approach. Anthropic's signature research bet is
//! "Constitutional AI" — training models to critique and revise their
//! own outputs against a written set of principles. Shipped Claude 1
//! (Mar 2023), Claude 2 (Jul 2023), Claude 3 (Mar 2024), Claude 3.5 Sonnet
//! (Jun 2024 then upgraded Oct 2024). Google has invested up to $2B and
//! Amazon up to $8B; Claude runs natively on AWS Bedrock and Vertex AI.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Anthropic Claude personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Amodei founders, ex-OpenAI safety bet");
    println!("    claude        Opus, Sonnet, Haiku tiers");
    println!("    api           Messages API, tools, computer use");
    println!("    cai           Constitutional AI training method");
    println!("    interp        Mechanistic interpretability research");
    println!("    cloud         AWS Bedrock, GCP Vertex AI native");
    println!("    pricing       Per-token + Claude.ai subscription tiers");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("anthropic-cli 0.1.0 (Claude-family personality build)"); }

fn run_about() {
    println!("Anthropic PBC (Public Benefit Corporation).");
    println!("  Founded:    2021, San Francisco.");
    println!("  Founders:   Dario Amodei (CEO, ex-OpenAI VP Research),");
    println!("              Daniela Amodei (President, ex-OpenAI VP Safety+Policy),");
    println!("              Tom Brown, Sam McCandlish, Jared Kaplan, others");
    println!("              from the original GPT-3 paper team.");
    println!("  Mission:    AI safety research with a frontier-lab business model");
    println!("              that funds the safety work.");
    println!("  Structure:  Public Benefit Corp + Long-Term Benefit Trust");
    println!("              holds special class of shares for governance.");
    println!("  Funding:    Google up to $2B, Amazon up to $8B, Spark Capital,");
    println!("              Lightspeed, others. Valuation rumored $60B+ late 2024.");
}

fn run_claude() {
    println!("Claude model family:");
    println!("  Claude 3.5 Opus    next flagship (announced).");
    println!("  Claude 3.5 Sonnet  the workhorse: frontier capability,");
    println!("                     mid-price, fast. Upgraded Oct 2024.");
    println!("  Claude 3.5 Haiku   small fast model, frontier-class at its size.");
    println!("  Claude 3 Opus      Mar 2024 flagship, still strong on long-form.");
    println!("  Context windows: 200K tokens standard, 1M tokens experimental.");
    println!("  Multimodal:       text + image input (vision).");
    println!("  Tool use:         function calling + parallel tools.");
}

fn run_api() {
    println!("API surface:");
    println!("  /v1/messages              the primary chat API.");
    println!("  /v1/messages with tools   function calling, parallel calls.");
    println!("  Prompt caching            up to 90% discount on cached prefixes.");
    println!("  Batch API                 50% discount, 24h async.");
    println!("  Computer use (beta)       Claude sees a screenshot, returns");
    println!("                            mouse+keyboard commands. Sonnet 3.5+.");
    println!("  Files API + PDF support   document understanding inputs.");
}

fn run_cai() {
    println!("Constitutional AI (CAI) — flagship training technique.");
    println!("  Instead of RLHF with human labels alone, train the model to");
    println!("  critique its own answers against a written 'constitution' —");
    println!("  a list of principles (be helpful, honest, harmless; avoid X; etc).");
    println!("  Two phases: SL-CAI (model edits its own answers to be safer),");
    println!("  then RL-AIF (model preference labels train the reward model).");
    println!("  Scales feedback better than human-only RLHF.");
}

fn run_interp() {
    println!("Mechanistic interpretability research.");
    println!("  Active research thread: understand model internals at the level");
    println!("  of features, circuits, and superposition.");
    println!("  Notable papers: 'Toy Models of Superposition' (2022),");
    println!("  'Scaling Monosemanticity' (May 2024) — sparse autoencoders");
    println!("  decompose Claude 3 Sonnet activations into millions of");
    println!("  human-interpretable features.");
    println!("  Bet: interpretability is a precondition for safe deployment.");
}

fn run_cloud() {
    println!("Cloud distribution — multi-platform.");
    println!("  Anthropic API     direct, api.anthropic.com.");
    println!("  AWS Bedrock       first-class, Claude is the headline model.");
    println!("  Google Vertex AI  also first-class, GCP-native deployment.");
    println!("  Why: investors are also distribution; reduces single-cloud risk.");
    println!("  Pricing parity across direct + cloud channels (mostly).");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  API per-token, per-model. Claude 3.5 Sonnet: $3/M in, $15/M out;");
    println!("  Haiku 3.5: $0.80/M in, $4/M out; Opus 3: $15/M in, $75/M out.");
    println!("  Prompt cache writes ~1.25x base, cache reads ~10% of base.");
    println!("  Batch API 50% off, 24h async.");
    println!("  Claude.ai consumer: Free / Pro $20/mo / Team $30/seat / Ent custom.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Notion AI, Quora Poe, DuckDuckGo AI Chat, Slack AI, Zoom AI Companion,");
    println!("  Lex, Salesforce Agentforce, Snowflake Cortex, GitLab Duo.");
    println!("  Heavy adoption in legal, healthcare, financial services for");
    println!("  long-context document review (200K window).");
    println!("  Claude is also the model powering Claude Code (this very CLI).");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "anthropic-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "claude" => run_claude(),
        "api" => run_api(),
        "cai" => run_cai(),
        "interp" => run_interp(),
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
        run_claude();
        run_api();
        run_cai();
        run_interp();
        run_cloud();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("anthropic-cli");
        print_version();
    }
}
