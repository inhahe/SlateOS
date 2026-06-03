#![deny(clippy::all)]

//! filmora-cli — OurOS Wondershare Filmora (consumer video editor)
//!
//! Single personality: `filmora`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fil(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: filmora [OPTIONS]");
        println!("Wondershare Filmora 14 (OurOS) — Consumer video editor (China-based)");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --ai-copilot           AI Copilot Editing (chat-driven video edit)");
        println!("  --ai-smart-cutout      AI Smart Cutout (subject masking)");
        println!("  --ai-music             AI Music (generate royalty-free score)");
        println!("  --ai-text-to-video     AI Text-to-Video (script → edit)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Wondershare Filmora 14.0.11 (OurOS)"); return 0; }
    println!("Wondershare Filmora 14.0.11 (OurOS)");
    println!("  Vendor: Wondershare Technology (HQ Shenzhen, China — founded 2003)");
    println!("  Founder: Wu Taibing (CEO)");
    println!("  Platforms: Windows, macOS, iOS, iPadOS, Android (cross-platform parity)");
    println!("  Pricing: Annual $49.99/yr, Cross-Platform $79.99/yr, Perpetual $79.99 (Win/Mac one-time)");
    println!("  Free tier: yes, with watermark + export limits");
    println!("  Education: free for students/teachers");
    println!("  Engine: GPU-accelerated, supports 8K, vertical/9:16 export for TikTok/Reels");
    println!("  AI features (Filmora 14):");
    println!("    - AI Copilot Editing (GPT-4 chat assistant)");
    println!("    - AI Smart Cutout (rotoscope auto-mask)");
    println!("    - AI Music Generator (royalty-free, mood/genre input)");
    println!("    - AI Text-to-Video (script → assembled cut)");
    println!("    - AI Vocal Remover, AI Audio Stretch, AI Translation, AI Image gen, AI Background Removal");
    println!("  Target users: YouTubers, TikTok/Reels creators, hobbyists — pitched at the");
    println!("               'better than iMovie, friendlier than Premiere' middle ground");
    println!("  Companion apps: Filmstock (asset library), Filmora Mobile, FilmoraGo (renamed Filmora)");
    println!("  Other Wondershare: PDFelement, MobileTrans, Recoverit, EdrawMax, UniConverter");
    println!("  Controversy: 2020 SafeBreach reported Wondershare bundling, since cleaned up");
    println!("  Differentiator: heavy AI feature roll-out + good price/feature ratio at consumer tier");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "filmora".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fil(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fil};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/filmora"), "filmora");
        assert_eq!(basename(r"C:\bin\filmora.exe"), "filmora.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("filmora.exe"), "filmora");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fil(&["--help".to_string()], "filmora"), 0);
        assert_eq!(run_fil(&["-h".to_string()], "filmora"), 0);
        assert_eq!(run_fil(&["--version".to_string()], "filmora"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fil(&[], "filmora"), 0);
    }
}
