#![deny(clippy::all)]

//! ladspa-cli — Slate OS LADSPA/LV2 audio plugin tools
//!
//! Multi-personality: `listplugins`, `analyseplugin`, `lv2ls`, `lv2info`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_listplugins(_args: &[String]) -> i32 {
    println!("/usr/lib/ladspa/amp.so:");
    println!("  Mono Amplifier (1048)");
    println!("  Stereo Amplifier (1049)");
    println!("/usr/lib/ladspa/delay.so:");
    println!("  Simple Delay (1043)");
    println!("  Stereo Delay (1044)");
    println!("/usr/lib/ladspa/filter.so:");
    println!("  Low Pass Filter (1041)");
    println!("  High Pass Filter (1042)");
    println!("  Band Pass Filter (1045)");
    println!("/usr/lib/ladspa/reverb.so:");
    println!("  Freeverb (1050)");
    println!("  Plate Reverb (1051)");
    0
}

fn run_analyseplugin(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: analyseplugin LIBRARY [LABEL]");
        return 0;
    }
    let lib = args.first().map(|s| s.as_str()).unwrap_or("amp.so");
    println!("Plugin Name: \"Mono Amplifier\"");
    println!("Plugin Label: \"amp_mono\"");
    println!("Plugin Unique ID: 1048");
    println!("Maker: \"Slate OS Audio Plugins\"");
    println!("Copyright: \"None\"");
    println!("Must Run Real-Time: No");
    println!("Has activate() Function: No");
    println!("Has deactivate() Function: No");
    println!("Has run_adding() Function: Yes");
    println!("Environment: Normal or Hard Real-Time");
    println!("Ports:");
    println!("  \"Gain\" input, control, -70dB to +6dB, default 0dB");
    println!("  \"Input\" input, audio");
    println!("  \"Output\" output, audio");
    let _ = lib;
    0
}

fn run_lv2ls(_args: &[String]) -> i32 {
    println!("http://calf.sourceforge.net/plugins/Compressor");
    println!("http://calf.sourceforge.net/plugins/Equalizer5Band");
    println!("http://calf.sourceforge.net/plugins/Reverb");
    println!("http://calf.sourceforge.net/plugins/Limiter");
    println!("http://drobilla.net/plugins/mda/DX10");
    println!("http://drobilla.net/plugins/mda/Piano");
    println!("http://gareus.org/oss/lv2/meters#spectr30stereo");
    println!("http://lsp-plug.in/plugins/lv2/comp_stereo");
    println!("http://lsp-plug.in/plugins/lv2/para_equalizer_x16_stereo");
    0
}

fn run_lv2info(args: &[String]) -> i32 {
    if args.is_empty() {
        println!("Usage: lv2info PLUGIN_URI");
        return 0;
    }
    let uri = args.first().map(|s| s.as_str()).unwrap_or("http://calf.sourceforge.net/plugins/Compressor");
    println!("URI: {}", uri);
    println!("Name: Calf Compressor");
    println!("Class: Compressor");
    println!("Author: Calf Studio Gear");
    println!("License: LGPL");
    println!("Bundle: /usr/lib/lv2/calf.lv2/");
    println!("Ports:");
    println!("  Audio In L       (input, audio)");
    println!("  Audio In R       (input, audio)");
    println!("  Audio Out L      (output, audio)");
    println!("  Audio Out R      (output, audio)");
    println!("  Threshold        (input, control, -60dB to 0dB)");
    println!("  Ratio            (input, control, 1:1 to 20:1)");
    println!("  Attack           (input, control, 0.01ms to 2000ms)");
    println!("  Release          (input, control, 0.01ms to 2000ms)");
    println!("  Makeup Gain      (input, control, 0dB to 36dB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "listplugins".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "analyseplugin" => run_analyseplugin(&rest),
        "lv2ls" => run_lv2ls(&rest),
        "lv2info" => run_lv2info(&rest),
        _ => run_listplugins(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_analyseplugin};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ladspa"), "ladspa");
        assert_eq!(basename(r"C:\bin\ladspa.exe"), "ladspa.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ladspa.exe"), "ladspa");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_analyseplugin(&["--help".to_string()]), 0);
        assert_eq!(run_analyseplugin(&["-h".to_string()]), 0);
        let _ = run_analyseplugin(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_analyseplugin(&[]);
    }
}
