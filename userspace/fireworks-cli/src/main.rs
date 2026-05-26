#![deny(clippy::all)]
//! fireworks-cli — personality CLI for Fireworks AI, the open-model
//! inference + fine-tuning platform built by the ex-Meta PyTorch crew.
//!
//! Founded 2022 in Redwood City by Lin Qiao (CEO, ex-Meta engineering
//! leader of PyTorch from 2017-2022) with a founding team of senior
//! PyTorch + Caffe2 engineers. Fireworks competes head-on with Together AI
//! and Anyscale Endpoints by serving open-source LLMs (Llama, Mixtral,
//! DeepSeek, Qwen) on its own optimised inference stack — including the
//! FireAttention CUDA kernels (custom rewrites of attention/decoding) and
//! FireFunction (a function-calling-tuned variant of Mixtral). Raised a
//! $52M Series B led by Sequoia at a ~$552M post (Jul 2024).

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Fireworks AI open-model inference personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Qiao + ex-Meta PyTorch crew, Redwood City");
    println!("    models        Llama, Mixtral, DeepSeek, Qwen, FireFunction");
    println!("    fireattn      Custom CUDA kernels for inference speed");
    println!("    finetune      LoRA + full fine-tune service, instant deploy");
    println!("    api           OpenAI-compatible chat completions");
    println!("    compound      Compound AI Systems vision");
    println!("    pricing       Per-token serverless + on-demand GPU");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("fireworks-cli 0.1.0 (FireAttention personality build)"); }

fn run_about() {
    println!("Fireworks AI.");
    println!("  Founded:    2022, Redwood City, California.");
    println!("  Founder:    Lin Qiao (CEO) — led PyTorch at Meta 2017-2022.");
    println!("  Founding team: senior PyTorch + Caffe2 + GPU engineers from");
    println!("              Meta, Google, IBM Watson.");
    println!("  Funding:    $52M Series B Jul 2024 led by Sequoia at ~$552M post.");
    println!("              Earlier seed + Series A from Benchmark.");
    println!("  Pitch:      'The fastest, easiest place to use, customise, and");
    println!("              evaluate open-source AI models'.");
}

fn run_models() {
    println!("Models served:");
    println!("  Llama 3.3 70B Instruct       flagship general open chat.");
    println!("  Llama 3.1 405B Instruct      frontier-class open, served at scale.");
    println!("  Mixtral 8x22B + 8x7B         Mistral MoE workhorses.");
    println!("  DeepSeek V3 + R1             671B-class reasoning model.");
    println!("  Qwen 2.5 + Qwen Coder 32B    Alibaba models, strong on code.");
    println!("  FireFunction v2              Mixtral 8x7B tuned for OpenAI-style");
    println!("                               function calling.");
    println!("  Stable Diffusion 3, FLUX     image generation models also hosted.");
}

fn run_fireattn() {
    println!("FireAttention — custom CUDA kernels.");
    println!("  Hand-written attention + decoding kernels that target specific");
    println!("  GPU SKUs (H100, A100, MI300).");
    println!("  Optimised for the regimes Fireworks actually serves:");
    println!("  - long prompt / short response (RAG)");
    println!("  - many concurrent requests (chat workloads)");
    println!("  - speculative decoding + medusa heads where applicable.");
    println!("  Benchmarks: typically 2-5x throughput vs vanilla vLLM at same latency.");
}

fn run_finetune() {
    println!("Fine-tuning service.");
    println!("  LoRA fine-tunes: cheap, fast (minutes-to-hours), no GPU babysitting.");
    println!("  Full fine-tunes: for larger model surgery, separate pricing.");
    println!("  Trained models deploy instantly to the same API endpoint with");
    println!("  a model id; LoRA adapters are merged dynamically at request time.");
    println!("  Up to many adapters per base model coexist on shared GPUs.");
}

fn run_api() {
    println!("API surface:");
    println!("  api.fireworks.ai endpoint, OpenAI-compatible /chat/completions.");
    println!("  Drop-in: change base_url + api_key in OpenAI client.");
    println!("  Streaming, function calling, JSON mode, vision input where supported.");
    println!("  /completions for raw text-completion-style LLM use.");
    println!("  /images for diffusion model generation.");
}

fn run_compound() {
    println!("Compound AI Systems — the broader thesis.");
    println!("  Fireworks pushes the idea that production AI = pipelines of");
    println!("  multiple models + retrievers + tools + verifiers, not single");
    println!("  monolithic frontier-model calls.");
    println!("  Their platform features (function calling, retrieval, routing,");
    println!("  agent primitives) are designed around that 'compound system' view.");
    println!("  Aligned with the Berkeley AI Research 'compound AI' framing.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  Serverless per-token. Llama 3.3 70B ~$0.90/M; Mixtral 8x22B ~$1.20/M.");
    println!("  Speculative decoding tier sometimes cheaper for compatible models.");
    println!("  On-demand reserved GPUs: per-hour, no surprise bill caps.");
    println!("  Fine-tuning: per-token-trained pricing, separate from inference.");
    println!("  Volume discounts + enterprise contracts available.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Cresta, Sourcegraph, Quora Poe, Notion AI, Cursor, Upwork,");
    println!("  Hugging Face inference endpoints (partial), various AI-startups");
    println!("  doing customer-support copilots and code assistants.");
    println!("  Strong adoption among teams that need open-weight inference");
    println!("  with predictable latency + LoRA fine-tunes.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "fireworks-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "models" => run_models(),
        "fireattn" => run_fireattn(),
        "finetune" => run_finetune(),
        "api" => run_api(),
        "compound" => run_compound(),
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
        run_fireattn();
        run_finetune();
        run_api();
        run_compound();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("fireworks-cli");
        print_version();
    }
}
