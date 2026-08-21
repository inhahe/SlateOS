//! The compositor binary: start a display server and run it until killed.
//!
//! Everything is in the library beside this file. The split is deliberate and
//! recent: while the compositor was a binary only, *nothing outside its own
//! source file could link it*, so the only tests that could exist against the
//! real compositor were the ones inside it. An application could be tested
//! against a stand-in compositor, and the compositor against a stand-in client,
//! and the two halves could disagree without any test noticing — which is
//! precisely the failure mode `known-issues.md` →
//! `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` was opened about.
//!
//! With a library, `apps/*` can start a real compositor on an ephemeral port,
//! connect to it the way the shipped binary would, and assert on what arrives.
//!
//! ## Where the frame goes
//!
//! On Windows this opens a host window and draws into it, so a person can see
//! the desktop the compositor composites and type at it — see
//! [`compositor::present::host`]. That is a **development harness**, not the
//! target: the target is a SlateOS display driver, which is
//! `known-issues.md` → `TD-COMPOSITOR-HAS-NO-SCANOUT` and will be another
//! `impl Present` with nothing else here changing. Everywhere else, and under
//! `--headless`, the composited frame is produced and dropped, which is right
//! for a display server whose clients are all remote.

use compositor::{Compositor, Server};

/// The default size of a window a person is expected to look at.
///
/// Smaller than [`HEADLESS_SIZE`] for the mundane reason that a 1920x1080
/// *client* area does not fit on a 1920x1080 screen once a title bar and a
/// taskbar have taken their share, and a harness whose bottom edge is off the
/// display is a harness that hides exactly the thing it exists to show.
const WINDOWED_SIZE: (u32, u32) = (1280, 800);

/// The default size when nothing will be looked at.
const HEADLESS_SIZE: (u32, u32) = (1920, 1080);

/// The refresh rate to composite at.
const REFRESH_HZ: u32 = 60;

/// What the command line asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Options {
    /// The address to listen on, if one was given.
    addr: Option<String>,
    /// Whether to run with no display at all.
    headless: bool,
    /// The framebuffer size, if one was given.
    size: Option<(u32, u32)>,
    /// Whether to print usage and stop.
    help: bool,
}

/// What to print for `--help`, and on a usage error.
const USAGE: &str = "\
usage: compositor [ADDRESS] [--headless] [--size WxH]

  ADDRESS       host:port to listen on. Defaults to $SLATE_DISPLAY, then to
                127.0.0.1:7373.
  --headless    composite without opening a window. The default off Windows,
                and correct for a display server whose clients are all remote.
  --size WxH    framebuffer size in pixels. Defaults to 1280x800 windowed,
                1920x1080 headless.
  --            stop reading options; the next argument is the address.";

/// Parse a `WxH` size.
///
/// Rejects a zero dimension rather than passing it on: a framebuffer with no
/// pixels composites nothing, and the error it eventually produces names an
/// allocation rather than the argument that asked for it.
fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (w, h) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got `{text}`"))?;
    let width: u32 = w
        .parse()
        .map_err(|_| format!("`{w}` is not a width in pixels"))?;
    let height: u32 = h
        .parse()
        .map_err(|_| format!("`{h}` is not a height in pixels"))?;
    if width == 0 || height == 0 {
        return Err(format!("a {width}x{height} display has no pixels"));
    }
    Ok((width, height))
}

/// Read the command line. `args` excludes the program name.
///
/// An unrecognised `--option` is an error rather than being taken as an
/// address, because `--headles` should not silently become a hostname that
/// then fails to resolve — the second error would be about DNS and would send
/// whoever read it looking in the wrong place entirely.
fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
    let mut out = Options::default();
    let mut args = args.into_iter();
    let mut options_over = false;
    while let Some(arg) = args.next() {
        if options_over {
            out.addr = Some(arg);
            continue;
        }
        match arg.as_str() {
            "--" => options_over = true,
            "-h" | "--help" => out.help = true,
            "--headless" => out.headless = true,
            "--size" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--size needs a size, as in `--size 1280x800`".to_owned())?;
                out.size = Some(parse_size(&value)?);
            }
            other if other.starts_with("--size=") => {
                out.size = Some(parse_size(other.trim_start_matches("--size="))?);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option `{other}`"));
            }
            _ => out.addr = Some(arg),
        }
    }
    Ok(out)
}

