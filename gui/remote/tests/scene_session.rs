//! End-to-end integration test for the `guiremote` scene streaming protocol,
//! exercising the public API the way a real remote-desktop pairing does:
//!
//! * a **server** (the compositor) holds authoritative per-window state, drives
//!   a [`SceneSession`] to build delta frames, and `encode_scene_frame`s them;
//! * a simulated byte transport carries the wire frames;
//! * a **viewer** `decode_scene_frame`s them and `apply_scene_frame`s the deltas
//!   onto its running reconstruction.
//!
//! After every frame we assert the viewer's reconstructed scene is byte-for-byte
//! (well, `Debug`-for-`Debug`, since `RenderCommand` has no `PartialEq`) equal to
//! the server's authoritative window set — across new windows, unchanged windows
//! (delta suppression), content changes, geometry-only moves, window removal, and
//! a viewer reconnect (`reset` → full resend).

use std::collections::BTreeMap;

use guiremote::scene::{
    SceneSession, WindowSnapshot, apply_scene_frame, decode_scene_frame, encode_scene_frame,
};
use guitk::color::Color;
use guitk::render::{RenderCommand, RenderTree};
use guitk::style::CornerRadii;

/// One window's authoritative state on the server side.
#[derive(Clone)]
struct ServerWindow {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    opacity: f32,
    tree: RenderTree,
}

/// A solid-rect render tree, used as easily-distinguishable window content.
fn rect_tree(color: Color, w: f32, h: f32) -> RenderTree {
    RenderTree {
        commands: vec![RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            color,
            corner_radii: CornerRadii::ZERO,
        }],
    }
}

/// `RenderCommand` has no `PartialEq` (it carries `f32`s), so compare via `Debug`.
fn trees_eq(a: &RenderTree, b: &RenderTree) -> bool {
    a.commands.len() == b.commands.len()
        && a.commands
            .iter()
            .zip(&b.commands)
            .all(|(ca, cb)| format!("{ca:?}") == format!("{cb:?}"))
}

/// Build z-ordered snapshots that borrow from the server's window map.
fn snapshots<'a>(order: &[u64], windows: &'a BTreeMap<u64, ServerWindow>) -> Vec<WindowSnapshot<'a>> {
    order
        .iter()
        .filter_map(|id| {
            windows.get(id).map(|w| WindowSnapshot {
                id: *id,
                x: w.x,
                y: w.y,
                width: w.width,
                height: w.height,
                opacity: w.opacity,
                commands: &w.tree,
            })
        })
        .collect()
}

/// Assert the viewer's reconstructed command set matches the server's
/// authoritative content for exactly the windows in `order`.
fn assert_viewer_matches(
    order: &[u64],
    server: &BTreeMap<u64, ServerWindow>,
    viewer: &BTreeMap<u64, RenderTree>,
) {
    assert_eq!(
        viewer.len(),
        order.len(),
        "viewer should track exactly the present windows"
    );
    for id in order {
        let want = &server.get(id).expect("present window").tree;
        let got = viewer.get(id).expect("viewer has the window");
        assert!(
            trees_eq(want, got),
            "window {id} content diverged between server and viewer"
        );
    }
}

