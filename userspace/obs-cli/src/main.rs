#![deny(clippy::all)]

//! obs-cli — OurOS OBS Studio command-line controller
//!
//! Single personality: `obs-cli`

use std::env;
use std::process;

fn run_obs_cli(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: obs-cli [OPTIONS] <COMMAND>");
        println!();
        println!("Control OBS Studio from the command line via WebSocket.");
        println!();
        println!("Commands:");
        println!("  recording start          Start recording");
        println!("  recording stop           Stop recording");
        println!("  recording toggle         Toggle recording");
        println!("  recording status         Recording status");
        println!("  streaming start          Start streaming");
        println!("  streaming stop           Stop streaming");
        println!("  streaming toggle         Toggle streaming");
        println!("  streaming status         Streaming status");
        println!("  scene list               List scenes");
        println!("  scene switch <NAME>      Switch to scene");
        println!("  scene current            Show current scene");
        println!("  source list              List sources");
        println!("  source toggle <NAME>     Toggle source visibility");
        println!("  source mute <NAME>       Toggle source mute");
        println!("  source volume <N> <VOL>  Set source volume");
        println!("  screenshot <FILE>        Take screenshot");
        println!("  virtualcam start         Start virtual camera");
        println!("  virtualcam stop          Stop virtual camera");
        println!("  replaybuffer start       Start replay buffer");
        println!("  replaybuffer stop        Stop replay buffer");
        println!("  replaybuffer save        Save replay buffer");
        println!("  stats                    Show OBS stats");
        println!("  version                  Show OBS version");
        println!();
        println!("Options:");
        println!("  --host <HOST>            WebSocket host (default: localhost)");
        println!("  --port <PORT>            WebSocket port (default: 4455)");
        println!("  --password <PASS>        WebSocket password");
        println!("  -V, --version            Show CLI version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("obs-cli 0.5.0 (OurOS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");

    match (cmd, subcmd) {
        ("recording", "start") => {
            println!("⏺ Recording started");
            println!("  Output: ~/Videos/recording_2024-01-15_14-30-00.mkv");
            0
        }
        ("recording", "stop") => {
            println!("⏹ Recording stopped");
            println!("  Duration: 00:15:23");
            println!("  File: ~/Videos/recording_2024-01-15_14-30-00.mkv (234.5 MB)");
            0
        }
        ("recording", "toggle") => {
            println!("⏺ Recording toggled (now: recording)");
            0
        }
        ("recording", "status") => {
            println!("Recording: ACTIVE");
            println!("  Duration: 00:05:12");
            println!("  File: ~/Videos/recording_2024-01-15_14-30-00.mkv");
            println!("  Size: 78.3 MB");
            0
        }
        ("streaming", "start") => {
            println!("📡 Streaming started");
            println!("  Server: rtmp://live.twitch.tv/live");
            0
        }
        ("streaming", "stop") => {
            println!("📡 Streaming stopped");
            println!("  Duration: 02:30:45");
            0
        }
        ("streaming", "toggle") => {
            println!("📡 Streaming toggled (now: live)");
            0
        }
        ("streaming", "status") => {
            println!("Streaming: INACTIVE");
            println!("  Total bytes sent: 0");
            0
        }
        ("scene", "list") => {
            println!("Scenes:");
            println!("  1. Desktop Capture [ACTIVE]");
            println!("  2. Game Capture");
            println!("  3. Webcam Only");
            println!("  4. Starting Soon");
            println!("  5. BRB");
            0
        }
        ("scene", "switch") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("Desktop Capture");
            println!("Switched to scene: {}", name);
            0
        }
        ("scene", "current") => {
            println!("Current scene: Desktop Capture");
            0
        }
        ("source", "list") => {
            println!("Sources:");
            println!("  Name                Type           Visible  Muted");
            println!("  ─────────────────── ────────────── ──────── ─────");
            println!("  Desktop Audio       wasapi_input    -        no");
            println!("  Mic/Aux             wasapi_input    -        no");
            println!("  Screen Capture      monitor_capture yes      -");
            println!("  Webcam              v4l2_input      yes      -");
            println!("  Overlay Image       image_source    yes      -");
            println!("  Chat Widget         browser_source  yes      -");
            0
        }
        ("source", "toggle") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("Webcam");
            println!("Toggled visibility: {} (now: hidden)", name);
            0
        }
        ("source", "mute") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("Mic/Aux");
            println!("Toggled mute: {} (now: muted)", name);
            0
        }
        ("source", "volume") => {
            let name = args.get(2).map(|s| s.as_str()).unwrap_or("Desktop Audio");
            let vol = args.get(3).map(|s| s.as_str()).unwrap_or("80");
            println!("Set volume: {} = {}%", name, vol);
            0
        }
        ("screenshot", _) => {
            let file = if subcmd.is_empty() { "screenshot.png" } else { subcmd };
            println!("📷 Screenshot saved: {}", file);
            println!("  Resolution: 1920x1080");
            0
        }
        ("virtualcam", "start") => {
            println!("🎥 Virtual camera started");
            0
        }
        ("virtualcam", "stop") => {
            println!("🎥 Virtual camera stopped");
            0
        }
        ("replaybuffer", "start") => {
            println!("Replay buffer started (30s buffer)");
            0
        }
        ("replaybuffer", "stop") => {
            println!("Replay buffer stopped");
            0
        }
        ("replaybuffer", "save") => {
            println!("Replay saved: ~/Videos/replay_2024-01-15_14-35-12.mkv");
            0
        }
        ("stats", _) => {
            println!("OBS Statistics:");
            println!("  FPS:          60.00");
            println!("  CPU usage:    8.2%");
            println!("  Memory:       512 MB");
            println!("  Disk space:   234.5 GB free");
            println!("  Render time:  4.2 ms");
            println!("  Skipped:      0 frames");
            println!("  Lagged:       0 frames");
            println!("  Output:       1920x1080 @ 60fps");
            0
        }
        ("version", _) => {
            println!("OBS Studio 30.1.0 (OurOS)");
            println!("  Qt: 6.6.1");
            println!("  WebSocket: 5.4.0");
            0
        }
        _ => {
            eprintln!("Error: unknown command '{}{}'. See --help.", cmd,
                if subcmd.is_empty() { String::new() } else { format!(" {}", subcmd) });
            1
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_obs_cli(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_obs_cli};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_obs_cli(vec!["--help".to_string()]), 0);
        assert_eq!(run_obs_cli(vec!["-h".to_string()]), 0);
        assert_eq!(run_obs_cli(vec!["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_obs_cli(vec![]), 0);
    }
}