/// Start the display server and serve clients until it is killed, or until the
/// host window is closed.
///
/// The address comes from the first argument, or from `SLATE_DISPLAY`, or from
/// `guiremote::socket::DEFAULT_DISPLAY` — in that order, so a second display
/// can be started on one machine without touching the environment of the first.
///
/// What this used to be is worth recording: it created one window, drew a blue
/// rectangle into it, composited once, and then looped forever composing frames
/// nobody could connect to, with a comment saying a real loop would poll for
/// IPC. Every part of the compositor beneath it was real and none of it was
/// reachable. That demo is not deleted: it is now a test
/// (`the_demo_scene_still_composites`), because it is a compact tour of the
/// API, and being a test means it is actually run.
fn main() {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("compositor: {e}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if options.help {
        println!("{USAGE}");
        return;
    }

    let addr = match options.addr {
        Some(explicit) => explicit,
        None => match guiremote::socket::display_addr() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("compositor: {e}");
                std::process::exit(2);
            }
        },
    };

    let (width, height) = options.size.unwrap_or(if options.headless {
        HEADLESS_SIZE
    } else {
        WINDOWED_SIZE
    });

    let mut compositor = match Compositor::new(width, height, REFRESH_HZ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("compositor: failed to initialize: {e}");
            std::process::exit(1);
        }
    };

    // The user's window-corner and drop-shadow choices. Read here rather than
    // in `Compositor::new` so that the library has no opinion about `$HOME` and
    // its tests do not depend on the machine running them. A missing or
    // unreadable file yields the defaults, which is the ordinary state of a
    // fresh install rather than something to warn about.
    compositor.set_appearance(appearance::AppearanceFile::load().settings);

    let mut server = match Server::bind(&addr) {
        Ok(s) => s,
        Err(e) => {
            // Overwhelmingly this is "address already in use", which means a
            // compositor is already serving this display. Said plainly, because
            // the raw errno reads like a fault in this program.
            eprintln!("compositor: cannot listen on {addr}: {e}");
            eprintln!("  If a compositor is already running on that address, either stop it or");
            eprintln!("  start this one elsewhere: `compositor 127.0.0.1:7374`.");
            std::process::exit(1);
        }
    };

    eprintln!("compositor: initialized ({width}x{height} @ {REFRESH_HZ}Hz)");
    match server.local_addr() {
        Ok(bound) => eprintln!("compositor: listening on {bound}"),
        Err(e) => eprintln!("compositor: listening, but cannot name the address: {e}"),
    }

    let outcome = run(
        &mut server,
        &mut compositor,
        options.headless,
        width,
        height,
    );
    if let Err(e) = outcome {
        eprintln!("compositor: the listening socket failed: {e}");
        std::process::exit(1);
    }
}

/// Serve, onto a host window where there can be one.
///
/// Split out of `main` so that the `#[cfg]` is in one small place rather than
/// wrapped around the whole of startup: the argument parsing, the bind and the
/// diagnostics are identical on every platform and should not be compiled
/// twice.
#[cfg(windows)]
fn run(
    server: &mut Server,
    compositor: &mut Compositor,
    headless: bool,
    width: u32,
    height: u32,
) -> std::io::Result<()> {
    if headless {
        return server.run(compositor);
    }
    match compositor::present::host::Window::new("SlateOS", width, height) {
        Ok(mut window) => {
            eprintln!("compositor: showing the desktop in a host window; close it to stop");
            server.run_with(compositor, &mut window)
        }
        Err(e) => {
            // A missing window station — a service, or a session with no
            // desktop. Serving remote clients is still entirely useful, so
            // this is a warning and not an exit.
            eprintln!("compositor: no host window ({e}); compositing headless");
            server.run(compositor)
        }
    }
}

/// Serve. There is no window to open on this platform.
#[cfg(not(windows))]
fn run(
    server: &mut Server,
    compositor: &mut Compositor,
    headless: bool,
    _width: u32,
    _height: u32,
) -> std::io::Result<()> {
    if !headless {
        eprintln!("compositor: this build has no way to open a window; compositing headless");
    }
    server.run(compositor)
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it — that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects
    )]

    use super::{Options, parse_args, parse_size};

    fn parse(args: &[&str]) -> Result<Options, String> {
        parse_args(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn an_empty_command_line_asks_for_nothing_in_particular() {
        assert_eq!(parse(&[]).unwrap(), Options::default());
    }

    #[test]
    fn a_bare_argument_is_the_address() {
        assert_eq!(
            parse(&["10.0.0.1:9000"]).unwrap().addr.as_deref(),
            Some("10.0.0.1:9000")
        );
    }

    #[test]
    fn a_size_can_be_written_either_way_round() {
        assert_eq!(
            parse(&["--size", "800x600"]).unwrap().size,
            Some((800, 600))
        );
        assert_eq!(parse(&["--size=800x600"]).unwrap().size, Some((800, 600)));
    }

    #[test]
    fn a_display_with_no_pixels_is_refused_at_the_argument_rather_than_the_allocation() {
        // The bug this prevents: `--size 1280x0` builds a compositor whose
        // failure is reported as a framebuffer error, which reads like a fault
        // in this program rather than in what was typed.
        assert!(parse(&["--size", "1280x0"]).is_err());
        assert!(parse(&["--size", "0x800"]).is_err());
    }

    #[test]
    fn a_size_that_is_not_a_size_is_an_error_naming_what_was_wrong() {
        assert!(parse_size("1280").unwrap_err().contains("WIDTHxHEIGHT"));
        assert!(parse_size("widexhigh").unwrap_err().contains("width"));
        assert!(parse_size("1280xhigh").unwrap_err().contains("height"));
    }

    #[test]
    fn a_size_with_no_value_after_it_says_so() {
        assert!(parse(&["--size"]).unwrap_err().contains("--size"));
    }

    #[test]
    fn a_misspelt_option_is_refused_rather_than_taken_for_an_address() {
        // `--headles` must not become a hostname: the resulting error would be
        // about DNS and would send the reader looking in the wrong place.
        let err = parse(&["--headles"]).unwrap_err();
        assert!(err.contains("--headles"), "{err}");
    }

    #[test]
    fn a_double_dash_ends_the_options_so_an_odd_address_can_still_be_given() {
        let parsed = parse(&["--", "--weird-host:80"]).unwrap();
        assert_eq!(parsed.addr.as_deref(), Some("--weird-host:80"));
        assert!(!parsed.headless);
    }

    #[test]
    fn headless_and_help_are_recognised() {
        assert!(parse(&["--headless"]).unwrap().headless);
        assert!(parse(&["--help"]).unwrap().help);
        assert!(parse(&["-h"]).unwrap().help);
    }

    #[test]
    fn the_last_address_wins_so_a_wrapper_script_can_append_one() {
        let parsed = parse(&["127.0.0.1:1", "127.0.0.1:2"]).unwrap();
        assert_eq!(parsed.addr.as_deref(), Some("127.0.0.1:2"));
    }
}