#[test]
fn remote_session_reconstructs_full_scene_across_deltas() {
    let display = (1920u32, 1080u32);

    // Server authoritative state.
    let mut server: BTreeMap<u64, ServerWindow> = BTreeMap::new();
    let mut session = SceneSession::new();

    // Viewer reconstruction (window id -> commands).
    let mut viewer: BTreeMap<u64, RenderTree> = BTreeMap::new();

    // The transport: encode on the server, decode on the viewer.
    let roundtrip = |session: &mut SceneSession,
                         order: &[u64],
                         server: &BTreeMap<u64, ServerWindow>,
                         viewer: &mut BTreeMap<u64, RenderTree>|
     -> usize {
        let snaps = snapshots(order, server);
        let frame = session.build_frame(display.0, display.1, &snaps);
        let bytes = encode_scene_frame(&frame);
        let decoded = decode_scene_frame(&bytes).expect("viewer decodes the frame");
        // The decoded frame must describe the same window set + geometry.
        assert_eq!(decoded.windows.len(), order.len());
        for (slot, id) in order.iter().enumerate() {
            let w = &server[id];
            let dw = &decoded.windows[slot];
            assert_eq!(dw.id, *id);
            assert_eq!(dw.x, w.x);
            assert_eq!(dw.y, w.y);
            assert_eq!(dw.width, w.width);
            assert_eq!(dw.height, w.height);
            assert!((dw.opacity - w.opacity).abs() < f32::EPSILON);
        }
        *viewer = apply_scene_frame(viewer, &decoded);
        bytes.len()
    };

    // --- Frame 0: two brand-new windows; both carry full commands. ---
    server.insert(
        1,
        ServerWindow {
            x: 0,
            y: 0,
            width: 400,
            height: 300,
            opacity: 1.0,
            tree: rect_tree(Color::rgba(200, 30, 30, 255), 400.0, 300.0),
        },
    );
    server.insert(
        2,
        ServerWindow {
            x: 500,
            y: 100,
            width: 200,
            height: 150,
            opacity: 0.9,
            tree: rect_tree(Color::rgba(30, 200, 30, 255), 200.0, 150.0),
        },
    );
    let full_bytes = roundtrip(&mut session, &[1, 2], &server, &mut viewer);
    assert_viewer_matches(&[1, 2], &server, &viewer);

    // --- Frame 1: nothing changed; both windows stream geometry-only. ---
    let delta_bytes = roundtrip(&mut session, &[1, 2], &server, &mut viewer);
    assert_viewer_matches(&[1, 2], &server, &viewer);
    assert!(
        delta_bytes < full_bytes,
        "an unchanged frame ({delta_bytes} B) must be smaller than the full frame ({full_bytes} B)"
    );

    // --- Frame 2: window 1 content changes; window 2 only moves. ---
    server.get_mut(&1).unwrap().tree = rect_tree(Color::rgba(30, 30, 200, 255), 400.0, 300.0);
    server.get_mut(&2).unwrap().x = 520; // geometry-only move
    roundtrip(&mut session, &[1, 2], &server, &mut viewer);
    assert_viewer_matches(&[1, 2], &server, &viewer);

    // --- Frame 3: window 2 closes; only window 1 remains. ---
    server.remove(&2);
    {
        let snaps = snapshots(&[1], &server);
        let frame = session.build_frame(display.0, display.1, &snaps);
        assert_eq!(frame.removed, vec![2], "the closed window must be reported");
        let bytes = encode_scene_frame(&frame);
        let decoded = decode_scene_frame(&bytes).expect("decode");
        assert_eq!(decoded.removed, vec![2]);
        viewer = apply_scene_frame(&viewer, &decoded);
    }
    assert_viewer_matches(&[1], &server, &viewer);

    // --- Frame 4: a new window 3 appears above window 1. ---
    server.insert(
        3,
        ServerWindow {
            x: 50,
            y: 400,
            width: 320,
            height: 240,
            opacity: 1.0,
            tree: rect_tree(Color::rgba(200, 200, 30, 255), 320.0, 240.0),
        },
    );
    roundtrip(&mut session, &[1, 3], &server, &mut viewer);
    assert_viewer_matches(&[1, 3], &server, &viewer);

    // --- Viewer reconnect: reset forces a full resend even for unchanged windows.
    session.reset();
    // A freshly-connected viewer has no prior state to carry forward.
    let mut fresh_viewer: BTreeMap<u64, RenderTree> = BTreeMap::new();
    let snaps = snapshots(&[1, 3], &server);
    let frame = session.build_frame(display.0, display.1, &snaps);
    assert!(
        frame.windows.iter().all(|w| w.commands.is_some()),
        "after reset every present window must resend full commands"
    );
    let bytes = encode_scene_frame(&frame);
    let decoded = decode_scene_frame(&bytes).expect("decode");
    fresh_viewer = apply_scene_frame(&fresh_viewer, &decoded);
    assert_viewer_matches(&[1, 3], &server, &fresh_viewer);
}

#[test]
fn corrupt_transport_is_rejected_not_panicked() {
    // A frame that is truncated or bit-flipped in transit must surface a decode
    // error rather than panic or silently produce a bogus scene.
    let mut session = SceneSession::new();
    let server: BTreeMap<u64, ServerWindow> = BTreeMap::from([(
        1,
        ServerWindow {
            x: 10,
            y: 10,
            width: 100,
            height: 100,
            opacity: 1.0,
            tree: rect_tree(Color::rgba(1, 2, 3, 255), 100.0, 100.0),
        },
    )]);
    let snaps = snapshots(&[1], &server);
    let frame = session.build_frame(800, 600, &snaps);
    let bytes = encode_scene_frame(&frame);

    // Truncation at every length must be rejected (never a panic).
    for len in 0..bytes.len() {
        assert!(
            decode_scene_frame(&bytes[..len]).is_err(),
            "truncated-to-{len} frame must be rejected"
        );
    }

    // A flipped magic byte is rejected.
    let mut bad = bytes.clone();
    bad[0] ^= 0xFF;
    assert!(decode_scene_frame(&bad).is_err());

    // The intact frame still decodes.
    assert!(decode_scene_frame(&bytes).is_ok());
}
