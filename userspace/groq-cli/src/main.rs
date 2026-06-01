#![deny(clippy::all)]
//! groq-cli — personality CLI for Groq, the LPU custom-silicon inference
//! company.
//!
//! Founded 2016 in Mountain View by Jonathan Ross, the engineer who started
//! Google's TPU project as a 20% project in 2013. Groq's silicon — the
//! Language Processing Unit (LPU) — is a deterministic, single-core chip
//! with on-die SRAM and no caches. Combined with a tightly-optimised
//! compiler stack, it delivers per-token latencies for LLM inference that
//! GPU-based providers cannot match (sub-millisecond first-token, hundreds
//! of output tokens per second on 70B-class models). Raised $640M Series D
//! at a $2.8B valuation (Aug 2024) led by BlackRock + Cisco + Samsung Catalyst.
//! Not the same company as Elon Musk's xAI 'Grok' chatbot.

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Groq LPU ultra-fast-inference personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Ross, ex-Google TPU, LPU silicon thesis");
    println!("    lpu           Language Processing Unit architecture");
    println!("    models        Llama, Mixtral, Gemma, Whisper served");
    println!("    api           GroqCloud OpenAI-compatible endpoint");
    println!("    speed         Tokens-per-second benchmarks");
    println!("    notgrok       Disambiguation from xAI Grok");
    println!("    pricing       Per-token + on-prem GroqRack");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("groq-cli 0.1.0 (LPU-silicon personality build)"); }

fn run_about() {
    println!("Groq Inc.");
    println!("  Founded:    2016, Mountain View, California.");
    println!("  Founder:    Jonathan Ross (CEO) — initiated Google's TPU project");
    println!("              as a 20%-time effort starting 2013.");
    println!("  Funding:    $640M Series D Aug 2024 at $2.8B led by BlackRock,");
    println!("              with Cisco, Samsung Catalyst, KDDI, others.");
    println!("              Earlier rounds from Tiger, D1, TDK Ventures.");
    println!("  Pitch:      'AI's fastest inference'. Custom silicon, not GPUs.");
    println!("  Not Grok:   Has nothing to do with Elon Musk's xAI chatbot.");
}

fn run_lpu() {
    println!("LPU — Language Processing Unit architecture.");
    println!("  Single deterministic core with thousands of execution units.");
    println!("  Entirely on-die SRAM (~230 MB per chip) — no DRAM, no cache.");
    println!("  Software-managed everything: the compiler schedules every cycle.");
    println!("  Determinism: same prompt -> identical timing every run.");
    println!("  Scale: chips link into chains; a model can span many LPUs.");
    println!("  Tradeoff: low aggregate FLOPS vs GPUs, but tiny memory latency");
    println!("  -> extremely low time-to-first-token for autoregressive LLMs.");
}

fn run_models() {
    println!("Models served on GroqCloud:");
    println!("  Llama 3.3 70B Versatile      flagship general-purpose.");
    println!("  Llama 3.1 8B Instant         fast, cheap, decent quality.");
    println!("  Llama 3.2 1B/3B/11B/90B      Vision variants in 11B + 90B.");
    println!("  Mixtral 8x7B Instruct        MoE workhorse.");
    println!("  Gemma 2 9B / 27B             Google open-weights.");
    println!("  Whisper Large v3             ASR, fast.");
    println!("  DeepSeek R1 distill (Llama)  reasoning at LPU speed.");
}

fn run_api() {
    println!("API surface — GroqCloud.");
    println!("  api.groq.com endpoint, OpenAI-compatible /v1/chat/completions.");
    println!("  Drop-in: change base_url + api_key in the OpenAI client.");
    println!("  Streaming responses (SSE) with per-token deltas.");
    println!("  Function calling, JSON mode, tool use supported on most models.");
    println!("  Free-tier playground for development; paid for production volume.");
}

fn run_speed() {
    println!("Speed — the headline number.");
    println!("  Llama 3 70B:       ~250+ tokens/sec on GroqCloud (typical).");
    println!("  Llama 3 8B:        ~750-1000+ tokens/sec.");
    println!("  Time-to-first-token: typically <100ms, often <50ms.");
    println!("  Comparison: GPU-served 70B models usually deliver 20-80 t/s.");
    println!("  Why it matters: real-time voice agents, low-latency tool loops,");
    println!("  human-perceptible 'instant' chat UI.");
}

fn run_notgrok() {
    println!("Groq is NOT Grok.");
    println!("  Groq (this company) — custom inference silicon, founded 2016,");
    println!("  Jonathan Ross, ex-Google TPU.");
    println!("  Grok — Elon Musk's xAI chatbot, launched 2023, runs on GPUs.");
    println!("  The naming collision is a continuous source of confusion.");
    println!("  Groq publicly objected when xAI introduced the chatbot name;");
    println!("  the two products are unrelated and not competitors.");
}

fn run_pricing() {
    println!("Pricing model:");
    println!("  GroqCloud per-token. Llama 3.3 70B ~$0.59/M in, $0.79/M out;");
    println!("  Llama 3.1 8B ~$0.05/M in, $0.08/M out;");
    println!("  Mixtral 8x7B ~$0.24/M in/out.");
    println!("  Generally undercuts GPU-served pricing for the same open models.");
    println!("  GroqRack: on-prem turnkey LPU appliance for enterprises wanting");
    println!("  the speed in their own data centre. Custom pricing.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Saudi Arabia's HUMAIN (gigawatt-scale LPU build-out partnership),");
    println!("  Aramco Digital, Bell Canada, US Department of Energy labs,");
    println!("  Argonne National Lab.");
    println!("  Heavy use by AI-startup developer community: voice agents,");
    println!("  agent frameworks, batch inference pipelines that need throughput.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "groq-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "lpu" => run_lpu(),
        "models" => run_models(),
        "api" => run_api(),
        "speed" => run_speed(),
        "notgrok" => run_notgrok(),
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
        run_lpu();
        run_models();
        run_api();
        run_speed();
        run_notgrok();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("groq-cli");
        print_version();
    }
}
