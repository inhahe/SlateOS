#![deny(clippy::all)]
//! modal-cli — personality CLI for Modal Labs, the Python-native serverless
//! GPU + container platform.
//!
//! Founded 2021 by Erik Bernhardsson (CEO, ex-Spotify, creator of the Luigi
//! workflow scheduler and the Annoy nearest-neighbour library) and Akshat
//! Bubna. The defining product idea: data and ML engineers write ordinary
//! Python functions decorated with @app.function(...), and Modal handles
//! container builds, GPU provisioning, autoscaling to thousands of workers,
//! cold-starts in seconds, and serverless billing. Raised $80M total
//! (seed + Series A + Series B) from Redpoint, Lux, Definition, and others.

use std::env;

fn basename(p: &str) -> &str {
    let s = p.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(p);
    s
}

fn strip_ext(s: &str) -> &str {
    s.strip_suffix(".exe").unwrap_or(s)
}

fn print_help(prog: &str) {
    println!("{prog} — Modal Labs serverless-Python-GPU personality CLI");
    println!();
    println!("USAGE:");
    println!("    {prog} <command> [args]");
    println!();
    println!("COMMANDS:");
    println!("    about         Bernhardsson, ex-Spotify Luigi/Annoy");
    println!("    sdk           @app.function decorator programming model");
    println!("    images        Builder DSL for Docker images in Python");
    println!("    gpus          T4, L4, L40S, A10G, A100, H100, H200");
    println!("    sandboxes     Code-exec sandboxes (LLM agent workloads)");
    println!("    runtime       Custom container runtime, fast cold start");
    println!("    pricing       Per-second compute + storage");
    println!("    customers     Selected named accounts");
    println!("    help          Show this help");
    println!("    version       Show version");
}

fn print_version() { println!("modal-cli 0.1.0 (Python-serverless-GPU personality build)"); }

fn run_about() {
    println!("Modal Labs.");
    println!("  Founded:    2021, San Francisco / NYC.");
    println!("  Founders:   Erik Bernhardsson (CEO; created Luigi at Spotify,");
    println!("              Annoy ANN library, several blog-famous data essays);");
    println!("              Akshat Bubna (CTO).");
    println!("  Funding:    $80M+ across seed/A/B. Redpoint led; Lux, Definition,");
    println!("              Amplify, Y Combinator W22 participation.");
    println!("  Pitch:      'The cloud, but for Python developers'.");
    println!("  Heritage:   Bernhardsson's Spotify infrastructure background");
    println!("              shows in the focus on reproducible builds and the");
    println!("              workflow-engine ergonomics.");
}

fn run_sdk() {
    println!("SDK — Python-native, decorator-based.");
    println!("  pip install modal");
    println!("  @app.function(image=...) wraps any Python function.");
    println!("  fn.remote(arg)         -> run on Modal cloud, get result.");
    println!("  fn.spawn(arg)          -> fire-and-forget, returns FunctionCall.");
    println!("  fn.map(iterable)       -> fan-out, parallel execution.");
    println!("  @app.cls               -> class-based with @enter() warm setup.");
    println!("  @app.web_endpoint      -> expose function as HTTP endpoint.");
    println!("  @app.schedule          -> cron-like periodic execution.");
}

fn run_images() {
    println!("Image builder — Docker as a Python DSL.");
    println!("  modal.Image.debian_slim()");
    println!("    .pip_install('torch', 'transformers')");
    println!("    .apt_install('ffmpeg')");
    println!("    .run_commands('python -c \"import torch; torch.hub.download(...)\"')");
    println!("    .copy_local_file('weights.bin', '/root/weights.bin')");
    println!("  Builds run on Modal's infra, cached per layer.");
    println!("  No Dockerfile string-templating; everything is real Python.");
}

fn run_gpus() {
    println!("GPU lineup:");
    println!("  T4         entry-level, 16GB, cheapest GPU tier.");
    println!("  L4         24GB Ada, modern + efficient.");
    println!("  A10G       AWS-flavour Ampere, 24GB.");
    println!("  L40S       48GB Ada, common bread-and-butter.");
    println!("  A100 40G   workhorse training/inference.");
    println!("  A100 80G   bigger context, 70B-class.");
    println!("  H100       Hopper top tier.");
    println!("  H200       latest, 141GB HBM3e.");
    println!("  Multi-GPU + multi-node supported for distributed training.");
}

fn run_sandboxes() {
    println!("Sandboxes — secure code execution for agents.");
    println!("  modal.Sandbox.create(image=..., timeout=...).");
    println!("  Use case: LLM agents that need to run arbitrary generated code.");
    println!("  Each sandbox is an isolated container; file system, network,");
    println!("  GPU access can be scoped per sandbox.");
    println!("  Stream stdout/stderr, attach to TTY, kill on timeout.");
    println!("  Cold start fast enough for interactive agent loops.");
}

fn run_runtime() {
    println!("Runtime + container internals.");
    println!("  Custom container runtime built on top of gVisor + their own");
    println!("  snapshotting layer.");
    println!("  Cold start: tens of milliseconds to a few seconds, even for");
    println!("  heavy ML images (LLM weights pre-loaded into memory snapshot).");
    println!("  Container snapshotting + checkpoint-restore lets a 'warm pool'");
    println!("  of pre-initialised GPU workers wake in <1s.");
    println!("  Distributed file system primitive: modal.Volume + modal.Dict.");
}

fn run_pricing() {
    println!("Pricing model — pay-per-second of compute.");
    println!("  CPU:   ~$0.0000131/core-second.");
    println!("  Memory:~$0.0000003/MB-second.");
    println!("  GPU:   per-GPU per-second; H100 ~$0.001097/s, A100-80G ~$0.000824/s,");
    println!("         A10G ~$0.000306/s.");
    println!("  Volumes + bandwidth: charged separately.");
    println!("  Free tier: $30/mo of compute credit for individual accounts.");
}

fn run_customers() {
    println!("Selected customers + adopters:");
    println!("  Suno (AI music), Substack (ML infra), Quora Poe, Notion ML team,");
    println!("  Ramp, Anysphere/Cursor, Cohere internal tooling.");
    println!("  Heavy adoption in AI-startup space: fine-tuning pipelines,");
    println!("  RAG batch jobs, video-generation backends, eval pipelines.");
    println!("  Also used internally by ML research teams that prefer Python");
    println!("  over Kubernetes for their dev velocity.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog: String = args
        .first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "modal-cli".to_string());

    if args.len() < 2 {
        print_help(&prog);
        return;
    }

    match args[1].as_str() {
        "about" => run_about(),
        "sdk" => run_sdk(),
        "images" => run_images(),
        "gpus" => run_gpus(),
        "sandboxes" => run_sandboxes(),
        "runtime" => run_runtime(),
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
        run_sdk();
        run_images();
        run_gpus();
        run_sandboxes();
        run_runtime();
        run_pricing();
        run_customers();
    }

    #[test]
    fn help_and_version() {
        print_help("modal-cli");
        print_version();
    }
}
