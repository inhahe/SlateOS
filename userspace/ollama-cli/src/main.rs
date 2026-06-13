#![deny(clippy::all)]

//! ollama-cli — Slate OS Ollama CLI
//!
//! Single personality: `ollama`

use std::env;
use std::process;

fn run_ollama(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ollama <COMMAND> [OPTIONS]");
        println!();
        println!("Ollama local LLM runner (Slate OS).");
        println!();
        println!("Commands:");
        println!("  serve        Start ollama server");
        println!("  run          Run a model");
        println!("  pull         Pull a model");
        println!("  push         Push a model");
        println!("  list         List local models");
        println!("  show         Show model info");
        println!("  create       Create a model from Modelfile");
        println!("  cp           Copy a model");
        println!("  rm           Remove a model");
        println!("  ps           List running models");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ollama 0.3.0 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "serve" => {
            let host = args.windows(2).find(|w| w[0] == "--host").map(|w| w[1].as_str()).unwrap_or("127.0.0.1");
            let port = args.windows(2).find(|w| w[0] == "--port").map(|w| w[1].as_str()).unwrap_or("11434");
            println!("time=2024-01-15T14:00:00Z level=INFO msg=\"starting ollama\"");
            println!("time=2024-01-15T14:00:00Z level=INFO msg=\"listening on {}:{}\"", host, port);
            println!("time=2024-01-15T14:00:00Z level=INFO msg=\"detected GPU: NVIDIA RTX 4090 (24 GB)\"");
            0
        }
        "run" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("llama3");
            let prompt = args.get(2).map(|s| s.as_str());
            println!("pulling manifest for {}", model);
            println!("verifying sha256 digest");
            println!("using {} model", model);
            if let Some(p) = prompt {
                println!();
                println!("> {}", p);
                println!();
                println!("This is a simulated response from the {} model running locally via Ollama.", model);
                println!("In a real deployment, the model would generate contextual responses.");
            } else {
                println!(">>> Send a message (/? for help)");
            }
            0
        }
        "pull" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("llama3");
            println!("pulling manifest");
            println!("pulling {} layers...", model);
            println!("  pulling abc123def456... 100%  3.8 GB");
            println!("  pulling 789ghi012jkl... 100%  1.2 KB");
            println!("  pulling mno345pqr678... 100%  8.4 KB");
            println!("verifying sha256 digest");
            println!("writing manifest");
            println!("success");
            0
        }
        "push" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("user/mymodel");
            println!("pushing manifest for {}", model);
            println!("  pushing abc123def456... 100%  3.8 GB");
            println!("  pushing 789ghi012jkl... 100%  1.2 KB");
            println!("success");
            0
        }
        "list" => {
            println!("NAME                    ID              SIZE      MODIFIED");
            println!("llama3:latest           abc123def456    3.8 GB    2 hours ago");
            println!("mistral:latest          789ghi012jkl    4.1 GB    3 days ago");
            println!("codellama:13b           mno345pqr678    7.4 GB    1 week ago");
            println!("phi3:mini               stu901vwx234    2.3 GB    2 weeks ago");
            println!("gemma:7b                yza567bcd890    4.8 GB    1 month ago");
            0
        }
        "show" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("llama3");
            println!("  Model");
            println!("    architecture    llama");
            println!("    parameters      8.0B");
            println!("    quantization    Q4_0");
            println!("    context length  8192");
            println!("    embedding length 4096");
            println!();
            println!("  Parameters");
            println!("    stop    \"<|start_header_id|>\"");
            println!("    stop    \"<|end_header_id|>\"");
            println!("    stop    \"<|eot_id|>\"");
            println!();
            println!("  License");
            println!("    META LLAMA 3 COMMUNITY LICENSE");
            println!("  Model: {}", model);
            0
        }
        "create" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("mymodel");
            let modelfile = args.windows(2).find(|w| w[0] == "-f").map(|w| w[1].as_str()).unwrap_or("Modelfile");
            println!("reading {} ...", modelfile);
            println!("creating model '{}'", name);
            println!("  using base model llama3");
            println!("  applying parameters");
            println!("  applying system prompt");
            println!("success");
            0
        }
        "cp" => {
            let src = args.get(1).map(|s| s.as_str()).unwrap_or("llama3");
            let dst = args.get(2).map(|s| s.as_str()).unwrap_or("my-llama3");
            println!("copied '{}' to '{}'", src, dst);
            0
        }
        "rm" => {
            let model = args.get(1).map(|s| s.as_str()).unwrap_or("llama3");
            println!("deleted '{}'", model);
            0
        }
        "ps" => {
            println!("NAME              ID              SIZE      PROCESSOR       UNTIL");
            println!("llama3:latest     abc123def456    5.9 GB    100% GPU        4 minutes from now");
            0
        }
        _ => {
            if cmd.is_empty() {
                eprintln!("Usage: ollama <command>. See --help.");
            } else {
                eprintln!("Error: unknown command '{}'. See --help.", cmd);
            }
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ollama(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ollama};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ollama(vec!["--help".to_string()]), 0);
        assert_eq!(run_ollama(vec!["-h".to_string()]), 0);
        let _ = run_ollama(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ollama(vec![]);
    }
}
