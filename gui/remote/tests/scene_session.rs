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

// An integration test is all test code, so the allows that a `#[cfg(test)]`
// module carries inside a source file belong at the top of the file here. A
// test that indexes a window it has just asserted exists, or unwraps a decode
// whose success is the thing under test, is saying "this must hold" in the
// shortest way there is; making it recover instead would turn a failure into a
// silently skipped assertion.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeMap;

use guiremote::scene::{
    ImageSnapshot, PatchMark, SceneImage, SceneSession, SceneViewer, WindowSnapshot,
    apply_scene_frame, decode_scene_frame, encode_scene_frame,
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
fn snapshots<'a>(
    order: &[u64],
    windows: &'a BTreeMap<u64, ServerWindow>,
) -> Vec<WindowSnapshot<'a>> {
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
                images: Vec::new(),
                video: None,
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
        let (decoded, used) = decode_scene_frame(&bytes).expect("viewer decodes the frame");
        assert_eq!(
            used,
            bytes.len(),
            "the frame must account for all its bytes"
        );
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
        let (decoded, _) = decode_scene_frame(&bytes).expect("decode");
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
    let (decoded, _) = decode_scene_frame(&bytes).expect("decode");
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

// ---------------------------------------------------------------------------
// Pictures
// ---------------------------------------------------------------------------

/// One picture as the server holds it: the pixels, a revision changed by every
/// upload and patch, and the patch log a viewer a few patches behind is brought
/// up to date from -- what the compositor keeps per `ImageAsset`.
struct ServerImage {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    revision: u64,
    patch_base: u64,
    patches: Vec<PatchMark>,
}

/// Every window's pictures, and the revision counter they share.
#[derive(Default)]
struct Pictures {
    next_revision: u64,
    windows: BTreeMap<u64, BTreeMap<u64, ServerImage>>,
}

/// The longest patch log a picture keeps here -- short, so that a viewer
/// several patches behind is sent the picture whole.
const LOG: usize = 4;

impl Pictures {
    fn revision(&mut self) -> u64 {
        self.next_revision += 1;
        self.next_revision
    }

    fn upload(&mut self, window: u64, id: u64, width: u32, height: u32, seed: u32) {
        let revision = self.revision();
        let pixels = (0..width * height)
            .map(|i| 0xFF00_0000 | ((i.wrapping_mul(2_654_435_761) ^ seed) & 0x00FF_FFFF))
            .collect();
        self.windows.entry(window).or_default().insert(
            id,
            ServerImage {
                width,
                height,
                pixels,
                revision,
                patch_base: revision,
                patches: Vec::new(),
            },
        );
    }

    fn patch(&mut self, window: u64, id: u64, rect: (u32, u32, u32, u32), colour: u32) {
        let revision = self.revision();
        let Some(image) = self.windows.get_mut(&window).and_then(|w| w.get_mut(&id)) else {
            return;
        };
        let (x, y, w, h) = rect;
        let (w, h) = (w.min(image.width - x), h.min(image.height - y));
        for row in y..y + h {
            for col in x..x + w {
                image.pixels[(row * image.width + col) as usize] = colour;
            }
        }
        image.revision = revision;
        image.patches.push(PatchMark {
            revision,
            x,
            y,
            width: w,
            height: h,
        });
        if image.patches.len() > LOG {
            let dropped = image.patches.remove(0);
            image.patch_base = dropped.revision;
        }
    }

    fn drop(&mut self, window: u64, id: u64) {
        if let Some(images) = self.windows.get_mut(&window) {
            images.remove(&id);
        }
    }

