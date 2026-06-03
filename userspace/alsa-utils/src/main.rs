#![deny(clippy::all)]

//! alsa-utils — OurOS ALSA sound utilities
//!
//! Multi-personality: `aplay`, `arecord`, `amixer`, `alsamixer`, `alsactl`, `speaker-test`

use std::env;
use std::process;

fn run_aplay(args: Vec<String>, recording: bool) -> i32 {
    let name = if recording { "arecord" } else { "aplay" };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTION]... [FILE]...", name);
        println!();
        println!("Options:");
        println!("  -l, --list-devices     List playback devices");
        println!("  -L, --list-pcms        List PCMs");
        println!("  -D, --device=NAME      Select device");
        println!("  -r, --rate=#           Sample rate");
        println!("  -c, --channels=#       Channels");
        println!("  -f, --format=FORMAT    Sample format");
        println!("  -d, --duration=#       Duration in seconds");
        println!("  -t, --file-type=TYPE   File type (wav/raw/au)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("{} version 1.2.11 (OurOS)", name);
        return 0;
    }
    if args.iter().any(|a| a == "-l" || a == "--list-devices") {
        println!("**** List of {} Hardware Devices ****", if recording { "CAPTURE" } else { "PLAYBACK" });
        println!("card 0: HDA [HDA Intel PCH], device 0: ALC897 Analog [ALC897 Analog]");
        println!("  Subdevices: 1/1");
        println!("  Subdevice #0: subdevice #0");
        println!("card 0: HDA [HDA Intel PCH], device 1: ALC897 Digital [ALC897 Digital]");
        println!("  Subdevices: 1/1");
        println!("  Subdevice #0: subdevice #0");
        return 0;
    }

    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("audio.wav");
    if recording {
        println!("Recording WAVE '{}' : Signed 16 bit LE, Rate 44100 Hz, Stereo", file);
    } else {
        println!("Playing WAVE '{}' : Signed 16 bit LE, Rate 44100 Hz, Stereo", file);
    }
    0
}

fn run_amixer(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: amixer [command] [args...]");
        println!();
        println!("Commands:");
        println!("  scontrols    Show simple mixer controls");
        println!("  scontents    Show simple mixer contents");
        println!("  sset NAME V  Set mixer control");
        println!("  sget NAME    Get mixer control");
        println!("  controls     Show all controls");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("scontents");
    match cmd {
        "scontrols" => {
            println!("Simple mixer control 'Master',0");
            println!("Simple mixer control 'Headphone',0");
            println!("Simple mixer control 'Speaker',0");
            println!("Simple mixer control 'Capture',0");
        }
        "scontents" | "sget" => {
            println!("Simple mixer control 'Master',0");
            println!("  Capabilities: pvolume pswitch");
            println!("  Playback channels: Front Left - Front Right");
            println!("  Limits: Playback 0 - 65536");
            println!("  Front Left: Playback 55705 [85%] [on]");
            println!("  Front Right: Playback 55705 [85%] [on]");
        }
        "sset" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("Master");
            let val = args.get(2).map(|s| s.as_str()).unwrap_or("85%");
            println!("Simple mixer control '{}',0", name);
            println!("  Front Left: Playback {} [on]", val);
            println!("  Front Right: Playback {} [on]", val);
        }
        _ => println!("({} — simulated)", cmd),
    }
    0
}

fn run_alsactl(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alsactl [options] command");
        println!("  store    Store current settings");
        println!("  restore  Restore settings");
        println!("  init     Initialize sound cards");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("store");
    println!("alsactl: {} (simulated)", cmd);
    0
}

fn run_speaker_test(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: speaker-test [options]");
        println!("  -D <dev>    Device name");
        println!("  -r <rate>   Sample rate");
        println!("  -c <ch>     Channels");
        println!("  -t <type>   Test type (sine/wav/pink)");
        println!("  -f <freq>   Sine frequency");
        println!("  -l <loops>  Number of loops (0=infinite)");
        return 0;
    }

    let channels = args.iter().position(|a| a == "-c")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("2");
    println!("speaker-test 1.2.11 (OurOS)");
    println!("Playback device: default");
    println!("Stream parameters: 48000Hz, S16_LE, {} channels", channels);
    println!("Sine wave rate: 440.0000Hz");
    println!("  Front Left");
    println!("  Front Right");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("aplay");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    };
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog_name.as_str() {
        "arecord" => run_aplay(rest, true),
        "amixer" => run_amixer(rest),
        "alsamixer" => { println!("alsamixer: TUI mixer (simulated)"); 0 }
        "alsactl" => run_alsactl(rest),
        "speaker-test" => run_speaker_test(rest),
        _ => run_aplay(rest, false),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_aplay};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_aplay(vec!["--help".to_string()], false), 0);
        assert_eq!(run_aplay(vec!["-h".to_string()], false), 0);
        assert_eq!(run_aplay(vec!["--version".to_string()], false), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_aplay(vec![], false), 0);
    }
}
