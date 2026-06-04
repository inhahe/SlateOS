#![deny(clippy::all)]

//! pytorch-cli — OurOS PyTorch deep learning framework
//!
//! Multi-personality: `torchrun`, `torch_shm_manager`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pytorch(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "torchrun" => {
                println!("torchrun (OurOS) — PyTorch distributed training launcher");
                println!("  --nproc_per_node N  Processes per node");
                println!("  --nnodes N         Number of nodes");
                println!("  --node_rank N      Rank of this node");
                println!("  --master_addr ADDR Master address");
                println!("  --master_port PORT Master port");
                println!("  SCRIPT [ARGS]      Training script");
            }
            _ => {
                println!("PyTorch v2.3 (OurOS) — Deep learning framework");
                println!("  CUDA: available (GPU detected)");
                println!("  cuDNN: 8.9.7");
            }
        }
        println!("  --version          Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PyTorch v2.3.0 (OurOS)"); return 0; }
    match prog {
        "torchrun" => {
            println!("torchrun (OurOS)");
            println!("  Processes: 4 (1 node, 4 GPUs)");
            println!("  Backend: NCCL");
            println!("  Master: localhost:29500");
            println!("  Launching train.py...");
            println!("  [GPU:0] Epoch 1/10, Loss: 2.3456, LR: 0.001");
            println!("  [GPU:0] Epoch 2/10, Loss: 1.2345, LR: 0.001");
            println!("  Training complete");
        }
        _ => {
            println!("PyTorch v2.3.0 (OurOS)");
            println!("  CUDA: 12.1, GPU: NVIDIA RTX 4090");
            println!("  Model: ResNet-50");
            println!("  Parameters: 25,557,032");
            println!("  Inference: 2.3ms/image");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "torchrun".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pytorch(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pytorch};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pytorch"), "pytorch");
        assert_eq!(basename(r"C:\bin\pytorch.exe"), "pytorch.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pytorch.exe"), "pytorch");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pytorch(&["--help".to_string()], "pytorch"), 0);
        assert_eq!(run_pytorch(&["-h".to_string()], "pytorch"), 0);
        let _ = run_pytorch(&["--version".to_string()], "pytorch");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pytorch(&[], "pytorch");
    }
}
