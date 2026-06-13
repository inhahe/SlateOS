#![deny(clippy::all)]

//! wechat-cli — Slate OS Tencent WeChat super-app
//!
//! Single personality: `wechat`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wechat [OPTIONS]");
        println!("WeChat / Weixin (Slate OS) — Tencent super-app");
        println!();
        println!("Options:");
        println!("  --chat                 Messages");
        println!("  --moments              Friend Moments (timeline)");
        println!("  --pay                  WeChat Pay (PSP/QR payments)");
        println!("  --miniapps             Mini Programs (in-app apps)");
        println!("  --official             Official Accounts (subscription/service)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("WeChat 3.9.12 / Weixin 8.0.50 (Slate OS)"); return 0; }
    println!("WeChat 3.9.12 / Weixin 8.0.50 (Slate OS)");
    println!("  Vendor: Tencent (Shenzhen, China)");
    println!("  Launched: Jan 2011 by Allen Zhang (creator of QQmail before)");
    println!("  Two products: Weixin (微信, China) and WeChat (overseas) — separate data");
    println!("  Users: 1.3B+ monthly active — China's dominant social/comm/payment app");
    println!("  Features: messaging, voice/video calls, Moments (FB-like feed), Channels (video),");
    println!("            Official Accounts (broadcast), Mini Programs (in-app apps, no install),");
    println!("            WeChat Pay (QR code payments, dominant in China alongside Alipay)");
    println!("  Mini Programs: 4M+ mini-apps inside WeChat — full ecosystem, JS+WXML");
    println!("  Enterprise: Enterprise WeChat / WeCom (Tencent Conference, drive, mail)");
    println!("  Strengths: super-app model — chat + payment + identity + transit + ride + food");
    println!("  Controversies: surveillance, censorship (Tiananmen, NBA), 2019/2021 US bans (rolled back)");
    println!("  Westwise: app store, distribution outside China, ID required since 2017");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wechat".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wechat"), "wechat");
        assert_eq!(basename(r"C:\bin\wechat.exe"), "wechat.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wechat.exe"), "wechat");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wc(&["--help".to_string()], "wechat"), 0);
        assert_eq!(run_wc(&["-h".to_string()], "wechat"), 0);
        let _ = run_wc(&["--version".to_string()], "wechat");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wc(&[], "wechat");
    }
}
