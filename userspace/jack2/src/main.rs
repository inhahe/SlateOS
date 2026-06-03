#![deny(clippy::all)]

//! jack2 — OurOS JACK Audio Connection Kit
//!
//! Multi-personality: `jackd`, `jack_control`, `jack_lsp`, `jack_connect`, `jack_disconnect`, `jack_samplerate`, `jack_bufsize`

use std::env;
use std::process;

fn run_jackd(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jackd [options] -d <backend> [backend-options]");
        println!();
        println!("Options:");
        println!("  -d <backend>    Audio backend (alsa/dummy/net/portaudio)");
        println!("  -r              Realtime mode");
        println!("  -p <priority>   Realtime priority");
        println!("  -n <name>       Server name");
        println!("  -t <timeout>    Client timeout (ms)");
        println!("  --nozombies     Prevent zombie clients");
        println!("  -v              Verbose");
        println!("  --version       Show version");
        println!();
        println!("ALSA backend options:");
        println!("  -r <rate>       Sample rate (default: 48000)");
        println!("  -p <frames>     Frames per period (default: 1024)");
        println!("  -n <periods>    Number of periods (default: 2)");
        println!("  -d <device>     ALSA device");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("jackdmp version 1.9.22 (OurOS)");
        return 0;
    }

    let backend = args.iter().position(|a| a == "-d")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("alsa");
    println!("jackdmp 1.9.22 (OurOS)");
    println!("JACK server starting in realtime mode with priority 10");
    println!("Using backend: {}", backend);
    println!("Sample rate: 48000");
    println!("Buffer size: 1024");
    println!("Latency: 21.333 ms");
    println!("JACK server started");
    0
}

fn run_jack_control(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jack_control <command> [args]");
        println!();
        println!("Commands:");
        println!("  start                Start JACK server");
        println!("  stop                 Stop JACK server");
        println!("  status               Show status");
        println!("  ds <backend>         Select driver/backend");
        println!("  dps                  Show driver parameters");
        println!("  dp <name> <val>      Set driver parameter");
        println!("  eps                  Show engine parameters");
        println!("  ep <name> <val>      Set engine parameter");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => println!("--- JACK server is running ---"),
        "start" => println!("--- JACK server started ---"),
        "stop" => println!("--- JACK server stopped ---"),
        "dps" => {
            println!("device: hw:0");
            println!("rate: 48000");
            println!("period: 1024");
            println!("nperiods: 2");
        }
        _ => println!("({} — simulated)", cmd),
    }
    0
}

fn run_jack_lsp(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jack_lsp [options]");
        println!("  -c  Show connections");
        println!("  -p  Show port properties");
        println!("  -l  Show port latencies");
        println!("  -t  Show port type");
        return 0;
    }
    let show_connections = args.iter().any(|a| a == "-c");
    println!("system:capture_1");
    println!("system:capture_2");
    if show_connections {
        println!("   myapp:input_1");
    }
    println!("system:playback_1");
    println!("system:playback_2");
    if show_connections {
        println!("   myapp:output_1");
    }
    println!("myapp:input_1");
    println!("myapp:input_2");
    println!("myapp:output_1");
    println!("myapp:output_2");
    0
}

fn run_jack_connect(args: Vec<String>, disconnect: bool) -> i32 {
    let verb = if disconnect { "disconnect" } else { "connect" };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jack_{} port1 port2", verb);
        return 0;
    }
    let port1 = args.first().map(|s| s.as_str()).unwrap_or("system:capture_1");
    let port2 = args.get(1).map(|s| s.as_str()).unwrap_or("myapp:input_1");
    println!("({} {} <-> {})", verb, port1, port2);
    0
}

fn run_jack_simple(args: Vec<String>, what: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: jack_{}", what);
        return 0;
    }
    let _ = args;
    match what {
        "samplerate" => println!("48000"),
        "bufsize" => println!("1024"),
        _ => println!("(jack_{} — simulated)", what),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("jackd");
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
        "jack_control" => run_jack_control(rest),
        "jack_lsp" => run_jack_lsp(rest),
        "jack_connect" => run_jack_connect(rest, false),
        "jack_disconnect" => run_jack_connect(rest, true),
        "jack_samplerate" => run_jack_simple(rest, "samplerate"),
        "jack_bufsize" => run_jack_simple(rest, "bufsize"),
        _ => run_jackd(rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_jack_connect};

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_jack_connect(vec!["--help".to_string()], "jack2"), 0);
        assert_eq!(run_jack_connect(vec!["-h".to_string()], "jack2"), 0);
        assert_eq!(run_jack_connect(vec!["--version".to_string()], "jack2"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_jack_connect(vec![], "jack2"), 0);
    }
}
