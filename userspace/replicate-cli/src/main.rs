#![deny(clippy::all)]
//! replicate-cli — personality CLI for Replicate, the run-any-model-as-an-API
//! platform.
//!
//! Founded 2019 in San Francisco by Ben Firshman (CEO, ex-Docker, created
//! Docker Compose) and Andreas Jansson (CTO, ex-Spotify ML infra). The
//! core product: any open-source ML model gets a hosted HTTP API. The
//! distinctive technical piece is Cog, an open-source tool that packages
//! a model + its weights + a predict.py into a Docker image with a
//! standardised REST interface. Replicate hosts those Cog containers,
//! autoscales GPU capacity, and bills per-second of GPU time. Raised a
//! $40M Series B from Andreessen Horowitz (Apr 2024).

use std::env;

fn basename(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Replicate run-any-model hosting personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Firshman+Jansson, ex-Docker, ex-Spotify ML");
    println!("    cog           Open-source model containerisation tool");
    println!("    models        Community model gallery, FLUX, SDXL, Llama");
    println!("    api           Prediction API, webhooks, streaming");
    println!("    deploy        Push your own model, autoscaled GPU");
    println!("    gpus          A100, H100, L40S, T4 pricing tiers");
    println!("    pricing       Per-second-of-GPU model");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("replicate-cli 0.1.0 (per-second-GPU personality build)"); }

fn run_about() {
    println!("Replicate (Replicate Inc.).");
    println!("  Founded:    2019, San Francisco.");
    println!("  Founders:   Ben Firshman (CEO, ex-Docker, created Compose),");
    println!("              Andreas Jansson (CTO, ex-Spotify ML platform).");
    println!("  Funding:    $40M Series B Apr 2024 led by a16z;");
    println!("              earlier rounds from Sequoia, Y Combinator.");
    println!("  Pitch:      'Run open-source machine learning with one line of code'.");
    println!("  Cultural:   Strong indie-developer + creator community angle;");
    println!("              big presence on Hacker News + AI Twitter.");
}

fn run_cog() {
    println!("Cog — the open-source containerisation tool.");
    println!("  Apache 2.0 on github.com/replicate/cog.");
    println!("  cog.yaml describes: CUDA version, Python deps, system packages.");
    println!("  predict.py implements a Predictor class with setup() + predict().");
    println!("  `cog build` produces a Docker image with a standardised");
    println!("  HTTP server: POST /predictions, GET /predictions/:id.");
    println!("  Same images run locally, on Replicate, on your own k8s.");
}

fn run_models() {
    println!("Model gallery — thousands of public models.");
    println!("  Image: FLUX.1 dev/schnell/pro, SDXL, Stable Diffusion 3,");
    println!("         Ideogram, Recraft, photo upscalers + face restorers.");
    println!("  Video: HunyuanVideo, Mochi, Kling, Luma Ray, Runway-class.");
    println!("  Audio: MusicGen, AudioGen, Whisper, XTTS, voice cloning.");
    println!("  Text:  Llama 3, Mistral, DeepSeek, Qwen, plus chat models.");
    println!("  3D:    TripoSR, mesh + texture generation.");
    println!("  Each model has a hosted demo page + per-second pricing visible.");
}

fn run_api() {
    println!("API surface — predictions are the unit.");
    println!("  POST /v1/predictions    create a prediction with version + input.");
    println!("  GET  /v1/predictions/:id poll for status / output.");
    println!("  Webhooks: completed, output, logs, all delivered on events.");
    println!("  Streaming output (server-sent events) for models that support it.");
    println!("  Idempotency keys, cancellation, file-input via signed-URL upload.");
}

fn run_deploy() {
    println!("Push your own model.");
    println!("  `cog push r8.im/<user>/<model>` — Replicate stores the image.");
    println!("  Public model: anyone can run it (you earn a share if monetised).");
    println!("  Private model: only your team can run it.");
    println!("  Deployments: a private endpoint with min/max GPU instances,");
    println!("  autoscaled by queue depth. Cold-start optimisations on the platform.");
    println!("  Fine-tunes: LoRA train + serve loop for FLUX, SDXL, Llama, etc.");
}

fn run_gpus() {
    println!("GPU tiers + accelerators:");
    println!("  CPU            cheapest, for tiny models or post-processing.");
    println!("  Nvidia T4      small/old GPU for compatibility runs.");
    println!("  Nvidia L40S    bread-and-butter mid-tier modern Ada GPU.");
    println!("  Nvidia A100 40G workhorse for 7-13B LLM inference.");
    println!("  Nvidia A100 80G larger context / 70B-class.");
    println!("  Nvidia H100    top tier, frontier-image-gen + 70B LLM real-time.");
}

fn run_pricing() {
    println!("Pricing model — per-second of GPU time.");
    println!("  Indicative: CPU ~$0.0001/s, T4 ~$0.000225/s, L40S ~$0.000975/s,");
    println!("  A100-80G ~$0.001400/s, H100 ~$0.001400-$0.002/s.");
    println!("  Public model pricing shown on the model page.");
    println!("  You pay only while a prediction runs; idle = $0.");
    println!("  For high-volume: dedicated deployments with min-instance>0.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Buzzfeed, character.ai (early), Microsoft research demos,");
    println!("  ad-tech startups using FLUX/SDXL at scale, B2C creator apps,");
    println!("  Bytedance research teams, countless indie hackers.");
    println!("  Heavy use as a backing API for AI image-generator startups");
    println!("  that don't want to operate their own GPU fleet.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "replicate-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "cog" => run_cog(),
        "models" => run_models(),
        "api" => run_api(),
        "deploy" => run_deploy(),
        "gpus" => run_gpus(),
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
        run_cog();
        run_models();
        run_api();
        run_deploy();
        run_gpus();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("replicate-cli");
        print_version();
    }
}
