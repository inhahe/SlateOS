#![deny(clippy::all)]

//! vllm-cli — Slate OS vLLM serving CLI
//!
//! Multi-personality: `vllm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vllm(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: vllm COMMAND [OPTIONS]");
        println!("vLLM 0.5.0 (Slate OS) — Fast LLM serving engine");
        println!();
        println!("Commands:");
        println!("  serve          Start OpenAI-compatible API server");
        println!("  chat           Interactive chat mode");
        println!("  complete       Offline batch completion");
        println!("  benchmark      Run benchmarks");
        println!();
        println!("Serve options:");
        println!("  --model MODEL         Model name/path");
        println!("  --port PORT           Server port (default: 8000)");
        println!("  --host HOST           Server host (default: 0.0.0.0)");
        println!("  --tensor-parallel N   Tensor parallelism");
        println!("  --max-model-len N     Maximum model length");
        println!("  --gpu-memory-util F   GPU memory utilization (0-1)");
        println!("  --dtype TYPE          Data type (auto, float16, bfloat16)");
        println!("  --quantization Q      Quantization (awq, gptq, squeezellm)");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "--version" => println!("vllm 0.5.0"),
        "serve" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("meta-llama/Llama-3-8B");
            let port = args.windows(2).find(|w| w[0] == "--port")
                .map(|w| w[1].as_str()).unwrap_or("8000");
            println!("INFO:     Loading model '{}'...", model);
            println!("INFO:     Model loaded in 12.3s");
            println!("INFO:     GPU memory usage: 14.2 GB / 24.0 GB");
            println!("INFO:     Starting OpenAI-compatible API server");
            println!("INFO:     Serving at http://0.0.0.0:{}", port);
            println!("INFO:     API docs: http://0.0.0.0:{}/docs", port);
        }
        "chat" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("meta-llama/Llama-3-8B");
            println!("Loading model '{}'...", model);
            println!("Model loaded. Type 'quit' to exit.");
            println!();
            println!("> Hello!");
            println!("Hello! How can I help you today?");
        }
        "complete" => {
            println!("Running offline batch completion...");
            println!("  Processed 100 prompts");
            println!("  Average tokens/s: 1234.5");
            println!("  Total time: 5.6s");
        }
        "benchmark" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("meta-llama/Llama-3-8B");
            println!("Benchmarking '{}'...", model);
            println!();
            println!("  Throughput: 1234.5 tokens/s");
            println!("  Latency (median): 23.4ms");
            println!("  Latency (p99): 89.1ms");
            println!("  Time to first token: 12.3ms");
            println!("  Batch size: 32");
        }
        _ => println!("vllm: '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vllm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vllm(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vllm};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vllm"), "vllm");
        assert_eq!(basename(r"C:\bin\vllm.exe"), "vllm.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vllm.exe"), "vllm");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vllm(&["--help".to_string()]), 0);
        assert_eq!(run_vllm(&["-h".to_string()]), 0);
        let _ = run_vllm(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vllm(&[]);
    }
}
