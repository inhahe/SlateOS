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

use compositor::{Compositor, Server};

/// Start the display server and serve clients until it is killed.
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
    let addr = match std::env::args().nth(1) {
        Some(explicit) => explicit,
        None => match guiremote::socket::display_addr() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("compositor: {e}");
                std::process::exit(2);
            }
        },
    };

    let mut compositor = match Compositor::new(1920, 1080, 60) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("compositor: failed to initialize: {e}");
            std::process::exit(1);
        }
    };

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

    eprintln!("compositor: initialized (1920x1080 @ 60Hz)");
    match server.local_addr() {
        Ok(bound) => eprintln!("compositor: listening on {bound}"),
        Err(e) => eprintln!("compositor: listening, but cannot name the address: {e}"),
    }

    if let Err(e) = server.run(&mut compositor) {
        eprintln!("compositor: the listening socket failed: {e}");
        std::process::exit(1);
    }
}
