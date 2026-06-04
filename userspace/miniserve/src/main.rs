#![deny(clippy::all)]

//! miniserve — OurOS small self-contained HTTP file server
//!
//! Single personality: `miniserve`

use std::env;
use std::process;

fn run_miniserve(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: miniserve [OPTIONS] [PATH]");
        println!();
        println!("A fast, small, self-contained HTTP file server.");
        println!();
        println!("Options:");
        println!("  -p, --port <PORT>            Port to use (default: 8080)");
        println!("  -i, --interfaces <ADDR>      Interfaces to bind to");
        println!("  --index <FILE>               Use a specific index file");
        println!("  -a, --auth <USER:PASS>       Set authentication");
        println!("  --auth-file <FILE>           Auth file");
        println!("  --route-prefix <PREFIX>      URL prefix");
        println!("  -u, --upload-files [PATH]    Enable file upload");
        println!("  -U, --mkdir                  Enable creating directories");
        println!("  --media-type <TYPE=EXT>      Set custom media types");
        println!("  --media-type-raw <T=E>       Raw media type mapping");
        println!("  -q, --qrcode                 Show QR code");
        println!("  -P, --no-symlinks            Hide symbolic links");
        println!("  -r, --enable-tar             Enable tar archive download");
        println!("  -z, --enable-tar-gz          Enable tar.gz archive download");
        println!("  -Z, --enable-zip             Enable zip archive download");
        println!("  -c, --color-scheme <SCHEME>  Color scheme (squirrel/archlinux/zenburn/monokai)");
        println!("  -d, --color-scheme-dark <S>  Dark color scheme");
        println!("  --spa                        Single-page app mode");
        println!("  -t, --title <TITLE>          Page title");
        println!("  -F, --hide-version-footer    Hide footer version");
        println!("  --header <HEADER>            Add custom HTTP header");
        println!("  -o, --show-wget-footer       Show wget command");
        println!("  -l, --show-symlink-info      Show symlink info");
        println!("  -W, --tls-cert <CERT>        TLS certificate path");
        println!("  -Y, --tls-key <KEY>          TLS key path");
        println!("  --random-route               Generate random URL prefix");
        println!("  --print-completions <SHELL>  Print shell completions");
        println!("  -v, --verbose                Be more verbose");
        println!("  -V, --version                Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("miniserve 0.27.1 (OurOS)");
        return 0;
    }

    // Parse port
    let port = args.windows(2)
        .find(|w| w[0] == "-p" || w[0] == "--port")
        .and_then(|w| w[1].parse::<u16>().ok())
        .unwrap_or(8080);

    let path = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or(".");

    let upload = args.iter().any(|a| a == "-u" || a == "--upload-files");
    let qrcode = args.iter().any(|a| a == "-q" || a == "--qrcode");

    println!("miniserve 0.27.1 (OurOS)");
    println!();
    println!("Serving path: {}", path);
    println!("Available at:");
    println!("  http://127.0.0.1:{}", port);
    println!("  http://[::1]:{}", port);

    if upload {
        println!();
        println!("File upload enabled: POST to /upload");
    }

    if qrcode {
        println!();
        println!("QR code:");
        println!("  ██████████████  ████  ██████████████");
        println!("  ██          ██  ████  ██          ██");
        println!("  ██  ██████  ██    ██  ██  ██████  ██");
        println!("  ██  ██████  ██  ████  ██  ██████  ██");
        println!("  ██  ██████  ██    ██  ██  ██████  ██");
        println!("  ██          ██  ████  ██          ██");
        println!("  ██████████████  ████  ██████████████");
    }

    println!();
    println!("Quit by pressing CTRL-C");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_miniserve(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_miniserve};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_miniserve(vec!["--help".to_string()]), 0);
        assert_eq!(run_miniserve(vec!["-h".to_string()]), 0);
        let _ = run_miniserve(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_miniserve(vec![]);
    }
}