    fn snapshots(&self, window: u64) -> Vec<ImageSnapshot<'_>> {
        self.windows
            .get(&window)
            .map(|images| {
                images
                    .iter()
                    .map(|(&id, image)| ImageSnapshot {
                        id,
                        revision: image.revision,
                        width: image.width,
                        height: image.height,
                        pixels: &image.pixels,
                        patch_base: image.patch_base,
                        patches: &image.patches,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A frame for `session` over these windows and pictures, through the wire.
fn frame_over_the_wire(
    session: &mut SceneSession,
    windows: &BTreeMap<u64, ServerWindow>,
    pictures: &Pictures,
) -> guiremote::scene::SceneFrame {
    let snaps: Vec<WindowSnapshot<'_>> = windows
        .iter()
        .map(|(&id, w)| WindowSnapshot {
            id,
            x: w.x,
            y: w.y,
            width: w.width,
            height: w.height,
            opacity: w.opacity,
            commands: &w.tree,
            images: pictures.snapshots(id),
            video: None,
        })
        .collect();
    let frame = session.build_frame(640, 480, &snaps);
    let bytes = encode_scene_frame(&frame);
    let (decoded, used) = decode_scene_frame(&bytes).expect("the viewer decodes the frame");
    assert_eq!(used, bytes.len());
    decoded
}

fn assert_pictures_match(viewer: &SceneViewer, pictures: &Pictures, step: usize) {
    for (window, images) in &pictures.windows {
        let held = &viewer.windows[window].images;
        assert_eq!(
            held.keys().collect::<Vec<_>>(),
            images.keys().collect::<Vec<_>>(),
            "step {step}: window {window} holds other pictures than the server"
        );
        for (id, image) in images {
            assert_eq!(
                (held[id].width, held[id].height),
                (image.width, image.height),
                "step {step}"
            );
            assert!(
                held[id].pixels == image.pixels,
                "step {step}: picture {id} of window {window} differs"
            );
        }
    }
}

/// Uploads, patches (overlapping, several between frames, more than the log
/// keeps) and drops across two windows, at random: after every frame the
/// viewer holds exactly the server's pixels. A second viewer joining halfway
/// holds them after its first frame. And a small patch travels as a patch.
#[test]
fn pictures_reach_the_viewer_through_uploads_patches_and_drops() {
    let windows: BTreeMap<u64, ServerWindow> = [1u64, 2]
        .iter()
        .map(|&id| {
            (
                id,
                ServerWindow {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 48,
                    opacity: 1.0,
                    tree: rect_tree(Color::rgba(1, 2, 3, 255), 64.0, 48.0),
                },
            )
        })
        .collect();
    let mut pictures = Pictures::default();
    let mut session = SceneSession::new();
    let mut viewer = SceneViewer::new();
    let mut late: Option<(SceneSession, SceneViewer)> = None;
    let mut state: u32 = 0x2545_F491;
    let mut rand = move |n: u32| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state % n
    };
    let mut patches_seen = 0;

    for step in 0..400 {
        for _ in 0..=rand(6) {
            let window = u64::from(rand(2)) + 1;
            let id = u64::from(rand(3));
            match rand(10) {
                0 => pictures.upload(window, id, 8 + rand(40), 6 + rand(30), rand(1 << 24)),
                1 => pictures.drop(window, id),
                _ => {
                    let held = pictures.windows.get(&window).and_then(|w| w.get(&id));
                    if let Some((w, h)) = held.map(|i| (i.width, i.height)) {
                        let rect = (rand(w), rand(h), 1 + rand(6), 1 + rand(5));
                        pictures.patch(window, id, rect, 0xFF00_0000 | rand(1 << 24));
                    }
                }
            }
        }
        let frame = frame_over_the_wire(&mut session, &windows, &pictures);
        patches_seen += frame
            .windows
            .iter()
            .flat_map(|w| &w.images)
            .filter(|c| matches!(c, SceneImage::Patch { .. }))
            .count();
        viewer
            .apply(&frame)
            .expect("the viewer applies every frame");
        assert_pictures_match(&viewer, &pictures, step);

        if step == 200 {
            late = Some((SceneSession::new(), SceneViewer::new()));
        }
        if let Some((late_session, late_viewer)) = late.as_mut() {
            let frame = frame_over_the_wire(late_session, &windows, &pictures);
            late_viewer
                .apply(&frame)
                .expect("the late viewer applies it");
            assert_pictures_match(late_viewer, &pictures, step);
        }
    }
    assert!(
        patches_seen > 100,
        "only {patches_seen} patches went as patches; the log is not being used"
    );

    // One small patch, alone: exactly its rectangle crosses the wire.
    pictures.upload(1, 9, 50, 40, 7);
    let _ = frame_over_the_wire(&mut session, &windows, &pictures);
    pictures.patch(1, 9, (10, 12, 3, 2), 0xFF12_3456);
    let frame = frame_over_the_wire(&mut session, &windows, &pictures);
    let changes: Vec<&SceneImage> = frame.windows.iter().flat_map(|w| &w.images).collect();
    assert_eq!(
        changes,
        [&SceneImage::Patch {
            id: 9,
            x: 10,
            y: 12,
            width: 3,
            height: 2,
            pixels: vec![0xFF12_3456; 6],
        }]
    );
}

/// A patch to a picture the viewer does not hold is refused, and the viewer
/// is left exactly as it was: the stream and the viewer disagree, which a
/// reset of the session resolves.
#[test]
fn a_frame_the_viewer_cannot_apply_changes_nothing() {
    let windows: BTreeMap<u64, ServerWindow> = [(
        3u64,
        ServerWindow {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            opacity: 1.0,
            tree: rect_tree(Color::rgba(9, 9, 9, 255), 10.0, 10.0),
        },
    )]
    .into_iter()
    .collect();
    let mut pictures = Pictures::default();
    pictures.upload(3, 1, 4, 4, 1);
    let mut session = SceneSession::new();
    let mut viewer = SceneViewer::new();
    viewer
        .apply(&frame_over_the_wire(&mut session, &windows, &pictures))
        .unwrap();

    // A viewer that missed the upload: it is sent only the patch.
    let mut stale = SceneViewer::new();
    pictures.patch(3, 1, (0, 0, 1, 1), 0xFFFF_FFFF);
    let frame = frame_over_the_wire(&mut session, &windows, &pictures);
    let before = format!("{stale:?}");
    assert!(stale.apply(&frame).is_err());
    assert_eq!(
        format!("{stale:?}"),
        before,
        "a refused frame changed the viewer"
    );
    // The one that holds the picture applies it; a reset brings the stale one in.
    viewer.apply(&frame).unwrap();
    session.reset();
    stale
        .apply(&frame_over_the_wire(&mut session, &windows, &pictures))
        .unwrap();
    assert_pictures_match(&stale, &pictures, 0);
}
